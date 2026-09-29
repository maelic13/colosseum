# ADR-0012: Names and repositories after the split

- **Status:** Accepted. Revised 2026-09-29, before any of it was acted on:
  the swap moved from after the desktop application's first release to
  before both releases, when the maintainer chose to merge both `dev`
  branches only after the rename (see *Branches and order*)
- **Date:** 2026-09-28
- **Supersedes:** [ADR-0009](0009-retain-colosseum-for-1-0.md)'s release-lane
  row once the repositories are swapped; ADR-0009's product and executable
  names are retained unchanged
- **Relates to:** PLAN §Phase 11 (step 11.1, milestone M0) and §Phase 14;
  [ADR-0011](0011-desktop-application-as-separate-cli-client.md);
  [naming research](../naming-decision.md)

## Context

[ADR-0011](0011-desktop-application-as-separate-cli-client.md) splits the
product into two repositories: the desktop application, re-implemented as a
client of the CLI, and this repository, which becomes the CLI alone. Step
11.1 names both products, both executables and both repositories.

The plan went in recommending that the CLI keep `colosseum-cli` because a
rename would land on the projects that adopted it. The maintainer removed that
premise on 2026-09-28: the only adopter is Rarog, the maintainer's own engine,
which is updated by hand. The maintainer also has no stronger name than
Colosseum and wants the desktop application to keep it.

The CLI executable could therefore be either `colosseum` or `colosseum-cli`.
Four facts decide it:

1. The desktop application bundles one pinned CLI release per platform
   (the GUI repository's PLAN, packaging). Windows and default macOS file
   systems are case-insensitive, so `Colosseum.exe` and `colosseum.exe` are
   one name there, and two processes called `colosseum` in a task list or a
   crash report would be indistinguishable.
2. **Colosseum** is the desktop application; a command called `colosseum` is
   expected to open it.
3. Engine developers know the pattern: Cute Chess ships `cutechess` and
   `cutechess-cli`, and fastchess users come from the same tooling.
4. `colosseum-cli` is what exists: no rename, no transition release, no
   documentation, archive or workflow churn.

Shortness is the only argument for `colosseum`, and run files already keep
invocations short.

The repositories are a separate question. The desktop application should own
`maelic13/colosseum`: it carries the brand, and the 1.x updater and About link
point there. But this repository holds the history, the published releases of
both products, and every CLI link in use, so it cannot simply give the name
up while the 1.x application is still maintained from it.

## Decision

### Names

| Surface | Name |
|---|---|
| Desktop product | **Colosseum** |
| Desktop executable | `Colosseum` / `colosseum` as each platform's packaging names it; never `colosseum-cli` |
| CLI product | **Colosseum CLI** |
| CLI executable and Cargo package | `colosseum-cli` (unchanged) |
| Shared packages in this repository | `colosseum-*` (unchanged) |

ADR-0009's descriptive qualifiers ("Colosseum chess-engine testing",
"Colosseum CLI for UCI chess engines") and its no-alias rule continue.

### Repositories: a temporary name, then a swap

| | Until the swap | After the swap |
|---|---|---|
| Desktop repository | `maelic13/colosseum-gui` (created at 11.2) | `maelic13/colosseum` |
| This repository | `maelic13/colosseum` | `maelic13/colosseum-cli`, with its full history and the CLI releases |

The maintainer performs the swap once the desktop application has been
accepted against the CLI release candidate (M7), before either `dev` branch is
merged and before either release (step 14.1):

1. Rename `maelic13/colosseum` to `maelic13/colosseum-cli`.
2. Immediately rename `maelic13/colosseum-gui` to `maelic13/colosseum`.

GitHub redirects a renamed repository's old address only until another
repository takes that name. Step 2 ends the redirect, so from then on every old
`maelic13/colosseum` address, including CLI release downloads and clone URLs,
resolves to the desktop repository. The same day:

- Rarog's CLI download addresses are updated;
- every clone of this repository runs
  `git remote set-url origin https://github.com/maelic13/colosseum-cli.git`,
  and every clone of the desktop repository points its `origin` at
  `maelic13/colosseum`; otherwise the next fetch reads the other repository;
- the links in both repositories' documents move to the new addresses, in
  the `dev` branches about to be merged.

