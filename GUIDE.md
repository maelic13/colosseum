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
| Branch / versions | Colosseum GUI **1.0.2** released (legacy `v` tag), **1.1.0** in the manifest. Colosseum CLI **0.1.0**, unreleased. The next release ships both: GUI **1.1.0** (`gui-v1.1.0`, "latest") and CLI **0.1.0** (`cli-v0.1.0`, never "latest") |
| What exists | Phases 0–9 complete; **Phase 10 is complete pending publication** — 10.1–10.10, with the acceptance repeat passed locally on the step 10.10 source (10.9m rejected; 10.9h, 10.9p, 10.9t deferred behind the release). Every correction landed with its tests; qualification and symmetry runs recorded; the acceptance repeat re-recorded the parity matrix on the released source |
| What is missing | Only the maintainer's remote operations: squash-merge the release pull request on green CI, dispatch the CLI and GUI candidates on `main` and check their four-platform archives, tag `gui-v1.1.0` and `cli-v0.1.0`, check both release pages. **Phase 11** (GUI on the harness) starts after publication |
| Validation engines | **Rarog** (Rust) and **Basilisk** (C++) — available, active, different languages and build systems. Any two UCI engines would serve; nothing depends on these |
| Platform status | Windows/Linux/macOS ☑ required debug and optimized CI · Windows x86-64/ARM64, Linux x86-64 and macOS ARM64 CLI candidate archives ☑ exact-archive smoke · the `gui-v` release lane has never run; its candidate mode is the rehearsal to dispatch before the tag, and the only thing that will have exercised `deb`, `rpm`, `dmg` and `pkg.tar.zst` |
| Next step | **Publication is the maintainer's** — see "What to do now". Then **11.1** — Sol High (Claude: Opus 5, high). **12.1** — Terra High (Claude: Sonnet 5, high) — is independent of Phase 11 and can be taken first, after publication, as the CLI's first patch |
| Confirmed decisions | The 2026-09-22 set, confirmed by the maintainer and not reopened, recorded with the release preparation in [`phase-10-record.md`](docs/architecture/phase-10-record.md): exclusion for unspawnable engines, GUI 1.1.0, the xtask surface, the explicit GUI artifact list, versions kept in artifact names, the updater prerelease fix now and pagination later, the throughput margin not chased |

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

### Phase 12 — CLI maintenance from adoption (beside Phase 11, CLI patch releases)

- ☑ **12.1** — **Model: Terra High.** Rarog's tooling notes from the first
  adoption: a regression test that a resumed tune's `spsa status` has a
  finite ETA (the ETA itself was fixed by carrying the run's elapsed time in
  the checkpoint); the resume note names completed and remaining units
  ("resuming: N of M games complete, K to play") instead of the
  post-checkpoint replay count, and `run.log` records the stop and the
  resume events its documentation promises; under `--json` the note stays
  on stderr and the JSON value also carries the resume facts; one
  regression test per item; `CHANGELOG-CLI.md` Unreleased — PLAN §Phase 12(a)
- ☐ **12.2** — **Model: Terra High.** `spsa history`: the centre vector after
  every completed iteration, rebuilt from the journal, as a table, `--json`
  or `--csv` — PLAN §Phase 12(b)
- ☐ **12.3** — **Model: Terra High.** `spsa --seed-from <run dir>`: a fresh
  tune starting from a finished tune's rounded final centres, same surface
  unless `--tune` is given — PLAN §Phase 12(b)
- ☐ **12.4** — **Model: Terra High.** A warning, kept in the run record,
  whenever a command-line option list replaces a run file's list and drops
  values it named — PLAN §Phase 12(b)

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

## What to do now

**Phase 10 is complete; the release is the maintainer's to publish.** In
order, and all of it remote work that no agent performs:

1. Squash-merge the release pull request into `main` on green CI. The
   release workflows can only be dispatched once they are on `main`.
2. Dispatch the CLI candidate and the GUI candidate on `main` (Actions →
   *Colosseum CLI/GUI candidate and release* → Run workflow, or
   `gh workflow run release-cli.yml --ref main` and the same for
   `release-gui.yml`) and check their four-platform archives. The GUI lane has
   never run end to end, and this is the only thing that will have exercised
   `deb`, `rpm`, `dmg` and `pkg.tar.zst` before a tag exists. A failure is
   fixed on `main` and the candidate dispatched again.
3. Check out `main` locally and run `cargo xtask release-check gui-v1.1.0`
   and `cargo xtask release-check cli-v0.1.0`.
4. Tag the accepted commit `gui-v1.1.0` and `cli-v0.1.0`, and push the tags;
   each tag's workflow publishes its product's GitHub Release.
5. Check both release pages and the two post-tag links recorded with the
   Phase 10 entry in
   [`phase-10-record.md`](docs/architecture/phase-10-record.md).

**Then Phase 11 starts, and is the main line of development from then on.**
Work its steps in order, one commit per step, as before.

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
