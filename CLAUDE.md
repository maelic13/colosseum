# Colosseum

The repository of Colosseum CLI, a headless tool for testing ordinary UCI
chess engines. GPL-3.0. The primary development machine is Windows.

The desktop application, Colosseum, is a separate product in its own
repository, `maelic13/colosseum-gui` (local checkout `D:\code\colosseum-gui`),
and drives this CLI through its process protocol (ADR-0011, ADR-0012). The 1.x
egui application left this repository's `dev` at step 11.2.1; `main` keeps it
and its `gui-v` release lane until the merge at step 14.1, and its documents
are archived in `docs/archive/gui-1.x/`. `docs/gui-v2/` only points to the new
repository.

## Workspace

| Crate | Role |
|---|---|
| `colosseum-core` | Pure domain logic, no I/O: option, time, format and adjudication types, pairings, standings, rating math (`ml_ratings`, `performance_rating`, `rating_error`), SPRT/LOS and pentanomial statistics, SPSA, the RNG contract |
| `colosseum-application` | Runtime-neutral use cases, launch/run models and driven ports |
| `colosseum-uci` | UCI protocol + engine process management (spawn, handshake, search) |
| `colosseum-engine` | The one-game runner, PGN, openings, incident forensics (feature `runner`) and OS topology/allowed-CPU/affinity adapters (feature `platform`) |
| `colosseum-cli` | The headless composition root; ordinary UCI executables only. `composition.rs` holds the parser, dispatch and shared resolvers, with one module per command in `composition/` |

Commands: `cargo check --workspace --tests`, `cargo clippy --workspace
--all-targets -- -D warnings`, `cargo test --workspace --all-targets`; run the
CLI with `cargo run -p colosseum-cli -- --help`; builds and archives through
`cargo xtask build|package cli`. Before implementation work, read `AGENTS.md`,
`PLAN.md` and `GUIDE.md`. `PLAN.md` and `GUIDE.md` are the maintainer-facing
specification/tracker; `README.md`, `packaging/cli/README.md`, `docs/cli/` and
`CHANGELOG-CLI.md` are **user-facing** (keep them simple, no phase/internal-
method detail); `docs/DEVELOPMENT.md` holds implemented build, test, workspace
and release facts.

The user's real engine binaries are in `D:\chess\engines\` (37-entry library
incl. old, buggy engines — Rybka, Junior, Hydra — that crash/misbehave; treat
engine bugs as a real possibility when diagnosing, see the incident reports).

## Architecture in one paragraph

Every command resolves its inputs (command line over run file) into a run
directory: the resolved configuration and its hash, the engine executables'
hashes, one master seed, an append-only journal of committed games, the PGN,
checkpoints, a run record and the results. The drivers (`match_runner`,
`sprt_runner`, `spsa_driver`, `tournament_driver`) launch games up to the
concurrency limit on CPU slots from the placement plan, and each game is played
by `colosseum_engine::runner` through two UCI engine processes. Resume replays
the journal; a changed configuration is refused. Faults are classified and
counted, never folded into a score.

## Conventions that matter (learned the hard way)

- **Engine identity is "name version"** (e.g. `Basilisk 1.7.0`) wherever a
  single string names an engine — the desktop application keeps name and
  version apart in its library and passes the joined string as the label.
- **Ratings are always a joint ML recompute** (`ml_ratings`, Ordo-style,
  anchored to the participants' starting mean unless pinned) from the
  standings — never incremental K-factor Elo. Every engine carries
  `PRIOR_WEIGHT` virtual draws against its own prior (Bayesian damping — one
  win must not produce a capped ±400 split). Error bars via `rating_error`
  (Fisher information). The CLI computes; the desktop application keeps the
  engine library, passes library ratings as starting ratings, pins what must
  not move, and writes the CLI's figures back (PLAN, maintainer requirements,
  2026-09-29).
- **UCI option mapping is allowlist-based**: thread/hash options are matched
  by exact (whitespace/case-insensitive) names (`is_thread_option`,
  `is_hash_option`) — substring heuristics corrupted options like Rybka's
  "CPU Usage" (a % throttle) before. An unrecognised name is a visible miss,
  not silent corruption.
- **Engine processes: kept per slot for matches, fresh per game for
  tournaments.** The CLI keeps each slot's engines between games by default
  (`--engine-processes per-slot`, the Phase 10 record, item (ae)), replacing
  one after any fault: fresh processes put an engine's one-time work inside a
  game's search (Rarog's KPK bitbase forfeited games that way), and fastchess
  keeps its engines too. Tournaments keep fresh processes: measured on the
  real library, spawn+handshake+ucinewgame costs 17–350 ms (Rybka 3 ~840 ms)
  against ~34 s games, and reuse would trust `ucinewgame` in exactly the old
  engines known to leak state.
- **Durable writes are never silent**: a failed journal, PGN or checkpoint
  write ends the run as an infrastructure error.
- Serde configs tolerate unknown/missing fields (`#[serde(default)]`), so
  added fields stay compatible with existing run files and run directories.

## Verifying changes

Unit/integration tests cover core math, committed statistics fixtures, the
drivers against fixture engines, and the CLI surface
(`cargo test --workspace --all-targets`). The required suite is
repository-only. Real-engine runner/UCI smoke targets require the explicit
`real-engine-smoke` feature and `COLOSSEUM_SMOKE_ENGINE`; they do not count as
release or platform evidence. A change to anything that plays games repeats the
oracle replay and parity checks (`GUIDE.md`, recurring procedures).

## Implementation and commits

`GUIDE.md` numbered items are implemented in order, with the phase exit
demonstrated before proceeding. Complete one numbered step—including tests,
documentation and status evidence—then commit it before starting the next.
Use a short imperative subject naming the outcome, preferably including the
step identifier. Never add `Co-authored-by` or assistant-attribution trailers.
The full worktree, architecture and commit rules are binding in `AGENTS.md`.
