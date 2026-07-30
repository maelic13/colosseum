# Colosseum CLI — engine-development harness plan

`colosseum-cli` is a headless, cross-platform harness for **developing** chess
engines: SPRT gates, SPSA tuning, gauntlets, speed measurement, and the
provenance/calibration machinery that makes those numbers trustworthy.

The desktop app answers *"who is stronger?"*. The CLI answers *"is this commit
stronger than that commit, and can I believe the answer?"* — a different
question with much harsher requirements: reproducibility, explicit CPU
placement, byte-level provenance, and a calibration path that proves the
harness itself is not lying.

**Audience of this file:** the maintainer. It carries the specifications,
success criteria, the evidence behind each requirement, and the forward plan.
[`GUIDE.md`](GUIDE.md) is the short operational view — what to do next.
`README.md` and `CHANGELOG.md` stay user-facing and must not carry method,
phase numbers or internal naming.

---

## S1. Current state

**Nothing is built yet. This document is step 0.**

What exists is the reason the plan is short: `colosseum-core`,
`colosseum-uci` and `colosseum-engine` are already headless (no `egui`
dependency anywhere in them), already cross-platform, already released for
Windows/Linux/macOS including arm64, and already carry ~136 tests. The CLI is a
new workspace member consuming them — not a refactor.

First consumers: **Rarog** (`D:/code/rarog`, Rust) and **Basilisk**
(`D:/code/basilisk`, C++). Both currently drive `fastchess` through ~3,400
lines of PowerShell that runs only on Windows, and both maintain their own
patched copy of `weather-factory` for SPSA — including the *same* upstream bug,
found and fixed twice independently.

---

## S2. Why this lives in Colosseum

A separate Python project was proposed first and **rejected after looking at
the code**. That analysis assumed the game-playing layer would be built from
scratch; it would not be. Recorded so the decision is not re-litigated:

| Already in Colosseum | Would have been rebuilt in Python |
|---|---|
| SPRT (LLR, bounds, H0/H1) + tests, `core/stats.rs` | ~200 lines + validation |
| Elo ± error, LOS, Ordo-style joint ML ratings | ~400 lines |
| UCI handshake, option auto-detect, quirky-engine option mapping | ~1,100 lines, and the quirks are only learned by running Rybka/Junior |
| Tournament scheduler, pairing, parallel games | ~1,000 lines |
| Adjudication (draw/resign/max-moves) | ~150 lines |
| Books EPD **and** PGN, seeded random, both colours per opening | ~200 lines |
| TCs: movetime, sudden death, base+inc, **fixed nodes**, fixed depth | ~200 lines |
| PGN/CSV export, SQLite persistence, **resume after crash**, incident reports | ~1,500 lines |
| **Windows + Linux + macOS builds and a release pipeline, incl. arm64** | the entire original motivation |

Three further points that only became clear from the source:

- **Distribution flips to Rust.** A single static binary with no runtime and no
  package manager is a better deliverable for a tool shared by a Rust engine and
  a C++ engine than a `pip install`, and better again if it is ever published.
- **The statistics are not a scipy-scale problem.** LLR, pentanomial variance,
  nElo and bootstrap CIs are a few hundred lines of arithmetic. `stats.rs`
  already hand-rolls `erf`/`normal_cdf`.
- **The per-iteration process tax disappears structurally.** The SPSA driver
  relaunches `fastchess` once per iteration: measured ~4 s fixed overhead on top
  of ~0.77 s/game, i.e. ~14% of a 40-hour tune (~5.5 h), most of it re-parsing a
  **167 MB / 2,632,036-position** opening book *to use 16 openings*. Colosseum's
  driver is long-lived and loads the book once, so this cost is not optimised —
  it does not exist. (Per-*game* engine spawn stays, deliberately: measured
  17–350 ms against ~34 s games, and it is what buys crash isolation and
  per-game forensics. See `CLAUDE.md`.)

### The one real cost, accepted with mitigation

**Runner independence is lost.** Today `fastchess` (gates) and Colosseum
(gauntlets) are independent implementations, and Rarog's 9.8 boundary gauntlet
exploited exactly that: self-play SPRT predicted ~+60 Elo, Colosseum returned
+76 ± 21 and +78 ± 28 in two independent conditions. That agreement is what
established the gains transfer. If the gate and the gauntlet both run on this
code, a shared bug is invisible in both at once — and the record already shows
harnesses mis-measure in ways engines cannot see.

