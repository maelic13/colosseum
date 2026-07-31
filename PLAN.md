# Colosseum — engine-development harness plan

`colosseum-cli` is a planned headless, cross-platform harness for
**developing** chess engines on one machine: SPRT gates, SPSA tuning, fixed matches, tournaments,
speed measurement, position suites, and the run-record machinery that makes
those numbers trustworthy. Its only contract with an engine is a UCI
executable: no repository manifest, custom build command or non-standard
benchmark command is required.

The desktop app answers *"who is stronger?"*. The CLI answers *"is this build
stronger than that build, and can I believe the answer?"* — a different
question with harsher requirements: reproducibility, explicit CPU placement,
byte-level identification of inputs, durable long runs, and results that mean
the same thing next month.

**It is a general tool.** Rarog and Basilisk are the first two engines used to
validate it, not its specification. Where a policy could reasonably differ
between projects, the tool ships a default with a stated reason and lets the
user change it — see S3.

**It is deliberately local.** Distributed workers, source checkout, compilation
and result coordination belong to systems such as testing farms. The CLI owns
the generic mechanics of a trustworthy experiment on one host; the engine
project owns its source, build, correctness and experiment policy.

**Document audiences.** This file and [`GUIDE.md`](GUIDE.md) are the
maintainer-facing pair: specifications, success criteria, evidence, forward
plan. `README.md` is user-facing and covers the whole project — the Colosseum
GUI and CLI — at an introductory level. The **user documentation**
(placement decided in Phase 9) is also user-facing and carries CLI detail:
command reference, worked examples, and how to trust a result. Neither
user-facing surface may carry phase numbers, internal naming or method
argumentation.

---

## S1. Current state

**The CLI is not built yet. This document is step 0 for that work.**

**Colosseum is the implementation identity for the whole product.** The desktop
product and executable are **Colosseum** / `colosseum`; its Cargo package is
`colosseum-gui`. The independent CLI product, Cargo package and executable are
**Colosseum CLI** / `colosseum-cli`. Shared packages keep their coherent
`colosseum-*` names. Phase 0.6 established real search and supportability risks,
and its CLI-only **UCI Rig** proposal was rejected. ADR-0008 accepts those risks
for implementation and defers an optional whole-product reconsideration to
Phase 9.0, when the implemented product can be judged as a whole. The
[naming research](docs/architecture/naming-decision.md) remains evidence, not a
requirement to rename.

What exists is the reason the plan can reuse rather than rewrite:
`colosseum-core`, `colosseum-uci` and `colosseum-engine` are already headless
(no `egui` dependency anywhere in them), already cross-platform, already
released for Windows/Linux/macOS including arm64, and the workspace carries
more than 160 required tests.

They are not yet the final CLI architecture. The shared model still contains
GUI engine-library metadata and rating-writeback policy, while
`colosseum-engine` exposes GUI application directories and configuration.
Phase 0 therefore begins with a current-state dependency audit and a bounded
Clean Architecture refactor. The game runner, UCI implementation and statistics
are reused; ownership and dependency direction are corrected before new
workflows are built.

Validation engines: **Rarog** (Rust) and **Basilisk** (C++), chosen because they
are available, actively developed, and differ in language and build system.
Any two UCI engines would serve; nothing in the design depends on these.

---

## S2. Why this lives in this repository

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
- **A7. Statistical conclusions never claim more than their design supports.**
  Fixed-N results report their interval and achieved resolution; equivalence
  requires interval containment. SPRT results report their hypotheses, error
  rates, LLR, bounds and terminal sample. A capped SPRT is inconclusive.
- **A8. Every sequential run has a finite pair cap**, supplied explicitly or by
  a documented default. Reaching it is an inconclusive result, never an
  implicit H0.
- **A9. The clock accounting model is explicit, versioned and recorded**
  (S5.4a). Two harnesses that charge time differently produce different Elo for
  the same engines and neither is wrong; the number is only interpretable
  alongside the model that produced it.
- **A10. One master seed governs every random choice**, and the run record
  carries it (S5.0). A run that cannot be reproduced from its own record is not
  a measurement.

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

For a project house style, commit a run file beside the engine source. This is
portable, reviewable and does not depend on hidden per-user state. Resolution
order:

```text
built-in defaults  <  run file (with its inherited chain)  <  command line
```

**Run files compose.** A run file may `extend` another by relative path, so a
project's shared conditions live in one file and each workflow — gate, long-time
confirmation, calibration, tune, tournament, speed — overrides only what differs.
Without this, a project with six workflows keeps six copies of the same ten
settings and they drift apart, which is the failure mode this plan works hardest
to prevent everywhere else.

Composition has one portable definition:

- Each file has at most one `extend`. Its path is relative to the file that
  declares it, not the process working directory. Parent files are resolved
  first; canonical file identities are used for cycle detection; the maximum
  chain depth is 16.
- Tables merge recursively. A child scalar replaces its parent value and a child
  array replaces the whole parent array; arrays are never merged by position.
- A child may clear inherited optional values with `unset`, an array of RFC 6901
  JSON Pointers applied after the parent is resolved and before the child is
  overlaid. An invalid pointer is an error naming the declaring file and pointer.
  `extend` and `unset` are control keys and do not appear in the result.
- A path value declared in a run file is resolved relative to that declaring
  file; a CLI path is resolved relative to the invocation directory. This rule
  survives inheritance, so moving the leaf file cannot reinterpret a parent's
  engine or book path.
- The final value is normalized to the documented schema and serialized as
  canonical JSON (stable key order and canonical units). This **fully resolved**
  value is what gets hashed, recorded and compared on resume.

### Tier C — Recommended. Documentation only, zero code impact.

Guidance the tool never enforces, published in the user documentation because it
is what separates a number from a result.

- **C1. A sequential test is the verdict; everything else is a diagnostic.**
  Static-evaluation losses, node counts, search depth and NPS all correlate
  imperfectly with strength; several have moved the wrong way while strength
  improved, and vice versa.
- **C2. A null is not proof of no effect.** Before a fixed-N test, minimum
  detectable effect depends on significance, desired power and an assumed
  outcome distribution. Afterwards, report the achieved interval. An SPRT H0
  means only that the declared H0 boundary was reached under the declared model;
  it is not a universal "no effect larger than X" statement.
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

Phase 0 records the current and target architecture before implementation. The
target follows the **Clean Architecture dependency rule**: source dependencies
point inward; policy does not depend on frameworks, storage, operating systems,
the CLI or the GUI.

| Layer | Responsibility | Allowed dependencies |
|---|---|---|
| Domain/entities | Scores, pair outcomes, statistical models, schedules, run-state invariants and opaque identity values | Side-effect-free value/math/serialization libraries only; no I/O, OS, clocks or entropy sources |
| Application/use cases | Match, SPRT, calibration, SPSA, NPS, tournament, suite, status; ports for engines, persistence, time, affinity and progress | Domain |
| Interface adapters | CLI parsing/config resolution, GUI mapping, UCI adapter, SQLite/run-directory adapter, PGN/external-log adapters | Application + domain |
| Frameworks/drivers | Tokio/processes, filesystem, `rusqlite`, OS topology/affinity, terminal and GUI frameworks | Adapters |

The target package map is chosen in Phase 0 after the dependency audit. A
separate application crate is the default design because both CLI and GUI may
invoke use cases; keeping workflows in the CLI would make the command-line
adapter the owner of application policy. Phase 0 may choose another package
layout only if it enforces the same dependency direction.

```
colosseum-core          domain/entities          ← pure statistics and invariants
colosseum-application   use cases + ports        ← new by default; no OS/UI/storage
colosseum-uci           UCI driven adapter       ← engine sessions/process protocol
colosseum-engine        infrastructure adapters  ← runner, store, topology, affinity
colosseum-cli           command-line adapter     ← parse, compose, present
colosseum-gui           desktop adapter          ← map GUI library entries to use cases
```

