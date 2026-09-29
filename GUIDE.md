# Colosseum — development guide

The short operational view: where the work stands and what to do next.
Rationale, specifications, success criteria and the open items' full form
live in [`PLAN.md`](PLAN.md); the evidence of completed phases lives in
[`docs/architecture/`](docs/architecture/README.md).

**This file and `PLAN.md` are the maintainer-facing pair.** `README.md` is the
user-facing front door for the whole project (the GUI and the CLI);
`docs/cli/` is the user-facing CLI detail. Neither may carry phase numbers,
internal naming or method argumentation.

## Current checkpoint

| | |
|---|---|
| Branch / versions | `main` holds the published **GUI 1.1.0** (`gui-v1.1.0`, "latest", 2026-09-22) and **CLI 0.2.0** (`cli-v0.2.0`, 2026-09-25). `dev` builds the CLI alone since 11.2.1 (the egui application stays on `main` until 14.1) and carries the GUI v2 programme's plans and ADRs 0011–0012 |
| What exists | Phases 0–10 complete and published (10.9m rejected; 10.9h, 10.9p, 10.9t deferred). Phase 12.1–12.4 complete and published as CLI 0.2.0. GUI v2 decided 2026-09-27: the desktop application becomes a separate client of the CLI in its own repository ([ADR-0011](docs/architecture/adr/0011-desktop-application-as-separate-cli-client.md)); the programme was reviewed and re-cut the same day into Phases 11, 13 and 14 here and GUI Phases 1–5 in the GUI repository. 11.1 complete 2026-09-28: names and repositories in [ADR-0012](docs/architecture/adr/0012-names-and-repositories-after-the-split.md). 11.2 complete 2026-09-29: the GUI repository [`maelic13/colosseum-gui`](https://github.com/maelic13/colosseum-gui) published with the seed as its root (M1) |
| What is missing | Here: **Phase 11** (the CLI gap analysis and its decisions 11.3.1–11.3.5, the protocol specification), **Phase 13** (the protocol in the CLI, one minor release), **Phase 14** (swap, merge and release); 12.5. In the GUI repository: GUI Phases 1–5 (design, repository and technology proof, screen specifications, implementation, release) |
| Validation engines | **Rarog** (Rust) and **Basilisk** (C++) — available, active, different languages and build systems. Any two UCI engines would serve; nothing depends on these |
| Platform status | Windows/Linux/macOS ☑ required debug and optimized CI · Windows x86-64/ARM64, Linux x86-64 and macOS ARM64 CLI candidate archives ☑ exact-archive smoke · the `gui-v` release lane has run once, for 1.1.0, left `dev` at 11.2.1 and leaves `main` at 14.1; its candidate mode is the rehearsal before any `gui-v1.1.x` patch |
| Next step | **12.5** — an unparsable start position refused — `I1` (Claude Sonnet 5 — Medium), the one step here with nothing to wait for. In the GUI repository: **1.1.1–1.1.4**, the maintainer's decisions from the inventory — `R2` (Claude Opus 5 — High) — then 1.2; 2.1 may start. 11.3 waits for GUI 1.2. Both repositories work on `dev`; Phase 12 patches ship from `main` and are merged into `dev` |
| Confirmed decisions | The 2026-09-22 set, confirmed by the maintainer and not reopened, recorded with the release preparation in [`phase-10-record.md`](docs/architecture/phase-10-record.md): exclusion for unspawnable engines, GUI 1.1.0, the xtask surface, the explicit GUI artifact list, versions kept in artifact names, the updater prerelease fix now and pagination later, the throughput margin not chased. The 2026-09-27 set for GUI v2, in [ADR-0011](docs/architecture/adr/0011-desktop-application-as-separate-cli-client.md) and PLAN §Phase 11: design first; the desktop application a separate repository over a CLI protocol; this repository the CLI alone; 1.x tournaments not migrated; no CLI runs in the GUI; device theme with light and dark; no served dashboard, broadcast and remote control kept possible; front-end technology provisional (TypeScript + React) until measured. The same day's review set, in PLAN §S8 maintainer requirements and §S5.15: placement stays a CLI feature and desktop tournaments run with it off, several at a time; 64 participants the design point; lifecycle and close behaviour analysed at implementation (13.4 / GUI 4.2) with *stop now on close* the starting preference; changed engines and amendments (add, remove, length) are CLI mechanisms decided at 11.3; configuration by run file, a rebuildable index, positions as FEN, two event classes, emission off the game path, a handshake version, short-lived queries, synthetic streams; the shell a measured choice at GUI 2.3 between Electron and Tauri with Electron the default expectation, shadcn/ui on Radix and Tailwind the provisional component system; the 1.x engine library imported. The 2026-09-28 naming set, in [ADR-0012](docs/architecture/adr/0012-names-and-repositories-after-the-split.md): Colosseum and `colosseum-cli` kept; the desktop repository `maelic13/colosseum-gui` until the swap at 14.1, before both releases, when it takes `maelic13/colosseum` and this repository becomes `maelic13/colosseum-cli`; both repositories on `dev`, merged by one pull request each after the swap, with the egui code off `dev` after GUI 1.1 (revised 2026-09-29); plain `v` tags in both, one `gui-v2.0.0` bridge release for the 1.1.0 updater, the 1.x releases and tags deleted from this repository at 14.2; the naming risk accepted, both products renamed together if a conflict ever arises; the new application's data directories apart from 1.x's |

## Current model mapping

PLAN §S8 records each step's stable capability class; this table maps the
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

## Completed phases

One line each; the numbered steps, their model labels and their evidence are
in [`docs/architecture/phases-0-9-record.md`](docs/architecture/phases-0-9-record.md)
and [`docs/architecture/phase-10-record.md`](docs/architecture/phase-10-record.md).
Step identifiers are never reused.

- ☑ **Phase 0** (0.1–0.8) — Current-state audit, Clean Architecture target, ADRs 0001–0008, independent product releases in one repository, the Colosseum naming decision.
- ☑ **Phase 1** (1.1–1.9) — Pentanomial statistics and normalized Elo in `colosseum-core`, the analytic and external oracle fixture corpus, the hermetic required suite.
- ☑ **Phase 2** (2.1–2.10) — Boundary migration, the independently versioned `colosseum-cli` package, run files, run records, run directories, `self-test`, `status`.
- ☑ **Phase 3** (3.1–3.8) — OS topology adapters, deterministic placement, hard affinity where the OS allows it, `capabilities`.
- ☑ **Phase 4A** (4A.1–4A.8) — Fixed-match runner with explicit clock accounting, typed engine faults, books and durability.
- ☑ **Phase 4B** (4B.1–4B.6) — Pair-atomic capped SPRT, oracle replay and live parity against fastchess and cutechess.
- ☑ **Phase 4C** (4C.1–4C.3) — Optional identical-binary calibration.
- ☑ **Phase 5** (5.1–5.10) — SPSA kernel and driver, `spsa plan`, `spsa status`, `sprt --apply`.
- ☑ **Phase 6** (6.1–6.9) — `nps` and scaling sweeps, `book` tools, `stats` replay and planning, `suite`.
- ☑ **Phase 7** (7.1–7.3) — Round-robin and gauntlet tournaments with joint or anchored ML ratings, matching the GUI on a frozen fixture.
- ☑ **Phase 8** (8.1–8.3) — Parity repeated on the candidate, ponder adopted, remaining gaps decided.
- ☑ **Phase 9** (9.0–9.7) — Naming retained (ADR-0009), versioned `docs/cli/` with a generated reference (ADR-0010), README, candidate bundle, coverage and usability acceptance, release acceptance of the step 9.7 candidate.
- ☑ **Phase 10** (10.1–10.10) — Adjudication off by default, class-aware placement, per-move PGN annotations, final-centre SPSA estimator, graceful stop, fixed rating field, book-range refusal, command-module split, shared game slots, pair identity in PGN, review defects, unscorable tagging, progress reports, commit path off the game loop, writer hardening, latency instrument, one slot per game, fault allowance, SPSA occupancy and scale, persistent engines per slot, real-engine qualification and symmetry, adoption-audit corrections, a game whose engine could not start excluded from scoring, the release versions and tag contract fixed, one build entry point for both products, user documentation reconciled with them, and the acceptance repeat passed. 10.9m rejected (the forfeits were an engine defect); 10.9h, 10.9p, 10.9t deferred behind the release; 10.9g's two maintainer probes still owed and not blocking.

## Forward tracker

<!-- TRACKER FORMATTING RULES — follow them, they get broken often:
     1. ONE step per bullet. Never join two steps on one line.
     2. Use renderer-independent Unicode markers; not every Markdown
        renderer implements GitHub task-list `[ ]` / `[x]` syntax:
            - ☐ **1.2** — todo
            - ◐ **1.2 — IN PROGRESS** — genuinely in flight
            - ☑ **1.2 — DONE** — resolved
     3. Resolved outcome labels are DONE · REJECTED · DEFERRED → <item> ·
        PARKED · FIXED. Anything resolved uses ☑, never ◐.
     4. Continuation lines indent 2 spaces. Sub-items indent 2 more spaces,
        use a normal `-` bullet, and indent their continuations another 2.
     5. Once implementation starts, NEVER renumber existing items — commits
        reference them. To insert before an item use a dotted suffix on the
        previous one (10.9w.1 follows 10.9w).
     6. Always mark a completed step here in the same commit. Add status or
        evidence to PLAN.md when it improves the durable specification; do not
        duplicate routine tracker detail there.
     7. Blank line AFTER the `###` heading, then NO blank lines between
        bullets: one continuous list per phase.
     8. ONLY NUMBERED STEPS live here. Recurring procedures go in their own
        section and never get a status marker.
     9. Every numbered step includes its PLAN §S8 model assignment. Keep the
        label and the routing table synchronized.
    10. When a phase completes, collapse its steps to one line under
        "Completed phases" and move its PLAN text to a record file under
        docs/architecture/. -->

Each phase ends with a verifiable exit criterion — see PLAN §S8. Nothing is
"done" because it compiles; it is done when its criterion is demonstrated.
When reporting the next step, always report its class and the model the
mapping above gives it.

### The GUI v2 programme — Phases 11, 13 and 14 here; GUI Phases 1–5 in the GUI repository

Both repositories advance together and release together; the joint
milestones M0–M9 are in PLAN §S8. A step here that names a GUI step waits
for it, and the GUI tracker says the same the other way.

#### Phase 11 — Separation

- ☑ **11.1 — DONE** — `R2` — Naming of both products, executables and
  repositories after a dated collision screen: Colosseum and
  `colosseum-cli` kept, the repository swap before both releases
  (revised 2026-09-29), plain `v` tags;
  [ADR-0012](docs/architecture/adr/0012-names-and-repositories-after-the-split.md)
  — PLAN §Phase 11 (M0)
- ☑ **11.2 — DONE** — `M` — The GUI repository seed finalised
  under the 11.1 names with ADR-0012's obligations and handed over;
  [`maelic13/colosseum-gui`](https://github.com/maelic13/colosseum-gui) published with it as its root
  commit (`22d0be1`, identical to the seed at `54b9c47`);
  [`docs/gui-v2/`](docs/gui-v2/README.md) reduced to a pointer; `README.md`
  names the new application — PLAN §Phase 11 (M1)
- ☑ **11.2.1 — DONE** — `I1` — After GUI 1.1, on `dev` only: the egui
  application, `engine::scheduler`, the SQLite store, the runtime adapter
  and the `gui-v` lane and packaging removed; `docs/design/` archived;
  documents describe a CLI-only repository; ADR-0006 superseded; `main`
  keeps 1.x until 14.1. Evidence 2026-09-29: `cargo fmt --check`, `check`,
  `clippy -D warnings`, 543 tests in 18 suites and the command-reference
  check green; only the architecture tests' forbidden-dependency guards
  still name the GUI stack — PLAN §Phase 11
- ☐ **11.3** — `R2` — After GUI 1.2: the CLI against every
  inventory and requirement item — supported / CLI extension / GUI-owned /
  not feasible; the changed-engine behaviour and the amendment semantics
  decided with the maintainer; `docs/architecture/gui-gap-analysis.md`;
  13.5–13.7 confirmed or re-cut; the inventory's CLI column taken in —
  PLAN §Phase 11 (M2)
- ☐ **11.3.1** — `R2` — Decide engine option resolution: threads and
  hash mapped and clamped, compatibility notes, tablebase settings and
  switch — in the CLI or composed by the application (inventory PLY-11–13)
  — PLAN §Phase 11
- ☐ **11.3.2** — `R2` — Decide ponder: `Ponder=false` when off; ponder
  without a clock or separate cores (PLY-14) — PLAN §Phase 11
- ☐ **11.3.3** — `R2` — Decide adjudication: defaults offered,
  one-sided resign, book and scoreless plies in the windows, the start
  position in threefold (PLY-15, PLY-35) — PLAN §Phase 11
- ☐ **11.3.4** — `R2` — Decide faults: an engine that cannot start,
  the fault allowance, a fixed-search hang (PLY-17, PLY-18, PLY-35) — PLAN
  §Phase 11
- ☐ **11.3.5** — `R2` — Decide scheduling: parallel games changed
  during a run; colours across cycles at one game per pair (PLY-26,
  DEF-14) — PLAN §Phase 11
- ☐ **11.4 — EXIT** — `R2` — Protocol specification: PLAN
  §S5.15 message catalogue, run-file hand-off, amendment and changed-engine
  semantics, versioning, JSON Schema from Rust types, conformance fixture
  format, synthetic-stream surface; `docs/cli/protocol.md` — PLAN §Phase 11
  (M3)

#### Phase 13 — The front-end protocol (one CLI minor release)

- ☐ **13.1** — `I2` — Session: flag, framing, handshake,
  bounded event pipeline with state coalescing, fact sequence numbers and
  `gap`, snapshot request, error responses, schema validation in tests; run
  state only — PLAN §Phase 13
- ☐ **13.2** — `I1` — Synthetic streams: a synthetic tournament
  at chosen scale and rate, and a replay of a finished run directory; both
  schema-valid without engines; recorded as fixtures — PLAN §Phase 13 (M4)
- ☐ **13.3** — `I2` — Live events from `tournament run`: game
  start, every move with SAN, clocks, FEN and the opening name, search state per side, game end,
  standings and rating snapshot per scored game; artifacts identical with
  and without the protocol (stub engines) — PLAN §Phase 13
- ☐ **13.4** — `R2`, then `I2` — Control and lifecycle: *stop* and *stop
  now* as protocol commands on every platform; the end-of-input, detach,
  re-attach and close-behaviour analysis done with GUI 4.2 and decided by
  ADR; clean-stop suite extended — PLAN §Phase 13 (M5)
- ☐ **13.5** — `I2` — Amendments: remove a participant, add a
  participant, change the length of a tournament in progress; append-only
  journal, stable game identity, amendment facts and record entries; kill /
  amend / resume tests; `stats` replays an amended run — PLAN §Phase 13
- ☐ **13.6** — `I1` — Changed engines: the 11.3 policy for
  an executable that changed since the run started, never silent, always
  recorded; a test per branch — PLAN §Phase 13
- ☐ **13.7** — `I1` — Queries: run summary, paged games with
  FEN per ply, standings and crosstable as read-only `--json` invocations
  within the CLI-side budgets on the largest target — PLAN §Phase 13 (M6)
- ☐ **13.7.1** — `I1` — Tournament statistics 1.x showed: SB and
  shared places, gauntlet score share, Perf, nps, depth, time per move,
  forfeits split, terminations, decisive/drawn; equal to 1.x on a frozen
  fixture — PLAN §Phase 13
- ☐ **13.7.2** — `I1` — Tournament options for the application:
  opening count cap, tournament name and date in the PGN, "name version"
  labels, and what 11.3.1–11.3.5 put in the CLI — PLAN §Phase 13
- ☐ **13.8 — EXIT** — `V` — Conformance suite green; schema
  and fixtures one release asset; `docs/cli/protocol.md` final; the CLI
  release candidate from `dev` for GUI 4.8 (M7) — PLAN §Phase 13

#### Phase 14 — Swap, merge and release

- ☐ **14.1** — `I1` — After GUI 4.8: on `dev`, the CLI lane on
  plain `v` tags claiming Latest and every old-address link moved; then the
  maintainer swaps the repositories and merges `dev` into `main` here and
  into `master` there, one pull request each (M8) — PLAN §Phase 14
- ☐ **14.2 — EXIT** — `V` — The CLI release (first `v` tag)
  from `maelic13/colosseum-cli`, then desktop 2.0.0 with its bridge
  release, pinned to it; then the 1.x releases and tags deleted (M9) —
  PLAN §Phase 14

### Phase 12 — CLI maintenance from adoption (beside Phases 11, 13 and 14, CLI patch releases)

- ☑ **12.1** — `I1` — Rarog's tooling notes from the first
  adoption: a regression test that a resumed tune's `spsa status` has a
  finite ETA (the ETA itself was fixed by carrying the run's elapsed time in
  the checkpoint); the resume note names completed and remaining units
  ("resuming: N of M games complete, K to play") instead of the
  post-checkpoint replay count, and `run.log` records the stop and the
  resume events its documentation promises; under `--json` the note stays
  on stderr and the JSON value also carries the resume facts; one
  regression test per item; `CHANGELOG-CLI.md` Unreleased — PLAN §Phase 12(a)
- ☑ **12.2** — `I1` — `spsa history`: the centre vector after
  every completed iteration, rebuilt from the journal, as a table, `--json`
  or `--csv` — PLAN §Phase 12(b)
- ☑ **12.3** — `I1` — `spsa --seed-from <run dir>`: a fresh
  tune starting from a finished tune's rounded final centres, same surface
  unless `--tune` is given — PLAN §Phase 12(b)
- ☑ **12.4** — `I1` — A warning, kept in the run record,
  whenever a command-line option list replaces a run file's list and drops
  values it named — PLAN §Phase 12(b)
- ☐ **12.5** — `I1` — An unparsable start position is refused
  instead of silently replaced by the standard one (from the 1.x
  inventory) — PLAN §Phase 12(c)

### Deferred and post-release (not steps until reopened)

- **10.9h** — Placement per platform research note (Linux isolated cores and
  IRQ affinity, macOS advisory contract, WSL excluded). Reopens on a Linux
  or macOS placement report, or before CLI 1.0.0 — [record](docs/architecture/phase-10-record.md), item (r)
- **10.9t** — Overlapped SPSA iterations, `--overlap 1`, off by default.
  Reopens when a real Rarog tune on the released binary shows wall-clock
  that matters; taken before 10.9p — [record](docs/architecture/phase-10-record.md), item (ad)
- **10.9p** — Two games per physical core for `spsa` and `match` only, as a
  measured experiment. Reopens only if 10.9t leaves a margin worth it —
  [record](docs/architecture/phase-10-record.md), item (z)
- **10.9g probes** — the 2,000-game 3+0.03 scramble probe and the 100 ms,
  14-slot, 50,000-move outlier probe, maintainer-run, informative, recorded
  in `docs/architecture/phase-10-record.md` when done
- **Asynchronous SPSA** — research, needs its own evidence — PLAN §S8
  Post-release research

## Recurring procedures

Not steps — they are never "done".

### Declaring a platform supported

- Full test suite green there, debug **and** optimized
  (`cargo test --workspace --all-targets --profile ci-release`).
- Affinity, process, timer and filesystem capabilities or fallbacks implemented,
  tested and documented there.
- The exact released CLI artifact passes `--version`, `--help`, headless
  `self-test` and one deterministic JSON-mode workflow.

### After changing anything that runs games

- Re-run the Phase 4B oracle replay and controlled live parity on compatible
  shared fields; repeat the release-candidate matrix at Phase 8.1.
- Consider a real-machine calibration after material clock, scheduling or
  affinity changes; it is evidence, not a release or usage prerequisite.
- Bump the harness version in run records, and `stats_version` if any reported
  statistic changed definition — with a changelog entry.

### After completing a generic workflow

- Migrate the corresponding Rarog and Basilisk harness workflow immediately.
- Compare old/new resolved inputs, schedule, durable artifacts and statistics.
- Archive the old generic implementation only after parity; retain declarative
  configs and thin project-policy/CI invocation.
- Record an exception as either a CLI mechanism gap or intentional
  engine-specific policy.

### Cutting a release (maintainer)

- `cargo xtask release-check gui-vX.Y.Z` and `cli-vX.Y.Z` pass on the
  commit to be tagged; both changelogs are dated.
- Open a pull request to `main`; wait for green CI; squash-merge; dispatch
  both candidates on `main` and check their archives; tag the accepted
  commit; push the tags; watch both release workflows; check both release
  pages and open the post-tag links the phase record lists. Wrong release: delete
  release and tag, fix on `main`, tag again.
- A GUI patch found in use goes to `main` as `gui-vX.Y.(Z+1)` on its own;
  the CLI is not re-released for it, and vice versa.
- The CLI release that carries a protocol change is tagged first; the GUI
  release that needs it follows, bundling and pinning that CLI version. A
  protocol minor version never ships in a CLI patch release.

## What to do now

**The GUI v2 programme is the main line of development; 11.1 and 11.2
are done.** Two repositories move together:

1. The GUI repository, [`maelic13/colosseum-gui`](https://github.com/maelic13/colosseum-gui), tracks its
   own steps in its `GUIDE.md`: GUI 1.1 (inventory of 1.x, read from this
   repository at `gui-v1.1.0`) and 2.1 (skeleton) start now. Once 1.1 is
   done, 11.2.1 removes the egui application from `dev` here.
2. Here: 11.3 waits for GUI 1.2 (requirements signed off); 11.4 follows and
   unblocks the GUI screen specifications (GUI 3.x).
3. Here: Phase 13 in order. 13.2 unblocks the GUI performance proof (GUI
   2.3); 13.3 and 13.4 unblock the GUI live view (4.5); 13.4's lifecycle
   decision is taken together with GUI 4.2; 13.5–13.7 unblock GUI 4.6.
4. 13.8 produces the CLI release candidate, from `dev`, that the GUI
   accepts against (GUI 4.8).
5. 14.1: the maintainer swaps the repositories (ADR-0012): this one becomes
   `maelic13/colosseum-cli`, the GUI repository takes `maelic13/colosseum`,
   and the same day Rarog's download addresses and every clone's `origin`
   are updated. Then `dev` is merged in both, one pull request each.
6. 14.2: the CLI release, then the GUI's 2.0.0 release pinned to it, then
   the 1.x releases deleted.

Each design step (GUI 1.1–1.4, 3.1–3.3) ends in a maintainer sign-off,
reviewed in pairs; no GUI screen is implemented before its specification is
signed off. Phase 12 steps are taken beside all of it as reports arrive.

```
git diff --check
```

## Working rhythm

```text
Pick the next ☐ step  ->  implement + test  ->  demonstrate its exit
criterion  ->  mark it ☑ here  ->  update PLAN.md when useful  ->  commit before
the next step.
```

Long game jobs (optional calibrations, parity runs, real gates) run on a real
machine and are pasted back; everything else is verified locally by tests.
Use a focused imperative commit subject, stage only the step's files and never
add co-author or assistant-attribution trailers; `AGENTS.md` is the binding
repository workflow.

## Decision rules

| Situation | Action |
|---|---|
| A phase "works" but its exit criterion is not demonstrated | Not done. Do not proceed |
| An inner layer needs a GUI/SQLite/Tokio/OS type | Stop and introduce or correct an application port/adapter; dependencies point inward |
| Our statistic disagrees with a compatible external oracle | Root-cause before shipping — one of them is wrong and it may be ours |
| External tools disagree or do not expose the same model | Record the matrix limitation; prefer analytic fixtures; never average or compare unsupported fields |
| OS cannot support a capability (e.g. hard macOS affinity) | Record the advisory/off fallback in the run record and PLAN; fail only if the capability was explicitly requested |
| Tempted to make one of our defaults mandatory | It belongs in PLAN §S3 Tier B with a reason, or Tier C as advice — Tier A needs a silent-wrong-number failure mode |
| Feature exists in an external runner but not here | Phase 8.2 decided the first set; "does a general engine developer need it?" is the tie-breaker for the next |
| Tempted to add engine-specific logic | It belongs in the engine's own tooling, not here |
| Engine project still needs scheduling/statistics/tuning/recovery code | Generic mechanism gap: add or explicitly decline it |
| Engine project keeps a run file or thin CI command | Expected project policy, not a CLI gap |
| Diagnostic heuristic looks stable | Report the observation; do not call it convergence or causation |
| A GUI behaviour differs from the CLI's for the same mechanism | The CLI's is the specification (PLAN §S5); Phase 11 removes the second implementation, so do not add a third |
| GUI v2 needs something the CLI does not provide | It is a CLI extension through the S5.15 protocol, recorded in the 11.3 gap analysis — never game-playing, rating or chess logic in the desktop application |
| Tempted to coordinate CPU placement across desktop tournaments | Placement is a CLI feature for gates and tunes; desktop tournaments run with it off |
| A protocol event could block on a slow reader | Not allowed: bounded queue, state dropped first, a `gap` fact instead of blocking (S5.15) |
| Tempted to decide the close / detach / re-attach behaviour early | Analysed and decided at 13.4 with GUI 4.2; until then every option must leave a resumable run |
| Tempted to implement a GUI v2 screen before its specification is signed off | Not allowed: design first (PLAN §Phase 11) |
