# Colosseum — engine-development harness plan

`colosseum-cli` is a headless, cross-platform harness for
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

## S1. Implemented state

**The CLI 0.1.0 implementation and release acceptance are complete; the
first public release is held behind Phase 10.** A maintainer review of the
accepted candidate against the validation engines' real harness policy
found first-release corrections that are cheaper before anyone depends on
published defaults: explicit product-latest handling, adjudication off by
default, class-aware CPU placement with one core of headroom, per-move PGN
annotations, the final-theta SPSA estimator, graceful stop, a fixed rating
field for tournaments and book-range refusal. Phase 11 then moves the GUI
onto the same game-playing mechanism. This document remains the binding
design record and maintenance specification.

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
| Adjudication | none: draw, resign and max-moves all off | See below |
| Resign, when enabled | `movecount=3 score=600 twosided=true` | See below |
| Draw, when enabled | `movenumber=40 movecount=8 score=10` | See below |
| Concurrency headroom | 1 physical core left free, with all its SMT siblings | Room for the harness and the OS; a second free core costs one game slot for no recorded benefit |
| Placement pool on hybrid hosts | the highest-performance core class only | Mixed classes make game slots unequal; see S5.2 |
| SPSA estimator | the final centre vector, rounded | No checkpoint is selected after the fact; a tail-window mean is optional (S5.5) |
| SPSA horizon | 5,000 iterations | A useful default, not a floor; freely configurable |
| SPSA mini-match | 32 games/iteration | Same |
| Engine-fault allowance, fixed-size runs | 1% of the scheduled games, at least 5 (`match`, `calibrate`, `tournament`) | A long run meets the occasional scheduling stall; one forfeit in thousands says nothing about an engine, a steady rate does |
| Engine-fault allowance, sequential runs | 0.5% of the games played so far, at least 3, checked at every commit (`sprt`, `spsa`) | A forfeit is scored as the loss it is, as fishtest and fastchess score it; the length is unknown, so the limit grows with the evidence. `--max-engine-faults 0` / `--max-time-losses 0` restore strict invalidation |
| Opening book | none | The tool ships no book and assumes no path |

**Adjudication is off by default** because every adjudication rule ends games
the engines would otherwise have had to convert, and conversion is part of what
a strength measurement measures. The validation projects measured the cost while
it was on: draw and resign rules ended more than half of all endgames before
they were reached, which starves both the rating and any training corpus of
exactly the positions weak engines misplay, and every adjudicated result rests
on the engines' own evaluations rather than on the rules of chess. The cost of
playing games out is throughput, which is the right thing to spend when the
number has to be trusted. Users who value throughput enable the rules
explicitly; the user documentation names common settings, including those of
well-known public testing frameworks.

**When resignation is enabled it is two-sided** because a one-sided rule
adjudicates on the losing side's own evaluation alone. That is a measurable
asymmetry whenever the two sides differ in how extreme their scores are — most
sharply in SPSA, where both arms are the *same binary* with perturbed
parameters, so the arm that scores more extremely resigns more readily than its
sibling and the difference lands directly in the estimated gradient. Requiring
both engines to agree removes the asymmetry by construction. The threshold of
600 cp over 3 moves is high enough that agreement at that margin is rarely
wrong.

**When draw adjudication is enabled its parameters are conservative on both
axes.** Adjudicating a draw ends a game that might still have contained a
decisive result, and a false draw biases the measured score directly. Three
properties reduce that risk: requiring agreement for 8 consecutive moves rather
than a single ply; requiring a tight band (|score| ≤ 10 cp) rather than a loose
one; and not starting before move 40, because evaluations in the opening and
early middlegame are least reliable and most likely to agree by coincidence
rather than because the position is drawn.

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
Non-chess variants and Chess960 are out of scope: the harness plays standard
chess only and refuses a Chess960 request rather than attempting it.

**Implementation evidence (Phase 10.9):** every path that sets a UCI option
refuses `UCI_Chess960` (and its common spellings) when set true, naming the
non-goal; setting it false stays an ordinary forwarded option. A position whose
castling field uses the Shredder/X-FEN file letters is rejected by the shared
FEN validator, so a book names it among its rejected indices and a suite
records it as malformed instead of searching it. `composition.rs` is now the
parser, the dispatch and the resolvers more than one command needs, with one
module per command beside it; the split changed no behaviour, the generated
command reference is byte-identical and no test changed.

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
  The names, the derivation and the generator algorithm are part of the
  random-stream version (`RNG_VERSION`, recorded as `rng_version`) and not of
  `stats_version`, which says how a reported statistic is defined.
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
  requires an `RNG_VERSION` change; resume keeps the stored version.
- `--dry-run` prints the fully resolved configuration and the exact engine
  invocations without playing a game.
- In machine-readable mode stdout contains one documented JSON value only;
  progress and diagnostics go to stderr.
- Implemented CLI output represents exact process invocations as an executable
  plus argument vector, working directory and environment rather than a
  platform-dependent shell string. `--dry-run` resolves paths and configuration
  identity but does not require the executable to exist or launch it.
- `self-test` launches an internal deterministic UCI stub mode from the same
  executable and checks process, protocol, persistence and one short match.
- The implemented run-directory adapter uses checksummed single-file checkpoint
  envelopes so checksum and payload cannot tear independently. Atomic current
  publication retains one previous valid generation; invalid current state
  falls back, while two invalid generations refuse recovery.
- `run-record.json` is the common read-only status authority. It is written at
  workflow start and every official transition; an ownership guard records an
  aborted state and anomaly if execution ends without an explicit terminal
  transition. Status never attempts recovery or changes run bytes.
- Engine processes run in an owned OS containment mechanism where available
  (process group/job object). Normal shutdown is bounded and escalates from UCI
  `quit` to termination; cancellation and harness failure leave no owned engine
  or descendant running.
- Implemented containment uses per-engine kill-on-close Windows Job Objects and
  dedicated Unix process groups. The CLI additionally installs an outer
  process-lifetime Windows job before doing any work, so owner death during the
  spawn-to-assignment window still reaps the new child; Windows children are
  created suspended, assigned before execution and then resumed. Linux children
  also request `PR_SET_PDEATHSIG(SIGKILL)` and verify the parent did not change
  during setup. The Phase-4C kill/resume fixture records both active PIDs and
  asserts their disappearance before resuming.
- Stdout and stderr are drained concurrently. Protocol lines and in-memory
  queues have documented finite limits; traffic is streamed to artifacts
  outside the clock-critical path. An over-limit protocol line is an
  engine-attributable protocol fault. Queue saturation, artifact write failure
  or inability to drain/contain a process is an infrastructure failure and is
  never converted into a game result.
- The implemented protocol-line limit is 64 KiB excluding newline; stderr keeps
  forty lines with at most 16 KiB per line. The executable self-test floods both
  pipes, crosses the line limit and verifies ignored-quit descendant reaping.

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
- Portable application and implemented topology identity is `(processor group,
  logical number)`, keeping
  Windows groups distinct; Linux consumes and cross-validates exact kernel
  sibling lists. Apple's public `sysctl` surface exposes enabled physical and
  logical counts but no logical-ID sibling map, so macOS records that mapping
  as unavailable rather than inferring it. Placement capability remains owned
  by the later affinity steps.
- Modes `auto` / `off` / explicit CPU list; configurable headroom (default 1
  physical core free, with all of its SMT siblings) in `auto`. The resolved `auto` selection contains full
  reported physical cores (all SMT siblings), while explicit mode retains the
  exact group-qualified logical IDs named by the user. Both require an exact
  sibling map; they fail to resolve rather than guessing where that map is
  unavailable. `off` makes no placement request.
- Detect and respect the CPUs available to the current process, including Linux
  `sched_getaffinity` (the effective scheduler/cpuset/cgroup intersection) and
  Windows process groups, affinity masks, process-default CPU Sets and
  exclusively reserved CPU Sets; never allocate from the machine total when the
  process is restricted. Validate the OS set against the topology snapshot and
  reject an empty or inconsistent result.
- Allocate the configured **cores-per-engine** separately to both engine
  processes in each concurrent game slot — not one core per game. Allocations
  are disjoint, retain all available SMT siblings of each physical core and
  reject `game-slots × 2 × cores-per-engine` requests larger than the selected
  pool. This allocation is independent of whichever UCI option controls the
  engine's internal worker count.
- **Shared game slots.** Without `--ponder` only one engine of a game
  searches at any moment; the other waits on a pipe. The default allocation
  for such games is therefore one core set per *game*, shared by both engine
  processes (`--cores-per-game N`, default 1), so a 16-core host runs 15
  one-thread games at once rather than 7. This is the model the validation
  projects null-calibrated for years. `--cores-per-engine N` selects the
  disjoint per-engine allocation instead; it is mandatory with `--ponder`,
  because a pondering engine searches on its opponent's time, and a shared
  request with `--ponder` is refused. The chosen mode, the pool arithmetic
  (`game-slots × cores-per-game` or `game-slots × 2 × cores-per-engine`) and
  every allocation are in the run record and the dry-run output.

  **Implementation evidence (Phase 10.9a):** `SlotAllocation` carries the mode
  through the placement API, so a slot takes `cores_per_slot()` cores from one
  class, node and cache-domain group and hands both engines the same set in
  the shared mode. `--cores-per-game` and `--cores-per-engine` are mutually
  exclusive at the parser, `--ponder` requires the disjoint one on `match`,
  `sprt`, `calibrate`, `spsa` and `tournament run`, and a pool too small is
  refused with the arithmetic it applied named in the message. Recorded
  fixtures assert 15 shared and 7 disjoint one-thread slots from the same
  16-core single-class SMT host at headroom 1. The run-record schema version
  is 4 because the execution plan reports the mode in place of a per-engine
  core count.
- Placement knows nothing about any particular processor. It reads what the
  operating system reports: physical cores and SMT siblings, core class
  (Windows CPU Set efficiency class, Linux `cpu_capacity`), NUMA node and the
  last-level cache domain (Windows `RelationCache`, Linux
  `cache/index3/shared_cpu_list`), which is the chiplet boundary on multi-die
  parts. `auto` selects only the highest-performance core class when classes
  differ, keeps every game slot inside one cache domain and one NUMA node when
  the pool allows it, and records class, node and cache domain for every
  engine allocation. When the host reports mixed classes but a class is
  unknown, or reports no cache topology for a multi-domain part, `auto`
  refuses with a message that names the detected topology and asks for an
  explicit CPU list; it never guesses. Any unavoidable asymmetry between the
  two engines of a slot is visible in the run record.
- Fail when requested placement cannot be applied (A1); allow and record `off`.
- macOS has no supported hard-affinity API: report the capability as advisory or
  unavailable, record which, and do not prohibit clock matches.

**Implementation evidence (Phase 10.3):** the default `auto` headroom is one
whole physical core with all of its SMT siblings. Last-level cache domains are
read from `GetLogicalProcessorInformationEx(RelationCache)` on Windows and from
`cpu*/cache/index*/{type,level,shared_cpu_list}` on Linux, taking the highest
unified or data level a core reports; domains are the distinct sharing sets at
that level, indexed by their lowest member so the identity is stable between
runs. A core the OS reported no cache for keeps no domain, and nothing is
inferred for it from a neighbour. `auto` selects only the highest-performance
class when classes differ, and slot allocation groups by class, node and cache
domain so a slot stays inside one domain and one node whenever the pool allows
it; class, node and cache-domain mismatches and spans are recorded per slot.
`auto` refuses, naming the detected topology and asking for an explicit CPU
list, when classes are mixed and one is unknown, when cache evidence exists for
only some cores, or when a part reporting no cache topology also reports more
than one NUMA node. A host reporting neither cache nor multiple nodes offers no
evidence of being multi-domain, so placement proceeds and both facts stay
visible as unreported. `capabilities` reports class, NUMA and cache domains and
its schema version is 2. Recorded fixtures assert both the chosen pool and the
slot allocation for a hybrid performance/efficiency host, a single-socket
dual-cache-domain host, a homogeneous SMT host, a no-SMT host, a restricted
cpuset, processor groups and a dual socket. macOS still has no sibling map, so
`auto` and explicit placement remain unresolvable there while `off` is
unaffected. The run record schema version is 3 because every engine placement
in the record gained its cache domain; later Phase 10 steps do not re-bump an
unpublished version.

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

**Implementation evidence (Phase 4C complete):** the CLI records the resolved
SHA-256 identities and representative conditions in the ordinary durable run
record. Hermetic acceptance covers exact configuration resume, mismatch
refusal, every interval/fault outcome and every automation exit. A short
Basilisk 1.9.0 Windows smoke verified real UCI processes, two concurrent slots,
enforced disjoint affinity and durable zero-variance `INCONCLUSIVE` output; it
is pipeline evidence, not a full tolerance measurement. See
[`docs/architecture/phase-4c-exit.md`](docs/architecture/phase-4c-exit.md).

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
- **Adjudication:** off by default (S3 Tier B). Draw, resign and max-moves are
  each individually enableable with explicit parameters; enabling one is part
  of the resolved configuration and run identity.

  **Implementation evidence (Phase 10.2):** `--draw-adjudication` and
  `--resign-adjudication` enable their rule on `match`, `sprt`, `calibrate`,
  `spsa` and `tournament run`; the former `--no-*` flags are gone rather than
  retained as no-ops. Every parameter (`--draw-move`, `--draw-moves`,
  `--draw-score-cp`, `--resign-moves`, `--resign-score-cp`) and the
  `--one-sided-resign-adjudication` modifier requires its enabling flag, so a
  parameter supplied alone is a visible refusal instead of a silently ignored
  setting. Resolved configuration, its SHA-256, the run record, dry-run output
  and the resume comparison all carry the three null rules by default. The
  user documentation names common public-framework settings for users who want
  throughput. Arbitrary engine tablebase UCI options may
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
  disconnect or illegal move is a forfeit and anomaly; a command is invalid
  once its configurable engine-fault threshold is exceeded (time losses count
  inside it; since Phase 10(q) the default is zero for the pair-atomic `sprt`
  and `spsa`, and 1% of scheduled games with a minimum of 5 for `match`,
  `calibrate` and `tournament`). An infrastructure/harness failure is never
  scored: pause or invalidate
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

### 5.4b Game record annotations — every game-playing command

`games.pgn` carries per-move search evidence, because a PGN of bare moves
cannot feed a training-data extractor or a tree-shape comparison, and the
runner already holds the values. After every engine move the comment is
`{s=<score> d=<depth> t=<ms>ms n=<nodes>}`: score from the mover's point of
view as a signed integer in centipawns, or `#<n>` / `#-<n>` for mate in `n`;
depth, harness-charged elapsed milliseconds and reported nodes as integers. A
field the engine did not report is omitted, never written as zero. Pre-played
book moves carry `{book}` and the `OpeningPlyCount` tag remains. The `stats`
telemetry parser reads this form in addition to the existing ones and gains
score coverage and mean absolute score, so `stats` on a Colosseum PGN reports
full coverage. The writer form is versioned in the run record.

**Pair identity.** Every Colosseum-written game carries tags that identify it
without the checkpoint: the harness game number, the pair number, the
opening index and label, and which colour assignment of the pair it is.
`stats` on a Colosseum PGN reconstructs complete pairs from those tags and
reports the same pentanomial vector as the checkpoint; only a PGN without
them falls back to unpaired statistics. Explicit zero fields (`d=0`, `n=0`)
are reports and the telemetry parser accepts them; only unreported fields
are omitted.

**Implementation evidence (Phase 10.9b):** every written game carries
`GameNumber`, `PairNumber`, `PairGame`, `OpeningIndex` where a book supplied
one, and `OpeningLabel`, beside the seven-tag roster and `OpeningPlyCount`.
`stats` on such a PGN takes each outcome from the pair's first engine — the one
that had White in assignment 1 — which is the checkpoint's own perspective, so
the two sources report the same pentanomial vector; a PGN without the tags is
still never paired by the order its games appear in. `stats <run-dir>` keeps
checkpoint authority for statistics and reads the directory's own `games.pgn`
for telemetry, so an annotated export beside a checkpoint is no longer reported
as no telemetry at all.

