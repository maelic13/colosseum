# ADR-0004: Keep GUI library data outside runtime engine input

- **Status:** Accepted
- **Date:** 2026-07-31
- **Relates to:** PLAN §S4; findings CS-02 and CS-03

## Context

Saved GUI types currently live in shared packages: `EngineConfig`/`EngineMeta`
are in core, while `EngineLibrary`, `AppConfig` and `AppDirs` are in engine.
`RatingWriteback` and GUI refresh/live-state concepts also cross inner
boundaries. The CLI must not read these files or require a saved engine record,
yet the existing GUI JSON/TOML/SQLite data and behavior must remain compatible.

The mapping also must not give CLI and GUI separate option-precedence or launch
validation implementations.

## Decision

Move saved engine/library and desktop configuration ownership to
`colosseum-gui`. Preserve their serialized field names, defaults and tolerant
deserialization during migration.

The GUI adapter performs an explicit, pure boundary mapping. It extracts launch
fields from a selected saved engine and passes them, plus per-run selections,
through the application-owned launch/configuration resolver. The resolver—not
GUI widgets—applies the common/saved/per-run option precedence and produces the
final `EngineLaunchSpec` from ADR-0002. The CLI config adapter calls the same
resolver with CLI/run-file inputs.

| Saved/current value | Boundary treatment |
|---|---|
| `path`, ordered `args`, `working_dir`, `env` | Launch input, resolved/canonicalized before final spec construction |
| `meta.name` + `meta.version` | Optional display label using the established “name version” convention |
| saved UCI `options` | Base option layer; final effective values come from the shared resolver |
| `detected_options` | GUI cache/display aid and possible preflight hint; never authoritative over a live handshake |
| `EngineId` | GUI library/database identity only; GUI keeps an explicit `ParticipantId -> EngineId` correlation map |
| configured Elo | Tournament-start rating prior, separate from process launch |
| logo, author/extra metadata, notes | GUI presentation/library data only |
| `RatingWriteback` | GUI post-commit policy using the correlation map; absent from domain/application run policy |
| `AppConfig` / `AppDirs` / presets | GUI persistence and path-selection policy only |

The application creates or restores run-local participant identities through
`IdGenerator`/the repository. A GUI library ID is never used implicitly as a
runtime identity, though the adapter may preserve a lossless correlation for
history and writeback.

There is no general reverse mapping from `EngineLaunchSpec` to a saved library
entry. Application results return participant identities and neutral result
models; the GUI chooses whether and how to update its library. Adding a CLI
engine to a GUI library is a separate future user action, not a side effect of
running a command.

Global tablebase preferences, tournament common options and one-engine
overrides are converted to explicit run option layers. Missing or unrecognised
UCI option names remain visible and are validated against the live schema; the
mapper must not use substring guesses or silently discard them.

## Compatibility migration

1. Introduce GUI-owned serde DTOs matching the current stored shape.
2. Golden-test old/current JSON, TOML, preset and SQLite fixture round trips.
3. Add pure mapping tests from saved DTO + run choices to application inputs
   and final launch specs.
4. Switch GUI consumers behind temporary workspace compatibility shims.
5. Move rating writeback and GUI events/read models fully outward.
6. Remove shared `EngineConfig`/`EngineMeta`, GUI config re-exports and shims
   before Phase 2 exits.

Unknown/missing serde fields retain current tolerant behavior. No database or
file migration is introduced solely to move Rust ownership; if a schema change
later becomes necessary, it receives its own migration and compatibility tests.

## Verification

- Existing saved engine/config/preset fixtures deserialize and reserialize
  compatibly.
- Equal GUI and CLI source inputs yield equal final launch specs.
- Display labels remain “name version” while UCI identity is still recorded
  from handshake.
- GUI ID/rating/logo/detected schema cannot enter `EngineLaunchSpec`.
- Rating writeback changes the GUI library only after a committed application
  result and never changes official experiment statistics.
- CLI tests fail if they access GUI app directories, files or SQLite history.

## Consequences

- The GUI keeps its established library experience and backward compatibility.
- CLI users are not asked to create or maintain GUI records.
- Mapping code becomes an explicit testable adapter rather than scattered field
  copying.
- The GUI must maintain a correlation map for history/writeback instead of
  leaking its persistent IDs into runtime types.
- A live handshake, not a stale detected-options cache, remains authoritative.

## Alternatives considered

### Keep library types in core as “shared configuration”

Rejected. They contain presentation, persistence and entropy concerns rather
than domain invariants and would keep the CLI coupled to GUI concepts.

### Let CLI read the GUI library optionally

Rejected. Even optional integration creates app-directory/version coupling and
weakens the independent CLI contract. Users can express the same launch fields
through CLI arguments or an optional run file.

### Use the saved `EngineId` as runtime participant identity

Rejected. Path-only CLI runs have no such ID, and the same saved executable may
appear in multiple experimental arms with different options.

### Make cached detected options authoritative

Rejected. The executable may have changed, the cache may be stale and UCI
provides the live schema during normal handshake.

### Write application results directly back into the library

Rejected. It makes GUI persistence an application side effect and gives the CLI
an irrelevant dependency. Writeback remains an explicit GUI policy.
