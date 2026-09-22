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

**The CLI 0.1.0 is implemented, corrected and qualified; the first public
release is held behind the last Phase 10 preparation steps.** Phases 0–9
built and accepted it; Phase 10 applied the first-release corrections a
maintainer review found cheaper before anyone depends on published defaults
(adjudication off by default, class-aware placement, per-move annotations,
the final-centre SPSA estimator, graceful stop, the slot pool and the
arrival-stamped clock model, persistent engines per slot, and the rest
recorded in [`docs/architecture/phase-10-record.md`](docs/architecture/phase-10-record.md))
and qualified the result on a real engine. What remains before the tags is
release preparation: versions, one build entry point, user documentation and
the acceptance repeat (S8, Phase 10 open items). The release ships both
products: the CLI as 0.1.0 and the GUI as its own new version from the same
merged source. Phase 11 then moves the GUI onto the same game-playing
mechanism. This document remains the binding design record and maintenance
specification.

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

The workspace today: `colosseum-core` (domain and statistics),
`colosseum-application` (use cases and ports), `colosseum-uci` (protocol and
process management), `colosseum-engine` (the one-game runner both products
use, the GUI's tournament scheduler and SQLite store, OS topology and
affinity adapters), `colosseum-gui` (desktop composition root, owning its
library, config and paths) and `colosseum-cli` (headless composition root
with its own match, SPRT, SPSA and tournament drivers). The GUI and CLI never
depend on each other. The one duplication left is the game-playing driver:
the GUI's scheduler and the CLI's drivers both sit on the same runner, and
Phase 11 removes the GUI's copy. The required suite is hermetic and runs on
Windows, Linux and macOS in debug and optimized profiles.

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

Phases 0–9 and most of Phase 10 are complete. Their per-item rationale and
evidence were moved out of this file on 2026-09-22 and live, verbatim, in
[`docs/architecture/phases-0-9-record.md`](docs/architecture/phases-0-9-record.md)
and [`docs/architecture/phase-10-record.md`](docs/architecture/phase-10-record.md);
the phase exit documents and ADRs beside them hold the acceptance evidence.
This section keeps only what an agent needs to continue: the model routing,
a one-line summary per completed phase, the open Phase 10 items in their
current form, Phase 11 and the post-release list.

### Model routing for numbered steps

The default model for each open `GUIDE.md` step. It is a task-risk
classification, not part of the product contract:

- **Terra High** — the design is settled and the work is bounded by explicit
  invariants, fixtures and an exit criterion. With Claude: Sonnet 5, high effort.
- **Sol High** — the step creates architecture or policy, combines several
  failure domains, or owns mathematical, concurrency, durability or
  cross-platform correctness. With Claude: Opus 5, high effort.

If Terra discovers a material design choice not resolved by this plan, stop
that step and continue it with Sol High rather than improvising the missing
contract. Raising effort beyond High is an explicit exception, not the default.

| Step | Model |
|---|---|
| 10.9w.1 | Sol High |
| 10.9x, 10.9y, 10.10 | Sol High |
| 10.9z | Terra High |
| 11.1–11.3 | Sol High |
| 11.4 | Terra High |

Completed steps keep the assignment recorded on their `GUIDE.md` line.

### Completed phases

