# ADR-0007: Name the independent CLI UCI Rig

- **Status:** Rejected (accepted as the Phase 0.6 proposal, rejected by the
  maintainer before implementation on 2026-07-31)
- **Date:** 2026-07-31
- **Relates to:** PLAN Phase 0(d); ADR-0006

## Context

The released `colosseum` desktop GUI already owns the natural executable name.
The proposed “Colosseum CLI” also collides in search with an existing
ColosseumCLI for wireless test infrastructure, while a separate “Coliseum”
chess-engine GUI serves nearly the same audience. The two spellings are not a
supportable distinction in speech.

The CLI must remain independently identifiable and co-installable without
renaming working GUI storage, installer and application identities.

## Rejected decision

This decision is rejected and must not be implemented. ADR-0008 instead uses
Colosseum and `colosseum-cli` consistently through implementation, with an
optional whole-product naming review deferred to Phase 9.0.

Keep **Colosseum** as the desktop product name. Name the independent CLI product
**UCI Rig**, with Cargo package, crate and executable `ucirig` and workspace path
`crates/ucirig`.

Use `UCI Rig <semver>` for release titles and
`ucirig-<version>-<os>-<arch>` for archive basenames. Preserve ADR-0006's
`cli-v<semver>` tag namespace, CLI workflow/changelog lane and one-repository
architecture. Shared implementation packages retain their `colosseum-*` names.

Do not publish or document `colosseum-cli`, `colosseumcli`, `colosseum-lab` or
`colosseum` as aliases for the CLI. The GUI remains the sole owner of the
`colosseum` executable.

The research evidence, preliminary trademark limitations and pre-release
revalidation rule are in
[`naming-decision.md`](../naming-decision.md).

## Proposed consequences (not applied)

- Users can install and invoke Colosseum GUI and UCI Rig independently.
- CLI documentation and support have a short, searchable command that states
  the supported protocol.
- Public product naming differs from the repository and shared crate prefix;
  documentation must distinguish those intentionally.
- The proposal would have created `crates/ucirig` and used
  `cargo tree -p ucirig` for dependency tests.
- Phase 9 must repeat availability and preliminary trademark searches before
  first release. The current screen is evidence, not legal clearance or name
  reservation.

## Alternatives considered

### Retain “Colosseum CLI”

Rejected because a distinct binary suffix does not solve the existing
same-name CLI and near-identical chess-product search/spoken collisions.

### Rename only the executable under Colosseum branding

Rejected because users would still search for and discuss “Colosseum CLI”; the
collision is a product identity problem, not merely a PATH problem.

### Rename the desktop and CLI together

Rejected as disproportionate to the CLI goal. It would require migration of a
released GUI's package, installer, application-directory and saved-data
identities.