**Mitigation is a hard entry condition, not a nice-to-have** — see the parity
gate in S8 Phase 8, and the standing rule that `fastchess` stays installed as a
second opinion.

---

## S3. Measurement doctrine

Every rule here was paid for. They are requirements on the tools, not advice.

1. **SPRT is the only verdict.** Holdout MSE, bench nodes, EBF, NPS and depth
   are diagnostics. A −4.9% holdout improvement once lost 17 Elo.
2. **A node count is a fingerprint, not a metric.** Identical ⇒
   behaviour-preserving. Magnitude comparisons across parameter changes are
   meaningless (±1 threshold changes swing it several %).
3. **`[-3,+3]` is not an equivalence test.** Non-inferiority is `[-3,0]`;
   equivalence is a fixed-N calibration with a CI containment rule.
4. **A null is not proof of no effect.** Report the resolution limit alongside
   any null. A 3,000-game-per-arm paired difference resolves ±13.8 Elo; calling
   that "no effect" when hunting a ~16 Elo effect is wrong.
5. **Explicit CPU placement is mandatory for every clock match.** Unpinned Zen 3
   showed a hidden per-run offset of ~±10 nElo. `fastchess` before 1.7.0 failed
   to apply Windows affinity at all; 1.8.0 guesses SMT siblings from logical CPU
   order and pins **one core per game**, which starves `Threads>1` (a 4-thread
   comparison read −100 purely from starvation).
6. **Compiler equality.** A toolchain change between building A and B folds the
   compiler delta into the measured Elo, and **no null pair can see it** — a null
   runs one binary against itself, so both sides always share a compiler. Three
   unrelated Rarog gates clustered at −8.68/−8.22/−7.37 across the boundary.
   Hard-fail, never warn.
7. **Provenance travels with the result.** Binary SHA-256, fingerprint, compiler
   version, dirty-tree flag, harness version, book hash, seed, affinity list.
8. **The measuring instrument must be validated before it is trusted.** Two NPS
   estimators each read −0.2…−0.4% on a *self pair* (the same binary in both
   arms) and had already produced two confident false rejections. Bench NPS is
   left-skewed, so any estimator weighting the arms unequally against the slow
   tail manufactures bias. Validate on a self pair; compare arm-level **median**
   and **best-of**; pool ≥2 builds per arm (two PGO builds of identical source
   differ ~0.36%).
9. **Unified conditions.** Tune and gate share TC, book *and* adjudication.
   A tuner optimising under different game-termination rules than the gate
   measures is optimising the wrong objective.
10. **Two-sided resignation.** `-resign movecount=3 score=600 twosided=true`.
    Fishtest uses `score=600` with no `twosided` (verified in
    `official-stockfish/fishtest`, `worker/games.py`); we match the threshold
    and go one step stricter, because one-sided resignation adjudicates on the
    losing side's own evaluation — and in SPSA both arms are the same binary
    with perturbed parameters, so an arm producing more extreme scores resigns
    more readily than its sibling. That asymmetry lands directly in the
    gradient. Draw rule `movenumber=40 movecount=8 score=10` (stricter than
    fishtest's `34/8/20` on both axes).
11. **SPSA facts, from a calibrated convergence model:** iterations dominate and
    **5,000 is the floor** (at 1,000–2,500 a run barely beats its seed);
    **dimension is ~free** (p=6 and p=26 converge alike — merging groups is
    cheaper *and* more correct where knobs interact); there is a **noise floor**
    independent of the starting point, so re-tuning knobs already inside it
    scatters them; **curvature below ~0.5 Elo per full step is unfittable** at
    32 games/iteration; games-per-iteration is ~neutral at fixed game budget.
12. **Derive schedule constants from the horizon, and assert them at the point
    they are written.** Hand-picking the gain leaves it stale whenever the
    horizon changes. Fishtest's end-state form (`c_end`, `r_end` → back-solve
    `c`, `a`) makes staleness unrepresentable. Two independent correctness
    passes over one schedule both reviewed the *reasoning* and neither looked at
    the *written file*, which is how `A = 0.0965` shipped where `A = 500` was
    needed. **A derived constant that leaves no runtime trace must be asserted
    where it is produced.**
