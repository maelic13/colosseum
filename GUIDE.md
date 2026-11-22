# Colosseum — desktop application development guide

The short operational view: where the work stands and what to do next.
Rationale, specifications and exit criteria live in [`PLAN.md`](PLAN.md)
(§G9 holds the steps); the evidence of completed phases will live under
`docs/architecture/`.

**This file and `PLAN.md` are the maintainer-facing pair.** `README.md` and
`CHANGELOG.md` are user-facing and carry no phase numbers or internal
argumentation.

## Current checkpoint

| | |
|---|---|
| Repository | `maelic13/colosseum` since the swap of 2026-11-13 (formerly `maelic13/colosseum-gui`, seeded from the CLI repository at its step 11.2); the CLI is `maelic13/colosseum-cli` (ADR-0001, PLAN §G8). `master` is the trunk, protected; work reaches it by squash pull requests from short-lived branches (*Repository settings* below). No 2.0 code yet |
| Predecessor | Colosseum GUI 1.1.0 (egui), released 2026-09-22 from the CLI repository. Since the split it lives here in `legacy/`, frozen but for fixes, and continues on plain `vX.Y.Z` tags until 2.0.0 replaces it: **1.2.1**, released here 2026-11-22 as `v1.2.1`, after 1.2.0 (2026-11-14, `63ca788`); installed 1.1.0 copies are not offered either (ADR-0001) |
| What is missing | The CLI repository's clean-up and Rarog's CLI download address, both outside this repository (PLAN §G8 *Order* 3), Phase 1 (design foundations), Phase 2 (repository and technology proof), Phase 3 (screen specifications), Phase 4 (implementation), Phase 5 (release) |
| Depends on Colosseum CLI | 11.4 protocol specification (before Phase 3), 13.2 synthetic streams (before 2.3), 13.3–13.4 (before 4.5; the lifecycle decision with 4.2), 13.5–13.7 (before 4.6), 13.8 release candidate (4.8) — PLAN §G9 joint milestones |
| Next step | **1.1.1** — the engine-library decisions; Delete All, names and versions and rating lists decided in inventory §12; the rating anchor (Q1) waits for the maintainer — `R2` (Claude Opus 5 — High); then 1.1.2–1.1.4 and 1.2, each `R2`. **2.1** — skeleton — `I2` (Claude Opus 5 — High) may start at the same time |
| Confirmed decisions | PLAN §G2 (2026-09-27): design first; a client of the CLI over its protocol; TypeScript + React with shadcn/ui on Radix and Tailwind; the shell a measured choice at 2.3 between Electron and Tauri 2 with Electron the default expectation; placement off, several tournaments at once; 64 participants the design point; lifecycle and close behaviour decided at 4.2 with *stop now on close* the starting preference; amendments and changed engines are CLI mechanisms; run file per tournament, rebuildable index, positions as FEN, no chess logic here; the 1.x library imported, 1.x tournaments not migrated; both themes equal. Repository tools in platform-neutral Python, developed on Windows for now (PLAN §G4 *Tooling*, 2026-11-14) |

## Current model mapping

PLAN §G9 records each step's stable capability class; this table maps the
classes to the current Claude model and thinking mode and is the only place
model names appear. Edit it when model generations change; do not rewrite
the roadmap. These are maintainer judgements, not measured rankings.

| Class | Capability | Claude |
|---|---|---|
| `R3` | Frontier causal/architecture research | Claude Fable 5.1 — High |
| `R2` | Bounded correctness-sensitive reasoning | Claude Opus 5 — High |
| `I2` | Difficult implementation | Claude Opus 5 — High |
| `I1` | Well-specified implementation | Claude Sonnet 5 — Medium |
| `M` | Mechanical/docs/provenance | Claude Sonnet 5 — Medium |
| `V` | Verification/measurement | Claude Sonnet 5 — High |

## Phases and steps

Each phase ends with a verifiable exit criterion. Nothing is "done" because
it compiles or renders; it is done when its criterion is demonstrated. When
reporting the next step, always report its class and the model the
mapping above gives it. Step identifiers are never reused.

Every phase, step and sub-step is one checkbox line here, ending in its
class (`IN PROGRESS / <class>` while under way, `DONE <date>` once ticked);
sub-steps sit indented under their step. PLAN §G9 holds each step's
specification and exit; `(Mn)` marks a joint milestone.

