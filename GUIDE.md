# Colosseum CLI — development guide

The short operational view: where the harness stands and what to do next.
Rationale, specifications, success criteria and evidence live in
[`PLAN.md`](PLAN.md).

**This file and `PLAN.md` are the maintainer-facing pair.** `README.md` and
`CHANGELOG.md` are user-facing and must stay free of method, phase numbers and
internal naming.

## Current checkpoint

| | |
|---|---|
| Branch / version | `cli`; Colosseum GUI **1.0.2** released. CLI: **not started** |
| What exists | `colosseum-core` / `-uci` / `-engine` are headless (no `egui` dependency), cross-platform, released for Windows/Linux/macOS incl. arm64, 149 passing tests. `core/stats.rs` already has trinomial SPRT, Elo ± error, LOS, ML ratings |
| What is missing | pentanomial/nElo, CPU affinity, the CLI itself, SPSA, automatic run records, NPS protocol |
| First consumers | **Rarog** (`D:/code/rarog`, Rust) and **Basilisk** (`D:/code/basilisk`, C++) — both currently on ~3,400 lines of Windows-only PowerShell driving `fastchess`, plus a separately-patched `weather-factory` each |
| Platform status | Windows ☐ · Linux ☐ · macOS ☐ — support requires the full debug/release test suite and documented capability fallbacks; calibration is optional and machine-specific |
| Next step | **Phase 1 — pentanomial statistics and nElo in `colosseum-core`** |

## Forward tracker

<!-- FORMATTING RULES for this tracker — follow them, they get broken often:
     1. ONE step per `- [ ]` bullet. Never join two steps on one line.
     2. Continuation lines indent 6 spaces so they align under the text after
        "- [ ] ". Sub-items indent 4 SPACES then their own "- [ ]", with
        10-space continuations. At 2 spaces the renderer does NOT nest them.
     3. Status boxes: `[ ]` todo · `[~]` ONLY while genuinely in flight ·
        `[x]` finished — done, rejected, deferred or parked. Anything resolved
        is `[x]`, never `[~]`, so the renderer strikes it through.
     4. Every `[x]` item opens with its STEP NUMBER, then a BRACKETED OUTCOME
        TAG in bold: number BEFORE tag, never the reverse.
            - [x] 1.2 **[DONE]** Pentanomial variance — ...
            - [x] 5.3 **[DEFERRED → 8b]** Chess960 — ...
        Tags: DONE · REJECTED · DEFERRED → <item> · PARKED · FIXED.
     5. Once implementation starts, NEVER renumber existing items — commits
        reference them. To insert before the first item use a .0.
     6. Mirror any status change into PLAN.md in the same commit.
     7. Blank line AFTER the `###` heading, then NO blank lines between
        bullets: one continuous list per phase.
     8. ONLY NUMBERED STEPS live here. Recurring procedures go in their own
        section and never get a checkbox. -->

Each phase ends with a verifiable exit criterion — see PLAN §S8. Nothing is
"done" because it compiles; it is done when its criterion is demonstrated.

### Phase 1 — Pentanomial statistics and nElo (`colosseum-core`)

- [ ] 1.1 Pair-level scoring: fold paired games into `Ptnml(0-2)`, with a
      documented rule for odd/unpaired tails
- [ ] 1.2 Pentanomial variance, normalized Elo, logistic Elo ± error, LOS,
      draw ratio, pairs ratio, WL/DD ratio
- [ ] 1.3 SPRT over the pentanomial model (`elo0`/`elo1`/`alpha`/`beta`, LLR,
      both bounds, H0/H1/continue)
- [ ] 1.4 Typed errors for degenerate inputs — no NaN/Inf escapes
- [ ] 1.5 Property tests: LLR zero at N=0, monotone in score, arm-swap
      symmetry, bounds equal `log(β/(1−α))` and `log((1−β)/α)`
- [ ] 1.6 **EXIT** — golden-file parity against ≥6 stored Rarog runs (incl. one
      stopped mid-flight, one H0, one H1, one null): Elo/nElo/LLR to ≤1e-6,
      `Ptnml` exact

### Phase 2 — CLI skeleton, direct UCI invocation and run records

- [ ] 2.1 New `colosseum-cli` workspace member with a `[[bin]]`; argument
      parsing; `--version`/`--help`
- [ ] 2.2 Direct engine controls: executable, optional display name, arguments,
      working directory, arbitrary UCI options and allocated core count
- [ ] 2.3 Optional human-authored run TOML; precedence is built-in defaults <
      TOML < CLI; write the fully resolved configuration as JSON