**`stats_version` decision (Phase 10.9b), resolving the explicit-zero and
`t=0ms` questions together:** accepting explicit `d=0`, `n=0` and `t=0ms`, and
reconstructing pairs from a PGN that previously fell back to unpaired, all
change what `stats` reports for an unchanged input — but only relative to
unreleased builds. No published version reported the old definitions, so the
statistics 0.1.0 ships are simply version 1 and the constant is not bumped.
What was a real trap is fixed: `stats_version` was wired to `RNG_VERSION`, so
any future bump would have claimed the random stream changed. `STATS_VERSION`
is now its own constant in `colosseum-core`, and the SPSA schedule artifact
keeps `RNG_VERSION` for the stream identity it actually records.

**Success criteria:** a stub-engine match annotates every post-opening move;
`stats` replay on that PGN reports 100% coverage for score, depth, time and
nodes; a frozen annotated fixture in `tests/fixtures/` is parsed by the
telemetry parser and by the workspace PGN reader used for openings.

**Implementation evidence (Phase 10.4):** the shared PGN writer takes one
annotation per half-move, so every command that runs games through the runner
— `match`, `sprt`, `calibrate`, `spsa` and `tournament` — gets the comments
without its own writer. `t` is the harness-charged elapsed interval of the
recorded clock model, which is the only time the harness can honestly
attribute; an explicit `0ms` is a real sub-millisecond measurement and the
telemetry parser now reads it as one instead of discarding it as a placeholder.
The writer form is `colosseum-move-comment/2` and every run record names it.
The telemetry parser gained `s`/`score`, score coverage and mean absolute
score; a mate score counts as covered but is excluded from the centipawn
values because it is a distance claim, not an evaluation on the same scale.
[`tests/fixtures/annotated-games.pgn`](tests/fixtures/annotated-games.pgn)
freezes the form with book plies, both score signs, both mate signs and a move
whose engine reported no nodes, and is asserted through both the telemetry
parser and the openings PGN reader.

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

  **Implementation evidence (5.1):** [`colosseum-core::spsa`](crates/colosseum-core/src/spsa.rs)
  implements this kernel without runtime or UCI dependencies. Perturbation
  draws are random-access but byte-identical to the versioned named stream in
  iteration-major, tune-file-knob order; golden coefficients, vectors,
  half-away-from-zero ties, floating centres, score-sign symmetry and clipping
  are pinned by tests. End-state back-solving and persisted schedule assertion
  remain 5.2 responsibilities.

  **Implementation evidence (5.2):** the core back-solves `c0/a0` from each
  `c_end`, run `r_end` and `N`, then asserts the terminal `c/a/r` values at a
  relative tolerance of `1e-12`; an independent golden fixture and multiple
  horizons pin the result. The self-describing schedule artifact records its
  schema/statistics versions, exact ChaCha12/derivation/sampling identifiers,
  master and derived stream seeds, stable stream name, eight-byte Rademacher
  draws and iteration-major knob order. The CLI atomically creates
  `spsa-schedule.json` without replacing an existing resume artifact, reads it
  back, and only the application-layer verified-schedule token can cross the
  future game-launch boundary. Invalid, mutated or differently derived files
  cannot produce that token.
- The tune file selects numeric UCI options and supplies initial value, tuning
  bounds and `c_end` per knob, validated against the live UCI option schema.
  Its strict root schema is an ordered `[[parameters]]` array; each entry has
  exactly `name`, `initial`, `min`, `max` and `c_end`. Array order is the
  versioned SPSA knob/draw order, so a map keyed by parameter name is not an
  equivalent representation. TOML parsing belongs to the CLI adapter; the
  application binds every parsed name to a live advertised `spin` option before
  schedule derivation. The later config audit owns duplicate, range and
  rounding-resolution policy.

  **Implementation evidence (5.3):** [`SpsaTune`](crates/colosseum-application/src/spsa.rs)
  is the runtime-neutral ordered numeric vector and preserves both requested
  values and the observed spin schema. The strict CLI TOML adapter rejects
  unknown/malformed fields; absent and non-spin options are rejected against
  the handshake schema. A hermetic integration test proves that an ordinary UCI
  executable's live `Hash` spin declaration, rather than any engine descriptor,
  supplies the binding range.
- Defaults `N=5,000` and 32 games/iteration; configurable, not enforced minima.
  The only structural restriction is that a mini-match has a positive even game
  count, because it commits complete colour-reversed pairs; one iteration and
  two games are valid short-run settings.

  **Implementation evidence (5.4):** the application-owned, serializable
  `SpsaRunSettings` supplies the two defaults and a typed constructor for any
  positive iteration count and complete-pair game count. It exposes the derived
  pair count for the future driver while avoiding an artificial tuning minimum.
- Persistent driver: no per-iteration relaunch; a supplied book is loaded once.
- Multi-session per S5.11.
- A mini-match is committed only when all of its scheduled colour pairs
  complete. An engine-attributable fault invalidates the iteration and tune;
  it is never converted into a gradient or selectively retried.

  **Implementation evidence (5.5):** the `spsa` composition root accepts one
  ordinary UCI executable plus the required ordered tune file, binds it through
  the live handshake and passes only the schedule token read back from
  `spsa-schedule.json` into the application-owned `SpsaTuningState`. That state
  machine alone prepares the next arms, requires the complete mini-match,
  applies a fault-free update, rejects fault-derived gradients and exactly
  replays durable history. The CLI adapter owns UCI execution and persistence,
  not tuning policy. Its optional book is parsed once per process session and
  reused in memory; engines retain deliberate per-game isolation. Each
  iteration runs a deterministic global pair-ID range and publishes one
  checkpoint only after every colour pair finishes. A non-scorable failure
  publishes no iteration; any engine fault publishes the complete invalid
  evidence with no `centers_after` and no gradient. Resume replays every stored
  centre, RNG draw, arm vector, score and update before continuing, compares
  executable SHA-256, and keeps the stored schedule authoritative. The JSON
  adapter enables exact float round trips after a three-iteration regression
  exposed a one-ULP schedule change without it.
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

  **Implementation evidence (5.6):** the application-owned audit checks the
  ordered vector before schedule derivation for duplicate names, valid tuning
  bounds, an initial centre inside those bounds and an integer-resolvable final
  perturbation. After the live UCI handshake it rejects every requested value
  outside the advertised spin range, while a non-default centre or either
  tuning rail becomes an ordered, serializable warning. The CLI presents those
  warnings and records them in the SPSA report and run record; dry-run performs
  every audit that does not require a live engine.
- **Closing the loop.** A tune must not end at "here is a vector". On
  completion, and on demand mid-run, emit the final completed centre vector,
  rounded half away from zero (the registered estimator of both validation
  projects: no checkpoint is selected after the fact), as
  (a) a ready-to-paste `setoption` list, (b) JSON, and (c) a run file fragment.
  `colosseum-cli sprt --apply <result.json>` then gates the tuned values against
  the original vector **using the same executable and UCI options only** — no
  source edit, rebuild or engine-specific baking step. The artifact contains
  executable hash, original/tuned vectors, tune conditions, schema and schedule
  versions. A changed executable hash is refused unless explicitly overridden,
  and an override is prominent in the gate record. A tail-window mean
  (`--final-window-percent`, 1–100) remains available; when selected it is
  frozen in the configuration and result exactly as the default is, and
  `spsa status` exposes the same candidate policy mid-run. The result schema
  version identifies which estimator produced a vector.

  **Implementation evidence (5.7, superseded by Phase 10.5 for the default):** application policy freezes a configurable
  1–100% final horizon window (default 10%, sample count rounded up), validates
  ordered centre history and emits the half-away-from-zero rounded mean with
  original values, rails, exact window, executable hash and result/schedule/
  statistics versions. A completed tune writes ready UCI lines, versioned JSON
  and an `[engine.options]` TOML fragment; its main result also retains the
  resolved engine launch and full schedule/tune conditions. `sprt --apply`
  consumes that result unedited, constructs tuned A versus original B from an
  otherwise identical launch, verifies executable content before dry-run or
  launch, refuses per-side mutation, and records either `verified` or the
  prominently warned `mismatch-overridden` identity in configuration, result
  and run record. The result policy accepts partial history only once the frozen
  final window begins so the Phase-5.9 read-only adapter can expose the same
  calculation mid-run without inventing a second rule.

  **Implementation evidence (Phase 10.5):** the default estimator is the
  half-away-from-zero rounded centre vector after the last completed iteration,
  so no checkpoint is chosen after the fact. `--final-window-percent N` selects
  the optional tail-window mean; whichever is in force is frozen in the
  resolved configuration and named in the result together with its exact
  window, and the result schema version is 2. `spsa status` reuses the same
  policy, so the default estimator exposes an on-demand candidate from the
  first committed iteration rather than only once a window begins.
  `--stop-after-iteration N` stops on a committed iteration boundary, writes
  the checkpoint, records `cancelled`, exits with code 6 and still emits a gate
  candidate; the stored horizon is untouched, so the same run directory resumes
  towards it. The stop request is an invocation fact in the run record, not
  part of the resolved configuration that a resume compares.

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

**Implementation evidence (5.8):** runtime-neutral application policy validates
the ordered tune and exact schedule, emits every per-knob `c/a/r` point, reports
the first sub-half-unit rounding hazard, and computes iteration/game/pair,
checkpoint-generation and schedule-artifact counts with checked arithmetic.
Optional timing is derived only from an explicit end-to-end seconds/game range
or repeated pilot-game observations and accounts for sequential iterations plus
concurrent mini-match waves. Repeated comparison horizons re-derive first/final
gain and cost rather than scaling one schedule. Output labels this as factual
workload planning, never a chess-convergence forecast; no synthetic objective
surface is implied when none is supplied.

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

**Implementation evidence (5.9):** the application layer validates the exact
durable centre prefix and computes normalized trajectories, three equal history
segments, current perturbation and linear durable-checkpoint ETA. Six completed
iterations are required before history heuristics: exact bound contact at 20%,
net seed movement at 1% of range and latest-third span at 1%; the half-unit UCI
rounding boundary remains schedule-factual. Every signal carries the same
objective/noise/gain/clipping/range caveat and no continue/abandon advice.
The CLI reads the checksum-verified current checkpoint with previous-generation
fallback, verifies the persisted schedule, replays every committed update, and
never opens run ownership or writes recovery state. Live-fixture coverage proves
bounded non-blocking observation and a stopped-run byte snapshot proves the
command is non-mutating. Once the frozen final window begins, status reuses the
5.7 result policy to expose an on-demand gate candidate.

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

Implemented Phase 6.1 establishes the authoritative boundary: the application
requests a typed fixed-node search, the UCI adapter returns the monotonic charged
interval plus separately labelled engine claims, and the use case refuses a
sample unless `info nodes` proves at least the requested work completed. Speed
is requested nodes divided by harness wall time; neither `info time` nor
`info nps` participates. Multi-arm experiment design remains Phase 6.2.

Phase 6.2 implements that design as Cartesian build pairs so unequal build
counts still permit strict arm alternation. Position, pair and warm-up order use
named seeded streams; cold starts exclude startup from the charged interval,
while warm sessions use `ucinewgame`/`isready`. JSON retains the exact schedule,
raw samples, per-build medians, arm median/best-build, bootstrap CI and
per-round B/A ratio SD. A self pair is optional and tolerance-labelled.

Phase 6.3 composes the existing topology, allowed-set and hard-affinity adapters
with UCI sessions. A scaling sweep requires an explicit advertised thread spin
name and a one-thread baseline; every point uses that many whole physical cores
from the same stable pool. Fixed-total/per-thread Hash, selected CPU class/NUMA,
median wall-time NPS, speedup and efficiency are structured evidence. Missing
exact placement evidence is a refusal, not an unpinned fallback.

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
standings/crosstable CSV and resume per S5.11.

A tournament may also declare a **fixed field**: any number of participants
pinned at supplied ratings, with only the remaining participants estimated
jointly against them and each other. This is how a newcomer is placed in an
established pool without spending games on re-measuring the pool, and it uses
the core's anchored maximum-likelihood rating with an anchor set rather than a
second implementation. Pinned participants report no error bar; the run record
retains the fixed ratings as inputs. A single `--anchor` is the degenerate
case and stays. A `gauntlet` alias may exist for
convenience, but it resolves to the same tournament use case rather than a
second implementation.

Implemented Phase 7.1 places deterministic round-robin and one/multi-seed
gauntlet planning in one application use case. `tournament plan` and the
optional `gauntlet` alias dispatch to that same implementation; participant
order gives stable identity and the scheduler is the same shared core used by
the GUI.

Implemented Phase 7.2 drives that plan through the independent UCI runner with
bounded concurrency, optional opening/affinity controls and checksum-protected
per-game resume. The application use case validates durable game identity and
computes the shared joint ML ratings, 95% error bars, optional single-engine
anchor, standings and crosstable data. The CLI writes both CSV exports plus PGN,
JSON, log and common run-record artifacts; exploratory non-strict engine-fault
handling is explicit and a strict limit remains selectable.

**Success criteria:** schedules and ratings match the GUI on stored data
(ratings ≤0.01 Elo); kill/resume with deterministic stubs produces identical
standings for both formats.

**Implementation evidence (Phase 10.7):** `tournament run --fixed INDEX:RATING`
is repeatable and pins any number of participants at supplied ratings; a pinned
rating replaces that participant's prior, so the fixed field *is* the scale the
remaining participants are estimated against through the same anchored
maximum-likelihood rating, not a second implementation. A pinned participant
reports no error bar, because its rating is an input rather than something the
tournament measured. `--anchor` remains the degenerate case, pinned at its own
prior, and naming one participant through both is refused. A malformed,
duplicated or out-of-range entry, and a field that pins every participant, are
all refused at configuration time rather than silently dropped. The supplied
ratings are retained in the result and the run record as run inputs; JSON rows
carry `fixed`, the shared standings CSV gained a `Fixed` column beside the
already-empty `EloDelta`, and text rows are marked `[fixed]`.

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

Implemented Phase 6.4 shares the match parser but adds strict candidate
accounting: verify names every rejected one-based EPD-line/PGN-game index.
Hash covers exact bytes; stats adds usable/unique/duplicate and ply counts plus
EPD `ce` or PGN `%eval` bands when present. Slice requires a clean audit,
applies the named opening-order stream, materializes canonical EPD with LF line
endings, refuses overwrite by default and records input/output hashes.

**Success criteria:** slicing is byte-reproducible across platforms; `verify`
rejects a known-bad fixture.

**Range policy for consumers.** Every game-playing command consumes book
entries sequentially from `--book-start` in the resolved order. A run whose
schedule needs more entries than remain refuses at resolution time, naming the
shortfall, so two segments of one book cannot silently replay openings;
`--book-wrap` opts into modular reuse and is recorded. Dry-run reports the
exact index range a run will consume.

**Implementation evidence (Phase 10.8):** opening resolution takes the number
of entries a schedule actually consumes — one per colour-reversed pair for
`match`, `sprt`, `calibrate` and `spsa`, one per encounter for `tournament`,
which plays every game of an encounter from the same opening — and refuses when
that exceeds what remains, naming the requirement, the remainder and the
shortfall and pointing at the four ways out. `--book-wrap` is the only way to
reuse and is recorded in the resolved configuration; the reused fraction is
reported either way. The resolved configuration and every dry run carry
`first_index`, `last_index`, `scheduled_openings` and `wrap`, so a long
schedule is checked against its book before it starts. The match documentation
carries the S5.13 datagen recipe written against this policy: shard a book with
`book slice`, size the shard to the pair count, advance `--book-start` by the
range the previous shard consumed, and never rely on wraparound for a corpus.

### 5.10 Statistics replay — `colosseum-cli stats`

Read a CLI run, a PGN, or a supported external result log and report the
same block used live. External formats and versions are explicitly listed; when
pair/opening identity is absent, fall back to labelled unpaired statistics
rather than guessing pairs.

**Authority order:** the structured run store/checkpoint is authoritative; PGN
is the portable game export; logs are forensic evidence; console output is
observational only. Every live number must be reproducible from the structured
run directory. PGN replay reproduces only information the PGN actually carries.
A run directory is one evidence set: statistics come from its checkpoint and
telemetry from its own `games.pgn`, so `stats <run-dir>` never reports
telemetry as unavailable when the directory holds an annotated PGN.
Implemented in Phase 10.9b together with the PGN pair identity above.

Implemented Phase 6.5 walks that order explicitly and retains an attempt audit.
Checkpoint generations are checksum-verified. Exact structured schedule number,
colour reversal and opening equality are required before two games enter a
pentanomial bin; PGN/console replay is always labelled unpaired, while complete
JSON-lines `game-completed` evidence may retain pair identity.

