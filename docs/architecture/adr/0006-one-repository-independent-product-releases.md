# ADR-0006: Keep one repository with independent product releases

- **Status:** Accepted
- **Date:** 2026-07-31
- **Relates to:** PLAN §S4 and Phase 0(c); finding CS-12

## Context

All current crates inherit workspace version `1.0.2`, and one unscoped
`vX.Y.Z` GitHub release builds only the `colosseum` GUI. The future CLI must be
versioned, packaged, smoke-tested and documented independently, while changes
to core/application/UCI/runner code must be validated against both products.

Splitting repositories would require publishing or pinning shared internal
crates and coordinating every boundary/statistics change across repositories.
No separate access-control, ownership, licensing or scaling requirement exists
to offset that cost.

## Decision

Keep the GUI, CLI and shared packages in this repository and Cargo workspace.
Treat `colosseum-gui` and `colosseum-cli` as independent products:

- each product package declares its own SemVer rather than inheriting a root
  product version;
- GUI tags use `gui-v<semver>` and CLI tags use `cli-v<semver>`;
- each tag creates one product-scoped GitHub Release with only that product's
  artifacts and release notes;
- GUI and CLI have separate changelogs/release-note streams;
- separate release workflows build only the tagged product, while required
  push/pull-request CI always validates the whole shared workspace;
- the exact tagged commit and `Cargo.lock` identify shared-code revisions; a
  shared package version is not presented as either product version.

Historic `v0.1.0` through `v1.0.2` tags/releases remain unchanged and are
documented as GUI releases. The next GUI release adopts the `gui-v` namespace;
the first CLI release uses the `cli-v` namespace chosen here regardless of the
public binary name resolved by Phase 0.6.

GitHub's repository-wide “latest release” is not authoritative for either
product because the most recently published GUI or CLI release would hide the
other. Product update/download code filters releases by tag prefix and stable
versus prerelease status. README links are product-specific and do not use
`/releases/latest`.

The detailed version, artifact, CI, release-note and migration contracts are in
[`release-architecture.md`](../release-architecture.md).

## Consequences

- Shared changes, fixtures and dependency updates stay atomic in one commit and
  lockfile.
- GUI and CLI can release at different cadences without artificial version
  bumps or mixed artifacts.
- Maintainers must select and validate the correct product tag and changelog;
  repository tooling automates that check.
- A commit may legitimately carry both a GUI and CLI tag with different
  versions when both products release the same shared revision.
- Generic GitHub “latest” links and the GUI's current latest-release API query
  must be replaced before independent releases begin.
- Shared crates remain implementation packages unless a later ADR establishes a
  real external publishing use case.

## Alternatives considered

### Keep one workspace version and `vX.Y.Z` release

Rejected. Every CLI-only or GUI-only release would bump the other product and
one release would mix unrelated notes and artifacts.

### Keep one tag but mark product artifacts by filename

Rejected. Versions and release notes would still be coupled, and update checks
could not identify which product version a release represents.

### Split GUI and CLI into separate repositories

Rejected. It creates cross-repository coordination for the application/core/UCI
boundary, fixture parity and security fixes without a concrete organizational
or distribution benefit.

### Publish shared crates and consume them by registry version

Rejected for the current product. It adds a public compatibility/release
surface solely to make a repository split possible. Revisit only if third-party
library consumers become an explicit supported audience.

### Use moving `gui-latest` and `cli-latest` tags

Rejected. Moving release tags weaken provenance and caching. Product-specific
release discovery filters immutable version tags instead.
