# Colosseum CLI — engine-development harness plan

`colosseum-cli` is a headless, cross-platform harness for **developing** chess
engines: SPRT gates, SPSA tuning, fixed matches, gauntlets, speed measurement,
and the run-record machinery that makes those numbers trustworthy. Its only
contract with an engine is a UCI executable: no repository manifest, custom
build command or non-standard benchmark command is required.

The desktop app answers *"who is stronger?"*. The CLI answers *"is this build
stronger than that build, and can I believe the answer?"* — a different
question with harsher requirements: reproducibility, explicit CPU placement,
byte-level identification of inputs, durable long runs, and results that mean
the same thing next month.

**It is a general tool.** Rarog and Basilisk are the first two engines used to
validate it, not its specification. Where a policy could reasonably differ
between projects, the tool ships a default with a stated reason and lets the
user change it — see S3.

**Document audiences.** This file and [`GUIDE.md`](GUIDE.md) are the
maintainer-facing pair: specifications, success criteria, evidence, forward
plan. `README.md` is user-facing and covers the whole project — what Colosseum
is, the GUI, and the CLI — at an introductory level. The **user documentation**
(placement decided in Phase 9) is also user-facing and carries CLI detail:
command reference, worked examples, and how to trust a result. Neither
user-facing surface may carry phase numbers, internal naming or method
argumentation.

---

## S1. Current state

**The CLI is not built yet. This document is step 0 for that work.**

What exists is the reason the plan is short: `colosseum-core`, `colosseum-uci`
and `colosseum-engine` are already headless (no `egui` dependency anywhere in
them), already cross-platform, already released for Windows/Linux/macOS
including arm64, and already carry 149 tests. The CLI is a new workspace member
consuming them — not a refactor.

Validation engines: **Rarog** (Rust) and **Basilisk** (C++), chosen because they
are available, actively developed, and differ in language and build system.
Any two UCI engines would serve; nothing in the design depends on these.

---

## S2. Why this lives in Colosseum

A separate Python project was proposed first and **rejected after looking at
the code**. That analysis assumed the game-playing layer would be built from
scratch; it would not be. Recorded so the decision is not re-litigated:

| Already in Colosseum | Would have been rebuilt in Python |
|---|---|
| SPRT (LLR, bounds, H0/H1) + tests, `core/stats.rs` | ~200 lines + validation |
| Elo ± error, LOS, Ordo-style joint ML ratings | ~400 lines |
| UCI handshake, option auto-detect, quirky-engine option mapping | ~1,100 lines, and the quirks are only learned by running old engines |
| Tournament scheduler, pairing, parallel games | ~1,000 lines |
| Adjudication (draw/resign/max-moves) | ~150 lines |
| Books EPD **and** PGN, seeded random, both colours per opening | ~200 lines |
| TCs: movetime, sudden death, base+inc, fixed nodes, fixed depth | ~200 lines |
| PGN/CSV export, SQLite persistence, **resume after crash**, incident reports | ~1,500 lines |
| **Windows + Linux + macOS builds and a release pipeline, incl. arm64** | the entire original motivation |

Three further points that only became clear from the source:

- **Distribution flips to Rust.** A single static binary with no runtime and no
  package manager is a better deliverable than a `pip install`, and better again
  for a tool intended to be picked up by strangers.
- **The statistics are not a scipy-scale problem.** LLR, pentanomial variance,
  normalized Elo and bootstrap CIs are a few hundred lines of arithmetic.
  `stats.rs` already hand-rolls `erf`/`normal_cdf`.
- **The per-iteration process tax disappears structurally.** A common SPSA
  arrangement relaunches the match runner once per iteration; measured on the
  reference setup that is ~4 s of fixed overhead against ~0.77 s/game — ~14% of
  a 40-hour tune — most of it re-parsing a 167 MB / 2.6M-position opening book
  *to use 16 openings*. A long-lived driver loads the book once, so the cost is
  not optimised: it does not exist. (Per-*game* engine spawn stays, deliberately:
  17–350 ms against ~34 s games, and it buys crash isolation and per-game
  forensics. See `CLAUDE.md`.)

### The one real cost, accepted with mitigation

**Runner independence is lost** for anyone who uses this tool for both their
gates and their tournaments. Independent implementations catch each other's
bugs; one implementation cannot. Mitigation is the Phase 8 parity gate against
**two** external runners, and the standing recommendation that users keep a
second runner available for periodic cross-checks.

---

## S3. Policy model — enforced, default, recommended

The harness must not impose one project's methodology on its users, and must
still let a project run its own way in one flag. Every rule below is therefore
tagged with **how strongly the tool holds it**.

### Tier A — Enforced. The tool refuses to proceed.

Reserved for cases where continuing produces a **silently wrong number**. This
list is deliberately short, and additions need a stated failure mode.

- **A1. A requested capability that cannot be delivered is an error**, never a
  silent degradation. If CPU placement is requested and cannot be applied, the
  run fails. Explicitly disabling it is always allowed and is recorded.
- **A2. Degenerate statistics return a typed error, never `NaN`/`Inf`.**
- **A3. Every run writes a run record** (S5.8), including aborted runs.
- **A4. A resume never silently restarts, and never pools mismatched runs**
  (S5.11).
- **A5. Derived schedule constants are asserted against the file they were
  written to, before the first game is played** (S5.5).