| Phase | What it delivered | Evidence |
|---|---|---|
| 0 | Current-state audit, Clean Architecture target, ADRs 0001–0008, the one-repository/independent-release model and the Colosseum naming decision | [record](docs/architecture/phases-0-9-record.md), [current-state](docs/architecture/current-state.md), [target](docs/architecture/target-architecture.md), [release architecture](docs/architecture/release-architecture.md), [ADRs](docs/architecture/adr/README.md) |
| 1 | Pentanomial statistics and normalized Elo in `colosseum-core`, the analytic and external oracle fixture corpus, the hermetic required suite | [record](docs/architecture/phases-0-9-record.md), `tests/fixtures/statistics/` |
| 2 | Boundary migration (runtime participant, application ports), the independently versioned `colosseum-cli` package, run files, run records, run directories, `self-test`, `status` | [exit](docs/architecture/phase-2-exit.md) |
| 3 | OS topology adapters, deterministic placement, hard affinity where the OS allows it, `capabilities` | [exit](docs/architecture/phase-3-exit.md) |
| 4A | Fixed-match runner with explicit clock accounting, fault classification, books and durability | [exit](docs/architecture/phase-4a-exit.md) |
| 4B | Pair-atomic capped SPRT, oracle replay and live parity against fastchess and cutechess | [exit](docs/architecture/phase-4b-exit.md) |
| 4C | Optional identical-binary calibration | [exit](docs/architecture/phase-4c-exit.md) |
| 5 | SPSA kernel, driver, `spsa plan` and `spsa status`, `sprt --apply` | [exit](docs/architecture/phase-5-exit.md) |
| 6 | `nps`, scaling sweeps, `book` tools, `stats` replay and planning, `suite` | [exit](docs/architecture/phase-6-exit.md) |
| 7 | Round-robin and gauntlet tournaments with joint or anchored ML ratings | [exit](docs/architecture/phase-7-exit.md) |
| 8 | Parity repeated on the candidate, ponder adopted, remaining gaps decided | [exit](docs/architecture/phase-8-exit.md), [parity](docs/architecture/phase-8-parity.md), [gaps](docs/architecture/phase-8-gap-decisions.md) |
| 9 | Naming retained (ADR-0009), versioned `docs/cli/` with a generated reference (ADR-0010), README, candidate bundle, coverage and usability acceptance | [9.4](docs/architecture/phase-9.4-candidate.md), [9.5](docs/architecture/phase-9.5-coverage.md), [9.6](docs/architecture/phase-9.6-usability.md), [9.7](docs/architecture/phase-9.7-release-acceptance.md) |
| 10 (a)–(af) | First-release corrections: adjudication off by default, class-aware placement, PGN annotations, final-centre SPSA estimator, graceful stop, fixed rating field, book-range refusal, the slot pool, the arrival-stamped clock model and journal, latency instrument, fault allowance, persistent engines per slot, real-engine qualification and symmetry, the adoption-audit corrections | [record](docs/architecture/phase-10-record.md), [qualification](docs/architecture/phase-10-qualification.md) |

Facts from those phases that still govern new work: the CLI's public
contract is S5; the required suite is hermetic (S6.3); every game-playing
change repeats the oracle replay and parity matrix (`GUIDE.md`, recurring
procedures); `--engine-processes per-slot` is the CLI default and tournaments
keep fresh processes (10(ae)); pre-10.9g run directories are refused;
Chess960 is refused; 10(w) was closed as rejected because the forfeits it
studied were an engine defect, not the harness.

### Phase 10 — open items (before `cli-v0.1.0`)

The corrections are done; what remains is release preparation. The CLI
version stays 0.1.0 because nothing has been published. The recurring "after
changing anything that runs games" procedure runs once, at 10.10, on the
final state.

**Decisions proposed 2026-09-22, for the maintainer to confirm.** The items
below are written on the assumption that they stand; each names the line to
change if they do not.

