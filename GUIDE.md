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
| Branch / versions | `cli`; Colosseum GUI **1.0.2** released (legacy `v` tag), **1.1.0** in the manifest. Colosseum CLI **0.1.0**, unreleased. The first release from `cli` ships both: GUI **1.1.0** (`gui-v1.1.0`, "latest") and CLI **0.1.0** (`cli-v0.1.0`, never "latest") |
| What exists | Phases 0–9 complete; Phase 10 corrections 10.1–10.9y complete (10.9m rejected; 10.9h, 10.9p, 10.9t deferred behind the release). Candidate `823b398` passed four-platform archive smoke; every correction since has landed with its tests; qualification and symmetry runs recorded |
| What is missing | **10.9z** (user documentation, finished), then **10.10** (acceptance repeat, merge, tags). Phase 11 (GUI on the harness) after publication |
| Validation engines | **Rarog** (Rust) and **Basilisk** (C++) — available, active, different languages and build systems. Any two UCI engines would serve; nothing depends on these |
| Platform status | Windows/Linux/macOS ☑ required debug and optimized CI · Windows x86-64/ARM64, Linux x86-64 and macOS ARM64 CLI candidate archives ☑ exact-archive smoke · the `gui-v` release lane has never run; 10.9y gave it a candidate mode to rehearse with |
| Next step | **10.9z** — Terra High (Claude: Sonnet 5, high). Then **10.10 — EXIT** |
| Confirmed decisions | The 2026-09-22 set in PLAN §S8 "Phase 10 — open items", confirmed by the maintainer and not reopened: exclusion for unspawnable engines, GUI 1.1.0, the xtask surface, explicit GUI artifact list, versions kept in names, updater prerelease fix now and pagination later, throughput margin not chased, private-commit measure for the flaky test |

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
- ☑ **Phase 9** (9.0–9.7) — Naming retained (ADR-0009), versioned `docs/cli/` with a generated reference (ADR-0010), README, candidate bundle, coverage and usability acceptance, release acceptance of candidate `823b398`.
- ☑ **Phase 10, corrections** (10.1–10.9y) — Adjudication off by default, class-aware placement, per-move PGN annotations, final-centre SPSA estimator, graceful stop, fixed rating field, book-range refusal, command-module split, shared game slots, pair identity in PGN, review defects, unscorable tagging, progress reports, commit path off the game loop, writer hardening, latency instrument, one slot per game, fault allowance, SPSA occupancy and scale, persistent engines per slot, real-engine qualification and symmetry, adoption-audit corrections, a game whose engine could not start excluded from scoring, the release versions and tag contract fixed, one build entry point for both products. 10.9m rejected (the forfeits were an engine defect); 10.9h, 10.9p, 10.9t deferred behind the release; 10.9g's two maintainer probes still owed and not blocking.

## Forward tracker

<!-- TRACKER FORMATTING RULES — follow them, they get broken often:
     1. ONE step per bullet. Never join two steps on one line.
     2. Use renderer-independent Unicode markers; Codex does not reliably
        implement GitHub task-list `[ ]` / `[x]` syntax:
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
When reporting the next step, always report its model as well.

### Phase 10 — Release preparation (before `cli-v0.1.0` and `gui-v1.1.0`)

- ☑ **10.9w.1 — DONE** — **Model: Sol High.** The GUI honours `scorable`: a game whose
  engine could not be spawned (`Termination::Aborted`, `scorable: false`) is
  excluded from standings and from the ML rating, on the live path and on
  the DB replay at resume; the recent-errors text says "not scored"; the
  tournament still completes without retrying the game; an aborted game is
  re-queued as pending on the next Start so it is played once the engine's
  path is fixed; smoke test `failed_engine_loses_with_error`
  becomes `failed_engine_is_not_scored` (zero games for both sides, rating
  unchanged); a store unit test covers the replay; `CHANGELOG-GUI.md` 1.1.0
  records it under Changed. Blocks the GUI release; the CLI is untouched —
  PLAN §Phase 10(af.1)
  - Two additions the implementation needed, both in PLAN §Phase 10(af.1):
    `resume_tournament` takes the caller's library and repairs one thing —
    a participant whose recorded executable no longer exists, when the
    library holds the same engine id at a path that does — because resume
    otherwise rebuilds every engine from the stored snapshot and would
    replay the same missing executable for ever; and an unscorable game is
    left out of the PGN export too
  - Demonstrated in the app on 2026-09-22, a `--portable` copy with "Ghost
    1.0" at a path that does not exist: the tournament finished 2/2 with
    both engines on zero games, zero points and unchanged Elo, terminations
    `Aborted` 2, and the message "Ghost 1.0 vs Rarog 2.4.0 (round 1) — not
    scored, the game was not played. Detail: io error: The system cannot
    find the path specified. (os error 3)". Correcting Ghost's path and
    reopening showed the tournament back at 0/2 and Stopped; Start then
    played both games to Checkmate