**Search telemetry from a PGN.** Where move annotations are present, report per
engine the coverage fraction and mean/median depth, elapsed time and implied
nodes per second. List the supported annotation syntaxes; preserve unknown
annotations; exclude pre-played opening moves; and report `unavailable` when
coverage is insufficient. Fixed-node telemetry is comparable only when node
accounting has compatible semantics—normally the same engine lineage.

Implemented Phase 6.7 recognizes bracketed `%depth`/`%emt`/`%nodes` and
explicit-unit key/value comments. It reports denominators and coverage beside
every aggregate, never maps missing values to zero, and keeps the analysis
read-only so unknown annotations remain untouched. Colosseum PGNs now record
`OpeningPlyCount`; external per-move `book` comments are the fallback exclusion
signal. The report retains a node-semantics compatibility warning.

**Experiment planning**

- `stats plan fixed` accepts target effect/equivalence margin, significance,
  desired power and an assumed pair distribution, then estimates required
  pairs and states every assumption.
- `stats plan sprt` accepts hypotheses/error rates plus an assumed true effect
  and pair distribution, then simulates an expected-length distribution. It is
  a planning model, not a stopping guarantee.
- Post-run output reports achieved intervals/resolution; it never back-fits a
  planning MDE and presents it as a fact.

Implemented Phase 6.6 keeps this workflow engine-free. Fixed difference and
symmetric TOST-equivalence designs state their normal-approximation,
distribution and zero-effect assumptions and may report descriptive achieved
resolution from supplied counts. SPRT planning uses a dedicated versioned
named RNG stream, retains the seed/cap/algorithm, counts capped simulations and
labels its output as an expected-length model rather than a stopping guarantee.

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
- **Graceful stop.** Ctrl-C (SIGINT, and the Windows console control event)
  stops launching new units, lets in-flight games finish or aborts them after
  a bounded grace period, writes a checkpoint, marks the run record
  `cancelled` and exits with the documented cancelled exit code; a second
  Ctrl-C is the hard kill. Units not yet committed are replayed on resume; for
  SPSA that is the whole current mini-match, which is accepted.
  `spsa --stop-after-iteration N` requests the same clean stop at an
  iteration boundary without changing the stored horizon. The documented
  cancelled exit code is 6.
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

**Implementation evidence (Phase 10.6):** one `Cancellation` handle is created
per invocation, listens for the console control event on Windows and `SIGINT`
elsewhere, and is threaded through the match, SPRT, tournament, SPSA and suite
drivers. Each driver stops launching new units, selects on a bounded grace
period (`--stop-grace-secs`, default 30) and aborts the work still in flight
when it expires or a second interrupt arrives; `kill_on_drop` reaps the engine
processes. The run writes its checkpoint, records `RunStatus::Cancelled` and
exits with code 6, which `status` reports. A stop is never dressed as a
conclusion: an SPRT that reached no boundary reports `cancelled` rather than
`inconclusive`, and a calibration that stopped short of its fixed sample says
so instead of classifying a partial interval. The suite asks its progress port
whether to stop, so the application layer stays runtime-neutral. A committed
unit budget is a second trigger for the same path, which is what makes the
interrupt behaviour testable: a console interrupt cannot be delivered portably
to one child process on Windows, so the shared clean-stop suite drives the
identical path through a hidden `__stop-after-units` option and asserts, for
each of the six durable commands, that the run stopped, checkpointed, recorded
`cancelled`, exited 6 and resumed to the uninterrupted statistics.

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

Implemented Phase 6.8 parses EPD/FEN through a chess-legal adapter and sends
only ordinary UCI `position`/bounded `go` commands through the shared engine
session port. It preserves unknown EPD operations as ignored evidence,
distinguishes unscored and malformed input from pass/fail, commits every input
index before publishing progress, and resumes without duplication. Versioned
parsed-position and fixed-work hashes guard baseline comparison while allowing
the engine build itself to differ.

### 5.13 Data generation — deferred as a separate command

Fixed-node/depth self-play written as PGN is already expressible as a normal
match using the same scheduler, placement and durability. Document that recipe
and do not create a second `datagen` workflow in v1.

Revisit in Phase 8 only if concrete engine-independent requirements exceed
`match`: corpus sharding, deterministic game IDs, deduplication, controlled
randomisation or effectively unbounded horizons. Training-format extraction,
position filtering, labelling and trainer-specific records remain out of scope.
With S5.4b annotations and the S5.9 range policy the recipe is complete for the
validation projects' corpora; a dedicated command remains declined.

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
11. **CI matrix: Windows, Linux, macOS × debug and optimized.** Debug is not
    optional — a debug build is far slower, a CI runner slower again, and that
    combination is exactly how a flat-timeout test passes locally for months and
    fails on a runner. The optimized leg runs the `ci-release` profile: the
    shipped release profile without the distribution-only LTO and single
    codegen unit, which change nothing the suite can observe and cost minutes
    across forty test binaries. Each matrix leg carries its own build cache;
    sharing one lets the legs race to save it and leaves the loser permanently
    cold.
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

When a Claude model is used instead, **Sol High** corresponds to Claude Opus 5
at high effort and **Terra High** to Claude Sonnet 5 at high effort.

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
| 10 | 10.1, 10.4, 10.7–10.9 | 10.2–10.3, 10.5–10.6, 10.9a–10.9c, 10.10 |
| 11 | 11.4 | 11.1–11.3 |

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
descriptor. Phase 3.3 extended CPU identity without adding engine metadata:
unqualified values remain group zero and Windows group-qualified ranges use
`GROUP:START-END`.

**Implemented configuration resolver (2.4):** generic CLI adapter code applies
each inherited file's RFC 6901 `unset` to its resolved parent before recursively
merging that file, then applies command-line clearing/overrides. Canonical file
identities, a 16-file bound and chain-rich errors cover inheritance; every leaf
retains built-in/file/CLI origin. Command schemas enumerate their path pointers,
which are normalized relative to those origins before stable-key JSON is hashed
and written with an origin sidecar. Inherited/flattened and run-file/all-CLI
fixtures are byte- and hash-identical, including Windows path-alias handling.

**Implemented master-seed contract (2.4a):** the domain derives each stable
ASCII stream name with the specified SHA-256 label/master-seed/name byte
sequence and runs an explicit ChaCha12 stream at counter/stream zero. Version-1
sampling defines little-endian u64 reads, rejection-bounded integers, descending
Fisher–Yates, Rademacher mapping and bootstrap-with-replacement. Independent
Python-reference-derived vectors pin all built-in names, raw bytes and sampling
outputs. The CLI retains a configured u64 or obtains OS entropy, inserts the
generated seed before hashing/writing and exposes the resolved value through the
application seed port. The built-in vectors pin `RNG_VERSION`, which Phase
10.9d separated from `stats_version` in the artifact that records it.

**Implemented engine diagnosis (2.5):** `engine inspect` launches the supplied
ordinary executable with direct process controls and reports handshake identity
and the advertised option schema. `engine check` owns its compliance sequence in
the application layer and emits separate handshake, ready, requested-schema,
option-command-plus-readyok, bounded legal start-position search, bounded
stop/bestmove, new-game/readyok and shutdown results. It never calls option
acceptance a read-back. The UCI adapter now supports start/stop collection and
reports a quit timeout after killing/reaping rather than mislabelling it clean.

**Accepted (2.10):** the complete hermetic exit corpus covers two independent
path-only UCI executables, isolated GUI-state sentinels, configuration
equivalence, named-stream invariance, copied-executable self-test, bounded pipe
and descendant containment, durable recovery, aborted/terminal status and
inward dependencies. The same path-only compliance command also passed locally
with the independently developed Rarog (Rust) and Basilisk (C++) engines; their
machine paths are deliberately not repository inputs. The CLI's unused legacy
engine/SQLite dependency was removed. The full workspace/GUI suite remained
green. Detailed criterion ownership and intentional later-phase boundaries are
recorded in `docs/architecture/phase-2-exit.md` and the machine-readable Phase 2
acceptance manifest.

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

**Implemented topology-quality placement (3.5):** Windows uses OS CPU Set
efficiency-class and NUMA data; Linux uses kernel `cpu_capacity` when present
and sysfs NUMA membership, retaining unknown rather than guessing when no class
signal exists. Slot allocation first seeks one class/node location for both
engines, then preserves per-engine node locality and class symmetry where
possible. Every allocation records its class/node sets and explicit mismatch or
span flags, so unavoidable asymmetry remains visible to later run records.

**Implemented affinity application (3.6):** the OS adapter applies and reads
back Windows process masks or Linux per-thread scheduler masks. Unsupported,
invalid and unverifiable hard requests are typed failures rather than silent
fallbacks. Windows inspects the target's current thread primary groups, exposes
its single-primary-group constraint as capability data and fails mismatched
group requests instead of misidentifying group-relative CPU numbers;
macOS reports hard pinning unavailable because its affinity tags are advisory.
Explicit `off` is a successful recorded no-op, so lack of hard affinity does
not prohibit an otherwise valid clock match.

**Implemented capability probe (3.7):** `colosseum-cli capabilities` is a
strictly read-only text/JSON probe of topology, exact sibling identity,
current-process restrictions, core class/NUMA metadata and hard-affinity
support, mechanisms, limitations and unavailable reasons. The CLI consumes a
feature-minimal platform surface from `colosseum-engine`; architecture tests
prove the independent artifact does not pull in SQLite, tournament scheduling
or chess-position dependencies merely to report host capabilities.

**Accepted (3.8):** a recorded six-shape corpus covers 16c/32t SMT, hybrid
performance/efficiency cores, restricted cpusets, duplicate group-relative IDs
in distinct Windows processor groups, no-SMT and dual-socket NUMA. Every fixture
asserts exact per-engine CPU lists and visible asymmetry. On enforceable hosts,
two busy child processes are pinned and repeatedly sample only their assigned
logical processor; unsupported hosts emit the documented unavailable reason.
The machine-readable acceptance manifest assigns every exit gate, the CLI
dependency graph remains headless and free of tournament/SQLite payload, and
the full workspace regression suite remains green. Detailed evidence and the
current Windows single-primary-group limitation are recorded in
`docs/architecture/phase-3-exit.md`.

**Implemented fixed match surface (4A.1):** `colosseum-cli match --games N`
plays exactly N sequential direct-UCI games and alternates colours. Its two
launch specifications are independent, so the same executable path with
different side-specific options is a supported ordinary-UCI comparison. It
provides strict JSON and dry-run output, exposes runner-visible engine errors,
and deliberately leaves time controls, adjudication, fault policy, placement,
concurrency, books and durable output to their assigned later steps. The CLI
uses `colosseum-engine`'s runner feature without enabling its scheduler or
SQLite storage feature; architecture coverage preserves that boundary.

**Implemented per-side controls (4A.2):** fixed matches independently resolve
movetime, sudden-death, base-plus-increment, fixed-node or fixed-depth limits
for each arm, with the Tier-B `3+0.03` default and a separately configurable
per-side forfeit margin. The shared runner now chooses limits and clock state
from the side to move; the GUI scheduler maps its existing symmetric control
to both sides, preserving desktop behaviour.

**Implemented clock accounting (4A.2a, superseded by Phase 10(q)):** the
runner used clock model `go-write-to-bestmove-read` version 1 through 10.9f;
since 10.9g the model is `go-write-to-bestmove-arrival` version 2, in which
the interval ends at the instant the pipe reader received the complete
`bestmove` line rather than when the game task read it. The monotonic charged
interval excludes position setup and begins immediately after the flushed
`go` write. The explicit `E > R + M` boundary forfeits before
increment, equality is accepted, and every structured game result records the
model/version, margins, probed resolution and per-side charged elapsed
min/median/max without inventing an engine-versus-harness split.

**Implemented adjudication controls (4A.3):** fixed matches resolve the Tier-B
two-sided resignation and draw defaults into every configuration, expose each
threshold plus an optional maximum-move cap, and allow draw or resignation to
be disabled independently. Tablebase UCI values remain arbitrary per-engine
options; no harness-side probing or adjudication dependency was introduced.

**Implemented failure policy (4A.4):** game reports now distinguish typed,
side-attributed engine faults from non-scorable infrastructure failures.
Engine timeouts, disconnects, protocol faults and illegal moves remain visible
forfeits, while spawn/harness failures cannot enter W/L/D. Fixed matches enforce
separate configurable engine-fault and time-loss thresholds with strict zero
defaults and stop as invalid or infrastructure-error without a selective
retry/discard mechanism.

**Implemented match resources (4A.5):** fixed matches expose bounded
concurrency and retain deterministic schedule-order output. Placement `off`,
`auto` and explicit pools compose the Phase-3 topology, symmetry and hard
affinity adapters; direct per-side lists remain available for a single slot.
The resolved slot allocations are structured evidence. Explicit numeric Hash
values produce only `concurrency × (A Hash + B Hash)` as a lower bound, and
memory is refused only against an explicit trusted budget.

**Implemented opening policy (4A.6):** a book remains optional and no asset is
shipped. EPD/PGN input supports sequential or RNG-v1 named-stream order, a
validated start index and PGN ply depth. One opening is assigned to each
colour-reversed pair; every game records its assignment and the result reports
reuse fraction. No-book runs remain valid startpos runs with an explicit
diversity warning, and supplied or generated master seeds are recorded.

**Implemented match output (4A.7):** every live fixed match now composes the
common recoverable run directory. Completed games enter checksummed
two-generation state, an append-only JSON-lines event log, a checkpoint-derived
PGN, final structured report and versioned run record; abnormal games retain
their UCI traffic separately. Configurable progress is stderr-only, JSON stdout
remains one document, and exit codes distinguish valid completion, invalid
experiment, configuration refusal and infrastructure/runtime failure.

**Phase 4A exit evidence (4A.8):** the hermetic suite now kills and resumes a
live fixture match, replays the same seeded paired-book schedule across
different concurrency, injects non-scorable infrastructure faults, charges a
commanded engine sleep, and pins sub/equal/super-margin attribution,
deduction-before-increment and monotonic wall-clock independence. The reviewed
matrix and platform-evidence boundary are recorded in
`docs/architecture/phase-4a-exit.md`.

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

**Implemented design contract (4B.1):** the runtime-neutral application layer
validates and serializes the selected normalized/logistic model, ordered
hypotheses, alpha/beta, exact Wald bounds and nonzero finite pair cap before any
engine launch. `gainer` expands to normalized `[0,5]` and `simplify` to
normalized `[-5,0]`, both at 5%/5%; every expanded field remains explicitly
overridable and visible in dry-run output.

**Implemented pair commit contract (4B.2):** a complete colour-reversed pair is
now a typed application value containing both games. Concurrent worker results
enter a fail-closed commit queue and become observable/durable only as the
contiguous pair-ID prefix, so completion timing cannot reorder the official
sample and no half-pair can reach statistics. The CLI adapter executes both
colours of the assigned opening before submitting that value.

**Implemented terminal cut (4B.3):** only the deterministic committed prefix is
fed to pentanomial SPRT. The pair that first crosses H0/H1 remains in the
official sample; the scheduler immediately stops launching new pairs, lets
already running complete pairs finish, and records their results in a distinct
post-terminal collection that cannot change the LLR, decision or terminal pair.
Cap exhaustion with no boundary retains the complete capped prefix.

**Implemented live/report contract (4B.4):** `sprt` now composes the same
ordinary-engine, independent per-side controls, adjudication, resources,
optional books, seed and recoverable run-directory adapter as `match`. Human,
JSON, final-result and run-record output retain model, hypotheses, alpha/beta,
Wald bounds, LLR when defined, cap, official pentanomial sample, terminal or
invalid pair and separately persisted post-terminal evidence. Process exits
are H1=0, H0=1, configuration=2, error=3, inconclusive=4 and invalid=5.
Run-record schema 2 adds required command-specific workflow evidence.

**Implemented parity contract (4B.5):** the hermetic Phase-4B corpus replays a
reviewed, opening-diverse Fastchess 1.8.0 normalized-SPRT stream pair by pair.
For the exact `[4, 4, 2, 0, 0]` terminal sample and matched `[-50, 50]`, 5%/5%
design, both implementations first accept H0 at pair 10 and Colosseum matches
Fastchess's displayed LLR/bounds precision. Prefixes below Colosseum's minimum
sample or with zero variance remain typed non-verdicts; they are not coerced
into external smoothing semantics. A bounded same-binary live smoke against
Fastchess and Cute Chess agrees on eight games, four reversed-colour pairs,
0/8/0 W/D/L, draw termination and zero faults; Fastchess and Colosseum also
agree on `[0, 0, 4, 0, 0]`. The zero-variance smoke deliberately excludes Elo,
interval, LOS and sequential-statistics presentation. Exact artifacts,
commands, hashes and exclusions are in
`tests/fixtures/statistics/phase-4b-parity.toml`.

