# Colosseum

One repository for the independently versioned Colosseum desktop GUI and
headless Colosseum CLI chess-engine testing products. GPL-3.0. The primary
development machine is Windows.

## Workspace

| Crate | Role |
|---|---|
| `colosseum-core` | Pure domain logic, no I/O: config types, pairings, standings, rating math (`ml_ratings`, `performance_rating`, `rating_error`), SPRT/LOS stats, adjudication |
| `colosseum-application` | Runtime-neutral use cases, launch/run models and driven ports |
| `colosseum-uci` | UCI protocol + engine process management (spawn, handshake, search) |
| `colosseum-engine` | Tournament scheduler/driver, game runner, SQLite store, PGN/openings, incident forensics and OS topology/affinity adapters |
| `colosseum-gui` | eframe GUI composition root plus GUI-owned library/config/path adapters |
| `colosseum-cli` | Independent headless composition root; ordinary UCI executables only |

Commands: `cargo check --workspace --tests`, `cargo clippy --workspace`,
`cargo test --workspace --all-targets`; run the GUI with
`cargo run -p colosseum-gui` and the CLI with `cargo run -p colosseum-cli -- --help`.
Before implementation work, read `AGENTS.md`, `PLAN.md` and `GUIDE.md`.
`PLAN.md` and `GUIDE.md` are the maintainer-facing CLI specification/tracker;
`README.md` and the product changelogs are **user-facing** (keep them simple, no
phase/internal-method detail); `docs/DEVELOPMENT.md` holds implemented build,
test, workspace and release facts.
App data lives in `%APPDATA%\colosseum\` (`config/engines.json`,
`data/colosseum.db`, `data/logs/` incl. per-game incident reports);
`--portable` keeps everything next to the exe. The user's real engine
binaries are in `D:\chess\engines\` (37-entry library incl. old, buggy
engines — Rybka, Junior, Hydra — that crash/misbehave; treat engine bugs as
a real possibility when diagnosing, see the incident logs).

## Architecture in one paragraph

The GUI's `Backend` owns a tokio runtime and a `Vec<ActiveTournament>`; each
loaded tournament has a driver future (`scheduler::drive`) that launches
games up to the concurrency limit, and publishes a `TournamentSnapshot`
(standings, ML Elo entries, in-flight games with live search state) behind an
`Arc<Mutex>` the GUI reads every frame. Everything is persisted in SQLite
(`store.rs`) as it happens; resume replays finished games from the DB to
rebuild standings. Each game owns its two engine processes — spawned,
configured, played, and quit within `runner::run_game` (`kill_on_drop` covers
aborts).

## Conventions that matter (learned the hard way)

- **Engine identity is "name version"** (e.g. `Basilisk 1.7.0`) everywhere a
  single string names an engine: PGN White/Black tags, live view, standings,
  error messages (`versioned_name` in scheduler, `join_name_version` in GUI).
  Name and version are separate fields in the library/DB.
- **Ratings are always a joint ML recompute** (`ml_ratings`, Ordo-style,
  anchored to the participants' *tournament-start* mean — the DB `start_elo`
  seeds, never the current library) from the standings — never incremental
  K-factor Elo. Every engine carries `PRIOR_WEIGHT` virtual draws against
  its own prior (Bayesian damping — one win must not produce a capped ±400
  split). Error bars via `rating_error` (Fisher information). The
  `RatingWriteback` (None / All / Chosen) is applied to the library **after
  every finished game** (`Backend::apply_rating_writebacks`), and the Elo
  column shows exactly what the library holds; Δ is always vs tournament
  start. `Estimate(id)` survives only for deserializing old tournaments.
- **UCI option mapping is allowlist-based**: thread/hash options are matched
  by exact (whitespace/case-insensitive) names (`is_thread_option`,
  `is_hash_option`) — substring heuristics corrupted options like Rybka's
  "CPU Usage" (a % throttle) before. An unrecognised name is a visible miss,
  not silent corruption.
- **The Arena tab is live-only**: no per-game browsing/viewer in-app; users
  export PGN for analysis elsewhere. One tournament is always selected and
  auto-loaded; there is no "close tournament".
- **Engines are spawned per game, deliberately — do not add a process pool.**
  Measured on the real library (spawn+handshake+ucinewgame): modern engines
  17–350 ms, worst case Rybka 3 ~840 ms, vs ~34 s average game time — a 1–3%
  overhead. Reuse would break crash isolation and per-game forensics, keep
  idle Hash allocations alive, and trust `ucinewgame` state resets in exactly
  the old engines known to leak state (learning files, etc.).
- **Store writes are batched**: schedule inserts are one transaction
  (`insert_pending_games`) — per-row inserts froze the UI for minutes.
- GUI: all visual rules live in `docs/design/GUIDELINES.md` (binding). The
  non-negotiables: fixed-width table columns (never `Column::auto` on live
  data — jitter), no `selectable_label` in row layouts (hover shift), no
  `egui::ComboBox` (phantom scrollbar — use `widgets::select`), real bold via
  `theme::semibold` (embedded Inter), only font-verified glyphs, widget size
  must never depend on hover/selection state.
- Serde configs tolerate unknown/missing fields (`#[serde(default)]`),
  so removing config fields is backward-compatible with stored tournaments
  and presets.

## Verifying changes

Unit/integration tests cover core math, committed statistics fixtures, store,
scheduler, and GUI logic (`cargo test --workspace --all-targets`). The required
suite is repository-only. Real-engine runner/scheduler/UCI smoke targets require
the explicit `real-engine-smoke` feature and `COLOSSEUM_SMOKE_ENGINE`; they do
not count as release or platform evidence. Live-view/UI changes need a real
run: launch the app, start a short tournament (e.g. 2 engines, 100 ms/move)
with engines from `D:\chess\engines\`, and delete the test tournament
afterwards.

## Implementation and commits

`GUIDE.md` numbered items are implemented in order, with the phase exit
demonstrated before proceeding. Complete one numbered step—including tests,
documentation and status evidence—then commit it before starting the next.
Use a short imperative subject naming the outcome, preferably including the
step identifier. Never add `Co-authored-by` or assistant-attribution trailers.
The full worktree, architecture and commit rules are binding in `AGENTS.md`.