13. **Book choice is a measurement decision.** UHO (unbalanced human openings,
    played from both colours per pair) for SPRT/SPSA: symmetric so unbiased, but
    decisive, cutting draws ~56% → ~35–45% so gates resolve in far fewer games.
    A balanced book for CCRL-comparable gauntlets. **Book exhaustion inflates
    error bars** — one run recycled 23% of its pairs and reported optimistic
    error. For SPSA, opening reuse across iterations is harmless and arguably
    beneficial (common random numbers reduce gradient variance).
14. **Speed → Elo is ≈ 2 Elo per 1% NPS at 3+0.03** (measured on a
    bench-identical gate that isolated execution speed). The older 0.7 figure
    was an LTC estimate and must not be transferred to STC, nor this one to LTC
    unmeasured.
15. **Pentanomial pairs and normalized Elo are the reporting basis.** Paired
    openings are correlated; the pentanomial model is why the error bars are
    honest, and `model=normalized` is what the existing ledgers are denominated
    in.

---

## S4. Architecture

```
colosseum-core      pure math + domain      ← pentanomial stats, nElo, SPSA schedule
colosseum-uci       UCI protocol/process    ← unchanged
colosseum-engine    driver/scheduler/store  ← + CPU affinity, book slicing
colosseum-cli       NEW: bin + commands     ← descriptors, manifests, NPS, SPSA driver
colosseum-gui       desktop app             ← unchanged; may consume new stats
```

**Placement rule:** pure functions go in `core` (so the GUI can reuse them and
so they are trivially testable); anything that spawns or places a process goes
in `engine`; anything that is a *workflow* goes in `cli`.

### The engine descriptor

The single mechanism that makes one tool serve two engines. A per-engine TOML,
living **in the engine's repo**, never in Colosseum:

```toml
# rarog/.colosseum/engine.toml
name    = "Rarog"
version_cmd = ["uci"]            # or read from the binary's id string

[build]
command = ["cargo", "xtask", "build", "--arch", "pext", "--pgo"]
artifact_glob = "target/dist/rarog-*-pext-pgo{exe}"

[fingerprint]                     # the "is this behaviour-identical?" check
command = ["bench", "13"]         # sent on stdin, or argv if stdin_command=false
pattern = "Nodes searched\\s*:\\s*(\\d+)"

[speed]
pattern = "Nodes/second\\s*:\\s*(\\d+)"

[tunables]                        # for SPSA
source  = "spsa/params.json"      # engine exports its own knob table

[conditions]                      # defaults for this engine's gates
tc = "3+0.03"
book = "books/UHO_Lichess_4852_v1.epd"
```

Everything engine-specific is here. Colosseum never learns what `cargo xtask`
or `bench 13` mean. **Success criterion: adding Basilisk requires writing one
TOML and zero lines of Rust.**

### Explicitly out of scope

Anything that belongs to one engine: parameter *baking* into source, build
flavour logic (PGO/PEXT/AVX2), Texel tuning, datagen, NNUE. The CLI may *call*
an engine's build command; it must never contain build knowledge.

---

## S5. Tool specifications and success criteria

Every criterion below must be checkable by a test or a single command. "It
looks right" is not a criterion.

### 5.1 Pentanomial statistics and normalized Elo — `colosseum-core`

**Why first:** everything downstream reports through it, and it is what makes
results comparable with the existing Rarog/Basilisk ledgers. The present
`sprt()` is trinomial over W/D/L and would silently disagree with every number
either project has recorded.

**Requirements**

- Game pairs, not games, are the unit: each opening played from both colours
  yields a pair score in {0, 0.5, 1, 1.5, 2} → the pentanomial vector
  `Ptnml(0-2)`.
- Pentanomial variance; normalized Elo (`nElo`); logistic Elo with error bars;
  LOS; draw ratio; pairs ratio; `WL/DD` ratio.
- SPRT over the pentanomial model with `elo0`/`elo1`/`alpha`/`beta`, reporting
  LLR and both bounds, with H0/H1/continue.
