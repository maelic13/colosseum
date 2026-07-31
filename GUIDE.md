# Colosseum CLI — development guide

The short operational view: where the harness stands and what to do next.
Rationale, specifications, success criteria and evidence live in
[`PLAN.md`](PLAN.md).

**This file and `PLAN.md` are the maintainer-facing pair.** `README.md` is the
user-facing front door for the whole project (what Colosseum is, the GUI, the
CLI); the user documentation is user-facing CLI detail. Neither may carry phase
numbers, internal naming or method argumentation.

## Current checkpoint

| | |
|---|---|
| Branch / version | `cli`; Colosseum GUI **1.0.2** released. CLI: **not started** |
| What exists | `colosseum-core` / `-uci` / `-engine` are headless (no `egui` dependency), cross-platform, released for Windows/Linux/macOS incl. arm64, 149 passing tests. `core/stats.rs` has trinomial SPRT, Elo ± error, LOS, ML ratings |
| What is missing | pentanomial/nElo, CPU affinity, the CLI itself, SPSA, durable runs, run records, NPS protocol |
| Validation engines | **Rarog** (Rust) and **Basilisk** (C++) — available, active, different languages and build systems. Any two UCI engines would serve; nothing depends on these |
| Platform status | Windows ☐ · Linux ☐ · macOS ☐ — support requires the full debug/release suite and documented capability fallbacks; calibration is optional and machine-specific |
| Next step | **Phase 0 — naming and release model**, then Phase 1 |

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
            - [x] 8.2 **[DEFERRED]** Chess960 — ...
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

### Phase 0 — Project identity and release model

- [ ] 0.1 **Naming.** A separate "Coliseum" chess application already exists in
      a similar space, creating ambiguity in search, package registries and
      conversation. Evaluate: keep the product name with a distinct CLI binary
      name / rename the CLI only / rename the product. Criteria: search
      collision, registry availability on the targeted package managers,
      trademark risk, survives being said aloud in a bug report, and the cost of
      renaming a released 1.0 product. Record the decision and rejected options
- [ ] 0.2 **Release model.** One repo with two independently versioned release
      tracks, or split the CLI into its own repo consuming the core crates.
      Criteria: can a CLI change destabilise a GUI release, CI complexity, how
      the shared core is versioned and tested, contributor friction, how a core
      fix reaches both. Sketch the pipeline for the chosen model
- [ ] 0.3 **EXIT** — both decisions recorded in PLAN with reasoning

### Phase 1 — Pentanomial statistics and nElo (`colosseum-core`)

- [ ] 1.1 Pair-level scoring into the pentanomial vector, with a documented rule
      for odd/unpaired tails
- [ ] 1.2 Pentanomial variance, normalized Elo, logistic Elo ± error, LOS, draw
      ratio, pairs ratio, WL/DD ratio
- [ ] 1.3 SPRT over **both** the pentanomial/normalized and logistic models,
      selectable, always naming the model in force
- [ ] 1.4 Minimum detectable effect for a design, so nulls report "no effect
      larger than X"
- [ ] 1.5 Typed errors for degenerate inputs — no NaN/Inf escapes
- [ ] 1.6 Property tests: LLR zero at N=0, monotone in score, arm-swap symmetry,
      bounds equal `log(β/(1−α))` and `log((1−β)/α)`
- [ ] 1.7 **Fixture corpus**: a documented generator that plays any two UCI
      engines through **both** `fastchess` and `cutechess-cli`; commit the logs
      with tool name, tool version and exact command line; anonymise engine
      names. Plus analytic fixtures with hand-derived expected values
- [ ] 1.8 CI check that **no test reads a path outside the repository**
- [ ] 1.9 **EXIT** — analytic fixtures pass; golden-file parity against both
      oracles to ≤1e-6 on continuous values and exactly on integer vectors

### Phase 2 — CLI skeleton, UCI invocation, run records, durable runs

- [ ] 2.1 New `colosseum-cli` workspace member with a `[[bin]]`; argument
      parsing; `--version`/`--help`
- [ ] 2.2 Direct engine controls: executable, optional display name, arguments,
      working directory, arbitrary UCI options, allocated cores