The roles, edges and `colosseum-*` spellings above are binding implementation
names. Architecture must not add a generic brand-token service, neutral aliases
or other indirection solely to ease a hypothetical one-time rename. Product
display names, executable/application paths and packaging metadata still belong
to the outer adapters and composition roots; that responsibility placement is
what keeps a possible Phase 9.0 rename bounded.

**Binding architecture rules**

- A runtime `EngineLaunchSpec` contains only path, arguments, working directory,
  environment, display label, effective UCI options and allocated CPUs. It has
  no logo, library rating, arbitrary metadata or GUI persistence fields.
- Saved GUI engine-library data (`EngineConfig`/`EngineMeta` today),
  `EngineLibrary`, `AppConfig`, `AppDirs` and rating writeback remain in the GUI
  adapter. They are mapped to runtime specifications at the boundary.
- Application use cases receive ports such as `EngineSessionFactory`,
  `RunRepository`, `ArtifactSink`, `CpuPlacement`, `Clock`, `IdGenerator`,
  `MasterSeedSource` and `ProgressSink`; they do not open global paths, obtain
  entropy, generate process-global identities or select concrete databases.
- Framework types (`rusqlite::Connection`, Tokio handles, GUI types, OS handles)
  do not cross into the domain.
- Paths and artifact sinks are injected. No process-wide mutable/global output
  directory is part of the application contract.
- CLI and GUI are composition roots. Neither depends on the other.

**Independence contract**

- The CLI reads no GUI engine library, configuration or application-data path.
- Every CLI run is self-contained in its selected run directory (S5.11).
- `cargo tree -p colosseum-cli` contains no GUI/windowing dependencies.
- The published CLI starts and completes `self-test` on a headless host.
- CLI and GUI have separate versions, tags, artifacts and release notes even if
  they stay in one repository.
- Changes to shared layers run both CLI and GUI test suites.

### Required end state

At completion, the repository contains reviewed current/target architecture and
ADRs; inward-only shared layers; a still-working independently released GUI; and
an independently versioned/packageable CLI with this public capability surface:

| Need | CLI surface |
|---|---|
| UCI/protocol diagnosis | `engine inspect`, `engine check`, `self-test` |
| Host diagnosis | `capabilities`, common `status` |
| Strength experiments | `match`, pair-atomic capped `sprt`, optional `calibrate` |
| Parameter tuning | `spsa`, `spsa plan`, `spsa status`, `sprt --apply` |
| Performance | `nps` A/B and thread-scaling sweep |
| Multi-engine comparison | `tournament` round-robin/gauntlet |
| Experiment design/replay | `stats plan fixed|sprt`, `stats` |
| Input preparation | `book slice|hash|stats|verify`, `suite` |

The published CLI archive needs no GUI, separately installed language runtime,
engine manifest, bundled book, external script or writable installation
directory. The validation projects retain only declarative policy/CI glue plus
engine-specific responsibilities from S5.14.

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
logic, distributed execution, training-data extraction/filtering/labelling and
neural-net management. The CLI consumes finished UCI executables. Correctness
suites tied to a custom move generator or search command stay with the engine.
Non-chess variants are out of scope; Chess960 is a Phase-8 decision.

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
  handshake, `isready`, validation of a requested value against the advertised
  option schema, option acceptance followed by `readyok`, a bounded search
  returning a legal `bestmove`, `stop` honoured promptly, clean shutdown, and
  behaviour on `ucinewgame`. UCI has no option read-back, so this is explicitly
  not called a round trip. Exit code reflects the outcome.
- Run file and CLI arguments resolve per S3 Tier B, including `extend`
  inheritance, `unset`, path origins, merge rules and the depth limit. Cycles,
  excessive depth and unreadable parents are errors naming the file chain.
- **One master seed, many named sub-streams.** A single `--seed` (generated and
  recorded when not supplied) deterministically derives an independent stream per
  consumer — opening order, SPSA perturbations, bootstrap resampling, position
  order, warm-up scheduling — by a documented derivation from the master seed and
  the stream's **name**, never by sequential draws from one shared generator.
  The names, the derivation and the generator algorithm are part of
  `stats_version` and are recorded.
  Deriving by name rather than by draw order is what makes the streams
  independent: adding a new random consumer, or changing how many values an
  existing one takes, cannot shift any other stream — so a later feature cannot
  silently change what an old seed reproduces.
  The version-1 contract is exact: the displayed master seed is an unsigned
  64-bit integer; a stream seed is
  `SHA-256("colosseum-rng-v1\0" || master-seed-u64-LE || stream-name-UTF-8)`;
  those 32 bytes seed ChaCha12 at its initial stream position. Stream names are
  stable ASCII identifiers. Shuffle, bounded-integer, Rademacher and bootstrap
  sampling algorithms are specified rather than delegated to dependency helper
  APIs, and golden vectors pin derivation and the first samples. A future change
  requires a `stats_version` change; resume keeps the stored version.
- `--dry-run` prints the fully resolved configuration and the exact engine
  invocations without playing a game.
- In machine-readable mode stdout contains one documented JSON value only;
  progress and diagnostics go to stderr.
- `self-test` launches an internal deterministic UCI stub mode from the same
  executable and checks process, protocol, persistence and one short match.
- Engine processes run in an owned OS containment mechanism where available
  (process group/job object). Normal shutdown is bounded and escalates from UCI
  `quit` to termination; cancellation and harness failure leave no owned engine
  or descendant running.
- Stdout and stderr are drained concurrently. Protocol lines and in-memory
  queues have documented finite limits; traffic is streamed to artifacts
  outside the clock-critical path. An over-limit protocol line is an
  engine-attributable protocol fault. Queue saturation, artifact write failure
  or inability to drain/contain a process is an infrastructure failure and is
  never converted into a game result.

**Success criteria**

- Two arbitrary UCI executables can be inspected and compliance-checked using
  paths and CLI arguments only.
- The same run launched from a run file plus overrides resolves to
  byte-identical JSON as the equivalent all-CLI invocation.
- An `extend` chain resolves to byte-identical JSON as the equivalent flattened
  single file; recursive-table/scalar/array replacement, `unset`, per-file path
  origins and maximum depth have fixtures; a cycle is rejected with the chain
  named.
- The same master seed and configuration reproduce every sub-stream exactly, on
  every platform. Adding a consumer of a *new* stream leaves all existing streams
  bit-identical — asserted by a test, because this is the property that makes an
  old seed still mean something after the tool gains features.
- Adding a new conforming engine requires no file in the engine's repository and
  no Rust code.
- Flooding either engine pipe cannot deadlock or grow memory without bound; a
  stub that ignores `quit` or spawns a descendant is fully reaped on every
  supported platform; injected artifact-write failure invalidates rather than
  scores the game.

### 5.1 Pentanomial statistics and normalized Elo — `colosseum-core`

**Why first:** everything downstream reports through it, and the existing
`sprt()` is trinomial over W/D/L.

**Requirements**

- Pairs, not games, are the unit: each opening played from both colours yields a
  pair score in {0, 0.5, 1, 1.5, 2} → the pentanomial vector.