- **A6. The statistical model in force is printed in every result block and
  stored in every run record.** `elo0=0 elo1=3` means materially different
  things under normalized and logistic models; the number alone is ambiguous.
- **A7. Every null result is reported with its resolution limit** (S3-C2).

### Tier B — Defaults. Shipped with a reason, changeable by anyone.

The tool ships opinionated defaults so the common case needs no configuration,
and every one of them is overridable on the command line or in a run file.

| Default | Value | Reason |
|---|---|---|
| Time control | `3+0.03` | Short enough for a gate to resolve in hours, long enough to exercise real time management |
| Hash | 64 MB when a compatible option is advertised | Small enough that concurrency × hash fits in ordinary RAM |
| Worker count | 1 when a compatible option is advertised | One variable at a time; parallel search adds its own nondeterminism |
| Pairing | both colours per opening | The pentanomial unit (S3-C4) requires it |
| Resign | `movecount=3 score=600 twosided=true` | See below |
| Draw | `movenumber=40 movecount=8 score=10` | See below |
| Concurrency headroom | 2 physical cores left free | Leaves room for the harness and the OS so game slots are not descheduled |
| SPSA horizon | 5,000 iterations | A useful default, not a floor; freely configurable |
| SPSA mini-match | 32 games/iteration | Same |
| Opening book | none | The tool ships no book and assumes no path |

**Resignation defaults to two-sided** because a one-sided rule adjudicates on
the losing side's own evaluation alone. That is a measurable asymmetry whenever
the two sides differ in how extreme their scores are — most sharply in SPSA,
where both arms are the *same binary* with perturbed parameters, so the arm that
scores more extremely resigns more readily than its sibling and the difference
lands directly in the estimated gradient. Requiring both engines to agree
removes the asymmetry by construction. The threshold of 600 cp over 3 moves is
high enough that agreement at that margin is rarely wrong.

**The draw default is conservative on both axes.** Adjudicating a draw ends a
game that might still have contained a decisive result, and a false draw biases
the measured score directly. Three properties reduce that risk: requiring
agreement for 8 consecutive moves rather than a single ply; requiring a tight
band (|score| ≤ 10 cp) rather than a loose one; and not starting before move 40,
because evaluations in the opening and early middlegame are least reliable and
most likely to agree by coincidence rather than because the position is drawn.
The cost is throughput — games run longer — which is a deliberate trade of speed
for fewer false terminations. Faster settings are perfectly reasonable for
users who value throughput more; the user documentation names common
alternatives, including the values used by well-known public testing
frameworks.

**Profiles.** `--profile <name>` applies a named bundle of Tier-B defaults, so a
project can run its house style in one flag without that style being imposed on
anyone else. Ships with at least `default` and `strict`; users can define their
own in the config file. Resolution order:

```text
built-in defaults  <  profile  <  run file  <  command line
```

### Tier C — Recommended. Documentation only, zero code impact.

Guidance the tool never enforces, published in the user documentation because it
is what separates a number from a result.

- **C1. A sequential test is the verdict; everything else is a diagnostic.**
  Static-evaluation losses, node counts, search depth and NPS all correlate
  imperfectly with strength; several have moved the wrong way while strength
  improved, and vice versa.
- **C2. A null is not proof of no effect.** Report it as "no effect larger than
  X", where X is the smallest effect the design could have detected. The tool
  computes and prints that limit (A7) from the game count, the draw rate and the
  pairing, so the user does not have to derive it.
- **C3. `[-3,+3]` is not an equivalence test.** Non-inferiority is `[-3,0]`;
  equivalence is a fixed-N run with a containment rule on the interval.
- **C4. Prefer pentanomial pairs and normalized Elo** — see S3-C4 below.
- **C5. Validate a measuring instrument before trusting it.** A self pair (the
  same binary in both arms) should read zero. Two independently written speed
  estimators once read −0.2…−0.4% on a self pair and had already produced two
  confident false rejections before anyone checked; the cause was that the
  underlying sample is left-skewed, so any estimator weighting the arms
  unequally against the slow tail manufactures a bias.
- **C6. Keep tuning and gating conditions identical** — same TC, same book, same
  adjudication. A tuner optimising under different game-termination rules than
  the gate measures is optimising a different objective.
- **C7. Book choice is a measurement decision.** Unbalanced openings played from
  both colours are symmetric (so unbiased) but decisive, cutting the draw rate
  substantially and resolving tests in far fewer games. Balanced books give
  rating estimates more comparable with public rating lists. Running out of
  openings inflates error bars by reusing pairs; the tool detects and reports
  reuse (S5.4).
- **C8. Explicit CPU placement matters for clock-based tests.** Leaving the OS
  to schedule engine processes can introduce a per-run offset large enough to
  swamp the effect being measured, and it varies with machine topology. Related
  trap: pinning one core per *game* starves engines configured to use several
  worker threads.
- **C9. Speed is reported as speed.** The tool never converts an NPS difference
  into Elo; the conversion factor depends on the engine, the time control and
  the search, and is only valid where it was measured.

#### S3-C4. Why pentanomial pairs and normalized Elo

Two games from the same opening played from both colours are **correlated** —
they share whatever imbalance the opening carries. Treating them as independent
underestimates the variance and yields error bars that are too narrow, which
makes tests look more conclusive than they are. The pentanomial model takes the
*pair* as the unit and gets the variance right.

