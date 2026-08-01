# Colosseum — development guide

The short operational view: where the harness stands and what to do next.
Rationale, specifications, success criteria and evidence live in
[`PLAN.md`](PLAN.md).

**This file and `PLAN.md` are the maintainer-facing pair.** `README.md` is the
user-facing front door for the whole project (the GUI and CLI);
the user documentation is user-facing CLI detail. Neither may carry phase
numbers, internal naming or method argumentation.

## Current checkpoint

| | |
|---|---|
| Branch / version | `cli`; Colosseum GUI **1.0.2** released. Independent Colosseum CLI foundation: **0.1.0**, unreleased |
| What exists | Phases 0–3 plus fixed-N direct-engine matches with independent per-side time controls are complete: Clean Architecture boundaries, independent CLI composition/version lane, deterministic statistics/random streams/configuration, ordinary-UCI inspect/check, strict JSON/dry-run output, exact-executable self-test, process containment, durable run primitives, topology/affinity handling, and same-binary option comparisons |
| What is missing | Match output durability; pair-atomic SPRT; optional calibration; SPSA; benchmarking; Texel/data-generation and release work |
| Validation engines | **Rarog** (Rust) and **Basilisk** (C++) — available, active, different languages and build systems. Any two UCI engines would serve; nothing depends on these |
| Platform status | Windows ☑ local through Phase 4A.6 · Linux/macOS ☐ CI execution evidence pending — required CI is configured for debug/release on all three |
| Next step | **Phase 4A.7 — durable and structured output** |
| Recommended model | **GPT-5.6 Terra — High** |

## Forward tracker

<!-- TRACKER FORMATTING RULES — follow them, they get broken often:
     1. ONE step per bullet. Never join two steps on one line.
     2. Use renderer-independent Unicode markers; Codex does not reliably
        implement GitHub task-list `[ ]` / `[x]` syntax:
            - ☐ **1.2** — todo
            - ◐ **1.2 — IN PROGRESS** — genuinely in flight
            - ☑ **1.2 — DONE** — resolved
     3. Resolved outcome labels are DONE · REJECTED · DEFERRED → <item> ·
        PARKED · FIXED. Anything resolved uses ☑, never ◐.
     4. Continuation lines indent 2 spaces. Sub-items indent 2 more spaces,
        use a normal `-` bullet, and indent their continuations another 2.
     5. Once implementation starts, NEVER renumber existing items — commits
        reference them. To insert before the first item use a .0.
     6. Always mark a completed step here in the same commit. Add status or
        evidence to PLAN.md when it improves the durable specification; do not
        duplicate routine tracker detail there.
     7. Blank line AFTER the `###` heading, then NO blank lines between
        bullets: one continuous list per phase.
     8. ONLY NUMBERED STEPS live here. Recurring procedures go in their own
        section and never get a status marker.
     9. Every numbered step includes its PLAN §S8 model assignment. Keep the
        label and the authoritative routing table synchronized. -->

Each phase ends with a verifiable exit criterion — see PLAN §S8. Nothing is
"done" because it compiles; it is done when its criterion is demonstrated.
The model labels below are defaults from PLAN §S8, not substitutes for the
step's tests or exit criterion. When reporting the next step, always report its
model as well.

### Phase 0 — Current-state analysis and target architecture

- ☑ **0.1 — DONE** — **Model: Terra High.** Inventory the current crate/module dependency graph with
  `cargo metadata`, `cargo tree` and source inspection — evidence:
  [`docs/architecture/dependency-inventory.md`](docs/architecture/dependency-inventory.md)
- ☑ **0.2 — DONE** — **Model: Sol High.** Write `docs/architecture/current-state.md`: responsibilities, public
  boundary types, I/O/global state, framework dependencies, GUI/release
  coupling and every violation of PLAN §S4; explicitly audit UUID
  generation and branding/path policy in `colosseum-core`, GUI config/store
  seams, incident globals, SQLite scheduling, external-engine test paths,
  workspace-version inheritance, GUI-only release automation and CI coverage —
  evidence: [`docs/architecture/current-state.md`](docs/architecture/current-state.md)
- ☑ **0.3 — DONE** — **Model: Sol High.** Write `docs/architecture/target-architecture.md` using Clean
  Architecture: domain, application use cases/ports, adapters, drivers,
  composition roots, error/cancellation flow and current-to-target migration —
  evidence: [`docs/architecture/target-architecture.md`](docs/architecture/target-architecture.md)
- ☑ **0.4 — DONE** — **Model: Sol High.** Record ADRs for package boundaries, runtime `EngineLaunchSpec`,
  injected persistence/artifact/affinity/identity/master-seed ports,
  GUI-library mapping and the smallest refactor that enforces inward
  dependencies — evidence:
  [`docs/architecture/adr/README.md`](docs/architecture/adr/README.md)
- ☑ **0.5 — DONE** — **Model: Sol High.** Design independent CLI/GUI versions, tags, artifacts, release notes
  and shared-layer regression CI. Prefer one repo; split only with a
  documented concrete advantage — evidence:
  [`docs/architecture/release-architecture.md`](docs/architecture/release-architecture.md)
  and [ADR-0006](docs/architecture/adr/0006-one-repository-independent-product-releases.md)
