# ADR-0002: Use a minimal runtime engine launch specification

- **Status:** Accepted
- **Date:** 2026-07-31
- **Relates to:** PLAN §S4 and §5.0; finding CS-02

## Context

The current `EngineConfig` combines a saved GUI library ID, editable metadata,
rating, executable launch fields, chosen option values and the last detected
UCI schema. The concrete UCI `SpawnOptions` then duplicates part of that object.
This makes plain executable input awkward and risks turning a GUI library entry
or a new engine manifest into a requirement for the CLI.

Colosseum must accept ordinary UCI executables. The same executable with
different options is a valid A/B pair, and identity advertised by UCI is not a
substitute for run-local participant identity.

## Decision

`colosseum-application` owns a final, resolved `EngineLaunchSpec` containing
only:

| Field | Semantics |
|---|---|
| executable path | Canonical path to the finished executable, resolved before work is scheduled |
| arguments | Ordered user-supplied argument vector |
| working directory | Optional canonical directory; absence uses the process adapter's documented default |
| environment | Ordered configured overrides applied to the inherited process environment |
| display label | Optional presentation label; never identity or a UCI claim |
| UCI options | Final effective name/value map after configuration precedence and live-schema validation |
| allocated CPUs | Resolved logical CPU set assigned to this process, or explicit unrestricted/advisory state |

The Phase-2 Rust representation uses serializable standard-library/application
types (`PathBuf`, strings, ordered maps and an application CPU-allocation value),
not `tokio::process::Command`, OS handles or GUI types. Paths, arguments and
environment values must round-trip through the resolved JSON record. Non-Unicode
input is rejected clearly until a portable, lossless, versioned encoding is
specified; it is never converted lossily.

Relative user paths are resolved by the relevant configuration adapter using
the origin rules in PLAN §5.0/step 2.4. Construction fails before scheduling if
the executable or an explicitly supplied working directory cannot be resolved.
The UCI adapter maps the resolved spec once to its private process-spawn model;
application code never accepts `SpawnOptions` or `EngineProcess`.

The following values are deliberately separate:

- `RuntimeParticipant` contains a run-local `ParticipantId` and one launch
  spec. IDs come from `IdGenerator` or persisted resume state.
- UCI `id name`/`id author` and the advertised option schema are observations
  in `EngineInspection`, populated by a normal handshake.
- executable SHA-256, source path origin, host data and schema/stats versions
  belong to `ResolvedRunConfig`/the run record, not launch mechanics;
- opening books, suites, time controls, adjudication and scheduling are run
  inputs, not engine properties;
- rating priors and GUI writeback correlation are experiment/GUI data, not
  process launch data.

CPU allocation and engine worker count remain independent. A CPU set never
causes Colosseum to guess or modify a thread option; an explicit/safely
recognised UCI option controls engine threads under the command's policy.

No descriptor, manifest, build recipe, compiler identity, source-tree field,
custom bench/fingerprint command, logo, rating, author note or arbitrary
metadata extension map is added. A path-only invocation constructs a valid
spec with defaults, and a run file remains optional except for SPSA's parameter
vector.

## Validation

Phase 2 must prove:

- path-only construction for two arbitrary UCI executables;
- CLI and GUI inputs resolving to equal launch specs under equal settings;
- the same executable can occupy both arms with distinct options/labels;
- GUI-only fields cannot be serialized into or accessed from the runtime spec;
- adapter mapping preserves argument order, environment overrides, working
  directory, option values and allocated CPUs exactly;
- the resolved run record, not the launch spec, carries hashes and handshake
  identity.

## Consequences

- CLI users provide normal process/UCI controls and need no Colosseum-specific
  engine integration.
- GUI persistence may evolve independently of the runtime contract.
- Process spawning has one translation boundary instead of repeated copying.
- The spec is intentionally not a complete reproducibility record; that is the
  role of `ResolvedRunConfig` and the durable run record.
- Non-Unicode process arguments/environment are rejected clearly until a
  portable lossless encoding is specified.

## Alternatives considered

### Reuse `EngineConfig`

Rejected. It would make GUI IDs, ratings, metadata and detected schema part of
the CLI/application contract.

### Promote UCI `SpawnOptions`

Rejected. It is a concrete process-adapter type and lacks runtime participant,
effective option and CPU-allocation semantics.

### Require an engine descriptor or manifest

Rejected. It is non-standard, redundant with UCI handshake data and violates
the path-only product requirement.

### Include hashes and handshake identity in `EngineLaunchSpec`

Rejected. Those are evidence *about* resolved input and a launched engine, not
instructions for launching it. Mixing them would complicate preflight, resume
and re-inspection.

### Keep an arbitrary metadata map for future use

Rejected. Uninterpreted fields have no runtime responsibility and recreate the
descriptor/library coupling this boundary removes.