Normalized Elo then rescales by the standard deviation of the pair outcome, so a
given bound means the same thing across time controls, books and engines with
different draw rates. That is what makes a bound like `[0,3]` portable between
projects and stable over time; the same bound expressed in logistic Elo is not.

Both models are supported and the model in force is always printed (A6).

---

## S4. Architecture

```
colosseum-core      pure math + domain      ← pentanomial stats, nElo, SPSA schedule
colosseum-uci       UCI protocol/process    ← unchanged
colosseum-engine    driver/scheduler/store  ← + CPU affinity, durable runs
colosseum-cli       NEW: bin + commands     ← run config/records, NPS, SPSA driver
colosseum-gui       desktop app             ← unchanged; may consume new stats
```

**Placement rule:** pure functions go in `core` (so the GUI can reuse them and
so they are trivially testable); anything that spawns or places a process goes
in `engine`; anything that is a *workflow* goes in `cli`.

**The CLI never depends on the GUI**, on the GUI's configuration, or on its
application-data locations. It reads no engine library and writes no shared
database by default; a run is self-contained in its run directory (S5.11).

### Engine invocation and configuration

The required input is an executable path. Each side may additionally declare a
display name, arguments, working directory, allocated cores and arbitrary UCI
option values. Engine identity and the supported option schema come from the
normal UCI handshake.

Every ordinary workflow is fully controllable with CLI arguments. A run file
(TOML) is optional, for repeatability. SPSA is the one exception: its parameter
vector needs a tune file, because a large vector is not usable as command-line
syntax. Every run writes its fully resolved configuration as JSON — generated
output, never a required user-authored manifest.

### Explicitly out of scope

Building engines, artifact discovery, custom bench/fingerprint commands,
compiler/source-tree inspection, parameter baking into source, build-flavour
logic, dataset generation and neural-net management. The CLI consumes finished
UCI executables. Non-chess variants are out of scope; Chess960 is a Phase-8
decision.

---

## S5. Tool specifications and success criteria

Every criterion must be checkable by a test or a single command. "It looks
right" is not a criterion.

### 5.0 CLI invocation and configuration

**Requirements**

- Bare UCI executable paths are sufficient. Per side: optional display name,
  arguments, working directory, arbitrary UCI option values, allocated cores.
- `engine inspect` prints UCI identity and the advertised option schema.
- `engine check` runs a **compliance report** with per-requirement pass/fail:
  handshake, `isready`, option round-trip, a bounded search returning a legal
  `bestmove`, `stop` honoured promptly, clean shutdown, and behaviour on
  `ucinewgame`. Exit code reflects the outcome.
- `--profile`, run file and CLI arguments resolve per S3 Tier B.
- `--dry-run` prints the fully resolved configuration and the exact engine
  invocations without playing a game.

**Success criteria**

- Two arbitrary UCI executables can be inspected and compliance-checked using
  paths and CLI arguments only.
- The same run launched from a run file plus overrides resolves to
  byte-identical JSON as the equivalent all-CLI invocation.
- Adding a new conforming engine requires no file in the engine's repository and
  no Rust code.

### 5.1 Pentanomial statistics and normalized Elo — `colosseum-core`

**Why first:** everything downstream reports through it, and the existing
`sprt()` is trinomial over W/D/L.

**Requirements**

- Pairs, not games, are the unit: each opening played from both colours yields a
  pair score in {0, 0.5, 1, 1.5, 2} → the pentanomial vector.
- Pentanomial variance; normalized Elo; logistic Elo with error bars; LOS; draw
  ratio; pairs ratio; WL/DD ratio.
- SPRT over both the pentanomial/normalized and logistic models, selectable,
  reporting LLR and both bounds with H0/H1/continue — and always naming the
  model in force (A6).
- **Minimum detectable effect** for the design, so nulls can be reported per A7.
- An unpaired fallback for odd counts and gauntlets, clearly labelled.

**Success criteria**

- **Golden-file parity** against vendored fixtures (S6.2) reproduces every
  reported statistic to ≤1e-6 on continuous values and exactly on integer
  vectors, for both external oracles.
- **Analytic fixtures**: hand-computed pentanomial vectors with independently
  derived LLR/nElo/variance, so correctness does not rest on any third-party
  tool being right.
- **Property tests:** LLR is 0 at zero games; monotone in the score at fixed N;
  symmetric under swapping arms and negating bounds; bounds equal
  `log(β/(1−α))` and `log((1−β)/α)`.
- Degenerate inputs (all draws, zero games, one pair, 100% score) return typed
  errors.

### 5.2 CPU topology and affinity — `colosseum-engine`

**Requirements**

- Detect physical cores and their logical CPUs per OS: Windows
  `GetLogicalProcessorInformationEx`, Linux `thread_siblings_list`, macOS
  `sysctl`. **Never infer SMT siblings from logical CPU numbering.**
- Modes `auto` / `off` / explicit CPU list; configurable headroom (default 2
  physical cores free) in `auto`.
- Allocate the configured **cores-per-engine** to each game slot — not one core
  per game. This allocation is independent of whichever UCI option controls the
  engine's internal worker count.
- Fail when requested placement cannot be applied (A1); allow and record `off`.
- macOS has no supported hard-affinity API: report the capability as advisory or
  unavailable, record which, and do not prohibit clock matches.

