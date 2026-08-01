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

In the current development build, `--dry-run --json` validates and prints this
design without launching engines. Live pair scheduling is not yet exposed.