- Pentanomial variance; normalized Elo; logistic Elo with error bars; LOS; draw
  ratio; pairs ratio; WL/DD ratio.
  The definitions are exact. For `N` complete pairs, let `n_i` be the five bin
  counts and `x_i = [0, 0.25, 0.5, 0.75, 1]` the pair-average game scores:
  `mu = Σ(n_i*x_i)/N`, population variance
  `v = Σ(n_i*(x_i-mu)^2)/N`, and `SE = sqrt(v/N)`. Logistic Elo is
  `400*log10(mu/(1-mu))`; transform `mu ± z*SE` for its interval. Normalized
  Elo is `(mu-0.5)*800/(ln(10)*sqrt(2*v))`; its interval uses the same fixed
  empirical `v`. LOS is `Φ((mu-0.5)/SE)`. Draw ratio is individual draws in
  complete pairs divided by `2N`; pairs ratio is pairs above one point divided
  by pairs below one point; WL/DD is one-win/one-loss pairs divided by
  draw/draw pairs. Retain the WL-vs-DD split behind the central pentanomial bin.
  A zero ratio denominator is undefined, never infinity. These diagnostic
  ratios are optional observations; all statistical calculations instead return
  `Result<_, StatisticsError>`, naming invalid scalar inputs, invalid
  probabilities/hypotheses, insufficient samples, zero variance, unavailable
  logistic intervals and failed constrained likelihood solves. No successful
  calculation may contain `NaN` or infinity.
- SPRT over both the pentanomial/normalized and logistic models, selectable,
  reporting LLR and both bounds with H0/H1/continue — and always naming the
  model in force (A6). Both are generalized multinomial likelihood-ratio tests
  over `x_i = [0, 0.25, 0.5, 0.75, 1]`. Logistic hypotheses constrain
  `E[x] = 1/(1+10^(-elo/400))`; normalized hypotheses constrain
  `(E[x]-0.5)/sqrt(Var[x]) = nElo*sqrt(2)/(800/ln(10))`. For each hypothesis,
  maximize the multinomial likelihood subject to its constraint and compute
  `LLR = Σ n_i*ln(p_i,H1/p_i,H0)`. Match maintained Fishtest support handling:
  replace an empty bin by `0.001` only while solving the constrained MLE. The
  displayed pair count remains the real count, and a genuinely degenerate real
  sample is still an error rather than being legitimized by that prior. Wald
  bounds are exactly `ln(beta/(1-alpha))` and `ln((1-beta)/alpha)`.
- Fixed-N design and achieved-resolution calculations with explicit
  significance, power and assumed pair distribution; never infer an MDE from
  game count alone. The core difference-test planner assumes the pair mean is
  normally distributed with known assumed variance `v`. For target score shift
  `delta`, required pairs are
  `ceil(v*((z_critical+z_power)/delta)^2)`, with
  `z_critical=z_(1-alpha)` for a declared one-sided test or
  `z_(1-alpha/2)` for two-sided, and `z_power=z_(power)`. Logistic target Elo is
  converted by `delta=L(target)-0.5`; normalized target Elo by
  `delta=nElo*sqrt(2v)/(800/ln(10))`. Report the model, tails, alpha, power,
  assumed five-bin probabilities/variance, converted shift and quantiles with
  the rounded pair count. This is a planning approximation, not a stopping
  guarantee. Equivalence is a distinct TOST objective requiring a margin and
  assumed true effect; Phase 6.6 composes it explicitly rather than reusing the
  difference formula. Post-run achieved resolution is the empirical two-sided
  `(1-alpha)` interval in the selected model; report both endpoints and use the
  larger asymmetric Elo error as conservative resolution. It is not post-hoc
  power, a back-fitted MDE, or an SPRT verdict.
- An unpaired fallback for imported data and tournaments, clearly labelled.
  An incomplete colour pair is never admitted to a pentanomial SPRT.

**Success criteria**

- **Golden-file parity** follows the oracle matrix in S6.2. Each field is
  compared only with an implementation that exposes the same model; no external
  runner is treated as an oracle for statistics it does not implement.
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
- Detect and respect the CPUs available to the current process, including Linux
  cpusets/cgroups and Windows processor groups; never allocate from the machine
  total when the process is restricted.
- Allocate the configured **cores-per-engine** to each game slot — not one core
  per game. This allocation is independent of whichever UCI option controls the
  engine's internal worker count.
- On hybrid systems, keep both A/B slots on the same core class. Record NUMA
  node and core class; avoid cross-node placement by default and make any
  unavoidable asymmetry visible.
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
- Use the same time control, opening policy, adjudication, concurrency and CPU
  placement intended for the real experiment. A calibration is evidence about
  that machine and configuration, not a permanent certificate.
- `PASS` iff the full confidence interval lies inside the tolerance.
  `FAIL` iff the full interval lies above `+tolerance` or below `-tolerance`.
  Every overlapping case is `INCONCLUSIVE`; no point estimate alone decides.
- Any timeout, crash, disconnect or illegal move marks the run invalid.

**Success criteria:** hash mismatch rejected; configurable values round-trip
through persistence and resume; PASS/FAIL/inconclusive/invalid each have
deterministic tests.

### 5.4 Fixed match and SPRT — `colosseum-cli match|sprt`

**Requirements**

- `match` is fixed-N with no sequential stopping. `sprt` accepts explicit
  `elo0`/`elo1`/`alpha`/`beta` and model; `gainer` and `simplify` are named
  convenience bundles, not hard-coded semantics.
- `sprt` has an explicit finite `max-pairs`. Reaching it without a boundary is
  `INCONCLUSIVE`.
- A colour-reversed opening pair is the atomic scheduling, persistence and
  statistical unit. LLR and verdict are evaluated only after both games finish.
  Pair IDs enter the official sample in deterministic schedule order, never
  completion order, so concurrency cannot change the terminal sample.
- After a boundary is reached, schedule no new pairs. Finish the mate of any
  half-played pair. Store other already-finished work as post-terminal evidence
  but exclude it from the official terminal sample.
- Two engines by path, per-side arguments, working directory, UCI options and
  allocated cores. The same binary may be tested against itself or against
  itself with different options.
- **Time controls, per side independently:** movetime, sudden death, base +
  increment, fixed nodes, fixed depth — plus a configurable time margin so
  scheduler jitter is not counted as a loss on time. Asymmetric controls are
  supported (odds matches, "same engine at double time").
- **Clock accounting per S5.4a**, explicit and recorded.
- **Adjudication:** draw, resign and max-moves each individually configurable and
  each individually **disableable**. Arbitrary engine tablebase UCI options may
  be forwarded; harness-side tablebase adjudication is deferred to Phase 8
  because it requires a new probing dependency and is not necessary for a
  trustworthy SPRT.
- **Time-loss accounting is first class**: losses on time, crashes, disconnects
  and illegal moves are counted per engine, printed in every report block, and
  stored in the run record. `--max-time-losses N` flags or aborts the run.
  A test whose engines forfeit is not a valid test, and the user must not have
  to go looking for that.
- **Concurrency** is explicit: number of parallel games, its interaction with
  cores-per-engine and headroom. Refuse a CPU request that cannot be placed.
  Report `concurrency × 2 × hash` only as a hash-memory lower bound; warn about
  available memory, but refuse on memory only when the user supplies an
  explicit trusted per-engine budget or hard cap.
- **Book is optional.** Without one, every game starts from the initial position
  and the tool warns that opening diversity is absent. With one: order
  (sequential or seeded random), start index, ply depth, and **reuse detection**
  — the fraction of openings played more than once is reported, because reuse
  narrows error bars misleadingly.
- **Failure policy separates cause.** An engine-attributable crash, timeout,
  disconnect or illegal move is a forfeit and anomaly; statistical commands are
  invalid once their configurable engine-fault threshold is exceeded (default
  zero). An infrastructure/harness failure is never scored: pause or invalidate
  the run. Retry is allowed only for a failure proved to occur before play or
  independently of either arm. Statistical runs never silently discard a game.
  Exploratory tournaments may opt into a recorded non-strict policy.
- Live report block on a configurable interval; full log to disk; PGN out; run
  record; per-game engine output retained for failed games.
- **Machine-readable results**: JSON to a file or stdout, and **exit codes that
  distinguish H1 / H0 / inconclusive / invalid / error**, so the tool can be
  scripted and wired into CI. JSON stdout is never mixed with progress text.