The desktop build fetches its pinned CLI from `maelic13/colosseum-cli` from
the start, because the CLI release it pins is published after the swap.

### Branches and order

Both repositories develop on `dev`; `main` (`master` in the desktop
repository) holds what is released. In this repository, `main` keeps the 1.x
application and CLI 0.2.0 until the final merge. A 1.x or Phase 12 patch
still ships from `main` and is merged into `dev`. On `dev`, the egui
application leaves as soon as the desktop inventory (GUI 1.1) has read it
(step 11.2.1), so the CLI work of Phases 11 and 13 no longer carries it.

The end of the programme, in order:

1. The CLI release candidate is dispatched from `dev` (13.8), and the desktop
   application is accepted against it (GUI 4.8, M7).
2. The swap (above), then one pull request per repository merges `dev` into
   its default branch (14.1, M8). This repository's `main` loses the 1.x
   application in that merge.
3. The CLI release, the first plain-`v` tag, from `maelic13/colosseum-cli`.
   The release workflow publishes only commits reachable from `main`, which is
   why the merge precedes it. Then the desktop 2.0.0 release with its bridge
   release, pinned to it (14.2, GUI 5.1, M9).
4. The maintainer deletes the 1.x releases and tags from
   `maelic13/colosseum-cli` (14.2).

Between the swap and the desktop release, 1.x's update check reads a desktop
repository with no release yet and reports "up to date". It is short and
harmless.

### Tags

| Repository | Until the swap | After the swap |
|---|---|---|
| Desktop | `v<semver>` from its first release, plus one bridge release (below) | `v<semver>` |
| CLI | `cli-v<semver>` | `v<semver>` from the first release after the swap (14.2); the published `cli-v0.x` releases stay |

- **The bridge release.** GUI 1.1.0's update check reads
  `maelic13/colosseum`'s releases but accepts only `gui-v` tags, and plain `v`
  tags only up to 1.0.2 (`crates/colosseum-gui/src/update.rs`). After the swap
  it would ignore `v2.0.0` and report "up to date" forever, and a 1.x patch
  cannot reach users who never install it. So the desktop repository
  publishes its first release twice: `v<first>` and a bridge release
  `gui-v<first>` on the same commit with the same assets, neither draft nor
  prerelease, and not marked Latest. 1.1.0 then offers the update and opens
  the bridge release's page. One bridge is enough: the new application's own
  updater reads `v` tags. The first version is 2.0.0, following the GUI
  plan's own rule for a continuing brand. The desktop repository starts with
  no tags, so nothing there can clash.
- **The 1.x releases and tags are deleted.** After the swap no updater reads
  this repository's release list: 1.x and the new application both read
  `maelic13/colosseum`. Once the desktop 2.0.0 release is out (14.2), the
  maintainer deletes every desktop release from `maelic13/colosseum-cli`
  together with its tag (`v1.0.0-rc.1` through `v1.0.2`, `gui-v1.1.0` and any
  later `gui-v1.1.x`). That leaves the repository with CLI releases only. No
  `v` tag clashes in the meantime: the CLI is at 0.x. The 1.x installers
  stop being downloadable, and the maintainer accepts that. The 1.x source
  stays in this repository's history. Anything that must still reach it
  names the commit, not the tag: `gui-v1.1.0` is
  `ba308297f8866dc7aa8fd3172730d941e05b3d68`.
- **No 1.x patch after the swap.** A `gui-v1.1.x` release published from
  `maelic13/colosseum-cli` would be invisible to 1.x updaters. The 1.x answer
  to a defect found after the swap is the new application.
- **Latest.** After the swap the CLI release lane claims the repository's
  "Latest" (14.1), which until then belongs to the stable GUI release.

### What keeping the name costs the desktop application

The new application must not share the 1.x application's data directories.
They are the default for an application named Colosseum (`%APPDATA%\Colosseum`
on Windows, `~/Library/Application Support/Colosseum` on macOS; on Linux,
`~/.config/colosseum` and `~/.local/share/colosseum`). The two may be installed
side by side during the transition, and 1.x tournaments are not migrated
(ADR-0011). The new application therefore uses its own directory. It reads the
1.x directory only to import the engine library. Whether its installers
replace an installed 1.x or coexist with it is decided at GUI step 4.7 against
the 1.x installer identities (`msi`, `deb`, `rpm`, `dmg`, Arch). This
constraint is carried into the GUI repository's `PLAN.md` at step 11.2.

