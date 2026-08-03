# ADR-0010: Version CLI documentation with the binary

- **Status:** Accepted
- **Date:** 2026-08-03
- **Relates to:** PLAN Phase 9.1; ADR-0006 and ADR-0009

## Context

Colosseum CLI has user contracts that must remain tied to the executable that
produced a result: command options, run-file and tune-file schemas,
`schema_version`, `stats_version`, evidence authority, and failure behavior.
The existing `docs/cli/` Markdown already grows with the implementation, but it
does not yet have an explicit publication or drift-prevention contract.

The documentation system must balance:

- exact versioning with a CLI tag and binary;
- web discoverability and useful repository browsing;
- offline access in a portable release archive;
- ordinary pull-request review and low contribution friction;
- command syntax generated from Clap rather than copied by hand.

## Decision

The canonical Colosseum CLI documentation is versioned Markdown in
`docs/cli/`, committed beside the source and reviewed in the same change as the
behavior it documents.

| Surface | Contract |
|---|---|
| Repository | `docs/cli/README.md` is the user guide index; all internal links are relative and work in a source checkout. |
| Web | The repository's rendered Markdown at the selected release tag is the V1 web documentation. A future static-site rendering may project the same files but is not a second source of truth. |
| Release archive | Every CLI archive includes `docs/cli/`, `README.md`, `CHANGELOG-CLI.md` and `LICENSE` from the exact release tag. |
| Command reference | `docs/cli/command-reference.md` is generated deterministically from the real Clap `Command` tree, including nested public subcommands; hidden implementation commands stay excluded. |
| Drift gate | A repository-owned Rust documentation tool provides write and `--check` modes. Required CI and release validation fail when regeneration differs from the committed reference. |
| Format contracts | Human explanation remains handwritten. Concrete run/tune schemas, version fields and examples are tested against the same serializers/parsers; format changes update the relevant changelog. |

The generator lives in a small unpublished workspace tool rather than in the
shipped executable. The CLI driving adapter exposes only a function returning
its Clap command specification; the tool recursively renders that model. This
keeps documentation mechanics out of the domain/application layers and does
not create a GUI-to-CLI dependency.

Generated command text owns syntax, defaults, allowed values and parser help.
Handwritten pages own concepts, workflows, tradeoffs, interpretation and
examples. They link to the generated reference rather than duplicating full
option tables.

User-facing documentation contains no implementation phase numbers or internal
architecture argumentation. Maintainer design and evidence remain in
`PLAN.md`, `GUIDE.md` and `docs/architecture/`.

## Consequences

- A tag identifies source, binary and documentation together; old versions
  remain browsable through Git tags and release archives.
- Portable users receive useful offline documentation without a network or a
  documentation-site deployment.
- Contributors edit ordinary Markdown and run one cross-platform Cargo command
  to update or check generated reference text.
- V1 does not require a separate static-site toolchain, hosting account or
  deployment gate. A later site can render the canonical files without moving
  their ownership.
- Parser prose still needs review: generation prevents structural drift but
  cannot make unclear Clap help clear automatically.

## Alternatives considered

### GitHub wiki

Rejected. Wiki history is separate from product tags, is absent from source and
release archives, and makes documentation changes harder to review with code.

### Dedicated static-site source tree

Rejected for V1. It adds navigation/build/deployment machinery without
improving the canonical versioning or offline contract. Static rendering remains
an optional projection of `docs/cli/`.

### Handwritten command reference

Rejected. The large nested command surface would inevitably drift from Clap's
accepted arguments, defaults and value sets.

### Runtime-only `--help`

Rejected as the complete documentation system. It is authoritative for one
command path but is difficult to browse across the entire surface and cannot
replace conceptual or result-interpretation guidance.
