# CLI output contract

`colosseum-cli --json …` writes exactly one JSON value to standard output when
a command produces a final report. Progress, warnings and diagnostics are
written to standard error. A configuration or runtime failure before a final
report leaves standard output empty. A statistically invalid completed run is
still a report and therefore emits JSON with a nonzero exit status. The
top-level `type` field identifies the document schema.

`--dry-run` is a global option. It resolves configuration paths and prints the
configuration hash, the complete resolved configuration and every exact process
invocation without starting an engine or playing a game. An invocation is
represented by separate executable, argument-vector, working-directory,
environment, UCI-option and CPU-allocation fields. It is deliberately not a
shell command string: shell quoting is neither exact nor portable. The
`invocations` block shows the launch template both engines are started from,
and `execution.slots` shows the per-slot pinning each concurrent game will
actually be given.

The command-specific `type` values include:

- `capabilities`, `dry-run`, `self-test` and `run-status`;
- `engine-inspection` and `engine-compliance`;
- `fixed-match`, `sprt`, `calibration`, `spsa`, `spsa-plan` and `spsa-status`;
- `nps`, `nps-comparison` and `nps-scaling`;
- `book-hash`, `book-stats`, `book-verify` and `book-slice`;
- `stats-replay`, `stats-fixed-plan` and `stats-sprt-plan`;
- `tournament-plan` and `tournament`.

Position-suite JSON follows the same one-document stdout rule and identifies
its dry-run or suite report explicitly.

Every game Colosseum writes carries its schedule identity in the PGN header:
`GameNumber`, `PairNumber`, `PairGame`, `OpeningIndex` where a book supplied
one, and `OpeningLabel`, beside the standard seven tags and `OpeningPlyCount`.
It also names the CPU slot the game ran on, `GameSlot`, counting from zero, and
each side's time margin, `WhiteTimeMarginMs` and `BlackTimeMarginMs`; a forfeit
forensic names the slot too.
A game the run recorded but did not count — one abandoned on an infrastructure
fault, or a pair played after a sequential test had already concluded — also
carries `ColosseumSample`. Together they are what lets an exported PGN be replayed into
the same pentanomial vector as the run directory it came from; see
[statistics replay](stats.md).

A run directory is one evidence set. `stats <run-dir>` takes its statistics
from the game journal, which stays authoritative, and its search telemetry from
the directory's own `games.pgn`, so telemetry is never reported unavailable
when the annotated export is sitting beside the journal.

## Progress

A durable run prints one progress block on standard error every
`--progress-every N` units of its own work, and one more when it terminates.
The unit is what the command is made of: complete pairs for `sprt` and
`calibrate`, games for `match` and `tournament run`, committed iterations for
`spsa`. The defaults are 10 pairs, 20 games and 1 iteration.

Counting units rather than seconds means a block always reports the same amount
of new evidence, whatever the time control. `--progress-min-secs` (default 5)
is the one concession to the clock: a run whose units finish faster than that
coalesces its blocks instead of flooding the console, and prints at the first
boundary after the floor expires. A block is never printed for every committed
unit, and neither flag is part of the hashed configuration, so changing either
one does not stop a run directory resuming.

Each block names the command, the units done against the cap and the elapsed
time, then the figures a decision needs, and a rule closes it:

```text
progress [sprt]: 105/200 pairs (52%), 8m41s elapsed
  players         candidate vs. baseline
  games           210
  Elo             +193.1 +/- 45.0
  nElo            +248.0 +/- 47.0
  W/D/L           158/0/52
  Ptnml           [0, 0, 52, 0, 53]
  faults          time: candidate 0, baseline 0; other: candidate 0, baseline 0; 0 of 3 allowed
  LLR             +2.96 in [-2.94, 2.94] (accept H1)
  rate            724 pairs/hour
  time remaining  0s
--------------------------------------------------
```

An Elo estimate is a value and the half-width of its 95% interval; `Ptnml` is
the pentanomial vector; `time remaining` extrapolates the rate observed so far
and is `0s` once the run has stopped. A sequential test caps that estimate at
the pairs its `--max-pairs` still allows, and takes the nearer of that and the
pairs its LLR would need at the drift it has. The `faults` line counts
engine faults with losses on time among them, their rate over the games
played, and the allowance: 1% of the scheduled games and at least 5 for
`match`, `calibrate` and `tournament`; for `sprt` and `spsa`, which cannot know
their length, 0.5% of the games played so far and at least 3, rising as they
go. A forfeit within the allowance is scored as a loss and the run goes on. A
tournament reports the standings header with ratings and error bars and its
faults; a tune reports its faults, the
centres sitting on a rail and the knobs it has moved furthest since it began.

A block may also carry lines it records without printing: a tune's per
iteration trajectory is one of them, because a console that shows it every
block is a console nobody reads. Every block, printed lines and recorded
lines alike, is appended to `run.log`, and the most recent one is retained in
`run-record.json`, so [`status`](status.md) prints exactly what the console
last showed and a closed console loses nothing.

A final report never lists a run's games one by one. It says how many ended
abnormally and in which way; `games.pgn` holds every result, `run.log` every
event, and `failed-games/` the round-trip timing and UCI traffic of each
abnormal game.

Human-readable mode is the default. Automation should select `--json` and use
the process exit status as the primary success/failure signal.

For `match`, exit `0` means completed/valid, `1` means completed/invalid under
the fault thresholds, `2` means command-line or configuration refusal, and `3`
means infrastructure, persistence or runtime error. Sequential commands add
their documented H0/H1/inconclusive distinctions without reusing the error
codes.

For `sprt`, exit `0` is H1, `1` is H0, `4` is capped inconclusive and `5` is an
invalid experiment. Configuration refusal remains `2` and infrastructure,
runtime or persistence error remains `3`.

Exit `6` means a durable run stopped cleanly on request **and still had work
left to do** — an interrupt, or `spsa --stop-after-iteration`. Its run record is
`cancelled`, its checkpoint is written, and the same run directory resumes
where it stopped. A clean stop is not a failure and not a statistical
conclusion: an SPRT that stopped before a boundary or its cap reports
`cancelled` rather than `inconclusive`.

An interrupt that arrives once the last unit has been scored takes nothing
away. A match that played every requested game is `completed`, and a schedule
that reached its cap is capped `inconclusive` with exit `4`, because both had
already earned that verdict. See [run directories](run-directories.md) for how
stopping works.
