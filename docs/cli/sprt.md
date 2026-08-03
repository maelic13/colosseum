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

The final report always names the model, hypotheses, alpha/beta, exact Wald
bounds, finite cap, official pentanomial vector, fault counts and terminal or
invalid pair. LLR and decision are present once the sample is non-degenerate;
an all-identical early/capped sample reports that LLR is unavailable rather than
inventing a finite statistic.

Run artifacts use the common layout. The checkpoint stores official and
post-terminal pairs separately, `games.pgn` labels both classes, `run.log`
records pair commits, and both `result.json` and `run-record.json` retain the
resolved statistical design. Resume accepts only the same resolved conditions.

Automation exit codes are: `0` H1, `1` H0, `2` configuration refusal, `3`
infrastructure/runtime/persistence error, `4` cap-reached inconclusive, and `5`
invalid due to the engine/time-fault policy. Every terminal report, including
inconclusive and invalid, emits one JSON document with `--json`; pre-report
errors leave stdout empty.
