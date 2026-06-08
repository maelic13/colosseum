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
| 12 | README, CHANGELOG, docs polish | Small |

## 11. Deferred (architecture-ready)

Tournament History tab UI; game/board viewer; non-RR formats (gauntlet/SPRT/knockout);
error-bar/Ordo rating recompute; engine process pool; tablebase-based adjudication (optional
feature, off by default); macOS notarization; UCI_Chess960; localization.
