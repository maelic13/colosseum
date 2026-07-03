# Colosseum — Build Plan (living source of truth)

> Colosseum is a cross-platform (Windows/Linux/macOS) desktop app for **chess engine testing**:
> running UCI engine-vs-engine tournaments with a live, modern results table.
> This document is the canonical plan. `GUIDE.md` tracks progress with checkboxes.

## 1. Product summary

- **Engine management**: add one engine, add all executables in a folder, auto-detect & edit UCI
  options, store metadata (name/version/elo, extensible). One window/tab.
- **Tournament** (main tab): pick engines (same library), Round-Robin, flexible time control
  (time-per-move first, down to ~10 ms/move), common forwarded engine options
  (Threads/Hash/SyzygyPath/Syzygy50MoveRule/**Ponder=off**), concurrency, configurable
  adjudication, PGN output, **Go / Stop / Force-Stop**, Elo-update policy, and a **live results
  table** (name/version/elo/**Elo Δ**/points/games/W-D-L/**head-to-head matrix**/nps) sortable by
  clicking points/elo/name headers.
- **Modern, intuitive, uncluttered, well-arranged** UI is a first-class requirement.

## 2. Locked decisions

| Topic | Decision | Why |
|---|---|---|
| Language/UI | **Rust + egui/eframe** | Single static binary, no npm/runtime; `shakmaty` rules+PGN; `tokio` async I/O; immediate-mode redraw ideal for a live table. |
| Backend | **Custom** UCI driver + scheduler | Needed for true live updates, exact Stop/Go/Force-Stop/resume, Elo timing, extensibility. |
| Openings (v1) | **Single start position** until Step 10 adds EPD/PGN opening books | `StartPosition` seam present from the start. |
| Time control (v1) | **Time-per-move**, extensible `TimeControl` + value+unit (ms/s/min) UI | Must hit 10 ms/move; ready to grow to base+increment/nodes/depth. |
| Elo | **Incremental K-factor** behind a `Rating` trait | Matches per-game/end/never; modular for future Ordo recompute; exposes per-game Δ. |
| License | **GPL-3.0-or-later** (whole project) | Open source, not for sale; frees GPL deps like `shakmaty-syzygy`. |
| Storage | TOML/JSON config + engine library; **SQLite** (`rusqlite bundled`) for tournaments/games | Live queries, crash-resume, history. |
| Distribution | `cargo-dist` + GH Actions; **Linux=Flatpak+tarball**, Win=MSI+zip, macOS=DMG | Self-contained native binaries. |
| Testing | Real **Stockfish copied to temp** (no mock engine); skip if absent | Realistic timing/protocol behavior. |

## 3. Naming (easy rename)

- Identifier from `Cargo.toml`: code reads `env!("CARGO_PKG_NAME")` / `env!("CARGO_PKG_VERSION")`.
- Single `DISPLAY_NAME` const = `"Colosseum"` (pretty name ≠ crate name).
- Single `ProjectDirs` decision: **`ProjectDirs::from("", "", "Colosseum")`** →
  Windows `%APPDATA%\Colosseum`, Linux `~/.config/colosseum`, macOS `~/Library/Application Support/Colosseum`.
- Rename = change Cargo package name + `DISPLAY_NAME` + the ProjectDirs app string (one place each).
  Changing the app string moves the data dir → ship a one-time migration if renaming post-release.

## 4. Workspace layout

```
colosseum/                      (workspace root, git repo)
├─ Cargo.toml                   (workspace; resolver = "2")
├─ crates/
│  ├─ colosseum-core/           lib — pure domain types & logic (no I/O, no UI)
│  ├─ colosseum-uci/            lib — UCI protocol & process management (tokio)
│  ├─ colosseum-engine/         lib — orchestration: scheduler, persistence, events
│  └─ colosseum-gui/            bin — eframe/egui app (shipped binary)
└─ .github/workflows/ci.yml     matrix build/test (windows, ubuntu, macos)
```

(No `mock-engine` crate — tests use a temp-copied real Stockfish.)

## 5. Architecture notes

**Concurrency / data flow.** Backend runs a `tokio` runtime on its own thread(s). Each game is a
task spawning two `tokio::process::Command` children, line-framed over stdin/stdout, kept alive
for the whole game. A `Semaphore` enforces concurrency. The GUI never blocks on engine I/O: the
backend pushes `TournamentEvent`s over a channel; each egui frame drains it, updates in-memory
state, requests a repaint. Repaints + DB writes are **throttled/batched** (~30 Hz). **Child
cleanup guaranteed** on game end and app exit (process groups / Windows job objects, kill-on-drop).

**Controls / state machine.**
- **Go**: start; or resume if stopped; or top-up running games to the concurrency limit.
- **Stop** (graceful): no new games; in-flight finish and count as real results.
- **Force-Stop** (abort): kill all running engines, **discard** in-flight games (no results),
  stop. Resumable from completed games only.

**Time control.** `go movetime <ms>`; pre-warm (`isready`/`ucinewgame`) before timing; prompt
unbuffered reads; wall-clock measurement; configurable timeout tolerance.

**Adjudication (configurable, persisted with the tournament).** Natural results always on
(mate/stalemate/50-move/threefold/insufficient via `shakmaty`); optional draw adjudication
(min plies + N consecutive moves under a cp threshold), optional resign adjudication (cp
threshold + N moves), optional max-move cap. Each toggleable. Crash/hang → loss-on-time/errored,
surfaced in UI + logged.

## 6. Implementation reference — types (illustrative, finalize in Steps 2–3)

```rust
// ---------- Engine library ----------
pub struct EngineMeta {
    pub name: String,                         // defaults to UCI `id name`
    pub version: String,                      // user-editable
    pub elo: Option<i32>,                      // configured baseline
    pub extra: std::collections::BTreeMap<String, String>, // future: logo, author...
}

pub struct EngineConfig {
    pub id: EngineId,                          // stable uuid
    pub meta: EngineMeta,
    pub path: std::path::PathBuf,
    pub args: Vec<String>,
    pub working_dir: Option<std::path::PathBuf>,
    pub env: std::collections::BTreeMap<String, String>,
    pub options: std::collections::BTreeMap<String, UciOptionValue>, // user-set
    pub detected_options: Vec<UciOption>,      // schema from handshake
}

pub enum UciOption {
    Check  { name: String, default: bool },
    Spin   { name: String, default: i64, min: i64, max: i64 },
    Combo  { name: String, default: String, vars: Vec<String> },
    Button { name: String },
    Str    { name: String, default: String },
}
pub enum UciOptionValue { Check(bool), Spin(i64), Combo(String), Str(String) }

// ---------- Time control (extensible) ----------
pub enum TimeControl {
    PerMove { ms: u64 },
    // future: Increment { base_ms, inc_ms }, Game { .. }, Nodes(u64), Depth(u32)
}

// ---------- Tournament ----------
pub enum Format { RoundRobin { cycles: u32 } }   // more formats later

pub struct CommonEngineOptions {
    pub threads: Option<u32>,                  // default 1
    pub hash_mb: Option<u32>,
    pub syzygy_path: Option<String>,
    pub syzygy_50_move_rule: Option<bool>,
    pub ponder: bool,                          // default false
}

pub struct DrawAdjudication   { pub min_ply: u32, pub move_count: u32, pub score_cp: i32 }
pub struct ResignAdjudication { pub move_count: u32, pub score_cp: i32 }
pub struct AdjudicationConfig {
    pub max_moves: Option<u32>,
    pub draw: Option<DrawAdjudication>,        // None = disabled
    pub resign: Option<ResignAdjudication>,    // None = disabled
}

pub enum EloPolicy { PerGame, EndOfTournament, Never }

pub enum StartPosition { Startpos, /* OpeningBook(BookRef) added in Step 10 */ }

pub struct TournamentConfig {
    pub format: Format,
    pub games_per_pair: u32,                   // e.g. 2 (both colors) × cycles
    pub time_control: TimeControl,
    pub concurrency: usize,
    pub common: CommonEngineOptions,
    pub adjudication: AdjudicationConfig,
    pub elo_policy: EloPolicy,
    pub k_factor: f64,                         // for IncrementalElo
    pub start_position: StartPosition,
    pub pgn_output: Option<std::path::PathBuf>,
}

// ---------- Rating ----------
pub struct RatingDelta { pub engine: EngineId, pub delta: f64 }
pub trait Rating {
    fn update(&mut self, white: EngineId, black: EngineId, result: GameResult) -> Vec<RatingDelta>;
    fn current(&self, e: EngineId) -> f64;
    fn delta_since_start(&self, e: EngineId) -> f64; // for the Elo-Δ column
}
pub struct IncrementalElo { /* k, baseline, current maps */ }

// ---------- Game / events ----------
pub enum GameResult { WhiteWin, BlackWin, Draw }
pub enum Termination {
    Checkmate, Stalemate, FiftyMove, Threefold, InsufficientMaterial,
    AdjudicatedDraw, AdjudicatedResign, MaxMoves,
    TimeForfeit, EngineCrash, IllegalMove, Aborted,
}
pub struct GameStats { pub plies: u32, pub white_nps: Option<u64>, pub black_nps: Option<u64> }

pub enum TournamentEvent {
    GameStarted    { game_id: GameId, white: EngineId, black: EngineId, round: u32 },
    MoveMade       { game_id: GameId, ply: u32, nps: Option<u64> },
    GameFinished   { game_id: GameId, result: GameResult, termination: Termination, stats: GameStats },
    StandingsUpdated,
    EngineError    { engine: EngineId, message: String },
    TournamentFinished,
}
```