**Success criteria**

- Unit tests over recorded topology fixtures (SMT 16c/32t, performance/efficiency
  cores, single-socket no-SMT, dual-socket) assert the chosen CPU list.
- An integration test spawns busy children under a pinning request and samples
  residency; skipped with a clear message where the OS cannot enforce it.
- `capabilities` command prints what this platform can and cannot do.

### 5.3 Calibration — `colosseum-cli calibrate`

An optional end-to-end symmetry test on the actual machine. It does not prove
correctness and is never a prerequisite for another command.

**Requirements**

- Byte-identical binary on both sides — refuse if the SHA-256 differs.
- Configurable fixed N (default 30,000), confidence (default 95%) and tolerance
  (default ±5 nElo); no early stopping.
- PASS iff the full interval lies inside the tolerance; report *inconclusive* if
  the interval is wider than the tolerance rather than declaring PASS.
- Any timeout, crash, disconnect or illegal move marks the run invalid.

**Success criteria:** hash mismatch rejected; configurable values round-trip
through persistence and resume; PASS/FAIL/inconclusive/invalid each have
deterministic tests.

### 5.4 Fixed match and SPRT — `colosseum-cli match|sprt`

**Requirements**

- `match` is fixed-N with no sequential stopping. `sprt` accepts explicit
  `elo0`/`elo1`/`alpha`/`beta` and model; `gainer` and `simplify` are named
  convenience bundles, not hard-coded semantics.
- Two engines by path, per-side arguments, working directory, UCI options and
  allocated cores. The same binary may be tested against itself or against
  itself with different options.
- **Time controls, per side independently:** movetime, sudden death, base +
  increment, fixed nodes, fixed depth — plus a configurable time margin so
  scheduler jitter is not counted as a loss on time. Asymmetric controls are
  supported (odds matches, "same engine at double time").
- **Adjudication:** draw, resign and max-moves each individually configurable and
  each individually **disableable**; optional tablebase adjudication where
  tablebase paths are supplied.
- **Time-loss accounting is first class**: losses on time, crashes, disconnects
  and illegal moves are counted per engine, printed in every report block, and
  stored in the run record. `--max-time-losses N` flags or aborts the run.
  A test whose engines forfeit is not a valid test, and the user must not have
  to go looking for that.
- **Concurrency** is explicit: number of parallel games, its interaction with
  cores-per-engine and headroom, and a startup estimate of total memory
  (concurrency × 2 × hash). Refuse or warn — configurably — when the request
  exceeds available cores or memory.
- **Book is optional.** Without one, every game starts from the initial position
  and the tool warns that opening diversity is absent. With one: order
  (sequential or seeded random), start index, ply depth, and **reuse detection**
  — the fraction of openings played more than once is reported, because reuse
  narrows error bars misleadingly.
- **Engine crash policy** is explicit and configurable: abandon the game and
  count it, discard it, or retry it once. The choice biases results and is
  therefore recorded, never implicit.
- Live report block on a configurable interval; full log to disk; PGN out; run
  record; per-game engine output retained for failed games.
- **Machine-readable results**: JSON to a file or stdout, and **exit codes that
  distinguish H1 / H0 / still-running / invalid / error**, so the tool can be
  scripted and wired into CI.
- Resume per S5.11.

**Success criteria**

- Replaying stored fixture outcomes reaches the same verdict at the same game
  number as both external oracles (±1 report interval).
- A path-only invocation needs no configuration file; identical binaries with
  identical options are allowed for self-play.
- Every exit code is asserted by a test.
- A forfeit-injecting stub engine triggers the time-loss counters and the
  `--max-time-losses` policy.
- Killing mid-run and resuming yields the same final statistics as an
  uninterrupted run at the same seed.

### 5.5 SPSA — `colosseum-cli spsa` + `colosseum-core` schedule

**Requirements**

- **End-state parameterization:** each knob declares `c_end`; the run declares
  `r_end` and horizon `N`; `c`, `a` and `A = 0.1·N` are back-solved
  (`alpha=0.601`, `gamma=0.102`). Deriving from the horizon is what stops a
  hand-picked gain going stale when the horizon changes.
- Decay per **iteration**, never per game.
- The tune file selects numeric UCI options and supplies initial value, tuning
  bounds and `c_end` per knob, validated against the live UCI option schema.
- Defaults `N=5,000` and 32 games/iteration; configurable, not enforced minima.
- Persistent driver: no per-iteration relaunch; a supplied book is loaded once.
- Multi-session per S5.11.
- **Config audit**, against the live schema:
  1. option absent or not a numeric `spin` *(error)*
  2. duplicate parameter *(error)*
  3. initial value or bounds outside the advertised range *(error)*
  4. `min >= max` — the knob cannot be measured *(error)*
  5. perturbation rounds to zero before the horizon *(error)* — the engine
     receives a rounded integer, so once the perturbation falls below half a
     unit both arms see the same value: the knob stops being measured while
     still being updated, and random-walks for the rest of the run
  6. initial value disagrees with the engine default *(warn — may be deliberate)*
  7. initial value on a rail *(warn — one-sided gradient)*