### Phase 0 — The split (ADR-0001)

- [x] **Phase 0** The split: 1.x moved into `legacy/` from the CLI repository at `8fa63cc` with its CI and release lane gating `master`; the swap (2026-11-13); the update check on plain `vX.Y.Z` tags; 1.2.0 released here from `63ca788`. The record is ADR-0001 — DONE 2026-11-14

### Phase 1 — Design foundations (documents only; sign-offs in pairs)

- [ ] **1.1** Inventory of 1.x from the CLI repository at `gui-v1.1.0` (`ba30829`): every screen, control, setting, stored item, workflow, keyboard path, error surface, background behaviour, defect and lesson, classified keep / change / drop / decide; guidelines split into product rules and egui workarounds ([`docs/design/inventory.md`](docs/design/inventory.md)). Draft 2026-09-29: 163 items (keep 68, change 74, drop 6, decide 15), 21 product rules, 17 egui workarounds, 18 defects, 12 lessons, 19 questions (14 in 1.1.1–1.1.4, 5 for CLI 11.3.1–11.3.5); reviewed with 1.2 — **IN PROGRESS / R2**
    - [ ] **1.1.1** Decide the engine-library questions: Delete All, clone identity, library rating provenance and history (ENG-10, ENG-19, ENG-23); Delete All dropped; engine, version and profile decided; rating lists decided with the anchor open (Q1), inventory §12, 2026-09-29 — **IN PROGRESS / R2**
    - [ ] **1.1.2** Decide the tournament-setup questions: multi-seed gauntlet, an extra PGN file, what presets carry, the default time allowed past the clock (TRN-06, TRN-21, TRN-22, PLY-08) — **R2**
    - [ ] **1.1.3** Decide the results and live-view questions: ratings reverted on delete, evaluation point of view (ARN-05, LIV-09) — **R2**
    - [ ] **1.1.4** Decide the application questions: update checks, portable mode, writeback conflicts, second instance, long-run behaviour (APP-06, APP-08, APP-15, APP-22, APP-23) — **R2**
- [ ] **1.2** Requirements: users, workflows and frequency, scale targets confirmed (64 participants the design point), performance budgets as numbers, keyboard and accessibility, the library import, the open decisions answered as requirements (several tournaments, close expectations, amendments, changed engines) and the answers of 1.1.1–1.1.4; `docs/design/requirements.md` signed off with 1.1; handed to CLI 11.3 (M2) — **R2**
- [ ] **1.3** Layout concepts: two or three as static HTML wireframes in both themes, walked through the workflows; the maintainer chooses; `docs/design/concepts.md` — **R2**
- [ ] **1.4** EXIT — Design system: tokens for both themes and the component catalogue with states, sizes and keyboard behaviour; `docs/design/design-system.md` signed off with 1.3 — **R2**

### Phase 2 — Repository and technology proof (beside Phase 1)

- [ ] **2.1** Skeleton: the §G4 front end behind a shell-agnostic interface in the Electron shell; formatter, linter, type check, unit / component / end-to-end runners, CI on the three required platforms, a packaged empty application per required target, `docs/dependencies.md`, user-facing `README.md`, the verification baseline written into `AGENTS.md` — **I2**
- [ ] **2.2** Component gallery: the 1.4 catalogue on the real stack in both themes with screenshot tests; gaps fed back into 1.4 before Phase 3 — **I1**
- [ ] **2.3** EXIT — Performance proof against the 1.2 budgets at scale on the CLI's synthetic streams (CLI 13.2), the same front end in Electron and in Tauri 2, on macOS arm64, Windows x64 and Linux x64; the shell chosen by ADR (Electron unless Tauri meets every budget and its smaller bundle is judged worth it), the other build removed; the §G4 fallbacks measured the same way only if the front end itself fails (M4) — **V**

### Phase 3 — Screen specifications (after 1.4, 2.2 and CLI 11.4)

