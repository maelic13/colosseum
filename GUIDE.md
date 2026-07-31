# Colosseum CLI — development guide

The short operational view: where the harness stands and what to do next.
Rationale, specifications, success criteria and evidence live in
[`PLAN.md`](PLAN.md).

**This file and `PLAN.md` are the maintainer-facing pair.** `README.md` is the
user-facing front door for the whole project (what Colosseum is, the GUI, the
CLI); the user documentation is user-facing CLI detail. Neither may carry phase
numbers, internal naming or method argumentation.

## Current checkpoint

| | |
|---|---|
| Branch / version | `cli`; Colosseum GUI **1.0.2** released. CLI: **not started** |
| What exists | `colosseum-core` / `-uci` / `-engine` are headless (no `egui` dependency), cross-platform, released for Windows/Linux/macOS incl. arm64, 149 passing tests. `core/stats.rs` has trinomial SPRT, Elo ± error, LOS, ML ratings. Phase 0.1's dependency/module inventory is complete |
| What is missing | Clean Architecture boundary design, pentanomial/nElo, CPU affinity, the CLI itself, SPSA, durable runs, run records, trustworthy NPS |
| Validation engines | **Rarog** (Rust) and **Basilisk** (C++) — available, active, different languages and build systems. Any two UCI engines would serve; nothing depends on these |
| Platform status | Windows ☐ · Linux ☐ · macOS ☐ — support requires the full debug/release suite and documented capability fallbacks; calibration is optional and machine-specific |
| Next step | **Phase 0.2 — write the current-state architecture analysis** |

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
        section and never get a status marker. -->

Each phase ends with a verifiable exit criterion — see PLAN §S8. Nothing is
"done" because it compiles; it is done when its criterion is demonstrated.

### Phase 0 — Current-state analysis and target architecture

- ☑ **0.1 — DONE** — Inventory the current crate/module dependency graph with
  `cargo metadata`, `cargo tree` and source inspection — evidence:
  [`docs/architecture/dependency-inventory.md`](docs/architecture/dependency-inventory.md)
- ☐ **0.2** — Write `docs/architecture/current-state.md`: responsibilities, public
  boundary types, I/O/global state, framework dependencies, GUI/release
  coupling and every violation of PLAN §S4; explicitly audit UUID
  generation and branding/path policy in `colosseum-core`, GUI config/store
  seams, incident globals, SQLite scheduling, external-engine test paths,
  workspace-version inheritance, GUI-only release automation and CI coverage
- ☐ **0.3** — Write `docs/architecture/target-architecture.md` using Clean
  Architecture: domain, application use cases/ports, adapters, drivers,
  composition roots, error/cancellation flow and current-to-target migration
- ☐ **0.4** — Record ADRs for package boundaries, runtime `EngineLaunchSpec`,
  injected persistence/artifact/affinity/identity/master-seed ports,
  GUI-library mapping and the smallest refactor that enforces inward
  dependencies
- ☐ **0.5** — Design independent CLI/GUI versions, tags, artifacts, release notes
  and shared-layer regression CI. Prefer one repo; split only with a
  documented concrete advantage
- ☐ **0.6** — Resolve the existing “Coliseum” naming/search/package collision and
  record the decision plus rejected alternatives
- ☐ **0.7** — **EXIT** — both architecture documents and ADRs reviewed; every module
  has a target owner; dependency/release diagrams and independence tests are
  specified; naming/release decisions recorded

### Phase 1 — Pentanomial statistics and nElo (`colosseum-core`)

- ☐ **1.1** — Pair-level scoring into the pentanomial vector; incomplete pairs are
  excluded from pentanomial SPRT and labelled unpaired elsewhere
- ☐ **1.2** — Pentanomial variance, normalized Elo, logistic Elo ± error, LOS, draw
  ratio, pairs ratio, WL/DD ratio
- ☐ **1.3** — SPRT over **both** the pentanomial/normalized and logistic models,
  selectable, always naming the model in force
- ☐ **1.4** — Fixed-N planning/achieved-resolution primitives with explicit
  significance, power and assumed pair distribution; SPRT conclusions
  remain limited to their declared hypotheses
- ☐ **1.5** — Typed errors for degenerate inputs — no NaN/Inf escapes
- ☐ **1.6** — Property tests: LLR zero at N=0, monotone in score, arm-swap symmetry,
  bounds equal `log(β/(1−α))` and `log((1−β)/α)`
- ☐ **1.7** — **Fixture corpus**: a documented generator that plays any two UCI
  engines through `fastchess` and `cutechess-cli`; commit engine/tool
  identity, versions, hashes/licence provenance, exact commands and logs.
  Define a per-field oracle matrix plus analytic hand-derived fixtures