- Resume per S5.11.

**Success criteria**

- Replaying the same ordered fixture outcomes reaches the same verdict and
  terminal pair as a compatible external oracle. Live runners are compared on
  shared conditions and outcome distributions, not required to produce the
  same clock-game sequence.
- A path-only invocation needs no configuration file; identical binaries with
  identical options are allowed for self-play.
- Every exit code is asserted by a test.
- A forfeit-injecting stub engine triggers the time-loss counters and the
  `--max-time-losses` policy.
- Killing mid-run and resuming yields the same final statistics as an
  uninterrupted deterministic-stub run at the same seed; an incomplete pair is
  resumed without entering the official sample early.

#### 5.4a Clock accounting — explicit, versioned, recorded

**Why this is specified rather than left to the implementation.** A harness
decides where the boundary of "the engine's time" lies, and reasonable
implementations differ: whether the clock starts when the position is sent or
when `go` is written, whether the harness's own write and read latency is
charged to the mover, and whether increment is credited before or after the
move's cost is deducted. Those choices are individually defensible and
collectively worth real Elo — engines expose a move-overhead option precisely to
compensate for a model they cannot observe. Two harnesses with different models
produce different Elo for the same pair of engines, and neither is wrong. The
number is therefore only interpretable next to the model, which is why A9 makes
recording it non-negotiable and why the parity gate (Phase 4B) would otherwise
report an unattributable divergence.

**The model**

- The mover's clock starts when the harness finishes writing `go` and stops when
  the harness finishes reading `bestmove`. Everything in between is charged to
  the mover, including its own search start-up and the harness's read latency.
- Time spent preparing and sending `position` before `go` is **not** charged.
- The clock is read from a monotonic source, never wall-clock time of day, so a
  system time change cannot alter a result.
- Increment is credited **after** the move's elapsed time is deducted. Let `R`
  be the remaining time before the move, `E` the charged elapsed time,
  `M` the configured margin and `I` the increment. If `E > R + M`, the mover
  forfeits before receiving increment. Otherwise the move is accepted and the
  new clock is `max(0, R - E) + I`. Equality at `R + M` is accepted, avoiding a
  rounding-dependent boundary. This makes the deduction/increment order and the
  margin interaction explicit.
- The **time margin is a forfeit tolerance only.** It never adds to the engine's
  budget and is never visible to the engine — it only prevents a marginal
  overrun from being scored as a loss. A margin that extended the budget would
  change how the engine plays, which is a different experiment.
- When pondering is disabled, no time is charged to a side that is not to move.
- Each run records the model identifier and version, the margin, monotonic-clock
  resolution and the charged-elapsed min / median / max. The harness cannot
  portably separate engine search time from pipe, scheduler and read latency
  inside the charged interval, so it does **not** report invented
  "harness-overhead" numbers. Separately measurable pre/post-I/O diagnostics may
  be recorded, but are labelled by the operation actually measured.

**Success criteria**

- A stub engine that sleeps a commanded duration is charged that duration within
  a stated tolerance, on every platform.
- A stub overrunning by less than the margin is not forfeited; one overrunning by
  more is, exact equality is accepted, and the forfeit is attributed to the
  correct side.
- A run whose system clock is changed mid-game produces an unchanged result.
- Increment ordering has fixtures below, at and above the exhaustion/margin
  boundaries.
- Clock model/version, margin, resolution and charged-elapsed summary are present
  for every completed clock-based run and are not mislabelled as engine or
  harness overhead.

### 5.5 SPSA — `colosseum-cli spsa` + core schedule

**Requirements**

- **Exact Fishtest-compatible variant.** For iteration `k = 0..N-1`, each knob
  `i` receives a seeded independent Rademacher perturbation
  `Δ[k,i] ∈ {-1,+1}`. With `alpha=0.601`, `gamma=0.102` and `A=0.1·N`:

  ```text
  c[k,i] = c0[i] / (k + 1)^gamma
  a[k,i] = a0[i] / (A + k + 1)^alpha
  r[k,i] = a[k,i] / c[k,i]^2
  ```

  Each knob declares `c_end[i]`; the run declares `r_end` and `N`.
  `a_end[i] = r_end × c_end[i]^2`, and `c0[i]`/`a0[i]` are back-solved so the
  final iteration has exactly those end values. Decay is per iteration, never
  per game.
- Keep the internal centre vector in floating point. The two sent UCI vectors
  are `round_half_away_from_zero(clamp(theta ± c×Δ))`; this tie rule is binding
  and cross-platform tested. Both arms play the same opening pairs with colours
  reversed. For the plus arm, `D = wins - losses` across the complete mini-match;
  draws contribute zero. Update:

  ```text
  theta[k+1,i] = clamp(theta[k,i] + c[k,i] × r[k,i] × D × Δ[k,i])
  ```

  Rounding is applied when values are sent or emitted, not to the stored centre
  after every update. The exact RNG algorithm, seed and draw order are part of
  `stats_version` and persisted state.
- The tune file selects numeric UCI options and supplies initial value, tuning
  bounds and `c_end` per knob, validated against the live UCI option schema.
- Defaults `N=5,000` and 32 games/iteration; configurable, not enforced minima.
- Persistent driver: no per-iteration relaunch; a supplied book is loaded once.
- Multi-session per S5.11.
- A mini-match is committed only when all of its scheduled colour pairs
  complete. An engine-attributable fault invalidates the iteration and tune;
  it is never converted into a gradient or selectively retried.
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
- **Closing the loop.** A tune must not end at "here is a vector". On
  completion, and on demand mid-run, emit the rounded mean of the final 10% of
  completed centre vectors (window configurable and frozen in the run record) as
  (a) a ready-to-paste `setoption` list, (b) JSON, and (c) a run file fragment.
  `colosseum-cli sprt --apply <result.json>` then gates the tuned values against
  the original vector **using the same executable and UCI options only** — no
  source edit, rebuild or engine-specific baking step. The artifact contains
  executable hash, original/tuned vectors, tune conditions, schema and schedule
  versions. A changed executable hash is refused unless explicitly overridden,
  and an override is prominent in the gate record.

**Success criteria**

- Schedule property tests: `c[N-1] == c_end`,
  `a[N-1] == r_end × c_end²`, and `r[N-1] == r_end` within a stated floating
  tolerance; arm swap negates the update; the same seed reproduces every
  perturbation and vector.
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

#### 5.5a SPSA sizing — `colosseum-cli spsa plan`

Offline, no games. Validate the exact schedule and report total iterations,
games and pairs; `c/a/r` trajectories; the first rounding-resolution hazard;
checkpoint/storage count; and a wall-clock range from user-supplied timing or a
short pilot sample. Show how cost and schedule change with the horizon.

An optional synthetic-objective simulation may report convergence bands only
when the user supplies that objective/noise model. It is labelled a model
simulation, never a prediction that a chess tune will converge: real convergence
also depends on unknown curvature, sensitivity, interactions and starting
distance.

**Success criteria:** schedule and cost arithmetic match hand-computed fixtures;
the wall-clock estimator covers a controlled stub run; a synthetic simulation
is reproducible by seed and clearly separated from factual schedule output.

#### 5.5b SPSA diagnostics — `colosseum-cli spsa status`

Read an atomic snapshot of a run directory without touching the running tune.
Report iteration, percent, ETA, per-knob current value and trajectory, plus a
thirds comparison of completed history normalised to the knob range.

These are explicitly **heuristics, not causal or convergence claims**. Flag
frequent contact with a bound, little net movement from the seed, recent
trajectory stability, and perturbation below the engine's rounding resolution.
Explain that each observation may result from the objective, noise, gain,
clipping or an unsuitable range; never automatically advise continue/abandon.