- [ ] **3.1** Shell, settings and engine library; guards DEF-02, 04, 05, 06, 08 — **R2**
- [ ] **3.2** Tournament creation, presets, history and results; guards DEF-07, 13, 15, 17, 18 — **R2**
- [ ] **3.3** EXIT — Live view, lifecycle (stop, stop now, close options, resume) and amendments; guards DEF-01, 10, 11, 12, 16; `docs/design/screens.md` signed off; every keep and change item placed or dropped; every defect guarded — **R2**

### Phase 4 — Implementation (a screen only after its specification is signed off)

- [ ] **4.1** Design system in code, gallery finalised, screenshot regression — **I1**
- [ ] **4.2** Shell and CLI process manager: bundled and pinned CLI, handshake and version check, crash handling, Windows Job Object; the lifecycle decision (close, detach, re-attach) analysed and taken with CLI 13.4, recorded by ADR; guards DEF-01, 02, 09 — **R2, then I2**
- [ ] **4.3** Engine library with the 1.x import; guards DEF-02, 03, 04, 05, 06, 08 — **I1**
- [ ] **4.4** Tournament creation and presets: run file writer, dry-run validation, the largest tournament within budget; guards DEF-07, 15, 17 — **I1**
- [ ] **4.5** Live tournament view: facts and state coalesced to one render per frame, gap and snapshot resync, several tournaments at once, following a game; guards DEF-12, 15 (M5) — **I2**
- [ ] **4.6** Results, history and amendments: rebuildable index, standings, crosstable, games browser on paged queries, export, stop and resume, add / remove / length, changed engines, rating writeback; guards DEF-10, 11, 13, 16, 18 (M6) — **I2**
- [ ] **4.7** Packaging, signing, updater and release lane for the required targets and the cheap wanted ones; the CLI bundled and pinned — **I2**
- [ ] **4.8** EXIT — Every keep and change item present or dropped; no inventory defect reproducible; budgets met on a real tournament on the three required platforms (native Rarog and Basilisk on macOS and Linux); usability walkthrough; against the CLI 13.8 release candidate (M7) — **V**

### Phase 5 — Release

- [ ] **5.1** EXIT — First 2.x release: 2.0.0, `CHANGELOG.md`, the pin set to the released CLI, `legacy/` removed, published right after the CLI release and offered to 1.2.0 and later (M9) — **V**

## What to do now

**1.2.1 is out** (2026-11-22; 1.2.0 on 2026-11-14). Outside this
repository the CLI repository's clean-up (PLAN §G8 *Order* 3) and Rarog's
CLI download address remain, the maintainer's work there. Here: **decide
1.1.1–1.1.4, then write 1.2; 2.1 beside them.** The inventory draft (1.1)
is written; its fourteen questions for this repository are
steps 1.1.1–1.1.4, each answered, dated, in inventory §12. 1.2 carries the
answers into the requirements and is reviewed with 1.1; when it is signed
off it goes to the CLI repository, whose 11.3 and 11.4 produce the protocol
specification Phase 3 needs. 2.1 stands the repository up on the
provisional stack at any time. 2.3 waits for CLI 13.2. Nothing in Phase 4
starts before its Phase 3 specification is signed off.

```
git diff --check
```

## Working rhythm

```text
Pick the next open step  ->  do it + tests  ->  demonstrate its exit
criterion  ->  tick it here  ->  update PLAN.md when useful  ->  commit before
the next step.
```

Use a focused imperative commit subject, stage only the step's files and
never add co-author or assistant-attribution trailers; `AGENTS.md` is the
binding repository workflow.

## Recurring procedures

### Bumping the pinned CLI

- A numbered step: update the pin, the bundled binary checksums and the
  contract test's schema and fixtures together; run the end-to-end suite on
  the new synthetic streams; note the protocol version in `CHANGELOG.md`.

### Pull requests

- One larger piece of work per short-lived branch, its step commits on it;
  merge `master` into the branch first if `master` moved.
- The title and description become the squash commit (`AGENTS.md`, *Pull
  requests*). CI and Release run on it; squash-merge once both are green
  (the merge box is already filled from the title and description; check
  it, do not retype it, and remove the ` (#N)` GitHub appends to the title). GitHub deletes the branch; delete the local one.
- If `master` moved, press *Update branch* on the pull request (a merge of
  `master` into the branch) or merge it locally; never rebase a pushed
  branch.

### Cutting a 1.x release (maintainer, until 2.0.0)