- An unpaired fallback for odd game counts and for gauntlets, clearly labelled.

**Success criteria**

- **Golden-file parity:** replaying stored `fastchess` logs from Rarog's
  `tools/results/` reproduces the reported `Elo ± err`, `nElo ± err`, `LOS`,
  `DrawRatio`, `PairsRatio`, `LLR` and `Ptnml(0-2)` to **≤1e-6** on Elo/LLR and
  exactly on the integer vectors. At least 6 stored runs including one stopped
  mid-flight, one H0, one H1, and one null calibration.
- **Property tests:** LLR is 0 at zero games; monotone in the score for fixed N;
  symmetric under swapping the arms and negating the bounds; the SPRT bounds
  equal `log(β/(1−α))` and `log((1−β)/α)`.
- **Degenerate inputs return a typed error, never NaN:** all draws, zero games,
  one pair, 100% score.

### 5.2 CPU topology and affinity — `colosseum-engine`

**Why:** doctrine 5. This is the single subtlest component in the harness and
the most platform-divergent.

**Requirements**

- Detect *physical* cores and the logical CPUs belonging to each, per OS:
  Windows `GetLogicalProcessorInformationEx`, Linux
  `/sys/devices/system/cpu/*/topology/thread_siblings_list`, macOS
  `sysctl hw.physicalcpu / hw.logicalcpu`. **Never infer SMT siblings from
  logical CPU numbering** — that is the 1.8.0 defect.
- One logical CPU **per physical core**, leaving a configurable headroom
  (default 2 cores free).
- Pin per *game slot*, and a slot must receive **`Threads` cores, not one** —
  the starvation bug. At `Threads=1` this reduces to one core per game.
- Report the resolved placement in the run manifest, and fail loudly if pinning
  is requested and unavailable (never silently continue unpinned).
- macOS caveat: there is no supported hard CPU-affinity API. Either use affinity
  *hints* and mark the run as `affinity=advisory` in the manifest, or refuse
  clock-TC gates on macOS by default. **Decide by measurement in Phase 2, not
  now** — and whichever way it goes, the manifest must say which.

**Success criteria**

- Unit tests over recorded topology fixtures (a Zen 3 16c/32t map, a
  P-core/E-core map, a single-socket no-SMT map, a 2-socket map) assert the
  chosen CPU list.
- An integration test spawns N busy child processes under a pinning request and
  samples actual CPU residency, asserting each lands on its assigned core.
  Skipped with a clear message where the OS cannot enforce it.
- **The real gate is the null calibration (5.3), run per platform.**

### 5.3 Null calibration — `colosseum-cli calibrate`

**Why:** it is the only test that can catch a harness that mis-measures.

**Requirements**

- Byte-identical binary on both sides — **refuse** if the SHA-256 differs.
- Fixed game count (default 30,000), no early stopping.
- PASS iff the **full 95% nElo CI lies inside ±5**. Report inconclusive if the
  interval is wider than the tolerance rather than declaring PASS.
- Any timeout, crash, disconnect or illegal move invalidates the run.
- Must be re-run after **any** harness change, and per platform.

**Success criteria**

- Passes on Windows, Linux and macOS before that platform is declared supported.
- A deliberately-broken build (affinity disabled) is shown to *fail* it on at
  least one machine — a calibration that cannot fail proves nothing.

### 5.4 SPRT gate — `colosseum-cli sprt`

**Requirements**

- Modes: `gainer` `[0,3]`, `simplify` `[-3,0]`, `fixed` (N games, no stopping),
  `calibrate` (5.3).
- Two engines by path, per-side UCI options and per-side `Threads` (so one
  binary can be A/B-tested on a knob without a rebuild).
- Defaults from doctrine: `tc=3+0.03`, hash 64 MB, `Threads=1`, UHO book,
  `-repeat`/both colours per opening, adjudication per doctrine 10.
- **Hard-fail** on compiler mismatch between the two manifests (doctrine 6).
- Live report block on an interval; full log to disk; PGN out; run manifest.
- Resume a stopped gate from the store without double-counting.

**Success criteria**

- Replaying a stored Rarog gate's game outcomes reaches the **same verdict at
  the same game number** as `fastchess` did (±1 report interval).