**Phase 4B exit evidence (4B.6):** analytic and compatible external parity now
pass together. Synthetic ascending, interleaved and reverse worker completions
produce identical H1 and H0 terminal pair IDs, samples and post-terminal cuts.
The strict fault matrix covers timeout, crash, disconnect, protocol and illegal
move; infrastructure failures remain non-scorable and never enter statistics.
Finite-cap inconclusive, configuration, invalid and infrastructure exits plus
the complete H1/H0/inconclusive/invalid exit map are executable gates. All live
differences are root-caused in `docs/architecture/phase-4b-exit.md`, and the
binding gate inventory is `docs/fixtures/phase4b/acceptance.json`.

**Exit:** analytic/oracle verdict parity; concurrency cannot change the terminal
pair; capped/invalid/H0/H1 exits pass; live disagreements are root-caused before
SPSA builds on the runner.

### Phase 4C — Optional calibration

Implement 5.3 over the trusted runner. **Exit:** representative configuration
round-trips; byte mismatch is rejected; PASS/FAIL/INCONCLUSIVE/INVALID fixtures
and one real-machine smoke run behave as specified. **Accepted:** hermetic
resume/config/outcome gates and the recorded Basilisk smoke pass; evidence is
versioned in [`docs/fixtures/phase4c/acceptance.json`](docs/fixtures/phase4c/acceptance.json).

### Phase 5 — SPSA

Implement the exact 5.5 algorithm, loop closure, plan and status. **Exit:**
formula/RNG/rounding properties and every hard audit pass; recovery preserves
the exact stream; synthetic convergence smoke test passes; plan arithmetic and
diagnostics match fixtures; the tune result feeds `sprt --apply` unedited with
the original executable hash.

**Accepted (5.10):** the exact schedule/RNG/rounding and written-artifact
properties, complete hard-audit matrix, pair-atomic fault policy, kill/resume,
factual plan, read-only diagnostics and unedited hash-verified gate loop pass as
one hermetic suite. A seeded noisy two-dimensional quadratic finishes within a
declared 5.0 RMSE band; this checks the mechanism and is explicitly not a chess
convergence forecast. Criterion owners and the real-engine evidence boundary
are recorded in [`docs/architecture/phase-5-exit.md`](docs/architecture/phase-5-exit.md)
and [`docs/fixtures/phase5/acceptance.json`](docs/fixtures/phase5/acceptance.json).

### Phase 6 — Speed, planning, replay, books and position suites

Specs 5.6 + 5.9 + 5.10 + 5.12. **Exit:** wall-clock/fixed-node and skew-bias
regressions pass; scaling sweep fixtures pass; book slicing is reproducible;
every golden fixture replays; fixed/SPRT planning matches fixtures; EPD suite
and baseline compatibility tests pass.

**Accepted (6.9):** fixed-node authority and fake-NPS resistance, left-skew
robust summaries, cold/warm process behavior, scaling/Hash arithmetic, seeded
book tools, replay authority, fixed/SPRT planning, PGN telemetry and durable
position suites pass as one hermetic gate. Criterion owners and the real-engine
evidence boundary are recorded in
[`docs/architecture/phase-6-exit.md`](docs/architecture/phase-6-exit.md) and
[`docs/fixtures/phase6/acceptance.json`](docs/fixtures/phase6/acceptance.json).

### Phase 7 — Tournaments

Spec 5.7. **Exit:** round-robin and gauntlet schedules/ratings match the GUI;
kill/resume is identical with deterministic stubs.

**Accepted (7.3):** a frozen GUI-origin fixture matches every round-robin and
two-seed-gauntlet pairing and reproduces joint ratings within 0.01 Elo.
Deterministic interrupted and uninterrupted runs produce identical schedules,
standings, error bars and crosstables for both formats, with every scheduled
game committed exactly once. Criterion ownership and the real-engine evidence
boundary are recorded in
[`docs/architecture/phase-7-exit.md`](docs/architecture/phase-7-exit.md).

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

**Accepted (8.1):** the 0.1.0 release candidate at commit
`86fc42b442d0f2a354a1fcc1ec5c09cad47a0f43` was compared on Windows with
FastChess 1.8.0-alpha and Cute Chess 1.5.1 using the same hashed Rarog binary.
All three runners agreed on the oracle matrix's shared game count, complete
pairs, colour reversal, W/D/L, draw ratio, termination and fault fields;
FastChess and Colosseum also agreed on the pentanomial vector. The capped-SPRT
exit and zero-variance presentation differences are classified and excluded,
with executable, artifact and raw-result hashes frozen in
[`docs/architecture/phase-8-parity.md`](docs/architecture/phase-8-parity.md).

**Decided (8.2):** UCI pondering is adopted for 1.0 as an explicit, recorded,
default-off clock-test condition across every game-playing CLI workflow. It is
rejected for fixed movetime/nodes/depth because those controls have no
opponent-clock budget. Harness-side Syzygy adjudication is useful but deferred
until its full correctness boundary can be implemented; Chess960 was deferred
here and became a non-goal in Phase 10 (S4).
Additional tournament formats, output formats and a dedicated datagen command
are declined for 1.0 because the current static formats and JSON/PGN/CSV plus a
normal durable `match` cover the demonstrated general engine-development
requirements. The rationale and revisit triggers are binding in
[`docs/architecture/phase-8-gap-decisions.md`](docs/architecture/phase-8-gap-decisions.md).

**Accepted (8.3):** the exact-candidate external parity, reasoned divergences,
complete six-item gap audit, adopted ponder protocol and workspace regression
are one owned acceptance gate. The resulting 1.0 boundary is recorded in
[`docs/architecture/phase-8-exit.md`](docs/architecture/phase-8-exit.md).

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

**Accepted (9.0):** the dated web, same-domain, GitHub, crates.io and
preliminary TMview revalidation found the known collision risk unchanged.
Colosseum and Colosseum CLI are retained for the 1.0 product family; public
introductory surfaces use a chess-engine-testing qualifier, and the executable
remains exactly `colosseum-cli`. [ADR-0009](docs/architecture/adr/0009-retain-colosseum-for-1-0.md)
records the evidence, accepted risk, legal limitation and future revisit
triggers. No alias or speculative branding layer is introduced.
- **(a) Documentation placement analysis.** Decide where user documentation
  lives: in-repo `docs/` published via a static site, a GitHub wiki, or
  generated command reference plus a handful of guides. Criteria: versioning
  with the binary (a wiki does not version, which matters once `stats_version`
  exists), discoverability, offline availability, contribution friction, and
  whether the command reference can be generated from the argument parser so it
  cannot drift. Record the decision.

**Accepted (9.1):** canonical CLI documentation is repository-versioned
Markdown under `docs/cli/`, rendered on the web from the selected tag and
included in exact release archives for offline use. A small unpublished Rust
tool generates the complete public command reference from the real Clap model
and provides a required drift-check mode. Handwritten guides own concepts and
examples; a separate wiki or static-site source tree is not introduced for V1.
[ADR-0010](docs/architecture/adr/0010-version-cli-documentation-with-the-binary.md)
records the placement, generation and publication contract.
- **(b) Write it.** README stays the front door for the whole project —
  what Colosseum GUI and Colosseum CLI are (or their Phase 9.0 replacement),
  install, links. User documentation covers
  the CLI in depth: quickstart, command reference, run-file and tune-file
  reference, worked examples per command, a "how to trust a result" page drawn
  from S3 Tier C, and a compatibility page (what the tool needs from a UCI
  engine, and what it does with non-conforming ones). Explain that engines are
  launched as separate processes and tell users to consult the relevant licence
  terms; do not make a blanket legal conclusion.

**Accepted (9.3):** `docs/cli/` now contains the quickstart, generated complete
command reference, per-command examples, run/tune format references,
result-trust guidance and explicit UCI compatibility/failure contract. The
unpublished `colosseum-docs` workspace tool renders the real Clap model and CI
rejects drift. Documentation review also found that Phase 2.4's tested run-file
resolver had never been connected to the executable; the public `--run-file`
and `--unset-run-option` adapter now expands inheritable TOML through that
resolver into the ordinary parser, with command-line precedence and declaring-
file-relative paths. An integration fixture proves its normalized configuration
and hash equal the equivalent all-CLI invocation.
- **(c) Ship.** Per Phase 0(c)'s release model; all supported platforms; use
  the identity accepted at Phase 9.0 and its dated
  web/GitHub/package-channel/preliminary-trademark screen;
  smoke-test exact archives (`--version`, `--help`, `self-test`, one
  deterministic JSON workflow, and architecture/dependency inspection).
  Before merge, push the intended `cli` candidate with `[cli candidate]` in
  its commit subject (or manually dispatch once the workflow already exists on
  `main`): `release-cli.yml` retains an unpublished candidate bundle identified
  by commit SHA and workflow-run ID, but creates no tag or GitHub prerelease.
  Ordinary `cli` pushes leave the candidate jobs skipped. Steps 9.5–9.7 use
  those archives; rerun the candidate after any change that affects the CLI or
  its package. After acceptance, merge `cli` to `main`, tag the resulting
  stable source with `cli-v<version>`, and let the workflow rebuild, smoke and
  publish the final archives. Normal push/tag CI remains the test gate and is
  not duplicated inside the packaging workflow.

**Accepted (9.4):** unpublished candidate `0.1.0` at commit
`22aefa8a4374405f7cedbcf2d1baf09066f9ebe7` passed required debug/release CI and
exact-archive smoke on the four supported build targets. Workflow run
`31199592962` retained the checksum-verified aggregate without creating a tag
or GitHub Release. The complete identity, archive hashes and acceptance record
are in
[`docs/architecture/phase-9.4-candidate.md`](docs/architecture/phase-9.4-candidate.md).

**Accepted (9.5):** Rarog commit `8f35647` and Basilisk commit `3cbf90b`
remove their duplicate runner, statistics, SPSA, affinity, recovery, NPS,
datagen and tool-vendoring implementations. Both retain only declarative
Colosseum profiles/tune vectors and explicitly classified engine-owned build,
correctness, profiling and Texel responsibilities. The audit found and closed
the missing externally selectable one-sided resignation policy; no generic
mechanism exception remains. The complete mapping and verification record is
in
[`docs/architecture/phase-9.5-coverage.md`](docs/architecture/phase-9.5-coverage.md).

**Accepted (9.6):** a clean-room Windows usability exercise drove two public
UCI Stockfish binaries, supplied only as executable paths, through the
published guides' fixed match, intentionally capped one-pair SPRT and
one-iteration SPSA flows.  All game workflows completed with zero faults; the
SPRT's exit-4 inconclusive result, missing-book warning and SPSA rail warning
were clear and documented.  The current-source local archive is usability
evidence only: commit `89d24a0` changed the CLI after the Phase 9.4 candidate,
so a fresh four-platform CI candidate is mandatory before Phase 9.7.  Exact
commands, identities, hashes and triage are in
[`docs/architecture/phase-9.6-usability.md`](docs/architecture/phase-9.6-usability.md).

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

**Accepted (9.7):** corrected final candidate
`823b398a273ae5631c24e32b4bbfec5b3b35749f` (workflow `31213773139`) passed
checksums and exact-archive smoke for all four supported packages. Every archive
contains the CLI-only README, product changelog and identical 24-file offline
guide, with local links checked from the extracted package. The Linux
executable is byte-identical to the prior engine-tested candidate; its Rarog
and Basilisk gates carry forward. The rebuilt Windows executable repeated both
gates, and all four accepted results match eight draws, four complete pairs,
pentanomial `[0,0,4,0,0]`, adjudicated-draw termination, zero faults and the
expected capped-inconclusive exit. The independent `0.1.0` version,
`cli-v0.1.0` tag contract and product-owned release notes validate. Exact
identities and hashes are retained in
[`docs/architecture/phase-9.7-release-acceptance.md`](docs/architecture/phase-9.7-release-acceptance.md).

**Release publication moved behind Phase 10** by maintainer decision on
2026-09-17. The 9.7 acceptance evidence stands for the candidate it names and
the same gates are repeated at 10.10 on the corrected candidate.

### Phase 10 — First-release corrections (before `cli-v0.1.0`)

The accepted candidate was reviewed against the validation engines' real
harness policy and a maintainer's release expectations. Nothing found is a
defect in what 0.1.0 promised; each item is a default or a mechanism that is
cheaper to correct before anyone depends on it. The CLI version stays 0.1.0
because nothing has been published. Every step that changes game-playing
behaviour is followed by the recurring "after changing anything that runs
games" procedure at 10.10, once, on the final state.

- **(a) Product-latest release handling.** GitHub keeps one repository-wide
  "latest" release and both release workflows leave it to chance. The CLI
  workflow marks its release `make_latest: false`; the GUI workflow marks a
  stable release latest and a prerelease not. The architecture test asserts
  both. README and product documentation link to product tag lists, never to
  `/releases/latest`.
- **(b) Adjudication off by default** across `match`, `sprt`, `calibrate`,
  `spsa` and `tournament`, per the revised S3 Tier B. Draw and resignation are
  enabled by explicit flags that carry their parameters; the previous `--no-*`
  flags are removed rather than kept as no-ops because nothing has been
  published. `--one-sided-resign-adjudication` requires resignation to be
  enabled. Run files, resolved-configuration hashing, dry-run output, fixtures,
  acceptance tests and user documentation follow; the documentation names the
  common settings of public frameworks for users who want them.
- **(c) Class-aware CPU placement** per the revised S5.2: headroom of one
  physical core, highest-performance class only on hybrid hosts, last-level
  cache domains detected and kept per slot, refusal with a topology-naming
  message where the OS evidence is insufficient. `capabilities` reports class,
  NUMA and cache domains. Fixtures cover a hybrid performance/efficiency host,
  a dual-cache-domain single-socket host, a homogeneous SMT host and a
  no-SMT host; the chosen pool and slot allocation are asserted for each.
- **(d) Game record annotations** per S5.4b, in every game-playing command,
  plus score in the telemetry parser and `stats`.
- **(e) SPSA estimator and staged stop** per the revised S5.5: the final
  centre vector is the default estimator, the tail-window mean is optional and
  recorded, the result schema version is bumped, `spsa status` follows the
  same policy, and `--stop-after-iteration N` requests a clean stop at an
  iteration boundary without touching the stored horizon.
- **(f) Graceful stop** per the revised S5.11 for every durable command: one
  cancellation path through the drivers, bounded grace for in-flight games,
  checkpoint, `cancelled` run status, documented exit code, `status` showing
  the cancelled state. The shared kill/resume suite gains a clean-stop case
  per command; resumed statistics equal an uninterrupted run.
- **(g) Fixed rating field** per the revised S5.7: `--fixed <index>:<rating>`
  repeatable on `tournament run`, no error bar for pinned participants, fixed
  ratings retained as run inputs, JSON/CSV/text rows labelled accordingly.
- **(h) Book range policy** per S5.9: refusal instead of silent wraparound,
  `--book-wrap` opt-in, dry-run index range, and the datagen recipe in the
  match documentation written against it.
- **(i) Command-layer split and non-goals.** `composition.rs` holds every
  command in one file of more than six thousand lines; split it into one
  module per command with no behaviour change, guarded by a byte-identical
  generated command reference and unchanged tests. Record Chess960 as a
  non-goal in S4 and the compatibility page; a Chess960 FEN or option request
  is refused with a clear message.
- **(k) Shared game slots** per the revised S5.2: `--cores-per-game` as the
  default allocation without `--ponder`, `--cores-per-engine` as the explicit
  disjoint alternative and the only one accepted with `--ponder`, pool
  arithmetic per mode, mode and allocations in run record and dry-run,
  fixtures asserting 15 shared one-thread slots on a 16-core single-class
  host and 7 disjoint ones, user documentation. Found when a validation
  project's one-thread gate at concurrency 14 was refused on 16 cores.
  **Verified live 2026-09-17** on the 16-core, two-cache-domain host: a
  14-slot match sampled every second showed all 28 engine processes on 14
  distinct physical cores (masks `0x3` through `0xC000000`, cores 0–13) from
  four seconds after launch, core 14 spare and core 15 the headroom; a
  process is unpinned only for the sub-second window between spawn and the
  verified apply, before its handshake. A one-thread engine pinned to a
  core floats between that core's SMT siblings, so a system monitor shows
  every logical CPU active at about 45% utilisation; that is the expected
  picture, not a placement failure.