- [ ] 2.4 `engine inspect` and `engine check` over the normal UCI protocol
- [ ] 2.5 Automatic run records (PLAN §5.8) for every run, including aborted
      ones; no engine-supplied manifest or build metadata
- [ ] 2.6 Decide foundational runner controls now: restart/recovery,
      per-engine time margin, debug logging, opening repetition and
      machine-readable output
- [ ] 2.7 **EXIT** — two arbitrary UCI executables pass path-only inspect/check
      workflows; TOML plus CLI overrides resolves identically; run-record
      schema and aborted-run tests pass

### Phase 3 — CPU topology and affinity

- [ ] 3.1 Physical-core/sibling detection per OS — Windows
      `GetLogicalProcessorInformationEx`, Linux `thread_siblings_list`, macOS
      `sysctl`. ⚠ Never infer siblings from logical CPU numbering
- [ ] 3.2 Modes `auto` / `off` / explicit CPU list; one logical CPU per physical
      core; configurable headroom (default 2 free)
- [ ] 3.3 Allocate explicit `cores-per-engine` per game slot, independent of the
      engine-specific UCI option controlling worker threads
- [ ] 3.4 Fail when requested placement is unavailable; allow and record
      explicit `off`; report macOS as advisory or unavailable without
      prohibiting clock matches
- [ ] 3.5 **EXIT** — topology fixtures (Zen 3 16c/32t, P/E-core, no-SMT,
      2-socket) pass; residency integration tests pass where enforceable;
      platform capability reporting is documented

### Phase 4 — Fixed match, SPRT and optional calibration

- [ ] 4.1 `match` is fixed-N; `sprt` accepts explicit bounds/error rates/model,
      with configurable `gainer` `[0,3]` and `simplify` `[-3,0]` defaults;
      `calibrate` is separate
- [ ] 4.2 Configurable defaults: `tc=3+0.03`, compatible Hash=64 and worker
      count=1 options when advertised, both colours per pair, resign
      `movecount=3 score=600 twosided=true`, draw
      `movenumber=40 movecount=8 score=10`
- [ ] 4.3 Optional book argument. Without a book, start every game from the
      normal position; warn about poor opening diversity for SPRT/SPSA but run
- [ ] 4.4 Live report block, full log to disk, PGN out, generated run record,
      resume without double-counting
- [ ] 4.5 `calibrate`: enforce byte-identical binaries; configurable fixed N,
      confidence and tolerance (defaults 30k / 95% / ±5 nElo); classify PASS,
      FAIL, inconclusive or invalid; never require it before another command
- [ ] 4.6 **EXIT** — stored Rarog replay reaches the same verdict at the same
      game number (±1 report interval); path-only/no-book runs and every
      calibration outcome pass tests

### Phase 5 — SPSA

- [ ] 5.1 End-state schedule in `core`: knobs declare `c_end`, run declares
      `r_end` and horizon `N`; back-solve `c`, `a`, `A = 0.1N`; decay per
      ITERATION
- [ ] 5.2 Tune TOML selects numeric UCI options and gives initial values,
      tuning bounds and `c_end`; validate against the live UCI option schema
- [ ] 5.3 Defaults are 5,000 iterations and 32 games/iteration, both
      configurable and neither enforced as a minimum
- [ ] 5.4 Multi-session: periodic state save, log APPENDS on resume, stops
      itself at the horizon, prints iteration/percent/ETA, says out loud that
      the horizon is frozen at first launch
- [ ] 5.5 Tail-mean output for baking — the whole vector, no per-knob filter
- [ ] 5.6 Reject absent/non-spin options, duplicates, invalid/out-of-engine
      bounds, `min>=max`, and perturbations that round to zero; warn on
      default disagreement and a seed on a rail
- [ ] 5.7 Persistent driver: no per-iteration harness relaunch; load a supplied
      book once; assert the persisted schedule before the first game
- [ ] 5.8 **EXIT** — schedule property tests pass; every hard audit class has a
      rejected fixture; kill/resume continues the schedule; synthetic
      noisy-quadratic run lands in a stated RMSE band

### Phase 6 — Speed/NPS, book tools and statistics replay

- [ ] 6.1 `nps` drives an optional user-supplied EPD suite through UCI bounded
      searches, or warns and uses startpos; strict alternation, warm-up,
      arm-level median/best-of and bootstrap CI
- [ ] 6.2 Accept one or more executables per arm; a matching self-pair is
      optional and produces a configurable warning outside ±0.5%