- **(af.1) The GUI must honour `scorable`** (GUIDE 10.9w.1, Sol High). Found
  by (af): a game whose engine cannot be spawned is classified by the runner
  as an infrastructure fault and reported as an unscorable draw
  (`Termination::Aborted`, `scorable: false`). The CLI excludes such a game;
  the GUI scheduler never reads the flag, so it records the draw into the
  standings and, through the writeback after every finished game, into the
  library Elo. In 1.0.2 the engine that failed to start lost both games.
  Decision: **exclude, as the CLI does** — not 1.0.2's loss and not the
  current draw. Reasons: a process that cannot be created is the host's
  condition (path, DLL, permissions), never the engine's play, so no side has
  earned a result; a loss hands the opponents free points that distort their
  ratings against each other, and a draw does the same at half the weight
  while also writing into the library; exclusion is what Phase 11 arrives at
  anyway, so the desktop changes behaviour once, not twice. An engine that
  spawns and then fails the handshake, crashes, plays an illegal move or
  loses on time still loses, so the old, misbehaving engines in the library
  are unaffected. Scope: `scheduler.rs` skips `standings.record` when
  `scorable` is false and does the same in the DB replay on resume (an
  `Aborted` termination is the stored marker; no schema change), the
  recent-errors text says "not scored", the snapshot's finished count still
  advances so the tournament completes, the live view shows the game as
  aborted; `failed_engine_loses_with_error` becomes
  `failed_engine_is_not_scored` and asserts zero games for both sides and an
  unchanged rating; a store unit test covers the replay path; the GUI
  changelog records it under Changed. **What the user sees:** the game is
  not retried inside the run — a wrong path or missing DLL fails the same
  way every time, so a retry would only repeat the error — and the
  tournament continues with its other pairings; the game is stored as
  aborted with its error, appears in the recent-errors panel and the
  termination counts, and contributes nothing to points, games played,
  head-to-head or ratings, so the engine that could not start shows zero
  games and its opponents' ratings move only on games actually played.
  **Aborted games are re-queued on the next Start:** the resume replay treats
  an `Aborted` row as pending rather than finished, so the user fixes the
  engine's path in the library, selects the tournament and presses Start,
  and the missed games are played then. That is the whole outcome: no
  phantom results, nothing lost, one visible message. **This blocks the GUI
  release** (a known regression that silently alters library ratings must
  not ship) and does not touch the CLI. If the maintainer prefers 1.0.2's loss instead,
  the runner must carry the failed side on `GameFault::Infrastructure`, which
  is a larger change and the reason it is not recommended.
- **(ag) Versions and the tag contract** (GUIDE 10.9x, Sol High).
  - **The GUI's new version is 1.1.0.** (af)'s list has four user-visible
    additions — `colosseum --version`, the `gui-v` update check, the `Fixed`
    CSV column, and per-move search comments plus the `OpeningPlyCount` and
    time-margin tags in every PGN the GUI writes — and (af.1) is a behaviour
    change, which under SemVer is a minor release, not a patch. Nothing is
    removed and the SQLite schema, `engines.json` and the tournament serde
    formats are unchanged, so it is not a major.
  - **The CLI's first version is 0.1.0.** It becomes **1.0.0** when all of
    the following hold: Phase 11 has extracted the harness library, so the
    CLI is a thin composition root over a stable library boundary; the
    deferred (r), (z) and (ad) have each landed or been declined, because
    each changes the run record; and one full CLI minor release after that
    has been used by an adopting project's CI (Rarog) with no change to the
    command surface, run-file schema, run-directory layout, JSON report
    schemas or exit codes. Until then, 0.x minor releases may change those
    formats and say so in the changelog; adopters pin an archive by SHA-256.
  - **The tag scheme is `gui-v<semver>` and `cli-v<semver>`**; the legacy
    `v<semver>` form ends with 1.0.2 (maintainer decision 2026-09-21).
    `colosseum-release`, both workflows and both manifests accept exactly
    that and refuse the rest; GitHub's repository-wide "latest" belongs to
    the stable GUI release and a CLI release never claims it.
  - **Updater** (`crates/colosseum-gui/src/update.rs`): a release marked
    prerelease on GitHub with a clean `gui-v` tag is still offered, because
    only the tag shape is checked. Fix here — filter `prerelease` beside
    `draft`, one test — since this step owns the tag contract. The
    `?per_page=100` limit is recorded as a known limit, not fixed: the
    repository has four releases and adds a handful a year; add pagination
    when the count passes fifty.
  - The manifests, `CHANGELOG-GUI.md` (a `[1.1.0]` section, with the one
    sentence telling a 1.0.2 user to download the new version by hand) and
    `CHANGELOG-CLI.md` agree; `cargo run -p colosseum-release -- gui-v1.1.0`
    and `-- cli-v0.1.0` both pass.
