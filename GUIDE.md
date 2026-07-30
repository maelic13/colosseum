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
| Branch / version | `main`; Colosseum GUI **1.0.1** released. CLI: **not started** |
| What exists | `colosseum-core` / `-uci` / `-engine` are headless (no `egui` dependency), cross-platform, released for Windows/Linux/macOS incl. arm64, ~136 tests. `core/stats.rs` already has trinomial SPRT, Elo ± error, LOS, ML ratings |
| What is missing | pentanomial/nElo, CPU affinity, the CLI itself, SPSA, provenance/guards, NPS protocol |
| First consumers | **Rarog** (`D:/code/rarog`, Rust) and **Basilisk** (`D:/code/basilisk`, C++) — both currently on ~3,400 lines of Windows-only PowerShell driving `fastchess`, plus a separately-patched `weather-factory` each |
| Platform status | Windows ☐ · Linux ☐ · macOS ☐ — *none supported until it passes the full test suite **and** a null calibration there* |
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
     5. NEVER renumber existing items — commits reference them. To insert
        before the first item use a .0.
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

### Phase 2 — CPU topology, affinity, calibration

- [ ] 2.1 Physical-core/sibling detection per OS — Windows
      `GetLogicalProcessorInformationEx`, Linux `thread_siblings_list`, macOS
      `sysctl`. ⚠ Never infer siblings from logical CPU numbering
- [ ] 2.2 Slot allocation: one logical CPU per physical core, configurable
      headroom (default 2 free), **`Threads` cores per slot, not one**
- [ ] 2.3 Hard failure when pinning is requested and unavailable — never
      silently continue unpinned
- [ ] 2.4 macOS decision: advisory hints vs refusing clock gates. Measure, then
      record the answer in PLAN §5.2
- [ ] 2.5 Topology fixture tests (Zen 3 16c/32t, P/E-core, no-SMT, 2-socket)
      and a residency integration test, skipped with a message where the OS
      cannot enforce it
- [ ] 2.6 `calibrate` command — byte-identical binaries enforced by SHA-256,
      fixed 30k games, PASS iff the full 95% nElo CI is inside ±5, invalid on
      any timeout/crash/illegal move
- [ ] 2.7 **EXIT** — calibration passes on Windows; a deliberately-broken
      (affinity-disabled) build is shown to FAIL it

### Phase 3 — CLI skeleton and the engine descriptor

- [ ] 3.1 New `colosseum-cli` workspace member with a `[[bin]]`; argument
      parsing; `--version`/`--help`
- [ ] 3.2 Engine descriptor TOML (PLAN §S4) — load, validate, clear errors on
      malformed input
- [ ] 3.3 `fingerprint` and `build` passthrough via the descriptor
- [ ] 3.4 Run manifests (PLAN §5.8) written for every run, including aborted
      ones
- [ ] 3.5 **EXIT** — `colosseum-cli fingerprint` reproduces Rarog's `bench 13`
      node total from a descriptor, on all three OSes

### Phase 4 — SPRT gate

- [ ] 4.1 `sprt` command: modes `gainer` `[0,3]`, `simplify` `[-3,0]`, `fixed`,
      `calibrate`; per-side UCI options and per-side `Threads`
- [ ] 4.2 Doctrine defaults: `tc=3+0.03`, hash 64, `Threads=1`, UHO book, both
      colours per opening, resign `movecount=3 score=600 twosided=true`, draw
      `movenumber=40 movecount=8 score=10`
- [ ] 4.3 Compiler-equality guard — HARD-fail on mismatch, warn on a missing
      manifest
- [ ] 4.4 Live report block, full log to disk, PGN out, resume without
      double-counting
- [ ] 4.5 **EXIT** — replaying a stored Rarog gate reaches the same verdict at
      the same game number (±1 report interval); guard tests pass

### Phase 5 — SPSA

- [ ] 5.1 End-state schedule in `core`: knobs declare `c_end`, run declares
      `r_end` and horizon `N`; back-solve `c`, `a`, `A = 0.1N`; decay per
      ITERATION
- [ ] 5.2 Written-artifact assertion — read the persisted schedule back and
      verify before a single game is played
- [ ] 5.3 Persistent driver: book loaded once, no per-iteration process
      relaunch
- [ ] 5.4 Multi-session: periodic state save, log APPENDS on resume, stops
      itself at the horizon, prints iteration/percent/ETA, says out loud that
      the horizon is frozen at first launch
- [ ] 5.5 Tail-mean output for baking — the whole vector, no per-knob filter
- [ ] 5.6 Config audit, 7 classes; refuse to launch on class 5 (pinned/discrete
      knob) and class 6 (perturbation rounds to zero before the horizon)
