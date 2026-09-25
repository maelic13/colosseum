# Colosseum CLI changelog

All notable changes to Colosseum CLI are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
