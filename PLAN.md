# Colosseum — desktop application plan

> **Seed.** Written in the CLI repository (`docs/gui-v2/PLAN.md`) and
> committed as this repository's root `PLAN.md` at CLI step 11.2, under the
> names CLI step 11.1 decided ([CLI ADR-0012](https://github.com/maelic13/colosseum-cli/blob/dev/docs/architecture/adr/0012-names-and-repositories-after-the-split.md)).
> A step number prefixed `CLI` is the CLI repository's. This file is
> maintainer-facing: it holds the binding requirements, the architecture,
> the specifications each step fills in, and the open work. `GUIDE.md` is
> the ordered tracker; `README.md` is user-facing.

## G1. What this is

A desktop application for running and watching chess-engine tournaments —
round-robin and gauntlet — over an engine library the user maintains, with
live boards, standings, ratings, history and export, on macOS, Linux and
Windows. It is the successor of Colosseum GUI 1.x, re-designed and
re-implemented from scratch, and it is a **client of the Colosseum CLI**: it
starts the CLI as a child process and talks to it through the CLI's
front-end protocol, the way a chess GUI talks to a UCI engine. Everything
that plays or scores a game is the CLI's; everything the user sees, and the
user's own data, is this application's
([CLI ADR-0011](https://github.com/maelic13/colosseum-cli/blob/dev/docs/architecture/adr/0011-desktop-application-as-separate-cli-client.md)).

Not in scope: the CLI's own workflows (matches, SPRT gates, SPSA tunes,
position suites, calibration) are not shown here; there is no served
dashboard; 1.x tournaments are not migrated. Playing on one machine and
watching or controlling from another is a later feature the architecture
keeps possible and does not schedule.

## G2. Requirements — binding

Set by the maintainer on 2026-09-27 and revised the same day after review.
Every step in §G9 is bound by them.

- **Design first.** Inventory, requirements, layout, design system and
  screen specifications are finished and signed off before a screen is
  implemented. The design is written without depending on the technology;
  the component catalogue is validated in a gallery on the real stack (2.2)
  before screens are specified against it.
- **Mature technology agents work in reliably.** Any language is
  acceptable. The front end is TypeScript + React; the shell is a measured
  choice at 2.3 between Electron and Tauri 2, Electron the default
  expectation (§G4).
- **Responsiveness and performance come first.** No freeze and no long
  loading, including for large tournaments: loading, creating and starting
  them. The numbers are set at 1.2 and are exit criteria at 2.3 and 4.8.
- **One set of standard components.** Every screen is composed from the
  design system's catalogue (1.4); a new need extends the catalogue first.
- **Dependencies where they pay.** Mature, widely used, maintained,
  GPL-3.0-compatible, saving real work; refused when overlapping or heavy for
  little. `docs/dependencies.md` carries one line of reason per entry.
- **Platforms.** Required: macOS arm64, Linux x64, Windows x64. Wanted where
  cheap: macOS x64, Linux arm64, Windows arm64.
- **Theme.** Follows the device by default; light and dark selectable; both
  designed and tested equally.
- **Scope.** Tournaments (round-robin, gauntlet), the engine library, live
  viewing, results, history and export. Several tournaments may run at once,
  as in 1.x. The 1.x engine library is imported; 1.x tournaments are not.
- **Placement.** CPU placement is a CLI feature for gates and tunes. This
  application runs tournaments with placement off and offers no placement
  control, which is what lets several tournaments run at once without
  coordination.
- **Scale.** Tournaments of **64 participants** are the design point, not
  the ceiling. The proposed targets are in §G5 and are confirmed at 1.2.
- **Lifecycle and stopping.** Two user actions — *finish the games in
  progress and stop*, and *stop now* — both leaving a tournament that
  resumes. What closing the application does, whether a tournament can keep
  running without it, and whether it can be re-attached are analysed and
  chosen at implementation (4.2, together with CLI 13.4). The
  maintainer's starting preference: closing the application stops now;
  keeping a tournament running without it is worth evaluating.
- **Changed engines and amendments.** An engine executable that changed
  since a tournament started; removing a participant from an active
  tournament; adding one to a running tournament; changing an active
  tournament's length. All are CLI mechanisms (decided at CLI 11.3,
  implemented in CLI 13.5 and 13.6) that this application presents.
- **Ratings (2026-09-29).** The CLI computes every tournament's ratings,
  error bars and performance ratings and publishes a snapshot after every
  scored game. This application owns the engine library and its ratings:
  it passes each engine's library rating as the starting rating, pins the
  engines a tournament must not move (*Chosen* mode pins the rest), and
  writes the CLI's figure into the library after each scored game. It never
  computes a rating.
- **Sign-offs.** Each design step ends in a maintainer sign-off; consecutive
  document steps are reviewed in pairs (1.1 with 1.2, 1.3 with 1.4).

## G3. Architecture

Three parts, one process boundary:

| Part | Runs | Owns |
|---|---|---|
| **Front end** | the WebView (provisionally React) | rendering, navigation, view state; nothing that can take longer than a frame |
| **Shell** | the host process (Electron's main process by default expectation, or Tauri's Rust side; decided at 2.3) | CLI child processes, windows, file dialogs, the updater, the GUI-owned store, the app data directory |
| **CLI** | one child process per running tournament, plus short-lived query invocations | scheduling, clocks, adjudication, faults, persistence and resume, PGN, ratings and statistics — everything in the run directory |

**Ownership.** This application owns the engine library (names, versions,
logos, saved options, library ratings and their writeback), tournament
presets, the list of the user's tournaments and where their run directories
are, and the application settings. It never re-implements a rating or a
statistic and holds no chess logic: positions arrive as FEN. Ratings split
as §G2 says: the CLI computes, this application keeps the library.

**Data flow of a tournament.**

1. *Create.* The user composes participants, format, clock and options from
   the library and presets. The application writes the tournament's **run
   file** into a new run directory under its app data (`tournaments/<id>/`)
   and validates it with the CLI's `--dry-run --json`, whose refusals are
   shown in place. Every tournament is therefore reproducible from a shell
   with the same file.
2. *Start.* The shell starts `colosseum-cli tournament run --run-file …
   --dir …` in protocol mode, reads the `hello`, checks the protocol version
   against the one this build was written for, and forwards facts and state
   to the front end, coalesced to at most one render per frame.
3. *Watch.* Live boards, clocks, search lines and the evaluation graph come
   from state events; standings and ratings from the snapshot fact after
   every scored game; rating writeback to the library happens on that fact.
4. *Stop, close, resume.* Stop and stop now are protocol commands. The close
   behaviour is chosen at 4.2. Resume starts the CLI on the same directory.
5. *History and results.* The tournament list is a **rebuildable index**
   over the run directories: losing the index loses nothing. Standings,
   crosstable and the games browser read the CLI's short-lived query
   invocations (`--json`, paged, with FEN per ply). Export copies what the
   CLI already wrote: `games.pgn`, `standings.csv`, `crosstable.csv`.
6. *Amend.* Remove or add a participant, change the length: protocol
   commands, presented as edits on the running tournament.

**The protocol** is the CLI's contract, not this repository's: principles in
the CLI repository's `PLAN.md` §S5.15, catalogue and schema in
`docs/cli/protocol.md` there. What this application relies on: a handshake
with versions; facts that are sequence-numbered and never silently dropped, with a `gap` fact and
a snapshot request for resync; state that coalesces; emission that never
blocks an engine; positions as FEN; synthetic streams for tests; transport
independence so a later remote transport needs no second protocol.

**The CLI is bundled and pinned.** Each release of this application ships
one CLI release per platform inside its bundle, and runs only that one; an
advanced setting may point at another CLI binary for development, with the
version check still enforced. On Windows the CLI child lives in a Job Object
that ends with the application, so no engine outlives a crash.

**Several tournaments at once.** Each running tournament is its own CLI
process with its own concurrency; placement is off, so they do not
coordinate. The live view shows all of them.

## G4. Technology

**Front end** (settled unless it fails the budgets): TypeScript, React,
Vite; **shadcn/ui on Radix primitives with Tailwind** as the component
system, because the components live in this repository — which is what one
reviewed catalogue in two themes needs — and Radix supplies keyboard and
accessibility behaviour; CSS variables for the tokens; TanStack Table with
TanStack Virtual for every long table; uPlot for the live evaluation graph,
built for streaming; chessground (lichess's board) or a small SVG board of
our own, since positions arrive as FEN and the board only renders; Inter and
JetBrains Mono embedded. The front end is written shell-agnostic: it talks
to the shell through one small typed interface (start, stop, events,
queries, dialogs, settings), so the same code runs in either shell below.
`docs/dependencies.md` is the record; this paragraph is the starting point.

**Shell — a measured choice at 2.3.** The same front end is built into
**Electron** and into **Tauri 2** and both are measured against the 1.2
budgets on the three required platforms. Electron is the default
expectation: this application links no Rust, so Tauri's Rust side would only
spawn a child process that Node spawns as well; one language and one
toolchain in the repository; one Chromium on every platform, which removes
the WebKitGTK rendering and performance differences on Linux; mature
packaging, signing and updater tooling; the cost is roughly 100 MB more on
disk and 100–300 MB of idle memory, which does not matter next to a
tournament running engines. Tauri is chosen only if it meets the budgets
everywhere and its smaller bundle is judged worth its Rust side and the
Linux testing overhead; its bundled Chromium runtime is alpha and is not
planned on.

**If the web front end itself fails the budgets** on a required platform in
both shells, the fallbacks are Qt/QML and then Flutter, each measured the
same way before anything is built. The process boundary means none of this
touches the CLI.

**Tooling** (maintainer decision 2026-11-14). The repository's own tools —
release, packaging and fixture helpers, diagnostics, measurement and
checks, anything run as `python tools/…` — are written in Python 3,
standard library first, so that moving development from Windows to Linux
or macOS is a change of machine and not a rewrite. Development and every
local check run on Windows for now and only Windows is exercised, but no
tool assumes it: paths through `pathlib`, processes without a shell,
no PowerShell or POSIX-shell scripts beyond a few lines of CI glue, no
reliance on Windows-only programs. What the front-end stack runs itself
(its package scripts, bundler and test runners) stays in that stack.
`legacy/` keeps its Rust `xtask` and PowerShell smoke script unchanged.

**Why not the Rust-native toolkits** is recorded in Colosseum's
`docs/architecture/gui-v2-research.md` §4 and §5.

## G5. Performance and scale

**Rules** (binding, from §G2): the UI thread only renders; anything that can
take longer than a frame runs in the CLI process or off the UI thread; every
long list is virtualised; live updates coalesce to one render per frame; the
window is usable before data arrives and fills in as it does; a query the
CLI answers is never re-computed here.

**Scale targets**, proposed here and confirmed at 1.2:

| Target | Proposed |
|---|---|
| Engines in the library | 200 |
| Participants per tournament | 64 (design point) |
| Games per tournament | 100,000 |
| Concurrent games in one tournament | 32 |
| Tournaments running at once | 4 |
| Tournaments in history | 500 |
| Live updates per running game | 10 per second |

**Budgets**, as numbers set at 1.2, each measured at 2.3 on synthetic
streams and at 4.8 on a real tournament, on all three required platforms:
cold start to a usable window; response to any input; frame time under full
live load (all concurrent games of all running tournaments); opening the
largest tournament in history; creating and starting the largest tournament;
memory at idle and under full load; time from *stop now* to stopped.

## G6. Design

The design documents live in `docs/design/` and are produced in Phase 1 and
Phase 3:

| Document | Step | Holds |
|---|---|---|
| `inventory.md` | 1.1 | everything 1.x does, classified keep / change / drop / maintainer decides; 1.x's guidelines split into product rules (kept) and egui workarounds (dropped) |
| `requirements.md` | 1.2 | users, workflows and frequency, scale targets, budgets, keyboard and accessibility, the open decisions answered |
| `concepts.md`, `wireframes/` | 1.3 | two or three information architectures as static HTML in both themes, walked through the workflows |
| `design-system.md` | 1.4 | tokens for both themes and the component catalogue with states, sizes and keyboard behaviour |
| `screens.md` | 3.1–3.3 | every screen and flow: purpose, data and its source (message or store), components, states, keyboard paths, behaviour at scale |

Rules that already hold: both themes are designed together, never one
derived from the other; fixed-width columns on live data; no widget changes
size on hover or selection; engine identity is "name version" wherever one
string names an engine; the Elo shown is what the library holds and a delta
is always against tournament start. The inventory carries the rest of 1.x's
product rules across.

## G7. Testing — binding

- **Unit tests** for everything pure in the front end and the shell.
- **Component tests** with screenshots of the gallery in both themes, kept as
  regression from 2.2 on.
- **End-to-end tests** against the CLI's synthetic streams (`protocol
  simulate` and the replay of a recorded run directory), so no engine is
  needed in CI and both repositories test the same messages.
- **Contract test**: the pinned CLI binary is run against the schema and
  conformance fixtures published with its release; a protocol bump is a
  numbered step that updates the pin and this test together.
- **Performance benchmarks**: the 2.3 harness stays in CI at a coarser
  threshold as a regression guard; the exact budgets are re-measured at
  every phase exit.
- **Acceptance on real tournaments** (4.8): the maintainer's engine library
  on Windows, native builds of Rarog and Basilisk on macOS and Linux.

## G8. Platforms, packaging and release

- Required targets build, test and package in CI from 2.1; wanted targets
  are added at 4.7 where the cost is a matrix entry.
- The bundle includes the pinned CLI release's binary for that platform,
  fetched by checksum from the CLI repository's release, and it is signed
  and notarised together with the application.
- Installers: msi or nsis, dmg, deb, rpm, AppImage, an Arch package where
  1.x had one; the updater checks this repository's releases only.
- **Apart from 1.x.** Colosseum 1.x keeps its data in the directories an
  application named Colosseum gets by default (`%APPDATA%\Colosseum`,
  `~/Library/Application Support/Colosseum`, `~/.config/colosseum` and
  `~/.local/share/colosseum`). This application uses its own directory,
  reads the 1.x one only to import the engine library, and never writes
  there; the two may be installed side by side. Whether its installers
  replace an installed 1.x or coexist with it is decided at 4.7 against the
  1.x installer identities.
- **Tags and versions.** Plain `vX.Y.Z` tags for both generations
  (ADR-0001). 1.x, in `legacy/` until 2.0.0, continues from **1.2.0**
  (now **1.2.1**, 2026-11-22), whose update check reads `v` tags;
  installed 1.1.0 copies, which accept only `gui-v` tags, are not offered
  it, and that is accepted. 2.0's first release is **2.0.0**, the
  Colosseum brand continuing, and every 1.x from 1.2.0 on is offered it. No bridge release. A release is named `Colosseum X.Y.Z`
  on its release page (`Colosseum 1.2.0`, `Colosseum 2.0.0`); only the tag
  carries the `v`.
- **Delivery** (as Rarog's). `master` is the trunk. Each larger piece of
  work lives on its own short-lived branch with its step commits and reaches
  `master` as one squash commit through a pull request; CI and the release
  candidate run on it, and the maintainer squash-merges it once both are
  green. A release is a pull request that dates the changelog, sets the
  version and records the release in the documents (`GUIDE.md`, *Cutting a
  1.x release*), then a `vX.Y.Z` tag pushed on the merged `master`; the
  release workflow validates, builds and publishes it. Pull-request titles and
  descriptions follow `AGENTS.md`.
- **Order** ([ADR-0001](docs/architecture/adr/0001-1x-maintained-here-and-the-swap-at-the-split.md),
  amending CLI ADR-0012 *Branches and order*):
  1. *The split* (Phase 0, done 2026-11-14; M8). 1.x moves into
     `legacy/` here, builds, tests and packages, and becomes 1.2.0 with an
     update check on `v` tags. The work reaches `master` by pull request.
  2. *The swap*, by the maintainer, done 2026-11-13: the CLI repository is
     `maelic13/colosseum-cli` (its `main` renamed `master`), this one
     `maelic13/colosseum`. Taking the name ended GitHub's redirect for the
     old address, so every clone points its `origin` at the new address,
     Rarog's CLI download address (`tools/colosseum/colosseum.pin.json`)
     moves, and every link to the CLI repository here names
     `maelic13/colosseum-cli` (moved 2026-11-14).
  3. *1.2.0 from here*, done 2026-11-14: the first release of this
     repository, from the protected `master` (`63ca788`). Then the
     maintainer deletes the desktop releases and tags
     from `maelic13/colosseum-cli` (`v1.0.0` to `v1.0.2`, `gui-v1.1.0`),
     whose `dev` is brought up to date and merged into its `master` without
     the GUI (the CLI repository's work).
  4. *The end of the programme*: the CLI release candidate (CLI 13.8) and
     this application accepted against it (4.8, M7); the CLI release that
     carries the protocol, from `maelic13/colosseum-cli`; then 2.0.0, pinned
     to it, in the change that removes `legacy/` and its release lane (5.1,
     CLI 14.2, M9). The build fetches the pinned CLI from
     `maelic13/colosseum-cli` from the start.
- A CLI patch never requires a release here; a protocol minor bump does.
- `CHANGELOG.md` is user-facing; the release notes are its section.

## G9. Implementation plan

### Capability classes for numbered steps

Each open `GUIDE.md` step carries a stable **capability class**; `GUIDE.md`'s
"Current model mapping" table is the single maintainer-edited source that
maps classes to the current Claude model and thinking mode. The classes are
the ones the CLI and Rarog repositories use. They are task-risk
judgements, not part of the product and never a measured ranking.

| Class | Required capability | Typical use here |
|---|---|---|
| `R3` | Frontier causal/architecture research | none scheduled |
| `R2` | Bounded correctness-sensitive reasoning | inventory classification, requirements, layout concepts, the design system, screen specifications, the lifecycle analysis |
| `I2` | Difficult implementation | the skeleton and CI on three platforms, the process manager, the live view, results and amendments, packaging and signing |
| `I1` | Well-specified implementation | the design system in code, the gallery, the engine library and creation screens built to signed-off specifications |
| `M` | Mechanical documentation/provenance | changelog rows, pins, records |
| `V` | Verification/measurement | the performance proof, acceptance, the release |

If an `I1`, `M` or `V` step exposes a material design choice not resolved by
this plan, stop that step and continue it as `R2`; a recorded class is never
silently downgraded. When reporting the next step, name its class and
recommend the mapped model and mode (`Claude: <model> — <mode>`) with a brief
reason; the recommendation never changes the active model by itself.

| Step | Class |
|---|---|
| 1.1, 1.1.1–1.1.4, 1.2–1.4 | `R2` |
| 2.1 | `I2` |
| 2.2 | `I1` |
| 2.3 | `V` |
| 3.1–3.3 | `R2` |
| 4.1, 4.3, 4.4 | `I1` |
| 4.2 | `R2` for the lifecycle analysis, then `I2` |
| 4.5, 4.6, 4.7 | `I2` |
| 4.8, 5.1 | `V` |

### Joint milestones with the CLI repository

| | Milestone | CLI repository | Here |
|---|---|---|---|
| M0 | Names decided | 11.1 | — |
| M1 | This repository exists | 11.2 hands over the seed | 1.1 and 2.1 start |
| M2 | Requirements signed off | 11.3 gap analysis starts | 1.2 |
| M3 | Protocol specified | 11.4 | 3.1–3.3 reference it |
| M4 | Synthetic streams available | 13.2 | 2.3 performance proof |
| M5 | Live events and control | 13.3, 13.4 | 4.5 on the real CLI; 4.2's lifecycle decision with 13.4 |
| M6 | Amendments, changed engines, queries | 13.5–13.7 | 4.6 |
| M7 | CLI release candidate | 13.8, from `dev` | 4.8 acceptance against it |
| M8 | The split and the swap (ADR-0001, moved from the end) | the swap; `dev` up to date and into `master`, without the GUI; desktop releases deleted after 1.2.0 | Phase 0, done 2026-11-14: 1.x in `legacy/`; takes `maelic13/colosseum`; 1.2.0 released here |
| M9 | Releases in quick succession | 14.2: the CLI release | 5.1, pinned to it; `legacy/` removed |

### Phase 1 — Design foundations (documents only)

- **1.1 Inventory of 1.x** (`R2`). Everything the 1.x application
  does, read from its code at the CLI repository's `gui-v1.1.0` (commit
  `ba30829`), its changelog, its
  guidelines and its release history — every screen, control, setting,
  stored item, workflow, keyboard path, error and warning surface,
  background behaviour (updater, rating writeback, resume, incident reports,
  logs, ECO names, engine check) and the defects and lessons recorded along
  the way. Each item classified *keep*, *change* (what is wrong), *drop*
  (why) or *maintainer decides*. `GUIDELINES.md` split into product rules,
  which carry over, and egui workarounds, which do not.
  **Exit:** `docs/design/inventory.md`, every item classified; reviewed with
  1.2.
- **1.1.1 Decide the engine-library questions** (`R2`). Inventory §12:
  ENG-10 Delete All; ENG-19 whether a clone keeps the same "name version";
  ENG-23 whether the library rating shows where it came from and keeps a
  history.
  **Exit:** each answer dated in inventory §12 and carried into
  `requirements.md` (1.2).
- **1.1.2 Decide the tournament-setup questions** (`R2`). TRN-06
  gauntlets with more than one seed; TRN-21 a PGN file outside the run
  directory; TRN-22 what a preset carries; and PLY-08, the default time
  allowed past the clock that this application passes as the CLI's
  `--margin-ms` (any value; 1.x used 2 s, the CLI defaults to 20 ms), and
  whether the user can change it.
  **Exit:** as 1.1.1.
- **1.1.3 Decide the results and live-view questions** (`R2`). ARN-05
  whether deleting a tournament reverts the ratings it wrote; LIV-09 the
  point of view of evaluations.
  **Exit:** as 1.1.1.
- **1.1.4 Decide the application questions** (`R2`). APP-06 automatic
  update checks; APP-08 portable mode; APP-15 rating writeback conflicts
  (two tournaments, a typed rating during a run); APP-22 a second instance;
  APP-23 keeping the machine awake, a notification when a tournament ends,
  progress on the taskbar or Dock.
  **Exit:** as 1.1.1.
- **1.2 Requirements** (`R2`). Who uses the application and for what;
  the workflows with their frequency (set up, start, watch, stop and resume,
  manage engines, review results, export, amend); the §G5 scale targets
  confirmed or changed; the **performance budgets as numbers**; keyboard and
  accessibility expectations; the 1.x library import; the open decisions
  answered as requirements — several tournaments at once, what the user
  expects on close (the options 4.2 will analyse), what amending a running
  tournament should look like, what the user expects when an engine changed
  — and the answers of 1.1.1–1.1.4.
  **Exit:** `docs/design/requirements.md` signed off with 1.1; handed to
  CLI 11.3. (M2)
- **1.3 Layout concepts** (`R2`). Two or three information
  architectures — at least the sidebar / list / detail model, a workspace
  with docked panels, and the strongest alternative the analysis suggests —
  as static, dependency-free HTML wireframes in both themes, each walked
  through the 1.2 workflows, evaluated on steps per workflow, what is
  visible while several tournaments run, and how the layout scales from a
  laptop to a large monitor.
  **Exit:** the maintainer chooses one; `docs/design/concepts.md` records
  why.
- **1.4 — EXIT: Design system** (`R2`). Tokens (colour for both themes,
  typography, spacing, radius, elevation, motion) and the component
  catalogue: buttons, fields, selects, toggles, tables with sort and
  virtualisation, lists, list / detail, tabs, sheets and dialogs, menus,
  toasts, empty / loading / error states, progress, the board, clocks, the
  evaluation graph, engine identity (logo, name version, rating), result and
  score chips — each with states, sizes and keyboard behaviour. The product
  rules kept by 1.1 are carried into it.
  **Exit:** `docs/design/design-system.md` signed off with 1.3.

### Phase 2 — Repository and technology proof (beside Phase 1 from M1)

- **2.1 Skeleton** (`I2`). The front end stack of §G4 behind the
  shell-agnostic interface, in the Electron shell; formatter, linter, type
  check, unit, component and end-to-end test runners; CI on the three
  required platforms; a packaged empty application for each required
  target; `docs/dependencies.md`; the user-facing `README.md`; the
  verification baseline written into `AGENTS.md`.
  **Exit:** CI green on the three platforms and the three packages start.
- **2.2 Component gallery** (`I1`). The 1.4 catalogue built on the real
  stack, in both themes, with screenshot tests; every gap between the
  catalogue and what the component system offers fed back into 1.4 as a
  dated amendment before Phase 3 begins.
  **Exit:** every catalogue component in the gallery in both themes, or the
  catalogue amended.
- **2.3 — EXIT: Performance proof** (`V`). Against the 1.2 budgets at
  the §G5 scale, using the CLI's synthetic streams (CLI 13.2): the
  largest tournament's history, the maximum concurrent live games of the
  maximum running tournaments at the CLI's update rate, the virtualised
  games list, the 64-participant standings and crosstable, cold start,
  memory — the same front end built into Electron and into Tauri 2, both
  measured on macOS arm64, Windows x64 and Linux x64 (WebKitGTK is Tauri's
  expected weak point).
  **Exit:** the shell chosen by ADR from the measurements, Electron unless
  Tauri meets every budget and its smaller bundle is judged worth it
  (CLI ADR-0011's status updated there); the other shell's build
  removed; or, if the front end fails in both shells, the failures recorded
  and the §G4 fallbacks measured the same way before anything is built. (M4)

### Phase 3 — Screen specifications (after 1.4, 2.2 and CLI 11.4)

Each screen: purpose, the data it shows and where it comes from (the
protocol message or the GUI-owned store, by name), the catalogue components
it uses, every state (empty, loading, live, stopped, error), keyboard paths,
and behaviour at the §G5 scale.

- **3.1 Shell, settings and engine library** (`R2`). Navigation of the
  chosen concept; settings (theme, paths, the CLI in use); the library list
  and inspector; add, inspect through the CLI, options, logos, ratings,
  check; the 1.x import. Guards inventory DEF-02, DEF-04, DEF-05, DEF-06,
  DEF-08.
- **3.2 Tournament creation, history and results** (`R2`). The creation
  flow and presets at 64 participants; validation through dry-run; the
  tournament list as an index; standings, crosstable, games browser,
  terminations, export. Guards DEF-07, DEF-13, DEF-15, DEF-17, DEF-18.
- **3.3 — EXIT: Live view, lifecycle and amendments** (`R2`). Several
  tournaments at once; following a game; stop and stop now; the close
  options as 4.2 will analyse them; resume; remove or add a participant,
  change the length; the changed-engine case. Guards DEF-01, DEF-10,
  DEF-11, DEF-12, DEF-16.
  **Exit:** `docs/design/screens.md` signed off; every *keep* and *change*
  item of 1.1 appears on a screen (a *change* item with its change) or is
  recorded as dropped by the maintainer; every inventory defect (§11) has
  its guard in a specification (DEF-14 is the CLI's, 11.3.5).

### Phase 4 — Implementation (a screen only after its specification is signed off)

- **4.1 Design system in code** (`I1`). Tokens, the catalogue
  components finalised from the gallery, screenshot tests as regression.
- **4.2 Shell and CLI process manager** (`R2`, then `I2`). Navigation, settings,
  the bundled and pinned CLI, the handshake and version check, crash
  handling and error surfaces, the Windows Job Object; the **lifecycle
  decision** analysed and taken with CLI 13.4 — what close does,
  whether a tournament keeps running without the application, whether it
  can be re-attached — recorded in an ADR here and in CLI S5.15. Guards
  DEF-01, DEF-02 (settings), DEF-09.
- **4.3 Engine library** (`I1`). Per 3.1, including the 1.x import.
  Guards DEF-02 (library), DEF-03, DEF-04, DEF-05, DEF-06, DEF-08.
- **4.4 Tournament creation and presets** (`I1`). Per 3.2: the run-file
  writer, dry-run validation, creation and start of the largest tournament
  within budget. Guards DEF-07, DEF-15, DEF-17.
- **4.5 Live tournament view** (`I2`). Facts and state consumed through
  the shell, coalesced to one render per frame; gap and snapshot resync;
  several tournaments at once; following a game. Guards DEF-12, DEF-15.
  (M5)
- **4.6 Results, history and amendments** (`I2`). The rebuildable index;
  standings, crosstable and the games browser on paged queries; export;
  stop and resume; amendments; the changed-engine handling; rating
  writeback. Guards DEF-10, DEF-11, DEF-13, DEF-16, DEF-18. (M6)
- **4.7 Packaging, signing, updater and release lane** (`I2`). Required
  targets and the cheap wanted ones; the CLI bundled and pinned; signing
  and notarisation of the whole bundle.
- **4.8 — EXIT: Acceptance** (`V`). Every *keep* and *change* item
  present, with its change, or dropped by the maintainer; no inventory
  defect reproducible, each covered by a test or a walkthrough check; the
  lessons (LES) checked where they apply; the 1.2 budgets met on a real tournament from
  the maintainer's library on the three required platforms; a usability
  walkthrough with the maintainer; all of it against the CLI 13.8
  release candidate. (M7)

### Phase 5 — Release

- **5.1 — EXIT: First release** (`V`). 2.0.0 (§G8); `CHANGELOG.md`; the
  pin set to the released CLI; `legacy/` and its release lane removed in the
  same change; the release published right after the CLI's (CLI 14.2), from
  `master` at `maelic13/colosseum`, and offered to 1.2.0 and later by
  their update check. (M9)

**Programme exit:** the desktop application released from this repository
over the protocol, with one game-playing implementation, in the CLI.

## G10. Risks

| Risk | Mitigation |
|---|---|
| A 1.x capability users rely on is lost | 1.1 inventory with every item classified; 3.3 and 4.8 check every *keep* item |
| The CLI cannot serve a requirement, discovered late | 1.2 is handed to CLI 11.3 before any screen is specified; *not feasible* items are resolved with the maintainer first |
| The web front end is not fast enough, notably WebKitGTK | Budgets as numbers at 1.2, measured at 2.3 on synthetic streams in both shells before anything is built; Electron's single Chromium is the default expectation; Qt/QML and Flutter only if the front end itself fails |
| The shell choice leaks into the front end | one small typed shell interface from 2.1 on; the front end never imports a shell API directly |
| The design system is fiction | 2.2 builds the whole catalogue on the real stack before Phase 3 specifies screens against it |
| Protocol drift between the repositories | the handshake version, the published schema and fixtures, the contract test against the pinned CLI in CI, one CLI bundled per release |
| A stalled front end stalls an engine | the CLI's bounded emission (S5.15); here, one render per frame and a snapshot resync after a `gap` |
| Chess logic creeps in | positions as FEN from the CLI; the games browser reads paged queries; nothing here generates moves |
| Design work never converges | sign-offs in pairs; 1.3 limited to two or three concepts; the gallery makes the catalogue concrete |
| The two releases drift apart | joint milestones; the CLI release candidate is what 4.8 accepts against; the CLI tags first and this repository follows the same day |

## G11. Reference

| Path | What |
|---|---|
| `docs/design/` | inventory, requirements, concepts and wireframes, design system, screens |
| `docs/architecture/adr/` | decisions of this repository (technology at 2.3, lifecycle at 4.2, …) |
| `docs/dependencies.md` | every dependency with its reason |
| `src/` | the front end and shell layout fixed at 2.1 |
| CLI repository `PLAN.md` §S5.15, §S8 | protocol principles; the programme and joint milestones |
| CLI repository `docs/cli/protocol.md` | the protocol contract; schema and fixtures published with each CLI release |
| CLI repository `docs/architecture/gui-v2-research.md` | the technology research behind §G4 |
| `legacy/` | the 1.x application, maintained until 2.0.0 and read for the inventory (ADR-0001); CLI repository commit `ba30829` is `gui-v1.1.0` |