- **Closing the loop (S5.5a).** A tune must not end at "here is a vector".
  On completion, and on demand mid-run, emit the tail-mean vector as
  (a) a ready-to-paste `setoption` list, (b) JSON, and (c) a run file fragment.
  `colosseum-cli sprt --apply <result.json>` then gates the tuned values against
  the untuned binary **using UCI options only** — no source edit, no rebuild, no
  engine-specific baking step. Users who prefer to bake values into their source
  still can; they no longer have to.

**Success criteria**

- Schedule property tests: `c_t == c_end` and `a_t == a_end` exactly at `t = N`;
  `r_end` invariant as `N` varies while `a` moves.
- Written-artifact assertion (A5): the persisted schedule is read back and
  verified before any game is played; a test mutates the file and asserts the
  launch refuses.
- Every hard audit class has a fixture that must be rejected.
- Recovery: kill at iteration K, resume, schedule continues rather than
  restarting at full gain; the log retains pre-kill iterations.
- Convergence smoke test against a synthetic noisy quadratic with a known
  optimum lands within a stated RMSE band.
- **Loop test:** a tune over a stub engine produces a result file that
  `sprt --apply` consumes without hand-editing.

### 5.6 Speed / NPS A/B — `colosseum-cli nps`

**Requirements**

- Drive an optional user-supplied position suite through standard bounded
  searches; use the initial position when omitted and warn that the workload is
  weaker. Consume `info nodes`/`time`/`nps` and report clearly when the engine
  does not expose enough information.
- Strict alternation, warm-up, arm-level **median** and **best-of**, bootstrap
  CI on the median.
- Accept one or more executables per arm; show per-executable medians so
  non-overlap is visible. Multiple builds are supported, never required.
- A self pair is recommended and optional; warn when a matching recorded self
  pair lies outside a configurable tolerance (default ±0.5%).
- Report per-round SD as a machine-noise indicator.
- **Scaling sweep:** measure across a list of worker counts (e.g. 1, 2, 4, 8,
  16) and report speed and efficiency per count. Answers "does this engine
  scale?" without a second tool, and the position set must be pinned and
  recorded so a later sweep is comparable.

**Success criteria**

- A self-pair result is reported without being a prerequisite.
- A synthetic left-skewed sample reproduces the known bias in a naive
  alternating-pair estimator and *not* in the shipped one.

### 5.7 Gauntlet — `colosseum-cli gauntlet`

Opponent ladder, joint ML ratings with error bars, optional anchor,
standings/crosstable CSV, and **resume per S5.11** — gauntlets are the longest
runs the tool performs.

**Success criteria:** ratings match the GUI to ≤0.01 Elo on stored data;
kill/resume produces identical standings.

### 5.8 Run record

Generated JSON per run: both engines' canonical path, SHA-256, UCI identity,
arguments, working directory and effective options; harness version and build;
**`schema_version` and `stats_version`**; host summary (OS, CPU model, physical
and logical core counts); optional book path and hash; seed; resolved affinity
and capability mode; time control; adjudication settings; concurrency;
full command line; UTC start/end; outcome, statistics and anomaly counts.

**Versioning policy.** `schema_version` changes when the record's shape changes;
`stats_version` changes when any reported statistic changes definition. Both are
documented in a changelog so results taken months apart remain interpretable.

**Success criteria:** a record is written for every run including aborted ones;
a test asserts every observable field is populated and every not-applicable
optional field is explicitly null with a reason; a schema-version bump fails a
test that pins the current schema unless the changelog is updated.

### 5.9 Book tools — `colosseum-cli book`

`slice` (deterministic given a seed), `hash`, `stats` (count, ply depth, eval
band where present), `verify` (every position legal and parseable).

**Success criteria:** slicing is byte-reproducible across platforms; `verify`
rejects a known-bad fixture.

### 5.10 Statistics replay — `colosseum-cli stats`

Read a Colosseum run, a PGN, or a supported external result log and report the
same block used live.

**The durable artifact is authoritative, not the console.** A console log can be
buffered, truncated by a lost terminal, or simply not flushed for hours; none of
that is evidence about the run. Every command that reports live must be
reproducible from the run directory and the PGN afterwards, and `stats` is how.
A stalled log must never be the only way to ask "is this run alive?" — that is
what `status` (5.5c, generalised to every long command) answers.

**Search telemetry from a PGN.** Where move annotations are present, report per
engine: mean and median reported depth, time per move, and implied nodes per
second. At a fixed node limit this is the cleanest available comparison of how
two engines spend the same budget, and it needs no instrumentation in either.

**Success criteria:** every golden fixture replays through the command with the
same result as the library API; telemetry aggregates match hand-computed values
on a fixture PGN; a PGN written without annotations produces a clear
"unavailable" rather than zeros.

### 5.5b SPSA sizing — `colosseum-cli spsa plan`

Offline, no games. Given a horizon, a knob count, a mini-match size, a measured
or assumed per-iteration cost and a noise model, simulate the shipped schedule
and report expected convergence, the wall-clock estimate, and how the answer
changes with the horizon.

**Why it earns its place:** the alternative to modelling a tune is folklore, and
the unit of error is *machine-nights*. A run that would have needed 5,000
iterations, launched at 1,500, produces a fitted-looking vector that is barely
distinguishable from its seed — and nothing in the output says so. This is a few
hundred lines of arithmetic that prevents that.

**Success criteria:** given a synthetic objective whose optimum is known, the
predicted RMSE band contains the observed RMSE of an actual `spsa` run over the
same objective; the wall-clock estimate is within 10% of a measured short run.

