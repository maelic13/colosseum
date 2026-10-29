# Colosseum CLI 0.5.0 record

Two reporting corrections the maintainer asked for after 0.4.0, built on the
`cli-0.5.0` branch from `main` at `cli-v0.4.0`. Reporting only: nothing in
game play, scheduling, pairing, scoring, the SPRT or the SPSA update changed,
and no option entered or left the hashed configuration, so a run directory
0.4.0 left resumes under 0.5.0.

## What changed

1. **Games, not pairs.** An SPRT block counted pairs in its headline and
   reported games per hour beside it, two numbers that did not agree.
   `sprt` and `calibrate` now count the games in complete colour-reversed
   pairs (`2 x` the pairs: the games their statistics are computed over),
   in progress blocks, resume and stop events, the SPRT final report and
   `stats`/`stats plan`/`spsa --dry-run` text. `--progress-every` for both is
   in games, default 20, so blocks come as often as the old default of 10
   pairs. That games are played in pairs is a setting: `--max-pairs` and the
   JSON field names are unchanged. `ProgressUnit::Pairs` stays only so
   `status` reads 0.4.0 run records.
2. **Results in a file.** 0.4.0 printed the colour split, terminations,
   faults, search telemetry and a tournament's pairs below each command's
   own report, which buried the verdict. They now go to `results.txt` in the
   run directory, written through the run's writer before the record turns
   terminal; the console report names it. `run-record.json` and `--json`
   carry the same figures as before.
3. **Telemetry per side.** Search telemetry was keyed by the PGN's
   `White`/`Black` names, so one binary run twice with different options and
   no labels was one engine at the end of the run and in `stats`; scoring
   was always per side and correct.

## Decisions

**Per side through `GameNumber`.** A run's PGN names players; its journal
records which side had White in each game. `pgn_telemetry::RunSides` maps
each PGN game's `GameNumber` to that side, and the end of a run and `stats`
on a run directory both read the PGN that way, so an earlier run directory
splits too. A game number the journal gives both sides, or a game without
the tag, falls back to its tags; a PGN read on its own is still per name.

**Names made unique where the game is specified, not in the configuration.**
Two specs of one display name are named `name (A)` and `name (B)` when the
runner builds them, so the PGN, progress blocks, reports and telemetry agree
with the colour split's existing `(A)`/`(B)`. The launch specs and the
resolved configuration are untouched, which is what keeps the config hash and
resume unchanged. `--a-label`/`--b-label` still name the sides.

**Said in the dry run only.** The `match` and `sprt` dry runs print a note
on stderr naming the two reported names. A run does not: its quiet stderr is
asserted by existing tests, and its progress blocks already show
`name (A) vs. name (B)`. A calibration is one binary by design and says
nothing.

## Tests

- `progress`, `sprt`, `calibrate` unit tests: headlines and fields in games,
  no `pair` in a block.
- `pgn_telemetry`: two sides of one name kept apart by side; a game the
  journal does not place is named by its tags.
- Integration: the capped SPRT's block and report in games with
  `results.txt` named; a match's results in `results.txt` and not on stdout;
  one binary with two option sets reports two searches, named by side, whose
  move counts sum to the PGN's engine moves, in the run's JSON, `results.txt`
  and a `stats` replay, and the dry run's note.

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
warnings` and `cargo test --workspace --all-targets` (632 passed) on Windows;
`cargo xtask release-check cli-v0.5.0` passes. Nothing that plays games
changed, so the oracle replay and parity checks were not repeated.

## Delivery

The first release through the pull-request process of
[`DEVELOPMENT.md`](../DEVELOPMENT.md) *Delivery* and *Releases*, adopted on the
same branch: its pull request's `CI complete`, `Verify the complete CLI
matrix` and `Verify the complete GUI matrix` are the rehearsal, the CLI
candidate also running the release check because 0.5.0 is not yet tagged, and
`cli-v0.5.0` is an annotated tag on the merged `main`.