- ☑ **0.6 — DONE** — **Model: Terra High.** Research the existing “Coliseum” naming/search/package collision
  and record the proposal plus rejected alternatives — outcome: material
  Colosseum collision risks are documented; the CLI-only **UCI Rig** proposal
  was rejected before implementation and is retained as evidence:
  [`docs/architecture/naming-decision.md`](docs/architecture/naming-decision.md)
  and [ADR-0007](docs/architecture/adr/0007-name-the-cli-uci-rig.md)
- ☑ **0.7 — DONE** — **Model: Sol High.** Review the current/target architecture, ADRs and release design as one
  contract; demonstrate that every module has a target owner, dependency and
  release diagrams agree, and every independence invariant has an executable
  test owner; correct inconsistencies and record review evidence — evidence:
  [`docs/architecture/phase-0-review.md`](docs/architecture/phase-0-review.md)
- ☑ **0.8 — DONE / EXIT** — **Model: Sol High.** Bind **Colosseum** / `colosseum` /
  `colosseum-gui` and **Colosseum CLI** / `colosseum-cli` as the coherent
  implementation identity; preserve the Phase 0.6 collision research, reject
  speculative rename indirection and defer an optional full keep-or-rename
  decision to Phase 9.0 — evidence:
  [ADR-0008](docs/architecture/adr/0008-use-colosseum-through-implementation.md)

### Phase 1 — Pentanomial statistics and nElo (`colosseum-core`)

- ☑ **1.1 — DONE** — **Model: Terra High.** Pair-level scoring maps every
  complete colour-reversed pair to the `[0, 0.5, 1, 1.5, 2]` pentanomial
  vector; incomplete games are explicitly counted as unpaired and cannot enter
  a pentanomial SPRT input — evidence:
  [`crates/colosseum-core/src/stats.rs`](crates/colosseum-core/src/stats.rs)
- ☑ **1.2 — DONE** — **Model: Sol High.** Implement population pentanomial
  variance and paired standard error, normalized and logistic Elo intervals,
  paired LOS, draw ratio, pairs ratio and WL/DD ratio; retain the WL/DD split
  inside the central bin and return undefined ratios without NaN/Inf — evidence:
  [`crates/colosseum-core/src/stats.rs`](crates/colosseum-core/src/stats.rs)
  and the exact formulas in PLAN §5.1
- ☑ **1.3 — DONE** — **Model: Sol High.** Implement selectable generalized
  multinomial SPRT for both normalized and logistic Elo over complete pairs;
  retain the selected model, hypotheses, error rates, LLR, Wald bounds and
  H0/H1/continue decision in the result, with unpaired games excluded —
  evidence: [`crates/colosseum-core/src/stats.rs`](crates/colosseum-core/src/stats.rs)
  and the exact likelihood contract in PLAN §5.1
- ☑ **1.4 — DONE** — **Model: Sol High.** Implement one-/two-sided fixed-N
  difference planning with explicit Elo model, target effect, significance,
  power and assumed five-bin distribution, plus empirical achieved intervals
  and conservative resolution that cannot carry an SPRT verdict — evidence:
  [`crates/colosseum-core/src/stats.rs`](crates/colosseum-core/src/stats.rs)
  and the exact normal-approximation contract in PLAN §5.1
- ☑ **1.5 — DONE** — **Model: Terra High.** Replace temporary `Option` failure
  signals across pentanomial estimates, SPRT, fixed-N planning and achieved
  resolution with the public `StatisticsError` contract; validate scalar
  inputs, probabilities and hypotheses precisely, while retaining optional
  diagnostic ratios for unavailable denominators. Zero games, one pair, all
  draws and clean sweeps now return named errors rather than `NaN`/`Inf` —
  evidence: [`crates/colosseum-core/src/stats.rs`](crates/colosseum-core/src/stats.rs)
- ☑ **1.6 — DONE** — **Model: Terra High.** Add deterministic property sweeps
  for both pentanomial models: the low-level empty-sample LLR is zero, LLR is
  strictly monotone as score rises at fixed pair count, swapping engine arms
  negates the LLR, and Wald bounds exactly equal `log(β/(1−α))` and
  `log((1−β)/α)` — evidence:
  [`crates/colosseum-core/src/stats.rs`](crates/colosseum-core/src/stats.rs)
- ☑ **1.7 — DONE** — **Model: Terra High. Fixture corpus:** add a documented
  generator for arbitrary UCI pairs through `fastchess` and `cutechess-cli`,
  with committed runner/engine identity, version, SHA-256, licence/source
  provenance, commands, console logs and PGNs. Add hand-derived pentanomial
  fixtures and a binding field-by-field oracle matrix that excludes unsupported
  comparisons — evidence:
  [`tests/fixtures/statistics/`](tests/fixtures/statistics/),
  [`scripts/Generate-StatisticsFixture.ps1`](scripts/Generate-StatisticsFixture.ps1)
- ☑ **1.8 — DONE** — **Model: Terra High. Hermetic CI suite:** add repository-only required CI and
  regression assertions; isolate runner, scheduler and UCI real-engine checks
  behind the `real-engine-smoke` feature and `COLOSSEUM_SMOKE_ENGINE`, with no
  implicit machine path, skip, platform, or release evidence — evidence:
  [`.github/workflows/ci.yml`](.github/workflows/ci.yml),
  [`crates/colosseum-engine/tests/hermetic_ci.rs`](crates/colosseum-engine/tests/hermetic_ci.rs),
  [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)