- Compiler mismatch, SHA-identical-with-identical-options, and missing-manifest
  cases each produce the specified error/warning — asserted by tests.
- Killing the process mid-run and resuming produces the same final statistics as
  an uninterrupted run over the same seed.

### 5.5 SPSA — `colosseum-cli spsa` + `colosseum-core` schedule

**Requirements**

- **End-state parameterization** (doctrine 12): each knob declares `c_end` and
  the run declares `r_end` and horizon `N`; `c`, `a` and `A = 0.1·N` are
  back-solved. `alpha=0.601`, `gamma=0.102`.
- Decay per **iteration**, never per game.
- Persistent driver: no per-iteration process relaunch, book loaded once.
- Multi-session: state saved every K iterations; log **appends** on resume;
  stops itself at the horizon; prints iteration/percent/ETA; the horizon is
  frozen at first launch and the tool must say so out loud.
- Emits the tail-mean vector for baking. **No per-knob filter** — SPSA estimates
  a *joint* optimum, and reverting a subset yields a point the tuner never
  evaluated. The tail mean already is the filter.
- **Config audit**, refusing to launch on the two hard-error classes:
  1. knob declared but in no group *(info)*
  2. group entry not declared by the engine *(error)*
  3. seed disagrees with the engine default *(warn — may be deliberate)*
  4. knob in more than one group *(info)*
  5. **pinned or near-discrete knob inside a tune (ERROR)** — a `min==max` knob
     is identical in both arms of every mini-match, so it is never *measured*,
     yet its value shapes every other knob's fit
  6. **perturbation rounds to zero before the horizon (ERROR)** — the engine
     receives `round(value)`, so once `c_end · c_t < 0.5` both arms see the same
     integer: the knob stops being measured but keeps being *updated*, i.e. it
     random-walks and drags the fit
  7. seed on a rail *(warn — one-sided gradient)*

**Success criteria**

- **Schedule property tests:** for any `N` and any `c_end`/`r_end`,
  `c_t == c_end` and `a_t == a_end` exactly at `t = N`; `r_end` is invariant as
  `N` varies over {1e3, 2.5e3, 5e3, 1e4} while `a` moves.
- **Written-artifact assertion:** the persisted schedule is read back and
  verified (`A == 0.1N`, `A > 0`, `a` matches) before any game is played. A test
  mutates the file and asserts the launch refuses.
- Audit classes 5 and 6 each have a fixture that must be rejected.
- **Recovery test:** kill at iteration K, resume, and the schedule continues
  (does not restart at full gain); the log retains the pre-kill iterations.
- **Convergence smoke test:** against a synthetic noisy quadratic objective with
  a known optimum, a 5,000-iteration run lands within a stated RMSE band —
  guards the optimiser, not the engine.

### 5.6 Speed / NPS A/B — `colosseum-cli nps`

**Requirements**

- Strict alternation of arms; arm-level **median** and **best-of**; bootstrap CI
  on the median.
- Pool ≥2 builds per arm and report per-build medians so non-overlap is visible.
- **Refuse to report a verdict until a self-pair run has been recorded** for the
  same machine and configuration, and warn if it read outside ±0.5%.
- Detect and report machine noise (per-round SD).

**Success criteria**

- A self pair reads within ±0.5% and the tool says so.
- A synthetic left-skewed sample injected into the estimator reproduces the
  known bias in a naive ABBA estimator and *not* in the shipped one — the test
  that would have caught the original defect.

### 5.7 Gauntlet — `colosseum-cli gauntlet`

Mostly a CLI surface over existing core: ladder of opponents, joint ML ratings
with error bars, one anchor, standings/crosstable CSV.

**Success criterion:** on a stored tournament, CLI ratings match the GUI's for
the same data to ≤0.01 Elo.

### 5.8 Provenance and guards — `colosseum-cli`

Manifest per run: both engines' path/SHA-256/fingerprint/compiler/dirty flag,
harness version and git SHA, book path+hash, seed, resolved affinity, TC, hash,
threads, adjudication, full command line, UTC start.

**Success criteria:** a manifest is written for every run including aborted
ones; a test asserts every field is populated or explicitly `null` with a
reason; the compiler guard hard-fails on mismatch.