- ☐ **1.8** — Make the required CI suite hermetic: no required test reads outside
  the repository; real-engine smoke tests are explicit opt-in,
  environment-only and never count as release/platform evidence
- ☐ **1.9** — **EXIT** — analytic fixtures and every compatible oracle-matrix cell
  pass; no unsupported field is compared or silently guessed

### Phase 2 — Architecture migration, CLI skeleton and durable foundation

- ☐ **2.1** — Implement the Phase-0 boundary migration: generic runtime participant
  type, application use cases/ports and GUI adapter for library/config data
- ☐ **2.2** — Add independently versioned CLI package and composition root;
  `--version`/`--help`; no GUI/windowing dependency
- ☐ **2.3** — Direct engine controls: executable, optional label, arguments, cwd,
  environment, arbitrary UCI options and allocated cores
- ☐ **2.4** — Resolution order is built-in defaults < committed run TOML (with its
  `extend` chain) < CLI; write the fully resolved JSON/config hash. Run
  files use one parent, maximum depth 16, canonical cycle detection,
  recursive table merge, scalar/whole-array replacement and RFC 6901
  `unset`; preserve each file's path origin and name bad chains/pointers
- ☐ **2.4a** — One master seed derives an independent stream per consumer **by
  stream name**, not by draw order, so adding a consumer cannot shift an
  existing stream. Pin PLAN §5.0's u64/SHA-256/ChaCha12 contract, sampling
  algorithms and golden vectors. Generate and record the seed when absent;
  derivation changes require a `stats_version` change
- ☐ **2.5** — `engine inspect`; `engine check` reports handshake, synchronisation,
  schema validation, option acceptance/no-failure, legal bounded search,
  stop/new-game/quit. Do not claim UCI option read-back
- ☐ **2.6** — `--dry-run`; JSON mode emits JSON only on stdout and diagnostics on
  stderr
- ☐ **2.7** — `self-test` launches the exact executable's hidden deterministic UCI
  stub mode and tests protocol, process containment/reaping, bounded
  stdout/stderr draining, persistence failures and one short match
- ☐ **2.8** — Run directories: unique default under `./colosseum-runs`; explicit
  `--dir` to resume; archive-on-restart; append-only logs; checksummed
  two-generation atomic checkpoints; config mismatch refusal
- ☐ **2.9** — Common read-only `status <run-dir>` plus run records containing
  schema/stats versions, official sample, host/capability summary and
  anomalies for every run, including aborted ones
- ☐ **2.10** — **EXIT** — two arbitrary UCI executables pass path-only checks;
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

- ☐ **3.1** — Physical-core/sibling detection per OS — Windows
  `GetLogicalProcessorInformationEx`, Linux `thread_siblings_list`, macOS
  `sysctl`. ⚠ Never infer siblings from logical CPU numbering
- ☐ **3.2** — Modes `auto` / `off` / explicit CPU list; configurable headroom
  (default 2 physical cores free)
- ☐ **3.3** — Respect Linux cpusets/cgroups, Windows processor groups and the
  current process's allowed CPU set rather than the machine total
- ☐ **3.4** — Allocate the configured `cores-per-engine` per game slot, independent
  of whichever UCI option controls the engine's worker count
- ☐ **3.5** — Keep A/B slots on the same P/E core class and NUMA locality where
  possible; record class/node and visible asymmetry
- ☐ **3.6** — Fail when requested placement is unavailable; allow and record `off`;
  report macOS as advisory or unavailable without prohibiting clock matches
- ☐ **3.7** — `capabilities` command printing what this platform can and cannot do
- ☐ **3.8** — **EXIT** — SMT, P/E, restricted-cpuset, processor-group, no-SMT and
  dual-socket fixtures pass; residency tests pass where enforceable;
  capability reporting documented

### Phase 4A — Fixed-match runner

- ☐ **4A.1** — `match` is fixed-N; two direct UCI engines; same executable with
  different options allowed
- ☐ **4A.2** — Time controls per side: movetime, sudden death, base+increment,
  fixed nodes/depth and configurable time margin
- ☐ **4A.2a** — **Clock accounting (PLAN §5.4a), explicit/versioned/recorded**:
  clock runs from finishing the write of `go` to finishing the read of
  `bestmove`, charging harness read latency and engine search start-up to the
  mover; `position` setup is not charged; monotonic source only; increment
  follows `E > R + M` forfeit, otherwise `max(0, R-E) + I`, with equality
  accepted; the margin is not sent to the engine; record model/version,
  margin, clock resolution and charged-elapsed min/median/max without
  claiming engine/harness overhead can be separated
- ☐ **4A.3** — Draw/resign/max-moves adjudication independently configurable and
  disableable; forward engine tablebase options but defer harness probing
- ☐ **4A.4** — Separate engine from infrastructure faults. Strict default: engine
  fault forfeits and invalidates above threshold zero; infrastructure fault
  is never scored; no selective retry/discard in statistical runs