- ☑ **1.9 — EXIT PASSED** — **Model: Sol High.** Parse and execute every analytic statistics fixture;
  reconstruct complete colour pairs and W/D/L from the reviewed Fastchess and
  Cutechess PGNs; enforce a machine-readable list of all accepted matrix cells
  and reasoned exclusions, with no unsupported comparison or guessed value —
  evidence:
  [`crates/colosseum-core/tests/statistics_fixtures.rs`](crates/colosseum-core/tests/statistics_fixtures.rs),
  [`tests/fixtures/statistics/phase-1-acceptance.toml`](tests/fixtures/statistics/phase-1-acceptance.toml)

### Phase 2 — Architecture migration, CLI skeleton and durable foundation

- ☑ **2.1 — DONE** — **Model: Sol High.** Implement the Phase-0 boundary migration: generic runtime
  participant and launch types, framework-independent application use cases and
  ports, a concrete UCI session adapter, and a GUI library-to-runtime adapter.
  Identity entropy and product/config path policy now live outside the domain;
  GUI engine-library/config serialization remains compatible — evidence:
  [`crates/colosseum-application/`](crates/colosseum-application/),
  [`crates/colosseum-uci/src/session.rs`](crates/colosseum-uci/src/session.rs),
  [`crates/colosseum-gui/src/runtime_adapter.rs`](crates/colosseum-gui/src/runtime_adapter.rs),
  [`crates/colosseum-application/tests/architecture.rs`](crates/colosseum-application/tests/architecture.rs)
- ☑ **2.2 — DONE** — **Model: Terra High.** Add the independently versioned,
  non-publishable `colosseum-cli` package and headless composition root with
  tested `--version`/`--help` and a dependency-graph rejection of GUI/windowing
  packages. GUI/CLI versions and changelogs now have separate authorities; the
  cross-platform push/PR workflow tests debug and release workspaces and builds
  the CLI independently. The shared release-metadata tool validates product tag,
  version and changelog routing; publication workflows remain 9.4 — evidence:
  [`crates/colosseum-cli/`](crates/colosseum-cli/),
  [`tools/release/`](tools/release/),
  [`.github/workflows/ci.yml`](.github/workflows/ci.yml),
  [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)
- ☑ **2.3 — DONE** — **Model: Terra High.** Add reusable direct engine controls:
  required executable plus optional display label, ordered process arguments,
  cwd, environment, arbitrary UCI values/buttons and logical-core allocation.
  Path-only input resolves to the minimal application launch spec; duplicate
  names, malformed values and ambiguous/unsafe core lists are rejected — evidence:
  [`crates/colosseum-cli/src/engine_args.rs`](crates/colosseum-cli/src/engine_args.rs),
  [`docs/cli/engine-controls.md`](docs/cli/engine-controls.md)
- ☑ **2.4 — DONE** — **Model: Sol High.** Implement deterministic resolution as
  built-in defaults < inherited run TOML < CLI. One-parent chains resolve
  relative to each declaring file, stop at depth 16 and reject canonical cycles;
  tables merge recursively, scalars/arrays replace, and per-layer RFC 6901
  `unset` is strict. Leaf origins survive inheritance, declared path fields are
  canonicalized, and exact stable JSON bytes plus SHA-256 and origin sidecars are
  written. Fixtures cover flattened/all-CLI byte identity and malformed chains,
  pointers, arrays and Windows path aliases — evidence:
  [`crates/colosseum-cli/src/config.rs`](crates/colosseum-cli/src/config.rs),
  [`crates/colosseum-cli/tests/config_resolution.rs`](crates/colosseum-cli/tests/config_resolution.rs),
  [`docs/cli/run-files.md`](docs/cli/run-files.md)
- ☑ **2.4a — DONE** — **Model: Sol High.** Implement the exact u64/SHA-256
  named-stream derivation and explicit ChaCha12 generator, little-endian draws,
  rejection-bounded integers, Fisher–Yates shuffle, Rademacher signs and
  bootstrap sampling. Stable consumer names and independent-stream properties
  have golden vectors. A supplied seed is retained; otherwise OS entropy creates
  and inserts it before resolved-config hashing/writing. RNG version is bound to
  the built-in `stats_version` — evidence:
  [`crates/colosseum-core/src/rng.rs`](crates/colosseum-core/src/rng.rs),
  [`crates/colosseum-cli/src/master_seed.rs`](crates/colosseum-cli/src/master_seed.rs),
  [`docs/cli/randomness.md`](docs/cli/randomness.md)
- ☑ **2.5 — DONE** — **Model: Terra High.** Add `engine inspect` for UCI identity
  and advertised schema plus `engine check` with individual handshake,
  synchronization, requested-value/schema validation, option acceptance plus
  readyok, legal bounded start-position search, bounded stop/bestmove,
  ucinewgame/readyok and quit checks. Reports explicitly say UCI has no option
  read-back; failures produce nonzero status. Application fake-port tests pin
  orchestration and prevent invalid options from being sent — evidence:
  [`crates/colosseum-application/src/check.rs`](crates/colosseum-application/src/check.rs),
  [`crates/colosseum-cli/src/main.rs`](crates/colosseum-cli/src/main.rs),
  [`crates/colosseum-uci/src/session.rs`](crates/colosseum-uci/src/session.rs)