## 7. SQLite schema (designed for History + resume)

```sql
CREATE TABLE engines (
  id          TEXT PRIMARY KEY,
  name        TEXT NOT NULL,
  version     TEXT,
  elo         INTEGER,
  path        TEXT NOT NULL,
  config_json TEXT NOT NULL          -- full EngineConfig snapshot
);

CREATE TABLE tournaments (
  id          TEXT PRIMARY KEY,
  name        TEXT,
  status      TEXT NOT NULL,         -- 'running' | 'stopped' | 'finished'
  config_json TEXT NOT NULL,         -- TournamentConfig snapshot (reproducibility)
  pgn_path    TEXT,
  created_at  TEXT NOT NULL,
  finished_at TEXT
);

CREATE TABLE tournament_engines (
  tournament_id TEXT NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
  engine_id     TEXT NOT NULL,
  seed          INTEGER,
  start_elo     REAL,                -- baseline for Elo-Δ since tournament start
  PRIMARY KEY (tournament_id, engine_id)
);

CREATE TABLE games (
  id            TEXT PRIMARY KEY,
  tournament_id TEXT NOT NULL REFERENCES tournaments(id) ON DELETE CASCADE,
  round         INTEGER NOT NULL,
  white_id      TEXT NOT NULL,
  black_id      TEXT NOT NULL,
  result        TEXT,                -- '1-0' | '0-1' | '1/2-1/2' | NULL while pending
  termination   TEXT,
  white_nps     INTEGER,
  black_nps     INTEGER,
  plies         INTEGER,
  pgn           TEXT,
  started_at    TEXT,
  finished_at   TEXT,
  status        TEXT NOT NULL        -- 'pending' | 'running' | 'finished' | 'discarded'
);
CREATE INDEX idx_games_tournament ON games(tournament_id);
```

**Resume:** on load, `finished` games count toward standings; `running`/`pending` are
re-scheduled; `discarded` (from Force-Stop) are ignored. Schema also backs a future History tab
(`list/load/delete/resume`) and a game/board viewer (PGN retained per game).

## 8. Storage locations

- Config dir (via `ProjectDirs`): `config.toml` (prefs, window state, last-used paths),
  `engines.json` (engine library; typed meta + open `extra` map).
- Data dir: `colosseum.sqlite` (tournaments/games), rotating `tracing` log, optional per-game I/O logs.
- PGN: appended on each game finish to the user-chosen path (stored on the tournament record).
- `--portable`: keep all of the above next to the executable.

## 9. Testing strategy

- Real engine at `D:\chess\engines\stockfish.exe`, **copied to a `tempfile` dir** first; tests
  skip gracefully if absent. Distinct "engines" = Stockfish with different options
  (Threads/Hash/Skill Level/UCI_LimitStrength).
- Unit (`core`): Elo math, pairings, adjudication, UCI `option`/`info` parsing (canned strings).
- Integration (`engine`): full RR, Stop→drain→Go→resume, Force-Stop→discard, crash (kill child).
- Responsiveness probe: `movetime=10ms`, concurrency > cores; table updates each game, UI stays live.

## 10. Steps & model labels

> **Small** = Sonnet 4.6 (medium) / GPT-5.5 (medium). **Large** = Opus 4.8 (high thinking) /
> GPT-5.5 (extra high). **Each step = its own commit.** After each step, report what was done +
> next step + its model, then pause.

| # | Step | Model |
|---|---|---|
| 0 | Initialize repository (git, .gitignore, GPL LICENSE) | Small |
| 1 | PLAN.md + GUIDE.md | Small |
| 2 | Architecture scaffold (workspace, crates, deps, seams, CI stub) | **Large** |
| 3 | `colosseum-core` domain & logic (types, Elo, pairings, adjudication) + unit tests | **Large** |
| 4 | `colosseum-uci` protocol + temp-Stockfish tests | **Large** |
| 5 | `colosseum-engine` orchestration (scheduler, state machine, PGN, persistence) + integration tests | **Large** |
| 6 | Persistence & resume wiring; `--portable` | Small (medium for resume) |
| 7 | GUI scaffold, modern theme, backend bridge, close-confirm | **Large** (bridge) / Small (theme) |
| 8 | Engine Management tab | Small |
| 9 | Tournament tab (centerpiece): options, Go/Stop/Force-Stop, live sortable table | **Large** |
| 10 | Starting positions / openings (EPD + PGN, UI) — final feature | **Large** |
| 11 | Cross-platform packaging & release (cargo-dist, Flatpak/MSI/DMG) | Small |
| 12 | README, CHANGELOG, docs polish — **v0.1.0** | Small |

### v0.2+ — post-v1 enhancements (detail in §11)

> These continue the "one step = one commit, report + pause" cadence. Ordered by
> value/effort; earlier steps unblock later ones (e.g. per-game timing in 15 feeds 18).

| # | Step | Model |
|---|---|---|
| 13 | Engine identity: detect `author` into `meta.extra`, parse version from `id name`, show Author field | Small |
| 14 | Apply/sync resulting Elo from a tournament back to the engine library | Small |
| 15 | Per-game timing → live **avg game time / elapsed / throughput / ETA**; set `games.started_at` | Small (medium for core plumbing) |
| 16 | Wire **Resume** into the GUI (backend `resume_tournament` already exists) | Small |
| 17 | **Tournament History** tab (`list`/`load`/`delete`/`resume`) ✅ | **Large** |
| 18 | Live **"currently playing"** panel (consume `GameStarted`/in-flight set) ✅ | Small |
| 19 | **Termination breakdown** (mate/timeout/crash/adjudication) summary in live view ✅ | Small |
| 20 | Engines-tab usability: broken-path indicator, clone engine, search/filter+sort, per-option reset, open-folder, Button-option handling ✅ | Small |
| 21 | Time controls: `Increment`/sudden-death/`Nodes`/`Depth` (extend `TimeControl` + UI) ✅ | **Large** |
| 22 | Tournament formats: gauntlet ✅ + honest Format control (knockout/SPRT deferred — need a dynamic scheduler) | **Large** |
| 23 | Config presets (save/load tournament settings) + remember last-used config | Small |
| 24 | Output & analysis: CSV standings/crosstable export, export-PGN-now, SPRT/LOS/Elo error bars, PGN/board viewer | **Large** |
| 25 | Cleanup: remove the unused SQLite `engines` table + dead `Store` engine methods | Small |

## 11. Post-v1 enhancement backlog (v0.2+)

Detail for steps 13–25 above. Grounded in the v0.1.0 code; file references are the
primary touch points. Tiered by priority.

