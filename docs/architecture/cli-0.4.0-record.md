# Colosseum CLI 0.4.0 record

Reporting additions a user asked for on 2026-10-21 so that a wrapper script
can build a run's manifest without parsing PGN comments itself. Built on a
branch from `main` at `cli-v0.3.0`. Reporting only: nothing in game play,
scheduling, pairing, scoring, the SPRT or the SPSA update changed. The dry run
of the recorded tune's configuration below hashes to the same `config_sha256`
as its recorded 0.3.0 dry run.

## What was added

1. **Results by colour.** For `match`, `sprt` and `calibrate`, each engine's
   wins, draws, losses, score and games as White and as Black, from its own
   side, over the official sample only (`scorable` and `sample = official` in
   `games.jsonl`; post-terminal pairs and unscorable games excluded). Printed
   at the end of the run, kept in `run-record.json` and the `--json` value,
   and reported by `stats` (text and JSON) for any source that says which
   engine had White and names exactly two engines that keep their sides.
2. **Telemetry at the end of a run.** The same per-engine search telemetry
   `stats` replays from `games.pgn`, plus `nodes_per_second` (total nodes over
   total time) and nodes per move in the text line.
3. **SPSA clamp counts.** Per knob, the completed iterations whose arm
   `centre - c_k` was below `min` or `centre + c_k` above `max`; in the final
   report (`clamped`), in `result.json`'s `tune_audit` (one
   `arms-clamped-at-range-end` warning per knob with a non-zero count) and as
   running counts in `spsa history`.
4. **Dry-run rail warning.** Every knob whose `initial ± c_0` leaves its
   range, on stderr and as `rail_warnings` in the dry-run JSON. A warning,
   never a refusal.

## Decisions

**`run-record.json`, not a separate `stats.json`.** The end-of-run figures go
into the run record's new `results` field (schema 6). The record is the
common status authority the wrapper already reads at completion; one file
keeps one schema version; and the field is written in the same record as the
terminal status, so a reader never sees a completed run without it. The
figures are read back from the run directory's own `games.jsonl` and
`games.pgn` after the writer has settled, by the code `stats` uses, so the end
of a run and a later replay cannot disagree. Version 5 records are still read,
so `status` works on runs 0.3.0 left; a resume rewrites the record as
version 6.

**Clamped means the unrounded arm left the range.** `colosseum_core::arm_clamp`
tests `centre - c < min` and `centre + c > max`. The alternative, "the integer
sent differs from what the unclamped arm rounds to", leaves out arms that are
outside by less than half a unit and are sent as the same end either way. It
undercounts against the oracle (CoreFpCorrection 51 against 76, CoreLdseMargin
28 against 50); the unrounded test reproduces every oracle count exactly, and
it is also the dry run's own wording. The arm pair `centre ± c` does not
depend on the perturbation sign, which only decides which engine plays which
arm, so "minus arm" here is the arm below the centre.

**Clamp counts are rebuilt, not stored.** Each iteration's arms follow from the
verified schedule's gain and the centres before it, as result schema 3 already
relies on. The end of a tune, and `spsa history` on a running one, recompute
them from the committed iterations; nothing new is written per iteration.

**Telemetry definitions unchanged.** Every annotated post-opening move counts,
mate scores included, and implied NPS stays the mean of per-move rates. The
new `nodes_per_second` is added beside it rather than replacing it.

**Scope.** Results by colour and end-of-run telemetry are for the two-engine
commands the request named. A tournament has many engines; its replay gives no
colour split.

## Oracle comparison

Oracle values computed independently by the requester from its recorded runs
(exact for counts, within 1% for telemetry).
Colosseum's figures from `stats` and from the end-of-run code on the same
directories; the end-of-run figures were produced by the ignored tests
`the_end_of_run_results_of_a_recorded_run` (`COLOSSEUM_RECORDED_RUN`) and
`the_end_of_run_clamp_counts_of_a_recorded_tune` (`COLOSSEUM_RECORDED_TUNE`),
which call the functions the commands call at their end, because neither run
can be replayed to its end without replaying its games.

### A recorded SPRT (19,344 official games): by colour — exact