**Success criteria:** fixture diagnostics match hand calculations; a short run
reports insufficient history rather than inventing a trend; status against a
live atomically-updated run neither blocks nor mutates it.

### 5.6 Speed / NPS A/B — `colosseum-cli nps`

**Requirements**

- Drive an optional user-supplied position suite through standard bounded
  searches; use the initial position when omitted and warn that the workload is
  weaker.
- The authoritative sample is harness monotonic wall time from sending
  `go nodes` until `bestmove`, over a fixed-node workload. Reported `info nodes`
  verifies comparable work; engine-reported `time`/`nps` is diagnostic only
  because it is produced by the system under test.
- Define and record state policy: `cold` restarts the engine per measured
  repetition with startup excluded; `warm` keeps it alive and sends
  `ucinewgame`/`isready`. Hash clearing is used only when the engine advertises a
  suitable button and is never assumed. Position order, repetitions and warm-up
  are seeded and stored.
- Strict alternation, warm-up, arm-level **median** and **best-of**, bootstrap
  CI on the median.
- Accept one or more executables per arm; show per-executable medians so
  non-overlap is visible. Multiple builds are supported, never required.
- A self pair is recommended and optional; warn when a matching recorded self
  pair lies outside a configurable tolerance (default ±0.5%).
- Report per-round SD as a machine-noise indicator.
- **Scaling sweep:** measure across a list of engine search-thread counts (for
  example 1, 2, 4, 8, 16) using an explicitly selected or safely recognised UCI
  spin option. Allocate the same number of physical cores; pin the identical
  position sequence/search limits; declare fixed-total versus per-thread Hash;
  and report wall-time speedup and parallel efficiency relative to one thread.
  Store CPU class/NUMA placement and warn when symmetric placement is impossible.

**Success criteria**

- A self-pair result is reported without being a prerequisite.
- A synthetic left-skewed sample reproduces the known bias in a naive
  alternating-pair estimator and *not* in the shipped one.
- A fake engine that lies in `info nps` cannot change the authoritative result.
- Cold/warm modes, scaling efficiency and fixed/per-thread Hash policies match
  hand-computed fixtures.

### 5.7 Tournaments — `colosseum-cli tournament`

Expose both formats already supported by the shared core:

- round-robin: every engine against every other engine
- gauntlet: one or more seeds against an opponent ladder

Both provide joint ML ratings with error bars, optional anchor,
standings/crosstable CSV and resume per S5.11. A `gauntlet` alias may exist for
convenience, but it resolves to the same tournament use case rather than a
second implementation.

**Success criteria:** schedules and ratings match the GUI on stored data
(ratings ≤0.01 Elo); kill/resume with deterministic stubs produces identical
standings for both formats.

### 5.8 Run record

Generated JSON per run: both engines' canonical path, SHA-256, UCI identity,
arguments, working directory and effective options; harness version and build;
**`schema_version` and `stats_version`**; host summary (OS, CPU model, physical
and logical core counts, allowed CPU set, core class/NUMA where known); optional
book path and hash; **master seed and the named sub-streams derived from it**;
resolved affinity and capability mode; time control; **clock model identifier,
version, margin, monotonic-clock resolution and charged-elapsed summary**
(S5.4a); adjudication settings; concurrency; the resolved configuration hash
**and the `extend` chain
that produced it**; full command line; UTC start/end; official terminal sample,
outcome, statistics and anomaly counts.

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

Read a CLI run, a PGN, or a supported external result log and report the
same block used live. External formats and versions are explicitly listed; when
pair/opening identity is absent, fall back to labelled unpaired statistics
rather than guessing pairs.

**Authority order:** the structured run store/checkpoint is authoritative; PGN
is the portable game export; logs are forensic evidence; console output is
observational only. Every live number must be reproducible from the structured
run directory. PGN replay reproduces only information the PGN actually carries.

**Search telemetry from a PGN.** Where move annotations are present, report per
engine the coverage fraction and mean/median depth, elapsed time and implied
nodes per second. List the supported annotation syntaxes; preserve unknown
annotations; exclude pre-played opening moves; and report `unavailable` when
coverage is insufficient. Fixed-node telemetry is comparable only when node
accounting has compatible semantics—normally the same engine lineage.

**Experiment planning**

- `stats plan fixed` accepts target effect/equivalence margin, significance,
  desired power and an assumed pair distribution, then estimates required
  pairs and states every assumption.
- `stats plan sprt` accepts hypotheses/error rates plus an assumed true effect
  and pair distribution, then simulates an expected-length distribution. It is
  a planning model, not a stopping guarantee.
- Post-run output reports achieved intervals/resolution; it never back-fits a
  planning MDE and presents it as a fact.

**Success criteria:** every golden fixture replays through the command with the
same result as the library API; telemetry aggregates match hand-computed values
on fixture PGNs in every supported syntax; missing pair/telemetry information
produces a clear labelled fallback; planning calculations match analytic or
seeded-simulation fixtures.

### 5.11 Durable runs and status — one contract for every long command

Applies uniformly to `match`, `sprt`, `spsa`, `calibrate`, `tournament` and
long position suites. It is Tier A because the failure mode is silent data loss
or silent pooling of incomparable work.

- A run lives in a **run directory** with a predictable layout (state, log, PGN,
  run record, resolved config). Without `--dir`, create a unique directory under
  `./colosseum-runs/`; never write beside the installed executable.
- Resume occurs only when the user explicitly selects an existing `--dir`.
  Starting over in that directory requires an explicit flag, and that flag
  **archives rather than deletes**. A generated default directory can never
  accidentally resume an unrelated run.
- **A resume refuses** if the stored configuration differs materially from the
  requested one — engine paths or hashes, time control, book or its hash,
  adjudication, bounds, model, schedule. Pooling games from different conditions
  is the failure this prevents.
- **Logs append; they never truncate.**
- The **stored** horizon and schedule win over command-line arguments on a
  resume, and the tool says so on screen rather than silently ignoring the flag.
- State is written at least every K units with a checksum and **two-generation
  atomic checkpoints** (write, flush, rename, retain previous), so a hard kill
  costs at most K units and a damaged current checkpoint can fall back to a
  verified previous generation.
- Interrupting is a supported operation, not an accident: a clean stop and a
  hard kill must both be recoverable.
- `colosseum-cli status <run-directory>` reads an atomic snapshot without
  mutation and reports command type/state, owning-process liveness where
  detectable, last durable checkpoint, completed/running/pending/failed units,
  current official statistics, anomalies and ETA. Command-specific status (for
  example SPSA trajectories) extends this common view.

**Success criteria:** one shared test suite runs against every long command —
kill at a random point, resume, and reach statistics identical to an
uninterrupted run; a mismatched-config resume is refused with a precise message;
a truncation attempt on the log fails; a state file corrupted mid-write is
detected and the previous state used; status against a live run is non-blocking,
read-only and consistent with the last committed checkpoint.

---

### 5.12 Position suites — `colosseum-cli suite`

Run standard UCI searches over EPD/FEN position sets at fixed time, nodes or
depth. Support EPD `bm`/`am` expectations, per-position outcome and latency,
aggregate pass rate, and comparison with a compatible previous suite run.
Unknown EPD operations are preserved and ignored with a report; no custom
engine `perft`, `bench` or diagnostic command is inferred.

**Success criteria:** legal `bm`/`am`, multiple accepted moves, no-solution and
malformed fixtures have deterministic outcomes; a baseline comparison refuses
incompatible position-set/search hashes; long suites resume without duplicating
positions.

### 5.13 Data generation — deferred as a separate command

Fixed-node/depth self-play written as PGN is already expressible as a normal
match using the same scheduler, placement and durability. Document that recipe
and do not create a second `datagen` workflow in v1.

