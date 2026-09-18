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

Inspect the last checksum-verified durable snapshot of a running, interrupted
or completed tune without acquiring ownership or changing any run bytes:

```text
colosseum-cli spsa status path/to/spsa-run
```

The report includes completed iteration and percentage, a linear ETA when an
uninterrupted elapsed-to-checkpoint basis exists, and every knob's current
floating centre and normalized trajectory. With at least six completed
iterations it compares the mean of each history third and labels these fixed
heuristics:

- frequent exact bound contact means at least 20% of completed centres;
- little seed movement means at most 1% of the requested range;
- recent stability means the latest third spans at most 1% of the range;
- a scheduled perturbation below `0.5` is below UCI integer resolution.

Short histories say `insufficient-history`; they do not manufacture a trend.
Every signal explains that it can arise from the objective, noise, gain,
clipping, host variation or an unsuitable range. None is a causal or
convergence claim, and the command never advises continuing or abandoning a
tune. JSON status also exposes the same candidate-vector policy used at
completion, so whichever estimator the run selected is the one you see mid-run
as well; a snapshot that cannot yet produce a candidate explains why.

The tuned vector is the half-away-from-zero rounded **final centre vector**:
the centre after the last completed iteration. No checkpoint is selected after
the fact, so the vector you gate is the vector the tune actually arrived at.

`--final-window-percent N` selects the optional alternative: the mean of the
centres over the final `N` percent of the fixed horizon, from 1 through 100.
The sample count is rounded up, so every valid horizon contributes at least one
centre. Whichever estimator is in force is frozen in the resolved configuration
exactly as the default is, and the result names it and the exact window, so a
stored vector always says how it was produced. The result schema version
identifies the estimator set a reader can expect.

Completion writes the vector as `tuned-options.txt` (ready-to-paste UCI
`setoption` lines), `tuned-options.json` (the versioned machine artifact), and
`tuned-options.toml` (an `[engine.options]` run-file fragment).

`--stop-after-iteration N` asks for a clean stop once `N` iterations are
committed. The stop lands on an iteration boundary, so nothing partial is
discarded; the run writes its checkpoint, is recorded as `cancelled`, exits
with code `6`, and still emits a gate candidate from what it completed. The
stored horizon is untouched: resuming the same `--dir` continues towards the
original number of iterations. Use it to take a look at a long tune, or to
stop one at a planned point without pretending it finished.

**`N` is cumulative**, counted against every iteration the run directory holds,
including those a resume replayed. Repeating the same command therefore plays
nothing once the tune has reached `N`; raise the number to go further, in the
same way you would name a larger checkpoint to stop at.

One floating-point centre vector is retained throughout the run. For each
iteration Colosseum derives deterministic plus/minus integer option vectors,
plays the same openings with colours reversed, and applies an update only after
every scheduled pair in that mini-match has completed. An engine fault — a
crash, a loss on time, a disconnect, a protocol fault or an illegal move — is
scored as the loss it is, and the iteration it lands in is committed and moves
the gradient like any other. The tune becomes `invalid` only when engine faults
exceed the larger of 3 and 0.5% of the games played so far, checked as each
iteration is committed; the iteration that crosses the limit is kept as
evidence and applies no gradient. `--max-engine-faults N` and
`--max-time-losses N` set fixed limits instead, and `0` invalidates on the
first fault of that kind. Infrastructure and persistence failures are never
scored and still stop the tune.

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

Adjudication is off unless `--draw-adjudication` or `--resign-adjudication`
asks for it, exactly as in a fixed match, and the choice is frozen into the
run identity. When resignation is enabled it stays two-sided, because SPSA
compares perturbed arms of the *same* executable: a one-sided rule resigns
more readily for whichever arm scores more extremely, and that asymmetry lands
directly in the estimated gradient. `--one-sided-resign-adjudication` requires
`--resign-adjudication` and exists only to reproduce an existing methodology
deliberately. Whatever you pick, use the same rules for the tune and for the
gate that measures it.

`--ponder` is available when the tune uses a base/increment clock. It controls
both perturbation arms, is off by default and is frozen into the resolved run
identity. It is rejected with movetime, fixed nodes or fixed depth so
opportunistic opponent-time search cannot change the nominal fixed work.

Before any game, `spsa-schedule.json` is written, read back and checked against
the schedule derived from the resolved configuration. It records the exact RNG
algorithm, seed, draw order and gain constants, and names the random-stream
version they came from in `rng_version` — how the numbers were drawn, not how
any reported statistic is defined. `--dry-run` resolves this
schedule without launching the engine; live UCI-schema validation consequently
occurs only when the actual run starts.

Progress is written to stderr every `--progress-every N` committed iterations
(default 10) and once more at termination:

```text
progress [spsa]: 400/2000 iterations (20%), 41m12s elapsed
  time remaining          2h44m
  faults                  time 1/0, other 0/0; 1 of 64 allowed
  at a rail               none
  moved most since start  Hash +0.1840, Threads -0.0625, MoveOverhead +0.0090
```

The remaining time is a linear projection of the rate observed so far and
reads `0s` once the horizon is reached. Movement is measured as a fraction of
each knob's own range, because ten units of a thousand-wide knob and ten units
of a twenty-wide one are not the same fact, and it is measured from where the
tune began: a knob that has travelled is one the search is pushing, and that
is the only trajectory signal worth acting on mid-run.

The per-iteration trajectory — the last mini-match's pair score, the gain and
perturbation scale that iteration used, and what moved since the previous
block — is recorded in `run.log` and printed by `spsa status`, not by every
block on the console.

The final report is one table in tune-file order:

```text
SPSA completed
parameter     initial  tuned  estimate  delta    range
Hash               16    204  203.6412   +188  1..1024
Threads             4      3    3.4410     -1    1..16
estimator: rounded centre vector after iteration 1999
iterations: 2000 of 2000 committed; games: 64000
faults: time 0/0, other 0/0; 0 of 320 allowed
centres at a rail: none
artifacts: ./colosseum-runs/spsa-...
```

`estimate` is the unrounded value the estimator produced and `tuned` is the
integer a UCI engine is given; `delta` is that integer against the tune file's
`initial`, and `range` is the knob's own bounds, so a value that has reached
one is visible. `centres at a rail` names every knob whose tuned value sits on
a bound, where the gradient was one-sided. The paste-ready forms remain the
three `tuned-options` artifacts below.

Use `--dir PATH` for an explicitly resumable run. The journal records every
game with its iteration, and a resume rebuilds the completed iterations from
it; only whole iterations count, so a hard stop during a mini-match replays
that entire mini-match on resume and cannot advance the gain schedule. Resume requires the
same resolved configuration, tune contents, engine path, schedule, book and
conditions. The stored iterations, games-per-iteration and `r_end` are
authoritative on resume, so repeating different values cannot silently change
the gain schedule. The stored estimator is authoritative as well.
Logs and `games.pgn` append, and `run-record.json` publishes each durable
iteration. Every
written game names its iteration in `ColosseumSpsaIteration`, and an
invalidated iteration's games are kept marked `ColosseumSample "invalid"` so
[statistics replay](stats.md) leaves them out of the sample.

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
refusal, `3` is an infrastructure/runtime/persistence failure, and `5` means
engine faults exceeded their allowance and invalidated the tune. With `--json`, a terminal completed or
invalid tune emits one document; failures before a report keep stdout empty.