| Engine A | Oracle W / D / L | Colosseum W / D / L |
|---|---|---|
| as White | 2755 / 5010 / 1907 | 2755 / 5010 / 1907 |
| as Black | 2050 / 4973 / 2649 | 2050 / 4973 / 2649 |

Engine B's split is the mirror image (as White 2649 / 4973 / 2050).

### The same SPRT: telemetry — differs by definition, explained

| | Oracle B / A | Colosseum B / A | Colosseum without mate-score moves |
|---|---|---|---|
| mean depth | 14.02 / 13.62 | 15.50 / 15.20 | 14.01 / 13.63 |
| nodes per move | 181,306 / 182,418 | 174,757 / 175,913 | 181,269 / 182,456 |
| ms per move | 73.6 / 73.7 | 70.4 / 70.5 | 73.58 / 73.72 |
| NPS | 2.46M / 2.48M | `nodes_per_second` 2.48M / 2.49M; implied NPS mean 2.79M / 2.81M | 2.464M / 2.475M (total over total) |

The oracle leaves out moves with a mate score (5.8% of moves; deep and short)
and computes NPS as total nodes over total time. Recomputed that way from the
same PGN, every figure is within 0.1% of the oracle. Colosseum counts every
annotated move, as its telemetry always has, so its depth is higher and its
nodes and time per move lower. Its implied NPS, the mean of per-move rates,
reads 13% above total-over-total because every move weighs the same; that is
why `nodes_per_second` was added. Coverage: 1,190,627 and 1,190,480 moves,
all annotated.

### A recorded tune (spsa, 82 knobs, 4,000 iterations): clamp counts — exact

| Knob | Oracle (minus arm) | Colosseum `clamped_at_min` |
|---|---:|---:|
| CoreRfpCorrection | 4000 | 4000 |
| CoreFpCorrection | 76 | 76 |
| CoreLdseMargin | 50 | 50 |
| CoreLmrAlphaGap | 18 | 18 |
| CoreRfpThreat | 5 | 5 |
| AspGrowthAdd | 4 | 4 |
| CoreCorrWeightMinor | 4 | 4 |
| CoreRfpImprovement | 1 | 1 |

No plus arm was clamped in either; every other knob counts zero.

### The same tune: dry-run rail warnings

The dry run of the tune's run file (120,000 games, seed 20261025) warns on CoreRfpCorrection (80 ± 256) and
CoreFpCorrection (213 ± 256), as the oracle expects, and on the six other
knobs whose seed lies within `c_0` of the minimum: AspGrowthAdd (5 ± 6),
CoreCorrWeightMinor (42 ± 48), CoreRfpImprovement (63 ± 64), CoreRfpThreat
(14 ± 16), CoreLmrAlphaGap (195 ± 256) and CoreLdseMargin (8 ± 12). They are
exactly the eight knobs the tune went on to clamp.

### Live run

A 12-pair SPRT between the same two engine builds (1+0.01 s, `Hash=16`, UHO book, concurrency 8) printed the colour and search
lines at its end and wrote them to a schema 6 `run-record.json`; the colour
split matched a direct count of its journal.

## Tests

- `colosseum-core`: `an_arm_is_clamped_when_its_unrounded_value_leaves_the_range`,
  `iteration_clamps_follow_the_shrinking_gain`.
- `stats_replay`: `run_results_split_the_official_games_by_colour_and_leave_the_rest_out`
  (post-terminal and unscorable games excluded),
  `a_pgn_of_two_engines_splits_by_colour_and_one_of_three_does_not` (also
  refuses a two-engine PGN whose engines swap which comes first, a defect the
  first draft had).
- `pgn_telemetry`: total nodes over total time beside the per-move mean.
- `run_record`: `a_version_five_record_still_reads_and_a_resume_rewrites_it_as_this_version`.
- Integration (`tests/cli/end_of_run_results.rs`): a match's console,
  `--json`, run record and replay agree; an SPRT that crosses its boundary
  splits only its official pairs; a calibration names its identical sides
  `(A)` and `(B)`.