- ☑ **2.6 — DONE** — **Model: Terra High.** `--dry-run` resolves path-aware
  configuration identity and prints exact structured process invocations
  without launching; `--json` emits one typed JSON document on successful
  stdout while failures leave stdout empty and diagnostics use stderr — evidence:
  [`crates/colosseum-cli/src/main.rs`](crates/colosseum-cli/src/main.rs),
  [`crates/colosseum-cli/tests/command_line.rs`](crates/colosseum-cli/tests/command_line.rs),
  [`docs/cli/output.md`](docs/cli/output.md)
- ☑ **2.7 — DONE** — **Model: Sol High.** `self-test` launches the exact
  executable's hidden deterministic UCI stub and verifies compliance, a
  deterministic four-ply two-process exchange, finite stdout/stderr handling,
  over-limit protocol rejection, required persistence-failure propagation and
  bounded process-tree reaping of an ignored-quit engine with a descendant.
  Engine processes use Windows kill-on-close Job Objects or Unix process groups
  — evidence: [`crates/colosseum-cli/src/self_test.rs`](crates/colosseum-cli/src/self_test.rs),
  [`crates/colosseum-cli/src/uci_stub.rs`](crates/colosseum-cli/src/uci_stub.rs),
  [`crates/colosseum-uci/src/process.rs`](crates/colosseum-uci/src/process.rs),
  [`docs/cli/self-test.md`](docs/cli/self-test.md)
- ☑ **2.8 — DONE** — **Model: Sol High.** The reusable CLI run-directory
  adapter creates collision-safe defaults under `./colosseum-runs`, treats an
  explicit directory as resume with exact configuration-hash refusal, archives
  complete old state before restart, syncs append-only logs, and atomically
  publishes checksummed current/previous checkpoint generations with fallback
  recovery — evidence:
  [`crates/colosseum-cli/src/run_directory.rs`](crates/colosseum-cli/src/run_directory.rs),
  [`crates/colosseum-cli/tests/run_directory.rs`](crates/colosseum-cli/tests/run_directory.rs),
  [`docs/cli/run-directories.md`](docs/cli/run-directories.md)
- ☑ **2.9 — DONE** — **Model: Terra High.** Common read-only
  `status <run-dir>` reports a versioned atomic run record containing exact
  configuration identity, official committed/pentanomial sample,
  host/capability summary and structured anomalies. A lifecycle owner writes
  running state immediately and records `aborted` on an unclosed exit, including
  zero-sample attempts; tests prove status changes no run bytes — evidence:
  [`crates/colosseum-cli/src/run_record.rs`](crates/colosseum-cli/src/run_record.rs),
  [`crates/colosseum-cli/tests/run_record.rs`](crates/colosseum-cli/tests/run_record.rs),
  [`docs/cli/status.md`](docs/cli/status.md)
- ☑ **2.10 — EXIT PASSED** — **Model: Sol High.** Hermetic acceptance proves
  two independently copied ordinary UCI executables pass path-only compliance,
  isolated CLI execution does not touch sentinel GUI state, and every exit gate
  has an executable owner. Local path-only checks also passed with Rarog and
  Basilisk without committing either path. Configuration/randomness,
  durable/status, copied-executable self-test, bounded flood/descendant reaping,
  inward-dependency and complete GUI/workspace suites pass; the unused legacy
  engine/SQLite CLI dependency was removed — evidence:
  [`crates/colosseum-cli/tests/phase2_acceptance.rs`](crates/colosseum-cli/tests/phase2_acceptance.rs),
  [`docs/fixtures/phase2/acceptance.json`](docs/fixtures/phase2/acceptance.json),
  [`docs/architecture/phase-2-exit.md`](docs/architecture/phase-2-exit.md)

### Phase 3 — CPU topology and affinity

- ☑ **3.1 — DONE** — **Model: Sol High.** OS topology adapters use Windows
  `GetLogicalProcessorInformationEx` group masks, Linux
  `thread_siblings_list`, and macOS `sysctl` counts. Logical identities retain
  Windows processor groups; exact sibling sets are validated for overlap and
  consistency. macOS explicitly reports the public sibling map unavailable
  instead of inferring from numbering — evidence:
  [`crates/colosseum-engine/src/topology.rs`](crates/colosseum-engine/src/topology.rs),
  [`docs/cli/cpu-topology.md`](docs/cli/cpu-topology.md)
- ☑ **3.2 — DONE** — **Model: Terra High.** Deterministic placement policy
  resolves `auto`, `off` and group-qualified explicit logical CPU lists.
  `auto` selects whole reported physical cores and leaves a configurable two-core
  default headroom; unresolved sibling maps fail rather than guessing — evidence:
  [`crates/colosseum-engine/src/placement.rs`](crates/colosseum-engine/src/placement.rs),
  [`docs/cli/cpu-topology.md`](docs/cli/cpu-topology.md)
- ☑ **3.3 — DONE** — **Model: Sol High.** Current-process availability uses
  Linux scheduler affinity (including cpuset/cgroup restrictions) and Windows
  group/process affinity plus CPU Set restrictions. Portable identities retain
  processor groups, and planning intersects the allowed set before counting
  physical-core headroom — evidence:
  [`crates/colosseum-engine/src/allowed_cpus.rs`](crates/colosseum-engine/src/allowed_cpus.rs),
  [`crates/colosseum-engine/src/placement.rs`](crates/colosseum-engine/src/placement.rs),
  [`docs/cli/cpu-topology.md`](docs/cli/cpu-topology.md)
