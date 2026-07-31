# Architecture decision records

These records make the consequential decisions from
[`target-architecture.md`](../target-architecture.md) binding before the CLI
boundary migration begins. An accepted ADR may be changed only by a later ADR
that supersedes it; edits that merely clarify wording must not change the
decision.

| ADR | Status | Decision |
|---|---|---|
| [0001](0001-clean-architecture-package-boundaries.md) | Accepted | Add `colosseum-application` and enforce inward package dependencies |
| [0002](0002-runtime-engine-launch-spec.md) | Accepted | Use a minimal runtime `EngineLaunchSpec` for ordinary UCI executables |
| [0003](0003-application-ports-and-commit-boundary.md) | Accepted | Inject object-safe, runtime-neutral ports and make durable commit authoritative |
| [0004](0004-gui-library-runtime-mapping.md) | Accepted | Keep saved engine/library policy in the GUI and map it explicitly to runtime input |
| [0005](0005-incremental-boundary-migration.md) | Accepted | Migrate by abstraction around the working UCI, runner, SQLite and GUI behavior |
| [0006](0006-one-repository-independent-product-releases.md) | Accepted | Keep one repository with independently versioned and released GUI/CLI products |
| [0007](0007-name-the-cli-uci-rig.md) | Rejected | Name the independent CLI UCI Rig |

Phase 0.5's detailed release and CI design is in
[`release-architecture.md`](../release-architecture.md).
Phase 0.6's collision research and rejected public-name proposal are in
[`naming-decision.md`](../naming-decision.md).
