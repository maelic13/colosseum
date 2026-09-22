# ADR-0009: Retain Colosseum for the 1.0 product family

- **Status:** Accepted
- **Date:** 2026-08-03
- **Supersedes:** ADR-0008's temporary Phase 9.0 decision gate
- **Relates to:** PLAN Phase 9.0; ADR-0006, ADR-0007 and ADR-0008

## Context

ADR-0008 kept **Colosseum** and **Colosseum CLI** as concrete implementation
names while leaving one final whole-product naming decision before release.
The implemented product is now a coherent family: a desktop tournament GUI and
an independently versioned command-line tool for reproducible UCI engine
testing. A late change would therefore be a real product migration, not merely
a crate rename.

The required 2026-08-03 revalidation confirmed the known collision risk:

| Surface | Revalidated evidence | Result |
|---|---|---|
| Same chess domain | The separately maintained [Coliseum chess GUI](https://phelrin.itch.io/coliseum/devlog/1404384/coliseum-update-tournaments-drag-and-drop-and-human-vs-engine) advertises engine tournaments and testing under the near-homophone spelling. | Material spoken and search ambiguity remains. |
| Existing CLI | The wireless research platform still documents its [Colosseum CLI](https://colosseumwireless.readthedocs.io/en/latest/radio_api_traffic/colosseum_cli.html) and `colosseumcli` command. | Generic searches for the display name remain ambiguous; the executable spelling differs. |
| GitHub | Public repository-name searches returned three results for `colosseum-cli` and two for `colosseumcli`, including the wireless project's public client. | The source-search identity is crowded but usable with a chess-engine qualifier. |
| Planned package channel | crates.io searches found no exact `colosseum-cli` package; [`colosseum`](https://crates.io/crates/colosseum) and [`coliseum`](https://crates.io/crates/coliseum) remain occupied. V1 remains a binary release, not a crates.io publication. | The planned binary name is available at the check date; no registry name is claimed or reserved. |
| Preliminary trademark screen | Exact-name [TMview COLOSSEUM](https://www.tmdn.org/tmview/#/tmview/results?page=1&pageSize=30&criteria=I&basicSearch=COLOSSEUM) and [COLISEUM](https://www.tmdn.org/tmview/#/tmview/results?page=1&pageSize=30&criteria=I&basicSearch=COLISEUM) searches returned 177 and 128 records respectively, unchanged from 2026-07-31. Results span jurisdictions, classes and statuses. | The name is crowded. This screen is not legal clearance and TMview is not an official register. |

No accepted replacement emerged from the earlier naming exercise. The
maintainer explicitly values a meaningful shared GUI/CLI identity over a split
brand or a generic placeholder and accepts the cost of a full migration if a
later legal or practical trigger makes it necessary.

## Decision

Retain the existing identity for the 1.0 product family:

| Surface | Final 1.0 identity |
|---|---|
| Project and desktop product | **Colosseum** |
| Desktop executable / package | `colosseum` / `colosseum-gui` |
| CLI product | **Colosseum CLI** |
| CLI executable / package | `colosseum-cli` |
| Shared packages | `colosseum-*` |
| Release lanes | `gui-v<semver>` and `cli-v<semver>` |

Use **“Colosseum chess-engine testing”** or **“Colosseum CLI for UCI chess
engines”** on introductory and search-oriented surfaces. Command examples use
the exact `colosseum-cli` spelling. Do not add `colosseumcli`, UCI Rig, or any
other compatibility alias.

The CLI remains independently installable, versioned and released. Shared
branding does not merge the GUI and CLI composition roots, runtime
dependencies, configuration, or release artifacts.

## Consequences

- Documentation can describe one coherent product family without a disruptive
  pre-release migration.
- Users may confuse this product with the similarly named chess GUI or wireless
  CLI. Descriptive qualifiers and exact executable spelling mitigate but do not
  eliminate that risk.
- This decision does not assert trademark availability. Obtain professional,
  jurisdiction- and class-specific clearance before commercial use,
  registration, or substantial promotion.
- A future rename is justified by a legal conflict, recurring support or
  discovery failures, or a replacement the maintainer judges materially
  stronger. It must be a complete migration, not a growing alias layer.

## Alternatives considered

### Rename the whole family now

Rejected. Earlier candidates did not improve both meaning and confidence, and
the known collision evidence has not materially changed. A speculative rename
would impose application-ID, path, installer, package, release and user
migration costs without an accepted destination.

### Give only the CLI a different name

Rejected by ADR-0007 and unchanged by this review. It would split one product
family while leaving the released desktop name untouched.

### Retain neutral aliases for a possible later rename

Rejected. Aliases would enlarge the permanent compatibility surface without
improving the current name or Clean Architecture boundaries.