- ☑ **10.9x — DONE** — **Model: Sol High.** Versions and the tag contract: GUI
  manifest, updater and changelog at **1.1.0**; CLI stays 0.1.0 with the
  1.0.0 conditions recorded; `gui-v`/`cli-v` are the only accepted tag
  forms; `colosseum-release`, both workflows and both manifests agree and
  both tags validate; the updater also filters releases marked prerelease,
  with a test; the `per_page=100` limit recorded, not fixed — PLAN §Phase
  10(ag)
  - `colosseum-release` validates `gui-v1.1.0` and `cli-v0.1.0` and refuses
    the rest: an unscoped `v1.1.0` and a `gui-1.1.0` on the prefix, build
    metadata, a CLI prerelease, and a tag whose version the manifest does
    not carry. `colosseum 1.1.0` and `colosseum-cli 0.1.0` from the built
    binaries; the workflows and the WiX package take every version from the
    manifest through that tool, so there is nothing else to keep in step
  - What `0.x` means for a CLI user is now in `CHANGELOG-CLI.md` under
    0.1.0; the conditions for 1.0.0 are in PLAN §Phase 10(ag)
- ☑ **10.9y — DONE** — **Model: Sol High.** One build entry point: `tools/xtask`
  with `build <gui|cli>`, `package <gui|cli>` (archives, smoke) and
  `release-check <tag>`; `.cargo/config.toml` alias; the three build
  scripts and the `/dist/` ignore rule deleted; one artifact naming scheme
  for both products, `<colosseum-gui|colosseum-cli>-<version>-<windows|linux|macos>-<x64|arm64>.<ext>`,
  applied to `tools/release`, both smoke scripts, both workflows and the
  README; both workflows call the xtask; no `SHA256SUMS` published;
  `release-gui.yml` downloads named patterns, asserts the exact ten-file
  list and count, publishes that list, and gains a `workflow_dispatch`
  candidate mode; `docs/DEVELOPMENT.md` updated. Exit: on Windows,
  `cargo xtask package cli` and `package gui` produce archives equal to the
  candidate's file lists, each passing its `Smoke-*Archive.ps1` — PLAN
  §Phase 10(ah)
  - Demonstrated on Windows 2026-09-22: `colosseum-cli-0.1.0-windows-x64.zip`
    (29 entries) and `colosseum-gui-1.1.0-windows-x64.zip` (3), each with its
    SHA-256 printed and each passing its smoke script; `release-check` passes
    for both tags and refuses an unscoped `v1.1.0`
  - Found and fixed while running it: `Smoke-GuiArchive.ps1` deleted its
    scratch directory while Windows still held the executable open, failing
    *after* reporting the archive good. The GUI lane had never run, so
    nothing had caught it — PLAN §Phase 10(ah)
- ☐ **10.9z** — **Model: Terra High.** User-facing documentation finished:
  `README.md` (front door, both products, downloads per platform under the
  unified names, first tournament, first `match`/`sprt`/`spsa` with a run
  file), `README-CLI.md`, both changelogs, `docs/cli/` and
  `docs/DEVELOPMENT.md` in agreement with the versions and commands 10.9x
  and 10.9y fixed; every link checked; post-tag links listed for the
  maintainer. The 2026-09-22 pass drafted README and the changelog section;
  this step reconciles them — PLAN §Phase 10(ai)
