# ADR-0005: Migrate incrementally around working adapters

- **Status:** Accepted
- **Date:** 2026-07-31
- **Relates to:** PLAN Phase 0/2; all current-state findings

## Context

Colosseum already has substantial working behavior: pure statistics and
scheduling, UCI compatibility, per-game process isolation, legality/SAN,
adjudication, opening handling, PGN output, batched SQLite persistence, resume,
live GUI state and stored configuration compatibility. A big-bang rewrite would
put those behaviors and old-engine quirks at risk before the CLI gains value.

At the same time, adding a CLI directly beside the current scheduler would
cement the wrong boundaries and create a second workflow implementation.

## Decision

Use branch-by-abstraction inside GUIDE step 2.1. Introduce application contracts
first, adapt existing behavior behind them, migrate the GUI, then remove the old
surfaces. The local slices below are not independently complete GUIDE steps or
commits; step 2.1 is checked and committed only when its full exit evidence is
met.

### Slice 1: Establish inner contracts

- add `colosseum-application` with the accepted models, failures and ports;
- add generic participant/run IDs and remove entropy acquisition from domain
  constructors;
- move pure option boundary values/policy out of core without behavior changes;
- add dependency checks and fake-port application test composition.

### Slice 2: Wrap existing adapters

- wrap `EngineProcess` as `EngineSessionFactory`/session;
- wrap the existing runner as `GameExecutor`, preserving fresh processes,
  legality, SAN, adjudication and protocol diagnostics;
- split Tokio task mechanics from scheduler into `ExecutionPool`;
- adapt SQLite, PGN/openings and incidents to repository/artifact/source ports;
- pin current adapter behavior with contract tests before moving policy.

### Slice 3: Move GUI-owned data and mapping

- introduce serde-compatible GUI library/config/path DTOs;
- map GUI selections through the shared application resolver;
- retain explicit GUI ID correlation and writeback/read-model adapters;
- keep existing files, database and visible behavior compatible.

### Slice 4: Move workflow policy inward

- move detection/check orchestration, scheduling, commit order, fault policy and
  generic snapshots into application use cases;
- leave Tokio, UCI, SQLite, filesystem and GUI mapping behind ports;
- make repository commit precede official statistics and progress publication.

### Slice 5: Remove globals and concrete boundary leaks

- replace the process-wide incident directory with run-scoped `ArtifactSink`;
- remove concrete `Store`, `SpawnOptions`, channels, `Arc<Mutex>` live handles,
  direct clocks and random UUID creation from use-case inputs;
- convert task loss and write failures to typed infrastructure-invalid states.

### Slice 6: Narrow and remove compatibility surfaces

- narrow crate-root exports to their accepted owners;
- remove temporary compatibility constructors/re-exports;
- prove the final Cargo dependency rules;
- leave the GUI suite and storage-compatibility tests green before step 2.1 is
  marked complete and committed.

Temporary shims must be narrow, documented as migration-only and unused by new
CLI code. They may translate old types to the accepted boundary but may not add
new policy, silently drop fields or permit an outward dependency. Every shim has
a named removal condition in Slice 6.

No process pool is introduced. No working UCI parser/game loop/opening parser,
SQLite schema or GUI workflow is replaced wholesale. Behavior changes required
by strict durability, fault classification, bounded process I/O or cancellation
are implemented at the accepted seams and gated by their later numbered tests.

Later phases complete adapters whose detailed contracts are not yet
implemented: process containment/stub in 2.7, run-directory durability in
2.8–2.9, CPU placement in Phase 3 and clock/fault policy in Phase 4A. Phase 2.1
must still establish their ports and explicit unsupported/fake adapters; later
features may not bypass the boundary while waiting for implementation.

## Migration invariants

At every slice:

- the workspace builds and existing behavior tests remain green;
- engine processes are still fresh per game and owned through shutdown;
- option-name matching remains exact/allowlist-based;
- schedule insertion remains batched;
- GUI JSON/TOML/SQLite/preset fixtures remain compatible;
- no persistence/artifact/task failure is newly swallowed;
- new application code depends only inward and is testable with fakes;
- current engine crashes/protocol quirks retain their diagnostics and are not
  hidden to make tests pass.

Step 2.1 is complete only when the application package and GUI adapter are the
real workflow boundary, the temporary shims are removed, architecture tests
pass and the GUI remains operational. Merely adding traits or compiling a CLI
skeleton is not completion.

## Consequences

- The riskiest behavioral code is preserved and surrounded by contracts before
  policy moves.
- Phase 2.1 is larger than a scaffold-only step, but it ends with a real
  enforceable boundary rather than permanent transitional architecture.
- Temporary duplication exists while the GUI migrates; explicit removal gates
  prevent it becoming a second model.
- Later durability/process/affinity work plugs into already-owned ports instead
  of reopening architecture.
- Each local slice is diagnosable and reversible before the single completed
  step commit, while user commits and persisted data remain untouched.

## Alternatives considered

### Rewrite scheduler, runner and persistence together

Rejected. It combines architecture change with behavior replacement across the
highest-risk process, chess and durability paths.

### Add the CLI first and refactor after feature parity

Rejected. New commands would depend on concrete GUI/SQLite/Tokio seams, making
the temporary design the de facto public architecture.

### Fork the runner for CLI use

Rejected. Fault, clock, UCI compatibility and game-rule fixes would drift across
two implementations and undermine parity.

### Keep permanent compatibility facades

Rejected. They would preserve mixed ownership and let new code bypass the
application boundary. Compatibility belongs in temporary adapters or persisted
format readers, not permanent inner APIs.

### Change persisted formats during ownership migration

Rejected unless independently necessary. Rust module ownership does not justify
user-data migration risk; any later schema evolution receives its own versioned
migration and tests.