Revisit in Phase 8 only if concrete engine-independent requirements exceed
`match`: corpus sharding, deterministic game IDs, deduplication, controlled
randomisation or effectively unbounded horizons. Training-format extraction,
position filtering, labelling and trainer-specific records remain out of scope.

### 5.14 Coverage target — generic machinery the CLI replaces

The target is not literally “no scripts”: an engine project may keep
declarative run/tune files and thin CI invocations that select project policy.
It must not have to reimplement generic scheduling, statistics, tuning,
affinity, recovery or result analysis. This is audited against a mature harness
(~3,400 lines of PowerShell plus Python across two engines). An unclaimed piece
of **generic mechanism** is a gap; project-specific policy is not.

| Existing tool | Replaced by |
|---|---|
| SPRT driver, null calibration | 5.3, 5.4 |
| SPSA driver | 5.5 |
| SPSA config audit | 5.5 audit classes |
| SPSA sizing model | 5.5a |
| "is my tune converging?" log analysis | 5.5b |
| Round-robin / gauntlet driver | 5.7 |
| NPS A/B, multi-build pooling | 5.6 |
| Thread-scaling sweep | 5.6 scaling sweep |
| UCI probe / handshake helper | 5.0 `engine inspect` / `check` |
| Result recomputation from PGN | 5.10 |
| Per-engine depth/time from PGN | 5.10 telemetry |
| Affinity, topology, concurrency, seeds, hashing | 5.2, 5.4, 5.8, 5.9 |
| Console log filtering, tee and liveness checks | CLI logging + 5.10 + 5.11 status |
| Book handling and slicing | 5.9 |
| Generic EPD best-move suites | 5.12 |
| PGN self-play corpus generation | 5.4 recipe; no duplicate command |
| Runner/tuner fetching and patching | evaporates — nothing to vendor or patch |

**Residual, and deliberately so — these stay with the engine, forever.** They
depend on the engine's source, build system or internals, so no general harness
can own them:

- building engines, PGO/instruction-set flavours, artifact naming
- choosing comparable compilers, flags and build conditions for A/B binaries
- profiling (sampling profilers, platform trace tooling)
- correctness suites tied to the engine's own move generator or search
- engine-specific diagnostic counters and their readouts
- declarative CLI run files and thin CI/release-policy invocations
- non-UCI evaluation tuning and training-data extraction, labelling/filtering
- baking tuned values into source, if the project prefers that to UCI options

**Success criterion:** after each relevant phase, migrate and compare the
corresponding real workflow. By Phase 9 both validation engines have archived
every replaced generic implementation and retain only declarative/thin policy
glue and the residual list, with no workflow lost. Any exception is recorded
here as either a named generic gap or an intentional project-specific policy.

## S6. Testing requirements — binding

A harness bug is worse than an engine bug, because it is invisible in exactly
the measurement meant to catch it.

1. **Every pure function is unit-tested**, including degenerate inputs. No
   statistical function returns `NaN`/`Inf` without a typed error.
2. **Golden-file parity against vendored fixtures, with an oracle matrix.**
   Fixtures come from public UCI engine versions run through `fastchess` and
   `cutechess-cli`. Commit engine/tool names and versions, executable or source
   hashes, licences/provenance, exact commands and raw logs. Do not anonymise
   away reproducibility. Compare each field only where the external tool
   implements the same model: analytic fixtures are authoritative for every
   formula; fastchess covers compatible pentanomial/normalized outputs; both
   external tools cover their shared logistic/trinomial surface and scheduling.
   The vendored corpus is `tests/fixtures/statistics/`; its per-field oracle
   matrix is binding, so unsupported fields are excluded rather than guessed.
   A documented generator extends the corpus from any engine pair. A
   disagreement is recorded and root-caused, never averaged away.
3. **⛔ The required test suite is hermetic.** CI rejects any required test that
   reads a path outside the repository. Explicitly opt-in real-engine smoke
   tests may consume an environment-provided executable, but are excluded from
   the required suite, have no hard-coded machine path and cannot establish a
   supported-platform or release pass. This keeps every clone independently
   buildable while preserving useful local interoperability checks.
4. **Analytic fixtures** with hand-derived expected values, so correctness does
   not rest on any external tool being right.
5. **Property-based tests** for statistics and the SPSA schedule.
6. **Integration tests with a cross-platform stub engine.** The CLI exposes
   `self-test` and internally spawns its own hidden deterministic UCI-stub mode.
   This exercises the exact published artifact without shipping a second public
   executable. Test-only fault modes replace current Windows-only shell stubs.
7. **Fault injection:** crash at handshake / mid-search / on quit; timeout;
   illegal move; never answering `isready`; garbage or an over-limit line on
   stdout; stdout/stderr flood; ignored `quit`; descendant process; artifact
   write failure. Each must produce a specific tested outcome, remain bounded,
   leave no owned process behind and never be silently absorbed into a result.
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
13. **Architecture tests enforce S4:** dependency inspection rejects GUI
    dependencies in the CLI and outward framework dependencies in inner crates;
    application tests use in-memory/fake ports; no CLI integration test reads
    GUI application directories.
14. **Published-artifact smoke tests** run `--version`, `--help`, `self-test`
    and one deterministic JSON-mode workflow from the packaged CLI, headlessly.

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

### Model routing for numbered steps

This table is the default model assignment for each numbered `GUIDE.md` step.
It is a task-risk classification, not part of the product contract:

- **Terra High** — the design is settled and the work is bounded by explicit
  invariants, fixtures and an exit criterion.
- **Sol High** — the step creates architecture or policy, combines several
  failure domains, or owns mathematical, concurrency, durability or
  cross-platform correctness.

Use the assigned model when starting a step. If Terra discovers a material
design choice not resolved by this plan, stop that step and continue it with
Sol High rather than improvising the missing contract. If Sol High cannot
resolve a genuinely frontier problem after inspecting the code and evidence,
raising its effort is an explicit exception, not the default. `Ultra` is not a
substitute for `Max`: use it only when the work can be divided into independent
subtasks. Model names are workflow metadata and may be revised as the available
lineup changes; tests, fixtures and phase exits remain the authority.

Every identifier is covered below; ranges are inclusive.

| Phase | Terra High | Sol High |
|---|---|---|
| 0 | 0.1, 0.6 | 0.2–0.5, 0.7–0.8 |
| 1 | 1.1, 1.5–1.8 | 1.2–1.4, 1.9 |
| 2 | 2.2–2.3, 2.5–2.6, 2.9 | 2.1, 2.4, 2.4a, 2.7–2.8, 2.10 |
| 3 | 3.2, 3.4, 3.6–3.7 | 3.1, 3.3, 3.5, 3.8 |
| 4A | 4A.1, 4A.3, 4A.6–4A.7 | 4A.2, 4A.2a, 4A.4–4A.5, 4A.8 |
| 4B | 4B.1, 4B.4 | 4B.2–4B.3, 4B.5–4B.6 |
| 4C | 4C.1–4C.2 | 4C.3 |
| 5 | 5.3–5.4, 5.6, 5.8–5.9 | 5.1–5.2, 5.5, 5.7, 5.10 |
| 6 | 6.4–6.5, 6.7–6.8 | 6.1–6.3, 6.6, 6.9 |
| 7 | 7.1 | 7.2–7.3 |
| 8 | — | 8.1–8.3 |
| 9 | 9.2–9.3, 9.6 | 9.0–9.1, 9.4–9.5, 9.7 |

### Phase 0 — Current-state analysis and target architecture

No CLI implementation begins until the boundary it will depend on is understood
and recorded.