### 5.5c SPSA diagnostics — `colosseum-cli spsa status`

Reads a run directory and reports, without touching the running tune: iteration
and percent complete, ETA, per-knob current value and trajectory, and a
**convergence check on the trajectory rather than on single iterations** —
compare each knob's mean over the second and third thirds of the run, normalised
to its range, because single-iteration values are far too noisy to eyeball and
SPSA wanders even at low gain.

Also flags, per knob: values pinned at a bound (the range was wrong, not the
value converged), values that have returned to their seed and stayed there (the
tuner has *rejected* that knob — a result, not a failure), and knobs whose
perturbation has decayed below the engine's rounding resolution (no longer
measured, still being updated).

**Why it earns its place:** deciding whether to abandon a multi-night tune is a
routine operation, and doing it by hand means parsing a multi-hundred-megabyte
log correctly under time pressure. The tool has the state; it should answer the
question.

**Success criteria:** on a recorded fixture run, the thirds comparison, the rail
detection and the dead-knob detection each match hand-computed values; running
`status` against a live run directory neither blocks nor mutates it.

### 5.12 Data generation — `colosseum-cli datagen`  ⚖ scope decision

**Proposed in scope, to be confirmed.** Self-play or engine-vs-engine games at a
fixed node or depth limit from a book, written as PGN, appending across runs, at
full concurrency with the same placement and durability guarantees as any other
long run.

The argument for: this is exactly what the harness already does — schedule games,
place them on cores, survive interruption, write PGN. Every engine project that
trains anything needs it, and re-implementing it outside means a second
scheduler with none of the durability or placement work.

The line: **generating games is in scope; interpreting them is not.** Extraction
into a training format, position filtering, deduplication and label construction
are specific to a trainer and stay with whoever owns it. The tool's output is
PGN.

**Success criteria (if adopted):** a datagen run resumes per S5.11 and appends
without duplicating games; a fixed node limit produces the documented node count
per move; concurrency and placement behave as in `match`.

### 5.11 Durable runs — one contract for every long command

Applies uniformly to `match`, `sprt`, `spsa`, `calibrate` and `gauntlet`. It is
Tier A because the failure mode is silent data loss or silent pooling of
incomparable games.

- A run lives in a **run directory** with a predictable layout (state, log, PGN,
  run record, resolved config). `--dir` selects it; a portable mode keeps
  everything beside the executable.
- **Resume is the default** when a run directory already exists. Starting over
  requires an explicit flag, and that flag **archives rather than deletes**.
- **A resume refuses** if the stored configuration differs materially from the
  requested one — engine paths or hashes, time control, book or its hash,
  adjudication, bounds, model, schedule. Pooling games from different conditions
  is the failure this prevents.
- **Logs append; they never truncate.**
- The **stored** horizon and schedule win over command-line arguments on a
  resume, and the tool says so on screen rather than silently ignoring the flag.
- State is written at least every K units and **atomically** (write to a
  temporary file, then rename), so a hard kill costs at most K units and can
  never leave a corrupt state file.
- Interrupting is a supported operation, not an accident: a clean stop and a
  hard kill must both be recoverable.

**Success criteria:** one shared test suite runs against every long command —
kill at a random point, resume, and reach statistics identical to an
uninterrupted run; a mismatched-config resume is refused with a precise message;
a truncation attempt on the log fails; a state file corrupted mid-write is
detected and the previous state used.

---

### 5.13 Coverage target — what the CLI must replace

The point of the tool is that an engine project keeps **no harness scripts of
its own**. That is only auditable if the boundary is written down, so here is
the mapping against a real, mature harness (~3,400 lines of PowerShell plus
Python across two engines). Anything in the left column that is not claimed by a
spec above is a gap in this plan, not a script the user should keep writing.

| Existing tool | Replaced by |
|---|---|
| SPRT driver, null calibration | 5.3, 5.4 |
| SPSA driver | 5.5 |
| SPSA config audit | 5.5 audit classes |
| SPSA sizing model | 5.5b |
| "is my tune converging?" log analysis | 5.5c |
| Gauntlet driver | 5.7 |
| NPS A/B, multi-build pooling | 5.6 |
| Thread-scaling sweep | 5.6 scaling sweep |
| UCI probe / handshake helper | 5.0 `engine inspect` / `check` |
| Result recomputation from PGN | 5.10 |
| Per-engine depth/time from PGN | 5.10 telemetry |
| Affinity, topology, concurrency, seeds, hashing | 5.2, 5.8 |
| Console log filtering and tee | CLI logging + 5.10 |
| Book handling and slicing | 5.9 |
| Self-play data generation | 5.12 (if adopted) |
| Runner/tuner fetching and patching | evaporates — nothing to vendor or patch |

**Residual, and deliberately so — these stay with the engine, forever.** They
depend on the engine's source, build system or internals, so no general harness
can own them:

- building engines, PGO/instruction-set flavours, artifact naming
- profiling (sampling profilers, platform trace tooling)
- correctness suites tied to the engine's own move generator or search
- engine-specific diagnostic counters and their readouts
- evaluation tuning and training-data extraction, labelling and filtering
- baking tuned values into source, if the project prefers that to UCI options

**Success criterion (checked at Phase 9):** both validation engines can delete
every script in the left column and keep only the residual list, with no
workflow lost. Any exception is recorded here as a named gap with a decision.