- ☐ **4A.5** — Explicit concurrency and placement; report
  `concurrency × 2 × Hash` only as a memory lower bound; refuse memory only
  with a trusted explicit budget/cap
- ☐ **4A.6** — Optional book with deterministic order/start/plies/reuse reporting;
  no book starts from startpos with a diversity warning
- ☐ **4A.7** — Live/structured output, full log, PGN and failed-game traffic;
  JSON-only stdout mode and documented exit codes
- ☐ **4A.8** — **EXIT** — path-only/no-book and paired-book matches pass; fault
  injection never scores infrastructure failures; output/resume/schedule
  tests pass; a stub sleeping a commanded duration is charged it within
  tolerance on every platform, a sub-margin overrun is not forfeited while a
  super-margin one is and is attributed correctly, exact equality is
  accepted, a mid-game system-clock change does not alter the result, and
  below/at/above increment-margin boundaries have fixtures

### Phase 4B — Pair-atomic SPRT and parity

- ☐ **4B.1** — Explicit hypotheses/error rates/model and finite `max-pairs`;
  `gainer`/`simplify` are named bundles
- ☐ **4B.2** — Opening colour-pair is atomic. Commit complete pairs in deterministic
  schedule order; never evaluate an incomplete pair
- ☐ **4B.3** — At a boundary, schedule no new pairs, complete a half-pair, and
  exclude separately stored post-terminal work from the official sample
- ☐ **4B.4** — Exit/reporting distinguishes H1/H0/inconclusive/invalid/error and
  includes model, hypotheses, LLR, bounds, cap and terminal pair
- ☐ **4B.5** — Replay identical ordered outcomes through compatible external
  statistics; controlled live parity compares only shared fields
- ☐ **4B.6** — **EXIT** — analytic/oracle parity; concurrency cannot change the
  terminal pair; every terminal/fault case passes; live differences
  root-caused before Phase 5

### Phase 4C — Optional calibration

- ☐ **4C.1** — Byte-identical binaries; representative TC/book/adjudication/
  concurrency/placement; configurable fixed N/confidence/tolerance
  (defaults 30k / 95% / ±5 nElo); never a prerequisite
- ☐ **4C.2** — PASS iff interval is inside tolerance; FAIL iff wholly outside one
  edge; overlap is INCONCLUSIVE; any engine fault is INVALID
- ☐ **4C.3** — **EXIT** — hash/config/persistence checks, deterministic tests for
  every outcome and one real-machine smoke run

### Phase 5 — SPSA

- ☐ **5.1** — Implement PLAN §5.5's exact seeded Rademacher perturbation, arm
  construction, Fishtest-compatible `c/a/r` schedule, update, clipping and
  send-time rounding; decay per iteration
- ☐ **5.2** — Back-solve from each `c_end`, run `r_end` and horizon; persist exact RNG
  algorithm/seed/draw order; assert written schedule before play
- ☐ **5.3** — Tune TOML selects numeric UCI options with initial value, bounds and
  `c_end`; validated against the live UCI option schema
- ☐ **5.4** — Defaults 5,000 iterations and 32 games/iteration — configurable,
  neither enforced as a minimum
- ☐ **5.5** — Persistent driver, book loaded once, complete paired mini-match as the
  commit unit; engine fault invalidates rather than becoming a gradient;
  multi-session per the durable contract
- ☐ **5.6** — Config audit: reject absent/non-spin options, duplicates, bounds
  outside the engine's range, `min>=max`, and perturbations rounding to
  zero; warn on default disagreement and a seed on a rail
- ☐ **5.7** — **Close the loop** — rounded mean over frozen final-10% window as
  setoptions/JSON/run fragment; `sprt --apply` gates original versus tuned
  vector with the same executable hash unless explicitly overridden
- ☐ **5.8** — `spsa plan` reports factual schedule/game/cost/resolution information.
  Optional convergence simulation requires an explicit synthetic model and
  is never presented as a chess-convergence forecast
- ☐ **5.9** — `spsa status` reads an atomic snapshot and reports trajectory/thirds,
  boundary contact, little seed movement, stability and dead perturbation
  as labelled heuristics—never causal proof or automatic advice
- ☐ **5.10** — **EXIT** — schedule property tests; every hard audit class rejected
  by a fixture; exact RNG stream survives resume; synthetic convergence
  smoke test passes; plan arithmetic/status match fixtures; stub tune feeds
  `sprt --apply` unedited with verified executable hash

### Phase 6 — Speed, planning, replay, books and position suites

- ☐ **6.1** — `nps` uses harness monotonic wall time over fixed-node searches;
  reported nodes verify work; engine time/nps is diagnostic only