- **(l) Pair identity in PGN and run-directory telemetry** per the revised
  S5.4b and S5.10: identity tags on every written game, `stats <pgn>`
  reconstructing pairs and the pentanomial vector from them, `stats
  <run-dir>` reading its own `games.pgn` for telemetry, explicit zero fields
  accepted by the parser, the annotated fixture regenerated. Found when
  `stats` on a live run directory reported telemetry unavailable and `stats`
  on its PGN lost pair identity.
- **(m) Review defects**, each with a regression test: a match or SPRT
  interrupted while its last unit is being joined must report its completed
  or cap-reached verdict, not `cancelled`; `suite` must exit with the
  cancelled code when it writes `cancelled`; `--stop-after-iteration N` must
  be idempotent on resume against the cumulative iteration count; the stop
  grace period is one bounded period from the interrupt, not restarted per
  commit; `--anchor` together with `--fixed` on the same participant is a
  configuration refusal (exit 2) at resolution and in dry-run.
- **(n) Pair-identity replay defects** found by review of (l), each with a
  regression test: `stats` must pair games by the `PairNumber` and
  `PairGame` tags, never by game-number arithmetic, so a tournament
  encounter with any `--games-per-pair` replays to the checkpoint's vector;
  post-terminal SPRT pairs and invalid SPSA iterations are marked in a PGN
  tag the replay honours, so the official vector from a PGN equals the
  checkpoint's for a run that crossed a boundary at concurrency above one;
  tournament games carry the encounter's real `OpeningIndex`; the SPSA
  schedule artifact's `stats_version` field is renamed to what it holds
  (the RNG version).
- **(o) Unscorable games in the PGN**, found by review of (n): a game the
  runner aborted for an infrastructure fault is still written to `games.pgn`
  with a result and identity tags but no `ColosseumSample` tag, so a PGN
  replay scores it while the checkpoint does not. Tag it `unscorable`, let
  the replay exclude it and report the count, and cover it with a
  run-directory-versus-PGN equality test on a run with one aborted game.
  Also correct the `rng.rs` doc comment that still ties sampling to
  `stats_version`, and keep the versioned schema refusal reachable for old
  SPSA schedule and result files instead of a raw unknown-field error.
- **(p) Progress reports.** A long run must tell the operator where it
  stands without spamming the console. Progress is counted in the run's
  own unit, as fastchess and cutechess do with `-ratinginterval N` games
  and as an SPSA driver does per iteration: every durable command prints
  one progress block on standard error every `--progress-every N` units
  (pairs for `sprt`, `calibrate` and `spsa`-free paired runs, games for
  `match` and `tournament`, iterations for `spsa`; defaults 10 pairs,
  20 games, 1 iteration) plus a final block at termination. A time floor
  (`--progress-min-secs`, default 5) coalesces blocks when units complete
  faster than that, so a fixed-node run cannot flood the console; a block
  is never printed on every commit and never delayed past the next unit
  boundary once the floor has elapsed. `--progress-interval-secs` is
  removed. The block
  carries what a decision needs: for `sprt` and `calibrate`, games and
  pairs done, W/D/L and the pentanomial vector, the point estimate with its
  95% interval in the run's Elo model (both models for SPRT), LLR against
  its bounds, fault and time-loss counts, pairs per hour and, for SPRT, the
  expected remaining games at the current drift; for `match`, score, Elo
  with interval and rate; for `spsa`, iteration and percentage, elapsed and
  a linear ETA, the last mini-match pair score, the current gain and
  perturbation scale, and the three centres that moved most in absolute
  range units since the previous report; for `tournament`, games done, the
  current standings header with ratings and error bars, rate and ETA. The
  same block is what `status` prints for the run. The append-only `run.log`
  records every block so a console can be closed and the trajectory
  recovered. Found when a maintainer stopped a real SPRT because the console
  showed only pair counts.
- **(q) Commit path off the game loop, and headroom from the bottom.**
  Found on 2026-09-18 by a 30,000-game identical-binary calibration and a
  2,000-game fixed match on the 16-core host: 57 and 14 time forfeits where
  fastchess had produced none for the same engine at the same control. In
  every forfeit the engine's last `info` line reported 14–17 ms of search
  while the harness charged 165–608 ms (median 327 ms). The cause is the
  per-game commit: the driver's synchronous completion callback rewrites
  the whole checkpoint with `fsync` and truncates and rewrites the whole
  `games.pgn`, on the async runtime thread, after every game; both files
  reached 167 MB, so late in a long run each commit blocks the runtime for
  hundreds of milliseconds and the `bestmove` of another game waits behind
  it, because charged time is read when the game task reaches the line,
  not when the line arrived. Required: (1) the `bestmove` timestamp is
  taken by the pipe reader at line arrival, in the UCI session, so no
  harness work after arrival can be charged to an engine; (2) a run
  directory holds four files with distinct jobs: `games.jsonl`, an
  append-only journal with one checksummed record per game (identity,
  result, termination, fault kind, clock summary; never a PGN);
  `games.pgn`, appended one game at a time and never rewritten;
  `checkpoint.json`, constant size, aggregates only (counts, official
  prefix, LLR, SPSA centre and iteration, the journal offset and running
  hash it covers), two generations, written every K units or every few
  seconds and on stop; and `run.log`, human events only (progress blocks,
  faults, stop and resume), never a game. Resume loads the checkpoint,
  verifies the journal to the recorded offset by hash, drops a torn last
  line and replays only the tail, so commit cost is O(1) per game and the
  official prefix and iteration boundaries are recomputed, never pooled.
  Durability is group-committed: appends without sync, one `fsync` of the
  journal and PGN every K units or one second and at every checkpoint and
  stop; a hard kill loses at most that window, which resume simply does
  not have. In-memory driver state holds summaries, not PGN text; (3)
  every file write, sync and rename runs on a blocking thread
  (`spawn_blocking`), never on a runtime worker; (4) `auto`
  headroom is taken from the lowest-numbered cores upward, so CPU 0, where
  Windows services most interrupts, is never a game core; (5) the fault
  policy documents that time losses count inside engine faults, and the
  default for `calibrate`, `match` and `tournament` becomes a documented
  non-zero fraction with the count reported, while `sprt` and `spsa` keep
  invalidation because their samples are pair-atomic. Success criteria: a
  30,000-game stub run's per-commit wall time is flat from first to last
  game; the charged-time distribution in scrambles (remaining under 100 ms)
  on the real host shows no move charged above the engine's reported time
  plus 20 ms in a 2,000-game 3+0.03 match; the fixed-movetime outlier
  probe (100 ms, 14 slots, 50,000 moves) is at or below fastchess's rate.
- **(r) Placement per platform.** Linux can reserve cores where Windows can
  only restrict: detect `/sys/devices/system/cpu/isolated`, `nohz_full` and
  IRQ affinity, prefer isolated cores in `auto` when present, never require
  them, and record what was found. macOS stays advisory as recorded. WSL is
  a virtual machine with synthetic topology and cannot supply placement or
  latency evidence; the Linux evidence needs a native boot. Deliverable: a
  research note with the per-platform contract and the measurements that
  justify it, then the implementation as its own step if the note says so.
- **(s) Writer hardening**, from review of (q): the writer queue is bounded
  so the game loop cannot run unboundedly ahead of the disk (a full queue
  blocks the committing task off the runtime, never a runtime worker); a
  writer failure still leaves a terminal run record on disk through a
  direct atomic write, never `running`; a pre-10.9g run directory is refused
  with the `--restart` guidance rather than a bare schema message; an
  over-long protocol line is a per-read fault that keeps the session
  draining, not a permanent termination; an unscorable game keeps its pair
  or iteration class in the journal beside the `unscorable` mark so replay
  cannot drop the rest of an iteration. Each with a regression test.

  **Implementation evidence (Phase 10.9i):** the writer queue is a
  `sync_channel` of `WRITER_QUEUE_CAPACITY` (256) commands. A send tries
  first; when the queue is full it waits inside `block_in_place` on a
  multi-threaded runtime, so the committing task hands its worker to the
  other games before it blocks (outside a runtime, or on a current-thread
  one, it simply waits; the writer drains on its own thread). The regression
  runs a one-worker runtime with the writer held and a queue of two: the
  committing task stays blocked while a ticker task keeps running, and with
  `block_in_place` removed the same test fails on the stopped ticker. A
  terminal run record is written through `replace_and_report`, which waits
  for the writer's answer; a failed writer, which writes nothing after its
  first failure, answers with that failure and the recorder then writes the
  record directly and atomically with a `writer-failed` anomaly. Both
  `finish` and the dropped-owner `aborted` path are tested against a writer
  whose replace failed. A checkpoint of an earlier schema is now a distinct
  `EarlierLayout` error that names the directory and the `--restart`
  remedy, preferred over the bare checksum/schema pair; the test downgrades
  a real run's checkpoints, removes its journal, sees the guidance and no
  bare schema message, then restarts it successfully. An over-long protocol
  line is skipped to its newline by the reader thread and reported as one
  `Overlong` event; the read that meets it fails as a protocol fault and the
  reader goes on, so later lines still arrive (unit test with the long line
  spanning many buffer refills) and the session still answers (`self-test`
  now requires `isready` to succeed after the fault); only an I/O failure
  or end of stream ends the reader. Unscorable games are journalled under
  the class of their pair or iteration with `scorable: false`, through one
  helper used by `sprt` and `spsa`, and `match` and `tournament` keep
  `official`; the PGN tag stays `unscorable`, and the journal replay in
  `stats` sets such a game aside as `unscorable`, so the 10.9e equality
  between a run directory and its PGN holds. The regression rebuilds an
  SPSA iteration containing an unscorable game from its journal records and
  gets both pairs. Deviation: journal lines written by 10.9g–10.9j with
  `sample: "unscorable"` still read, but a resume cannot place those games
  in their pair; a directory from those builds should be restarted if it
  holds one.
- **(t) Round-trip latency instrument.** After (q) the commit stall is gone
  and the run directory stays small, but the real-host 3+0.03 forfeits did
  not move (15 in 2,000 games at 14 shared slots; 3 and 4 in 1,000 at 7
  disjoint and 7 shared; fastchess 0 in 2,000). Every forfeit position
  replayed directly against the bare engine answers inside its limit, and
  in each forfeit no `bestmove` reached the reader before the deadline, so
  the time is lost in the harness-to-engine round trip, not in the search
  and not in core sharing. Guessing stops here. Required: per move, the
  runner records a phase breakdown with monotonic stamps taken by the
  thread that performs each step: `go` stamped, `go` write returned, first
  `info` arrived, last `info` arrived with the engine's own reported
  `time`, `bestmove` arrived, `bestmove` consumed by the game task. The
  journal record keeps per-side maxima of each phase and of
  `charged − engine time`; `games.pgn` gains `h=<ms>` (harness overhead,
  charged minus engine-reported time) beside `t=`; a forfeit forensic
  prints the full breakdown of its last five moves. `stats` reports the
  overhead distribution per engine (p50/p99/p999/max) and the count of
  moves whose overhead exceeded the margin. Success criterion: on the
  16-core host at 14 slots, a 2,000-game 3+0.03 match names the phase
  that carries every overhead above 20 ms; the fix for that phase is its
  own step and is not guessed here. Corrections owed from review of the
  instrument, to land with that fix step: a `ponderhit` search charges from
  the `ponderhit` write while the engine reports time since `go ponder`,
  so `h=` is wrongly negative under `--ponder`; the forfeited move's own
  overhead is recorded in the journal maxima but not in `games.pgn`, so
  the `stats` over-margin count misses the one move that matters; the
  late-`bestmove` wait stops on a per-read fault; PGN `h=` truncates to
  milliseconds while the journal keeps nanoseconds; an SPSA resume over a
  rebuilt iteration with an unscorable game reports a checkpoint mismatch
  instead of naming the game.