## S6. Testing requirements — binding

A harness bug is worse than an engine bug, because it is invisible in exactly
the measurement meant to catch it.

1. **Every pure function is unit-tested**, including degenerate inputs. No
   statistical function returns `NaN`/`Inf` without a typed error.
2. **Golden-file parity against vendored fixtures, from two independent
   oracles.** Fixtures are produced by running real UCI engines — two versions
   of the same engine is a convenient way to get a small, known Elo difference,
   and any pair of engines will do — through **both `fastchess` and
   `cutechess-cli`**, and the resulting logs are committed with the tool name,
   tool version and the exact command line that produced them. Engine names are
   anonymised in the fixtures so nothing implies a particular project.
   A documented **generator** regenerates or extends the corpus from any engine
   pair. Two oracles matter: agreement between independently written
   implementations is much stronger evidence than agreement with one, and where
   they disagree that disagreement is itself the finding.
3. **⛔ No test may depend on a path outside the repository.** Enforced in CI.
   This is what makes the project independently buildable and testable by
   anyone who clones it.
4. **Analytic fixtures** with hand-derived expected values, so correctness does
   not rest on any external tool being right.
5. **Property-based tests** for statistics and the SPSA schedule.
6. **Integration tests with a cross-platform stub engine.** A tiny UCI responder
   built as a test binary, replacing the current Windows-only shell stubs, and
   **shipped as a documented artifact** so users can smoke-test the harness with
   no real engine and file reproducible bug reports.
7. **Fault injection:** crash at handshake / mid-search / on quit; timeout;
   illegal move; never answering `isready`; garbage on stdout. Each must produce
   a specific tested outcome and none may be silently absorbed into a result.
8. **Determinism:** same seed and stubs ⇒ same pairings, opening order and final
   statistics, on every platform.
9. **Durable-run suite** (S5.11) against every long command.
10. **Calibration is optional end-to-end evidence**, not a CI or user
    prerequisite; its outcome classification is tested deterministically.
11. **CI matrix: Windows, Linux, macOS × debug and release.** Debug is not
    optional — a debug build is far slower, a CI runner slower again, and that
    combination is exactly how a flat-timeout test passes locally for months and
    fails on a runner.
12. **No new `clippy` warnings**; the workspace lint wall stays at zero.

---

## S7. Multi-platform requirements

Windows, Linux, macOS — all first-class, x64 and arm64 where the release
pipeline already builds them.

Divergences to handle explicitly, each with a test or a documented fallback: CPU
topology and affinity (5.2; macOS may be advisory-only); executable suffix and
path separators; process spawn/kill semantics; symlink and permission handling
for engine binaries; high-resolution timing; file locking on the store; line
endings in PGN/EPD parsing.

**Rule:** platform support requires the full test suite and documented
capability/fallback behaviour there. Calibration results describe one machine
and configuration; they never determine whether an operating system is
supported.

---

## S8. Implementation plan

### Phase 0 — Project identity and release model
Decisions that shape everything downstream, taken before any code.

- **(a) Naming.** A separate "Coliseum" chess application already exists and
  aims at a similar space, which creates real ambiguity in search, package
  registries and user conversation. Evaluate: keep the product name and give the
  CLI a distinct binary name; rename the CLI only; or rename the product.
  Criteria: search-result collision, availability on the package registries the
  release targets, trademark risk, whether the name survives being said aloud in
  a bug report, and the cost of renaming a released 1.0 product (users, links,
  packaging, existing installs). Record the decision and the rejected options.
- **(b) Release model.** The GUI is a released 1.0 desktop product; the CLI will
  churn. Evaluate one repository with two release tracks and independent version
  numbers, versus splitting the CLI into its own repository consuming the core
  crates (via a registry release or a git dependency). Criteria: whether a CLI
  change can destabilise a GUI release, CI complexity, how the shared core is
  versioned and tested, contributor friction, and how a bug fix in the core
  reaches both. **Exit:** both decisions recorded here with reasoning, and the
  release pipeline sketched for the chosen model.

### Phase 1 — Pentanomial statistics and normalized Elo (`core`)
Spec 5.1 plus the fixture corpus (S6.2–S6.4). First because everything reports
through it and it needs no I/O or platform surface. **Exit:** analytic fixtures
pass; golden-file parity against both oracles; no test reads outside the repo.

### Phase 2 — CLI skeleton, direct UCI invocation, run records, durable runs
Specs 5.0 + 5.8 + 5.11. Workspace member, argument parsing, profiles and
resolution order, run file, `engine inspect`/`check` compliance report,
`--dry-run`, run directory layout, run records with schema/stats versioning, and
the durable-run contract with its shared test suite. **Exit:** two arbitrary UCI
executables pass path-only workflows; run file plus overrides resolves
identically to all-CLI; the durable-run suite passes against a stub command.

### Phase 3 — CPU topology and affinity
Spec 5.2, including the `capabilities` command. **Exit:** topology fixtures pass
on all three OSes; residency tests pass where enforceable; platform capability
reporting documented.

### Phase 4 — Fixed match, SPRT, calibration
Specs 5.3 + 5.4. Time controls per side, adjudication including disable and
tablebases, time-loss accounting and policy, concurrency and memory checks, book
options with reuse detection, crash policy, JSON output and exit codes, resume.
**Exit:** verdict parity against both oracles; forfeit and exit-code tests;
path-only and no-book runs; calibration outcome tests.

