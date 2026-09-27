# <GUI> — development guide

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
| Repository | Seeded from the Colosseum repository at its step 11.2 (`docs/gui-v2/`). No code yet |
| Predecessor | Colosseum GUI 1.1.0 (egui), released 2026-09-22, in maintenance in the Colosseum repository until this application's first release |
| What is missing | Everything: Phase 1 (design foundations), Phase 2 (repository and technology proof), Phase 3 (screen specifications), Phase 4 (implementation), Phase 5 (release) |
| Depends on Colosseum | 11.4 protocol specification (before Phase 3), 13.2 synthetic streams (before 2.3), 13.3–13.4 (before 4.5; the lifecycle decision with 4.2), 13.5–13.7 (before 4.6), 13.8 release candidate (4.8) — PLAN §G9 joint milestones |
| Next step | **1.1** — inventory of 1.x — `R2` (Claude Opus 5 — High). **2.1** — skeleton — `I2` (Claude Opus 5 — High) may start at the same time |
| Confirmed decisions | PLAN §G2 (2026-09-27): design first; a client of the CLI over its protocol; provisional TypeScript + React on Tauri 2 until measured at 2.3, Electron the first fallback; placement off, several tournaments at once; 64 participants the design point; lifecycle and close behaviour decided at 4.2 with *stop now on close* the starting preference; amendments and changed engines are CLI mechanisms; run file per tournament, rebuildable index, positions as FEN, no chess logic here; the 1.x library imported, 1.x tournaments not migrated; both themes equal |

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
mapping above gives it. Step identifiers
are never reused.

### Phase 1 — Design foundations (documents only; sign-offs in pairs)

- ☐ **1.1** — `R2` — Inventory of 1.x from the Colosseum
  code at `gui-v1.1.0`, its changelog, guidelines and release history: every
  screen, control, setting, stored item, workflow, keyboard path, error
  surface, background behaviour, defect and lesson, each classified keep /
  change / drop / maintainer decides; guidelines split into product rules
  and egui workarounds; `docs/design/inventory.md` — PLAN §G9
- ☐ **1.2** — `R2` — Requirements: users, workflows and
  frequency, scale targets confirmed (64 participants the design point),
  performance budgets as numbers, keyboard and accessibility, the library
  import, the open decisions answered as requirements (several tournaments,
  close expectations, amendments, changed engines);
  `docs/design/requirements.md` signed off with 1.1; handed to Colosseum
  11.3 — PLAN §G9 (M2)
- ☐ **1.3** — `R2` — Two or three layout concepts as static
  HTML wireframes in both themes, walked through the workflows; the
  maintainer chooses; `docs/design/concepts.md` — PLAN §G9
- ☐ **1.4 — EXIT** — `R2` — Design system: tokens for both
  themes and the component catalogue with states, sizes and keyboard
  behaviour; `docs/design/design-system.md` signed off with 1.3 — PLAN §G9

### Phase 2 — Repository and technology proof (beside Phase 1)

- ☐ **2.1** — `I2` — Skeleton on the provisional stack:
  formatter, linter, type check, unit / component / end-to-end runners, CI
  on the three required platforms, a packaged empty application per
  required target, `docs/dependencies.md`, user-facing `README.md`, the
  verification baseline written into `AGENTS.md` — PLAN §G9
- ☐ **2.2** — `I1` — Component gallery: the 1.4 catalogue on
  the real stack in both themes with screenshot tests; gaps fed back into
  1.4 before Phase 3 — PLAN §G9
- ☐ **2.3 — EXIT** — `V` — Performance proof against the 1.2
  budgets at scale on the CLI's synthetic streams (Colosseum 13.2), on macOS
  arm64, Windows x64 and Linux x64; technology confirmed by ADR or the
  fallbacks measured the same way — PLAN §G9 (M4)

### Phase 3 — Screen specifications (after 1.4, 2.2 and Colosseum 11.4)

- ☐ **3.1** — `R2` — Shell, settings and engine library —
  PLAN §G9
- ☐ **3.2** — `R2` — Tournament creation, presets, history and
  results — PLAN §G9
- ☐ **3.3 — EXIT** — `R2` — Live view, lifecycle (stop, stop
  now, close options, resume) and amendments; `docs/design/screens.md`
  signed off; every keep item placed or dropped — PLAN §G9

### Phase 4 — Implementation (a screen only after its specification is signed off)

- ☐ **4.1** — `I1` — Design system in code, gallery
  finalised, screenshot regression — PLAN §G9
- ☐ **4.2** — `R2`, then `I2` — Shell and CLI process manager: bundled
  and pinned CLI, handshake and version check, crash handling, Windows Job
  Object; the lifecycle decision (close, detach, re-attach) analysed and
  taken with Colosseum 13.4, recorded by ADR — PLAN §G9
- ☐ **4.3** — `I1` — Engine library with the 1.x import —
  PLAN §G9
- ☐ **4.4** — `I1` — Tournament creation and presets: run
  file writer, dry-run validation, the largest tournament within budget —
  PLAN §G9
- ☐ **4.5** — `I2` — Live tournament view: facts and state
  coalesced to one render per frame, gap and snapshot resync, several
  tournaments at once, following a game — PLAN §G9 (M5)
- ☐ **4.6** — `I2` — Results, history and amendments:
  rebuildable index, standings, crosstable, games browser on paged queries,
  export, stop and resume, add / remove / length, changed engines, rating
  writeback — PLAN §G9 (M6)
- ☐ **4.7** — `I2` — Packaging, signing, updater and release
  lane for the required targets and the cheap wanted ones; the CLI bundled
  and pinned — PLAN §G9
- ☐ **4.8 — EXIT** — `V` — Every keep item present or
  dropped; budgets met on a real tournament on the three required platforms
  (native Rarog and Basilisk on macOS and Linux); usability walkthrough;
  against the Colosseum 13.8 release candidate — PLAN §G9 (M7)

### Phase 5 — Release

- ☐ **5.1 — EXIT** — `V` — First release: version chosen by
  the maintainer, `CHANGELOG.md`, the pin set to the released CLI, published
  right after the CLI release — PLAN §G9 (M8)

## What to do now

**Start 1.1 and 2.1 together.** 1.1 reads the Colosseum checkout beside this
one at `gui-v1.1.0` and writes the inventory; 2.1 stands the repository up
on the provisional stack. 1.2 follows 1.1 and is reviewed with it; when it is
signed off it goes to the Colosseum repository, whose 11.3 and 11.4 produce
the protocol specification Phase 3 needs. 2.3 waits for Colosseum 13.2.
Nothing in Phase 4 starts before its Phase 3 specification is signed off.

```
git diff --check
```

## Working rhythm

```text
Pick the next ☐ step  ->  do it + tests  ->  demonstrate its exit
criterion  ->  mark it ☑ here  ->  update PLAN.md when useful  ->  commit before
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
| The CLI does not provide something a screen needs | A CLI extension through the protocol, requested via Colosseum's gap analysis — never game-playing, rating or chess logic here |
| A protocol message is unknown to this build | Ignore it if the handshake's major version matches; refuse to start the CLI if it does not |
| A `gap` fact arrives | Request a snapshot and re-apply facts above it; never guess |
| Tempted to keep a derived statistic | Ask the CLI's query again; the run directory is the record |
| A dependency would save work | Accept only if mature, widely used, maintained, GPL-3.0-compatible and not overlapping one already taken; one line of reason in `docs/dependencies.md` |
| A GUI behaviour would differ from the CLI's for the same mechanism | The CLI's is the specification |
| Tempted to coordinate CPU placement across tournaments | Placement is a CLI feature; tournaments here run with it off |
