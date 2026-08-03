# SPSA tuning

`colosseum-cli spsa` tunes numeric UCI `spin` options exposed by one ordinary
engine executable. The engine needs no manifest, custom benchmark command or
source-tree integration. Colosseum reads the option schema from the normal UCI
handshake before playing the first game.

Before a live run, Colosseum audits every requested parameter against that
advertised `spin` range. Duplicate names, an initial value outside its requested
tuning range, invalid tuning bounds, any initial/bound outside the engine range,
and a terminal perturbation below `0.5` are rejected. The last rule matters
because UCI receives integers: below that magnitude the final plus/minus arms
can both receive the same value, so the knob is no longer being measured. A
starting value different from the engine default or equal to either requested
tuning rail is allowed but reported as a warning and retained in the result and
run record; both can be intentional choices.

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

Inspect the exact gain schedule and workload before launching an engine:

```text
colosseum-cli spsa plan --tune tune.toml --r-end 0.002 \
  --seconds-per-game-low 20 --seconds-per-game-high 35 \
  --concurrency 8 --compare-iterations 2500 --compare-iterations 10000
```

`spsa plan` is offline. It validates the tune-file invariants, reports total
iterations/games/pairs and durable checkpoint publications, and emits every
knob's exact `c/a/r` trajectory plus the first perturbation below half a UCI
integer unit, if one exists. Repeated `--compare-iterations` values show how
the first/final gains and cost change when the horizon changes.

A wall-time range is emitted only from explicit end-to-end game-duration
evidence. Supply a low/high seconds-per-game assumption as above, or repeat
`--pilot-game-seconds` with observed complete-game durations. Iterations remain
sequential, while games inside one mini-match are grouped into the requested
concurrency waves. This is workload arithmetic, not a prediction that a chess
tune will converge; curvature, sensitivity, interactions, noise and distance
from the optimum remain unknown.

On successful completion, the tuned vector is the half-away-from-zero rounded
mean of the final 10% of completed centre vectors. `--final-window-percent`
changes that percentage from 1 through 100. The sample count is rounded up, so
every valid horizon contributes at least one centre; the percentage and exact
zero-based window are frozen in the configuration and result. Completion writes
the same vector as `tuned-options.txt` (ready-to-paste UCI `setoption` lines),
`tuned-options.json` (the versioned machine artifact), and
`tuned-options.toml` (an `[engine.options]` run-file fragment).

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
the gain schedule. The stored final-window percentage is authoritative as well.
Logs append, PGN is rebuilt from committed evidence, and
`run-record.json` publishes each durable iteration after its checkpoint.

Gate the completed vector against its original values without editing the
result file:

```text
colosseum-cli sprt --apply path/to/spsa-run/result.json \
  --max-pairs 10000 --preset gainer
```

The tuned vector is engine A and the original vector is engine B. Both arms use
the exact recorded executable, arguments, environment, working directory and
non-tuned UCI options. The executable content must match the recorded SHA-256.
`--apply-executable PATH` can relocate it; identical content remains verified.
`--allow-executable-mismatch` is available for an intentional changed binary,
but the mismatch is printed prominently and retained in the SPRT result,
resolved configuration and run record.

Exit code `0` means the requested horizon completed, `2` is a configuration
refusal, `3` is an infrastructure/runtime/persistence failure, and `5` means an
engine fault invalidated the tune. With `--json`, a terminal completed or
invalid tune emits one document; failures before a report keep stdout empty.
