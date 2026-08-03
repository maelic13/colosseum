# SPSA tuning

`colosseum-cli spsa` tunes numeric UCI `spin` options exposed by one ordinary
engine executable. The engine needs no manifest, custom benchmark command or
source-tree integration. Colosseum reads the option schema from the normal UCI
handshake before playing the first game.

The required tune file is an ordered TOML parameter vector:

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

Array order is significant because the persisted random stream draws one sign
per parameter in this order. Start a tune with the executable, tune file and
terminal gain ratio:

```text
colosseum-cli spsa ./engine --tune tune.toml --r-end 0.002
```

The defaults are 5,000 iterations and 32 games per iteration. They are useful
starting values, not minimums. `--iterations 1 --games-per-iteration 2` is a
valid smoke run; games per iteration must be positive and even so every opening
has both colour assignments.

One floating-point centre vector is retained throughout the run. For each
iteration Colosseum derives deterministic plus/minus integer option vectors,
plays the same openings with colours reversed, and applies an update only after
every scheduled pair in that mini-match has completed. A crash, timeout,
disconnect, protocol fault or illegal move invalidates the iteration and the
tune. Its games remain as evidence, but a forfeit is never treated as a tuning
gradient. Infrastructure and persistence failures are likewise never scored.

All ordinary match conditions remain explicit: one of `--movetime-ms`,
`--base-ms` with optional `--increment-ms`, `--nodes` or `--depth`; adjudication
controls; concurrency and CPU placement; and an optional EPD/PGN `--book` with
order, start and PGN-ply controls. Without a book every game starts from
`startpos` and output records the lack of opening diversity. A supplied book is
parsed once when a process session starts and its in-memory openings are reused
across the complete tune. Engine processes themselves retain per-game isolation.
The single-engine `--cores` control is rejected because it cannot express two
disjoint arm allocations; use `--placement` with `--cores-per-engine` instead.
If Hash itself is tuned, trusted memory-budget checks use its declared upper
rail for both concurrent arms rather than the initial value.

Before any game, `spsa-schedule.json` is written, read back and checked against
the schedule derived from the resolved configuration. It records the exact RNG
algorithm, seed, draw order and gain constants. `--dry-run` resolves this
schedule without launching the engine; live UCI-schema validation consequently
occurs only when the actual run starts.

Use `--dir PATH` for an explicitly resumable run. Each checkpoint contains only
whole completed iterations; a hard stop during a mini-match replays that entire
mini-match on resume and cannot advance the gain schedule. Resume requires the
same resolved configuration, tune contents, engine path, schedule, book and
conditions. The stored iterations, games-per-iteration and `r_end` are
authoritative on resume, so repeating different values cannot silently change
the gain schedule. Logs append, PGN is rebuilt from committed evidence, and
`run-record.json` publishes each durable iteration after its checkpoint.

Exit code `0` means the requested horizon completed, `2` is a configuration
refusal, `3` is an infrastructure/runtime/persistence failure, and `5` means an
engine fault invalidated the tune. With `--json`, a terminal completed or
invalid tune emits one document; failures before a report keep stdout empty.