- ☑ **3.4 — DONE** — **Model: Terra High.** Each concurrent game slot receives
  two disjoint allocations of the configured physical `cores-per-engine`, with
  available SMT siblings kept together and capacity checked independently of
  UCI worker-thread options — evidence:
  [`crates/colosseum-engine/src/placement.rs`](crates/colosseum-engine/src/placement.rs),
  [`docs/cli/cpu-topology.md`](docs/cli/cpu-topology.md)
- ☑ **3.5 — DONE** — **Model: Sol High.** OS-native core-class and NUMA metadata
  keep A/B engines on the same class and node where possible, prefer per-engine
  node locality when perfect symmetry is impossible, and record every class/node
  set plus visible asymmetry — evidence:
  [`crates/colosseum-engine/src/characteristics.rs`](crates/colosseum-engine/src/characteristics.rs),
  [`crates/colosseum-engine/src/placement.rs`](crates/colosseum-engine/src/placement.rs),
  [`docs/cli/cpu-topology.md`](docs/cli/cpu-topology.md)
- ☑ **3.6 — DONE** — **Model: Terra High.** Hard affinity is applied and read
  back on Windows/Linux or fails as a typed error, never silently degrading;
  `off` is a recorded successful no-op, and macOS reports hard placement as
  unavailable while permitting `off` clock matches — evidence:
  [`crates/colosseum-engine/src/affinity.rs`](crates/colosseum-engine/src/affinity.rs),
  [`docs/cli/cpu-topology.md`](docs/cli/cpu-topology.md)
- ☑ **3.7 — DONE** — **Model: Terra High.** The read-only `capabilities`
  command prints stable text/JSON topology, allowed-set, core-class/NUMA and
  hard-affinity availability/limitations without launching engines or pulling
  the legacy tournament/SQLite backend into the independent CLI artifact —
  evidence:
  [`crates/colosseum-cli/src/capabilities.rs`](crates/colosseum-cli/src/capabilities.rs),
  [`crates/colosseum-cli/tests/command_line.rs`](crates/colosseum-cli/tests/command_line.rs),
  [`docs/cli/cpu-topology.md`](docs/cli/cpu-topology.md)
- ☑ **3.8 — DONE · EXIT** — **Model: Sol High.** Recorded SMT 16c/32t, P/E,
  restricted-cpuset, processor-group, no-SMT and dual-socket fixtures select
  exact CPU lists; two busy children remain on their enforced processors where
  supported; capability reporting, limitations and gate ownership are durable —
  evidence:
  [`crates/colosseum-engine/tests/phase3_acceptance.rs`](crates/colosseum-engine/tests/phase3_acceptance.rs),
  [`docs/fixtures/phase3/topologies.json`](docs/fixtures/phase3/topologies.json),
  [`docs/architecture/phase-3-exit.md`](docs/architecture/phase-3-exit.md)

### Phase 4A — Fixed-match runner

- ☑ **4A.1 — DONE** — **Model: Terra High.** `match --games N` runs exactly N
  direct UCI games with alternating colours and no sequential stopping; each
  side has independent executable, process and UCI-option controls, including
  same-path option comparisons; strict JSON and dry-run work for both sides —
  evidence:
  [`crates/colosseum-cli/src/match_runner.rs`](crates/colosseum-cli/src/match_runner.rs),
  [`crates/colosseum-cli/tests/command_line.rs`](crates/colosseum-cli/tests/command_line.rs),
  [`docs/cli/match.md`](docs/cli/match.md)
- ☑ **4A.2 — DONE** — **Model: Sol High.** Per-side movetime, sudden-death,
  base-plus-increment, fixed-node and fixed-depth controls resolve independently,
  including asymmetric odds matches and per-side configurable margins; omitted
  controls use the documented `3+0.03` default — evidence:
  [`crates/colosseum-cli/src/match_runner.rs`](crates/colosseum-cli/src/match_runner.rs),
  [`crates/colosseum-cli/tests/command_line.rs`](crates/colosseum-cli/tests/command_line.rs),
  [`docs/cli/match.md`](docs/cli/match.md)
- ☑ **4A.2a — DONE** — **Model: Sol High. Clock accounting (PLAN §5.4a), explicit/versioned/recorded:**
  clock runs from finishing the write of `go` to finishing the read of
  `bestmove`, charging harness read latency and engine search start-up to the
  mover; `position` setup is not charged; monotonic source only; increment
  follows `E > R + M` forfeit, otherwise `max(0, R-E) + I`, with equality
  accepted; the margin is not sent to the engine; record model/version,
  margin, clock resolution and charged-elapsed min/median/max without
  claiming engine/harness overhead can be separated — evidence:
  [`crates/colosseum-uci/src/process.rs`](crates/colosseum-uci/src/process.rs),
  [`crates/colosseum-engine/src/runner.rs`](crates/colosseum-engine/src/runner.rs),
  [`crates/colosseum-cli/tests/command_line.rs`](crates/colosseum-cli/tests/command_line.rs)