### Tier 1 — correctness fixes + the explicitly-requested features

- ✅ **13 · Engine identity (author + version).** Detection currently stores the UCI
  `id author` into `meta.version` (`engines_tab.rs::add_engine_from_detect` and the
  re-detect merge in `poll_redetect`), and never parses a real version. Fix: put
  `author` in `meta.extra["author"]`, parse the trailing version token out of
  `id name` (e.g. `Stockfish 16.1` → name `Stockfish`, version `16.1`), and add an
  **Author** row to the Identity card in the edit panel. `EngineMeta.extra` already
  exists for exactly this (see §6 / `engine.rs`).
- **14 · Apply Elo to library.** ✅ The scheduler computes Elo only into the live
  snapshot (`EloEntry.current`); nothing wrote it back to `meta.elo` in
  `engines.json`. Implemented as an explicit **"Apply Elo → Library"** button in the
  live control bar (`Backend::apply_active_elo_to_library` rounds each participant's
  current Elo into `backend.engines` and calls `save_engines()`). Manual-only by
  design — automatic write-back on finish is intentionally not done to avoid
  surprising library mutation; revisit if a per-`EloPolicy` auto-apply is wanted.
- ✅ **15 · Live timing.** Added `duration_ms: Option<u64>` to `GameStats`; `run_game`
  records wall-clock time via `Instant` (including setup, shared across the three early-return
  paths). The scheduler accumulates `total_game_ms`/`games_timed` + captures `started_at`
  (set when first game launches) into `TournamentSnapshot`. The live control bar now shows
  **⏱ elapsed**, **avg N/game**, and **ETA N** (once ≥1 game finishes). Fixed
  `mark_game_running` to write `started_at = now` in the DB.

### Tier 2 — high-impact gaps found in the v0.1.0 GUI

- ✅ **16 · Resume wiring.** Added `Backend::find_resumable()` (queries the DB for the
  most recent non-finished tournament) and `Backend::try_resume(row)` (calls the existing
  `resume_tournament` scheduler function, builds `ParticipantInfo` from stored engine-config
  snapshots, sets `backend.active`). The setup view now shows a compact accent-colored banner
  when a resumable tournament exists; "↩ Resume" loads it in `Stopped` state (user presses
  Go to continue); "×" dismisses for the session. Removed the `#[expect(dead_code)]` on
  `Backend.store` since it is now actively queried.
- ✅ **17 · Tournament History tab.** New `History` top-level tab. Added a read-only
  `load_tournament_results(store, row)` to the engine crate (`scheduler.rs`) that
  replays finished games to rebuild `Standings` + Elo (`TournamentResults`/
  `ResultParticipant`), matching the live end-state for all Elo policies. `Backend`
  gained `list_tournaments()`, `tournament_results(row)`, and `delete_tournament(id)`.
  The tab (`history_tab.rs`) shows a selectable list (name/status/date) on the left and
  a detail pane on the right: config summary, decisive/drawn counts, a final-standings
  table (rank/Elo/Δ/points/W-D-L/nps), **↩ Resume** (when unfinished and nothing busy —
  reuses `Backend::try_resume`, then switches to the Tournament tab via `HistoryAction`),
  a two-step **Delete** confirm, and a copy-PGN-path button. List is cached and refreshed
  on open / after actions / via Refresh, never per-frame.
- **18 · "Currently playing" panel.** The scheduler emits `GameStarted` with the
  pairing/round, but the live view only renders finished standings. Show in-flight
  games (which engines, which round) to make a running tournament legible.
- **19 · Termination breakdown.** Win-by-mate vs. timeout vs. crash vs. adjudication
  is stored per game (`games.termination`) but never surfaced; add a compact summary
  (or column) so flaky engines are easy to spot.

### Tier 3 — Engines-tab usability (bundled into step 20)

- Broken/missing **path indicator** when `EngineConfig.path` no longer exists.
- **Clone/duplicate** an engine config (same binary, different Hash/Threads/options).
- **Search/filter + sort** the engine list (currently insertion-ordered).
- Per-option **"Reset to default" / "Reset all"** (overrides only accumulate today),
  and real handling for UCI **Button** options (currently a no-op placeholder).
- **Open containing folder** for an engine path.

### Tier 4 — Tournament setup (steps 21–23)

- ✅ **21 · Time controls** beyond per-move. `TimeControl` now carries
  `SuddenDeath{base_ms}`, `Increment{base_ms,inc_ms}`, `Nodes{nodes}`, and
  `Depth{depth}` alongside `PerMove`; `GoLimits` grew matching `Clock`/`Nodes`/`Depth`
  variants. The runner runs per-side game clocks for the clock-based controls
  (deduct elapsed, credit increment, flag via the search deadline) and applies a fixed
  safety deadline to node/depth searches. The setup UI has a time-control **Type**
  selector with per-kind inputs and live resolved hints. New unit tests cover the
  clock math and `go` command rendering.
- ✅ **22 · Formats (gauntlet) + honest Format control.** Added
  `Format::Gauntlet{seeds,cycles}` and a `gauntlet()` pairing generator (each of the
  first `seeds` engines plays every non-seed, color-balanced, per cycle);
  `generate_schedule` dispatches it. The setup "Format" row is now a real combo —
  **Round Robin** and **Gauntlet** work; **Knockout** and **SPRT** are shown as
  *disabled* "planned" options with a tooltip explaining they need a result-dependent
  (dynamic) scheduler, which the current static-schedule architecture doesn't yet
  provide. Game-count estimate and history config summaries are format-aware.
  **Deferred:** Knockout (result-dependent bracket) and SPRT (sequential stopping
  rule) — both require the scheduler to extend/stop the schedule mid-tournament based
  on results, plus persistence/resume support; SPRT additionally needs the LLR math
  and a live readout (overlaps Step 24).
- ✅ **23 · Presets.** Named tournament configs are saved as JSON files in
  `<config_dir>/presets/`; the "Presets ▾" menu in the setup action bar lists them
  with load and delete per entry, plus a name field to save the current form.
  The last-used config is auto-saved to `last_used_config.json` whenever a
  tournament starts, and loaded back on next launch so the form reopens with the
  previous settings. All preset I/O is in a new `presets.rs` module
  (`PresetManager` + `PresetData`); the form's `to_preset`/`apply_preset` helpers
  handle the round-trip without touching engine selection or the openings preview cache.

### UI hardening (cross-cutting, between steps 23 and 24)

A systemic pass on UI robustness in `colosseum-gui`, so visibility/stability
problems don't recur per-tab:

- **Control visibility (theme-level).** `inactive.bg_stroke` was `Stroke::NONE`,
  so resting buttons and unchecked checkboxes had no border and shared the
  `BG_ELEVATED` fill of the cards they sit on — effectively invisible until
  hover. Added a dedicated `BORDER_INTERACTIVE` color and gave the `inactive`
  widget state a 1 px border (`theme.rs`). One change fixes buttons, checkboxes,
  combo boxes, and text fields everywhere.
- **Layout stability.** Every `ScrollArea` used the default
  `auto_shrink([true, true])`, which together with `set_min_width(available_width)`
  oscillates as the scrollbar toggles. Panel-filling scroll areas now use
  `auto_shrink([false, false])` (height-capped nested ones `[false, true]`) and
  the redundant width-setting was removed.
- **Live-table jitter.** The results + history tables used `Column::auto()`,
  which re-measures cell content every frame, so live-updating numbers made the
  columns visibly jump. Numeric columns are now fixed-width (`Column::exact`).

### Tier 5 — output & analysis (step 24, split into sub-commits)

- ✅ **24a · Exports.** New pure `colosseum-core::export` module builds RFC-4180
  CSV for the standings table and the head-to-head crosstable (unit-tested).
  A shared `export_ui` GUI helper opens a native save dialog (`rfd`). Both the
  live Tournament control bar and the History detail pane gained an
  **Export ▾** menu: Standings (CSV), Crosstable (CSV), and Game PGN (the
  per-game PGN stored in the DB, concatenated via `Backend::collect_pgn`) —
  replacing the previous append-only PGN path.