- **(ah) One build entry point** (GUIDE 10.9y, Sol High). A `cargo xtask`
  package under `tools/xtask`, added to the workspace members, with
  `.cargo/config.toml` carrying `[alias] xtask = "run --package xtask --"`.
  It replaces `build_windows.ps1`, `build_linux.sh` and `build_macos.sh`,
  which are deleted in the same step together with the `/dist/` ignore rule.
  The final surface:

  ```text
  cargo xtask build   <gui|cli> [--target <triple>] [--profile release|ci-release]
  cargo xtask package <gui|cli> [--target <triple>] [--format <list>] [--no-smoke]
  cargo xtask release-check <gui-vX.Y.Z|cli-vX.Y.Z>
  ```

  - The product is positional. `--target` defaults to the host triple and is
    always passed to cargo, so every output is under `target/<triple>/`.
    `--locked` is unconditional. `build` compiles exactly one product
    (`-p colosseum-gui --bin colosseum` or `-p colosseum-cli --bin
    colosseum-cli`) and prints the binary path; it copies nothing — the bare
    binary is not a deliverable, the archive is.
  - `package` builds with the `release` profile only (a `ci-release` binary
    is a different binary and must never be archived), stages the product's
    allowlisted contents exactly as the workflows do today, writes the
    archives to `target/dist/` and prints each one's SHA-256, and runs the matching
    `tools/release/Smoke-*Archive.ps1` on each archive it produced;
    `--no-smoke` skips that for local iteration. `--format` defaults to the
    portable archive for the host (`zip` on Windows, `tar.gz` elsewhere);
    CI passes the full set (`gui`: `zip,msi` / `tar.gz,deb,rpm` /
    `tar.gz,dmg`, plus `pkg.tar.zst` in the Arch container; `cli`: `zip` or
    `tar.gz`). A requested format whose tool is missing (WiX, `cargo-deb`,
    `cargo-generate-rpm`, `makepkg`, `hdiutil`) is an error, never a skip.
    The macOS `.app` bundle is assembled here with the version from the
    product manifest, which retires the empty `CFBundleShortVersionString`
    that `build_macos.sh` writes on `cli`.
  - Versions come from `cargo metadata` for the product package, never from
    the workspace manifest. **One naming scheme for both products, readable
    by a user** (maintainer decision 2026-09-22):
    `<product>-<version>-<os>-<arch>.<ext>` with product `colosseum-gui` or
    `colosseum-cli`, os `windows`, `linux` or `macos`, and arch `x64` or
    `arm64` — so `colosseum-gui-1.1.0-windows-x64.msi`,
    `colosseum-gui-1.1.0-macos-arm64.dmg`,
    `colosseum-cli-0.1.0-linux-x64.tar.gz`. This replaces the old
    `colosseum-<version>-…` GUI stem and the `x86_64`/`aarch64` spellings;
    the CLI is unreleased and the GUI's names change with the new lane
    anyway, so nothing published is renamed. The executables inside keep
    their names (`colosseum`, `colosseum-cli`). The change touches the
    staging and platform tables in `tools/release`, both smoke scripts,
    both workflows' expected lists and the README tables, all in this step.
  - `release-check <tag>` runs `colosseum-release` on the tag, the
    documentation drift gate (`colosseum-docs --check`), `git diff --check`,
    and confirms the product manifest version equals the tag's and the
    product changelog has a section for it.
  - Deliberately not copied from Rarog's xtask: architecture tiers,
    `--native`, `--pgo`, `verify-isa`. Rarog's product is its codegen;
    Colosseum ships one portable binary per platform.
  - Both workflows call these commands. `release-gui.yml` additionally gains
    what `release-cli.yml` already has: the publish job downloads named
    artifact patterns instead of `*`, asserts the exact expected file list
    and count (ten files for a stable release: `zip`+`msi` for both Windows
    targets, `tar.gz`+`deb`+`rpm`+`pkg.tar.zst` for Linux, `tar.gz`+`dmg`
    for macOS; the `msi` pair is absent for a prerelease) and publishes that
    list. With the unified stems the two products' names can no longer
    match each other's glob, but the explicit list stays: it is what proves
    a platform job did not silently drop an artifact. Versions stay in
    artifact names: a file identifies itself in a download folder and
    Rarog's `setup_tools.ps1` pins a tagged archive by its SHA-256; the
    `gui-v`/`cli-v` tag-list links are the stable "latest" pointers.
    **No `SHA256SUMS` is published** (maintainer decision 2026-09-22):
    checksums are useless to most users as a download. The workflows keep
    computing and re-checking them between the build and publish jobs and
    in the retained candidate bundle, and GitHub shows each asset's SHA-256
    digest on the release page for anyone who wants to pin.
  - `release-gui.yml` gains a `workflow_dispatch` candidate mode like the
    CLI's: it builds, smokes and retains the ten artifacts without a tag or
    a release. The `gui-v` lane has never run end to end (only legacy `v`
    tags have). The maintainer's own process is to tag, watch, and fix and
    re-tag if something is wrong, which works for a first release nobody
    has downloaded; the candidate mode is the cheap rehearsal that avoids a
    half-published release, and it is worth having for every later one.
  - Exit: on Windows, `cargo xtask package cli` and `package gui` produce
    archives whose contents equal the candidate's file lists and each passes
    its smoke script; `docs/DEVELOPMENT.md` documents the commands and the
    three scripts are gone.