- [ ] 6.3 `book slice` / `hash` / `stats` / `verify`
- [ ] 6.4 `stats` replays Colosseum runs and supported PGN/fastchess/cutechess
      result data through the same reporting code used live
- [ ] 6.5 **EXIT** — injected left-skewed sample reproduces the known bias in a
      naive ABBA estimator and NOT in the shipped one; slicing is
      byte-reproducible across platforms; every golden fixture replays through
      the CLI

### Phase 7 — Gauntlet CLI surface

- [ ] 7.1 `gauntlet`: opponent ladder, joint ML ratings with error bars, one
      anchor, standings/crosstable CSV
- [ ] 7.2 **EXIT** — CLI ratings match the GUI to ≤0.01 Elo on stored data

### Phase 8 — Parity vs fastchess/cutechess, and remaining gaps

- [ ] 8.1 **Parity run — the entry gate for trusting this harness.** Prepare one
      deterministic opening sequence, then use it sequentially with the same
      two binaries, TC and adjudication through `colosseum-cli`, `fastchess`
      and `cutechess-cli`. Compare Elo/nElo/LOS, pentanomial vector, draw rate
      and time-loss counts. Agreement inside combined error bars is the gate;
      disagreement must be root-caused. Repeat after game-running changes
- [ ] 8.2 Revisit remaining feature gaps after Phase 2's foundational decisions;
      adopt / decline with a reason / defer. Candidates: Chess960, ponder under
      test conditions and additional tournament formats
- [ ] 8.3 **EXIT** — parity demonstrated and every gap has a recorded decision

### Phase 9 — Release preparation for `colosseum-cli`

- [ ] 9.1 Ship as its own binary asset, versioned and released separately from
      the desktop app
- [ ] 9.2 Windows/Linux/macOS × x64/arm64 through the existing pipeline;
      smoke-test the exact published artifacts
- [ ] 9.3 User docs: quickstart, direct-engine and optional run-TOML reference,
      SPSA tune-TOML reference, a worked example per command, and a "how to
      trust a result" page (optional calibration, run records, self-pair
      validation). No phase numbers or internal naming
- [ ] 9.4 Path-only and optional TOML examples using ordinary UCI executables
- [ ] 9.5 **EXIT / ACCEPTANCE** — Rarog and Basilisk each run one real gate
      end-to-end through the released artifact on ≥2 operating systems,
      agreeing with 8.1; calibration is demonstrated but not required

## Recurring procedures

Not steps — they are never "done".

### Declaring a platform supported

- Full test suite green there, debug **and** release.
- Affinity, process, timer and filesystem capabilities or fallbacks are
  implemented, tested and documented there.
- Exact released artifact passes `--version`, `--help`, `engine check` and one
  stub-engine match.

### After changing anything that runs games

- Re-run the Phase 8.1 parity check against `fastchess`.
- Consider a real-machine calibration after material clock, scheduling or
  affinity changes; it is evidence, not a release or usage prerequisite.
- Bump the harness version recorded in run records, so results taken before and
  after are distinguishable forever.

## What to do now

**Phase 1 — pentanomial statistics and nElo in `colosseum-core`.**

It is first for a hard reason: every later phase reports through it, and
`stats.rs` today is trinomial over W/D/L. Rarog's and Basilisk's entire ledgers
are denominated in normalized Elo over pentanomial pairs, so until this exists
any number this tool produces is quietly incomparable with every number either
project has recorded.

It is also the cheapest phase to get right — pure functions, no I/O, no
platform surface, and the fixtures already exist in
`D:/code/rarog/tools/results/`.

```
cargo test -p colosseum-core
```

## Working rhythm

```text
Pick the next unchecked step  ->  implement + test  ->  demonstrate its exit
criterion  ->  tick it here AND in PLAN.md in the same commit.
```

Long game jobs (optional calibrations, parity runs, real gates) are run by the
user on the dev machine and pasted back; everything else is verified locally by
tests.

## Decision rules

| Situation | Action |
|---|---|
| A phase "works" but its exit criterion is not demonstrated | Not done. Do not proceed |
| Statistic disagrees with a stored `fastchess` result | Root-cause before shipping — one of the two is wrong and it may be ours |
| OS cannot support a capability (e.g. hard macOS affinity) | Record the advisory/off fallback in the run record and PLAN; fail only if the unavailable capability was explicitly requested |
| Feature exists in fastchess but not here | Foundational controls are decided in Phase 2; Phase 8.2 decides remaining gaps |
| Tempted to add engine-specific logic | It belongs in the engine's build/development tooling, not Colosseum |