- [ ] 5.7 **EXIT** — schedule property tests (`c_t==c_end`, `a_t==a_end` at
      `t=N`; `r_end` invariant across N); both hard audit classes rejected by
      fixtures; kill/resume continues the schedule; synthetic noisy-quadratic
      run lands in a stated RMSE band

### Phase 6 — Speed/NPS and book tools

- [ ] 6.1 `nps` A/B: strict alternation, arm-level median and best-of,
      bootstrap CI, ≥2 builds pooled per arm with per-build medians shown
- [ ] 6.2 Refuse a verdict until a self-pair has been recorded for this machine
      and configuration; warn if it read outside ±0.5%
- [ ] 6.3 `book slice` / `hash` / `stats` / `verify`
- [ ] 6.4 **EXIT** — injected left-skewed sample reproduces the known bias in a
      naive ABBA estimator and NOT in the shipped one; a self pair reads within
      ±0.5%; slicing is byte-reproducible across platforms

### Phase 7 — Gauntlet CLI surface

- [ ] 7.1 `gauntlet`: opponent ladder, joint ML ratings with error bars, one
      anchor, standings/crosstable CSV
- [ ] 7.2 **EXIT** — CLI ratings match the GUI to ≤0.01 Elo on stored data

### Phase 8 — Parity vs fastchess/cutechess, and the feature-gap decision

- [ ] 8.1 **Parity run — the entry gate for trusting this harness.** Same two
      binaries, book, seed, TC and adjudication through `colosseum-cli`,
      `fastchess` and `cutechess-cli`. Compare Elo/nElo/LOS, the pentanomial
      vector, draw rate and time-loss counts. Agreement inside the combined
      error bars is the gate; disagreement is a defect in one of the three and
      must be root-caused. ⚠ Not optional — this is the mitigation for losing
      runner independence, and it repeats after any change to game-running code
- [ ] 8.2 Feature-gap enumeration and decision, per feature: adopt / decline
      with a reason / defer. Candidates to assess **at that time, not now**:
      SPRT reporting cadence, `-recover` semantics, per-engine time margins,
      Chess960, ponder under test conditions, missing tournament formats,
      output formats other tools consume
- [ ] 8.3 **EXIT** — parity demonstrated and every gap has a recorded decision

### Phase 9 — Release preparation for `colosseum-cli`

- [ ] 9.1 Ship as its own binary asset, versioned and released separately from
      the desktop app
- [ ] 9.2 Windows/Linux/macOS × x64/arm64 through the existing pipeline;
      smoke-test the exact published artifacts
- [ ] 9.3 User docs: quickstart, engine-descriptor reference, a worked example
      per command, and a "how to trust a result" page (calibration, provenance,
      self-pair validation). No phase numbers, no internal naming
- [ ] 9.4 Reference descriptors for Rarog and Basilisk as worked examples
- [ ] 9.5 **EXIT / ACCEPTANCE** — Rarog and Basilisk each run one real gate
      end-to-end through the released artifact on ≥2 operating systems, each
      with a passing null calibration, agreeing with 8.1

## Recurring procedures

Not steps — they are never "done".

### Declaring a platform supported

- [ ] Full test suite green there, debug **and** release.
- [ ] A null calibration passed there (30k games, full 95% nElo CI inside ±5).
- [ ] Record platform, date, machine and the calibration CI in the checkpoint
      table above.
- [ ] Re-run the calibration after ANY change to game-running or placement
      code. A harness change with no fresh calibration invalidates every result
      taken after it.

### After changing anything that runs games

- [ ] Re-run the null calibration (above).
- [ ] Re-run the Phase 8.1 parity check against `fastchess`.
- [ ] Bump the harness version recorded in run manifests, so results taken
      before and after are distinguishable forever.

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

Long game jobs (calibrations, parity runs, real gates) are run by the user on
the dev machine and pasted back; everything else is verified locally by tests.

## Decision rules

| Situation | Action |
|---|---|
| A phase "works" but its exit criterion is not demonstrated | Not done. Do not proceed |
| Statistic disagrees with a stored `fastchess` result | Root-cause before shipping — one of the two is wrong and it may be ours |
| OS cannot support a requirement (e.g. macOS affinity) | Record the limitation in the manifest and in PLAN; never silently degrade |
| Feature exists in fastchess but not here | Phase 8.2 decides — adopt, decline with a reason, or defer. Never drift into it ad hoc |
| Tempted to add engine-specific logic | It belongs in the engine descriptor, or in the engine's repo |