- **(ai) User-facing documentation for the release** (GUIDE 10.9z, Terra
  High). `README.md` is the front door for both products: what each is for,
  which one a reader wants, download per platform with checksum
  verification, a first tournament in the GUI, a first `match`, `sprt` and
  `spsa` with a run file in the CLI, then links to `docs/cli/`.
  `README-CLI.md` (the archive's `README.md`), both changelogs, `docs/cli/`
  and `docs/DEVELOPMENT.md` agree with it. No phase numbers or internal
  method in user documents. Every link is checked; the release links that
  exist only after the tags are listed for the maintainer to open after
  publication. A first pass of this landed on 2026-09-22 with the plan trim;
  the step finishes it once (ag) and (ah) have fixed the version and the
  build commands.
- **(j) Release acceptance repeat** (GUIDE 10.10, Sol High). Regenerate the
  command reference; date `CHANGELOG-CLI.md` 0.1.0 and `CHANGELOG-GUI.md`
  1.1.0; run the Phase 4B oracle replay and the Phase 8.1 parity matrix on
  the corrected source, exactly as recorded in
  `docs/fixtures/phase8/parity.json` (never drop the draw parameters to make
  a command parse; the recorded artifact hashes belong to one Rarog 2.3.1
  build, SHA-256 `2a95390d…`, which is not the copy in `D:\chess\engines\rarog`
  — locate it or re-record the matrix on the build used); repeat the short
  third-party usability flows; dispatch the CLI candidate and the new GUI
  candidate and pass exact archive smoke on all four platforms; run the full
  suite in debug and `ci-release` as evidence, with (af.2) done first so it
  cannot flake. Then the maintainer's release process (decided 2026-09-22):
  open a pull request from `cli` to `main`; `ci.yml` runs on the pull
  request on all three platforms in both profiles and must be green;
  **squash-merge** it (the step-by-step history stays on the `cli` branch,
  which is kept, not deleted, because commit subjects there are the only
  place the step identifiers survive); tag the squash commit `gui-v1.1.0`
  and `cli-v0.1.0` and push the tags; both release workflows build, smoke
  and publish with release notes extracted from the two changelog sections
  by `colosseum-release notes`; then check the two release pages — the
  expected artifact list per product, readable names, the GUI release
  marked "Latest" and the CLI release not, notes matching the changelogs.
  If anything is wrong: delete the release and the tag, fix on `main`, and
  tag again — acceptable for a first release nobody has downloaded, and the
  reason the GUI candidate mode of (ah) is worth having afterwards. The CLI
  publish job proves the tagged commit is reachable from `main`, which the
  squash commit is.
- **(af.2) Test robustness before the acceptance run** (part of 10.10). The
  test `a_resumed_tune_does_not_keep_the_games_it_replayed` compares two
  processes' resident working sets and failed once under full-suite load,
  then passed alone and in two clean full runs. The working set is trimmed
  under memory pressure, so the uninterrupted run's figure can shrink while
  the resumed run's does not, and the comparison fails without any leak.
  Measure private commit instead (`PROCESS_MEMORY_COUNTERS_EX.PrivateUsage`
  on Windows, `VmData` from `/proc/<pid>/status` on Linux), which trimming
  does not touch, keep the same limits, and record the change in the test's
  doc comment. Accepting the flake is rejected: 10.10 runs the full suite as
  release evidence and a rerun would be evidence of nothing.

**Deferred behind the release, with the condition that reopens each:**

- (r) placement per platform (10.9h): when a Linux or macOS user of the CLI
  reports placement that the Windows-derived policy gets wrong, or before
  the CLI's 1.0.0.
- (z) two games per physical core (10.9p) and (ad) overlapped SPSA
  iterations (10.9t): the 10(x) throughput run measured 83% occupancy and
  4,517 games per hour against the 85% and 5,000 fixed beforehand — up from
  2,745 and above weather-factory's 3,720 on the same surface. Neither is
  chased now. Reopen when the first real Rarog tune on the released binary
  shows wall-clock that matters to the project; take (ad) first, because it
  removes the iteration barrier for every mini-match size and its zero-game
  study is cheap, and (z) only if (ad) leaves a margin worth an experiment.
- 10.9g's two maintainer probes (the 2,000-game 3+0.03 scramble probe and
  the 100 ms, 14-slot, 50,000-move fixed-movetime outlier probe): owed,
  informative, not blocking; run when the machine is free and record them
  in [`phase-10-record.md`](docs/architecture/phase-10-record.md).

