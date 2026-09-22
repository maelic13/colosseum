# SPSA tune-file reference

SPSA needs one ordered TOML parameter vector. Each entry binds to a numeric UCI
`spin` option advertised by the engine during its normal handshake:

```toml
[[parameters]]
name = "Reduction"
initial = 12
min = 0
max = 64
c_end = 0.5

[[parameters]]
name = "Aspiration"
initial = 20
min = 1
max = 128
c_end = 1.25
```

| Field | Meaning |
|---|---|
| `name` | Exact case-sensitive advertised UCI option name; duplicates are errors |
| `initial` | Integer starting value; must lie within both requested and advertised ranges |
| `min`, `max` | Inclusive requested tuning rails, with `min < max` and both inside the engine's advertised range |
| `c_end` | Positive terminal perturbation size; must be at least `0.5` so the two rounded UCI arms remain distinguishable |

Array order is part of the reproducibility contract: the named random stream
draws one perturbation sign per entry in this order. Reordering entries changes
the schedule and therefore the run identity.

An `initial` value different from the engine default, or exactly on a requested
rail, is permitted but produces a durable warning. Colosseum validates the live
advertised schema before the first game; the file is not an engine descriptor
and cannot declare non-advertised options.

Audit the schedule and cost without launching an engine:

```text
colosseum-cli spsa plan --tune ./tune.toml --r-end 0.002
```

See [SPSA tuning](spsa.md) for gain controls, checkpoints, final-window output
and the `sprt --apply` gate.