- **(u) A slot belongs to one game at a time.** The instrument of (t) named
  the phase (all harness phases under 1.3 ms; every forfeiting `bestmove`
  42–64 ms after the engine's own hard cap, a second mode and not noise)
  and a maintainer's system monitor named the cause: cores dropping to idle
  while others ran, where fastchess keeps every core at 100%. All four
  drivers choose a game's CPU slot by arithmetic,
  `(number − 1) % slots.len()` (`match_runner`, `sprt_runner`,
  `spsa_driver`, `tournament_driver`), not by which slot is free. Games end
  at different times, so the next game is routinely placed on a slot that
  is still playing while a finished slot idles. Replaying that rule over a
  real 2,000-game run reproduces its wall time and gives 7.8% of slot-time
  double-booked, 7.6% idle, and 71% of games sharing their CPU with another
  game at some point; two searches on one pinned CPU alternate in scheduler
  quanta, which is the 50 ms stall. fastchess on the same host, day and
  binaries: 0 forfeits in 2,000 games; Colosseum: 14–15. Required: one
  free-slot pool per run, owned by the execution plan. A unit takes a slot
  before its first engine is spawned and returns it only after both engine
  processes of its last game have exited; the unit is a game for `match`,
  `calibrate` and `tournament`, and a colour-reversed pair for `sprt` and
  `spsa`, whose two games run on the same slot. Launch order, pair identity,
  opening assignment and commit order are unchanged: only the CPU placement
  of a unit changes, so results are reproducible from the same seed except
  for timing. The slot a game ran on is recorded in its journal record and
  as a PGN tag. Invariant, asserted in debug builds and tested: no two live
  units ever hold the same slot, and no unit waits while a slot is free.
  Test: a stub run with deliberately uneven game lengths, concurrency 4,
  that fails on the modulo rule. Success criterion on the real host: the
  2,000-game 3+0.03 match at 14 slots shows zero time losses and every game
  core continuously busy. **Demonstrated 2026-09-18** on the 16-core host:
  2,000 games at 14 whole-core slots, 0 time losses and 0 engine faults
  (fastchess the same day: 0; Colosseum before the pool: 14–15, and 206 with
  one logical CPU per game, the dose-response of double-booking); the journal
  shows 0 overlapping slot spans and a median hand-over gap of 0 ms; the run
  was stopped and resumed once through the journal; with under 100 ms on the
  clock the slowest charged move was 81 ms against fastchess's 95 ms. Elo
  +65.9 ± 11.0, nElo +93.7 ± 15.2 against fastchess's +54.3 ± 11.1 and
  +75.6 ± 15.2 on the same binaries.
- **(v) A rare forfeit must not void a sequential test.** With the pool in
  place a real `[0,10]` SPRT was invalidated at pair 6 by one time loss: last
  `info` at 21 ms, the engine's own cap at 39 ms, `bestmove` at 80.6 ms
  against a deadline of 80.5 ms, no overlapping slot span, no other process
  starting or exiting within 950 ms. One such event in 2,046 post-pool games
  is indistinguishable from fastchess's zero in 4,000, and by the PGN metric
  (scramble moves charged 25 ms past the engine's hard cap) fastchess shows
  0.44–0.69 per 1,000 scramble moves where Colosseum shows none, so the
  harness is not the outlier; an operating system occasionally holds a
  process for tens of milliseconds. A zero allowance therefore voids any
  long SPRT or tune: at this rate a 20,000-game test would die about ten
  times. fastchess and fishtest score a time loss as a loss and continue.
  Required: for `sprt` and `spsa` an engine-attributable forfeit is scored
  as the loss it is, its pair stays in the official sample in order, and the
  run becomes invalid only when faults exceed a documented rate (default
  0.5% of games played, minimum 3, evaluated continuously), with the count
  and rate in every progress block and the final report; `--max-time-losses
  0` restores strict invalidation. Infrastructure faults stay unscored and
  still invalidate the pair.
- **(w) Residual time losses: parity with fastchess.** The record so far, all
  on one 16-core host, the same two binaries, `3+0.03`, one thread, 14
  concurrent games, 20 ms margin: fastchess 0 time losses in about 4,400
  games and 0 in its 30,000-game identical-binary run; Colosseum 14–15 per
  2,000 before (u), 206 in 529 with double-booked single logical CPUs, and
  2 in about 2,500 since (u). If fastchess had Colosseum's present rate,
  its zero would be roughly a 3% event, so a residue probably remains.
  **Ruled out by measurement:** the engine's search (every forfeit position
  replayed against the bare engine answers inside its limit, and ordinary
  aborted iterations land within half a millisecond of the engine's own
  hard cap); the harness round trip (`go` write, first `info`, arrival lag
  and consumption all under 1.3 ms across 4,000 game-sides); the commit
  path (q); double-booked slots (u); process start and exit storms (no
  other boundary within 950 ms of a post-(u) forfeit); core sharing (3
  against 4 per 1,000 at 7 disjoint and 7 shared slots, measured before
  (u)); harness thread starvation (pinning the harness to the spare core
  changed nothing). **Not the cause, and kept:** CPU placement. With
  placement off the fixed-movetime probe showed more late moves, not fewer
  (6.2 against 4.2–4.7 per 1,000), and an unpinned host carries a hidden
  per-run offset of about ±10 nElo, which biases verdicts silently where a
  forfeit is counted and visible. **The signature:** a `bestmove` that
  arrives 40–60 ms after the engine's own hard cap, a second mode and not
  scatter, with the engine process itself held. **Untested differences
  from the engine's point of view:** Colosseum starts fresh engine
  processes for every game where fastchess keeps them alive between games
  (cold caches, first-touch faults on the hash, image and heap setup on
  every game); engines run inside a kill-on-close job object; the mask
  allows both SMT siblings where fastchess pins one logical CPU; process
  creation flags, priority class and pipe construction (anonymous against
  named, buffer sizes, overlapped against blocking reads); the clock
  messages themselves (Colosseum games spend far fewer moves under 100 ms
  than fastchess games, 8,600 against 32,000 per 2,000 games, so the two
  harnesses do not hand the engine the same clock trajectory). The
  desktop application, unpinned and on the older clock model, is reported
  never to forfeit the same engines; its incident logs are evidence to
  count, not an explanation. **Required, as research before any fix:**
  (1) a source study of fastchess recorded in
  `docs/architecture/fastchess-mechanics.md`: engine process lifetime and
  restart policy, process creation and priority, affinity application,
  pipe and reader design, exactly where its clock starts and stops, how
  `timemargin` is applied, what it sends between games, and every
  mechanism a match runner of this kind has that Colosseum lacks, each
  marked adopt, decline with a reason, or test; (2) a count of time
  forfeits in the desktop application's incident logs over a large pool;
  (3) a discriminating run for each untested difference above, 2,000 games
  each and maintainer-run, changing one thing at a time, with the late-mode
  metric (scramble moves charged 25 ms past the engine's hard cap) and the
  forfeit count as the two readings; (4) only then the fix, as its own
  step. Success criterion: 0 time losses in 10,000 games at 14 slots on the
  reference host, or a documented operating-system floor that fastchess
  shares. This does not block use: a symmetric forfeit per thousand games
  moves an estimate by a small fraction of its error bar, and (v) keeps a
  sequential test alive through one.
- **(x) An SPSA iteration must fill the machine.** Measured on a real 60-
  iteration tune at 14 slots, 32 games per iteration, mean game 8.9 s: mean
  iteration 38.4 s where 32 games on 14 slots need about 20 s, slot occupancy
  inside an iteration 53%, 2,900 games per hour against 5,500 for a fixed
  match. Cause: (u) made the colour-reversed pair the slot-holding unit for
  `spsa` as for `sprt`, so 16 pairs on 14 slots run as 14 pairs back to back
  and then 2 more while 12 slots idle, and the update must wait for them.
  For `sprt` the pair stays the unit. For `spsa` it buys nothing: both arms
  play in every game and slots are identical by construction. Required:
  within an iteration every game takes a free slot individually, in schedule
  order; the two games of a pair may run on different slots and at the same
  time; the iteration is still committed atomically and the gradient still
  uses whole pairs, reassembled by pair identity; a fault policy decision is
  still made on the whole mini-match. `spsa plan` and the dry run report the
  wave shape for the resolved concurrency (games per wave, slots idle in the
  last wave, expected occupancy) and warn when games per iteration is not a
  multiple of the slot count, naming the nearest even multiples; its
  wall-time estimate uses waves. Test: a stub iteration with more pairs than
  slots and uneven game lengths whose slot occupancy is asserted, and which
  fails on pair-held slots. Success criterion on the reference host: a tune at
  14 slots and 42 games per iteration, or 15 slots and 30, holds occupancy
  above 85% and exceeds 5,000 games per hour. **Deferred to after the
  release, as research:** asynchronous SPSA in the fishtest manner, where the
  next games start on free slots with the current parameters and results are
  applied as they return; it removes the iteration barrier at any mini-match
  size but updates on slightly stale parameters and gives up the one
  iteration, one update record, so it needs its own evidence that it reaches
  the same optimum for less game budget. **Found while verifying (x),
  2026-09-18:** the launch loops deep-copied the opening book per launched
  game (`spsa`) or pair (`sprt`), about 0.45 s and hundreds of megabytes with
  a 2.6-million-position book, on the one task that launches games: SPSA
  games started 0.44 s apart (6 s to start 15), and `sprt` ran at 4,460 games
  per hour where `match` reached 5,500. The book is now shared behind an
  `Arc`; 15 launches land within 1 ms and measured occupancy is 80–86%
  against the 81% predicted.
- **(y) SPSA at the scale of a real tune.** A 60-iteration run left a 6.2 MB
  `result.json`, about 100 KB per iteration, because the driver keeps every
  game's full record in memory for the whole run and writes them all at the
  end: over 500 MB and as much memory at 5,333 iterations. Required: the
  result carries per-iteration summaries (centres, arm vectors, pair score,
  faults) and refers to the journal for games; driver memory is bounded by
  the iteration in flight, not by the run; a 5,000-iteration stub tune shows
  flat per-iteration commit time, bounded resident memory and a result under
  a stated size. A launch-spread regression test for `spsa` and `sprt` with
  a large synthetic book (all launches of a wave within a few milliseconds)
  so no per-launch deep copy can return, and an audit of every value cloned
  per launch. The budget may be given in games (`--total-games`), from which
  the iteration count is derived for the chosen mini-match size, so a
  registered budget survives a change of size; `spsa plan` reports hours from
  the wave model. The opening book is held compactly (offsets into one
  buffer rather than an owned string per field) and loads in well under a
  second. `status` on a run that has printed no block yet says when the first
  is due.

  **Implementation evidence (Phase 10.9o):** an SPSA iteration is kept as a
  summary — centres before and after, the arm vectors, the pair score, its
  `faults` and `games` (the first and last game numbers of its journal lines)
  — and the observer receives the iteration's games beside it once, to
  journal them; the driver then drops them, so what it holds is the
  iteration in flight plus one small summary per committed iteration.
  `result.json` gained a document `schema_version` (2) and `games:
  "games.jsonl"`; `tuned_result` and its version 3 are unchanged, so
  `sprt --apply` reads old and new results alike. Resume rebuilds summaries
  from the journal (pair identity, scorability and score are checked there;
  the driver then checks each summary's iteration and game range), `spsa
  status` and the progress blocks read summaries (a resumed tune's faults now
  count in its blocks), and `stats` on an SPSA directory finds no game array
  in the result and reads the journal. A hidden `--__synthetic-games` switch
  plays games in-process as instant results for scale tests. A 5,000-
  iteration, 4-game tune under it: median commit cycle 3,421 µs over the
  first tenth and 3,406 µs over the last, resident memory 16.6 MB early and
  18.8 MB late, `result.json` 5.66 MB (bound 8 MB, about 1.1 KB per iteration
  of a one-knob tune against about 100 KB before); the test asserts the cycle
  within 3× + 2 ms, growth under 16 MB and the result under 8 MB. Launch
  audit: per launched game or pair the drivers clone two engine launch
  specifications, time controls, adjudication, a slot allocation and the
  `MatchOpenings` handle; the book was the only large value and is shared, and
  `OpeningList` is no longer `Clone`, so a deep copy of a book does not
  compile. The launch-spread test runs an SPSA iteration and an SPRT at four
  slots on a 200,000-line book and requires the first wave's launches within
  10 ms (measured 66 µs and 1 µs); with a per-clone copy of the book
  restored it measured 324 and 328 ms and failed. `--total-games` (on `spsa`
  and `spsa plan`, exclusive with `--iterations`) derives the horizon for the
  chosen mini-match, refuses a budget it does not divide and names the
  nearest two (160,000 at 42: 159,978 or 160,020), and is stored in the
  resolved configuration only when given, so existing configurations hash
  as before; the plan's estimate is printed in hours from the wave model.
  The book is `OpeningList`: one text buffer and a 24-byte entry per opening
  (`[FEN][moves][label]` offsets, the EPD label being the FEN itself), the
  shuffle permuting entries with the same RNG and swaps, EPD lines validated
  in parallel chunks cut at line boundaries and joined in file order; the
  test compares 60,000 lines through the parallel path and a PGN book with
  the previous owned loader for sequential, random and counted orders, and a
  2.6-million-line book loads in 212 ms in an optimised build (the test
  bounds 1 s there, and a tenth of the book within 10 s in a debug build).
  `status` on a run with no block yet says when the first is due, from a
  `progress` entry (every, unit, floor) each command now records in its
  workflow evidence. `run_game` prepares both engines with `tokio::join!`;
  a setup failure keeps its own side's error and forensics, and an engine
  that spawned but failed setup is still reaped before the game returns;
  the regression plays two games against an engine that fails its
  handshake and finds the fault on Black, then White, with the broken
  engine's forensic each time. Deviations: the result's own version is new
  (`SPSA_RESULT_SCHEMA_VERSION` 2) rather than a bump of the tuned-result
  version, which did not change; a run's retained summaries still grow with
  its iterations, at about 0.6 KB each; memory is measured from the working
  set on Windows and `VmRSS` on Linux and not asserted elsewhere; the scale
  test's games are synthetic, so it measures everything but engine play.
- **(z) Two games per physical core, as a measured experiment for tuning
  only.** A one-thread game keeps one logical CPU busy; the core's SMT sibling
  idles. Placing a second game on the sibling doubles the concurrent games
  (30 on the 15 game cores of the reference host). SMT usually returns 20–25%
  more total work, so each engine runs at roughly 60% of its speed: for the
  engines it is the same experiment at a shorter effective time control on
  slower hardware. It cannot bias a tune, because both arms are the same
  binary under identical conditions, but it departs from the rule that a
  tune runs under gate conditions, so whether its result transfers is the
  question, not whether it is faster. Required: an explicit, recorded
  placement mode (`--games-per-core 2`, default 1) accepted by `spsa` and
  `match` only and refused by `sprt`, `calibrate` and `tournament`; each game
  pinned to exactly one logical CPU, the two games of a core on its two
  siblings, never two games on one logical CPU; refused where the sibling map
  is unavailable or the core has no sibling; mode in the run record, PGN tag
  and dry run. Evidence, maintainer-run on the reference host, same binaries
  and book: (1) throughput and per-engine speed, as games per hour and the
  nodes-per-second ratio against one game per core; (2) forfeits and the
  late-move metric over 2,000 games; (3) transfer: the same short tune run in
  both modes from the same seed and budget, each gated by `sprt --apply` under
  ordinary one-game-per-core conditions. Adopt for tuning only if (3) shows
  the sibling-mode tune gates no worse, and record the measured wall-time
  saving; otherwise decline with the numbers. Never a gate condition.
  **Deferred behind the release by maintainer decision, 2026-09-18:** the
  transfer evidence costs two tunes and two gates, and two tunes cannot be
  compared by their values. A ten-minute pre-check needs no code: two
  matches started together, one with `--placement 2,4,…,30` and one with
  `--placement 3,5,…,31`, 15 games each, give the real throughput gain and
  the per-engine slowdown; if the gain is small the step is declined.
- **(aa) Corrections from review of (y), and a smaller result.** On a
  resumed tune the journal records loaded for the replay stay resident for
  the rest of the run (`composition/spsa.rs`, bound in the outer scope of
  the run function): drop them after the replay, and extend the scale test
  to a resumed run. `result.json` still stores, per iteration, the plus and
  minus values, perturbation signs and gain coefficients of every knob,
  about 16 KB per iteration at 82 knobs and some 200 MB pretty-printed for
  a full tune, all derivable from the schedule, the seed and the centres:
  store the centres after each iteration, the pair score and the faults,
  bump the result schema, keep `spsa status` and `sprt --apply` working.
  `book stats` and the desktop scheduler's `load_openings` still materialise
  the whole book; a self-comparison in `spsa_driver` checks nothing; `stats`
  refuses a `result.json` given as a file; a run file's `iterations` clashes
  with `--total-games` on the command line instead of being overridden. The
  maintainer's 60-iteration tune at 15 slots and 30 games per iteration is
  the throughput evidence for (x). **Measured 2026-09-18:** 1,800 games,
  23.9 s per iteration, 4,517 games per hour (weather-factory on the same
  surface 3,720; Colosseum before (x) and the shared book 2,745); slot
  occupancy inside an iteration 83% against 81% modelled; all 15 launches of
  a wave within a millisecond; no gap between iterations; no overlapping
  slot spans; no time loss or other fault at 15 slots. **Qualified
  2026-09-18:** every one of its 1,800 journalled games ran at the default
  2,000 ms margin (`--margin-ms` was not given), so its zero time losses
  are not comparable with a 20 ms run and say nothing about (w).

  **Implementation evidence (Phase 10.9q):** the driver keeps, and
  `result.json` stores, one `SpsaIterationSummary` per committed iteration
  (iteration, centres after, pair score, faults, journal game range) and an
  `SpsaInvalidSummary` (iteration, faults, reason, game range);
  `SPSA_RESULT_SCHEMA_VERSION` is 3. The arm vectors, signs, gains and
  centres before still reach the observer once, in the full
  `SpsaCommittedIteration`, for the journal and the progress block; the
  replay returns the last iteration in full for `spsa status`. Measured on
  the real 82-knob Rarog tune: 26.1 KB per iteration pretty-printed in
  version 2 (the 15-slot run's `result.json`), 2.2 KB in version 3; the
  5,000-iteration one-knob stub result fell from 5.66 MB to 2.46 MB. The
  dead self-comparison was the resume check: summaries rebuilt by the
  replay were compared with game ranges computed by the same function, and
  then replayed through `SpsaTuningState::resume` against histories that
  replay had just produced. Resume now recomputes each summarised iteration
  from the schedule and its stored score and refuses one whose centres
  after, score or number it does not reproduce (unit test: a moved centre,
  a changed score and a renumbered iteration are each refused at the
  iteration where they diverge). A resumed tune drops the journal records
  once the replay has built its summaries; the scale test gained a resume
  case (stop at 4,500 of 5,000 iterations, resume, same summaries as an
  uninterrupted tune from the same seed) that measured the resumed process
  2.8 MB above the uninterrupted one and 13.7 MB above it with the records
  kept, and asserts 8 MB. `load_openings` returns the compact
  `OpeningList`; the desktop scheduler materialises only the opening each
  scheduled game draws, `summarize` reads the first label in place, and
  `book stats` counts duplicates over borrowed (FEN, move text) pairs and
  plies in one pass; `into_resolved` is gone. `stats` given a result that
  names its journal (`"games": "games.jsonl"`) reads the run directory it
  belongs to, and a test requires the same report from the file and the
  directory. A run file's `iterations` yields to `--total-games` on the
  command line and `total-games` to `--iterations`, as a repeated option
  does. Real smoke on the release build: an 82-knob tune of 3 iterations of
  30 games at 15 slots and 1+0.01, stopped after 1 and resumed; 90
  journalled games, no fault, no overlapping slot span, `stats` identical
  on `result.json` and on the directory. Found in passing and left for
  10.10: `killed_match_resumes_missing_games_in_deterministic_schedule_order`
  fails about one run in eight at this commit and at its parent alike,
  when the match completes between the test's liveness check and its kill.
- **(ab) Qualification on a real engine is part of the release, and lives
  here.** A project that adopts the harness must be able to trust a
  released binary without re-testing it, so the evidence that the harness
  measures correctly belongs to this repository's release acceptance, run
  by the maintainer on the reference host with a validation engine and
  recorded in `docs/architecture/phase-10-qualification.md` with commands,
  binary and input hashes and results. On the release candidate: (1)
  **symmetry**, an identical-binary `calibrate` of 30,000 games with the
  whole 95% normalized-Elo interval inside ±5; (2) **scale**, a fixed
  2,000-game match of two builds whose difference a second runner has
  measured, intervals overlapping; (3) **verdict**, an SPRT of a pair the
  second runner has gated, same verdict; (4) **ratings**, a gauntlet with a
  fixed field whose anchors keep their ratings; (5) **tuning, the recovery
  test**, which replaces comparing two full tunes: two tunes of a wide
  surface end at different noise around a flat region, so their values
  cannot be compared, and their strength differs by less than a feasible
  match resolves, whereas a tune with a known answer can fail. Three or
  four high-sensitivity parameters are detuned far enough to cost at least
  30 Elo in a fixed 2,000-game match against the defaults, every other
  parameter fixed; `spsa` runs from the detuned start for about 30,000
  games at full concurrency; pass means every detuned parameter ends at
  least half way back to its default and `sprt --apply` of the tuned values
  against the detuned start accepts H1 at `[0,10]`. A sign, scale or
  schedule error fails it outright. (6) **faults**, zero time losses in the
  scale match, or the floor (w) documents. Already on record from
  2026-09-17/18, to be repeated on the candidate only where the code under
  test changed since: scale +55.7 to +65.9 against a second runner's +52.2
  and +54.3; verdict H1 at 220 pairs, nElo +94.1 ± 32.5, against 216 pairs
  and +94.0 ± 32.8; symmetry −1.1 ± 3.9, measured before the slot pool and
  therefore owed again. Item (j) does not close without this record.
  **Release order from here:** (aa), then (w) the fastchess study and what
  it concludes, then (ab) inside (j). Item (r), placement per platform, is
  recommended for deferral behind the release with the limitation stated in
  the release notes, since the reference platform is Windows; the
  maintainer decides.
- **(ac) Harness time that belongs to neither clock.** The same tune missed
  its predicted 4,860 games per hour by 7%, and not through the engines:
  moves per game (122) and charged time per game (8.6 s) were identical to
  the 14-slot run, and harness overhead inside the charged interval was
  unchanged (`h=` median 1 ms, 99th percentile 21 ms). What grew is the
  time per game outside both clocks, slot span minus charged time: 0.30 s at
  14 pair-held slots, 1.22 s at 15 per-game slots, 2.5 against 10 ms per
  move. It costs no fairness and 9% of throughput. Established so far: it
  grows with concurrency in short runs (6.9, 12.9 and 21.5 ms per move at
  8, 14 and 15 slots); raising the harness's priority class changes nothing
  (15.4 against 14.2), so it is not CPU scheduling; one engine reaches
  `readyok` in 18 ms alone and in 190 ms inside a burst of thirty, and since
  the shared-book fix every wave starts thirty processes in the same
  millisecond, which explains part of it and not all. Required: the journal
  records, per game, start-up (slot taken to first `go`), play (first `go`
  to last `bestmove`), the uncharged part of play, and teardown (last
  `bestmove` to both processes exited); `stats` reports their distribution;
  then the largest phase is reduced and the measurement repeated. The
  candidate mechanism, shared with (w), is what fastchess does: keep each
  slot's two engine processes alive between games (`ucinewgame`, options
  re-sent when they change, as an SPSA iteration does), restarting on any
  fault, on request, and always between different executables, with fresh
  processes per game remaining selectable for crash isolation. Success
  criterion on the reference host: uncharged time under 0.3 s per game at
  15 slots, and no change in the `h=` distribution or the fault rate.

  **Implementation evidence (Phase 10.9s):** `run_game` stamps the game's
  start, each search's start and return, the end of play and both engines'
  exit, and the clock accounting carries `phases` (`GamePhases`): start-up,
  play, charged, uncharged play and its three parts (the runner's work
  between searches, the `position` written before each `go`, a `bestmove`'s
  arrival to its search returning), and teardown. The journal records them
  per game and `stats` on a run directory or journal reports each phase's
  mean, p50, p90, p99 and maximum, with `outside-runner` (slot span less the
  three) beside them. The instrument named the phase at once: 30 games of
  b22core against b22base at 3+0.03 on 15 slots gave start-up mean 1,537 ms
  (the fifteen games of the first wave 2,661–2,835 ms each, later single
  starts 171–557 ms), uncharged play 16 ms and teardown 10 ms per game. A
  bare start of the same engine outside the harness took 25 ms alone and
  218–224 ms median in a burst of thirty, so the harness, not the engine,
  held most of it. Stamps inside the start then put 0.5–0.8 s of each
  engine's start in applying its CPU affinity, and none in process
  creation, the job object, resume or the reader thread (under 0.1 s
  together): the check that the engine's threads sit in the requested
  processor group took a system-wide thread snapshot
  (`CreateToolhelp32Snapshot`) per engine, synchronously on a runtime
  worker, so thirty at once serialised and held up other games' handshakes
  (half the handshakes read 0 ms, the other half 0.7–1.0 s). The check now
  asks the process itself (`GetProcessGroupAffinity`, which the allowed-CPU
  detection already used for the harness's own process); the requirement
  is unchanged. The same burst after the change: affinity 0.0 ms, start-up
  211–222 ms mean, which is the bare engines' own figure. On the real
  82-knob tune (3 iterations of 30 games at 15 slots, 1+0.01, 20 ms margin,
  seed 11), uncharged time per game from slot span less charged time fell
  from median 1,537 ms, mean 1,417 ms to median 215 ms, mean 236 ms, and
  the three iterations from about 30.6 s to 21.7 s. Test: a fixture engine
  that waits before `uciok` and after `quit` puts those delays in start-up
  and teardown, play equals charged plus uncharged exactly, and `stats`
  reports all nine phases; with the exit stamp moved before the quits the
  test fails on teardown (0.8 ms against 120 ms injected). Persistent engine
  processes, the candidate, were not built: the remaining start-up is the
  operating system's own cost of starting two processes, about 0.2 s of a
  9 s game, and the target is met without giving up per-game isolation.
  They stay a discriminating test of (w). **Confirmed 2026-09-18** by the
  maintainer's tune at 15 slots, 30 games per iteration, 3+0.03 and a 20 ms
  margin, otherwise the 15-slot run of (aa) repeated (same engine, tune,
  seed 1530, book and schedule): uncharged time per game from slot span less
  charged time median 231 ms, mean 241 ms, max 934 ms against 1,378, 1,224
  and 2,792 ms; start-up mean 150 ms, p99 285 ms; uncharged play 19 ms and
  teardown 12 ms per game; 22.0 s per iteration and 4,910 games per hour
  against 23.9 s and 4,517; `h=` p50 1 ms and p99 21 ms per arm in both
  runs, p999 139–159 ms against 142–147 ms, maximum 597 against 584 ms; 0
  time losses and 0 other faults in 1,800 games, now at a 20 ms margin; no
  overlapping slot span. About 1% of moves (1,137 and 1,132 per arm of some
  108,500) carried an overhead above the 20 ms margin and forfeited nothing,
  since none of them spent the whole clock; the same overhead tail was
  present before this step and is evidence for (w), where `h=` also holds
  the engine's own time after its last reported `time`.
- **(ad) Overlapped SPSA iterations, a recorded mode that is off until its
  evidence is in.** The iteration tail is the last structural loss: 17% of
  slot-time at 30 games on 15 slots, because the next iteration may not
  start until the last game of this one has returned. It exists only in
  `spsa`; `match`, `sprt`, `calibrate` and `tournament` already hand every
  freed slot to the next unit for the whole run and have nothing to gain.
  The mode: when a slot frees and the current iteration has no game left to
  launch, launch the next iteration's games at once, perturbed around the
  newest committed centre, which is one update behind; updates are applied
  strictly in iteration order from whole mini-matches; staleness is bounded
  at one update and recorded per iteration; `--overlap 1`, default 0;
  resume replays whole iterations as today. Why the cost should be small:
  one update moves a centre by `c × r × score`, a few percent of the
  perturbation `c` early in a tune and about one percent late, so a gradient
  measured around the previous centre is measured a small fraction of the
  probe distance from where it is applied; fishtest tunes this way with far
  larger staleness. Why that is not yet evidence here: fit quality is the
  purpose of a tune and outranks throughput. Required before the default
  may change: (1) a zero-game study on noisy synthetic objectives (separable
  and coupled quadratics, 80 parameters, the real schedule and mini-match
  noise), synchronous against overlapped over many seeds, reporting final
  distance to the optimum and its spread; (2) the recovery test of (ab) run
  in both modes from the same detuned start and budget, each gated by
  `sprt --apply`; adopt as the tuning default only if overlapped recovers no
  worse in both. Expected gain: occupancy from 83% towards 97%, about 15%
  more games per hour. Items (k) to (ad) precede (j), with (ad) allowed to
  follow the release if its evidence is not in.

  **Implementation evidence (Phase 10.9n):** `play_mini_match` now takes a
  slot from the run's pool per game: games are launched in schedule order
  (`2p − 1`, `2p`, … across the iteration's pairs), each on the lowest free
  slot, through `match_runner::play_pair_game`, which builds a game from its
  number exactly as `play_pair` does (colours, opening, identity, arms); the
  pair's second game no longer waits for its first. Returning games are
  held by pair identity until both halves are in, then handed to the same
  `PairCommitQueue`, so the iteration still commits atomically, the score
  and gradient still use whole pairs in pair order, the fault decision is
  still taken on the whole mini-match, and a stop still replays the whole
  interrupted iteration. The per-slot invariants of (u) hold per game (debug
  assertions on take, give-back and held count; a slot returns only when
  `run_game` has returned after both engine processes exited). `sprt` keeps
  `play_pair`, now two calls of `play_pair_game` on its one slot, with the
  pair as its unit. `spsa plan` and `spsa --dry-run` report
  `SpsaWaveShape` for the resolved concurrency: games per wave, waves, games
  and idle slots in the last wave, expected occupancy (games over
  slot-waves), and a warning naming the even multiples of the slot count
  between half and twice the request (14 slots, 32 games: 28, 42, 56; 15
  slots: 30, 60); the wall-time estimate already counted game waves and now
  matches the scheduler it describes. Test: `phase10_slot_pool` runs two
  iterations of five pairs on four slots with uneven stub games and asserts
  the per-slot invariants, launch order, whole pairs per iteration, at
  least two pairs whose games ran at once on different slots, and occupancy
  of at least 60% (measured 78–88% over three runs). With the pair-held
  scheduler restored it failed two runs of two, on launch order (game 3
  before game 2) — pair-held occupancy measured 71–76%, so on this stub the
  occupancy bound alone does not separate the two; the order and
  concurrency assertions do. Unit tests cover the wave arithmetic.
  Deviation: the dry run carries the shape as an optional `wave_shape`
  field of its document and prints no separate warning line, because its
  human form is that document; a live run prints none, so its standard
  error stays progress only. Owed and maintainer-run: a tune at 14 slots and
  42 games per iteration, or 15 and 30, above 85% occupancy and 5,000 games
  per hour.

  **Implementation evidence (Phase 10.9l):** `FaultPolicy` gained an
  optional `rate` (`FaultRate`: per mille, and which of the engine-fault and
  time-loss limits grow) and three methods, `engine_limit(games)`,
  `time_limit(games)` and `exceeded(faults, games)`, through which every
  check now passes. `FaultPolicy::sequential(engine, time)` is the policy of
  `sprt` and `spsa`: an omitted engine-fault limit grows as
  max(3, ⌊games × 5 / 1000⌋); an omitted time-loss limit follows it, because a
  time loss is an engine fault; a limit given explicitly stays fixed, so `0`
  invalidates on the first fault of its kind (and an explicit engine limit
  also fixes the defaulted time limit). `match`, `calibrate` and `tournament`
  keep their fixed 10.9g allowance. `sprt` already scored a forfeit as a
  result and kept its pair in the official sample; it now evaluates the
  allowance at each admitted pair over the games of the official sample so
  far, this pair's included, and marks the pair invalid only past it. `spsa`
  gained `--max-engine-faults` and `--max-time-losses`; the driver sums faults
  over every committed iteration (a resumed tune's included) and, within the
  allowance, commits an iteration containing forfeits with its score and
  gradient like any other; the iteration that crosses the limit is kept as
  invalid evidence as before. A rebuilt iteration may now hold engine
  faults; only an unscorable game is refused on resume. Every progress block
  and final report states the count, its rate over the games played and the
  allowance at that point, for example `2 in 812 games (0.25%); 4 allowed
  (0.5% of games played, at least 3)`; the policy is in the resolved
  configuration and the report. Tests: the policy arithmetic (floor, rate,
  boundaries, strict and time-only explicit limits); an SPRT against a
  fixture that forfeits every game becomes invalid at pair 2 (four faults
  over four games) and at pair 1 with `--max-engine-faults 0`; an SPSA tune
  with the same fixture commits iteration 0 with its two forfeits scored and
  becomes invalid at iteration 1; the two acceptance tests that assert
  invalidation on the first fault now pass `--max-engine-faults 0`.
  Deviation: the fault policy's serialized shape changed, so an `sprt` or
  `spsa` run directory from before this step no longer matches its resolved
  configuration on resume and must be restarted.

  **Implementation evidence (Phase 10.9k):** the execution plan hands each
  run a `SlotPool` (`MatchExecutionPlan::slot_pool`), and `match`
  (and so `calibrate`), `sprt`, `spsa` and `tournament` take the
  lowest-numbered free slot before spawning a unit and give it back when the
  unit's worker returns. A worker returns after `run_game`, which returns
  only once both engine processes have exited: the normal path quits and
  waits, and the setup-failure path now kills and reaps an engine that had
  spawned instead of dropping it. A pair (`sprt`, `spsa`) holds one slot for
  both games. Launch order, pair identity, openings, commit order and the
  official prefix are untouched; only which slot a unit is placed on
  changed. `plan_execution` and `slot_pool` refuse a plan whose slot count
  differs from its concurrency (`SlotCountMismatch`). Debug builds assert
  that a slot handed out is free, that a slot given back was held, and that
  the held count equals the live units after each launch. Every game records
  `slot: {index, started_unix_us, ended_unix_us}` in its journal record and
  report (from before the first spawn to after both exits), `GameSlot` in
  its PGN and `slot:` in its forensic. Tests: a pool unit test drives units
  finishing in arbitrary order and checks that no slot is held twice and
  none waits while a slot is free, plus the debug assertion and the
  slot-count refusal; `phase10_slot_pool` runs `match` (32 games), `sprt`
  (16 pairs), `spsa` (2 iterations of 6 pairs) and a tournament (3 engines,
  4 games per pair) at concurrency 4 with stub engines whose per-process
  delay factor makes games uneven, and checks from the journal that no two
  spans on one slot overlap, that every slot is used, that a pair's games
  share a slot, and that the PGN tag matches. With the modulo rule restored
  in `match_runner` the match case failed three runs of three (game 5 put on
  slot 0 while game 1 still ran), and in `sprt_runner` two of two.

  The corrections owed from review of (t) landed with it. A `ponderhit` or
  `stop` search is marked `earlier_origin`: the engine's clock started at
  an earlier command, so neither overhead nor last-`info` lag is computed
  and `h=` is omitted for it. `h=` is now the journal's nanosecond overhead
  rounded to the nearest millisecond (halves away from zero), so a game's
  largest `h=` per side equals its journal maximum rounded; `t=` stays
  truncated, so `h` and `t − engine time` agree to within one. A search that
  lost on time is written before the result as `{forfeit t=…ms h=…ms}`, or
  `{forfeit}` when no answer came, and `stats` counts it towards the side
  that forfeited in the overhead distribution and over-margin count and in
  nothing else (`colosseum-move-comment/3`). The late-`bestmove` wait
  continues past a per-read protocol fault; the regression puts an
  over-long line just before a late answer and still times the answer. An
  SPSA resume over a rebuilt iteration holding an unscorable or faulted game
  now names the iteration and the game (`UnusableJournalGame`) and gives the
  `--restart` remedy. Deviations: the pool is created per run by the
  execution plan rather than stored inside it, since the plan is a
  serialized report; the slot spans use wall-clock microseconds so a run's
  journal can be read as a timeline, which a clock step during the run
  would distort; the stub's unevenness comes from its process identifier,
  so the lengths are uneven but not reproducible run to run; the
  real-engine smoke test's `GameSpec` literals, missing `identity` since an
  earlier step, were repaired. Owed and maintainer-run: the 2,000-game
  3+0.03 match at 14 slots with zero time losses and every game core
  continuously busy.

  **Implementation evidence (Phase 10.9j):** `colosseum-uci` keeps a
  `SearchTiming` per search: the game task stamps `go` before the write, the
  write's return, and the moment it takes each line off the reader's
  channel; the reader thread's existing arrival stamp times the first and
  last `info` and the `bestmove`; the last reported `time` and the `time` on
  the last `info` are kept with them. Nothing was added to the reader
  thread, and the game task only copies `Instant`s. `ponderhit` and `stop`
  are timed the same way; a ponder that finished early has no round trip
  and no timing. The stamps survive a missed deadline: a line that arrived
  after it is kept, and the runner then waits up to one second more
  (`LATE_BESTMOVE_WINDOW`), only to learn when the `bestmove` came; the
  result is already decided. The runner keeps per side the maximum of five
  phases — `go` write, to first `info`, first to last `info`, last `info` to
  `bestmove` arrival, arrival to consumption — plus the last `info`'s lag
  behind the time it reported and the overhead (charged minus reported
  time); the maxima travel in the clock accounting, so the journal record
  carries `white_round_trip` / `black_round_trip`. The forfeited search
  counts, with its late arrival. Every abnormal-game forensic now begins
  with a table of each side's last five searches in milliseconds after the
  `go` stamp, `!` marking a late answer and `never` a missing one. The PGN
  comment gains `h=<ms>ms` beside `t=` (`colosseum-move-comment/2`), and
  every game carries `WhiteTimeMarginMs` / `BlackTimeMarginMs`; `stats`
  reads `h=` back into a per-engine p50/p99/p999/max distribution and the
  count of moves whose overhead exceeded that side's margin. Tests: the
  fixture engine gained per-phase delays (`--first-info-ms`,
  `--between-info-ms`, `--bestmove-after-info-ms`, `--report-time-ms`); a
  session test injects 30/40/50 ms into the three engine-side phases and
  blocks the game task's only thread while the answer arrives, and each
  delay lands in its own phase with the consumption delay uncharged; a
  match journals the same maxima for the delayed side only, writes `h =
  t − 35` on every delayed move and `stats` reports every one of those moves
  over a 20 ms margin; a forfeit forensic shows the late answer at its real
  arrival. The PGN writer and the parser round-trip `h=` including negative
  values. Deviations: the forensic prints the last five searches of each
  side rather than the last five plies, so the forfeiting side always shows
  five; the margin tags are new, because an over-margin count from a PGN
  alone needs the margin; the `go` write phase is measured but not injected
  in a test, because a pipe write blocks only on a full OS buffer, which no
  portable test controls; the shared runner gives GUI games the same `h=`,
  tags and forensic tables. Owed and maintainer-run: the 2,000-game 3+0.03
  match at 14 slots that names the phase.

  **Implementation evidence (Phase 10.9g):** the `bestmove` instant is now
  the pipe's. A dedicated OS thread per engine owns its standard output,
  reads it line by line with the existing length bound, and stamps each
  line with `Instant::now()` the moment it is read off the pipe; the
  session receives `(text, arrived)` over a channel and charges
  `arrived − start`, where `start` is stamped before the `go` (or `stop`,
  or `ponderhit`) is written. A deadline is judged by arrival too: a line
  that arrived in time is in time however late the game task reads it. The
  clock model is versioned `go-write-to-bestmove-arrival` 2, because the
  start stamp moved from after the write to before it (a reader thread can
  otherwise stamp a reply before the harness finished flushing the
  command). The shared UCI session carries this, so GUI games are charged
  the same way. A run directory now has four files with one job each.
  `games.jsonl` holds one line per game — line version, sequence number,
  the game record (identity, result, termination, fault, sample class,
  clock accounting, iteration or round), the offset, length and SHA-256 of
  its moves in `games.pgn`, and a SHA-256 over the line's canonical JSON —
  and never PGN text. `games.pgn` is opened for append and written one
  game at a time, moves before the journal line. `checkpoint.json`
  (`CHECKPOINT_SCHEMA_VERSION` 2, two generations) holds aggregates only
  and a `journal` anchor: byte offset, SHA-256 of the covered bytes, record
  count and the PGN offset. `run.log` holds progress blocks, faults, stop,
  resume and finish; the per-game event is gone. One writer per run
  (`RunWriter`) runs on `spawn_blocking` and does every append, sync,
  checkpoint, rename and final artifact; the game loop hands it a record
  and the moves over a channel and returns, and a driver strips a game's
  moves as soon as the observer has handed them over, so in-memory state
  holds summaries. Appends are unsynced; the three append-only files are
  synced together every 50 games or 1 second and at every checkpoint and
  barrier. Checkpoints come every 50 units or 5 seconds and at every stop.
  Resume loads the newest valid checkpoint, hashes the journal up to its
  offset and refuses on mismatch, reads the tail line by line, drops a torn
  last line, drops tail games whose moves are not in `games.pgn` with the
  recorded hash, truncates the PGN to the last kept game, and replays the
  kept records: the SPRT official prefix, a tune's iteration boundaries and
  each command's aggregates are recomputed from them. `status` adds the
  checkpoint's aggregates and the journal read-only (games, how many the
  checkpoint covers, what a resume would drop, or the refusal); `stats`
  on a run directory reads the journal. `auto` headroom now sorts the
  eligible cores by their lowest logical CPU and leaves the lowest ones
  free, so CPU 0 is never a game core; the placement unit tests and both
  headroom fixtures of the recorded topology corpus moved by one core.
  `match`, `calibrate` and `tournament` default `--max-engine-faults` to 1%
  of the scheduled games and at least 5, `--max-time-losses` defaults to
  the engine-fault limit because a time loss is an engine fault, and
  `calibrate` classifies `invalid` only past that allowance; `sprt` keeps
  zero and `spsa` still invalidates on any fault. Every progress block's
  and final report's `faults` line states the allowance. Tests: a
  30,000-game commit through the match observer into a real run directory
  measured a median observer call of 900 ns at both ends, 365 ms and 371 ms
  per 1,000 games including the writer and its syncs, and a checkpoint of
  483 then 497 bytes; a hard kill of a real match followed by a torn last
  journal line and a PGN cut inside the previous game resumes to the
  uninterrupted result with the durable journal bytes unchanged; one
  changed byte inside the covered journal is refused by resume and
  reported by `status` without either changing the directory. The Phase
  4B oracle fixtures, the 10.9d/10.9e run-dir-versus-PGN equality tests
  and the kill/resume suite pass unchanged in what they assert; their
  waits now watch the journal rather than the checkpoint. Deviations: the
  run configuration, the run record's first write and the resume's
  journal load happen before any game starts, synchronously for the first
  two, since nothing is being charged yet; K is an internal constant
  rather than a flag; a run directory written before this step is refused
  on resume with a request to `--restart`, since its checkpoint names no
  journal; the Phase 4C `fault-invalidity` gate in
  `docs/fixtures/phase4c/acceptance.json` now names the allowance test that
  replaced the any-fault test; `RUN_RECORD_SCHEMA_VERSION` is unchanged because the record's
  shape is. Owed and maintainer-run: the real-host scramble probe (a
  2,000-game 3+0.03 match, no move with under 100 ms remaining charged
  above the engine's reported time plus 20 ms) and the fixed-movetime
  outlier probe (100 ms, 14 slots, 50,000 moves, at or below fastchess).

  **Implementation evidence (Phase 10.9f):** one `ProgressBlock` type carries
  every report: a command, its unit count against the cap, the elapsed time
  and labelled lines. The same value is rendered to standard error, appended
  to `run.log` as a `progress` event and retained in the run record
  (`RUN_RECORD_SCHEMA_VERSION` 5), so `status` prints exactly what the console
  last showed rather than a second account of the run. The block is what the
  contract lists per command; the figures come from the estimators the final
  result already uses — `pentanomial_statistics` for both Elo models,
  `pentanomial_sprt` for the LLR and its Wald bounds, `elo_with_error` for an
  unpaired match, and `RateTournament::execute_with_fixed_field` on the games
  committed so far for the standings header. The one derived figure is the
  SPRT's expected remaining games, which extrapolates the computed LLR at its
  average drift per pair, capped at the games left to `--max-pairs` and
  labelled as the current drift because a sequential path is not a line.

  `ProgressSchedule` owns the decision: a block is due at `--progress-every`
  units past the last one, and the `--progress-min-secs` floor can only
  withhold it, never schedule it. A run polls that decision four times a
  second, which is finer than the smallest permitted floor, so the floor and
  not the polling is what bounds how close two blocks may be. A resumed run
  counts from the units it inherited, so no rate or ETA claims time it did not
  spend, and a final block that would repeat the last boundary block is not
  printed twice. A block reads as the operator's own summary: who is playing,
  the sample size, each Elo estimate as a value and the half-width of its 95%
  interval, W/D/L, `Ptnml`, faults, the LLR against its bounds, the rate and
  the time remaining, closed by a rule. The closing SPRT report states the
  hypotheses as an interval, names what was accepted rather than only which
  hypothesis it was, and ends with the invocation's total time. Neither flag reaches the resolved configuration, so an
  existing run directory resumes whatever it is told to report. Machine mode
  prints blocks too, and the tests that read "a quiet run says nothing on
  standard error" now read "nothing but progress".

  **Implementation evidence (Phase 10.9e):** the abandoned game was the one
  game every driver already refused to score and no writer said so. It now
  carries `[ColosseumSample "unscorable"]` from `match` and `calibrate`, from
  `tournament`, and per game inside an SPRT or SPSA pair, where a class that
  describes the group cannot describe one abandoned member of it. The class
  is per game deliberately: an infrastructure fault is a fact about that
  game, not about the pair it was scheduled in. The replay reports what it
  left out — `excluded_games` with an `excluded_by_sample` breakdown — rather
  than dropping it silently, and the structured reader counts its own
  unscorable games the same way, so a run directory and its PGN agree on the
  exclusions as well as on the sample. A gauntlet whose third engine does not
  exist plays one encounter out and abandons the first game of the next, which
  is a run with exactly one aborted game and no timing dependence.

  Both versioned SPSA artifacts deny unknown fields, and deserializing before
  asking for the version made the second rule answer for the first: a stale
  file was reported as "unknown field `stats_version`" and the version that
  renamed the field was never named. Reading `schema_version` on its own,
  before the fields, keeps the refusal reachable for the stored schedule and
  for a tune result offered to `sprt --apply`. `SPSA_PLAN_SCHEMA_VERSION` is 2,
  which 10.9d should have bumped with the field it renamed, and the `rng.rs`
  sampling comment no longer ties the draws to `stats_version`.

  **Implementation evidence (Phase 10.9d):** all four defects were the same
  mistake — the identity was written and the reader guessed anyway. `stats`
  now groups games by `PairNumber` together with the unit its `PairGame`
  falls in, so an encounter of four games is two pentanomial units and an
  encounter of one game is none, and the colour inversion follows every even
  assignment instead of a hard-coded `2`. The structured reader needed the
  same correction: a tournament game names its participants by identity
  rather than by side letter, so before this it scored every game from
  White's perspective and a checkpoint disagreed with its own PGN. Games a
  run keeps but does not count now carry `[ColosseumSample "post-terminal"]`
  or `"invalid"` inside the header — they were marked with a comment line
  above the game, which a PGN reader attributes to the previous game, so the
  marker existed but no reader could act on it. A file in which nothing is
  official is refused with that reason rather than replayed. Tournament games
  no longer have their identity patched into the rendered text: the driver
  supplies the encounter, the assignment and the opening it actually chose to
  the one-game match, which is why `OpeningIndex` was `0` everywhere —
  `select_encounter` hands the inner match a single-entry book. The SPSA
  schedule artifact's field is `rng_version` (schema version 2, tune result
  schema version 3), and the randomness documentation and §5.4 no longer
  attribute the stream contract to `stats_version`.

  **Implementation evidence (Phase 10.9c):** each defect shared one shape,
  treating "a stop was asked for" as "the run did not finish". A match is now
  cancelled only when games are actually missing, and a schedule only when
  pairs remain below its cap, so a late interrupt cannot take away a verdict
  already earned. `suite` exits with the cancelled code whenever it records
  `cancelled`, like every other driver. `--stop-after-iteration N` is checked
  before an iteration is played, against the cumulative committed count, so
  repeating the command is a no-op rather than one more iteration each time.
  The stop grace period has an absolute deadline taken from the interrupt:
  drivers rebuild that future on every completed unit, and a relative delay
  meant a run with several slots never reached it. `--anchor` together with
  `--fixed` on one participant is refused at resolution, before a run
  directory exists, because the two say different things about one rating.
- **(j) Release acceptance repeat.** Regenerate the command reference, update
  `CHANGELOG-CLI.md` under 0.1.0, run the Phase 4B oracle replay and the
  Phase 8.1 parity matrix on the corrected source, repeat the short third-party
  usability flows, build a fresh four-platform CI candidate and pass exact
  archive smoke. The maintainer then merges `cli` to `main` and tags
  `cli-v0.1.0`.

**Exit criterion:** every item above demonstrated by its tests and fixtures;
oracle replay and parity matrix agree on shared fields; the candidate's four
archives pass smoke; documentation, changelog and generated reference are
consistent; the tag contract validates.

### Phase 11 — The GUI on the harness

The GUI still plays games through `colosseum-engine::scheduler` and the SQLite
store, while the CLI plays through its own drivers with placement, pentanomial
statistics, fault policy and durable run directories. Two implementations of
one mechanism in one repository is the "one real cost" of S2 doubled. Phase 11
makes the CLI's drivers the single game-playing mechanism and keeps SQLite as
the GUI's history index. It starts only after `cli-v0.1.0` is published, so
the CLI release is never held by GUI work.

- **(a) Harness library.** Move the run directory, run record, placement
  resolution and the match, SPRT and tournament drivers from `colosseum-cli`
  into a library crate (`colosseum-harness`) with no argument parser and no
  `main`. Add an observer port that publishes per-game live state (board,
  latest search line per side, clocks) and run-level snapshots. The CLI becomes
  a thin composition root over the library; the architecture tests keep GUI
  and windowing packages out of the harness dependency graph.
- **(b) The GUI plays through the harness.** Tournament execution in the GUI
  uses the harness tournament driver: placement, the adjudication default,
  fault classification, `games.pgn` with annotations per tournament and run
  directories under the application data directory. SQLite remains the
  GUI-owned history index (tournament list, names, status, participant
  correlation and the mapping to run directories for resume); it is no longer
  the game store. The live view reads the observer port. **Everything Phase
  10 established reaches the desktop tournaments through this step, with no
  second implementation:** the free-slot pool (u), the arrival-stamped clock
  model and group-committed journal (q), class-aware whole-core placement
  with CPU 0 left free (c), shared or disjoint game slots (k), per-move
  annotations with harness overhead (d, t), pair identity and sample classes
  in the PGN (l, n, o), the fault allowance (q, v), the fixed rating field
  (g), progress blocks (p) and whatever (w) concludes. The desktop keeps its
  own presentation of them; it does not keep its own mechanics.
- **(c) Retire the duplicate scheduler.** Remove `engine::scheduler` and the
  `tournament` feature's game-store execution path. Pre-existing SQLite game
  history stays readable through a read-only migration so the History tab
  still opens old tournaments. Update `CLAUDE.md`, the architecture documents
  and an ADR recording the single-mechanism decision.
- **(d) GUI release.** Ratings on stored data agree with the previous
  implementation within 0.01 Elo; the design guidelines are checked; the GUI
  changelog records the adjudication default change and the new run
  directories prominently. The version is the maintainer's call at this step;
  a major bump is recommended because a shipped default changes.

**Exit criterion:** one game-playing implementation in the workspace; GUI
tournament parity demonstrated on stored data; the CLI's own tests, fixtures
and generated reference unchanged by the extraction; a GUI candidate passes
its archive smoke.

### Post-release research

Not scheduled steps; each needs its own evidence before it becomes one.

- **Asynchronous SPSA** (from Phase 10(x)): asynchronous SPSA in the
  fishtest manner, where the next games start on free slots with the current
  parameters and results are applied as they return; it removes the
  iteration barrier at any mini-match size but updates on slightly stale
  parameters and gives up the one iteration, one update record, so it needs
  its own evidence that it reaches the same optimum for less game budget.

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
| Phase 10 default changes silently alter fixtures | Each change is a numbered step with its own fixture update; oracle replay and parity repeat at 10.10 |
| GUI unification regresses the released GUI | Phase 11 starts after `cli-v0.1.0`; stored-data rating parity and read-only history migration are exit criteria |
| Class-aware placement guesses a processor | S5.2 refuses on insufficient OS evidence and asks for an explicit list; fixtures cover hybrid and multi-domain hosts |

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