**Progress:** Steps 0.1 through 0.8 are complete. The
[`dependency inventory`](docs/architecture/dependency-inventory.md) records all
workspace packages, internal Cargo edges, source modules, principal source
imports, test targets and current build/release targets. The
[`current-state analysis`](docs/architecture/current-state.md) classifies
responsibilities, public boundaries, side effects, globals, error/cancellation
behavior, tests and release coupling; findings CS-01 through CS-12 account for
every S4 gap. The
[`target architecture`](docs/architecture/target-architecture.md) assigns every
current module and consequential public boundary, defines the application
use cases and inward-facing ports, separates GUI library data from
`EngineLaunchSpec`, and specifies composition, durability, failure/cancellation
flow and the smallest safe migration. The accepted
[`architecture decisions`](docs/architecture/adr/README.md) bind the package
graph, minimal launch specification, runtime-neutral port and authoritative
commit boundary, GUI-library mapping and incremental migration. The
[`release architecture`](docs/architecture/release-architecture.md) and
[ADR-0006](docs/architecture/adr/0006-one-repository-independent-product-releases.md)
keep one repository while separating GUI/CLI versions, tags, notes, artifacts
and workflows, with required shared-layer CI. The
Phase 0.6 [`naming research`](docs/architecture/naming-decision.md) and rejected
[ADR-0007](docs/architecture/adr/0007-name-the-cli-uci-rig.md) preserve the
real collision evidence and rejected UCI Rig proposal. The
[`integrated review`](docs/architecture/phase-0-review.md) demonstrates complete
module ownership, consistent dependency/release responsibilities and executable
owners for every independence invariant. Accepted
[ADR-0008](docs/architecture/adr/0008-use-colosseum-through-implementation.md)
binds Colosseum, `colosseum-gui` and `colosseum-cli` as the coherent
implementation identity, rejects speculative rename indirection and moves an
optional whole-product naming review to Phase 9.0. The Phase-0 exit is passed.

- **(a) Current-state report.** Use `cargo metadata`, `cargo tree` and source
  inspection to write `docs/architecture/current-state.md`: crate/module
  dependency graph; ownership of domain, workflows, process I/O, persistence,
  paths and global state; framework dependencies; public types crossing crate
  boundaries; current GUI/CLI release coupling; and each violation of the S4
  dependency rule. Explicitly cover `EngineMeta`, `EngineLibrary`,
  `AppConfig`/`AppDirs`, `RatingWriteback`, incident output and the SQLite
  scheduler; `colosseum-core` UUID generation and branding/path policy; and the
  current hard-coded external-engine test fallbacks. Audit workspace-version
  inheritance, GUI-only build/release scripts and the absence or presence of a
  required cross-platform CI workflow rather than assuming release independence.
- **(b) Clean Architecture design.** Write
  `docs/architecture/target-architecture.md` with layer/package diagram, use
  cases, port contracts, runtime data types, composition roots, run-directory
  ownership, error/cancellation flow and a migration map from every current
  module. Record consequential choices as ADRs. Prefer the smallest refactor
  that enforces inward dependencies; do not rewrite working UCI/game logic.
- **(c) Independence and release design.** Define architecture tests and sketch
  CI/release pipelines. Default to one repository with independently versioned
  GUI and CLI packages, distinct tags/artifacts/release notes, and shared-layer
  regression tests. Split repositories only if the written analysis establishes
  a concrete advantage that outweighs cross-repository core coordination.
- **(d) Naming and migration.** Phase 0.6 records the genuine Colosseum search
  and spoken-support risks and the rejected CLI-only UCI Rig proposal. Phase 0.8
  accepts **Colosseum** / `colosseum` / `colosseum-gui` for the desktop product
  and **Colosseum CLI** / `colosseum-cli` for the independent CLI throughout
  implementation. Shared crates keep the `colosseum-*` stem. Do not weaken
  names or add a speculative rebranding framework: Clean Architecture keeps
  display, path, installer and packaging policy in outer owners. Phase 9.0 may
  retain this identity or deliberately rename the whole product after repeating
  exact web, same-domain, GitHub, package-channel and preliminary trademark
  checks and defining the complete one-time migration.
- **(e) Integrated review.** Review current/target architecture, ADRs and release
  design as one contract; demonstrate that every current module has a target
  owner, diagrams agree and each independence invariant has an executable test
  owner. Correct inconsistencies and record evidence before making the naming
  decision final.
- **EXIT — PASSED:** the integrated architecture review passes; ADR-0008 binds
  the implementation identity; dependency and release diagrams use it
  consistently; every module has a target owner and the independence contract
  in S4 has an executable test owner. Phase 1/2 code may start.

### Phase 1 — Pentanomial statistics and normalized Elo (`core`)
Spec 5.1 plus the fixture corpus (S6.2–S6.4). First because everything reports
through it and it needs no I/O or platform surface. **Exit:** analytic fixtures
pass; the per-field oracle matrix passes; the required suite is hermetic and
any opt-in real-engine smoke test is clearly excluded from release evidence.

**Accepted:** Phase 1.9 executes the hand-derived statistics, pentanomial and
trinomial SPRT, fixed-N planning/resolution and typed-error fixtures. It also
reconstructs W/D/L and complete colour pairs from both reviewed external PGNs,
while the machine-readable acceptance manifest records every compared field
and every reasoned exclusion. Required CI remains repository-only.

### Phase 2 — Architecture migration, CLI skeleton and durable foundation
Implement the Phase-0 migration needed by Specs 5.0 + 5.8 + 5.11: generic
runtime participant type, application ports/use cases, GUI mapping adapter, CLI
composition root and independently versioned CLI package. Add argument parsing,
run file resolution, inspect/check, dry-run, JSON/stdout contract, `self-test`,
run records, run directories, two-generation checkpoints and common status.
Move identity generation out of the domain, and replace hard-coded live-engine
test paths with explicitly opt-in environment-only smoke tests. Establish the
separate GUI/CLI version and changelog lanes and the required push/pull-request
shared-workspace CI baseline; Phase 9 owns final publication workflows.

**Implemented boundary baseline (2.1):** `colosseum-application` owns the
runtime participant/launch contract, typed application failures, use cases and
driven ports without runtime/framework dependencies. `colosseum-uci` implements
the session port, GUI detection now traverses the application use case, and the
GUI owns its product/config/path policy plus the explicit saved-library to
runtime mapper. Core identities accept injected values and no longer acquire
entropy. Architecture and fake-port tests enforce inward dependencies and
commit-before-publication.

**Implemented independent product baseline (2.2):** the headless
`colosseum-cli` package has its own `0.1.0` version, help/version contract and
transitive no-windowing architecture test. The GUI retains `1.0.2`; internal
packages use non-product versions and every package is explicitly
non-publishable. Product changelogs and `gui-v`/`cli-v` metadata validation are
separate. Required push/PR CI exposes `workflow_call`, runs Windows/Linux/macOS
debug and release suites, and builds the CLI artifact independently. The legacy
GUI publication workflow remains until Phase 9.4 replaces publication lanes.

**Implemented direct engine controls (2.3):** CLI driving adapters resolve a
bare executable or the optional `--label`, repeated `--engine-arg`, `--cwd`,
repeated `--env NAME=VALUE`, repeated `--option NAME=VALUE`, repeated
`--button NAME` and `--cores LIST` controls into the minimal application launch
spec. Names and arguments are preserved exactly; duplicate names and malformed,
descending, repeated or excessive CPU ranges are errors. Command-specific
composition in later steps reuses this parser rather than introducing an engine
descriptor.

**Implemented configuration resolver (2.4):** generic CLI adapter code applies
each inherited file's RFC 6901 `unset` to its resolved parent before recursively
merging that file, then applies command-line clearing/overrides. Canonical file
identities, a 16-file bound and chain-rich errors cover inheritance; every leaf
retains built-in/file/CLI origin. Command schemas enumerate their path pointers,
which are normalized relative to those origins before stable-key JSON is hashed
and written with an origin sidecar. Inherited/flattened and run-file/all-CLI
fixtures are byte- and hash-identical, including Windows path-alias handling.

