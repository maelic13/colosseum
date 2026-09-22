# Phases 0–9 — implementation record

Moved verbatim from `PLAN.md` §S8 on 2026-09-22 when the plan was trimmed to the open work. This is the durable evidence for the completed phases: progress notes, accepted results and decisions as they were recorded at the time. The binding specifications stay in `PLAN.md` §S3–S7; the phase exit documents beside this file hold the acceptance evidence.

---

### Phase 0 — Current-state analysis and target architecture

No CLI implementation begins until the boundary it will depend on is understood
and recorded.

**Progress:** Steps 0.1 through 0.8 are complete. The
[`dependency inventory`](../architecture/dependency-inventory.md) records all
workspace packages, internal Cargo edges, source modules, principal source
imports, test targets and current build/release targets. The
[`current-state analysis`](../architecture/current-state.md) classifies
responsibilities, public boundaries, side effects, globals, error/cancellation
behavior, tests and release coupling; findings CS-01 through CS-12 account for
every S4 gap. The
[`target architecture`](../architecture/target-architecture.md) assigns every
current module and consequential public boundary, defines the application
use cases and inward-facing ports, separates GUI library data from
`EngineLaunchSpec`, and specifies composition, durability, failure/cancellation
flow and the smallest safe migration. The accepted
[`architecture decisions`](../architecture/adr/README.md) bind the package
graph, minimal launch specification, runtime-neutral port and authoritative
commit boundary, GUI-library mapping and incremental migration. The
[`release architecture`](../architecture/release-architecture.md) and
[ADR-0006](../architecture/adr/0006-one-repository-independent-product-releases.md)
keep one repository while separating GUI/CLI versions, tags, notes, artifacts
and workflows, with required shared-layer CI. The
Phase 0.6 [`naming research`](../architecture/naming-decision.md) and rejected
[ADR-0007](../architecture/adr/0007-name-the-cli-uci-rig.md) preserve the
real collision evidence and rejected UCI Rig proposal. The
[`integrated review`](../architecture/phase-0-review.md) demonstrates complete
module ownership, consistent dependency/release responsibilities and executable
owners for every independence invariant. Accepted
[ADR-0008](../architecture/adr/0008-use-colosseum-through-implementation.md)
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
versioned in [`docs/fixtures/phase4c/acceptance.json`](../fixtures/phase4c/acceptance.json).

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
are recorded in [`docs/architecture/phase-5-exit.md`](../architecture/phase-5-exit.md)
and [`docs/fixtures/phase5/acceptance.json`](../fixtures/phase5/acceptance.json).

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
[`docs/architecture/phase-6-exit.md`](../architecture/phase-6-exit.md) and
[`docs/fixtures/phase6/acceptance.json`](../fixtures/phase6/acceptance.json).

### Phase 7 — Tournaments

Spec 5.7. **Exit:** round-robin and gauntlet schedules/ratings match the GUI;
kill/resume is identical with deterministic stubs.

**Accepted (7.3):** a frozen GUI-origin fixture matches every round-robin and
two-seed-gauntlet pairing and reproduces joint ratings within 0.01 Elo.
Deterministic interrupted and uninterrupted runs produce identical schedules,
standings, error bars and crosstables for both formats, with every scheduled
game committed exactly once. Criterion ownership and the real-engine evidence
boundary are recorded in
[`docs/architecture/phase-7-exit.md`](../architecture/phase-7-exit.md).

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
[`docs/architecture/phase-8-parity.md`](../architecture/phase-8-parity.md).

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
[`docs/architecture/phase-8-gap-decisions.md`](../architecture/phase-8-gap-decisions.md).

**Accepted (8.3):** the exact-candidate external parity, reasoned divergences,
complete six-item gap audit, adopted ponder protocol and workspace regression
are one owned acceptance gate. The resulting 1.0 boundary is recorded in
[`docs/architecture/phase-8-exit.md`](../architecture/phase-8-exit.md).

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
remains exactly `colosseum-cli`. [ADR-0009](../architecture/adr/0009-retain-colosseum-for-1-0.md)
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
[ADR-0010](../architecture/adr/0010-version-cli-documentation-with-the-binary.md)
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
[`docs/architecture/phase-9.4-candidate.md`](../architecture/phase-9.4-candidate.md).

**Accepted (9.5):** Rarog commit `8f35647` and Basilisk commit `3cbf90b`
remove their duplicate runner, statistics, SPSA, affinity, recovery, NPS,
datagen and tool-vendoring implementations. Both retain only declarative
Colosseum profiles/tune vectors and explicitly classified engine-owned build,
correctness, profiling and Texel responsibilities. The audit found and closed
the missing externally selectable one-sided resignation policy; no generic
mechanism exception remains. The complete mapping and verification record is
in
[`docs/architecture/phase-9.5-coverage.md`](../architecture/phase-9.5-coverage.md).

**Accepted (9.6):** a clean-room Windows usability exercise drove two public
UCI Stockfish binaries, supplied only as executable paths, through the
published guides' fixed match, intentionally capped one-pair SPRT and
one-iteration SPSA flows.  All game workflows completed with zero faults; the
SPRT's exit-4 inconclusive result, missing-book warning and SPSA rail warning
were clear and documented.  The current-source local archive is usability
evidence only: commit `89d24a0` changed the CLI after the Phase 9.4 candidate,
so a fresh four-platform CI candidate is mandatory before Phase 9.7.  Exact
commands, identities, hashes and triage are in
[`docs/architecture/phase-9.6-usability.md`](../architecture/phase-9.6-usability.md).

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
[`docs/architecture/phase-9.7-release-acceptance.md`](../architecture/phase-9.7-release-acceptance.md).

**Release publication moved behind Phase 10** by maintainer decision on
2026-09-17. The 9.7 acceptance evidence stands for the candidate it names and
the same gates are repeated at 10.10 on the corrected candidate.