### 5.9 Book tools — `colosseum-cli book`

`slice`, `hash`, `stats` (count, ply depth, eval band if present), `verify`
(every position legal and parseable).

**Success criteria:** slicing is deterministic given a seed and reproducible
across platforms (same hash); `verify` rejects a known-bad EPD fixture.

---

## S6. Testing requirements — binding

The harness produces numbers that decide weeks of work. **A harness bug is
worse than an engine bug, because it is invisible in exactly the measurement
meant to catch it.** These are requirements, not aspirations.

1. **Every pure function is unit-tested**, including degenerate inputs. No
   statistical function may return NaN/Inf without a typed error.
2. **Golden-file parity tests** against stored real runs from Rarog (and
   Basilisk where available) for every reported statistic. These are the tests
   that keep the ledgers comparable, and they must live in the repo with the
   fixtures.
3. **Property-based tests** for the statistics and the SPSA schedule
   (invariants in S5.1 and S5.5).
4. **Integration tests with stub engines.** The existing scheduler tests already
   stub engines with `cmd /c`; extend to a small cross-platform stub engine
   (a tiny UCI responder shipped as a test binary) so the same tests run on
   Linux and macOS. **Required — the current stubs are Windows-only.**
5. **Fault-injection tests:** engine crashes at handshake / mid-search / on
   quit; engine times out; engine emits an illegal move; engine never responds
   to `isready`. Each must produce a specific, tested outcome, and none may be
   silently absorbed into a result.
6. **Determinism tests:** same seed + same stubs ⇒ same pairings, same opening
   order, same final statistics, on every platform.
7. **Resume/kill tests** for both SPRT and SPSA.
8. **The calibration gate (5.3) is part of acceptance**, not part of testing —
   CI cannot run 30,000 games, so it is a documented manual gate per platform,
   recorded in GUIDE.
9. **CI matrix: Windows, Linux, macOS × debug and release.** Debug is not
   optional: a debug build searches ~an order of magnitude slower and a CI
   runner is slower again, which is exactly how a flat-timeout test passes
   locally for months and fails on the runner.
10. **No new `clippy` warnings**; workspace lint wall stays at zero.

---

## S7. Multi-platform requirements

Windows, Linux, macOS — all three first-class, x64 and arm64 where the release
pipeline already builds them.

Concrete divergences to handle explicitly, each with a test or a documented
fallback: CPU topology and affinity (5.2, and macOS may be advisory-only);
executable suffix and path separators; process spawn/kill semantics; symlink and
permission handling for engine binaries; high-resolution timing; file locking on
the SQLite store; line endings in PGN/EPD parsing.

**Rule: no platform is "supported" until it has passed a null calibration and
the full test suite there.** Platform status is tracked in GUIDE, per platform,
with the date and the CI of the calibration run.

---

## S8. Implementation plan

Ordered by dependency and by risk. Numbers are frozen once written — later
inserts use `.0`/letter sub-items, as Rarog does.

### Phase 1 — Pentanomial statistics and nElo (`core`)
Spec 5.1. First because every later phase reports through it and because it is
what makes results comparable with the existing ledgers. Pure functions, no I/O,
fully testable offline. **Exit:** golden-file parity against ≥6 stored runs.

### Phase 2 — CPU topology, affinity, and the calibration command
Specs 5.2 + 5.3. The highest-risk component, done early so its unknowns
(especially macOS) surface before anything depends on them. **Exit:** topology
fixtures pass on all three OSes; a null calibration passes on Windows; the macOS
affinity decision is recorded in this file.

### Phase 3 — `colosseum-cli` skeleton and the engine descriptor
New workspace member, argument parsing, descriptor loading, manifests (5.8),
`fingerprint` and `build` passthrough. **Exit:** `colosseum-cli fingerprint`
reproduces Rarog's `bench 13` node total from a descriptor, on all three OSes.

### Phase 4 — SPRT gate
Spec 5.4, including the compiler-equality guard and resume. **Exit:** verdict
replay parity against a stored Rarog gate; guard tests pass.

### Phase 5 — SPSA
Spec 5.5: schedule in `core`, driver in `cli`, audit, multi-session, written-
artifact assertion. **Exit:** schedule property tests, both hard audit classes
rejected, recovery test, synthetic convergence smoke test.