- [ ] 2.3 Profiles and resolution order: built-in defaults < `--profile` < run
      file < CLI. Ship `default` and `strict`; users may define their own
- [ ] 2.4 `engine inspect`, and `engine check` as a **compliance report** with
      per-requirement pass/fail and a meaningful exit code
- [ ] 2.5 `--dry-run`: print the resolved configuration and exact engine
      invocations without playing a game
- [ ] 2.6 Run directory layout (`--dir`, portable mode); run records with host
      summary, `schema_version` and `stats_version`, written for every run
      including aborted ones
- [ ] 2.7 **Durable-run contract** (PLAN §5.11): resume by default, explicit
      restart that archives, refusal on config mismatch, append-only logs,
      stored schedule wins with a printed notice, atomic periodic state writes
- [ ] 2.8 **EXIT** — two arbitrary UCI executables pass path-only
      inspect/check; run file plus overrides resolves byte-identically to
      all-CLI; the durable-run suite passes against a stub command

### Phase 3 — CPU topology and affinity

- [ ] 3.1 Physical-core/sibling detection per OS — Windows
      `GetLogicalProcessorInformationEx`, Linux `thread_siblings_list`, macOS
      `sysctl`. ⚠ Never infer siblings from logical CPU numbering
- [ ] 3.2 Modes `auto` / `off` / explicit CPU list; configurable headroom
      (default 2 physical cores free)
- [ ] 3.3 Allocate the configured `cores-per-engine` per game slot, independent
      of whichever UCI option controls the engine's worker count
- [ ] 3.4 Fail when requested placement is unavailable; allow and record `off`;
      report macOS as advisory or unavailable without prohibiting clock matches
- [ ] 3.5 `capabilities` command printing what this platform can and cannot do
- [ ] 3.6 **EXIT** — topology fixtures (SMT 16c/32t, P/E-core, no-SMT,
      2-socket) pass; residency tests pass where enforceable; capability
      reporting documented

### Phase 4 — Fixed match, SPRT, calibration

- [ ] 4.1 `match` fixed-N; `sprt` with explicit bounds/error rates/model;
      `gainer` and `simplify` as named bundles, not hard-coded semantics
- [ ] 4.2 Time controls **per side independently**: movetime, sudden death,
      base+increment, fixed nodes, fixed depth, plus a configurable time margin
      so scheduler jitter is not counted as a loss on time
- [ ] 4.3 Adjudication: draw, resign and max-moves individually configurable and
      individually **disableable**; optional tablebase adjudication when
      tablebase paths are supplied
- [ ] 4.4 **Time-loss accounting first class** — losses on time, crashes,
      disconnects and illegal moves counted per engine, printed in every report
      block, stored in the run record; `--max-time-losses N` flags or aborts
- [ ] 4.5 Explicit concurrency: parallel games, interaction with
      cores-per-engine and headroom, startup memory estimate
      (concurrency × 2 × hash), warn or refuse when the request exceeds
      available cores or memory
- [ ] 4.6 Optional book: order (sequential / seeded random), start index, ply
      depth, and **reuse detection** reporting the fraction of openings played
      more than once. Without a book, start from the initial position and warn
- [ ] 4.7 Explicit configurable engine-crash policy (abandon and count /
      discard / retry once), recorded in the run record — never implicit
- [ ] 4.8 Live report block on a configurable interval; full log; PGN out;
      per-game engine output retained for failed games
- [ ] 4.9 **Machine-readable results**: JSON to file or stdout, and exit codes
      distinguishing H1 / H0 / still-running / invalid / error
- [ ] 4.10 `calibrate`: byte-identical binaries enforced; configurable N,
      confidence and tolerance (defaults 30k / 95% / ±5 nElo); classify PASS /
      FAIL / inconclusive / invalid; never required before another command
- [ ] 4.11 **EXIT** — verdict parity against both oracles at the same game
      number (±1 report interval); forfeit-injection and exit-code tests;
      path-only and no-book runs; every calibration outcome tested

### Phase 5 — SPSA

- [ ] 5.1 End-state schedule in `core`: knobs declare `c_end`, run declares
      `r_end` and horizon `N`; back-solve `c`, `a`, `A = 0.1N`; decay per
      ITERATION