- ✅ **24b · Statistics.** New pure `colosseum-core::stats` module: Elo with a
  95% confidence interval (`elo_with_error`), likelihood-of-superiority (`los`,
  via a normal-CDF/`erf` approximation), and a trinomial SPRT (`sprt`, draw rate
  held fixed between hypotheses — the cutechess-cli approach) returning the LLR,
  decision bounds, and an accept-H0/H1/continue verdict. All unit-tested. The
  GUI `stats_ui::match_stats_card` surfaces Elo±, LOS, and SPRT (H0 +0 vs H1 +5
  Elo, α=β=0.05) and is shown above the results in both the live Tournament view
  and the History detail pane whenever a tournament has exactly two engines.
- ✅ **24c · PGN/board viewer.** New `colosseum-gui::viewer` module: parses a
  stored game's PGN movetext (dropping headers, `{}` comments, `()` variations,
  NAGs, move numbers, result token), replays the SAN with `shakmaty` to
  reconstruct the board after every ply, and renders a floating window with a
  painted board (last-move highlight), a clickable move list, and ⏮◀▶⏭ +
  arrow/Home/End navigation. The History detail pane gained a **Games (N)**
  list (cached via `Backend::list_games`) with a **View** button per game.
  Shakmaty added as a GUI dependency; SAN extraction + replay are unit-tested.
  The live Tournament view has the same viewer behind a **Games** toggle in the
  control bar (the game list is cached and refreshed only when a game finishes).

### Non-feature cleanup (step 25)

- ✅ Removed the dead SQLite `engines` table from `SCHEMA` and the unused
  `Store::{upsert_engine,list_engines,delete_engine}` methods — the GUI uses the
  JSON engine library (`engines.json`) exclusively. The store round-trip test
  was trimmed to `round_trips_tournaments_and_games`. Existing databases keep an
  orphan `engines` table (harmless, never written by the app); no migration is
  needed since the GUI never populated it.

### Engines-tab visual refresh (step 26)

- ✅ Reworked the Engine Management tab toward the clean, scannable look of
  GUIs like en-croissant, while staying inside the Colosseum design tokens
  (`docs/design/GUIDELINES.md`). The identity shown comes straight from the UCI
  handshake — `id name` (split into name + version by `split_name_version`) and
  `id author` — captured on add and on **Re-detect**.
  - **Monogram avatars.** Each engine gets a circular initial badge tinted by a
    stable hue (FNV-1a over the display name → `theme::AVATAR_PALETTE`), giving
    logo-less local engines the at-a-glance identity a logo pipeline would. A
    30 px badge in list rows, 40 px in the edit header.
  - **List rows.** Row 1 is now the bold engine name plus a muted **version
    chip**; row 2 shows the **author** (from `id author`) in place of the former
    executable file name, falling back to the file name only when no author was
    detected. Elo (when set) renders as a right-aligned chip. Broken-path ⚠ and
    the multi-select checkbox are unchanged.
  - **Edit header.** Avatar + name + gold version chip + a `by {author}`
    subtitle (`Unknown author` when absent), replacing the bare name line.
  - New shared `widgets::chip` (tinted static text chip) and the
    `AVATAR_PALETTE` token; the old `engine_display_name*` helpers were replaced
    by `engine_base_name` / `engine_subtitle` / avatar helpers. No behavior
    change to detection, persistence, or the UCI-options editor.

### Live-view fix + cross-tab polish + responsiveness (step 27)

- ✅ **Board live view fixed.** The viewer itself was fine (works from History);
  the live **Games** toggle lived in a single non-wrapping control-bar row that
  overflowed and pushed the toggle off the right edge on anything but a very wide
  window — so it was unreachable. `live_control_bar` is now two
  `horizontal_wrapped` rows (transport/progress/timing, then view-toggles +
  actions), so every control stays reachable at any width. The live **Games**
  list now shows only finished (PGN-bearing) games, newest first, with an
  always-enabled **View** (it previously listed every pending game as a disabled
  button).
- ✅ **Shared engine-row widgets.** `engine_base_name` / `engine_subtitle` /
  `engine_initial` / `avatar_color` / `engine_avatar` moved into `widgets.rs`;
  the **tournament-setup engine list** now uses the same monogram-avatar +
  version-chip + author-subtitle row (and an Elo chip) as the Engines tab.
- ✅ **Tab + cross-tab polish.** `pill_tab` gained a `BG_HOVER` hover fill and a
  pointing-hand cursor on idle tabs. The Engines toolbar error and the History
  detail action bar now truncate / wrap instead of overflowing. Two pre-existing
  collapsible-`if` clippy nits (`tournament_tab`, `viewer`) fixed.
- ✅ **Resize / fullscreen.** With the wrapped bars plus the existing
  `with_min_inner_size([860, 560])` clamp, no control bar can clip; maximize /
  fullscreen simply gain space (panels + scroll areas already size to available
  width). *(Note: a live visual pass was blocked by a multi-monitor
  screenshot-capture quirk in the dev session; verified via build/clippy/tests
  and the earlier Engines-tab visual confirmation of the shared row widgets.)*

### Engines tab restructure: grid + logos + global tablebases (step 28)

- ✅ **Two-pane layout.** The Engines tab is now an en-croissant-style **card
  grid** on the left (a fixed ~50% split via `Panel::exact_size`, so it tracks
  the window instead of persisting an absolute width) and a split right column:
  the selected engine's panel on top, a **global Endgame Tablebases** panel
  pinned to the bottom (~25% height). The grid's column count is responsive
  (1–4 by available width).
- ✅ **Engine logos.** New `logo` module: a chosen image is copied into the GUI
  `logos/` data dir under a unique `<id>-<millis>.<ext>` name (survives deletion
  of the original), its name stored in `meta.extra["logo"]`. Decoded via the
  `image` crate, cached by path, and drawn **aspect-fitted** (never cropped or
  squished) into a fixed box; falls back to the monogram avatar. Click the
  header logo to pick one; "Remove" clears it. Clone copies the logo file;
  delete removes it. `AppDirs::logos_dir()` added.
- ✅ **Engine panel.** Header = editable Name/Version/Author/Elo + logo box;
  a collapsible "Launch & environment" section; then the UCI options in a
  **scrollable** area; action row (Save/Clone/Delete) wraps so it never clips.
- ✅ **Option filtering.** `is_globally_managed_option` hides Threads / `Max CPUs`
  / Hash and tablebase **path** options from the per-engine editor — threads and
  hash are set in the Tournament tab, paths in the global panel. The header shows
  "N managed elsewhere".
- ✅ **Global tablebase paths** (`syzygy_path` / `gaviota_path` / `nalimov_path`
  on `AppConfig`, persisted in `config.toml`). `apply_global_tablebases` injects
  them into each engine that declares a matching `*Path` option at tournament
  start; the Tournament setup's old per-tournament SyzygyPath field was removed
  (replaced by a pointer to the Engines tab).

### Engines tab polish (step 29)

- ✅ **Aligned grid.** Cards are drawn at an exact fixed size
  (`allocate_exact_size` + a child `Ui` painted inside), so every card in a row
  lines up regardless of content; the grid is sorted alphabetically by
  name/version for display.
- ✅ **Autosave.** The Save Changes button is gone: the edit buffer commits
  automatically 600 ms after the last change (debounced, so typing doesn't spam
  writes), plus an immediate flush on selection change, clone, tab switch, and
  window close (`EnginesTab::flush_edit`, called from the app shell). A brief
  "✓ saved" flash in the header replaces the old "● unsaved" marker.
- ✅ **Card context menu.** Right-click selects the card and offers Clone,
  Re-detect, Open containing folder, Copy path, and Delete Engine (confirmed
  via an `egui::Modal`, shared with the panel's Delete button — the old inline
  two-step confirm is gone). Clone logic is factored into `clone_engine`.
- ✅ **Pinned actions.** The Clone / Delete row is a bottom `Panel` inside the
  engine panel, so it can't be clipped away at small window sizes; the detail
  column got proper left padding (16 pt) off the splitter.
