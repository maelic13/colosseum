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
| Branch / version | `cli`; Colosseum GUI **1.0.2** released. Independent CLI: **not started** |
| What exists | Phase 0 architecture and Phase 1 statistics are complete. `colosseum-core` / `-uci` / `-engine` are headless; paired normalized/logistic statistics, SPRT, fixed-N planning/resolution, typed errors, property tests, hermetic fixtures and explicit external-oracle boundaries pass in required CI |
| What is missing | The Phase 2 boundary migration and independent CLI composition root; CPU affinity, match/SPRT workflows, SPSA, durable runs, run records and trustworthy NPS |
| Validation engines | **Rarog** (Rust) and **Basilisk** (C++) — available, active, different languages and build systems. Any two UCI engines would serve; nothing depends on these |
| Platform status | Windows ☐ · Linux ☐ · macOS ☐ — support requires the full debug/release suite and documented capability fallbacks; calibration is optional and machine-specific |
| Next step | **Phase 2.5 — engine inspect/check** |
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
- ☐ **2.5** — **Model: Terra High.** `engine inspect`; `engine check` reports handshake, synchronisation,
  schema validation, option acceptance/no-failure, legal bounded search,
  stop/new-game/quit. Do not claim UCI option read-back
- ☐ **2.6** — **Model: Terra High.** `--dry-run`; JSON mode emits JSON only on stdout and diagnostics on
  stderr
- ☐ **2.7** — **Model: Sol High.** `self-test` launches the exact executable's hidden deterministic UCI
  stub mode and tests protocol, process containment/reaping, bounded
  stdout/stderr draining, persistence failures and one short match
- ☐ **2.8** — **Model: Sol High.** Run directories: unique default under `./colosseum-runs`; explicit
  `--dir` to resume; archive-on-restart; append-only logs; checksummed
  two-generation atomic checkpoints; config mismatch refusal
- ☐ **2.9** — **Model: Terra High.** Common read-only `status <run-dir>` plus run records containing
  schema/stats versions, official sample, host/capability summary and
  anomalies for every run, including aborted ones
- ☐ **2.10** — **EXIT · Model: Sol High.** Two arbitrary UCI executables pass path-only checks;
  run-file/all-CLI resolution is identical and an `extend` chain resolves
  byte-identically to a flattened file under every merge/unset/path-origin
  rule; the same master seed reproduces every sub-stream on every platform
  and a new consumer leaves existing streams bit-identical; durable/status
  suites and published-style headless `self-test` pass; identity generation
  is outside the domain, no hard-coded live-engine test path remains,
  pipe floods stay bounded, ignored-quit/descendant stubs are reaped,
  architecture tests prove no GUI dependency/data access, and the GUI
  suite remains green

### Phase 3 — CPU topology and affinity

- ☐ **3.1** — **Model: Sol High.** Physical-core/sibling detection per OS — Windows
  `GetLogicalProcessorInformationEx`, Linux `thread_siblings_list`, macOS
  `sysctl`. ⚠ Never infer siblings from logical CPU numbering
- ☐ **3.2** — **Model: Terra High.** Modes `auto` / `off` / explicit CPU list; configurable headroom
  (default 2 physical cores free)
- ☐ **3.3** — **Model: Sol High.** Respect Linux cpusets/cgroups, Windows processor groups and the
  current process's allowed CPU set rather than the machine total
- ☐ **3.4** — **Model: Terra High.** Allocate the configured `cores-per-engine` per game slot, independent
  of whichever UCI option controls the engine's worker count
- ☐ **3.5** — **Model: Sol High.** Keep A/B slots on the same P/E core class and NUMA locality where
  possible; record class/node and visible asymmetry
- ☐ **3.6** — **Model: Terra High.** Fail when requested placement is unavailable; allow and record `off`;
  report macOS as advisory or unavailable without prohibiting clock matches
- ☐ **3.7** — **Model: Terra High.** `capabilities` command printing what this platform can and cannot do
- ☐ **3.8** — **EXIT · Model: Sol High.** SMT, P/E, restricted-cpuset, processor-group, no-SMT and
  dual-socket fixtures pass; residency tests pass where enforceable;
  capability reporting documented

### Phase 4A — Fixed-match runner

- ☐ **4A.1** — **Model: Terra High.** `match` is fixed-N; two direct UCI engines; same executable with
  different options allowed
- ☐ **4A.2** — **Model: Sol High.** Time controls per side: movetime, sudden death, base+increment,
  fixed nodes/depth and configurable time margin
- ☐ **4A.2a** — **Model: Sol High. Clock accounting (PLAN §5.4a), explicit/versioned/recorded:**
  clock runs from finishing the write of `go` to finishing the read of
  `bestmove`, charging harness read latency and engine search start-up to the
  mover; `position` setup is not charged; monotonic source only; increment
  follows `E > R + M` forfeit, otherwise `max(0, R-E) + I`, with equality
  accepted; the margin is not sent to the engine; record model/version,
  margin, clock resolution and charged-elapsed min/median/max without
  claiming engine/harness overhead can be separated
- ☐ **4A.3** — **Model: Terra High.** Draw/resign/max-moves adjudication independently configurable and
  disableable; forward engine tablebase options but defer harness probing
- ☐ **4A.4** — **Model: Sol High.** Separate engine from infrastructure faults. Strict default: engine
  fault forfeits and invalidates above threshold zero; infrastructure fault
  is never scored; no selective retry/discard in statistical runs
- ☐ **4A.5** — **Model: Sol High.** Explicit concurrency and placement; report
  `concurrency × 2 × Hash` only as a memory lower bound; refuse memory only
  with a trusted explicit budget/cap
- ☐ **4A.6** — **Model: Terra High.** Optional book with deterministic order/start/plies/reuse reporting;
  no book starts from startpos with a diversity warning
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

**Phase 2.1 — Clean Architecture boundary migration. Model: GPT-5.6 Sol —
High.** Implement the Phase-0 target boundary: generic runtime participants,
application use cases and ports, plus GUI adapters for library/config data.

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
