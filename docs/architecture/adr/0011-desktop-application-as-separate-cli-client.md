# ADR-0011: Make the desktop application a separate client of the CLI

- **Status:** Accepted (technology provisional until GUI step 2.3). Revised
  2026-09-27 after the plan review, before any step started: step references,
  the fallback order in decision 5, the reason in decision 2 and the placement,
  run-file and FEN sentences in decision 4. The decision itself is unchanged
- **Date:** 2026-09-27
- **Relates to:** PLAN §S4 and Phase 11; [GUI v2 research](../gui-v2-research.md);
  partially supersedes [ADR-0006](0006-one-repository-independent-product-releases.md)
  when step 14.1 completes

## Context

The desktop application (GUI 1.1.0) is built on egui and plays games through
its own scheduler and SQLite store, while the CLI plays through its own
drivers with placement, pentanomial statistics, fault policy and durable run
directories. The original Phase 11 would have moved the CLI drivers into a
library crate and rebuilt the GUI on it in Rust.

The maintainer has decided the desktop application is re-designed and
re-implemented from scratch. The requirements, recorded in PLAN §Phase 11:

- the UI is fully designed before it is implemented, and the design does not
  depend on the technology;
- the technology is mature and one coding agents work in reliably — egui's
  immediate-mode quirks (`docs/design/GUIDELINES.md` §7) cost too much;
- responsiveness and performance are the first product requirement: no
  freezing and no long loading, including large tournaments;
- screens are built from one set of standard, reusable components;
- dependencies are used where they are mature, widely used and save real work,
  and not otherwise;
- macOS arm64, Linux x64 and Windows x64 are required; macOS x64, Linux arm64
  and Windows arm64 are wanted where they are cheap;
- the theme follows the device, with light and dark overrides;
- CLI runs (match, SPRT, SPSA, tunes) are not shown in the desktop
  application;
- no served dashboard now, but the architecture must not close off
  broadcasting tournaments on the web, or controlling them remotely, later;
- CPU placement matters for gates and tunes, which stay CLI work; the
  desktop application does not need it and must be able to turn it off;
- tournaments of 64 participants are the design point; several tournaments
  may run at once; a running tournament can be amended (a participant
  removed or added, the length changed) and an engine may change between
  sessions.

## Decision

1. **This repository's product is the CLI**, released for every supported
   platform. The egui application, `engine::scheduler` and the SQLite game
   store are retired from it once the new desktop application ships (step
   14.1). No 1.x tournament data is migrated.
2. **The desktop application lives in its own repository** and is a client of
   the CLI. It starts the CLI as a child process and talks to it through a
   versioned, line-delimited JSON protocol on standard input and output: the
   relationship a chess GUI has with a UCI engine. It links no Colosseum
   crate: the boundary is the protocol, and a shared crate would let a second
   game-playing or rating implementation grow in the desktop application by
   accident.
3. **The protocol is the contract.** Its specification, JSON Schema and
   conformance tests live in this repository with the CLI (PLAN §S5.15, filled
   in at step 11.4). The session handshake carries the protocol version; the
   desktop application pins and bundles one CLI release and runs it against
   the published fixtures in its own CI. Messages are defined apart
   from their transport, so the same messages can later be carried over a
   local socket or a WebSocket for re-attaching to a running tournament,
   broadcasting or remote control, without a second protocol.
4. **Ownership.** The CLI owns everything that plays or scores games:
   scheduling, placement, clocks, adjudication, fault policy, persistence and
   resume, PGN, and the joint maximum-likelihood ratings. The desktop
   application owns presentation and the user's own data: the engine library
   (names, versions, logos, saved options, library ratings and their
   writeback), tournament presets, the list of the user's tournaments and
   where their run directories are, and the application settings. The desktop
   application never re-implements a rating or statistic, and holds no chess
   logic: positions reach it as FEN. It configures a tournament by writing a
   run file the CLI is started on, and its tournament list is a rebuildable
   index over run directories. CPU placement stays a CLI feature; the desktop
   application starts tournaments with placement off.
5. **Technology, provisionally:** a TypeScript + React front end in a Tauri 2
   shell whose Rust side only manages the CLI process, windows, file dialogs
   and updates. It is confirmed or replaced by the performance proof at GUI step
   2.3, measured against the budgets set at GUI step 1.2 with the CLI's
   synthetic streams. The fallbacks keep the front end and change the shell
   first: Electron, then Tauri's Chromium runtime once it is stable; Qt/QML
   and Flutter only after those. Because the boundary is a
   process protocol, replacing the front-end technology does not touch this
   repository.

## Consequences

- The "one real cost" of two game-playing implementations (PLAN §S2) is
  removed by deletion rather than by extraction; the `colosseum-harness`
  library extraction of the old Phase 11 is not needed and is dropped.
- A crash of the desktop application cannot corrupt a tournament: the CLI's
  durable run directory is the record, and a stopped tournament resumes from
  it.
- The CLI gains a machine protocol (live events and a control channel) that
  any front end or script can use.
- Protocol changes need coordination across two repositories; the schema,
  the version field and the conformance suite are the mitigation.
- The desktop application ships a CLI binary per platform, so its release
  depends on a CLI release for every platform it supports.
- ADR-0006's one-repository model stays in force until step 14.1; from then
  on this repository releases only the CLI and the `gui-v` lane is retired.
  ADR-0004 (engine library policy stays with the desktop application) is
  unchanged in substance.
- Naming of both products and repositories is decided at step 11.1, which
  may supersede ADR-0009.
- The two repositories advance together and release in quick succession:
  the CLI release that carries the protocol first, the desktop application
  pinned to it immediately after (PLAN §S8 joint milestones).
