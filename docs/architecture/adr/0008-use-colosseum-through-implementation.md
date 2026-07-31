# ADR-0008: Use Colosseum consistently through implementation

- **Status:** Accepted
- **Date:** 2026-07-31
- **Relates to:** PLAN Phase 0(d); ADR-0001, ADR-0006 and rejected ADR-0007

## Context

Phase 0.6 found genuine search, package-name and spoken-support collisions around
“Colosseum” and “Coliseum”. Its proposed split identity, UCI Rig, was rejected
before implementation. Further replacement candidates did not offer enough
meaning or confidence to justify renaming the released desktop application.

Implementation needs coherent, specific names. Making every surface generic in
anticipation of a possible one-time rename would weaken the code and documents
without improving the Clean Architecture boundaries. The maintainer accepts the
cost of a deliberate final rename if later evidence makes one worthwhile.

## Decision

Use this identity consistently throughout implementation:

| Surface | Binding implementation name |
|---|---|
| Project and desktop product | **Colosseum** |
| Desktop executable | `colosseum` |
| Desktop Cargo package | `colosseum-gui` |
| CLI product | **Colosseum CLI** |
| CLI executable and Cargo package | `colosseum-cli` |
| CLI workspace path | `crates/colosseum-cli` |
| Rust CLI crate import | `colosseum_cli` (Cargo's normal hyphen mapping) |
| Shared packages | `colosseum-core`, `colosseum-application`, `colosseum-uci`, `colosseum-engine` |
| Default run-directory parent | `./colosseum-runs/` |

The CLI package is independently versioned and is `publish = false` unless a
later explicit distribution decision establishes a crates.io use case.
ADR-0006's independent `gui-v<semver>` and `cli-v<semver>` release lanes remain
unchanged.

Do not add a brand-token service, neutral product aliases, dual commands or
other indirection solely to prepare for a hypothetical rename. Clean
Architecture still places display strings, executable/application paths,
installer metadata, updater URLs and packaging policy in outer adapters and
composition roots. Domain and application code must not acquire presentation
or GUI identity policy.

Add an optional decision gate at Phase 9.0, before final user documentation and
release. It may either retain Colosseum and record that outcome or select a new
shared stem and perform the complete one-time migration. A rename at that point
must cover repository identity, Cargo packages/crates, binaries, tags,
artifacts, release titles, installer/application IDs, config/data paths and
compatibility, updater URLs and documentation. It also repeats dated web,
same-domain, GitHub, package-channel and preliminary trademark checks.

## Consequences

- Code, commands, tests and architecture documents can use meaningful concrete
  names now.
- GUI and CLI naming is coherent while their composition roots, versions and
  releases remain independent.
- The Phase 0.6 collision risk is accepted, not erased; Phase 9.0 reviews it
  against the implemented product.
- A late rename will cost more than replacing placeholders. That cost is
  accepted in exchange for avoiding speculative abstractions and premature
  compromise.
- Stable technical identifiers such as persisted paths and RNG namespaces may
  require explicit compatibility or versioning if a later rename occurs.

## Alternatives considered

### Select another shared name during Phase 0

Deferred. No candidate had sufficient meaning or support from the maintainer to
improve on Colosseum.

### Use UCI Rig only for the CLI

Rejected by ADR-0007. It splits one project into unrelated public identities.

### Keep placeholders or build a rebranding layer

Rejected. This makes present implementation less specific for a migration that
may never happen and does not improve dependency direction.
