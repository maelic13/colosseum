# Sequential tests

`colosseum-cli sprt` defines a finite, pair-based sequential probability ratio
test between two ordinary UCI executables. Every design names its Elo model,
ordered hypotheses, error probabilities and maximum number of complete
colour-reversed pairs.

An explicit custom design supplies every statistical field:

```text
colosseum-cli sprt ./candidate ./baseline \
  --max-pairs 10000 --model normalized \
  --elo0 0 --elo1 3 --alpha 0.05 --beta 0.05
```

`normalized` and `logistic` hypotheses are different statistical statements;
the model is never inferred. The cap is mandatory. Reaching it without either
Wald boundary is an inconclusive result, not acceptance of H0.

Two named convenience bundles establish ordinary starting values:

| Preset | Model | `elo0` | `elo1` | `alpha` | `beta` | Meaning |
|---|---:|---:|---:|---:|---:|---|
| `gainer` | normalized | 0 | 5 | 0.05 | 0.05 | H1 supports a material gain |
| `simplify` | normalized | -5 | 0 | 0.05 | 0.05 | H1 supports non-regression within the chosen margin |

Use them with `--preset gainer` or `--preset simplify`. They are transparent
bundles rather than hidden modes: `--model`, `--elo0`, `--elo1`, `--alpha` and
`--beta` may override any bundled value, and the complete resolved design plus
its exact Wald bounds is stored and reported.

A completed SPSA result can supply both gate arms directly:

```text
colosseum-cli sprt --apply path/to/spsa-run/result.json \
  --max-pairs 10000 --preset gainer
```

No positional engines or per-side launch/UCI overrides are accepted in this
mode. Engine A receives the tuned vector and engine B the original vector, over
the otherwise identical recorded launch specification. Colosseum verifies the
current executable against the artifact's SHA-256 before dry-run or launch.
Use `--apply-executable` for a relocated copy. A content mismatch is refused
unless `--allow-executable-mismatch` is explicitly present; that override is a
warning and a structured field in every gate record. Statistical design and
ordinary match conditions remain explicit SPRT arguments.

SPRT uses the same direct engine, per-side clock, adjudication, placement,
concurrency, memory, optional book, seed, progress and run-directory controls
as [`match`](match.md). Each opening's two colour-reversed games form one commit
value, and concurrent completions become official only in pair-ID order. Once
that prefix crosses a Wald boundary, no new pair is launched. Already-running
pairs still finish both colours but are retained as post-terminal evidence and
cannot alter the official LLR or verdict.

This includes the explicit `--ponder` condition for clock-based tests. It is
off by default and becomes part of the resolved SPRT identity and run record.

The final report always names the model, hypotheses, alpha/beta, exact Wald
bounds, finite cap, official pentanomial vector, fault counts and terminal or
invalid pair. LLR and decision are present once the sample is non-degenerate;
an all-identical early/capped sample reports that LLR is unavailable rather than
inventing a finite statistic.

Progress is written to stderr every `--progress-every N` official pairs
(default 10) and once more at termination. An SPRT block carries the whole
decision: the two engines, the test itself (the Elo bounds of its two
hypotheses in the model they are measured in, alpha and beta, and the preset
it came from, marked overridden when any of its values was changed — the same
line is printed once when the run starts), games and pairs committed, logistic and normalized
Elo each with the half-width of its 95% interval, W/D/L, the pentanomial
vector, engine faults and time losses, the LLR against its exact Wald bounds,
games per hour — the unit every command reports throughput in, so two runs
compare directly — and the time still expected. That last figure extrapolates the
LLR the test already computed at its average drift per pair, never exceeds the
pairs `--max-pairs` still allows, and is `0s` once the test has stopped; a
sequential path is not a straight line, so read it as a projection.

The closing report names the hypotheses as an interval, what was concluded and
how long the invocation took:

```text
SPRT [0.00, 10.00] completed - H1 accepted: the gain is at least 10.00 normalized Elo
official sample: 105 pairs; post-terminal: 1 pair
model normalized: alpha 0.05, beta 0.05, cap 200 pairs
LLR 2.960096 in [-2.944439, 2.944439]
terminal pair: 105; invalid pair: none
artifacts: ./colosseum-runs/sprt-...
Finished match
Total Time: 8m41s
```

`--json` replaces that report with the single JSON document; progress blocks
stay on standard error either way.

Run artifacts use the common [run-directory layout](run-directories.md). The
journal records official and post-terminal games with their class,
`games.pgn` labels both classes in a `ColosseumSample` tag so
[statistics replay](stats.md) reaches the same official vector from either,
and both `result.json` and `run-record.json` retain the resolved statistical
design. Resume recomputes the official prefix from the journal and accepts
only the same resolved conditions.

An engine fault — a loss on time, a crash, an illegal move, a disconnect — is
scored as the loss it is. Its pair stays in the official sample in pair order,
and the LLR counts it like any other result, as fishtest and fastchess do. An
operating system occasionally holds a process for tens of milliseconds, and a
test that died on the first such forfeit could not run long at all.

The test becomes `invalid` only when engine faults exceed the larger of 3 and
0.5% of the games played so far. The limit is checked at every committed pair,
so an engine that keeps faulting still voids the test, and early. The fault
count, its rate over the games played and the allowance at that point are on
the `faults` line of every progress block and of the final report:

```text
faults: time: 0-2; other: 0-0; 2 of 4 allowed
```

`--max-engine-faults N` and `--max-time-losses N` set fixed limits instead, and
`0` restores strict invalidation on the first fault of that kind. A loss on
time is an engine fault, so an omitted time-loss limit follows the engine-fault
allowance. An infrastructure fault — an engine that never started, a refused
CPU placement — is never scored and still invalidates the run.

Automation exit codes are: `0` H1, `1` H0, `2` configuration refusal, `3`
infrastructure/runtime/persistence error, `4` cap-reached inconclusive, and `5`
invalid due to the engine/time-fault policy. Every terminal report, including
inconclusive and invalid, emits one JSON document with `--json`; pre-report
errors leave stdout empty.