- ✅ **Sharp logos.** `LogoCache` now keeps the decoded image and uploads a
  Lanczos3-downscaled texture per distinct physical-pixel display size, with
  the draw rect snapped to the pixel grid — no more aliased minification of
  1000+ px logos into 34 pt slots. Identity fields cap at half the panel width
  and the header logo box grows up to 128 pt in the freed space.
- ✅ **Better identity detection.** `split_name_version` now treats the *last*
  digit-bearing token as the version start and keeps suffix words with it
  ("Deep HIARCS 14 WCSC" → version "14 WCSC"; "Rybka 2.3.2a mp" → "2.3.2a mp"),
  and numbered SIMD spellings (SSE42, SSE4.2, AVX-512…) are stripped as
  architecture noise ("Deep Rybka 4.1 SSE42" → version "4.1"). Re-detect now
  *overwrites* name/version/author (it's an explicit refresh), so stale wrong
  values can be fixed from the context menu. UCI button options read
  "run at game start" instead of the cryptic "arm".

### Engines tab en-croissant card redesign (step 30)

- ✅ **Spacious cards** (spec added as GUIDELINES §3.9). The library grid now
  uses 98-pt-tall cards targeting ~360 pt width (1–3 responsive columns,
  12-pt gaps): logo/avatar 36 + name 15 strong with the author subtitle under
  it on top, and two labeled stat columns pinned to the bottom — `ELO` left,
  `VERSION` right-aligned — with faint 11-pt uppercase labels over 13.5-pt
  strong values (`—` when unknown). Version/Elo chips inside cards are gone;
  the identity/UCI grids in the detail panel got the guideline `[12, 8]`
  spacing. Verified live via screenshots against en-croissant's Engines page.
- ✅ **Repaint-while-detecting fix** (found during live verification): results
  from background add/scan/re-detect jobs arrive on a channel that is only
  polled during a frame, so with no input the GUI never repainted and a
  finished detection appeared "stuck". `EnginesTab::show` now requests a
  repaint every 150 ms while a detection job is in flight.
- GUIDELINES §4.5 updated to describe the current tab (grid cards, context
  menu, autosave, pinned Clone/Delete) and the old inline "Save = primary"
  action row was removed from the spec.

### Engines tab polish round 2 (step 31)

- ✅ **Solid scrollbars** — `spacing.scroll = ScrollStyle::solid()` in the theme:
  scrollbars now reserve their own lane instead of floating over cards/options.
- ✅ **Rounded logo rendering** — `logo::draw_fitted` takes a corner radius and
  draws a textured `RectShape`, so square images on opaque backgrounds no
  longer poke square corners out from under rounded frames (cards: 6, header: 8).
- ✅ **Header logo centered** in the space right of the identity fields (was
  pinned to the panel edge); the Remove button is placed via `ui.put` on a
  fixed rect so it doesn't shift on hover.
- ✅ **Filter field** — compact (280×28) with an inline ✕ clear button that
  appears when text is entered (standard search-field affordance).
- ✅ **Multi-column UCI options** — 1–3 columns depending on panel width
  (~420 pt per column, min ~5 rows per column) instead of one skinny column.
- ✅ **Tablebases as a collapsible bottom bar**, collapsed by default: a
  one-line clickable header with a per-format ✓/— summary that expands to the
  Browse rows. Rationale: a set-once global setting shouldn't permanently hold
  ~25 % of the right column; collapsing also removes the small-window clipping.
  (If more global settings accumulate later, consider a Settings dialog.)
- ✅ **Small-window fix** — the tab body is now an `egui::CentralPanel` (was a
  bare `Frame`), so content clips to the space between header and status bar;
  previously tall content painted over the status bar at small sizes. The
  pinned action row also gained proper bottom padding.

### Embedded fonts + Engines tab polish round 3 (step 32)

- ✅ **Embedded fonts** (GUIDELINES §2.2, cross-platform consistency):
  `assets/fonts/` now ships Inter-Regular (proportional), Inter-SemiBold
  (registered as the `"semibold"` family — egui has one weight per family, so
  `.strong()` only brightens color; `theme::semibold(size)` gives real bold),
  and JetBrainsMono-Regular (monospace), each with its license file. egui's
  default fonts stay as fallbacks so symbols/emoji still resolve.
- ✅ **Tofu eliminated.** Glyphs missing from the default fonts (`✕ ✓ ▶ ▼`)
  rendered as squares. Replaced with: `×` on the filter-clear button, a
  painter-drawn `widgets::disclosure_triangle`, `●`/`○` for the tablebase
  set/unset summary, a plain italic "saved" flash, and "● runs at game start"
  for armed UCI buttons. Rule of thumb recorded: only use glyphs verified in
  egui's built-in fallbacks (`● ○ × ⚠ ♟ 🔍 ⏳ …`) or paint the shape.
- ✅ **Hover expansion set to 0** in the theme — expanding widgets on hover
  clipped their outline at panel edges (filter, Clone/Delete, Delete All) and
  made layout shift; panels also got 2 pt edge margins for focus rings.
- ✅ **Whole detail column scrolls** (header included) so the smallest window
  can reach every field; tablebases summary hides below 240 pt header width
  (it used to overlap the title).
- ✅ **Monogram avatars are rounded squares** in the Engines tab (same
  silhouette as uploaded logos; circles remain elsewhere), and the header
  logo box now **adopts the image's aspect ratio** (wide logos like Rybka4's
  banner use up to 220×128 pt instead of shrinking into a square);
  `draw_fitted` may upscale the *draw* rect to fill the slot (texture itself
  is never upscaled).
- ✅ **Persisted grid sort** — `AppConfig::engines_sort` ("name" default,
  "elo" = highest first / unrated last, "author" = alphabetical / unknown
  last), a `Sort:` combo next to Delete All, saved on change.

### Micro-polish + UCI-defaults audit (step 33)

- ✅ **Sort popup** gets `.height(140.0)` — the default popup height was a hair
  short of three entries, producing a phantom scrollbar that blinked on hover.
- ✅ **Context menus** breathe: `spacing.menu_margin` 10 (theme-wide) and the
  card menu sets item spacing 4 / button padding (10, 6).
- ✅ **`interaction.selectable_labels = false`** theme-wide: labels are UI
  chrome, not documents — clicking a card no longer selects its text. Text
  fields are unaffected.
- ✅ **UCI defaults audit** — compared `engines.json` stored `detected_options`
  against a live `uci` dump of Deep Shredder 12: byte-for-byte identical.
  Shredder's surprising values (Ponder default *true*, OwnBook *true*,
  UCI_Elo 1400 gated behind UCI_LimitStrength=false) are what the engine
  actually declares; the parser (`parse_option_line`) is faithful, including
  multi-word defaults, `<empty>` strings, and negative spin ranges.

### Tablebase probe caches + startup polish (step 34)