**Exit criterion:** (af.1), (ag), (ah), (ai) demonstrated by their tests;
oracle replay and parity matrix agree on shared fields; both candidates'
archives pass smoke; documentation, changelogs and generated reference are
consistent; both tags validate.

### Phase 11 — The GUI on the harness

The GUI still plays games through `colosseum-engine::scheduler` and the SQLite
store, while the CLI plays through its own drivers with placement, pentanomial
statistics, fault policy and durable run directories. Two implementations of
one mechanism in one repository is the "one real cost" of S2 doubled. Phase 11
makes the CLI's drivers the single game-playing mechanism and keeps SQLite as
the GUI's history index. It starts only after `cli-v0.1.0` and `gui-v1.1.0`
are published, so the CLI release is never held by GUI work, and it is the
main line of development afterwards: the maintainer's own use is the CLI for
Rarog development and the GUI for tournaments, so GUI defects found in use
are fixed as patch releases on `main` while Phase 11 proceeds on a branch.

- **(a) Harness library.** Move the run directory, run record, placement
  resolution and the match, SPRT and tournament drivers from `colosseum-cli`
  into a library crate (`colosseum-harness`) with no argument parser and no
  `main`. Add an observer port that publishes per-game live state (board,
  latest search line per side, clocks) and run-level snapshots. The CLI becomes
  a thin composition root over the library; the architecture tests keep GUI
  and windowing packages out of the harness dependency graph. The CLI's
  tests, fixtures and generated reference are unchanged by the extraction,
  and the CLI's version does not change for it (a library boundary is not a
  user-visible change).
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
  in the PGN (l, n, o), the fault allowance (q, v) and the unscorable
  exclusion (af.1), the fixed rating field (g), progress blocks (p) and
  persistent engines per slot (ae) as a per-tournament choice with the GUI's
  fresh-process default kept for the old engines in real libraries. The
  desktop keeps its own presentation of them; it does not keep its own
  mechanics. The `runtime_adapter.rs` seam (library entry to
  `RuntimeParticipant`/`EngineLaunchSpec`, computed since 10.9w and not yet
  consumed) is what this step consumes.