- Integration (`tests/cli/spsa.rs`):
  `arms_clamped_at_a_range_end_are_warned_counted_and_shown_in_history`
  covers the dry run, `result.json` schema 4 and the history counts.

The required suite is unchanged in kind. Nothing that plays games changed, so
the oracle replay and parity checks were not repeated.

## Second batch: tournament reporting and the remaining hand calculations

A second request, 2026-10-22, listed every value the client still computed
by hand after a run. Each was checked against what Colosseum should report
for any engine; only the general ones were implemented. Reporting only, as
before: nothing in game play, scheduling, pairing, scoring, the SPRT or the
SPSA update changed.

| Request | Outcome |
|---|---|
| Per-pair results in tournaments (trinomial Elo, pentanomial, matrix) | Done |
| Complete standings with intervals | Done: progress blocks, end of run, `status` |
| No pooled number for a multi-opponent run; reconcile pair counts | Done |
| Terminations overall and per engine | Done |
| Faults by engine, kind, phase and scoring; pair without faults; time losses | Done |
| Non-mate depth | Done |
| Conversion audit (persistent material advantage, fixed thresholds) | **Not implemented**: one engine project's diagnostic, with its own thresholds and lone-minor rule, not a measure a general harness owes every user |
| `standings.csv` `AvgNps`, `stats --subject` on a run directory, ASCII `status`, `spsa status` clamp counts | Done |
| `stats compare` of two runs | Not implemented; it was optional |

### Decisions

**Trinomial pair Elo with the fixed 1.96.** `colosseum_core::trinomial_elo`
is the hand calculation from a crosstable cell, `p = (W + D/2)/n`,
`var = (W + D/4)/n - p^2`, interval at `p -/+ 1.96 se`, kept exactly so the
figures agree with it to the last decimal. Where a pair's games are
colour-paired, its pentanomial and the existing pentanomial Elo sit beside it.

**One analysis, read from the run directory.** `run_analysis` builds the
terminations, faults, standings and pairs from the journal and the resolved
configuration. A tournament's end-of-run summary, `stats` and `status` all
call it, so they cannot disagree, and a 0.3.0 run directory is read the same
way. A fault is in the `setup` phase when neither side has a charged search,
which is how a crash while the engine was started or readied is journalled.

**No pooled statistics across opponents.** A tournament whose counted games
span more than one pair of participants reports no pooled pentanomial or Elo;
its top-level W/D/L is the subject's score against the whole field, labelled
as such, and its statistics are per pair. A PGN whose games name more than two
engines is treated the same way. The tournament run record now counts colour
pairs, as `stats` does, and keeps its pentanomial at zero. Two existing replay
tests ran three participants of one executable without labels, which a PGN
cannot tell apart; they now label them.

**Golden values outside the repository.** The figures were checked against
the client's recorded runs, but the required suite is repository-only and
names no engine project, so the comparison ran from a script outside the
repository on `stats --json` and `status --json` of the release build: 66 of
66 values match, among them every pair's W-D-L, score and interval, the full
standings, 1,200 colour pairs and no pooled figure on the 4-thread gauntlet,
both termination tables, the three setup crashes scored as losses and the
pair without them (241-95-61, +169.9 [+139.0, +203.4]), the non-mate depth of
every engine of three gauntlets (for example 13.20 mean, 13 median over
127,169 moves), a run directory read from engine B's and from a non-seed's
side, an unknown subject refused, and `spsa status`'s clamp counts.

### Tests

- `colosseum-core`: `the_trinomial_interval_reproduces_the_hand_calculation`,
  `a_perfect_or_empty_score_has_no_finite_elo`.
- `run_analysis`: terminations overall and per engine; faults with their
  phase and scoring, and time losses; a pair's trinomial Elo, pentanomial and
  result without faults.
- `pgn_telemetry`: non-mate depth on the frozen annotated fixture.
- `progress`: stored symbols printed as ASCII.
- Integration: a round robin's pairs in both directions, its whole field in
  `status`, its colour pairs in the run record, `AvgNps` in `standings.csv`
  and no pooled figure in `stats`; `spsa status` clamp counts in text and
  JSON.