- ✅ **Global probe-cache sizes.** `AppConfig` gains `nalimov_cache_mb` /
  `gaviota_cache_mb` (default **32**). The common UCI option names — verified
  against the user's engines — are `NalimovCache` (Shredder, Rybka) and
  `GaviotaTbCache` (Critter); Syzygy has no cache option (memory-mapped).
  Edited inline in the Endgame Tablebases bar (`Cache [n] MB` on the
  Gaviota/Nalimov rows); `apply_global_tablebases` forwards them (clamped to
  the option's declared min/max) to engines that declare a matching
  `*cache*` option, but only when the corresponding path is set.
  `is_globally_managed_option` now hides tb-cache options from the
  per-engine editor.
- ✅ **Sort selector → `MenuButton` popup.** `ComboBox` popups wrap contents
  in a `ScrollArea` with menu styling that flashed a needless scrollbar for
  three entries (a `.height()` bump did not fix it); menu popups don't.
  Recorded in GUIDELINES §7.4: menu popup for short static lists.
- ✅ **No startup blink.** The viewport is created `with_visible(false)` and
  the app sends `ViewportCommand::Visible(true)` + `Focus` after the first
  painted frame.
- ✅ **GUIDELINES §7 added** — "Established implementation decisions":
  typography/glyph policy, interaction rules (expansion 0, solid scrollbars,
  selectable_labels off, panel margins/clipping), the autosave model,
  component choices (context menus, menu popups, collapsible bars, image
  cache), and the hidden-until-first-frame startup. This is the reference
  for the upcoming Tournament/History redesign.

### UCI options measured layout (step 35)

- ✅ **Width-measured columns.** The 1–3 column split for UCI options was
  driven by a fixed ~420 pt per-column estimate, which let engines with long
  option names + wide range hints (Rybka: "Bishops Are Opposite Colored
  Penalty milipawns" + "(-1000–1000)") overflow into the neighboring column.
  `option_row_width` now measures each row (label galley at 13 pt + editor
  width by option type + spin range-hint galley at 11.5 pt + reset/spacing),
  and the column count is `avail / max_row_width` clamped 1–3 (still ≥ ~5
  rows per column). Rybka/Shredder get 1–2 columns depending on window
  width; short-named engines still get 3 on wide screens.
- ✅ **Hard guarantee:** each column `Ui` clips to its own rect, so even a
  pathological row truncates instead of painting over the next column.
- ✅ Tablebases rows and header summary reordered **Syzygy → Nalimov →
  Gaviota**. (Cache options were already hidden from per-engine editors in
  step 34 — verified live: Rybka shows only `NalimovProbeFrequency`, which is
  a probe frequency, not a cache size, and stays per-engine.)

### Scroll/header fixes + word-wise filter (step 36)

- ✅ **Options clip → horizontal only.** Step 35's per-column clip used the
  column's full `max_rect`, whose *vertical* bounds inside a `ScrollArea` cut
  the grid at a fixed content height — options were painted half-cut and the
  rest unreachable at small windows. The clip now constrains X only (that's
  all that's needed to prevent cross-column paint); Y stays unbounded so the
  scroll area works. Verified: Rybka's full option list scrolls to the last
  row at the minimum window size. **Rule: never clip the scroll axis inside
  a `ScrollArea`.**
- ✅ **Fixed-geometry engine header.** The header is one `allocate_exact_size`
  block (176 pt): identity fields (Name/Version/Author/Elo) at identical
  positions for every engine, and a logo slot right of them whose *center
  point is constant* — the image inside is aspect-fitted (≤ 220×128) so only
  its size varies; the Remove row is always reserved so the image doesn't
  shift when the button appears. Verified pixel-identical between Rybka
  (wide logo) and Houdini (default avatar).
- ✅ **Word-wise filter** — every whitespace-separated term must match in
  name/version/author/filename, so "shredder 12" or "rybka 4.1" work.
- ✅ **Startup blink** — reveal moved to the third pumped frame
  (`request_repaint` until then), so frames are actually presented before
  the window becomes visible.

### Shape-language unification (step 37)

- ✅ **One corner-radius language** (GUIDELINES §2.3 updated): everything is a
  rectangle with rounded corners — header tabs and status pills dropped from
  radius 14/10 to the app-wide 6; checkboxes get radius 3 via the new
  `widgets::checkbox(ui, checked, label)` (egui's default widget radius 6 on
  a ~16 px checkbox reads as a circle). All `ui.checkbox` call sites
  converted, including the Tournament tab, so the whole app is uniform.
- ✅ **One clear-button** — new `widgets::clear_button` ("×", the tablebase-row
  style): placed to the right of the field, shown only when there is text.
  The engine filter's inline floating ✕ was replaced with it; rule recorded
  in GUIDELINES §7.4.

### Scrollbar-free dropdowns app-wide (step 38)

- ✅ **`widgets::select`** — a `MenuButton::from_button`-based dropdown: button
  shows the current value (trailing spaces reserve room for a painted
  triangle arrow — font-safe per the glyph policy), the popup is a menu (no
  `ScrollArea`), items via `selectable_value`/`selectable_label`. Replaces
  `egui::ComboBox` at every call site: the UCI combo-option editor
  (engines tab), tournament Format / TC type / time-unit / Elo-policy
  selectors, and the engines sort selector (which had hand-rolled the same
  pattern). Root cause: ComboBox popups always wrap contents in
  `ScrollArea::max_height(combo_height)` which exhibits a phantom scrollbar
  with our theme even when content fits. Rule in GUIDELINES §7.4: always
  `widgets::select`, never `egui::ComboBox`.

### Ponder managed per tournament only (step 39)

- ✅ `resolve_options` (scheduler.rs) always inserts `Ponder` from the
  tournament's common options *after* merging per-engine overrides — a
  per-engine Ponder value was silently dead. The per-engine editor now hides
  `Ponder` (exact-name match, so options like `PonderTimeFactor` stay
  editable) via `is_globally_managed_option`; the "managed elsewhere"
  tooltip lists the full set: Threads/Hash/Ponder → Tournament tab,
  tablebase paths + probe caches → Endgame Tablebases panel. Rationale:
  Ponder is match-play policy, not an engine trait — one authoritative
  switch in tournament setup, no lying duplicate.
- ✅ 14 pt bottom padding after the last UCI option row, so the scrolled list
  doesn't sit flush against the pinned Clone/Delete action-row divider.

### Tournament/History to §7 standard + schedule estimate (step 40)

- ✅ **Glyph sweep** (§7.1): a full non-ASCII census of the remaining tabs
  found tofu in: Start Tournament (▶), Go/Stop/Force-Stop (▶⏸⏹), Presets and
  Export menus (▾), sort indicators (▲▼ → ↑↓), "✓ openings loaded",
  "▶ Playing" (→ ●), the viewer transport (⏮◀▶⏭ → «‹›»), History Refresh
  (⟳) and empty state (🏛 → ♟). New `widgets::dropdown_arrow(ui, rect)`
  paints the menu-button arrow (used by `select` and both Export/Presets
  menus — labels reserve room with trailing spaces).
- ✅ **Semibold headers** everywhere headers exist: `section_card` titles,
  `strong_header`/`sortable_header`, table headers, tab labels (selected),
  status pills, tinted buttons, live tournament name, History list/detail
  titles, viewer header, stats values. `.strong()` remains only for inline
  emphasis.
- ✅ **Schedule controls**: Cycles 1..=10 000 (SPRT-scale runs); Games/pair is
  a 1-or-2 dropdown (2 = both colours) — more repetition is exactly what
  Cycles does, so >2 was redundant; clamped in `build_config` and when
  loading old presets.
- ✅ **Schedule summary**: the Tournament card ends with "This will play N
  games · estimated ~D" (hover explains the ~60 moves/side assumption,
  sudden-death costed at full budget, ÷ parallel games, +5% overhead); the
  Start bar shows "N engines · G games · ~D". Nodes/depth controls say
  "duration depends on engine speed". `format_duration` gained h/m + d/h
  tiers; unit tests for clamping, formatting, and lane scaling.
- ✅ **Humanized TC hints**: `time_value_row` shows "= 2m00s" via
  `format_clock` and only when the unit isn't already milliseconds.
- ⚠ Code-complete and tested; **visual verification pending** (machine was
  locked at the end of the session).

### Tournament setup rework (step 41)

- ✅ **Two-column form** (`settings_form` split into per-card fns): Tournament
  / Engine Options / Elo / Output left, Time Control / Adjudication /
  Openings right when ≥ 720 pt, stacked otherwise. On a normal window the
  whole form and the Start bar are visible without scrolling.
- ✅ **Estimate placement**: games count stays in the Tournament card ("This
  will play N games."); the wall-clock estimate lives in the Time Control
  card ("Estimated length ~D (N games, L in parallel)") since duration is a
  function of the TC; the Start bar shows the compact combo. All per-field
  "= 2m00s"/ms conversion hints and the increment summary removed
  (`format_clock` deleted).
- ✅ **Engine selection**: filter (word-wise, shared `widgets::engine_matches`
  moved from engines_tab) with `clear_button`, Select-all respecting the
  filter, compact 36-pt rows — checkbox + 24-pt logo (LogoCache in the tab)
  or rounded-square monogram + semibold name + dim version + ⚠ + Elo right,
  whole-row click toggles. Circle-avatar helpers (`draw_avatar_in`,
  `engine_avatar`) deleted — the rounded square is the only silhouette now.
- ✅ **Verified end-to-end live**: a real 2-engine, 4-game, 100 ms/move
  tournament — estimate ~50 s vs 58 s wall clock, live table/Elo Δ/match
  stats/terminations all correct, history detail + game viewer + delete
  confirmed; the test tournament was removed from the DB afterwards.
- 🗒 **Live-board readiness** (analysis, not implemented): the GUI side is
  ready (snapshot `Arc<Mutex>` + event channel + 30 Hz live repaint + board
  renderer in `viewer.rs`), but `InFlightGame` carries only ids/round — the
  runner does not publish moves. Missing piece: the game runner records
  `moves: Vec<Move>` (+ clocks) into the in-flight snapshot entry after each
  half-move; the live view then adds per-game "Watch" buttons feeding a live
  variant of the board viewer, gated by a TC threshold (~≥ 300–500 ms/move)
  with a hint for faster games. Small, contained backend step.

### Per-engine per-tournament UCI overrides (step 42)

- ✅ **Data model**: `TournamentConfig.engine_overrides:
  HashMap<EngineId, BTreeMap<String, UciOptionValue>>` (`#[serde(default)]`
  → old stored tournaments load fine; persisted with the tournament, so
  resume keeps the overrides). `EngineId` serializes as a UUID string, valid
  as a JSON map key.
- ✅ **Precedence** (`resolve_options`, unit-tested): engine library options
  < tournament common (Threads/Hash/Ponder/Syzygy…) < per-engine tournament
  override. This is what makes the motivating case work: common Hash 8192
  with a Rybka 3 override of 2048.
- ✅ **UI**: right-click an engine-selection row → "Tournament UCI options…"
  opens a modal listing *all* detected options (including Threads/Hash —
  the point) with the engine's library-effective values as defaults
  (`effective_options` substitutes saved values into the detected schema),
  per-option × reset, Reset all, and a live "N options overridden" footer.
  Rows with overrides show an accent ● (hover explains); the menu's "Clear
  tournament overrides" reverts to Engines-tab + Tournament-tab settings.
  `build_config` prunes overrides to selected engines. Overrides are session
  state (not part of presets — they reference specific engine ids).
  `uci_option_row` moved to `widgets` (shared with the Engines tab editor).
- ✅ Labels trimmed per user: "40 games" and "~4h 12m" (assumptions and the
  games/parallel context moved into hover text).
- ✅ Verified live: modal opened from Rybka 3's row, Hash range shown as
  the engine declares (2–4096), override set to 2048, footer counted it,
  row dot appeared, Reset-all enabled.

### Tablebase globals, option mapping, compatibility notes (step 43)

- ✅ **Tablebases widget** (per the agreed analysis): Syzygy row gains
  "Probe limit" (3–7, default 7 — matches the files you own) and a "50-move
  rule" toggle (default on = FIDE-correct; the per-tournament checkbox was
  removed — one authoritative home, per-tournament exceptions possible via
  engine overrides); Gaviota row gains "Compression" (cp0–cp4, default cp4,
  sent only when the engine's combo offers the scheme). All injected by
  `apply_global_tablebases` when the matching path is set; hidden from
  per-engine editors (`is_globally_managed_option`: path/cache/50move/
  probelimit/compression). Probe depth, NalimovUsage/Frequency and vendor
  combos stay per-engine as agreed.
- ✅ **Option-name mapping** (real bug found by the user's Max CPUs remark):
  the scheduler used to insert literal `"Threads"`/`"Hash"`, which engines
  with different names (Rybka "Max CPUs") silently ignored. `resolve_options`
  now forwards to every detected option matching the shared
  `core::is_thread_option`/`is_hash_option` matchers, clamped to the
  option's min/max (Rybka 3 + Hash 8192 → 4096, not a crash). Unit-tested.
- ✅ **Compatibility notes**: `compatibility_notes` inspects selected engines
  against the tournament settings — no thread option (runs single-threaded),
  thread/hash values that will be clamped, missing Hash option, missing
  executable — skipping anything covered by a per-engine override. Shown as
  a "⚠ N compatibility notes" badge next to Start with the list on hover.
  Verified live: Fruit 2.1 "no thread option — runs single-threaded
  (Threads 6 requested)", Rybka 3 "Hash will be capped at 4096 MB".
- ✅ **Override dialog**: scroll area fills the modal (`auto_shrink
  [false, true]`) so the bar sits at the right edge; `effective_options`
  overlays tournament common Threads/Hash/Ponder (clamped, matching the
  scheduler) so Rybka's "Max CPUs" pre-fills with the tournament value.
- ✅ **Single status pill** — removed from the bottom status bar; the header
  pill (visible on every tab) is the one source of truth.
- ✅ **Logo loading budget** — with ~25 logo-carrying engines the first paint
  of a tab decoded + Lanczos-resized everything synchronously and froze the
  app for seconds ("Not Responding"). `LogoCache` now has a per-frame work
  budget (3 ops), returns "pending" beyond it (monogram fallback +
  `request_repaint`), and callers `begin_frame()` each frame. Logos fill in
  progressively over ~1–3 s with the UI fully responsive.

### Setup polish + instant startup (step 44)

- ✅ **Startup blink root-caused and fixed**: a Win32 window-creation monitor
  (poll `EnumWindows` for the process during launch) caught a visible
  `PseudoConsoleWindow` at ~135 ms — the debug binary was a console-subsystem
  exe (`windows_subsystem = "windows"` was release-only), so Windows spawned
  and then hid a console host at every launch. The attribute is now
  unconditional, and `AttachConsole(ATTACH_PARENT_PROCESS)` (tiny kernel32
  extern, no new deps) keeps tracing output working when launched from a
  terminal. Re-monitored: only the main window appears.
- ✅ **No layout shift on selection**: `pill_tab` lays the label out with the
  semibold galley in both states and reserves that width (`add_sized`), so
  switching tabs no longer nudges neighbours; `sortable_header` always
  appends the sort arrow via a two-section `LayoutJob`, transparent when the
  column is inactive, so activating a sort keeps every header in place.
- ✅ **Compatibility notes are clickable**: the "⚠ N compatibility notes"
  badge is a tinted-warn button (fill + stroke = obviously interactive)
  opening a modal that lists every note at once with a scroll cap at 320 pt;
  hover summary retained.
- ✅ **Visible entry point for per-engine tournament options** (UI practice:
  context menus must never be the only path to an action): engine rows show
  a "…" button on hover — in a permanently reserved 20-pt slot so rows never
  shift — that opens the Tournament-UCI-options editor directly; it stays
  visible (accent-coloured) on rows that carry overrides. Right-click menu
  kept. Gotcha fixed along the way: the row used `resp.hovered()` for its
  hover fill and the button reveal, but a child widget owning hover turns the
  parent's `hovered()` off — the button vanished under its own pointer and
  swallowed the click. Row state now uses `resp.contains_pointer()`.
- ✅ **Separator breathing room**: the settings `CentralPanel` has a 12-pt
  left inner margin, so the section cards no longer touch the engine panel's
  resize separator.

### Real startup fix, setup usability, Elo writeback (step 45)

- ✅ **Startup blink, actually fixed**: step 44 removed the console flash, but
  a blink remained. A visibility-*transition* monitor showed the main window
  going visible→hidden→visible: winit on Windows implements maximize via
  `ShowWindow(SW_MAXIMIZE)`, so `with_maximized(true)` on a hidden window
  briefly showed an empty maximized frame. `with_maximized` removed from the
  builder; the app sends `ViewportCommand::Maximized(true)` at reveal time
  (config value captured at construction — `capture_window_state` overwrites
  it every frame). Monitor confirms a single hidden→visible transition.
- ✅ **`widgets::dots_button`**: bordered rounded-square icon button with three
  painted dots (the "…" glyph sits on the baseline and read bottom-aligned),
  vertically centered in the engine row, accent-tinted when the row has
  overrides.
- ✅ **Gauntlet engine dropdown**: replaces the numeric seed count; lists the
  currently selected engines (name + version); picking one moves it to the
  front of the selection order (= seeding order) with `seeds = 1`.
- ✅ **TC type hints**: every type has a one-line explanation (fixed per-move /
  whole-game clock / base+increment / fixed nodes / fixed depth).
- ✅ **Elo section redesigned as library-rating writeback**
  (`TournamentConfig::rating_writeback`, serde-default None, honoured on
  resume): "Update ratings" = **Never** (default — ratings never touched) /
  **All engines** (final tournament ratings written back on finish; K-factor
  field) / **Estimate one engine** (engine picker defaulting to the gauntlet
  engine). Estimate uses `core::performance_rating` — bisection on the
  logistic Elo model against the opponents' *fixed library* ratings, ±800
  caps for 0%/100% scores, unit-tested — so only the estimated engine's
  rating changes. Applied automatically by `Backend::poll` on the
  Running→Finished transition (once, guarded). The live model is always
  per-game now (the standings Elo column ticks regardless of writeback); the
  old Update-policy select is gone. Verified live: 4-game gauntlet, Fruit
  0/4 vs Rybka 3 (3078) → library Fruit = 2278, Rybka unchanged.
- ✅ **Default name "Tournament"**, no longer restored from last-used settings
  (named presets still apply their stored name).

### Estimates split, hover-shift root cause, list sorting (step 46)

- ✅ **Estimates**: Time Control card = single game ("1 game: ~10s", weak
  label + semibold value, assumptions on hover); Tournament card = schedule
  summary ("2 engines · 200 games · ~2m 06s", semibold counts, faint dot
  separators, total on the estimate hover).
- ✅ **Hover shift ROOT-CAUSED** (pixel-diff method: capture the region with
  `Graphics::CopyFromScreen` before/after hover, diff bounding box): egui's
  `selectable_label`/`selectable_value` draws no frame while idle and adds
  frame padding when hovered — the widget widens and shifts everything to its
  right. New `widgets::choice_chip` (always-framed, accent-tinted when
  selected) replaced the in-row uses (openings Format/Order). RULE (also
  GUIDELINES §7): never use selectable labels in row layouts; popup menu
  items only, where nothing sits to their right.
- ✅ **Tournament engine-list sorting**: same Name/Elo/Author options as the
  Engines tab, persisted separately as `AppConfig::tournament_engines_sort`
  (default "name"); sort enum/comparator/select deduplicated into
  `widgets::EngineSort`, `sort_engine_indices`, `engine_sort_select`.
- ✅ **Output hint**: short "optional" hint (fits) + full explanation on
  hover.
- ✅ **Openings/Output audit** — both fully implemented: EPD (4-field → FEN,
  validated via shakmaty, opcodes ignored) and PGN (first N plies, `[FEN]`
  tag, comments/variations/NAGs stripped) parsing; deterministic SplitMix64
  Fisher–Yates shuffle from the stored seed (reproducible resume); count
  cap; scheduler draws one opening per *encounter* (`i / games_per_pair`) so
  both colours share it, cycling modulo the book; opening moves pre-played
  and included in stored PGN; live "N openings loaded" + first-line preview;
  PGN output appends each finished game to the chosen file as it ends.

### Results tab, ML ratings, live concurrency (step 47)

- ✅ **Results tab** (`results_tab.rs`) merges the live view and History:
  right-hand tournament list (active pinned "● live"), centre = live view or
  stored detail with Resume/Delete/Export; auto-follows starts/resumes
  (`TournamentTab::take_started` → app switches tabs). Tournament tab is
  setup-only; Start disabled while something is live; resume banner hidden
  while live.
- ✅ **Rating model**: `core::ml_ratings` — joint maximum-likelihood ratings
  (synchronous damped performance-rating iteration, re-centred on the
  participants' mean prior; engines without games keep their prior;
  unit-tested incl. 75% ≈ 191 Elo and anchor preservation). The live Elo
  column shows exactly what the writeback stores per mode (Never = static
  library, All = ML, Estimate = target-only performance rating, others
  anchored even in the live view). Priors are pinned on `ActiveTournament`
  at start/resume so the Δ column survives the automatic writeback.
  K-factor UI removed (the incremental model remains only as the Δ source
  for "Never" and for stored-history reconstruction).
- ✅ **Live concurrency**: `Command::SetConcurrency` + `Tournament::
  set_concurrency` + a Parallel DragValue in the live bar — running games
  always finish, only the launch rate changes; works running or stopped;
  persisted via `Store::update_tournament_config` so resume honours it;
  `TournamentSnapshot.concurrency` feeds the display and the ETA
  (measured average per game once ≥1 game timed, TC-based estimate before,
  ÷ lanes, +5%).
- ✅ **Per-engine forfeit tracking**: `GameOutcome::termination` →
  `EngineStanding::{time_losses, crash_losses}` (loser attribution,
  unit-tested) → "Forfeits" column ("2× time · 1× crash") in live and
  stored tables. `Standings::pair_results` keeps per-pair result sequences
  for the crosstable's per-game display.
- ✅ **H2H per format**: gauntlet (1 seed) = seed header + per-opponent rows
  (score + tinted record cell); otherwise crosstable. W-D-L / "Results"
  (`1 0 ½`) toggle via `choice_chip`. Fixed the dead-space bug: a
  *horizontal* ScrollArea with `auto_shrink([false,false])` claims all
  remaining height — shrink vertically.
- ✅ **Readable engine errors**: scheduler names the culprit, what happened,
  the opponent and the round ("X crashed vs Y (round 2) — loss awarded.
  Detail: …"); `EngineError` event now targets the actual loser.
- ✅ Setup polish: `small_button` retired app-wide (normal-framed buttons),
  tablebase note at the bottom of Engine Options, Start Tournament at the
  bottom-right with summary + compatibility badge beside it.

### Multiple simultaneous tournaments + Results UX (step 48)

- ✅ **Multi-active backend**: `Backend.actives: Vec<ActiveTournament>` with
  per-id APIs (`active(id)`, `active_priors(id)`, `active_ml_ratings(id)`,
  `apply_estimate_to_library(id, target)`, `set_active_concurrency(id, n)`,
  `close_tournament(id)`); `poll` drains every event channel, applies each
  finished tournament's writeback once, and returns the minimum repaint
  interval; `status()` aggregates for the header pill; `is_busy` = any
  running/stopping (close-confirm stops all). Starting no longer replaces —
  several tournaments can run in parallel; `try_resume` is a no-op when
  already loaded.
- ✅ **Click-to-load**: clicking an unfinished tournament in the Results list
  auto-resumes it (stopped, ready to Go) — no Resume button needed (it
  remains in the stored detail as a fallback). Initial auto-selection does
  NOT load; only explicit clicks do. Live-view caches are keyed by
  tournament id so switching is instant and correct.
- ✅ **Auto-refresh** (the "autosave" analogue for reads): the list re-reads
  at most once per second while the tab is visible plus immediately after
  every action; Refresh button removed. List rows show live progress
  ("109 / 1400 games") for loaded tournaments and the date otherwise.
- ✅ **Status convention**: Live (games in flight) / Stopped (idle or stored
  unfinished — a stored "running" row has no driver, so nothing is playing)
  / Finished (all games played), consistently capitalized, derived from the
  snapshot for loaded rows.
- ✅ **Banner removed**: the startup resume banner is gone entirely; the
  Results list is the single home for resuming (`find_resumable` deleted).
- ✅ Setup consistency: `widgets::filter_field` (28 pt text boxes shared by
  Engines + Tournament panels), Select all/Clear at the standard interact
  height matching the Sort select; engine rows carry explicit
  `UiBuilder::id_salt` (fixes a transient duplicate-ID red flash during
  relayout).

## 12. Deferred (architecture-ready)

Error-bar/Ordo rating recompute (see step 24); engine process pool; tablebase-based
adjudication (optional feature, off by default); macOS notarization; UCI_Chess960;
localization.
