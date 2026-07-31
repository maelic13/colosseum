# Phase 0 integrated architecture review

This is the evidence artifact for GUIDE step 0.7. The review was performed on
2026-07-31 after commit `9ceedc6`, using the actual workspace plus the Phase 0
inventory, current-state report, target architecture, ADRs and release design.
It reviews responsibility placement and testability; it does not claim that the
target has already been implemented.

## Verdict

The Clean Architecture target is coherent, incremental and sufficient to build
an independent CLI without making ordinary UCI engines adopt manifests or
project-specific conventions. Every current Rust source module and
consequential public boundary has one target owner, the dependency and release
designs agree, and every independence invariant has an executable test owner.

Phase 0 has **not exited**. The accepted architecture is brand-independent, but
the rejected Phase 0.6 split-brand decision left the future package, command,
artifact and path names inconsistent. The review corrected those references to
`<name>` / `<name>-cli`. Phase 0.8 must select the shared name with the
maintainer, apply its migration matrix and replace ADR-0007 before Phase 1 or 2
implementation starts.

## Evidence reproduced

The review reran:

```text
cargo metadata --no-deps --format-version 1
cargo tree --workspace --edges normal
rg --files crates
```

Cargo still reports the four-package baseline and these direct internal edges:

```text
colosseum-core   -> (none)
colosseum-uci    -> colosseum-core
colosseum-engine -> colosseum-core, colosseum-uci
colosseum-gui    -> colosseum-core, colosseum-engine
```

This matches the current-state and dependency-inventory diagrams. The source
graph has not changed during the documentation-only Phase 0 work; package and
module names in those two baseline documents therefore remain factual even
though Phase 0.8 will decide their migration.

The source-file stems under each `crates/*/src` directory were compared with
the exact per-package tables under “Current-module migration map” in
[`target-architecture.md`](target-architecture.md):

| Current package | Source modules | Target rows | Missing | Duplicate/extra |
|---|---:|---:|---|---|
| `colosseum-core` | 15 | 15 | none | none |
| `colosseum-uci` | 6 | 6 | none | none |
| `colosseum-engine` | 12 | 12 | none | none |
| `colosseum-gui` | 17 | 17 | none | none |

The GUI `build.rs` is a Cargo build target rather than a Rust source module; it
is assigned with the GUI packaging assets in the release migration table. The
target public-boundary table also accounts for every consequential cross-layer
surface identified in the current-state report, including engine library data,
runtime launch input, UCI processes, runner DTOs, persistence rows, live state
and application directories.

## Responsibility and dependency review

| Contract | Accepted owner | Consistency result |
|---|---|---|
| Chess/statistical values and deterministic rules | Domain/core | PLAN S4, target architecture and ADR-0001 agree; entropy, paths, branding and GUI events migrate outward |
| Experiment policy, run invariants and failure/commit decisions | Application use cases | Target architecture and ADR-0001/0003 agree; neither CLI, GUI, Tokio nor SQLite owns shared policy |
| Ordinary UCI executable launch | Minimal application `EngineLaunchSpec` | ADR-0002/0004 agree: path plus standard process/UCI controls, no descriptor, build recipe, GUI library record or arbitrary metadata |
| UCI/process mechanics | UCI adapter/driver | Existing parser and learned compatibility behavior are preserved behind `EngineSessionFactory` |
| Game, storage, artifact, opening and CPU mechanics | Headless engine adapters | Existing runner/SQLite/opening/PGN behavior is wrapped, not rewritten; application policy is removed from adapters incrementally |
| Saved engines, app paths, rating writeback and presentation state | GUI adapter | ADR-0004 preserves stored formats and maps once into application input; CLI cannot read GUI state |
| Parsing, presentation, runtime and shutdown | Separate GUI and CLI composition roots | ADR-0001/0006 agree; neither root imports or invokes the other |
| Official result boundary | Application commit through `RunRepository` before statistics/progress | Target architecture and ADR-0003 use the same order and never score infrastructure failures |

The allowed target arrows in PLAN S4, the target Mermaid graph and ADR-0001
are equivalent: application depends only on domain; UCI and engine packages are
outward adapters; the two product packages depend inward and remain siblings.
`colosseum-engine -> colosseum-uci` is deliberate because the retained game
executor drives UCI sessions. It does not let the application depend outward.

## Independence and release test ownership

The executable assertions are specified in the target architecture and routed
to numbered implementation steps:

| Independence requirement | Executable evidence | Owner |
|---|---|---|
| CLI has no GUI/windowing dependency and roots do not depend on each other | Cargo-tree and source-edge architecture checks | 2.2, required by 2.10 |
| CLI reads no GUI library/config/app-data path | Isolated-root integration test with sentinel GUI directories | 2.4, required by 2.10 |
| Every CLI run is self-contained | Shared unique/create/resume/archive/checkpoint/recovery suite | 2.8–2.10 |
| Application is framework-independent | Every use-case family composes with deterministic fake ports | 2.1 and each feature step |
| GUI persisted data remains compatible | Golden JSON/TOML/preset/SQLite mapping fixtures | 2.1 |
| Durable state precedes official statistics/progress | Fake-repository order test plus write-failure/recovery injection | 2.1 and 2.8 |
| Published CLI is headless and self-contained | Unpacked-artifact help/version/self-test/JSON smoke in an isolated directory | 2.7 and 9.4 |
| GUI and CLI release independently | Product-version/tag/changelog/artifact routing checks | 2.2 and 9.4 |
| Shared changes validate both products | Required hermetic push/PR workspace matrix on Windows, Linux and macOS, debug and release | 1.8 and 2.2; exact release candidate again in 9.4 |

Release architecture remains one repository with two release lanes. That is
independent functionality, not separate source ownership: GUI and CLI have
independent versions, tags, changelogs, artifacts, workflows and smoke tests,
while shared changes are atomic and exercise both products. Historic unscoped
GUI releases remain immutable. Phase 0.8 supplies only the public/package
mapping; it does not change the `gui-v` / `cli-v` lane identities.

## Corrections made by this review

| Finding | Correction |
|---|---|
| Rejected `UCI Rig` / `ucirig` remained binding in future architecture, commands and releases | Future-facing documents now use role names and `<name>` / `<name>-cli`; UCI Rig remains only in its explicitly rejected research/ADR record |
| GUI and CLI naming work was conflated with the architecture exit | Step 0.7 is the integrated review; new step 0.8 owns the shared-name decision, migration and Phase 0 exit |
| Default run directory still embedded the old product name | Contract now uses provisional `./<name>-runs/`, resolved by 0.8 |
| RNG domain-separation text embedded the old brand | The unimplemented version-1 contract now uses stable, brand-neutral `chess-harness-rng-v1\0` |
| Phase 2.2 did not visibly own the CI/release foundation assigned to it by the release design | PLAN and GUIDE now require separate version/changelog lanes and required shared-workspace push/PR CI in 2.2; final publication remains 9.4 |
| Rejected ADR-0007 still used imperative consequence language | It is now explicitly a rejected decision with proposed, unapplied consequences |

No Rust source, persisted format or released artifact was changed in step 0.7.
The next architecture-affecting edit is the reviewed Phase 0.8 rename matrix;
after that, implementation can begin without an unresolved public identity or
an accidental GUI/CLI dependency.