- [ ] 5.2 Assert the persisted schedule before the first game is played
- [ ] 5.3 Tune TOML selects numeric UCI options with initial value, bounds and
      `c_end`; validated against the live UCI option schema
- [ ] 5.4 Defaults 5,000 iterations and 32 games/iteration — configurable,
      neither enforced as a minimum
- [ ] 5.5 Persistent driver: no per-iteration relaunch; a supplied book loaded
      once; multi-session per the durable-run contract
- [ ] 5.6 Config audit: reject absent/non-spin options, duplicates, bounds
      outside the engine's range, `min>=max`, and perturbations rounding to
      zero; warn on default disagreement and a seed on a rail
- [ ] 5.7 **Close the loop** — emit the tail-mean vector as a `setoption` list,
      JSON and a run-file fragment; `sprt --apply <result.json>` gates the tuned
      values via UCI options with no source edit or rebuild
- [ ] 5.8 `spsa plan` — offline sizing (no games): given horizon, knob count,
      mini-match size, per-iteration cost and a noise model, report expected
      convergence and wall clock. The unit of error here is machine-nights
- [ ] 5.9 `spsa status` — read a run directory without disturbing it: iteration,
      ETA, per-knob trajectory, and a **thirds comparison** rather than
      eyeballed single iterations. Flag knobs pinned at a bound, knobs returned
      to seed (a result, not a failure), and knobs whose perturbation has
      decayed below the engine's rounding resolution
- [ ] 5.10 **EXIT** — schedule property tests; every hard audit class rejected
      by a fixture; kill/resume continues the schedule; synthetic
      noisy-quadratic run lands in a stated RMSE band; a stub tune feeds
      `sprt --apply` unedited; `plan`'s predicted band contains an actual run's
      observed RMSE; `status` matches hand-computed values on a fixture

### Phase 6 — Speed/NPS, book tools, statistics replay

- [ ] 6.1 `nps` drives an optional position suite through bounded searches, or
      warns and uses the initial position; strict alternation, warm-up,
      arm-level median/best-of, bootstrap CI, per-round SD
- [ ] 6.2 One or more executables per arm with per-executable medians; self pair
      recommended, optional, warned outside a configurable ±0.5%
- [ ] 6.3 Scaling sweep across a list of worker counts, with the position set
      pinned and recorded so a later sweep is comparable
- [ ] 6.4 `book slice` / `hash` / `stats` / `verify`
- [ ] 6.5 `stats` replays Colosseum runs, PGN and supported external result logs
      through the same reporting code used live. **The durable artifact is
      authoritative, not the console** — a buffered or truncated log is not
      evidence about a run
- [ ] 6.6 PGN search telemetry: per-engine mean/median depth, time per move and
      implied nps where annotations exist; a clear "unavailable" where they do
      not
- [ ] 6.7 **EXIT** — injected left-skewed sample reproduces the known bias in a
      naive alternating-pair estimator and NOT in the shipped one; slicing is
      byte-reproducible; every golden fixture replays through the CLI; telemetry
      matches hand-computed values on a fixture PGN
- [ ] 6.8 ⚖ **Scope decision — `datagen`** (PLAN §5.12). Self-play/engine-vs-
      engine games at a fixed node or depth limit, appending PGN, with the same
      placement and durability guarantees as any other long run. Generating
      games is in scope; extraction, filtering and labelling stay with the
      trainer. Decide adopt or decline, and record it

### Phase 7 — Gauntlet

- [ ] 7.1 `gauntlet`: opponent ladder, joint ML ratings with error bars,
      optional anchor, standings/crosstable CSV
- [ ] 7.2 Resume per the durable-run contract — gauntlets are the longest runs
      the tool performs
- [ ] 7.3 **EXIT** — ratings match the GUI to ≤0.01 Elo on stored data;
      kill/resume produces identical standings

### Phase 8 — Parity against external runners, and remaining gaps

- [ ] 8.1 **Parity run — the entry gate for trusting this harness.** One
      deterministic opening sequence, same two binaries, TC and adjudication,
      through `colosseum-cli`, `fastchess` and `cutechess-cli`. Compare
      Elo/nElo/LOS, pentanomial vector, draw rate, time-loss counts. Agreement
      inside combined error bars is the gate; disagreement must be root-caused.
      Repeat after any change to game-running code