### Phase 6 — Speed/NPS and book tools
Specs 5.6 + 5.9. **Exit:** the skew-bias regression test passes; self-pair
enforcement demonstrated.

### Phase 7 — Gauntlet CLI surface
Spec 5.7. Thin. **Exit:** ratings match the GUI to ≤0.01 Elo on stored data.

### Phase 8 — Parity check vs fastchess and cutechess, and the feature-gap decision
**This is development work, not analysis to do now.** Two parts:

- **(a) Parity run — the entry gate for trusting this harness.** Same two
  binaries, same book, same seed, same TC, same adjudication, run through
  `colosseum-cli`, `fastchess` and `cutechess-cli`. Compare final Elo/nElo/LOS,
  the pentanomial vector, draw rate, and time-loss counts. **Agreement within
  the combined error bars is the gate**; disagreement is a defect in one of the
  three and must be root-caused before either is trusted. This is the mitigation
  for the loss of runner independence (S2), so it is not optional and it is
  repeated after any change to game-running code.
- **(b) Feature-gap decision.** Enumerate what `fastchess`/`cutechess` do that
  Colosseum does not, and decide per feature: adopt, decline with a reason, or
  defer. Known candidates to evaluate at that time: SPRT-during-tournament
  reporting cadence, `-recover` semantics, per-engine time margins, Chess960,
  ponder handling under test conditions, tournament formats we lack, output
  formats other tools consume. **Do not pre-judge these here.** Feature and
  stability parity would make the tool useful beyond Rarog and Basilisk, which
  is welcome but explicitly **not the primary goal** — the primary goal is that
  our two engines can develop on one trustworthy harness on any OS.

### Phase 9 — Release preparation for `colosseum-cli`
The deliverable is a tool any engine developer can pick up.

- Ship as its **own binary asset**, versioned and released separately from the
  desktop app so harness churn cannot destabilise a 1.0 GUI product.
- Windows/Linux/macOS × x64/arm64 through the existing pipeline; smoke-test the
  exact published artifacts (`--version`, `--help`, one stub-engine match).
- User documentation: quickstart, the engine-descriptor reference, one worked
  example per command, and a short **"how to trust a result"** page carrying the
  parts of S3 a user needs (calibration, provenance, self-pair validation).
  User-facing docs must not carry phase numbers or internal naming.
- Reference descriptors for Rarog and Basilisk as worked examples.
- **Acceptance:** Rarog and Basilisk each run one real gate end-to-end through
  the released artifact, on at least two different operating systems, with a
  passing null calibration on each — and the result agrees with Phase 8(a).

---

## S9. Risks

| Risk | Mitigation |
|---|---|
| **Loss of runner independence** (S2) | Phase 8(a) parity gate; keep `fastchess` installed as a second opinion; re-run parity after any game-running change |
| macOS cannot enforce affinity | Decide in Phase 2; mark runs `affinity=advisory` or refuse clock gates there; the manifest always states which |
| Harness churn destabilises the released GUI | Separate bin crate, separate release asset, shared core covered by the existing test suite |
| Scope creep into engine-specific work | S4 "explicitly out of scope"; the descriptor is the only extension point |
| Silent statistical divergence from the old ledgers | Golden-file parity tests (S6.2) are permanent, not one-off |
| A derived constant is wrong and invisible | Assert written artifacts at the point of writing (doctrine 12) |

---

## S10. Reference

| Path | What |
|---|---|
| `crates/colosseum-core/src/stats.rs` | existing SPRT/Elo/LOS — extend here for pentanomial |
| `crates/colosseum-engine/src/{scheduler,runner,openings,store}.rs` | driver, game execution, books, persistence |
| `D:/code/rarog/tools/` | the PowerShell harness being replaced; the source of most doctrine in S3 |
| `D:/code/rarog/tools/results/` | stored real runs — the golden-file fixtures for S5.1 |
| `D:/code/rarog/PLAN.md` | the evidence behind every doctrine item, with dates and Elo figures |
| `D:/code/basilisk` | second consumer; proves the descriptor abstraction |
| `official-stockfish/fishtest` `worker/games.py` | reference adjudication settings |