- ☐ **10.10 — EXIT** — **Model: Sol High.** Release acceptance repeat: the
  two flaky tests made robust first — the working-set comparison moved to
  private commit, and the progress-status timing race in
  `phase10_progress.rs` replaced by a bounded wait (PLAN §Phase 10(af.2));
  command reference regenerated; both changelogs dated; Phase 4B
  oracle replay and Phase 8.1 parity matrix on the corrected source, run
  exactly as recorded (locate the Rarog 2.3.1 build the hashes belong to,
  SHA-256 `2a95390d…`, or re-record); short third-party usability flows; CLI
  candidate and GUI candidate dispatched and their exact archives smoked on
  all four platforms; full suite green in debug and `ci-release`. Then the
  maintainer opens the pull request, waits for green CI, squash-merges,
  tags the squash commit `gui-v1.1.0` and `cli-v0.1.0`, and checks both
  release pages (artifact lists, names, "Latest" on the GUI only, notes) —
  PLAN §Phase 10(j)

### Phase 11 — The GUI on the harness (after publication)

- ☐ **11.1** — **Model: Sol High.** Harness library crate
  (`colosseum-harness`): run directory, run record, placement resolution and
  the match/SPRT/tournament drivers moved out of `colosseum-cli`; observer
  port for per-game live state and run snapshots; CLI becomes a thin
  composition root; architecture tests keep GUI/windowing packages out of the
  harness graph; CLI tests, fixtures and generated reference unchanged; no
  CLI version change — PLAN §Phase 11(a)
- ☐ **11.2** — **Model: Sol High.** GUI tournaments run through the harness
  driver, consuming the `runtime_adapter.rs` seam: placement, adjudication
  default, fault classification and unscorable exclusion, annotated
  `games.pgn` and run directories under the app data directory; SQLite stays
  the GUI-owned history index and resume mapping, not the game store; live
  view reads the observer port; every Phase 10 mechanism reaches desktop
  tournaments here with no second implementation; engine-process policy a
  per-tournament choice with fresh processes the GUI default — PLAN §Phase
  11(b)
- ☐ **11.3** — **Model: Sol High.** Retire `engine::scheduler` and the
  `tournament` feature's game-store execution path; read-only migration keeps
  old SQLite history openable; `CLAUDE.md`, architecture docs and a new ADR
  record the single-mechanism decision — PLAN §Phase 11(c)
- ☐ **11.4 — EXIT** — **Model: Terra High.** GUI release: stored-data rating
  parity within 0.01 Elo, design guidelines checked, changelog records the
  adjudication default and run directories, version chosen by the maintainer
  (2.0.0 recommended), GUI candidate passes archive smoke — PLAN §Phase 11(d)

### Deferred and post-release (not steps until reopened)

- **10.9h** — Placement per platform research note (Linux isolated cores and
  IRQ affinity, macOS advisory contract, WSL excluded). Reopens on a Linux
  or macOS placement report, or before CLI 1.0.0 — PLAN §Phase 10(r)
- **10.9t** — Overlapped SPSA iterations, `--overlap 1`, off by default.
  Reopens when a real Rarog tune on the released binary shows wall-clock
  that matters; taken before 10.9p — PLAN §Phase 10(ad)
- **10.9p** — Two games per physical core for `spsa` and `match` only, as a
  measured experiment. Reopens only if 10.9t leaves a margin worth it —
  PLAN §Phase 10(z)
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
  branch; both changelogs are dated.
- Open a pull request to `main`; wait for green CI; squash-merge; keep the
  working branch (its commits carry the step identifiers); tag the squash
  commit; push the tags; watch both release workflows; check both release
  pages and open the post-tag links listed by 10.9z. Wrong release: delete
  release and tag, fix on `main`, tag again.
- A GUI patch found in use goes to `main` as `gui-vX.Y.(Z+1)` on its own;
  the CLI is not re-released for it, and vice versa.

## What to do now

**Phase 10 is open; the next step is 10.9w.1.** Work the steps in order, one
commit per step. Steps 10.2 through 10.9w changed game-playing behaviour or
its record; do not repeat the "after changing anything that runs games"
procedure per step, run it once at 10.10 on the final state. The merge of
`cli` to `main`, the two tags and publication remain maintainer-owned
operations and happen only after 10.10 passes. Phase 11 starts after
publication and is the main line of development from then on.

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