- **(c) Retire the duplicate scheduler.** Remove `engine::scheduler` and the
  `tournament` feature's game-store execution path. Pre-existing SQLite game
  history stays readable through a read-only migration so the Arena list
  still opens old tournaments. Update `CLAUDE.md` (the "GUI spawns engines
  per game" paragraph becomes a per-tournament setting), the architecture
  documents and a new ADR recording the single-mechanism decision.
- **(d) GUI release.** Ratings on stored data agree with the previous
  implementation within 0.01 Elo; the design guidelines are checked; the GUI
  changelog records the adjudication default change and the new run
  directories prominently. The version is the maintainer's call at this step;
  a major bump (2.0.0) is recommended because a shipped default changes and
  the game store moves.

**Exit criterion:** one game-playing implementation in the workspace; GUI
tournament parity demonstrated on stored data; the CLI's own tests, fixtures
and generated reference unchanged by the extraction; a GUI candidate passes
its archive smoke.

### Post-release research

Not scheduled steps; each needs its own evidence before it becomes one.

- **Asynchronous SPSA** (from Phase 10(x)): the next games start on free
  slots with the current parameters and results are applied as they return;
  it removes the iteration barrier at any mini-match size but updates on
  slightly stale parameters and gives up the one-iteration-one-update
  record, so it needs its own evidence that it reaches the same optimum for
  less game budget. Overlapped iterations (ad) are the bounded form of the
  same idea and come first.

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


---

## S10. Reference

| Path | What |
|---|---|
| `crates/colosseum-core/src/stats.rs` | SPRT, Elo, LOS, pentanomial and normalized Elo |
| `crates/colosseum-core/src/{ml_ratings,rating_error}.rs` | joint ML ratings and Fisher-information error bars shared by both products |
| `crates/colosseum-engine/src/{runner,scheduler,openings,store}.rs` | one-game runner (both products), the GUI's tournament driver and SQLite store (retired in Phase 11) |
| `crates/colosseum-engine/src/{topology,affinity}*` | OS topology and affinity adapters behind the application ports |
| `crates/colosseum-cli/src/composition.rs`, `composition/` | parser, dispatch, shared resolvers; one module per command |
| `crates/colosseum-cli/src/{match_runner,sprt_runner,spsa_driver,tournament_driver}.rs` | the harness drivers Phase 11 extracts into `colosseum-harness` |
| `crates/colosseum-gui/src/{backend,runtime_adapter,update}.rs` | GUI composition root, the Phase 11 seam, the `gui-v` updater |
| `docs/architecture/` | current/target architecture, ADRs, phase exit documents, the Phase 0–9 and Phase 10 records, the qualification |
| `docs/cli/` | user documentation; `command-reference.md` is generated by `colosseum-docs` |
| `docs/fixtures/` | per-phase acceptance manifests and the recorded parity commands |
| `tests/fixtures/` | vendored golden fixtures and the generator that produces them |
| `tools/release/` | tag/version/changelog validation, archive staging, the two archive smoke scripts |
| `CLAUDE.md` | workspace conventions, including why the GUI spawns engines per game |
