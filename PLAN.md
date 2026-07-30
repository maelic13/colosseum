# Colosseum CLI — engine-development harness plan

`colosseum-cli` is a headless, cross-platform harness for **developing** chess
engines: SPRT gates, SPSA tuning, gauntlets, speed measurement, and the
run-record/calibration machinery that makes those numbers trustworthy. Its only
contract with an engine is a UCI executable: no repository manifest, custom
build command or non-standard benchmark command is required.

The desktop app answers *"who is stronger?"*. The CLI answers *"is this commit
stronger than that commit, and can I believe the answer?"* — a different
question with much harsher requirements: reproducibility, explicit CPU
placement, byte-level identification of inputs, and an optional calibration
path that can exercise the complete harness on the machine where it runs.

**Audience of this file:** the maintainer. It carries the specifications,
success criteria, the evidence behind each requirement, and the forward plan.
[`GUIDE.md`](GUIDE.md) is the short operational view — what to do next.
`README.md` and `CHANGELOG.md` stay user-facing and must not carry method,
phase numbers or internal naming.

---

## S1. Current state

**The CLI is not built yet. This document is step 0 for that work.**

What exists is the reason the plan is short: `colosseum-core`,
`colosseum-uci` and `colosseum-engine` are already headless (no `egui`
dependency anywhere in them), already cross-platform, already released for
Windows/Linux/macOS including arm64, and already carry 149 tests. The CLI is a
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
2. **Binary identity is observable; source identity is not.** Colosseum hashes
   executable bytes and records the UCI identity and effective options. Custom
   node-count fingerprints may be useful inside an engine project, but they
   are not UCI and are not a Colosseum requirement.
3. **`[-3,+3]` is not an equivalence test.** Non-inferiority is `[-3,0]`;
   equivalence is a fixed-N calibration with a CI containment rule.
4. **A null is not proof of no effect.** Report the resolution limit alongside
   any null. A 3,000-game-per-arm paired difference resolves ±13.8 Elo; calling
   that "no effect" when hunting a ~16 Elo effect is wrong.
5. **Explicit CPU placement must be available for every clock match.** Unpinned Zen 3
   showed a hidden per-run offset of ~±10 nElo. `fastchess` before 1.7.0 failed
   to apply Windows affinity at all; 1.8.0 guesses SMT siblings from logical CPU
   order and pins **one core per game**, which starves `Threads>1` (a 4-thread
   comparison read −100 purely from starvation). Colosseum exposes automatic,
   disabled and explicit-CPU modes; if placement is requested but cannot be
   applied, it fails rather than silently degrading.
6. **Build comparability belongs to the test author.** Compiler, flags, source
   state, PGO data and auxiliary engine files are not exposed by UCI and cannot
   be verified reliably from an arbitrary executable. Colosseum neither builds
   engines nor rejects a comparison based on claimed build provenance.
7. **Observable run identity travels with the result.** Binary path and
   SHA-256, UCI identity, arguments, working directory, effective UCI options,
   harness version, book path/hash when used, seed, conditions and resolved
   affinity are written automatically. Unverifiable compiler/source metadata is
   not part of the contract.
8. **Calibration is available, not a prerequisite.** Two NPS
   estimators each read −0.2…−0.4% on a *self pair* (the same binary in both
   arms) and had already produced two confident false rejections. Bench NPS is
   left-skewed, so any estimator weighting the arms unequally against the slow
   tail manufactures bias. Validate on a self pair; compare arm-level **median**
   and **best-of**. Calibration is evidence about one machine and configuration,
   never a guarantee and never a condition for running another command.
9. **Unified conditions.** Tune and gate share TC, book *and* adjudication.
   A tuner optimising under different game-termination rules than the gate
   measures is optimising the wrong objective.