- ☑ **4A.3 — DONE** — **Model: Terra High.** Draw, two-sided resignation and
  maximum-move adjudication are independently configurable and disableable;
  conservative draw/resign defaults are resolved into structured output, while
  arbitrary engine tablebase UCI options continue through the ordinary option
  path without harness probing — evidence:
  [`crates/colosseum-cli/src/main.rs`](crates/colosseum-cli/src/main.rs),
  [`crates/colosseum-cli/tests/command_line.rs`](crates/colosseum-cli/tests/command_line.rs),
  [`docs/cli/match.md`](docs/cli/match.md)
- ☑ **4A.4 — DONE** — **Model: Sol High.** Typed engine faults retain their
  attributable forfeit and side, with zero-default engine/time-loss thresholds
  invalidating the run; pre-play spawn and infrastructure faults are explicitly
  non-scorable and terminate without changing W/L/D; there is no selective
  retry/discard path — evidence:
  [`crates/colosseum-engine/src/runner.rs`](crates/colosseum-engine/src/runner.rs),
  [`crates/colosseum-cli/src/match_runner.rs`](crates/colosseum-cli/src/match_runner.rs),
  [`crates/colosseum-cli/tests/command_line.rs`](crates/colosseum-cli/tests/command_line.rs)
- ☑ **4A.5 — DONE** — **Model: Sol High.** Explicit concurrency runs bounded
  parallel slots with deterministic report order; off/auto/explicit placement
  composes the Phase-3 topology allocator and verified child affinity, including
  direct per-side lists; configured Hash is reported only as a conservative
  lower bound and refusal requires a trusted explicit budget — evidence:
  [`crates/colosseum-cli/src/match_runner.rs`](crates/colosseum-cli/src/match_runner.rs),
  [`crates/colosseum-engine/src/runner.rs`](crates/colosseum-engine/src/runner.rs),
  [`crates/colosseum-cli/tests/command_line.rs`](crates/colosseum-cli/tests/command_line.rs)
- ☑ **4A.6 — DONE** — **Model: Terra High.** Optional EPD/PGN books support
  deterministic sequential or versioned named-stream random order, validated
  start and PGN-ply controls, colour-pair assignment and reuse-fraction
  reporting; no-book stays path-only startpos with a diversity warning —
  evidence:
  [`crates/colosseum-engine/src/openings.rs`](crates/colosseum-engine/src/openings.rs),
  [`crates/colosseum-cli/src/match_runner.rs`](crates/colosseum-cli/src/match_runner.rs),
  [`crates/colosseum-cli/tests/command_line.rs`](crates/colosseum-cli/tests/command_line.rs)
- ☐ **4A.7** — **Model: Terra High.** Live/structured output, full log, PGN and failed-game traffic;
  JSON-only stdout mode and documented exit codes
- ☐ **4A.8** — **EXIT · Model: Sol High.** Path-only/no-book and paired-book matches pass; fault
  injection never scores infrastructure failures; output/resume/schedule
  tests pass; a stub sleeping a commanded duration is charged it within
  tolerance on every platform, a sub-margin overrun is not forfeited while a
  super-margin one is and is attributed correctly, exact equality is
  accepted, a mid-game system-clock change does not alter the result, and
  below/at/above increment-margin boundaries have fixtures

### Phase 4B — Pair-atomic SPRT and parity

- ☐ **4B.1** — **Model: Terra High.** Explicit hypotheses/error rates/model and finite `max-pairs`;
  `gainer`/`simplify` are named bundles
- ☐ **4B.2** — **Model: Sol High.** Opening colour-pair is atomic. Commit complete pairs in deterministic
  schedule order; never evaluate an incomplete pair
- ☐ **4B.3** — **Model: Sol High.** At a boundary, schedule no new pairs, complete a half-pair, and
  exclude separately stored post-terminal work from the official sample
- ☐ **4B.4** — **Model: Terra High.** Exit/reporting distinguishes H1/H0/inconclusive/invalid/error and
  includes model, hypotheses, LLR, bounds, cap and terminal pair
- ☐ **4B.5** — **Model: Sol High.** Replay identical ordered outcomes through compatible external
  statistics; controlled live parity compares only shared fields
- ☐ **4B.6** — **EXIT · Model: Sol High.** Analytic/oracle parity; concurrency cannot change the
  terminal pair; every terminal/fault case passes; live differences
  root-caused before Phase 5

### Phase 4C — Optional calibration

- ☐ **4C.1** — **Model: Terra High.** Byte-identical binaries; representative TC/book/adjudication/
  concurrency/placement; configurable fixed N/confidence/tolerance
  (defaults 30k / 95% / ±5 nElo); never a prerequisite
- ☐ **4C.2** — **Model: Terra High.** PASS iff interval is inside tolerance; FAIL iff wholly outside one
  edge; overlap is INCONCLUSIVE; any engine fault is INVALID
- ☐ **4C.3** — **EXIT · Model: Sol High.** Hash/config/persistence checks, deterministic tests for
  every outcome and one real-machine smoke run

### Phase 5 — SPSA

- ☐ **5.1** — **Model: Sol High.** Implement PLAN §5.5's exact seeded Rademacher perturbation, arm
  construction, Fishtest-compatible `c/a/r` schedule, update, clipping and
  send-time rounding; decay per iteration