- ☐ **6.2** — One or more executables per arm with per-executable medians; self pair
  optional; seeded order, warm-up, cold/warm state policy, strict
  alternation, median/best-of, bootstrap CI and per-round SD
- ☐ **6.3** — Scaling sweep over explicit engine thread counts: matching physical
  cores, pinned workload, fixed-total/per-thread Hash policy, wall-time
  speedup/efficiency, CPU class/NUMA recorded
- ☐ **6.4** — `book slice` / `hash` / `stats` / `verify`
- ☐ **6.5** — `stats` authority: structured run store > PGN export > forensic log >
  console; missing pair identity falls back to labelled unpaired statistics
- ☐ **6.6** — `stats plan fixed|sprt` with explicit assumptions; fixed-N required
  pairs/achieved resolution and seeded SPRT expected-length simulation
- ☐ **6.7** — PGN telemetry lists supported annotations and coverage; excludes
  opening moves; reports unavailable rather than zero; warns that node
  semantics must be compatible
- ☐ **6.8** — `suite` runs EPD/FEN at fixed time/nodes/depth with `bm`/`am`,
  per-position results, aggregate pass rate and compatible-baseline compare
- ☐ **6.9** — **EXIT** — fake engine-reported nps cannot affect authoritative speed;
  skew/scaling/state-policy tests pass; slicing/replay/planning/telemetry and
  EPD suite match fixtures

### Phase 7 — Tournaments

- ☐ **7.1** — One `tournament` use case supports round-robin and one/multi-seed
  gauntlet; optional `gauntlet` alias has no second implementation
- ☐ **7.2** — Joint ML ratings/error bars, optional anchor, standings/crosstable CSV
  and durable resume for both formats
- ☐ **7.3** — **EXIT** — schedules/ratings match GUI (≤0.01 Elo); deterministic
  kill/resume produces identical standings for both formats

### Phase 8 — Parity against external runners, and remaining gaps

- ☐ **8.1** — Repeat Phase-4B parity with current supported external versions and
  the exact release candidate, comparing only oracle-matrix shared fields
- ☐ **8.2** — Remaining feature gaps: adopt / decline with a reason / defer.
  Candidates: Chess960, ponder, harness Syzygy adjudication, additional
  formats and whether datagen now has generic needs beyond a match recipe.
  Tie-breaker: does a general engine developer need it?
- ☐ **8.3** — **EXIT** — parity demonstrated and every gap has a recorded decision

### Phase 9 — Documentation and release

- ☐ **9.1** — **Documentation placement analysis**: in-repo `docs/` published as a
  static site, GitHub wiki, or generated reference plus guides. Criteria:
  versioning with the binary (a wiki does not version, which matters once
  `stats_version` exists), discoverability, offline availability,
  contribution friction, and whether the command reference can be generated
  from the argument parser so it cannot drift. Record the decision
- ☐ **9.2** — README as the project front door — what Colosseum is, the GUI, the
  CLI, install, links
- ☐ **9.3** — User documentation: quickstart, command reference, run-file and
  tune-file reference, a worked example per command, "how to trust a result"
  from PLAN §S3 Tier C, and a compatibility page (what the tool needs from a
  UCI engine, what it does with non-conforming ones). State that engines are
  separate processes and direct users to applicable licences; make no
  blanket legal conclusion
- ☐ **9.4** — Ship per Phase 0.5's release model; all supported platforms;
  smoke-test the exact published artifacts (`--version`, `--help`,
  `self-test`, one deterministic JSON workflow, dependency inspection)
- ☐ **9.5** — **Coverage acceptance** (PLAN §5.14) — archive replaced generic
  implementations in both validation engines; retain declarative configs,
  thin CI/policy glue and engine-specific residuals; classify exceptions
- ☐ **9.6** — **Release-candidate usability exercise** — a third-party engine pair
  driven only by published docs completes fixed match, SPRT and short SPSA;
  triage feedback before release without making a volunteer a permanent gate
- ☐ **9.7** — **EXIT / ACCEPTANCE** — both validation engines run one real gate
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
- Record an exception as either a Colosseum mechanism gap or intentional
  engine-specific policy.

## What to do now

**Phase 0.2 — write the current-state architecture analysis before creating the
CLI crate.** Use the completed
[`dependency inventory`](docs/architecture/dependency-inventory.md) to classify
responsibilities, public boundary types, I/O/global state, framework and
GUI/release coupling, and every violation of PLAN §S4.

The later Phase 0 steps design the target boundaries, migration, independent
release/versioning and naming. Only after the Phase 0 exit may implementation
move to **Phase 1 — pentanomial statistics**.

```
cargo test -p colosseum-core
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
| Engine project keeps a run file or thin CI command | Expected project policy, not a Colosseum gap |
| Diagnostic heuristic looks stable | Report the observation; do not call it convergence or causation |
