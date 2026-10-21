# Colosseum CLI changelog

All notable changes to Colosseum CLI are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] — 2026-10-21

What reading a finished run without a parser of your own asked for: each
engine's results by colour, its search at the end of the run, and where a
tune's arms ran into a range end. Reporting only: no game is played, paired,
scored or tested differently.

### Added
- `match`, `sprt` and `calibrate` end by printing each engine's wins, draws,
  losses, score and games as White and as Black, counting only the games the
  run counted, and each engine's search: annotated moves, depth, nodes and
  time per move and nodes per second. The same figures are kept in
  `run-record.json` under `results` and in the `--json` value
- `stats` splits a two-engine run's games by colour, in text and JSON
- Search telemetry reports nodes per move in its text line and
  `nodes_per_second`, total nodes over total search time, beside the
  per-move mean implied NPS
- A finished or stopped tune counts, per parameter, the iterations on which
  an arm was clamped at a range end — `centre - c` below the minimum or
  `centre + c` above the maximum — prints them as `clamped` in its final
  report and keeps one warning per such parameter in `result.json`
- `spsa history` shows those counts as they ran: `clamped_at_min` and
  `clamped_at_max` per row in JSON, and `<N` / `>N` beside a centre in the
  table
- `spsa --dry-run` warns about every parameter whose first arms, `initial ±
  c_0`, already leave its range, and lists them as `rail_warnings` in its
  JSON. It is a warning, not a refusal

### Changed
- `run-record.json` is schema version 6, and an SPSA `result.json` is schema
  version 4. `status` still reads a version 5 run record

## [0.3.0] — 2026-10-14

What moving engine data generation onto the harness asked for: the rules
counting repetitions as the Laws of Chess do, and short games played at
least as fast as other runners play them.

### Changed
- `tournament` and `gauntlet` keep each game slot's engine processes from
  game to game, like `match`, `sprt` and `spsa`, instead of starting two
  fresh processes for every game. Short games no longer pay an engine's
  start-up and handshake each time. `--engine-processes per-game` restores
  fresh processes. The setting is part of a tournament's configuration, so
  a tournament run directory from an earlier version is refused on resume
  as a changed configuration
- Commands reach an engine sooner and cost the harness less CPU: each engine
  has its own thread writing its input, and each command is one write. On
  Windows every command used to wait on the runtime's shared thread pool
  three times. The time charged to an engine's clock still runs from the
  `go` to the arrival of its `bestmove`