- ☐ **5.2** — **Model: Sol High.** Back-solve from each `c_end`, run `r_end` and horizon; persist exact RNG
  algorithm/seed/draw order; assert written schedule before play
- ☐ **5.3** — **Model: Terra High.** Tune TOML selects numeric UCI options with initial value, bounds and
  `c_end`; validated against the live UCI option schema
- ☐ **5.4** — **Model: Terra High.** Defaults 5,000 iterations and 32 games/iteration — configurable,
  neither enforced as a minimum
- ☐ **5.5** — **Model: Sol High.** Persistent driver, book loaded once, complete paired mini-match as the
  commit unit; engine fault invalidates rather than becoming a gradient;
  multi-session per the durable contract
- ☐ **5.6** — **Model: Terra High.** Config audit: reject absent/non-spin options, duplicates, bounds
  outside the engine's range, `min>=max`, and perturbations rounding to
  zero; warn on default disagreement and a seed on a rail
- ☐ **5.7** — **Model: Sol High. Close the loop:** rounded mean over frozen final-10% window as
  setoptions/JSON/run fragment; `sprt --apply` gates original versus tuned
  vector with the same executable hash unless explicitly overridden
- ☐ **5.8** — **Model: Terra High.** `spsa plan` reports factual schedule/game/cost/resolution information.
  Optional convergence simulation requires an explicit synthetic model and
  is never presented as a chess-convergence forecast
- ☐ **5.9** — **Model: Terra High.** `spsa status` reads an atomic snapshot and reports trajectory/thirds,
  boundary contact, little seed movement, stability and dead perturbation
  as labelled heuristics—never causal proof or automatic advice
- ☐ **5.10** — **EXIT · Model: Sol High.** Schedule property tests; every hard audit class rejected
  by a fixture; exact RNG stream survives resume; synthetic convergence
  smoke test passes; plan arithmetic/status match fixtures; stub tune feeds
  `sprt --apply` unedited with verified executable hash

### Phase 6 — Speed, planning, replay, books and position suites

- ☐ **6.1** — **Model: Sol High.** `nps` uses harness monotonic wall time over fixed-node searches;
  reported nodes verify work; engine time/nps is diagnostic only
- ☐ **6.2** — **Model: Sol High.** One or more executables per arm with per-executable medians; self pair
  optional; seeded order, warm-up, cold/warm state policy, strict
  alternation, median/best-of, bootstrap CI and per-round SD
- ☐ **6.3** — **Model: Sol High.** Scaling sweep over explicit engine thread counts: matching physical
  cores, pinned workload, fixed-total/per-thread Hash policy, wall-time
  speedup/efficiency, CPU class/NUMA recorded
- ☐ **6.4** — **Model: Terra High.** `book slice` / `hash` / `stats` / `verify`
- ☐ **6.5** — **Model: Terra High.** `stats` authority: structured run store > PGN export > forensic log >
  console; missing pair identity falls back to labelled unpaired statistics
- ☐ **6.6** — **Model: Sol High.** `stats plan fixed|sprt` with explicit assumptions; fixed-N required
  pairs/achieved resolution and seeded SPRT expected-length simulation
- ☐ **6.7** — **Model: Terra High.** PGN telemetry lists supported annotations and coverage; excludes
  opening moves; reports unavailable rather than zero; warns that node
  semantics must be compatible
- ☐ **6.8** — **Model: Terra High.** `suite` runs EPD/FEN at fixed time/nodes/depth with `bm`/`am`,
  per-position results, aggregate pass rate and compatible-baseline compare
- ☐ **6.9** — **EXIT · Model: Sol High.** Fake engine-reported nps cannot affect authoritative speed;
  skew/scaling/state-policy tests pass; slicing/replay/planning/telemetry and
  EPD suite match fixtures

### Phase 7 — Tournaments

- ☐ **7.1** — **Model: Terra High.** One `tournament` use case supports round-robin and one/multi-seed
  gauntlet; optional `gauntlet` alias has no second implementation
- ☐ **7.2** — **Model: Sol High.** Joint ML ratings/error bars, optional anchor, standings/crosstable CSV
  and durable resume for both formats
- ☐ **7.3** — **EXIT · Model: Sol High.** Schedules/ratings match GUI (≤0.01 Elo); deterministic
  kill/resume produces identical standings for both formats

### Phase 8 — Parity against external runners, and remaining gaps

- ☐ **8.1** — **Model: Sol High.** Repeat Phase-4B parity with current supported external versions and
  the exact release candidate, comparing only oracle-matrix shared fields
- ☐ **8.2** — **Model: Sol High.** Remaining feature gaps: adopt / decline with a reason / defer.
  Candidates: Chess960, ponder, harness Syzygy adjudication, additional
  formats and whether datagen now has generic needs beyond a match recipe.
  Tie-breaker: does a general engine developer need it?
- ☐ **8.3** — **EXIT · Model: Sol High.** Parity demonstrated and every gap has a recorded decision

### Phase 9 — Documentation and release

- ☐ **9.0** — **Model: Sol High. Optional final naming review:** judge the
  implemented product and either retain Colosseum with a recorded decision or
  choose one replacement and perform the complete one-time migration before
  documentation and release. Recheck dated web, same-domain, GitHub,
  package-channel and preliminary trademark evidence. A rename covers the
  repository, packages/crates, binaries, releases/artifacts, installer and app
  IDs, config/data compatibility, updater URLs and docs; do not build neutral
  aliases or a speculative branding framework merely to prepare for it