## Collision revalidation, 2026-09-28

| Surface | Evidence | Result |
|---|---|---|
| Same chess domain | The Coliseum chess GUI moved from itch.io to [PhelRin/Coliseum](https://github.com/PhelRin/Coliseum) and remains active (v3.4.0, 2026-06-18). It advertises engine tournaments and testing as an Arena replacement. | Unchanged risk, now against the desktop product only. The CLI's executable spelling is distinct. |
| Existing CLI | The wireless platform's [Colosseum CLI documentation](https://colosseumwireless.readthedocs.io/en/latest/radio_api_traffic/colosseum_cli.html) still documents `colosseumcli`; [colosseum-wiot/colosseumcli-public](https://github.com/colosseum-wiot/colosseumcli-public) is the only repository matching `colosseumcli`. | Unchanged. The executable spelling differs. |
| GitHub repository names | No repository is named exactly `colosseum-cli` (two near matches, both unrelated API clients). [tps01/colosseum-gui](https://github.com/tps01/colosseum-gui) is an unrelated GUI-testing plugin. `maelic13/colosseum-cli` and `maelic13/colosseum-gui` are free. | Both chosen names are available in the maintainer's namespace. |
| crates.io | `colosseum-cli` and `colosseum-gui` are unregistered; `colosseum` and `coliseum` remain taken. The CLI stays `publish = false`. | Unchanged. |
| Preliminary trademark screen | TMview is a browser-only application and was not re-run in this step. The 2026-08-03 counts (177 COLOSSEUM, 128 COLISEUM) stand. | Not legal clearance. |

The maintainer does not consider the same-domain name or the trademark
position a concern. The wireless `colosseumcli` belongs to Northeastern
University's Colosseum wireless-network emulator, a research testbed in an
unrelated domain, and overlaps only in web searches.

## Consequences

- No product, executable, package, path or run-format identifier changes.
  Internal format labels (`colosseum-rng-v1`, `colosseum-epd-fen-suite-v1`,
  `./colosseum-runs/`) are unaffected.
- The desktop application ends with the brand's address, and 1.x users are
  led to it through the bridge release.
- The CLI keeps its history, and its links move once, on a known day. Links
  published before the swap that name `maelic13/colosseum` for the CLI go to
  the desktop repository afterwards; that cost is accepted.
- Step 14.2 no longer renames an executable. It publishes the first
  plain-`v` CLI release from `maelic13/colosseum-cli`, and 14.1 before it
  moves the lane to `v` tags and to claiming Latest.
- If a naming conflict ever arises, both products are renamed together, as
  one complete migration (ADR-0009). No clearance is sought in advance.
- The 1.x installers are no longer published once 14.2 deletes their
  releases.

## Alternatives considered

### The CLI takes `colosseum`

Rejected for the four reasons in the context. The saving is a few keystrokes.

### A new name for both products, now that renaming is cheap

Not taken. The maintainer has no stronger candidate, and ADR-0009's triggers
(a legal conflict, recurring support or discovery failures, a materially
stronger replacement) have not occurred.

### The desktop repository keeps a permanent `colosseum-desktop` name

Rejected. It leaves the brand's address with the CLI, sends the 1.x updater
and About link to a repository with no new desktop release, and needs a 1.x
patch that 1.1.0 users would never see.

### The swap at the start of the programme

Rejected. The 1.x updater would read an empty desktop repository for the whole
programme, and `gui-v1.1.x` maintenance patches from this repository would be
invisible to it.

### The swap after the desktop application's first release

The original form of this ADR. Superseded on 2026-09-29: the maintainer merges
both `dev` branches only after the rename, and the CLI release must be on
`main`. The swap therefore comes before both releases. That also removes the
desktop build's change of CLI address on the day of the swap.

### The desktop repository keeps `gui-v` tags permanently

Rejected by the maintainer. A one-product repository needs no lane prefix,
and one bridge release covers 1.1.0.
