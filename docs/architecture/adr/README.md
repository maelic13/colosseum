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

Phase 0.5 separately decides versioning, repository, CI and release mechanics.
Phase 0.6 decides product/CLI naming. Those decisions are intentionally not
pre-empted here.
