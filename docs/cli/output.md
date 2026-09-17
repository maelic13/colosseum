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
A run that keeps games it did not count also marks each one with
`ColosseumSample`. Together they are what lets an exported PGN be replayed into
the same pentanomial vector as the run directory it came from; see
[statistics replay](stats.md).

A run directory is one evidence set. `stats <run-dir>` takes its statistics
from the checkpoint, which stays authoritative, and its search telemetry from
the directory's own `games.pgn`, so telemetry is never reported unavailable
when the annotated export is sitting beside the checkpoint.

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