**Exit:** two arbitrary UCI executables pass path-only workflows; run-file
inheritance/clearing/path origins resolve identically to equivalent all-CLI or
flattened input; seed golden vectors reproduce across platforms; durable and
status suites pass against the internal stub; identity generation is outside
the domain and hard-coded live-engine paths are gone; pipe floods remain bounded
and ignored-quit/descendant stubs are reaped; architecture tests prove no GUI
dependency or GUI-data access; GUI tests remain green; a headless CLI artifact
is produced independently; required CI exercises both product suites for shared
changes.

### Phase 3 — CPU topology and affinity
Spec 5.2, including the `capabilities` command. **Exit:** SMT, hybrid,
restricted-cpuset, processor-group, no-SMT and dual-socket fixtures pass;
residency tests pass where enforceable; platform capability reporting is
documented.

### Phase 4A — Fixed-match runner

Implement the non-sequential part of 5.4: fixed matches, per-side controls,
adjudication (without harness tablebase probing), strict failure classification,
concurrency/resource reporting, optional books, outputs and durability.
**Exit:** path-only/no-book and paired-book matches pass; fault injection never
silently scores infrastructure failures; JSON/exit contracts and resume pass;
the same deterministic schedule is replayable by pair ID.

### Phase 4B — Pair-atomic SPRT and runner parity

Add 5.1/5.4 SPRT orchestration: finite cap, deterministic pair commit order,
official terminal sample and post-terminal handling. Replay identical outcome
streams through compatible external statistics, then run controlled live parity
against fastchess and Cute Chess on their shared feature surface.
**Exit:** analytic/oracle verdict parity; concurrency cannot change the terminal
pair; capped/invalid/H0/H1 exits pass; live disagreements are root-caused before
SPSA builds on the runner.

### Phase 4C — Optional calibration

Implement 5.3 over the trusted runner. **Exit:** representative configuration
round-trips; byte mismatch is rejected; PASS/FAIL/INCONCLUSIVE/INVALID fixtures
and one real-machine smoke run behave as specified.

### Phase 5 — SPSA

Implement the exact 5.5 algorithm, loop closure, plan and status. **Exit:**
formula/RNG/rounding properties and every hard audit pass; recovery preserves
the exact stream; synthetic convergence smoke test passes; plan arithmetic and
diagnostics match fixtures; the tune result feeds `sprt --apply` unedited with
the original executable hash.

### Phase 6 — Speed, planning, replay, books and position suites

Specs 5.6 + 5.9 + 5.10 + 5.12. **Exit:** wall-clock/fixed-node and skew-bias
regressions pass; scaling sweep fixtures pass; book slicing is reproducible;
every golden fixture replays; fixed/SPRT planning matches fixtures; EPD suite
and baseline compatibility tests pass.

### Phase 7 — Tournaments

Spec 5.7. **Exit:** round-robin and gauntlet schedules/ratings match the GUI;
kill/resume is identical with deterministic stubs.

### Phase 8 — Parity against external runners, and remaining gaps

- **(a) Repeat the Phase-4 parity gate** against current supported external
  versions and the exact release candidate. Compare only the oracle matrix's
  shared fields and record every divergence.
- **(b) Remaining feature gaps.** Revisit what the external runners do that this
  does not, and decide per feature: adopt, decline with a reason, or defer.
  Candidates: Chess960, ponder under test conditions, harness-side Syzygy
  adjudication, additional tournament/output formats and whether datagen has
  gained concrete generic requirements beyond `match`. **The tie-breaker is
  whether a general engine developer needs it**, not whether the validation
  engines do.

### Phase 9 — Documentation and release
The deliverable is a tool any engine developer can pick up.

- **(0) Optional final naming review.** Reassess Colosseum only now that the
  complete GUI/CLI product can be judged. Either retain the identity and record
  that decision, or choose one replacement stem and perform the full one-time
  migration before user documentation and release. A rename covers the
  repository, Cargo packages/crates, binaries, tags, artifacts, release titles,
  installer/application IDs, config/data paths and compatibility, updater URLs
  and documentation. Repeat dated web, same-domain, GitHub, package-channel and
  preliminary trademark checks. This is a decision gate, not a mandatory
  rename, and does not justify neutral aliases or branding abstractions in the
  implementation.
- **(a) Documentation placement analysis.** Decide where user documentation
  lives: in-repo `docs/` published via a static site, a GitHub wiki, or
  generated command reference plus a handful of guides. Criteria: versioning
  with the binary (a wiki does not version, which matters once `stats_version`
  exists), discoverability, offline availability, contribution friction, and
  whether the command reference can be generated from the argument parser so it
  cannot drift. Record the decision.
- **(b) Write it.** README stays the front door for the whole project —
  what Colosseum GUI and Colosseum CLI are (or their Phase 9.0 replacement),
  install, links. User documentation covers
  the CLI in depth: quickstart, command reference, run-file and tune-file
  reference, worked examples per command, a "how to trust a result" page drawn
  from S3 Tier C, and a compatibility page (what the tool needs from a UCI
  engine, and what it does with non-conforming ones). Explain that engines are
  launched as separate processes and tell users to consult the relevant licence
  terms; do not make a blanket legal conclusion.
- **(c) Ship.** Per Phase 0(c)'s release model; all supported platforms; use
  the identity accepted at Phase 9.0 and its dated
  web/GitHub/package-channel/preliminary-trademark screen;
  smoke-test the exact published artifacts (`--version`, `--help`,
  `self-test`, one stub match, and architecture/dependency inspection).
- **(d) Release-candidate usability exercise.** A **third-party engine pair the maintainers did not
  write** — any two public UCI engines — driven by someone following only the
  published documentation, completing a fixed match, an SPRT and a short SPSA.
  That is the test of "usable by anyone"; the validation engines cannot
  demonstrate it because their authors know too much. Treat feedback as an RC
  gate requiring triage, not as an unautomatable permanent release dependency.
- **(e) Coverage acceptance.** Both validation engines archive every generic
  implementation covered by S5.14 and retain only declarative/thin policy glue
  and the residual list. Any exception is classified as a generic gap or
  intentional project policy. Each engine also runs one real gate through the
  released artifact on at least two operating systems, agreeing with 8(a).

---

## S9. Risks

| Risk | Mitigation |
|---|---|
| Loss of runner independence for users who adopt this for everything | Phase 4B/8 parity against two external runners; recommend periodic cross-checks |
| macOS cannot enforce affinity | Advisory or unavailable, recorded per run; fail only when hard placement was explicitly requested |
| CLI churn destabilises the released GUI | Phase 0 architecture/release design; independent releases; shared-layer regression suite |
| Clean Architecture becomes a rewrite | Current-to-target migration map; smallest boundary refactor; retain working runner/UCI logic |
| Name collision with an existing similar product | Phase 0.8 accepts coherent Colosseum implementation naming; optional Phase 9.0 revalidation and full one-time migration before release |
| Scope creep into engine-specific work | S4 boundary, S5.13 decision, S5.14 mechanism-vs-policy test |
| “No scripts” absorbs project CI/policy | Declarative configs and thin invocations explicitly remain with the engine |
| SPSA diagnostics are mistaken for proof | Label trajectory signals as heuristics; no automatic continue/abandon decision |
| Our defaults read as mandates | S3 tiers; committed run files; user docs name alternatives |
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
| `crates/colosseum-engine/src/{config,paths,incidents}.rs` | current GUI/path/global-state seams to classify in Phase 0 |
| `crates/colosseum-core/src/{engine,tournament}.rs` | current runtime/library model seam to classify in Phase 0 |
| `docs/architecture/` | Phase-0 current/target architecture and ADRs |
| `tests/fixtures/` | vendored golden fixtures + the generator that produces them |
| `CLAUDE.md` | workspace conventions, including why engines are spawned per game |