- ☐ **9.1** — **Model: Sol High. Documentation placement analysis:** in-repo `docs/` published as a
  static site, GitHub wiki, or generated reference plus guides. Criteria:
  versioning with the binary (a wiki does not version, which matters once
  `stats_version` exists), discoverability, offline availability,
  contribution friction, and whether the command reference can be generated
  from the argument parser so it cannot drift. Record the decision
- ☐ **9.2** — **Model: Terra High.** README as the project front door — what Colosseum GUI and
  Colosseum CLI are (or their Phase 9.0 replacement), install, links
- ☐ **9.3** — **Model: Terra High.** User documentation: quickstart, command reference, run-file and
  tune-file reference, a worked example per command, "how to trust a result"
  from PLAN §S3 Tier C, and a compatibility page (what the tool needs from a
  UCI engine, what it does with non-conforming ones). State that engines are
  separate processes and direct users to applicable licences; make no
  blanket legal conclusion
- ☐ **9.4** — **Model: Sol High.** Ship per Phase 0.5's release model; all supported platforms;
  use the Phase 9.0 identity and its dated
  web/GitHub/package-channel/preliminary-trademark screen;
  smoke-test the exact published artifacts (`--version`, `--help`,
  `self-test`, one deterministic JSON workflow, dependency inspection)
- ☐ **9.5** — **Model: Sol High. Coverage acceptance** (PLAN §5.14) — archive replaced generic
  implementations in both validation engines; retain declarative configs,
  thin CI/policy glue and engine-specific residuals; classify exceptions
- ☐ **9.6** — **Model: Terra High. Release-candidate usability exercise:** a third-party engine pair
  driven only by published docs completes fixed match, SPRT and short SPSA;
  triage feedback before release without making a volunteer a permanent gate
- ☐ **9.7** — **EXIT / ACCEPTANCE · Model: Sol High.** Both validation engines run one real gate
  through the released artifact on ≥2 operating systems and agree with 8.1;
  independent CLI version/tag/artifact/release notes verified

## Recurring procedures

Not steps — they are never "done".

### Declaring a platform supported

- Full test suite green there, debug **and** release.
- Affinity, process, timer and filesystem capabilities or fallbacks implemented,
  tested and documented there.
- The exact released CLI artifact passes `--version`, `--help`, headless
  `self-test` and one deterministic JSON-mode workflow.

### After changing anything that runs games

- Re-run the Phase 4B oracle replay and controlled live parity on compatible
  shared fields; repeat the release-candidate matrix at Phase 8.1.
- Consider a real-machine calibration after material clock, scheduling or
  affinity changes; it is evidence, not a release or usage prerequisite.
- Bump the harness version in run records, and `stats_version` if any reported
  statistic changed definition — with a changelog entry.

### After completing a generic workflow

- Migrate the corresponding Rarog and Basilisk harness workflow immediately.
- Compare old/new resolved inputs, schedule, durable artifacts and statistics.
- Archive the old generic implementation only after parity; retain declarative
  configs and thin project-policy/CI invocation.
- Record an exception as either a CLI mechanism gap or intentional
  engine-specific policy.

## What to do now

**Phase 4A.7 — durable and structured output. Model: GPT-5.6 Terra — High.**
Compose live/structured reporting, append-only logs, PGN, failed-game traffic,
strict JSON stdout and documented exit codes with the common run directory.

```
git diff --check
```

## Working rhythm

```text
Pick the next ☐ step  ->  implement + test  ->  demonstrate its exit
criterion  ->  mark it ☑ here  ->  update PLAN.md when useful  ->  commit before
the next step.
```

Long game jobs (optional calibrations, parity runs, real gates) run on a real
machine and are pasted back; everything else is verified locally by tests.
Use a focused imperative commit subject, stage only the step's files and never
add co-author or assistant-attribution trailers; `AGENTS.md` is the binding
repository workflow.

## Decision rules

| Situation | Action |
|---|---|
| A phase "works" but its exit criterion is not demonstrated | Not done. Do not proceed |
| An inner layer needs a GUI/SQLite/Tokio/OS type | Stop and introduce or correct an application port/adapter; dependencies point inward |
| Our statistic disagrees with a compatible external oracle | Root-cause before shipping — one of them is wrong and it may be ours |
| External tools disagree or do not expose the same model | Record the matrix limitation; prefer analytic fixtures; never average or compare unsupported fields |
| OS cannot support a capability (e.g. hard macOS affinity) | Record the advisory/off fallback in the run record and PLAN; fail only if the capability was explicitly requested |
| Tempted to make one of our defaults mandatory | It belongs in PLAN §S3 Tier B with a reason, or Tier C as advice — Tier A needs a silent-wrong-number failure mode |
| Feature exists in an external runner but not here | Phase 8.2 decides, with "does a general engine developer need it?" as the tie-breaker |
| Tempted to add engine-specific logic | It belongs in the engine's own tooling, not here |
| Engine project still needs scheduling/statistics/tuning/recovery code | Generic mechanism gap: add or explicitly decline it |
| Engine project keeps a run file or thin CI command | Expected project policy, not a CLI gap |
| Diagnostic heuristic looks stable | Report the observation; do not call it convergence or causation |