10. **Default to two-sided resignation.** `-resign movecount=3 score=600
    twosided=true`.
    Fishtest uses `score=600` with no `twosided` (verified in
    `official-stockfish/fishtest`, `worker/games.py`); we match the threshold
    and go one step stricter, because one-sided resignation adjudicates on the
    losing side's own evaluation — and in SPSA both arms are the same binary
    with perturbed parameters, so an arm producing more extreme scores resigns
    more readily than its sibling. That asymmetry lands directly in the
    gradient. Draw rule `movenumber=40 movecount=8 score=10` (stricter than
    fishtest's `34/8/20` on both axes).
11. **SPSA defaults are policy, not universal laws.** The default horizon is
    5,000 iterations and the default mini-match is 32 games because they are
    proven useful for the first consumers, but both are externally configurable.
    Colosseum does not enforce engine-specific claims about minimum horizon,
    dimensionality, curvature or noise floors.
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
    error. Books are optional: without one, games start from the normal initial
    position. SPRT/SPSA warn about the resulting lack of opening diversity but
    continue. Colosseum ships no book and never assumes a filesystem path.
14. **Speed is reported as speed.** Colosseum reports NPS differences and their
    uncertainty; it does not convert them to Elo using an engine- or
    time-control-specific rule.
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
colosseum-cli       NEW: bin + commands     ← run configs/records, NPS, SPSA driver
colosseum-gui       desktop app             ← unchanged; may consume new stats
```

**Placement rule:** pure functions go in `core` (so the GUI can reuse them and
so they are trivially testable); anything that spawns or places a process goes
in `engine`; anything that is a *workflow* goes in `cli`.

### Engine invocation and configuration

The required input is an executable path. Each side may additionally declare a
display name, arguments, working directory, allocated cores and arbitrary UCI
option values. Colosseum obtains engine identity and the supported option
schema from the normal UCI handshake.

Every ordinary workflow is fully controllable with CLI arguments. A
human-authored TOML run configuration is optional for repeatability; command
line values override it:

```text
built-in defaults < TOML run configuration < CLI arguments
```

SPSA is the exception only for its parameter vector: a tune TOML supplies the
selected numeric UCI option names and schedule inputs because a large vector is
not usable as command-line syntax. Global SPSA controls remain CLI options.
Colosseum writes the fully resolved configuration as machine-readable JSON in
the run directory.

### Explicitly out of scope

Anything that belongs to one engine: building, artifact discovery, custom
bench/fingerprint commands, compiler/source-tree inspection, parameter
*baking* into source, build flavour logic (PGO/PEXT/AVX2), Texel tuning,
datagen and NNUE management. The CLI consumes finished UCI executables.

---

## S5. Tool specifications and success criteria

Every criterion below must be checkable by a test or a single command. "It
looks right" is not a criterion.

### 5.0 CLI invocation and configuration — `colosseum-cli`

**Requirements**

- Bare UCI executable paths are sufficient. Per side, accept an optional display
  name, arguments, working directory, arbitrary UCI option values and allocated
  core count.
- `engine inspect` prints the UCI identity and advertised option schema;
  `engine check` exercises handshake, readiness, a bounded search, stop and
  clean shutdown.
- All ordinary workflow controls are CLI arguments. `--config <run.toml>` is an
  optional convenience, with CLI values taking precedence.
- SPSA requires a human-authored tune TOML only for its parameter vector.
- Every run writes its fully resolved configuration as JSON. JSON is generated
  output, not a required user-authored manifest.

**Success criteria**

- Two arbitrary UCI executables can be inspected and compliance-checked using
  paths and CLI arguments only.
- The same run launched from TOML plus overrides resolves to byte-identical JSON.
- Adding a new conforming engine requires no engine-repository file and no Rust
  code.

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
- Pin per *game slot*, and a slot must receive the explicitly configured
  `cores-per-engine`, not one — the starvation bug. This resource allocation is
  separate from whichever UCI option controls the engine's worker count.
- Modes: `auto`, `off`, or an explicit CPU list; configurable headroom defaults
  to 2 cores in automatic mode.
- Report the resolved placement in the run record, and fail loudly if pinning
  is requested and unavailable. An explicitly unpinned run is allowed and
  recorded.
- macOS caveat: there is no supported hard CPU-affinity API. Either use affinity
  *hints* and mark the run as `affinity=advisory`, or report hard affinity as
  unavailable and allow `off`. Clock matches are not prohibited.

**Success criteria**

- Unit tests over recorded topology fixtures (a Zen 3 16c/32t map, a
  P-core/E-core map, a single-socket no-SMT map, a 2-socket map) assert the
  chosen CPU list.
- An integration test spawns N busy child processes under a pinning request and
  samples actual CPU residency, asserting each lands on its assigned core.
  Skipped with a clear message where the OS cannot enforce it.

### 5.3 Null calibration — `colosseum-cli calibrate`

**Why:** it is an optional end-to-end symmetry test on the actual machine. It
does not prove correctness and is not required before other commands.

**Requirements**

- Byte-identical binary on both sides — **refuse** if the SHA-256 differs.
- Fixed game count (default 30,000), confidence (default 95%) and tolerance
  (default ±5 nElo), all externally configurable; no early stopping.
- PASS iff the full configured nElo CI lies inside the configured tolerance.
  Report inconclusive if the interval is wider than the tolerance rather than
  declaring PASS.
- Any timeout, crash, disconnect or illegal move invalidates the run.

**Success criteria**

- Different binary hashes are rejected; configurable fixed-N/CI/tolerance
  values round-trip through persistence and resume.
- PASS, FAIL, inconclusive and invalid outcomes each have deterministic tests.

### 5.4 Fixed match and SPRT gate — `colosseum-cli match|sprt`

**Requirements**

- `match` is fixed-N with no sequential stopping. `sprt` accepts explicit
  `elo0`/`elo1`/`alpha`/`beta` and model; `gainer` `[0,3]` and `simplify`
  `[-3,0]` are configurable convenience defaults. `calibrate` remains its own
  command (5.3).
- Two engines by path, with per-side arguments, working directory, arbitrary
  UCI options and allocated cores. The same binary may be tested with different
  options, or against itself.
- Configurable defaults: `tc=3+0.03`, hash 64 MB when a compatible option is
  advertised, worker count 1 when a compatible option is advertised,
  both colours per pair, and adjudication per doctrine 10.
- A book is optional. Without one, every game begins at the normal starting
  position; SPRT/SPSA print a lack-of-opening-diversity warning but continue.
- Live report block on an interval; full log to disk; PGN out; run record.
- Resume a stopped gate from the store without double-counting.

**Success criteria**

- Replaying a stored Rarog gate's game outcomes reaches the **same verdict at
  the same game number** as `fastchess` did (±1 report interval).
- A path-only invocation needs no configuration file; identical binaries and
  identical options are allowed for self-play.
- Killing the process mid-run and resuming produces the same final statistics as
  an uninterrupted run over the same seed.

### 5.5 SPSA — `colosseum-cli spsa` + `colosseum-core` schedule

**Requirements**

- **End-state parameterization** (doctrine 12): each knob declares `c_end` and
  the run declares `r_end` and horizon `N`; `c`, `a` and `A = 0.1·N` are
  back-solved. `alpha=0.601`, `gamma=0.102`.
- The tune TOML selects numeric UCI options and supplies each initial value,
  tuning bounds and `c_end`. The engine needs no tune manifest or source file.
- Defaults are `N=5,000` iterations and 32 games/iteration; both are
  configurable and neither is enforced as a minimum.
- Decay per **iteration**, never per game.
- Persistent driver: no per-iteration harness relaunch; a supplied book is
  loaded once.
- Multi-session: state saved every K iterations; log **appends** on resume;
  stops itself at the horizon; prints iteration/percent/ETA; the horizon is
  frozen at first launch and the tool must say so out loud.
- Emits the tail-mean vector for baking. **No per-knob filter** — SPSA estimates
  a *joint* optimum, and reverting a subset yields a point the tuner never
  evaluated. The tail mean already is the filter.
- **Config audit**, checked against the live UCI option schema:
  1. selected option absent or not numeric `spin` *(error)*
  2. duplicate parameter name *(error)*
  3. initial/tuning bounds outside the advertised UCI range *(error)*
  4. `min >= max`, so the knob cannot be measured *(error)*
  5. perturbation rounds to zero before the horizon *(error)*
  6. initial value disagrees with the engine default *(warn — may be deliberate)*
  7. initial value sits on a rail *(warn — one-sided gradient)*

**Success criteria**

- **Schedule property tests:** for any `N` and any `c_end`/`r_end`,
  `c_t == c_end` and `a_t == a_end` exactly at `t = N`; `r_end` is invariant as
  `N` varies over {1e3, 2.5e3, 5e3, 1e4} while `a` moves.
- **Written-artifact assertion:** the persisted schedule is read back and
  verified (`A == 0.1N`, `A > 0`, `a` matches) before any game is played. A test
  mutates the file and asserts the launch refuses.
- Every hard audit class has a fixture that must be rejected.
- **Recovery test:** kill at iteration K, resume, and the schedule continues
  (does not restart at full gain); the log retains the pre-kill iterations.
- **Convergence smoke test:** against a synthetic noisy quadratic objective with
  a known optimum, a 5,000-iteration run lands within a stated RMSE band —
  guards the optimiser, not the engine.

### 5.6 Speed / NPS A/B — `colosseum-cli nps`

**Requirements**

- Drive an optional user-supplied EPD position suite through standard UCI
  bounded searches; use the normal starting position when it is omitted and
  warn about the weaker workload. Consume `info nodes`/`time`/`nps` when
  provided and report clearly when the engine does not expose enough
  information.
- Strict alternation of arms; warm-up; arm-level **median** and **best-of**;
  bootstrap CI on the median.
- Accept one or more executables per arm and show per-executable medians, without
  requiring multiple builds.
- A self-pair is recommended but optional; warn when a recorded matching
  self-pair lies outside a configurable tolerance (default ±0.5%).
- Detect and report machine noise (per-round SD).

**Success criteria**

- A self-pair result is reported without being required as a prerequisite for
  comparing other executables.
- A synthetic left-skewed sample injected into the estimator reproduces the
  known bias in a naive ABBA estimator and *not* in the shipped one — the test
  that would have caught the original defect.

### 5.7 Gauntlet — `colosseum-cli gauntlet`

Mostly a CLI surface over existing core: ladder of opponents, joint ML ratings
with error bars, one anchor, standings/crosstable CSV.

**Success criterion:** on a stored tournament, CLI ratings match the GUI's for
the same data to ≤0.01 Elo.

### 5.8 Automatic run record — `colosseum-cli`

Generated JSON per run: both engines' canonical path/SHA-256, UCI identity,
arguments, working directory and effective options; harness version; optional
book path+hash; seed; resolved affinity; TC; adjudication; full command line;
UTC start/end and outcome. No engine-supplied manifest or unverifiable build
metadata is accepted or required.

**Success criteria:** a run record is written for every run including aborted
ones; a test asserts every observable field is populated and every
not-applicable optional field is explicitly `null` with a reason.

### 5.9 Book tools — `colosseum-cli book`

`slice`, `hash`, `stats` (count, ply depth, eval band if present), `verify`
(every position legal and parseable).

**Success criteria:** slicing is deterministic given a seed and reproducible
across platforms (same hash); `verify` rejects a known-bad EPD fixture.

### 5.10 Statistics/replay — `colosseum-cli stats`

Read a Colosseum run, PGN or supported fastchess/cutechess result log and report
the same pentanomial/nElo/SPRT block used live. This makes the parity machinery
useful outside the test suite.

**Success criterion:** replaying every 5.1 golden fixture through the command
produces the same result as the library API.

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
8. **Calibration (5.3) is optional end-to-end evidence**, not a CI or user
   prerequisite. PASS/FAIL/inconclusive/invalid classification is tested
   deterministically; maintainers may run real calibrations after material
   clock, scheduling or affinity changes.
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

**Rule:** platform support requires the full test suite and documented
capability/fallback behaviour there. Calibration results apply only to the
machine and configuration on which they were measured; they do not determine
whether the operating system is supported.

---

## S8. Implementation plan

Ordered by dependency and by risk. Numbers are frozen once written — later
inserts use `.0`/letter sub-items, as Rarog does.

### Phase 1 — Pentanomial statistics and nElo (`core`)
Spec 5.1. First because every later phase reports through it and because it is
what makes results comparable with the existing ledgers. Pure functions, no I/O,
fully testable offline. **Exit:** golden-file parity against ≥6 stored runs.

### Phase 2 — CLI skeleton, direct UCI invocation and run records
Specs 5.0 + 5.8. New workspace member, argument parsing, optional TOML input,
direct executable/arguments/working-directory/options controls, `engine
inspect/check`, and generated JSON run records. Decide the foundational
fastchess/cutechess interface gaps here: restart/recovery, per-engine time
margin, debug logging, opening repetition and machine-readable output.
**Exit:** two arbitrary UCI executables pass path-only inspect/check workflows;
TOML plus CLI overrides resolves identically; run-record schema and aborted-run
tests pass.

### Phase 3 — CPU topology and affinity
Spec 5.2. Implement `auto`/`off`/explicit-list placement, allocated cores per
engine and platform capability reporting. **Exit:** topology fixtures pass on
all three OSes; enforced-residency integration tests pass where supported; the
macOS advisory/unavailable behaviour is documented.

### Phase 4 — Fixed match, SPRT and optional calibration
Specs 5.3 + 5.4, including resume. **Exit:** verdict replay parity against a
stored Rarog gate; path-only and no-book runs pass; calibration outcome tests
pass.

### Phase 5 — SPSA
Spec 5.5: schedule in `core`, driver in `cli`, audit, multi-session, written-
artifact assertion. **Exit:** schedule property tests, every hard audit class
rejected by a fixture, recovery test, synthetic convergence smoke test.

### Phase 6 — Speed/NPS, book tools and statistics replay
Specs 5.6 + 5.9 + 5.10. **Exit:** the skew-bias regression test passes; optional
self-pair warnings work; book slicing is reproducible; golden fixtures replay
through the CLI.

### Phase 7 — Gauntlet CLI surface
Spec 5.7. Thin. **Exit:** ratings match the GUI to ≤0.01 Elo on stored data.

### Phase 8 — Parity check vs fastchess and cutechess, and remaining gaps
Two parts:

- **(a) Parity run — the entry gate for trusting this harness.** Prepare one
  deterministic opening sequence first, then use it sequentially with the same
  two binaries, TC and adjudication through
  `colosseum-cli`, `fastchess` and `cutechess-cli`. Compare final Elo/nElo/LOS,
  the pentanomial vector, draw rate, and time-loss counts. **Agreement within
  the combined error bars is the gate**; disagreement is a defect in one of the
  three and must be root-caused before either is trusted. This is the mitigation
  for the loss of runner independence (S2), so it is not optional and it is
  repeated after any change to game-running code.
- **(b) Remaining feature-gap decision.** Revisit what `fastchess`/`cutechess`
  do that Colosseum does not after the foundational decisions in Phase 2, and
  decide per remaining feature: adopt, decline with a reason, or defer. Known
  candidates: Chess960, ponder handling under test conditions and additional
  tournament formats. Feature and
  stability parity would make the tool useful beyond Rarog and Basilisk, which
  is welcome but explicitly **not the primary goal** — the primary goal is that
  our two engines can develop on one trustworthy harness on any OS.

### Phase 9 — Release preparation for `colosseum-cli`
The deliverable is a tool any engine developer can pick up.

- Ship as its **own binary asset**, versioned and released separately from the
  desktop app so harness churn cannot destabilise a 1.0 GUI product.
- Windows/Linux/macOS × x64/arm64 through the existing pipeline; smoke-test the
  exact published artifacts (`--version`, `--help`, one stub-engine match).
- User documentation: quickstart, direct-engine and optional run-TOML reference,
  SPSA tune-TOML reference, one worked example per command, and a short **"how
  to trust a result"** page carrying the parts of S3 a user needs (optional
  calibration, run records, self-pair validation).
  User-facing docs must not carry phase numbers or internal naming.
- Path-only and optional TOML worked examples using ordinary UCI executables.
- **Acceptance:** Rarog and Basilisk each run one real gate end-to-end through
  the released artifact, on at least two different operating systems, and the
  result agrees with Phase 8(a). A calibration example is documented but not
  required.

---

## S9. Risks

| Risk | Mitigation |
|---|---|
| **Loss of runner independence** (S2) | Phase 8(a) parity gate; keep `fastchess` installed as a second opinion; re-run parity after any game-running change |
| macOS cannot enforce affinity | Mark runs `affinity=advisory` or `off`; fail only when hard affinity was explicitly requested |
| Harness churn destabilises the released GUI | Separate bin crate, separate release asset, shared core covered by the existing test suite |
| Scope creep into engine-specific work | S4 "explicitly out of scope"; consume finished UCI executables and keep run configuration generic |
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
| `D:/code/basilisk` | second consumer; proves the bare-UCI abstraction |
| `official-stockfish/fishtest` `worker/games.py` | reference adjudication settings |