### Phase 5 — SPSA
Spec 5.5 including 5.5a loop closure. **Exit:** schedule property tests; every
hard audit class rejected; recovery; convergence smoke test; the tune result
feeds `sprt --apply` unedited.

### Phase 6 — Speed/NPS, book tools, statistics replay
Specs 5.6 + 5.9 + 5.10. **Exit:** skew-bias regression test; reproducible book
slicing; every golden fixture replays through the CLI.

### Phase 7 — Gauntlet
Spec 5.7. **Exit:** ratings match the GUI to ≤0.01 Elo; kill/resume identical.

### Phase 8 — Parity against external runners, and remaining gaps

- **(a) Parity run — the entry gate for trusting this harness.** Prepare one
  deterministic opening sequence, then play it with the same two binaries, time
  control and adjudication through `colosseum-cli`, `fastchess` and
  `cutechess-cli`. Compare Elo/nElo/LOS, the pentanomial vector, draw rate and
  time-loss counts. Agreement within combined error bars is the gate;
  disagreement is a defect in one of the three and must be root-caused. Repeat
  after any change to game-running code.
- **(b) Remaining feature gaps.** Revisit what the external runners do that this
  does not, and decide per feature: adopt, decline with a reason, or defer.
  Candidates: Chess960, ponder under test conditions, additional tournament
  formats, output formats other tools consume. **The tie-breaker is whether a
  general engine developer needs it**, not whether the validation engines do.

### Phase 9 — Documentation and release
The deliverable is a tool any engine developer can pick up.

- **(a) Documentation placement analysis.** Decide where user documentation
  lives: in-repo `docs/` published via a static site, a GitHub wiki, or
  generated command reference plus a handful of guides. Criteria: versioning
  with the binary (a wiki does not version, which matters once `stats_version`
  exists), discoverability, offline availability, contribution friction, and
  whether the command reference can be generated from the argument parser so it
  cannot drift. Record the decision.
- **(b) Write it.** README stays the front door for the whole project —
  what Colosseum is, the GUI, the CLI, install, links. User documentation covers
  the CLI in depth: quickstart, command reference, run-file and tune-file
  reference, worked examples per command, a "how to trust a result" page drawn
  from S3 Tier C, and a compatibility page (what the tool needs from a UCI
  engine, and what it does with non-conforming ones). Note that the licence
  applies to the tool and not to engines it runs as separate processes.
- **(c) Ship.** Per Phase 0(b)'s release model; all supported platforms;
  smoke-test the exact published artifacts (`--version`, `--help`,
  `engine check` against the shipped stub, one stub match).
- **(d) Acceptance.** A **third-party engine pair the maintainers did not
  write** — any two public UCI engines — driven by someone following only the
  published documentation, completing a fixed match, an SPRT and a short SPSA.
  That is the test of "usable by anyone"; the validation engines cannot
  demonstrate it because their authors know too much. Additionally the two
  validation engines each run one real gate through the released artifact on at
  least two operating systems, agreeing with 8(a).
- **(e) Coverage acceptance.** Both validation engines delete every harness
  script covered by S5.13 and retain only the residual list, with no workflow
  lost. Any exception is recorded in S5.13 as a named gap with a decision —
  because "as little tooling as possible" is only real if the scripts actually
  go away.

---

## S9. Risks

| Risk | Mitigation |
|---|---|
| Loss of runner independence for users who adopt this for everything | Phase 8(a) parity against two external runners; recommend keeping a second runner for periodic cross-checks |
| macOS cannot enforce affinity | Advisory or unavailable, recorded per run; fail only when hard placement was explicitly requested |
| CLI churn destabilises the released GUI | Phase 0(b) release model decision; shared core covered by the existing suite |
| Name collision with an existing similar product | Phase 0(a) |
| Scope creep into engine-specific work | S4 out-of-scope list; consume finished UCI executables only |
| Our defaults read as mandates | S3 tiers; profiles; user docs name alternatives |
| Statistics change meaning silently over time | `stats_version` + changelog (5.8) |
| A derived constant is wrong and invisible | A5: assert written artifacts before play |

### Rejected, with reasoning

- **Engine fingerprint / bench-identity checks as a harness feature.** A
  node-count fingerprint proves two builds search identically, which SHA-256
  cannot: two different hashes may be behaviourally identical (a rebuild) or
  not, and the harness cannot tell. That is a real gap. It is nonetheless
  rejected: the command is engine-specific, not UCI, so the harness would either
  impose a convention or accept an arbitrary user-supplied command it cannot
  validate — and the check belongs to whoever builds the engine, who can run it
  far more cheaply than a match harness can. Users who want it run it themselves
  before invoking the tool. Revisit only if a UCI-standard mechanism appears.

---

## S10. Reference

| Path | What |
|---|---|
| `crates/colosseum-core/src/stats.rs` | existing SPRT/Elo/LOS — extend here for pentanomial |
| `crates/colosseum-engine/src/{scheduler,runner,openings,store}.rs` | driver, game execution, books, persistence |
| `tests/fixtures/` | vendored golden fixtures + the generator that produces them |
| `CLAUDE.md` | workspace conventions, including why engines are spawned per game |