### Fixed
- Threefold repetition now counts the position a game starts from, as the
  Laws of Chess do: the start position (an EPD/FEN opening, or the position
  before a PGN opening's first move or `startpos`) occurring for the third
  time ends the game as a draw. Before, the start position needed a fourth
  occurrence. This is a rules fix in every command that plays games. It can
  shorten games whose start position recurs, and rarely changes their
  result; other games are unchanged. A run resumed from an earlier version
  plays its remaining games under the corrected rule

## [0.2.0] — 2026-09-25

What the first weeks of use asked for: resumed runs that keep their time and
say where they stand, a readable SPSA trajectory, and tunes that continue
from a finished one.

### Added
- `spsa history` prints a tune's centre vector after every completed
  iteration — as a table, CSV or JSON, optionally every Nth iteration — and
  works while the tune is still running
- `spsa --seed-from <run dir>` starts a new tune from a completed tune's final
  rounded values, keeping its parameters and bounds unless `--tune` gives new
  ones for the same parameters
- A warning, kept in the run record, when an option list on the command line
  replaces a run file's list and drops entries it named — such as a side's
  `Hash` and `Threads` options

### Changed
- A resumed run's progress blocks and final report show the run's total
  elapsed time, carried across every stop and resume, instead of restarting
  from zero. The pause between invocations is not counted, and the remaining
  time is still estimated from the current invocation's rate
- `spsa status` gives an ETA for a tune that has been stopped and resumed
- Every SPRT progress block, and the start of the run, shows what is being
  tested: the Elo bounds in their model, alpha and beta, and the preset they
  came from
- A resumed run says where it stood — "resuming: 1240 of 2000 games
  complete, 760 to play" — and with `--json` the JSON value carries the same
  facts as `resume`

### Fixed
- The resume note no longer reads "resuming 0 durable game(s)"
- `run.log` records a clean stop and a resume, as its documentation promised

## [0.1.0] — 2026-09-22

The first release of **Colosseum CLI**, a command-line harness for testing
chess engines. Point it at ordinary UCI executables — no plugin, manifest or
special build — and it plays matches, runs SPRT tests, tunes parameters with
SPSA, holds tournaments and measures speed, recording everything needed to
check or repeat a result later. It runs without a display, on a workstation or
a server.

### Highlights
- **`match`** — a fixed number of games between two engines, with an Elo
  estimate and its error bars
- **`sprt`** — the standard test for "is this change an improvement?": plays
  game pairs until the answer is clear or the cap is reached
- **`spsa`** — tunes an engine's numeric options, with `spsa plan` to cost a
  tune before starting it and `spsa status` to watch one while it runs
- **`tournament`** — round-robin and gauntlet events with joint ratings,
  standings and crosstable exports
- **`nps`** — engine speed at a fixed node count, A/B comparison between
  builds, and thread-scaling sweeps
- **`suite`** — scores an engine on an EPD/FEN test-position set
- **`engine inspect` / `engine check`** — shows what an engine reports and
  checks that it speaks UCI correctly before you spend hours on it
- **Interrupt and resume** — every long run can be stopped with Ctrl+C and
  picks up where it left off when the same command is run again

### Getting started
Download the archive for your system from the assets of this release:

| System | File |
|---|---|
| Windows x64 | `colosseum-cli-0.1.0-windows-x64.zip` |
| Windows on Arm | `colosseum-cli-0.1.0-windows-arm64.zip` |
| Linux x64 | `colosseum-cli-0.1.0-linux-x64.tar.gz` |
| macOS (Apple silicon) | `colosseum-cli-0.1.0-macos-arm64.tar.gz` |

Unpack it and run `colosseum-cli self-test` to confirm it works on your
machine. The archive includes the full guide in `docs/cli/`; it is also
online as the
[guide for this version](https://github.com/maelic13/colosseum/blob/cli-v0.1.0/docs/cli/README.md),
starting with the
[quickstart](https://github.com/maelic13/colosseum/blob/cli-v0.1.0/docs/cli/quickstart.md).

**What `0.x` means.** Commands, options, run files, the run-directory layout,
JSON output and exit codes may still change in a `0.x` minor release, and
this changelog will say so when they do. Automation that depends on them
should pin a specific version. Version `1.0.0` will be the promise that they
keep working.

### Added

#### Testing engines against each other
- Fixed-length matches with Elo and normalized-Elo estimates, played in
  colour-reversed pairs, from an opening book if you give one
- SPRT with ready-made settings for the two common questions — "is it
  stronger?" (`--preset gainer`) and "is this simplification no weaker?"
  (`--preset simplify`) — or your own bounds, a cap on the number of pairs, and
  a clear verdict: stronger, not stronger, or not yet decided
- Calibration: two copies of the same engine played against each other, to
  measure how much the result varies on your machine before trusting a
  small difference
- Time controls per side — move time, clock plus increment, fixed depth or
  fixed nodes — with an optional safety margin before a slow move loses on
  time
- Optional pondering, recorded with the run
- Games are played to the end. Draw and resignation adjudication are off
  unless you ask for them with `--draw-adjudication` or
  `--resign-adjudication`, and a rule setting given without its switch is
  refused rather than ignored
- Standard chess only: a Chess960 request is refused with an explanation
  rather than played wrongly

#### Tuning
- SPSA tuning of an engine's numeric (spin) options, described in a short
  TOML file
- `spsa plan` estimates the number of games and the wall time before you
  start; `spsa status` shows progress, an ETA and how each parameter is
  moving
- `spsa --stop-after-iteration N` stops a tune at a planned point without
  changing where it is heading when resumed
- The result records exactly which values were chosen, and
  `sprt --apply` tests the tuned values against the originals, refusing if
  the engine executable has changed since the tune

#### Tournaments
- Round-robin and gauntlet tournaments from one planner, with joint ratings
  and error bars, standings and crosstable CSV exports
- `--anchor` fixes one engine's rating; `--fixed <index>:<rating>` pins
  several engines at ratings you supply and rates only the others

#### Measuring speed and strength on positions
- Nodes-per-second measured at a fixed node count, A/B comparison of
  several builds with a fair alternating schedule, and thread-scaling
  sweeps
- EPD/FEN position suites with best-move and avoid-move scoring, resumable
  and comparable against an earlier run

#### Opening books and statistics
- Book tools: hash, verify, summarize and slice EPD/PGN books
- Openings are used in order from `--book-start`; a run that would need more
  openings than the book has left is refused, naming the shortfall, instead
  of silently repeating them (`--book-wrap` allows reuse), and `--dry-run`
  shows exactly which openings a run will use
- `stats` recomputes a result from a run directory or a PGN file, and plans
  how many games a test needs for the precision you want
- Every game in the PGN carries score, depth, time and nodes after each
  engine move, and `stats` summarizes them per engine

#### Runs you can trust and repeat
- Every run gets its own directory with the resolved configuration, the
  engines' executable hashes, the random seed and every game, so a result
  can be checked or reproduced later; `status` reads it without changing it
- A crashed, hung or misbehaving engine is recorded with evidence and
  attributed to the side that failed; a run is marked invalid only past the
  fault allowance you set
- Ctrl+C stops starting new games, lets the games in progress finish,
  saves and exits; a second Ctrl+C stops immediately. Either way, running
  the same command again resumes the run
- `--placement auto` pins engines to CPU cores chosen from what the
  operating system reports — keeping one core free, preferring
  performance cores and keeping each game on one cache and memory node —
  and refuses with an explanation when the system does not report enough
  to decide
- Optional TOML run files that can inherit from each other, with
  command-line options taking precedence
- `--dry-run` shows the complete resolved plan without starting an engine,
  and `--json` gives machine-readable output for every command
- `self-test` and `capabilities` check the installation and report what
  the system supports