- [ ] 8.2 Remaining feature gaps: adopt / decline with a reason / defer.
      Candidates: Chess960, ponder under test conditions, additional tournament
      formats, output formats other tools consume. **Tie-breaker is whether a
      general engine developer needs it**, not whether the validation engines do
- [ ] 8.3 **EXIT** — parity demonstrated and every gap has a recorded decision

### Phase 9 — Documentation and release

- [ ] 9.1 **Documentation placement analysis**: in-repo `docs/` published as a
      static site, GitHub wiki, or generated reference plus guides. Criteria:
      versioning with the binary (a wiki does not version, which matters once
      `stats_version` exists), discoverability, offline availability,
      contribution friction, and whether the command reference can be generated
      from the argument parser so it cannot drift. Record the decision
- [ ] 9.2 README as the project front door — what Colosseum is, the GUI, the
      CLI, install, links
- [ ] 9.3 User documentation: quickstart, command reference, run-file and
      tune-file reference, a worked example per command, "how to trust a result"
      from PLAN §S3 Tier C, and a compatibility page (what the tool needs from a
      UCI engine, what it does with non-conforming ones). Note that the licence
      covers the tool, not engines run as separate processes
- [ ] 9.4 Ship per Phase 0.2's release model; all supported platforms;
      smoke-test the exact published artifacts (`--version`, `--help`,
      `engine check` against the shipped stub, one stub match)
- [ ] 9.5 **Coverage acceptance** (PLAN §5.13) — both validation engines delete
      every harness script the CLI claims to replace and keep only the residual
      list (build, profiling, engine-specific correctness and diagnostics,
      training-data extraction). Any exception is recorded as a named gap
- [ ] 9.6 **EXIT / ACCEPTANCE** — a **third-party engine pair the maintainers
      did not write**, driven by someone following only the published docs,
      completes a fixed match, an SPRT and a short SPSA. Plus both validation
      engines running one real gate through the released artifact on ≥2
      operating systems, agreeing with 8.1

## Recurring procedures

Not steps — they are never "done".

### Declaring a platform supported

- Full test suite green there, debug **and** release.
- Affinity, process, timer and filesystem capabilities or fallbacks implemented,
  tested and documented there.
- The exact released artifact passes `--version`, `--help`, `engine check` and
  one stub-engine match.

### After changing anything that runs games

- Re-run the Phase 8.1 parity check against both external runners.
- Consider a real-machine calibration after material clock, scheduling or
  affinity changes; it is evidence, not a release or usage prerequisite.
- Bump the harness version in run records, and `stats_version` if any reported
  statistic changed definition — with a changelog entry.

## What to do now

**Phase 0 — naming and the release model.** Both are cheap to decide and
expensive to change later: the name reaches the binary, the docs, the package
registries and every link; the release model shapes CI and whether CLI churn can
destabilise a shipped GUI.

Then **Phase 1 — pentanomial statistics** in `colosseum-core`, which needs no
machine time and no platform surface.

```
cargo test -p colosseum-core
```

## Working rhythm

```text
Pick the next unchecked step  ->  implement + test  ->  demonstrate its exit
criterion  ->  tick it here AND in PLAN.md in the same commit.
```

Long game jobs (optional calibrations, parity runs, real gates) run on a real
machine and are pasted back; everything else is verified locally by tests.

## Decision rules

| Situation | Action |
|---|---|
| A phase "works" but its exit criterion is not demonstrated | Not done. Do not proceed |
| Our statistic disagrees with an external oracle | Root-cause before shipping — one of them is wrong and it may be ours |
| The two external oracles disagree with each other | That is a finding: record it, and prefer the analytic fixture |
| OS cannot support a capability (e.g. hard macOS affinity) | Record the advisory/off fallback in the run record and PLAN; fail only if the capability was explicitly requested |
| Tempted to make one of our defaults mandatory | It belongs in PLAN §S3 Tier B with a reason, or Tier C as advice — Tier A needs a silent-wrong-number failure mode |
| Feature exists in an external runner but not here | Phase 8.2 decides, with "does a general engine developer need it?" as the tie-breaker |
| Tempted to add engine-specific logic | It belongs in the engine's own tooling, not here |
