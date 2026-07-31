# CLI naming decision

This document is the research and application record for GUIDE step 0.6. It
fixes the public name left open by the release architecture without changing
its one-repository, two-product model.

## Decision

The released desktop product remains **Colosseum**. The new independent CLI
product is **UCI Rig**.

| Surface | Binding name |
|---|---|
| Public product/display name | `UCI Rig` |
| Command on every platform | `ucirig` (`ucirig.exe` on Windows) |
| Cargo package and crate | `ucirig` |
| Workspace path | `crates/ucirig` |
| GitHub Release title | `UCI Rig <semver>` |
| Release tag | `cli-v<semver>` |
| Artifact basename | `ucirig-<version>-<os>-<arch>` |
| Workflow and changelog lane | CLI (`release-cli.yml`, `CHANGELOG-CLI.md`) |

“UCI” always expands to **Universal Chess Interface** on first use in
user-facing introductory material. “Rig” describes a local, reproducible test
setup rather than a chess engine or testing farm. Spoken support uses “U-C-I
Rig”; source, commands and package names use the single lowercase token
`ucirig`.

The repository and shared implementation packages retain their existing
`colosseum-*` names. This is not a compatibility alias: `colosseum-cli`,
`colosseumcli`, `colosseum-lab` and `colosseum` are not accepted CLI command
names. The existing `colosseum` executable remains exclusively the desktop
GUI, so both products can be installed on one machine without a PATH collision.

ADR-0007 makes this decision binding. Phase 2 creates the package and command;
Phase 9 applies the public documentation and release names.

## Evidence snapshot

Research was performed on 2026-07-31. Availability is a point-in-time signal,
not a reservation, and the trademark screen is not legal clearance.

| Check | Colosseum / Coliseum evidence | UCI Rig evidence | Assessment |
|---|---|---|---|
| Same-domain search | A separate [Coliseum chess GUI](https://phelrin.itch.io/coliseum/devlog/1404384/coliseum-update-tournaments-drag-and-drop-and-human-vs-engine) targets engine matches and tournaments. The spelling is different but the name is effectively indistinguishable in speech. | Exact searches for `"UCI Rig" chess`, `"UCIRig"` and GitHub repositories named `ucirig` found no chess/software product using the name. | Strong reason not to launch another “Colosseum CLI”; good initial distinctiveness for UCI Rig. |
| Existing CLI search | The wireless-network [Colosseum CLI documentation](https://colosseumwireless.readthedocs.io/en/latest/radio_api_traffic/colosseum_cli.html) already uses “ColosseumCLI” and the `colosseumcli` command. | No exact competing CLI was found. | UCI Rig is materially easier to search and support. |
| crates.io | [`colosseum`](https://crates.io/crates/colosseum) and [`coliseum`](https://crates.io/crates/coliseum) are published. A [`colosseum-cli` search](https://crates.io/search?q=colosseum-cli) showed no package, but that unused registry spelling does not cure the product/search collision. | Searches for [`ucirig`](https://crates.io/search?q=ucirig) and [`uci-rig`](https://crates.io/search?q=uci-rig) showed no package. | Use package `ucirig`; re-check immediately before any registry publication. V1 binary releases do not depend on crates.io. |
| GitHub | Exact-name search found multiple `ColosseumCLI`/client repositories in addition to this repository. | The [GitHub repository search](https://github.com/search?q=ucirig+in%3Aname&type=repositories) returned no repository named `ucirig`. | The command has a clean source-search identity at the audit date. |
| Trademark screen | An exact-name [TMview search for COLOSSEUM](https://www.tmdn.org/tmview/#/tmview/results?page=1&pageSize=30&criteria=I&basicSearch=COLOSSEUM) returned 177 records; the corresponding [COLISEUM search](https://www.tmdn.org/tmview/#/tmview/results?page=1&pageSize=30&criteria=I&basicSearch=COLISEUM) returned 128. Results span many jurisdictions, statuses and classes and were not treated as a legal conclusion. | Exact-name TMview searches for [UCI RIG](https://www.tmdn.org/tmview/#/tmview/results?page=1&pageSize=30&criteria=I&basicSearch=UCI%20RIG) and [UCIRIG](https://www.tmdn.org/tmview/#/tmview/results?page=1&pageSize=30&criteria=I&basicSearch=UCIRIG) returned no rows. | Lower preliminary collision risk, not clearance. TMview itself states that it is not an official register and its information has no legal effect. |
| Meaning and speech | “Colosseum” is broad, crowded and easily confused with “Coliseum”; `colosseum-cli` is also cumbersome to dictate and competes with the GUI for the natural command. | “U-C-I Rig” names the supported engine protocol and a local test setup; `ucirig` is short and unambiguous in command examples. | UCI Rig better identifies the audience and independent tool. |

Only the planned Rust/GitHub binary distribution surfaces were checked. PyPI,
npm and operating-system package repositories are not planned V1 publication
channels and do not determine the command. Their availability must be checked
if a later decision adds one of them.

The [protocol author's official UCI description](https://www.shredderchess.com/index.php?catid=16%3Afeatures&id=39%3Auci-universal-chess-interface&lang=en-GB&option=com_content&view=article)
identifies Universal Chess Interface as an open, license-fee-free protocol
usable by any chess program. That makes the acronym descriptive of
compatibility, but it does not make the composite product name legally safe by
itself. Before commercial use, registration or substantial promotion, obtain a
jurisdiction- and class-specific professional clearance.

## Alternatives considered

### Retain “Colosseum CLI” and use `colosseum-cli`

Rejected. It avoids the existing GUI command but collides in search with an
established ColosseumCLI, retains the spoken collision with the separate
Coliseum chess GUI and inherits a crowded generic mark. An unused crates.io
spelling is not enough to make the product supportable.

### Retain Colosseum branding and use `colosseum-lab`

Rejected. The suffix improves the executable collision but does not fix the
spoken/search confusion in the same chess-engine GUI category. “Lab” is also
less explicit about the UCI contract.

### Rename the whole repository and desktop product to UCI Rig

Rejected for this plan. Colosseum GUI 1.0.2 is already released and owns
installer identities, application directories, saved data and user-facing
branding. Renaming it would create migration and compatibility work unrelated
to making the new CLI independent. The desktop name can be reconsidered in a
separate decision if its real users encounter confusion; this CLI decision does
not claim that the desktop name is collision-free.

### Use ChessForge or EloForge

Rejected during discovery. Both names already identify active game/chess
products, so they repeat the search problem under a new spelling.

## Revalidation rule

Before the first public UCI Rig release, Phase 9 repeats exact web, GitHub,
crates.io and TMview checks and records the date. If the package name has become
unavailable, or a same-domain product or material trademark conflict appears,
a new ADR decides the affected package or product identity before release. No
name change or compatibility alias is added silently.