- On a release branch, in one commit: date `legacy/CHANGELOG.md`'s
  section, add its link at the bottom, and set the version in
  `legacy/crates/colosseum-gui/Cargo.toml` (and `legacy/Cargo.lock`); and
  record the release in every document that names the current 1.x
  release: this guide (*Predecessor*, *What to do now*), `PLAN.md` (§G8
  *Tags and versions*) and the checksum examples in `README.md`. The record
  names the tag and the date, not a commit: the squash commit is known only
  once the pull request merges. Its pull request, titled `Release X.Y.Z`,
  runs the release check and the full candidate; nothing is left to record
  after the tag.
- On the merged `master`:

  ```
  git fetch origin
  git checkout origin/master --detach
  cd legacy
  cargo xtask release-check vX.Y.Z
  git tag vX.Y.Z
  git push origin vX.Y.Z
  ```

- Watch the release workflow (`gh run watch`); it publishes only once every
  build has passed, as `Colosseum X.Y.Z` (no `v` in the name; PLAN §G8,
  *Tags and versions*). Check the release page and the update offer from the
  previous 1.x.
- A failed run is repaired by deleting the tag
  (`git push origin :refs/tags/vX.Y.Z; git tag -d vX.Y.Z`), fixing `master`
  by pull request and tagging again.

### Repository settings (maintainer)

The settings that make the flow above the only one possible. Check them
after any change to the workflows' job names.

- **Settings → General → Pull Requests:** *Allow squash merging* only
  (merge commits and rebase merging off), its default message *Pull request
  title and description*; *Always suggest updating pull request branches*
  on; *Automatically delete head branches* on; *Allow auto-merge* on (a
  pull request merges itself once its required checks pass, with the
  squash title set when auto-merge is turned on, without GitHub's ` (#N)`).
- **Settings → General → Features:** Wiki and Projects off.
- **Settings → Rules → Rulesets → New branch ruleset** `master`:
  enforcement *Active*, no bypass list, target the default branch; *Restrict
  deletions*, *Require linear history*, *Require a pull request before
  merging* (0 approvals, allowed merge method *Squash*), *Require status
  checks to pass* (branches up to date; `CI complete` and `Verify the
  complete matrix`), *Block force pushes*. 2.1 adds its own completion
  check to the list.
- **Settings → Rules → Rulesets → New tag ruleset** `release tags`:
  enforcement *Active*, target `v*`; *Restrict updates*, so a release tag
  is never moved (deleting one to repair a failed run stays possible).
- **Settings → Actions → General:** workflow permissions *Read repository
  contents*, and Actions may not create or approve pull requests (the
  release job asks for write access itself).
- **About** (the repository's front page): description *Run and watch
  tournaments between UCI chess engines*.

### Cutting a release (maintainer)

- The CLI release this build pins is published first.
- Changelog dated; candidate dispatched and its packages started on the
  three required platforms; tag; watch the release workflow; check the
  release page and the updater from the previous version.

## Decision rules

| Situation | Action |
|---|---|
| A screen is wanted before its specification is signed off | Not allowed: design first (PLAN §G2) |
| A screen needs a component the catalogue lacks | Extend the catalogue and the gallery in both themes first, then use it |
| Something takes longer than a frame on the UI thread | Move it to the CLI or off the UI thread; virtualise the list; coalesce the updates |
| The CLI does not provide something a screen needs | A CLI extension through the protocol, requested via the CLI repository's gap analysis — never game-playing, rating or chess logic here |
| A protocol message is unknown to this build | Ignore it if the handshake's major version matches; refuse to start the CLI if it does not |
| A `gap` fact arrives | Request a snapshot and re-apply facts above it; never guess |
| Tempted to keep a derived statistic | Ask the CLI's query again; the run directory is the record |
| A dependency would save work | Accept only if mature, widely used, maintained, GPL-3.0-compatible and not overlapping one already taken; one line of reason in `docs/dependencies.md` |
| A GUI behaviour would differ from the CLI's for the same mechanism | The CLI's is the specification |
| Front-end code wants a shell API (Electron or Tauri) directly | Not allowed: go through the shell interface, so the 2.3 choice stays a build-level decision |
| Tempted to coordinate CPU placement across tournaments | Placement is a CLI feature; tournaments here run with it off |
