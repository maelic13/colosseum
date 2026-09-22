# Phase 10 — first-release corrections, implementation record

Moved verbatim from `PLAN.md` §S8 on 2026-09-22 when the plan was trimmed to the open work. Every Phase 10 item is listed here with its rationale and implementation evidence, including the maintainer decisions of 2026-09-18, 2026-09-19, 2026-09-21 and 2026-09-22. The qualification figures are in [`phase-10-qualification.md`](phase-10-qualification.md). Items (ag), (ah), (ai) and (j) were still open when this record was cut; `PLAN.md` §S8 carries their current form.

---

### Phase 10 — First-release corrections (before `cli-v0.1.0`)

The accepted candidate was reviewed against the validation engines' real
harness policy and a maintainer's release expectations. Nothing found is a
defect in what 0.1.0 promised; each item is a default or a mechanism that is
cheaper to correct before anyone depends on it. The CLI version stays 0.1.0
because nothing has been published. Every step that changes game-playing
behaviour is followed by the recurring "after changing anything that runs
games" procedure at 10.10, once, on the final state.

- **(a) Product-latest release handling.** GitHub keeps one repository-wide
  "latest" release and both release workflows leave it to chance. The CLI
  workflow marks its release `make_latest: false`; the GUI workflow marks a
  stable release latest and a prerelease not. The architecture test asserts
  both. README and product documentation link to product tag lists, never to
  `/releases/latest`.
- **(b) Adjudication off by default** across `match`, `sprt`, `calibrate`,
  `spsa` and `tournament`, per the revised S3 Tier B. Draw and resignation are
  enabled by explicit flags that carry their parameters; the previous `--no-*`
  flags are removed rather than kept as no-ops because nothing has been
  published. `--one-sided-resign-adjudication` requires resignation to be
  enabled. Run files, resolved-configuration hashing, dry-run output, fixtures,
  acceptance tests and user documentation follow; the documentation names the
  common settings of public frameworks for users who want them.
- **(c) Class-aware CPU placement** per the revised S5.2: headroom of one
  physical core, highest-performance class only on hybrid hosts, last-level
  cache domains detected and kept per slot, refusal with a topology-naming
  message where the OS evidence is insufficient. `capabilities` reports class,
  NUMA and cache domains. Fixtures cover a hybrid performance/efficiency host,
  a dual-cache-domain single-socket host, a homogeneous SMT host and a
  no-SMT host; the chosen pool and slot allocation are asserted for each.
- **(d) Game record annotations** per S5.4b, in every game-playing command,
  plus score in the telemetry parser and `stats`.
- **(e) SPSA estimator and staged stop** per the revised S5.5: the final
  centre vector is the default estimator, the tail-window mean is optional and
  recorded, the result schema version is bumped, `spsa status` follows the
  same policy, and `--stop-after-iteration N` requests a clean stop at an
  iteration boundary without touching the stored horizon.
- **(f) Graceful stop** per the revised S5.11 for every durable command: one
  cancellation path through the drivers, bounded grace for in-flight games,
  checkpoint, `cancelled` run status, documented exit code, `status` showing
  the cancelled state. The shared kill/resume suite gains a clean-stop case
  per command; resumed statistics equal an uninterrupted run.
- **(g) Fixed rating field** per the revised S5.7: `--fixed <index>:<rating>`
  repeatable on `tournament run`, no error bar for pinned participants, fixed
  ratings retained as run inputs, JSON/CSV/text rows labelled accordingly.
- **(h) Book range policy** per S5.9: refusal instead of silent wraparound,
  `--book-wrap` opt-in, dry-run index range, and the datagen recipe in the
  match documentation written against it.
- **(i) Command-layer split and non-goals.** `composition.rs` holds every
  command in one file of more than six thousand lines; split it into one
  module per command with no behaviour change, guarded by a byte-identical
  generated command reference and unchanged tests. Record Chess960 as a
  non-goal in S4 and the compatibility page; a Chess960 FEN or option request
  is refused with a clear message.
- **(k) Shared game slots** per the revised S5.2: `--cores-per-game` as the
  default allocation without `--ponder`, `--cores-per-engine` as the explicit
  disjoint alternative and the only one accepted with `--ponder`, pool
  arithmetic per mode, mode and allocations in run record and dry-run,
  fixtures asserting 15 shared one-thread slots on a 16-core single-class
  host and 7 disjoint ones, user documentation. Found when a validation
  project's one-thread gate at concurrency 14 was refused on 16 cores.
  **Verified live 2026-09-17** on the 16-core, two-cache-domain host: a
  14-slot match sampled every second showed all 28 engine processes on 14
  distinct physical cores (masks `0x3` through `0xC000000`, cores 0–13) from
  four seconds after launch, core 14 spare and core 15 the headroom; a
  process is unpinned only for the sub-second window between spawn and the
  verified apply, before its handshake. A one-thread engine pinned to a
  core floats between that core's SMT siblings, so a system monitor shows
  every logical CPU active at about 45% utilisation; that is the expected
  picture, not a placement failure.
- **(l) Pair identity in PGN and run-directory telemetry** per the revised
  S5.4b and S5.10: identity tags on every written game, `stats <pgn>`
  reconstructing pairs and the pentanomial vector from them, `stats
  <run-dir>` reading its own `games.pgn` for telemetry, explicit zero fields
  accepted by the parser, the annotated fixture regenerated. Found when
  `stats` on a live run directory reported telemetry unavailable and `stats`
  on its PGN lost pair identity.
- **(m) Review defects**, each with a regression test: a match or SPRT
  interrupted while its last unit is being joined must report its completed
  or cap-reached verdict, not `cancelled`; `suite` must exit with the
  cancelled code when it writes `cancelled`; `--stop-after-iteration N` must
  be idempotent on resume against the cumulative iteration count; the stop
  grace period is one bounded period from the interrupt, not restarted per
  commit; `--anchor` together with `--fixed` on the same participant is a
  configuration refusal (exit 2) at resolution and in dry-run.
- **(n) Pair-identity replay defects** found by review of (l), each with a
  regression test: `stats` must pair games by the `PairNumber` and
  `PairGame` tags, never by game-number arithmetic, so a tournament
  encounter with any `--games-per-pair` replays to the checkpoint's vector;
  post-terminal SPRT pairs and invalid SPSA iterations are marked in a PGN
  tag the replay honours, so the official vector from a PGN equals the
  checkpoint's for a run that crossed a boundary at concurrency above one;
  tournament games carry the encounter's real `OpeningIndex`; the SPSA
  schedule artifact's `stats_version` field is renamed to what it holds
  (the RNG version).
- **(o) Unscorable games in the PGN**, found by review of (n): a game the
  runner aborted for an infrastructure fault is still written to `games.pgn`
  with a result and identity tags but no `ColosseumSample` tag, so a PGN
  replay scores it while the checkpoint does not. Tag it `unscorable`, let
  the replay exclude it and report the count, and cover it with a
  run-directory-versus-PGN equality test on a run with one aborted game.
  Also correct the `rng.rs` doc comment that still ties sampling to
  `stats_version`, and keep the versioned schema refusal reachable for old
  SPSA schedule and result files instead of a raw unknown-field error.
- **(p) Progress reports.** A long run must tell the operator where it
  stands without spamming the console. Progress is counted in the run's
  own unit, as fastchess and cutechess do with `-ratinginterval N` games
  and as an SPSA driver does per iteration: every durable command prints
  one progress block on standard error every `--progress-every N` units
  (pairs for `sprt`, `calibrate` and `spsa`-free paired runs, games for
  `match` and `tournament`, iterations for `spsa`; defaults 10 pairs,
  20 games, 1 iteration) plus a final block at termination. A time floor
  (`--progress-min-secs`, default 5) coalesces blocks when units complete
  faster than that, so a fixed-node run cannot flood the console; a block
  is never printed on every commit and never delayed past the next unit
  boundary once the floor has elapsed. `--progress-interval-secs` is
  removed. The block
  carries what a decision needs: for `sprt` and `calibrate`, games and
  pairs done, W/D/L and the pentanomial vector, the point estimate with its
  95% interval in the run's Elo model (both models for SPRT), LLR against
  its bounds, fault and time-loss counts, games per hour (the unit every
  command reports, a pair being two games) and, for SPRT, the
  expected remaining games at the current drift; for `match`, score, Elo
  with interval and rate; for `spsa`, iteration and percentage, elapsed and
  a linear ETA, the last mini-match pair score, the current gain and
  perturbation scale, and the three centres that moved most in absolute
  range units since the previous report; for `tournament`, games done, the
  current standings header with ratings and error bars, rate and ETA. The
  same block is what `status` prints for the run. The append-only `run.log`
  records every block so a console can be closed and the trajectory
  recovered. Found when a maintainer stopped a real SPRT because the console
  showed only pair counts.
- **(q) Commit path off the game loop, and headroom from the bottom.**
  Found on 2026-09-18 by a 30,000-game identical-binary calibration and a
  2,000-game fixed match on the 16-core host: 57 and 14 time forfeits where
  fastchess had produced none for the same engine at the same control. In
  every forfeit the engine's last `info` line reported 14–17 ms of search
  while the harness charged 165–608 ms (median 327 ms). The cause is the
  per-game commit: the driver's synchronous completion callback rewrites
  the whole checkpoint with `fsync` and truncates and rewrites the whole
  `games.pgn`, on the async runtime thread, after every game; both files
  reached 167 MB, so late in a long run each commit blocks the runtime for
  hundreds of milliseconds and the `bestmove` of another game waits behind
  it, because charged time is read when the game task reaches the line,
  not when the line arrived. Required: (1) the `bestmove` timestamp is
  taken by the pipe reader at line arrival, in the UCI session, so no
  harness work after arrival can be charged to an engine; (2) a run
  directory holds four files with distinct jobs: `games.jsonl`, an
  append-only journal with one checksummed record per game (identity,
  result, termination, fault kind, clock summary; never a PGN);
  `games.pgn`, appended one game at a time and never rewritten;
  `checkpoint.json`, constant size, aggregates only (counts, official
  prefix, LLR, SPSA centre and iteration, the journal offset and running
  hash it covers), two generations, written every K units or every few
  seconds and on stop; and `run.log`, human events only (progress blocks,
  faults, stop and resume), never a game. Resume loads the checkpoint,
  verifies the journal to the recorded offset by hash, drops a torn last
  line and replays only the tail, so commit cost is O(1) per game and the
  official prefix and iteration boundaries are recomputed, never pooled.
  Durability is group-committed: appends without sync, one `fsync` of the
  journal and PGN every K units or one second and at every checkpoint and
  stop; a hard kill loses at most that window, which resume simply does
  not have. In-memory driver state holds summaries, not PGN text; (3)
  every file write, sync and rename runs on a blocking thread
  (`spawn_blocking`), never on a runtime worker; (4) `auto`
  headroom is taken from the lowest-numbered cores upward, so CPU 0, where
  Windows services most interrupts, is never a game core; (5) the fault
  policy documents that time losses count inside engine faults, and the
  default for `calibrate`, `match` and `tournament` becomes a documented
  non-zero fraction with the count reported, while `sprt` and `spsa` keep
  invalidation because their samples are pair-atomic. Success criteria: a
  30,000-game stub run's per-commit wall time is flat from first to last
  game; the charged-time distribution in scrambles (remaining under 100 ms)
  on the real host shows no move charged above the engine's reported time
  plus 20 ms in a 2,000-game 3+0.03 match; the fixed-movetime outlier
  probe (100 ms, 14 slots, 50,000 moves) is at or below fastchess's rate.
- **(r) Placement per platform.** Linux can reserve cores where Windows can
  only restrict: detect `/sys/devices/system/cpu/isolated`, `nohz_full` and
  IRQ affinity, prefer isolated cores in `auto` when present, never require
  them, and record what was found. macOS stays advisory as recorded. WSL is
  a virtual machine with synthetic topology and cannot supply placement or
  latency evidence; the Linux evidence needs a native boot. Deliverable: a
  research note with the per-platform contract and the measurements that
  justify it, then the implementation as its own step if the note says so.
- **(s) Writer hardening**, from review of (q): the writer queue is bounded
  so the game loop cannot run unboundedly ahead of the disk (a full queue
  blocks the committing task off the runtime, never a runtime worker); a
  writer failure still leaves a terminal run record on disk through a
  direct atomic write, never `running`; a pre-10.9g run directory is refused
  with the `--restart` guidance rather than a bare schema message; an
  over-long protocol line is a per-read fault that keeps the session
  draining, not a permanent termination; an unscorable game keeps its pair
  or iteration class in the journal beside the `unscorable` mark so replay
  cannot drop the rest of an iteration. Each with a regression test.

  **Implementation evidence (Phase 10.9i):** the writer queue is a
  `sync_channel` of `WRITER_QUEUE_CAPACITY` (256) commands. A send tries
  first; when the queue is full it waits inside `block_in_place` on a
  multi-threaded runtime, so the committing task hands its worker to the
  other games before it blocks (outside a runtime, or on a current-thread
  one, it simply waits; the writer drains on its own thread). The regression
  runs a one-worker runtime with the writer held and a queue of two: the
  committing task stays blocked while a ticker task keeps running, and with
  `block_in_place` removed the same test fails on the stopped ticker. A
  terminal run record is written through `replace_and_report`, which waits
  for the writer's answer; a failed writer, which writes nothing after its
  first failure, answers with that failure and the recorder then writes the
  record directly and atomically with a `writer-failed` anomaly. Both
  `finish` and the dropped-owner `aborted` path are tested against a writer
  whose replace failed. A checkpoint of an earlier schema is now a distinct
  `EarlierLayout` error that names the directory and the `--restart`
  remedy, preferred over the bare checksum/schema pair; the test downgrades
  a real run's checkpoints, removes its journal, sees the guidance and no
  bare schema message, then restarts it successfully. An over-long protocol
  line is skipped to its newline by the reader thread and reported as one
  `Overlong` event; the read that meets it fails as a protocol fault and the
  reader goes on, so later lines still arrive (unit test with the long line
  spanning many buffer refills) and the session still answers (`self-test`
  now requires `isready` to succeed after the fault); only an I/O failure
  or end of stream ends the reader. Unscorable games are journalled under
  the class of their pair or iteration with `scorable: false`, through one
  helper used by `sprt` and `spsa`, and `match` and `tournament` keep
  `official`; the PGN tag stays `unscorable`, and the journal replay in
  `stats` sets such a game aside as `unscorable`, so the 10.9e equality
  between a run directory and its PGN holds. The regression rebuilds an
  SPSA iteration containing an unscorable game from its journal records and
  gets both pairs. Deviation: journal lines written by 10.9g–10.9j with
  `sample: "unscorable"` still read, but a resume cannot place those games
  in their pair; a directory from those builds should be restarted if it
  holds one.
- **(t) Round-trip latency instrument.** After (q) the commit stall is gone
  and the run directory stays small, but the real-host 3+0.03 forfeits did
  not move (15 in 2,000 games at 14 shared slots; 3 and 4 in 1,000 at 7
  disjoint and 7 shared; fastchess 0 in 2,000). Every forfeit position
  replayed directly against the bare engine answers inside its limit, and
  in each forfeit no `bestmove` reached the reader before the deadline, so
  the time is lost in the harness-to-engine round trip, not in the search
  and not in core sharing. Guessing stops here. Required: per move, the
  runner records a phase breakdown with monotonic stamps taken by the
  thread that performs each step: `go` stamped, `go` write returned, first
  `info` arrived, last `info` arrived with the engine's own reported
  `time`, `bestmove` arrived, `bestmove` consumed by the game task. The
  journal record keeps per-side maxima of each phase and of
  `charged − engine time`; `games.pgn` gains `h=<ms>` (harness overhead,
  charged minus engine-reported time) beside `t=`; a forfeit forensic
  prints the full breakdown of its last five moves. `stats` reports the
  overhead distribution per engine (p50/p99/p999/max) and the count of
  moves whose overhead exceeded the margin. Success criterion: on the
  16-core host at 14 slots, a 2,000-game 3+0.03 match names the phase
  that carries every overhead above 20 ms; the fix for that phase is its
  own step and is not guessed here. Corrections owed from review of the
  instrument, to land with that fix step — which they did, with (u); see its
  implementation evidence: a `ponderhit` search charges from
  the `ponderhit` write while the engine reports time since `go ponder`,
  so `h=` is wrongly negative under `--ponder`; the forfeited move's own
  overhead is recorded in the journal maxima but not in `games.pgn`, so
  the `stats` over-margin count misses the one move that matters; the
  late-`bestmove` wait stops on a per-read fault; PGN `h=` truncates to
  milliseconds while the journal keeps nanoseconds; an SPSA resume over a
  rebuilt iteration with an unscorable game reports a checkpoint mismatch
  instead of naming the game.
- **(u) A slot belongs to one game at a time.** The instrument of (t) named
  the phase (all harness phases under 1.3 ms; every forfeiting `bestmove`
  42–64 ms after the engine's own hard cap, a second mode and not noise)
  and a maintainer's system monitor named the cause: cores dropping to idle
  while others ran, where fastchess keeps every core at 100%. All four
  drivers choose a game's CPU slot by arithmetic,
  `(number − 1) % slots.len()` (`match_runner`, `sprt_runner`,
  `spsa_driver`, `tournament_driver`), not by which slot is free. Games end
  at different times, so the next game is routinely placed on a slot that
  is still playing while a finished slot idles. Replaying that rule over a
  real 2,000-game run reproduces its wall time and gives 7.8% of slot-time
  double-booked, 7.6% idle, and 71% of games sharing their CPU with another
  game at some point; two searches on one pinned CPU alternate in scheduler
  quanta, which is the 50 ms stall. fastchess on the same host, day and
  binaries: 0 forfeits in 2,000 games; Colosseum: 14–15. Required: one
  free-slot pool per run, owned by the execution plan. A unit takes a slot
  before its first engine is spawned and returns it only after both engine
  processes of its last game have exited; the unit is a game for `match`,
  `calibrate` and `tournament`, and a colour-reversed pair for `sprt` and
  `spsa`, whose two games run on the same slot. Launch order, pair identity,
  opening assignment and commit order are unchanged: only the CPU placement
  of a unit changes, so results are reproducible from the same seed except
  for timing. The slot a game ran on is recorded in its journal record and
  as a PGN tag. Invariant, asserted in debug builds and tested: no two live
  units ever hold the same slot, and no unit waits while a slot is free.
  Test: a stub run with deliberately uneven game lengths, concurrency 4,
  that fails on the modulo rule. Success criterion on the real host: the
  2,000-game 3+0.03 match at 14 slots shows zero time losses and every game
  core continuously busy. **Demonstrated 2026-09-18** on the 16-core host:
  2,000 games at 14 whole-core slots, 0 time losses and 0 engine faults
  (fastchess the same day: 0; Colosseum before the pool: 14–15, and 206 with
  one logical CPU per game, the dose-response of double-booking); the journal
  shows 0 overlapping slot spans and a median hand-over gap of 0 ms; the run
  was stopped and resumed once through the journal; with under 100 ms on the
  clock the slowest charged move was 81 ms against fastchess's 95 ms. Elo
  +65.9 ± 11.0, nElo +93.7 ± 15.2 against fastchess's +54.3 ± 11.1 and
  +75.6 ± 15.2 on the same binaries.
- **(v) A rare forfeit must not void a sequential test.** With the pool in
  place a real `[0,10]` SPRT was invalidated at pair 6 by one time loss: last
  `info` at 21 ms, the engine's own cap at 39 ms, `bestmove` at 80.6 ms
  against a deadline of 80.5 ms, no overlapping slot span, no other process
  starting or exiting within 950 ms. One such event in 2,046 post-pool games
  is indistinguishable from fastchess's zero in 4,000, and by the PGN metric
  (scramble moves charged 25 ms past the engine's hard cap) fastchess shows
  0.44–0.69 per 1,000 scramble moves where Colosseum shows none, so the
  harness is not the outlier; an operating system occasionally holds a
  process for tens of milliseconds. A zero allowance therefore voids any
  long SPRT or tune: at this rate a 20,000-game test would die about ten
  times. fastchess and fishtest score a time loss as a loss and continue.
  Required: for `sprt` and `spsa` an engine-attributable forfeit is scored
  as the loss it is, its pair stays in the official sample in order, and the
  run becomes invalid only when faults exceed a documented rate (default
  0.5% of games played, minimum 3, evaluated continuously), with the count
  and rate in every progress block and the final report; `--max-time-losses
  0` restores strict invalidation. Infrastructure faults stay unscored and
  still invalidate the pair.
- **(w) Residual time losses: parity with fastchess.** The record so far, all
  on one 16-core host, the same two binaries, `3+0.03`, one thread, 14
  concurrent games, 20 ms margin: fastchess 0 time losses in about 4,400
  games and 0 in its 30,000-game identical-binary run; Colosseum 14–15 per
  2,000 before (u), 206 in 529 with double-booked single logical CPUs, and
  2 in about 2,500 since (u). If fastchess had Colosseum's present rate,
  its zero would be roughly a 3% event, so a residue probably remains.
  **Ruled out by measurement:** the engine's search (every forfeit position
  replayed against the bare engine answers inside its limit, and ordinary
  aborted iterations land within half a millisecond of the engine's own
  hard cap); the harness round trip (`go` write, first `info`, arrival lag
  and consumption all under 1.3 ms across 4,000 game-sides); the commit
  path (q); double-booked slots (u); process start and exit storms (no
  other boundary within 950 ms of a post-(u) forfeit); core sharing (3
  against 4 per 1,000 at 7 disjoint and 7 shared slots, measured before
  (u)); harness thread starvation (pinning the harness to the spare core
  changed nothing). **Not the cause, and kept:** CPU placement. With
  placement off the fixed-movetime probe showed more late moves, not fewer
  (6.2 against 4.2–4.7 per 1,000), and an unpinned host carries a hidden
  per-run offset of about ±10 nElo, which biases verdicts silently where a
  forfeit is counted and visible. **The signature:** a `bestmove` that
  arrives 40–60 ms after the engine's own hard cap, a second mode and not
  scatter, with the engine process itself held. **Untested differences
  from the engine's point of view:** Colosseum starts fresh engine
  processes for every game where fastchess keeps them alive between games
  (cold caches, first-touch faults on the hash, image and heap setup on
  every game); engines run inside a kill-on-close job object; the mask
  allows both SMT siblings where fastchess pins one logical CPU; process
  creation flags, priority class and pipe construction (anonymous against
  named, buffer sizes, overlapped against blocking reads); the clock
  messages themselves (Colosseum games spend far fewer moves under 100 ms
  than fastchess games, 8,600 against 32,000 per 2,000 games, so the two
  harnesses do not hand the engine the same clock trajectory). The
  desktop application, unpinned and on the older clock model, is reported
  never to forfeit the same engines; its incident logs are evidence to
  count, not an explanation. **Required, as research before any fix:**
  (1) a source study of fastchess recorded in
  `docs/architecture/fastchess-mechanics.md`: engine process lifetime and
  restart policy, process creation and priority, affinity application,
  pipe and reader design, exactly where its clock starts and stops, how
  `timemargin` is applied, what it sends between games, and every
  mechanism a match runner of this kind has that Colosseum lacks, each
  marked adopt, decline with a reason, or test; (2) a count of time
  forfeits in the desktop application's incident logs over a large pool;
  (3) a discriminating run for each untested difference above, 2,000 games
  each and maintainer-run, changing one thing at a time, with the late-mode
  metric (scramble moves charged 25 ms past the engine's hard cap) and the
  forfeit count as the two readings; (4) only then the fix, as its own
  step. Success criterion: 0 time losses in 10,000 games at 14 slots on the
  reference host, or a documented operating-system floor that fastchess
  shares. **Found 2026-09-19** (`docs/architecture/fastchess-mechanics.md`,
  Findings): the residual forfeits are not the harness's. Per-search held
  time, kernel time and page faults (10.9m instrument) show the forfeiting
  searches on their CPU the whole time, each taking the ~130 page faults of
  Rarog's 512 KiB KPK bitbase, which Rarog builds inside the evaluation on
  first use (33–37 ms, uninterruptible by its clock check). fastchess keeps
  processes for the whole run and pays it once; Colosseum's fresh processes
  pay it in most games, and it forfeits when it lands in a scramble. A
  Stockfish 18 null run under the same conditions: no losses, stalls up to
  50 ms, but never closer than 161 ms to a deadline. No harness change is
  needed; the target is to be confirmed on an engine build that initialises
  the table before searching. This does not block use: a symmetric forfeit per thousand games
  moves an estimate by a small fraction of its error bar, and (v) keeps a
  sequential test alive through one.
- **(x) An SPSA iteration must fill the machine.** Measured on a real 60-
  iteration tune at 14 slots, 32 games per iteration, mean game 8.9 s: mean
  iteration 38.4 s where 32 games on 14 slots need about 20 s, slot occupancy
  inside an iteration 53%, 2,900 games per hour against 5,500 for a fixed
  match. Cause: (u) made the colour-reversed pair the slot-holding unit for
  `spsa` as for `sprt`, so 16 pairs on 14 slots run as 14 pairs back to back
  and then 2 more while 12 slots idle, and the update must wait for them.
  For `sprt` the pair stays the unit. For `spsa` it buys nothing: both arms
  play in every game and slots are identical by construction. Required:
  within an iteration every game takes a free slot individually, in schedule
  order; the two games of a pair may run on different slots and at the same
  time; the iteration is still committed atomically and the gradient still
  uses whole pairs, reassembled by pair identity; a fault policy decision is
  still made on the whole mini-match. `spsa plan` and the dry run report the
  wave shape for the resolved concurrency (games per wave, slots idle in the
  last wave, expected occupancy) and warn when games per iteration is not a
  multiple of the slot count, naming the nearest even multiples; its
  wall-time estimate uses waves. Test: a stub iteration with more pairs than
  slots and uneven game lengths whose slot occupancy is asserted, and which
  fails on pair-held slots. Success criterion on the reference host: a tune at
  14 slots and 42 games per iteration, or 15 slots and 30, holds occupancy
  above 85% and exceeds 5,000 games per hour. **Deferred to after the
  release, as research:** asynchronous SPSA in the fishtest manner, where the
  next games start on free slots with the current parameters and results are
  applied as they return; it removes the iteration barrier at any mini-match
  size but updates on slightly stale parameters and gives up the one
  iteration, one update record, so it needs its own evidence that it reaches
  the same optimum for less game budget. **Found while verifying (x),
  2026-09-18:** the launch loops deep-copied the opening book per launched
  game (`spsa`) or pair (`sprt`), about 0.45 s and hundreds of megabytes with
  a 2.6-million-position book, on the one task that launches games: SPSA
  games started 0.44 s apart (6 s to start 15), and `sprt` ran at 4,460 games
  per hour where `match` reached 5,500. The book is now shared behind an
  `Arc`; 15 launches land within 1 ms and measured occupancy is 80–86%
  against the 81% predicted.
- **(y) SPSA at the scale of a real tune.** A 60-iteration run left a 6.2 MB
  `result.json`, about 100 KB per iteration, because the driver keeps every
  game's full record in memory for the whole run and writes them all at the
  end: over 500 MB and as much memory at 5,333 iterations. Required: the
  result carries per-iteration summaries (centres, arm vectors, pair score,
  faults) and refers to the journal for games; driver memory is bounded by
  the iteration in flight, not by the run; a 5,000-iteration stub tune shows
  flat per-iteration commit time, bounded resident memory and a result under
  a stated size. A launch-spread regression test for `spsa` and `sprt` with
  a large synthetic book (all launches of a wave within a few milliseconds)
  so no per-launch deep copy can return, and an audit of every value cloned
  per launch. The budget may be given in games (`--total-games`), from which
  the iteration count is derived for the chosen mini-match size, so a
  registered budget survives a change of size; `spsa plan` reports hours from
  the wave model. The opening book is held compactly (offsets into one
  buffer rather than an owned string per field) and loads in well under a
  second. `status` on a run that has printed no block yet says when the first
  is due.

  **Implementation evidence (Phase 10.9o):** an SPSA iteration is kept as a
  summary — centres before and after, the arm vectors, the pair score, its
  `faults` and `games` (the first and last game numbers of its journal lines)
  — and the observer receives the iteration's games beside it once, to
  journal them; the driver then drops them, so what it holds is the
  iteration in flight plus one small summary per committed iteration.
  `result.json` gained a document `schema_version` (2) and `games:
  "games.jsonl"`; `tuned_result` and its version 3 are unchanged, so
  `sprt --apply` reads old and new results alike. Resume rebuilds summaries
  from the journal (pair identity, scorability and score are checked there;
  the driver then checks each summary's iteration and game range), `spsa
  status` and the progress blocks read summaries (a resumed tune's faults now
  count in its blocks), and `stats` on an SPSA directory finds no game array
  in the result and reads the journal. A hidden `--__synthetic-games` switch
  plays games in-process as instant results for scale tests. A 5,000-
  iteration, 4-game tune under it: median commit cycle 3,421 µs over the
  first tenth and 3,406 µs over the last, resident memory 16.6 MB early and
  18.8 MB late, `result.json` 5.66 MB (bound 8 MB, about 1.1 KB per iteration
  of a one-knob tune against about 100 KB before); the test asserts the cycle
  within 3× + 2 ms, growth under 16 MB and the result under 8 MB. Launch
  audit: per launched game or pair the drivers clone two engine launch
  specifications, time controls, adjudication, a slot allocation and the
  `MatchOpenings` handle; the book was the only large value and is shared, and
  `OpeningList` is no longer `Clone`, so a deep copy of a book does not
  compile. The launch-spread test runs an SPSA iteration and an SPRT at four
  slots on a 200,000-line book and requires the first wave's launches within
  10 ms (measured 66 µs and 1 µs); with a per-clone copy of the book
  restored it measured 324 and 328 ms and failed. `--total-games` (on `spsa`
  and `spsa plan`, exclusive with `--iterations`) derives the horizon for the
  chosen mini-match, refuses a budget it does not divide and names the
  nearest two (160,000 at 42: 159,978 or 160,020), and is stored in the
  resolved configuration only when given, so existing configurations hash
  as before; the plan's estimate is printed in hours from the wave model.
  The book is `OpeningList`: one text buffer and a 24-byte entry per opening
  (`[FEN][moves][label]` offsets, the EPD label being the FEN itself), the
  shuffle permuting entries with the same RNG and swaps, EPD lines validated
  in parallel chunks cut at line boundaries and joined in file order; the
  test compares 60,000 lines through the parallel path and a PGN book with
  the previous owned loader for sequential, random and counted orders, and a
  2.6-million-line book loads in 212 ms in an optimised build (the test
  bounds 1 s there, and a tenth of the book within 10 s in a debug build).
  `status` on a run with no block yet says when the first is due, from a
  `progress` entry (every, unit, floor) each command now records in its
  workflow evidence. `run_game` prepares both engines with `tokio::join!`;
  a setup failure keeps its own side's error and forensics, and an engine
  that spawned but failed setup is still reaped before the game returns;
  the regression plays two games against an engine that fails its
  handshake and finds the fault on Black, then White, with the broken
  engine's forensic each time. Deviations: the result's own version is new
  (`SPSA_RESULT_SCHEMA_VERSION` 2) rather than a bump of the tuned-result
  version, which did not change; a run's retained summaries still grow with
  its iterations, at about 0.6 KB each; memory is measured from the working
  set on Windows and `VmRSS` on Linux and not asserted elsewhere; the scale
  test's games are synthetic, so it measures everything but engine play.
- **(z) Two games per physical core, as a measured experiment for tuning
  only.** A one-thread game keeps one logical CPU busy; the core's SMT sibling
  idles. Placing a second game on the sibling doubles the concurrent games
  (30 on the 15 game cores of the reference host). SMT usually returns 20–25%
  more total work, so each engine runs at roughly 60% of its speed: for the
  engines it is the same experiment at a shorter effective time control on
  slower hardware. It cannot bias a tune, because both arms are the same
  binary under identical conditions, but it departs from the rule that a
  tune runs under gate conditions, so whether its result transfers is the
  question, not whether it is faster. Required: an explicit, recorded
  placement mode (`--games-per-core 2`, default 1) accepted by `spsa` and
  `match` only and refused by `sprt`, `calibrate` and `tournament`; each game
  pinned to exactly one logical CPU, the two games of a core on its two
  siblings, never two games on one logical CPU; refused where the sibling map
  is unavailable or the core has no sibling; mode in the run record, PGN tag
  and dry run. Evidence, maintainer-run on the reference host, same binaries
  and book: (1) throughput and per-engine speed, as games per hour and the
  nodes-per-second ratio against one game per core; (2) forfeits and the
  late-move metric over 2,000 games; (3) transfer: the same short tune run in
  both modes from the same seed and budget, each gated by `sprt --apply` under
  ordinary one-game-per-core conditions. Adopt for tuning only if (3) shows
  the sibling-mode tune gates no worse, and record the measured wall-time
  saving; otherwise decline with the numbers. Never a gate condition.
  **Deferred behind the release by maintainer decision, 2026-09-18:** the
  transfer evidence costs two tunes and two gates, and two tunes cannot be
  compared by their values. A ten-minute pre-check needs no code: two
  matches started together, one with `--placement 2,4,…,30` and one with
  `--placement 3,5,…,31`, 15 games each, give the real throughput gain and
  the per-engine slowdown; if the gain is small the step is declined.
- **(aa) Corrections from review of (y), and a smaller result.** On a
  resumed tune the journal records loaded for the replay stay resident for
  the rest of the run (`composition/spsa.rs`, bound in the outer scope of
  the run function): drop them after the replay, and extend the scale test
  to a resumed run. `result.json` still stores, per iteration, the plus and
  minus values, perturbation signs and gain coefficients of every knob,
  about 16 KB per iteration at 82 knobs and some 200 MB pretty-printed for
  a full tune, all derivable from the schedule, the seed and the centres:
  store the centres after each iteration, the pair score and the faults,
  bump the result schema, keep `spsa status` and `sprt --apply` working.
  `book stats` and the desktop scheduler's `load_openings` still materialise
  the whole book; a self-comparison in `spsa_driver` checks nothing; `stats`
  refuses a `result.json` given as a file; a run file's `iterations` clashes
  with `--total-games` on the command line instead of being overridden. The
  maintainer's 60-iteration tune at 15 slots and 30 games per iteration is
  the throughput evidence for (x). **Measured 2026-09-18:** 1,800 games,
  23.9 s per iteration, 4,517 games per hour (weather-factory on the same
  surface 3,720; Colosseum before (x) and the shared book 2,745); slot
  occupancy inside an iteration 83% against 81% modelled; all 15 launches of
  a wave within a millisecond; no gap between iterations; no overlapping
  slot spans; no time loss or other fault at 15 slots. **Qualified
  2026-09-18:** every one of its 1,800 journalled games ran at the default
  2,000 ms margin (`--margin-ms` was not given), so its zero time losses
  are not comparable with a 20 ms run and say nothing about (w).

  **Implementation evidence (Phase 10.9q):** the driver keeps, and
  `result.json` stores, one `SpsaIterationSummary` per committed iteration
  (iteration, centres after, pair score, faults, journal game range) and an
  `SpsaInvalidSummary` (iteration, faults, reason, game range);
  `SPSA_RESULT_SCHEMA_VERSION` is 3. The arm vectors, signs, gains and
  centres before still reach the observer once, in the full
  `SpsaCommittedIteration`, for the journal and the progress block; the
  replay returns the last iteration in full for `spsa status`. Measured on
  the real 82-knob Rarog tune: 26.1 KB per iteration pretty-printed in
  version 2 (the 15-slot run's `result.json`), 2.2 KB in version 3; the
  5,000-iteration one-knob stub result fell from 5.66 MB to 2.46 MB. The
  dead self-comparison was the resume check: summaries rebuilt by the
  replay were compared with game ranges computed by the same function, and
  then replayed through `SpsaTuningState::resume` against histories that
  replay had just produced. Resume now recomputes each summarised iteration
  from the schedule and its stored score and refuses one whose centres
  after, score or number it does not reproduce (unit test: a moved centre,
  a changed score and a renumbered iteration are each refused at the
  iteration where they diverge). A resumed tune drops the journal records
  once the replay has built its summaries; the scale test gained a resume
  case (stop at 4,500 of 5,000 iterations, resume, same summaries as an
  uninterrupted tune from the same seed) that measured the resumed process
  2.8 MB above the uninterrupted one and 13.7 MB above it with the records
  kept, and asserts 8 MB. `load_openings` returns the compact
  `OpeningList`; the desktop scheduler materialises only the opening each
  scheduled game draws, `summarize` reads the first label in place, and
  `book stats` counts duplicates over borrowed (FEN, move text) pairs and
  plies in one pass; `into_resolved` is gone. `stats` given a result that
  names its journal (`"games": "games.jsonl"`) reads the run directory it
  belongs to, and a test requires the same report from the file and the
  directory. A run file's `iterations` yields to `--total-games` on the
  command line and `total-games` to `--iterations`, as a repeated option
  does. Real smoke on the release build: an 82-knob tune of 3 iterations of
  30 games at 15 slots and 1+0.01, stopped after 1 and resumed; 90
  journalled games, no fault, no overlapping slot span, `stats` identical
  on `result.json` and on the directory. Found in passing and left for
  10.10: `killed_match_resumes_missing_games_in_deterministic_schedule_order`
  fails about one run in eight at this commit and at its parent alike,
  when the match completes between the test's liveness check and its kill.
- **(ab) Qualification on a real engine is part of the release, and lives
  here.** A project that adopts the harness must be able to trust a
  released binary without re-testing it, so the evidence that the harness
  measures correctly belongs to this repository's release acceptance, run
  by the maintainer on the reference host with a validation engine and
  recorded in `docs/architecture/phase-10-qualification.md` with commands,
  binary and input hashes and results. On the release candidate: (1)
  **symmetry**, an identical-binary `calibrate` of 30,000 games with the
  whole 95% normalized-Elo interval inside ±5; (2) **scale**, a fixed
  2,000-game match of two builds whose difference a second runner has
  measured, intervals overlapping; (3) **verdict**, an SPRT of a pair the
  second runner has gated, same verdict; (4) **ratings**, a gauntlet with a
  fixed field whose anchors keep their ratings; (5) **tuning, the recovery
  test**, which replaces comparing two full tunes: two tunes of a wide
  surface end at different noise around a flat region, so their values
  cannot be compared, and their strength differs by less than a feasible
  match resolves, whereas a tune with a known answer can fail. Three or
  four high-sensitivity parameters are detuned far enough to cost at least
  30 Elo in a fixed 2,000-game match against the defaults, every other
  parameter fixed; `spsa` runs from the detuned start for about 30,000
  games at full concurrency; pass means every detuned parameter ends at
  least half way back to its default and `sprt --apply` of the tuned values
  against the detuned start accepts H1 at `[0,10]`. A sign, scale or
  schedule error fails it outright. (6) **faults**, zero time losses in the
  scale match, or the floor (w) documents. Already on record from
  2026-09-17/18, to be repeated on the candidate only where the code under
  test changed since: scale +55.7 to +65.9 against a second runner's +52.2
  and +54.3; verdict H1 at 220 pairs, nElo +94.1 ± 32.5, against 216 pairs
  and +94.0 ± 32.8; symmetry −1.1 ± 3.9, measured before the slot pool and
  therefore owed again. Item (j) does not close without this record.
  **Run on the candidate 2026-09-19/21** (GUIDE 10.9r, all four in the
  qualification document): scale +54.3 ± 10.6; verdict H1 at 241 pairs;
  ratings 3201.9 ± 14.0 against the desktop's 3191; tuning recovered a
  −56.2 ± 10.2 Elo detune to +9.6 ± 10.0 against the defaults, its gate
  accepting H1 at 252 pairs; symmetry (GUIDE 10.9v, 2026-09-21) −0.0 ± 3.9
  nElo over 30,000 identical-binary games, status `pass`. Zero faults of any
  kind in every run. The
  recovery test's per-coordinate rule is now known to be the wrong
  instrument: two of the four coordinates ended inside the schedule's own
  noise (±4 perturbation steps over 1,500 iterations at `r_end` 0.03) while
  the tuned vector was at least as strong as the defaults, because the
  surface is coupled and the defaults are not its only optimum. A repeat
  gates on strength — the applied SPRT and a fixed match against the
  defaults — and keeps coordinate movement as evidence. Symmetry ran as
  10.9v on 2026-09-21 and is recorded above; (ab) is complete.
  **Release order from here** (corrected 10.9w): (aa) landed as 10.9q and
  (w) closed as rejected on 2026-09-19, so what remains before (j) is
  (af) to (ai), GUIDE 10.9w–10.9z. Item (r), placement per platform, was
  deferred behind the release by the maintainer (GUIDE 10.9h), with the
  limitation to be stated in the release notes, since the reference
  platform is Windows.
- **(ac) Harness time that belongs to neither clock.** The same tune missed
  its predicted 4,860 games per hour by 7%, and not through the engines:
  moves per game (122) and charged time per game (8.6 s) were identical to
  the 14-slot run, and harness overhead inside the charged interval was
  unchanged (`h=` median 1 ms, 99th percentile 21 ms). What grew is the
  time per game outside both clocks, slot span minus charged time: 0.30 s at
  14 pair-held slots, 1.22 s at 15 per-game slots, 2.5 against 10 ms per
  move. It costs no fairness and 9% of throughput. Established so far: it
  grows with concurrency in short runs (6.9, 12.9 and 21.5 ms per move at
  8, 14 and 15 slots); raising the harness's priority class changes nothing
  (15.4 against 14.2), so it is not CPU scheduling; one engine reaches
  `readyok` in 18 ms alone and in 190 ms inside a burst of thirty, and since
  the shared-book fix every wave starts thirty processes in the same
  millisecond, which explains part of it and not all. Required: the journal
  records, per game, start-up (slot taken to first `go`), play (first `go`
  to last `bestmove`), the uncharged part of play, and teardown (last
  `bestmove` to both processes exited); `stats` reports their distribution;
  then the largest phase is reduced and the measurement repeated. The
  candidate mechanism, shared with (w), is what fastchess does: keep each
  slot's two engine processes alive between games (`ucinewgame`, options
  re-sent when they change, as an SPSA iteration does), restarting on any
  fault, on request, and always between different executables, with fresh
  processes per game remaining selectable for crash isolation. Success
  criterion on the reference host: uncharged time under 0.3 s per game at
  15 slots, and no change in the `h=` distribution or the fault rate.

  **Implementation evidence (Phase 10.9s):** `run_game` stamps the game's
  start, each search's start and return, the end of play and both engines'
  exit, and the clock accounting carries `phases` (`GamePhases`): start-up,
  play, charged, uncharged play and its three parts (the runner's work
  between searches, the `position` written before each `go`, a `bestmove`'s
  arrival to its search returning), and teardown. The journal records them
  per game and `stats` on a run directory or journal reports each phase's
  mean, p50, p90, p99 and maximum, with `outside-runner` (slot span less the
  three) beside them. The instrument named the phase at once: 30 games of
  b22core against b22base at 3+0.03 on 15 slots gave start-up mean 1,537 ms
  (the fifteen games of the first wave 2,661–2,835 ms each, later single
  starts 171–557 ms), uncharged play 16 ms and teardown 10 ms per game. A
  bare start of the same engine outside the harness took 25 ms alone and
  218–224 ms median in a burst of thirty, so the harness, not the engine,
  held most of it. Stamps inside the start then put 0.5–0.8 s of each
  engine's start in applying its CPU affinity, and none in process
  creation, the job object, resume or the reader thread (under 0.1 s
  together): the check that the engine's threads sit in the requested
  processor group took a system-wide thread snapshot
  (`CreateToolhelp32Snapshot`) per engine, synchronously on a runtime
  worker, so thirty at once serialised and held up other games' handshakes
  (half the handshakes read 0 ms, the other half 0.7–1.0 s). The check now
  asks the process itself (`GetProcessGroupAffinity`, which the allowed-CPU
  detection already used for the harness's own process); the requirement
  is unchanged. The same burst after the change: affinity 0.0 ms, start-up
  211–222 ms mean, which is the bare engines' own figure. On the real
  82-knob tune (3 iterations of 30 games at 15 slots, 1+0.01, 20 ms margin,
  seed 11), uncharged time per game from slot span less charged time fell
  from median 1,537 ms, mean 1,417 ms to median 215 ms, mean 236 ms, and
  the three iterations from about 30.6 s to 21.7 s. Test: a fixture engine
  that waits before `uciok` and after `quit` puts those delays in start-up
  and teardown, play equals charged plus uncharged exactly, and `stats`
  reports all nine phases; with the exit stamp moved before the quits the
  test fails on teardown (0.8 ms against 120 ms injected). Persistent engine
  processes, the candidate, were not built: the remaining start-up is the
  operating system's own cost of starting two processes, about 0.2 s of a
  9 s game, and the target is met without giving up per-game isolation.
  They stay a discriminating test of (w). **Confirmed 2026-09-18** by the
  maintainer's tune at 15 slots, 30 games per iteration, 3+0.03 and a 20 ms
  margin, otherwise the 15-slot run of (aa) repeated (same engine, tune,
  seed 1530, book and schedule): uncharged time per game from slot span less
  charged time median 231 ms, mean 241 ms, max 934 ms against 1,378, 1,224
  and 2,792 ms; start-up mean 150 ms, p99 285 ms; uncharged play 19 ms and
  teardown 12 ms per game; 22.0 s per iteration and 4,910 games per hour
  against 23.9 s and 4,517; `h=` p50 1 ms and p99 21 ms per arm in both
  runs, p999 139–159 ms against 142–147 ms, maximum 597 against 584 ms; 0
  time losses and 0 other faults in 1,800 games, now at a 20 ms margin; no
  overlapping slot span. About 1% of moves (1,137 and 1,132 per arm of some
  108,500) carried an overhead above the 20 ms margin and forfeited nothing,
  since none of them spent the whole clock; the same overhead tail was
  present before this step and is evidence for (w), where `h=` also holds
  the engine's own time after its last reported `time`.
- **(ad) Overlapped SPSA iterations, a recorded mode that is off until its
  evidence is in.** The iteration tail is the last structural loss: 17% of
  slot-time at 30 games on 15 slots, because the next iteration may not
  start until the last game of this one has returned. It exists only in
  `spsa`; `match`, `sprt`, `calibrate` and `tournament` already hand every
  freed slot to the next unit for the whole run and have nothing to gain.
  The mode: when a slot frees and the current iteration has no game left to
  launch, launch the next iteration's games at once, perturbed around the
  newest committed centre, which is one update behind; updates are applied
  strictly in iteration order from whole mini-matches; staleness is bounded
  at one update and recorded per iteration; `--overlap 1`, default 0;
  resume replays whole iterations as today. Why the cost should be small:
  one update moves a centre by `c × r × score`, a few percent of the
  perturbation `c` early in a tune and about one percent late, so a gradient
  measured around the previous centre is measured a small fraction of the
  probe distance from where it is applied; fishtest tunes this way with far
  larger staleness. Why that is not yet evidence here: fit quality is the
  purpose of a tune and outranks throughput. Required before the default
  may change: (1) a zero-game study on noisy synthetic objectives (separable
  and coupled quadratics, 80 parameters, the real schedule and mini-match
  noise), synchronous against overlapped over many seeds, reporting final
  distance to the optimum and its spread; (2) the recovery test of (ab) run
  in both modes from the same detuned start and budget, each gated by
  `sprt --apply`; adopt as the tuning default only if overlapped recovers no
  worse in both. Expected gain: occupancy from 83% towards 97%, about 15%
  more games per hour. Items (k) to (ad) precede (j), with (ad) allowed to
  follow the release if its evidence is not in.

  **Implementation evidence (Phase 10.9n):** `play_mini_match` now takes a
  slot from the run's pool per game: games are launched in schedule order
  (`2p − 1`, `2p`, … across the iteration's pairs), each on the lowest free
  slot, through `match_runner::play_pair_game`, which builds a game from its
  number exactly as `play_pair` does (colours, opening, identity, arms); the
  pair's second game no longer waits for its first. Returning games are
  held by pair identity until both halves are in, then handed to the same
  `PairCommitQueue`, so the iteration still commits atomically, the score
  and gradient still use whole pairs in pair order, the fault decision is
  still taken on the whole mini-match, and a stop still replays the whole
  interrupted iteration. The per-slot invariants of (u) hold per game (debug
  assertions on take, give-back and held count; a slot returns only when
  `run_game` has returned after both engine processes exited). `sprt` keeps
  `play_pair`, now two calls of `play_pair_game` on its one slot, with the
  pair as its unit. `spsa plan` and `spsa --dry-run` report
  `SpsaWaveShape` for the resolved concurrency: games per wave, waves, games
  and idle slots in the last wave, expected occupancy (games over
  slot-waves), and a warning naming the even multiples of the slot count
  between half and twice the request (14 slots, 32 games: 28, 42, 56; 15
  slots: 30, 60); the wall-time estimate already counted game waves and now
  matches the scheduler it describes. Test: `phase10_slot_pool` runs two
  iterations of five pairs on four slots with uneven stub games and asserts
  the per-slot invariants, launch order, whole pairs per iteration, at
  least two pairs whose games ran at once on different slots, and occupancy
  of at least 60% (measured 78–88% over three runs). With the pair-held
  scheduler restored it failed two runs of two, on launch order (game 3
  before game 2) — pair-held occupancy measured 71–76%, so on this stub the
  occupancy bound alone does not separate the two; the order and
  concurrency assertions do. Unit tests cover the wave arithmetic.
  Deviation: the dry run carries the shape as an optional `wave_shape`
  field of its document and prints no separate warning line, because its
  human form is that document; a live run prints none, so its standard
  error stays progress only. Owed and maintainer-run: a tune at 14 slots and
  42 games per iteration, or 15 and 30, above 85% occupancy and 5,000 games
  per hour. **Run, and short of both thresholds (recorded 10.9w):** the
  2026-09-18 60-iteration tune at 15 slots and 30 games per iteration
  measured 83% occupancy against 81% modelled and 4,517 games per hour, and
  10.9r's 1,500-iteration recovery tune at the same shape measured 4,913
  games per hour. Both are well above the 2,745 games per hour before this
  item and above weather-factory's 3,720 on the same surface, and neither
  reaches 85% and 5,000. Whether to chase the remaining margin — (z), two
  games per physical core, and (ad), overlapped iterations, are the two
  candidates, both deferred behind the release — is a maintainer decision;
  nothing in the release depends on it.

  **Implementation evidence (Phase 10.9l):** `FaultPolicy` gained an
  optional `rate` (`FaultRate`: per mille, and which of the engine-fault and
  time-loss limits grow) and three methods, `engine_limit(games)`,
  `time_limit(games)` and `exceeded(faults, games)`, through which every
  check now passes. `FaultPolicy::sequential(engine, time)` is the policy of
  `sprt` and `spsa`: an omitted engine-fault limit grows as
  max(3, ⌊games × 5 / 1000⌋); an omitted time-loss limit follows it, because a
  time loss is an engine fault; a limit given explicitly stays fixed, so `0`
  invalidates on the first fault of its kind (and an explicit engine limit
  also fixes the defaulted time limit). `match`, `calibrate` and `tournament`
  keep their fixed 10.9g allowance. `sprt` already scored a forfeit as a
  result and kept its pair in the official sample; it now evaluates the
  allowance at each admitted pair over the games of the official sample so
  far, this pair's included, and marks the pair invalid only past it. `spsa`
  gained `--max-engine-faults` and `--max-time-losses`; the driver sums faults
  over every committed iteration (a resumed tune's included) and, within the
  allowance, commits an iteration containing forfeits with its score and
  gradient like any other; the iteration that crosses the limit is kept as
  invalid evidence as before. A rebuilt iteration may now hold engine
  faults; only an unscorable game is refused on resume. Every progress block
  and final report states the count, its rate over the games played and the
  allowance at that point, for example `2 in 812 games (0.25%); 4 allowed
  (0.5% of games played, at least 3)`; the policy is in the resolved
  configuration and the report. Tests: the policy arithmetic (floor, rate,
  boundaries, strict and time-only explicit limits); an SPRT against a
  fixture that forfeits every game becomes invalid at pair 2 (four faults
  over four games) and at pair 1 with `--max-engine-faults 0`; an SPSA tune
  with the same fixture commits iteration 0 with its two forfeits scored and
  becomes invalid at iteration 1; the two acceptance tests that assert
  invalidation on the first fault now pass `--max-engine-faults 0`.
  Deviation: the fault policy's serialized shape changed, so an `sprt` or
  `spsa` run directory from before this step no longer matches its resolved
  configuration on resume and must be restarted.

  **Implementation evidence (Phase 10.9k):** the execution plan hands each
  run a `SlotPool` (`MatchExecutionPlan::slot_pool`), and `match`
  (and so `calibrate`), `sprt`, `spsa` and `tournament` take the
  lowest-numbered free slot before spawning a unit and give it back when the
  unit's worker returns. A worker returns after `run_game`, which returns
  only once both engine processes have exited: the normal path quits and
  waits, and the setup-failure path now kills and reaps an engine that had
  spawned instead of dropping it. A pair (`sprt`, `spsa`) holds one slot for
  both games. Launch order, pair identity, openings, commit order and the
  official prefix are untouched; only which slot a unit is placed on
  changed. `plan_execution` and `slot_pool` refuse a plan whose slot count
  differs from its concurrency (`SlotCountMismatch`). Debug builds assert
  that a slot handed out is free, that a slot given back was held, and that
  the held count equals the live units after each launch. Every game records
  `slot: {index, started_unix_us, ended_unix_us}` in its journal record and
  report (from before the first spawn to after both exits), `GameSlot` in
  its PGN and `slot:` in its forensic. Tests: a pool unit test drives units
  finishing in arbitrary order and checks that no slot is held twice and
  none waits while a slot is free, plus the debug assertion and the
  slot-count refusal; `phase10_slot_pool` runs `match` (32 games), `sprt`
  (16 pairs), `spsa` (2 iterations of 6 pairs) and a tournament (3 engines,
  4 games per pair) at concurrency 4 with stub engines whose per-process
  delay factor makes games uneven, and checks from the journal that no two
  spans on one slot overlap, that every slot is used, that a pair's games
  share a slot, and that the PGN tag matches. With the modulo rule restored
  in `match_runner` the match case failed three runs of three (game 5 put on
  slot 0 while game 1 still ran), and in `sprt_runner` two of two.

  The corrections owed from review of (t) landed with it. A `ponderhit` or
  `stop` search is marked `earlier_origin`: the engine's clock started at
  an earlier command, so neither overhead nor last-`info` lag is computed
  and `h=` is omitted for it. `h=` is now the journal's nanosecond overhead
  rounded to the nearest millisecond (halves away from zero), so a game's
  largest `h=` per side equals its journal maximum rounded; `t=` stays
  truncated, so `h` and `t − engine time` agree to within one. A search that
  lost on time is written before the result as `{forfeit t=…ms h=…ms}`, or
  `{forfeit}` when no answer came, and `stats` counts it towards the side
  that forfeited in the overhead distribution and over-margin count and in
  nothing else (`colosseum-move-comment/3`). The late-`bestmove` wait
  continues past a per-read protocol fault; the regression puts an
  over-long line just before a late answer and still times the answer. An
  SPSA resume over a rebuilt iteration holding an unscorable or faulted game
  now names the iteration and the game (`UnusableJournalGame`) and gives the
  `--restart` remedy. Deviations: the pool is created per run by the
  execution plan rather than stored inside it, since the plan is a
  serialized report; the slot spans use wall-clock microseconds so a run's
  journal can be read as a timeline, which a clock step during the run
  would distort; the stub's unevenness comes from its process identifier,
  so the lengths are uneven but not reproducible run to run; the
  real-engine smoke test's `GameSpec` literals, missing `identity` since an
  earlier step, were repaired. Owed and maintainer-run: the 2,000-game
  3+0.03 match at 14 slots with zero time losses and every game core
  continuously busy.

  **Implementation evidence (Phase 10.9j):** `colosseum-uci` keeps a
  `SearchTiming` per search: the game task stamps `go` before the write, the
  write's return, and the moment it takes each line off the reader's
  channel; the reader thread's existing arrival stamp times the first and
  last `info` and the `bestmove`; the last reported `time` and the `time` on
  the last `info` are kept with them. Nothing was added to the reader
  thread, and the game task only copies `Instant`s. `ponderhit` and `stop`
  are timed the same way; a ponder that finished early has no round trip
  and no timing. The stamps survive a missed deadline: a line that arrived
  after it is kept, and the runner then waits up to one second more
  (`LATE_BESTMOVE_WINDOW`), only to learn when the `bestmove` came; the
  result is already decided. The runner keeps per side the maximum of five
  phases — `go` write, to first `info`, first to last `info`, last `info` to
  `bestmove` arrival, arrival to consumption — plus the last `info`'s lag
  behind the time it reported and the overhead (charged minus reported
  time); the maxima travel in the clock accounting, so the journal record
  carries `white_round_trip` / `black_round_trip`. The forfeited search
  counts, with its late arrival. Every abnormal-game forensic now begins
  with a table of each side's last five searches in milliseconds after the
  `go` stamp, `!` marking a late answer and `never` a missing one. The PGN
  comment gains `h=<ms>ms` beside `t=` (`colosseum-move-comment/2`), and
  every game carries `WhiteTimeMarginMs` / `BlackTimeMarginMs`; `stats`
  reads `h=` back into a per-engine p50/p99/p999/max distribution and the
  count of moves whose overhead exceeded that side's margin. Tests: the
  fixture engine gained per-phase delays (`--first-info-ms`,
  `--between-info-ms`, `--bestmove-after-info-ms`, `--report-time-ms`); a
  session test injects 30/40/50 ms into the three engine-side phases and
  blocks the game task's only thread while the answer arrives, and each
  delay lands in its own phase with the consumption delay uncharged; a
  match journals the same maxima for the delayed side only, writes `h =
  t − 35` on every delayed move and `stats` reports every one of those moves
  over a 20 ms margin; a forfeit forensic shows the late answer at its real
  arrival. The PGN writer and the parser round-trip `h=` including negative
  values. Deviations: the forensic prints the last five searches of each
  side rather than the last five plies, so the forfeiting side always shows
  five; the margin tags are new, because an over-margin count from a PGN
  alone needs the margin; the `go` write phase is measured but not injected
  in a test, because a pipe write blocks only on a full OS buffer, which no
  portable test controls; the shared runner gives GUI games the same `h=`,
  tags and forensic tables. Owed and maintainer-run: the 2,000-game 3+0.03
  match at 14 slots that names the phase.

  **Implementation evidence (Phase 10.9g):** the `bestmove` instant is now
  the pipe's. A dedicated OS thread per engine owns its standard output,
  reads it line by line with the existing length bound, and stamps each
  line with `Instant::now()` the moment it is read off the pipe; the
  session receives `(text, arrived)` over a channel and charges
  `arrived − start`, where `start` is stamped before the `go` (or `stop`,
  or `ponderhit`) is written. A deadline is judged by arrival too: a line
  that arrived in time is in time however late the game task reads it. The
  clock model is versioned `go-write-to-bestmove-arrival` 2, because the
  start stamp moved from after the write to before it (a reader thread can
  otherwise stamp a reply before the harness finished flushing the
  command). The shared UCI session carries this, so GUI games are charged
  the same way. A run directory now has four files with one job each.
  `games.jsonl` holds one line per game — line version, sequence number,
  the game record (identity, result, termination, fault, sample class,
  clock accounting, iteration or round), the offset, length and SHA-256 of
  its moves in `games.pgn`, and a SHA-256 over the line's canonical JSON —
  and never PGN text. `games.pgn` is opened for append and written one
  game at a time, moves before the journal line. `checkpoint.json`
  (`CHECKPOINT_SCHEMA_VERSION` 2, two generations) holds aggregates only
  and a `journal` anchor: byte offset, SHA-256 of the covered bytes, record
  count and the PGN offset. `run.log` holds progress blocks, faults, stop,
  resume and finish; the per-game event is gone. One writer per run
  (`RunWriter`) runs on `spawn_blocking` and does every append, sync,
  checkpoint, rename and final artifact; the game loop hands it a record
  and the moves over a channel and returns, and a driver strips a game's
  moves as soon as the observer has handed them over, so in-memory state
  holds summaries. Appends are unsynced; the three append-only files are
  synced together every 50 games or 1 second and at every checkpoint and
  barrier. Checkpoints come every 50 units or 5 seconds and at every stop.
  Resume loads the newest valid checkpoint, hashes the journal up to its
  offset and refuses on mismatch, reads the tail line by line, drops a torn
  last line, drops tail games whose moves are not in `games.pgn` with the
  recorded hash, truncates the PGN to the last kept game, and replays the
  kept records: the SPRT official prefix, a tune's iteration boundaries and
  each command's aggregates are recomputed from them. `status` adds the
  checkpoint's aggregates and the journal read-only (games, how many the
  checkpoint covers, what a resume would drop, or the refusal); `stats`
  on a run directory reads the journal. `auto` headroom now sorts the
  eligible cores by their lowest logical CPU and leaves the lowest ones
  free, so CPU 0 is never a game core; the placement unit tests and both
  headroom fixtures of the recorded topology corpus moved by one core.
  `match`, `calibrate` and `tournament` default `--max-engine-faults` to 1%
  of the scheduled games and at least 5, `--max-time-losses` defaults to
  the engine-fault limit because a time loss is an engine fault, and
  `calibrate` classifies `invalid` only past that allowance; `sprt` keeps
  zero and `spsa` still invalidates on any fault. Every progress block's
  and final report's `faults` line states the allowance. Tests: a
  30,000-game commit through the match observer into a real run directory
  measured a median observer call of 900 ns at both ends, 365 ms and 371 ms
  per 1,000 games including the writer and its syncs, and a checkpoint of
  483 then 497 bytes; a hard kill of a real match followed by a torn last
  journal line and a PGN cut inside the previous game resumes to the
  uninterrupted result with the durable journal bytes unchanged; one
  changed byte inside the covered journal is refused by resume and
  reported by `status` without either changing the directory. The Phase
  4B oracle fixtures, the 10.9d/10.9e run-dir-versus-PGN equality tests
  and the kill/resume suite pass unchanged in what they assert; their
  waits now watch the journal rather than the checkpoint. Deviations: the
  run configuration, the run record's first write and the resume's
  journal load happen before any game starts, synchronously for the first
  two, since nothing is being charged yet; K is an internal constant
  rather than a flag; a run directory written before this step is refused
  on resume with a request to `--restart`, since its checkpoint names no
  journal; the Phase 4C `fault-invalidity` gate in
  `docs/fixtures/phase4c/acceptance.json` now names the allowance test that
  replaced the any-fault test; `RUN_RECORD_SCHEMA_VERSION` is unchanged because the record's
  shape is. Owed and maintainer-run: the real-host scramble probe (a
  2,000-game 3+0.03 match, no move with under 100 ms remaining charged
  above the engine's reported time plus 20 ms) and the fixed-movetime
  outlier probe (100 ms, 14 slots, 50,000 moves, at or below fastchess).

  **Implementation evidence (Phase 10.9f):** one `ProgressBlock` type carries
  every report: a command, its unit count against the cap, the elapsed time
  and labelled lines. The same value is rendered to standard error, appended
  to `run.log` as a `progress` event and retained in the run record
  (`RUN_RECORD_SCHEMA_VERSION` 5), so `status` prints exactly what the console
  last showed rather than a second account of the run. The block is what the
  contract lists per command; the figures come from the estimators the final
  result already uses — `pentanomial_statistics` for both Elo models,
  `pentanomial_sprt` for the LLR and its Wald bounds, `elo_with_error` for an
  unpaired match, and `RateTournament::execute_with_fixed_field` on the games
  committed so far for the standings header. The one derived figure is the
  SPRT's expected remaining games, which extrapolates the computed LLR at its
  average drift per pair, capped at the games left to `--max-pairs` and
  labelled as the current drift because a sequential path is not a line.

  `ProgressSchedule` owns the decision: a block is due at `--progress-every`
  units past the last one, and the `--progress-min-secs` floor can only
  withhold it, never schedule it. A run polls that decision four times a
  second, which is finer than the smallest permitted floor, so the floor and
  not the polling is what bounds how close two blocks may be. A resumed run
  counts from the units it inherited, so no rate or ETA claims time it did not
  spend, and a final block that would repeat the last boundary block is not
  printed twice. A block reads as the operator's own summary: who is playing,
  the sample size, each Elo estimate as a value and the half-width of its 95%
  interval, W/D/L, `Ptnml`, faults, the LLR against its bounds, the rate and
  the time remaining, closed by a rule. The closing SPRT report states the
  hypotheses as an interval, names what was accepted rather than only which
  hypothesis it was, and ends with the invocation's total time. Neither flag reaches the resolved configuration, so an
  existing run directory resumes whatever it is told to report. Machine mode
  prints blocks too, and the tests that read "a quiet run says nothing on
  standard error" now read "nothing but progress".

  **Implementation evidence (Phase 10.9e):** the abandoned game was the one
  game every driver already refused to score and no writer said so. It now
  carries `[ColosseumSample "unscorable"]` from `match` and `calibrate`, from
  `tournament`, and per game inside an SPRT or SPSA pair, where a class that
  describes the group cannot describe one abandoned member of it. The class
  is per game deliberately: an infrastructure fault is a fact about that
  game, not about the pair it was scheduled in. The replay reports what it
  left out — `excluded_games` with an `excluded_by_sample` breakdown — rather
  than dropping it silently, and the structured reader counts its own
  unscorable games the same way, so a run directory and its PGN agree on the
  exclusions as well as on the sample. A gauntlet whose third engine does not
  exist plays one encounter out and abandons the first game of the next, which
  is a run with exactly one aborted game and no timing dependence.

  Both versioned SPSA artifacts deny unknown fields, and deserializing before
  asking for the version made the second rule answer for the first: a stale
  file was reported as "unknown field `stats_version`" and the version that
  renamed the field was never named. Reading `schema_version` on its own,
  before the fields, keeps the refusal reachable for the stored schedule and
  for a tune result offered to `sprt --apply`. `SPSA_PLAN_SCHEMA_VERSION` is 2,
  which 10.9d should have bumped with the field it renamed, and the `rng.rs`
  sampling comment no longer ties the draws to `stats_version`.

  **Implementation evidence (Phase 10.9d):** all four defects were the same
  mistake — the identity was written and the reader guessed anyway. `stats`
  now groups games by `PairNumber` together with the unit its `PairGame`
  falls in, so an encounter of four games is two pentanomial units and an
  encounter of one game is none, and the colour inversion follows every even
  assignment instead of a hard-coded `2`. The structured reader needed the
  same correction: a tournament game names its participants by identity
  rather than by side letter, so before this it scored every game from
  White's perspective and a checkpoint disagreed with its own PGN. Games a
  run keeps but does not count now carry `[ColosseumSample "post-terminal"]`
  or `"invalid"` inside the header — they were marked with a comment line
  above the game, which a PGN reader attributes to the previous game, so the
  marker existed but no reader could act on it. A file in which nothing is
  official is refused with that reason rather than replayed. Tournament games
  no longer have their identity patched into the rendered text: the driver
  supplies the encounter, the assignment and the opening it actually chose to
  the one-game match, which is why `OpeningIndex` was `0` everywhere —
  `select_encounter` hands the inner match a single-entry book. The SPSA
  schedule artifact's field is `rng_version` (schema version 2, tune result
  schema version 3), and the randomness documentation and §5.4 no longer
  attribute the stream contract to `stats_version`.

  **Implementation evidence (Phase 10.9c):** each defect shared one shape,
  treating "a stop was asked for" as "the run did not finish". A match is now
  cancelled only when games are actually missing, and a schedule only when
  pairs remain below its cap, so a late interrupt cannot take away a verdict
  already earned. `suite` exits with the cancelled code whenever it records
  `cancelled`, like every other driver. `--stop-after-iteration N` is checked
  before an iteration is played, against the cumulative committed count, so
  repeating the command is a no-op rather than one more iteration each time.
  The stop grace period has an absolute deadline taken from the interrupt:
  drivers rebuild that future on every completed unit, and a relative delay
  meant a run with several slots never reached it. `--anchor` together with
  `--fixed` on one participant is refused at resolution, before a run
  directory exists, because the two say different things about one rating.
- **(ae) Persistent engine processes per slot**, by maintainer decision
  2026-09-19 after 10.9m: fastchess keeps its engines for a whole tournament,
  so an engine's one-time work (Rarog's KPK bitbase, 33–37 ms on first use) is
  paid once, where Colosseum's fresh processes paid it inside a game's search
  in most games; and a fresh process costs its slot about 1% of its time
  (start-up 61 ms of a 9 s game at 3+0.03). Crash isolation is kept by
  replacing an engine after any fault.

  **Implementation evidence (Phase 10.9u):** `run_game_keeping` in the
  runner takes a slot's kept engines (white, black) and returns the ones fit
  to keep: an engine that faulted in the game, or does not answer `isready`
  within 2 s after it (a running ponder stopped first), is quit, and nothing
  is kept after an infrastructure failure. A kept engine fits the next game
  when its executable, arguments, directory, environment, CPUs and option
  names are unchanged; it then receives only the options whose values
  changed (buttons again), `isready`, `ucinewgame` and `isready` — a fresh
  engine's setup less its handshake — so an SPSA arm gets its perturbed
  knobs every iteration and an unchanged `Hash` is never resent to
  reallocate; one that does not fit is quit and a fresh process started. The
  CLI's `SlotEngines` holds each slot's engines by side (A, B), taken and
  put back by the game on the slot, which the slot pool guarantees is the
  only one; `run_fixed_match`, `sprt` and `spsa` quit every kept engine when
  the run ends, and a killed harness takes them down through their
  kill-on-close jobs. `--engine-processes per-slot` is the default for
  `match`, `calibrate`, `sprt` and `spsa` and is recorded in the resolved
  configuration; `per-game` restores two fresh processes per game.
  Tournaments always use fresh processes. Tests, reading the fixture
  engine's own command log: six games at one slot ran one process per side,
  announced six new games and sent `Hash` once (six processes and six with
  `per-game`, and with the store disabled the test fails with six); a side
  that crashes in every game is replaced four times in four games while its
  opponent plays all four in one process; a kept engine is reaped when the
  harness is killed. The fresh-process kill test and the teardown phase test
  pass `per-game`, which is what they test. Real smoke at 15 slots on Rarog:
  a 60-game match, start-up median 8.3 ms (220–258 ms for a slot's first
  game), teardown 0.1 ms, no fault, no Rarog process left after the run; a
  3-iteration, 82-knob tune the same.
- **(af) Corrections from the adoption audit** (GUIDE 10.9w, Sol High), by
  maintainer decision 2026-09-21 after Rarog audited the harness it is about
  to adopt (Rarog `analysis/b2_audit_2026-09-21.md`). The audit recounted the
  symmetry and scale runs from their PGNs to the recorded figures and found
  no defect in the harness; what it found is release preparation:
  - The recorded Phase 8.1 parity command no longer runs, because
    adjudication is off unless asked for. Repair the record so the command
    that is written is the command that runs (`repeat_command` in
    `docs/fixtures/phase8/parity.json` carries `--draw-adjudication`), and
    prove it by running it.
  - An installed GUI 1.0.2 cannot see a `gui-v` release: its updater reads
    `releases/latest` and parses the tag with `trim_start_matches('v')`, so
    `gui-v1.1.0` yields no version and no notice. **Accepted by maintainer
    decision 2026-09-21:** the application has no user base to strand and
    the updater is hidden, so no bridge release, no legacy tag and no test
    for the old parser. One sentence in the GUI changelog tells a 1.0.2 user
    to download the new version by hand. The updater on `cli` reads the
    release list and accepts both forms; nothing here changes it.
  - The `cli` branch carries GUI source changes since 1.0.2 (the pinned
    field, the resignation policy, the application boundary, the updater):
    12 files, about 600 lines. Merging `cli` to `main` therefore changes the
    GUI that `main` builds. List them, and say which are user-visible, so
    (ag) can version the GUI.
  - Qualification §4's per-coordinate criterion is recorded as missed. Keep
    it so; add nothing that softens it.
  - Every binary in the qualification runs predates Rarog's start-up fix, so
    no run exercises fresh processes per game on a fixed engine. One optional
    maintainer-run `match --engine-processes per-game` of 2,000 games on a
    post-fix Rarog pair is offered as a command, not required, and does not
    block the release. It is the qualification's scale command with the
    post-fix pair and fresh processes per game, written for PowerShell and
    run from `D:\code\rarog\tools\results`, so the run directory lands beside
    the other `colosseum-qual-*` ones (dry-run from there, exit 0, 2026-09-22):

    ```text
    D:\code\colosseum\target\release\colosseum-cli.exe match `
      D:\code\rarog\tools\test_engines\rarog-startupfix-core-pext-pgo.exe `
      D:\code\rarog\tools\test_engines\rarog-startupfix-base-pext-pgo.exe `
      --a-label startupfix-core --b-label startupfix-base --games 2000 `
      --engine-processes per-game `
      --a-base-ms 3000 --a-increment-ms 30 --b-base-ms 3000 --b-increment-ms 30 `
      --a-option Hash=64 --a-option Threads=1 `
      --b-option Hash=64 --b-option Threads=1 `
      --a-margin-ms 20 --b-margin-ms 20 `
      --book D:\code\rarog\tools\books\UHO_Lichess_4852_v1.epd `
      --book-order random --seed 48 --placement auto --concurrency 14 `
      --dir colosseum-freshproc-postfix
    ```

    What it would answer: whether fresh processes per game still cost a
    forfeit on an engine that no longer does its one-time work inside a
    search. Read from the run directory: time losses (expect 0), `stats`
    start-up and teardown per game against the scale run's 7.3 ms and 0.1 ms
    with kept engines, and the overhead distribution. Nothing in the release
    depends on the answer; `per-slot` stays the default either way.

    **Run by the maintainer 2026-09-22.** 2,000 games in 21m46s at 5,512
    games per hour, +61.8 ± 11.1 Elo, ptnml [37, 151, 354, 339, 119],
    **zero time losses and zero faults of any kind**, no abnormal game. So
    fresh processes per game cost no forfeit on an engine that has moved its
    one-time work out of the search: the answer the item was asked for, and
    `per-game` is a safe explicit choice. What they do cost is start-up:
    `stats` reports 128.2 ms mean and 121.8 ms median per game (max 867.1 ms)
    against 7.3 ms median with engines kept per slot, and 7.9 ms teardown
    against 0.1 ms — about 1.4% of a nine-second game, which is why
    `per-slot` remains the default. The figure is higher than the 42.0 ms the
    10.9m baseline measured with fresh processes on the pre-fix binaries, and
    for a coherent reason rather than noise: Rarog's fix moved its KPK
    bitbase out of the first search and into process start-up, so a run that
    starts 4,000 processes pays it 2,000 times instead of hiding it inside a
    game. The two runs use different binaries, so that is an explanation, not
    a controlled comparison. Overhead stayed bounded (p50 1 ms, p99 18 and
    27 ms per side, max 0.6 s); 2,835 of 253,911 moves exceeded the 20 ms
    margin without any of them reaching a forfeit.
  - Sweep for anything else unfinished: every `☐`, `◐`, `TODO`, `FIXME` and
    "owed" in GUIDE, PLAN, `docs/` and the CLI crates is either done, deferred
    with its reason, or listed for the maintainer. Tracker text that has gone
    stale is corrected.

  **Implementation evidence (Phase 10.9w) — the parity command.** The record
  in `docs/fixtures/phase8/parity.json` now carries `--draw-adjudication`
  beside its draw parameters and is the command that runs; `command_note`
  says why the 2026-08-03 form differs and forbids dropping the draw
  parameters, and a test parses the recorded command against the shipped
  clap surface, so the two cannot drift apart again. Proved by running it on
  2026-09-21: colosseum-cli 0.1.0 built from `cli` at `6fd0150`,
  `rarog-v2.3.1-windows-pext-pgo.exe` (sha256 `033f6633…`) on both arms,
  exit 4, `inconclusive`, 8 games, 0/8/0, 4 complete pairs, pentanomial
  `[0, 0, 4, 0, 0]`, 0 faults — every shared field equal to the recorded
  observation. The pre-correction form exits 2. Noted for (j): that proof
  used a different build of Rarog 2.3.1 from the one the recorded artifact
  hashes belong to (`2a95390d…`), which is not in `D:\chess\engines\rarog`.

  **Implementation evidence (Phase 10.9w) — the GUI source since 1.0.2.**
  `git diff main..cli -- crates/colosseum-gui`: 12 files, 600 insertions, 38
  deletions, from six commits. What a user of the GUI can see is marked
  **user-visible**; the rest is internal.

  | Change | Kind | Commit |
  |---|---|---|
  | `colosseum --version` prints the product version and exits | **user-visible** | `9c3a03f` |
  | Update check reads the whole release list, takes the newest stable `gui-v` tag and accepts the legacy `v` form only up to 1.0.2; the fallback link is the releases page, not `/releases/latest` | **user-visible** | `f3a5bbf` |
  | Standings CSV export carries the `Fixed` column, `no` on every GUI row (the GUI has no fixed field) | **user-visible** | `01254ed` |
  | GUI package pinned to its own `1.0.2` instead of the workspace version, and `publish = false` | internal (release mechanics) | `9c3a03f` |
  | `AppDirs`, `AppConfig` and `EngineLibrary` moved from `colosseum-engine` into the GUI crate with their own `ConfigError`; paths, file names and formats unchanged | internal | `d2fdf4c` |
  | Product identity (`DISPLAY_NAME`, `APP_DIR_NAME`, `QUALIFIER`, `ORGANIZATION`) moved from `colosseum-core::branding` into the GUI's `product.rs`, same values | internal | `d2fdf4c` |
  | `runtime_adapter.rs`: library entry to `RuntimeParticipant`/`EngineLaunchSpec`, with tests that the launch spec leaks no library metadata; computed in `Backend::start_tournament` behind a `debug_assert` and not yet consumed — the seam Phase 11 consumes | internal | `d2fdf4c` |
  | Engine and tournament identifiers created by the caller (`EngineId::from_uuid(Uuid::new_v4())`) instead of the domain | internal | `d2fdf4c` |
  | The tournament form passes `two_sided: true` for resignation, which is the serde default and 1.0.2's behaviour | internal (behaviour-preserving) | `89d24a0` |
  | Live and export rows pass `fixed: false` | internal (the CSV column above is its visible half) | `01254ed` |
  | Build script gates `winresource` on the Windows *host* and the Windows *target* together, so the GUI builds on Unix hosts | internal | `4ab58ef` |
  | `colosseum-application`, `directories`, `thiserror`, `toml` and `uuid` added to the GUI's dependencies | internal | `d2fdf4c`, `9c3a03f` |

  **Implementation evidence (Phase 10.9w) — GUI behaviour arriving through
  the shared crates.** The GUI crate is only half the picture: `cli` also
  changes `colosseum-core` and `colosseum-engine`, which the GUI's scheduler,
  runner and store use. Two effects reach a GUI user and belong in the
  version decision and the changelog:
  - **user-visible.** Every PGN the GUI writes now carries per-move search
    comments (`{s= d= t= h= n=}`, `{book}` on pre-played moves) and the
    `OpeningPlyCount`, `WhiteTimeMarginMs` and `BlackTimeMarginMs` tags.
    Confirmed on the real run below.
  - **regression against 1.0.2, not fixed here.** A game whose engine cannot
    be spawned is classified as an infrastructure fault and reported as an
    unscorable draw (`Termination::Aborted`, `scorable: false`). The CLI
    honours `scorable` and excludes such a game; the GUI's scheduler does
    not read it, so it records the draw into standings and into the ML
    rating. In 1.0.2 the engine that failed to start lost both its games and
    the working engine won them. Reproduced by the existing GUI smoke test
    `failed_engine_loses_with_error`, which passes at `main` and fails on
    `cli` (`wins` 0, expected 2). The fix is a policy choice between 1.0.2's
    loss and the CLI's exclusion, and PLAN §Phase 11(b) is where fault
    classification reaches desktop tournaments, so it is left for the
    maintainer rather than decided here.

  **Implementation evidence (Phase 10.9w) — the GUI is buildable and runs
  games.** `cargo build --release -p colosseum-gui` succeeds and the binary
  reports `colosseum 1.0.2`; the workspace suite is green (589 tests). The
  app itself, run 2026-09-21 from a `--portable` copy so that no real user
  data was touched, played a short tournament: Basilisk 1.10.0 against Rarog
  2.4.0 from `D:\chess\engines`, round robin, two games at 100 ms per move.
  It finished in 37 s, 1.5–0.5 to Basilisk, terminations one `Checkmate` and
  one `50-move rule`, no forfeit and no engine error. The live view drew the
  board, move list, ECO opening, evaluation graph and both engines' depth,
  nodes and nps; standings, head-to-head, terminations and the tournament
  information panel all filled; the tournament was then deleted through the
  app. The GUI scheduler's real-engine smoke suite passes 6 of 7 against
  Rarog 2.4.0 (full round robin, openings, resume across restart,
  stop-drain-resume, force-stop); the seventh is the regression above. A
  headless four-game run through the same scheduler, runner and store
  confirmed the stored PGN carries the annotations above.
- **(ag) Versions and the tag contract** (GUIDE 10.9x, Sol High). Decide and
  record, then make `colosseum-release`, both workflows and the manifests
  agree:
  - **The CLI's first version is 0.1.0.** Its command surface, run-file
    schema, run-directory format and JSON reports have changed within
    Phase 10 (pre-10.9g directories are refused) and will change again when
    Phase 11 puts the GUI on the harness and when (z), (r) and (ad) land.
    1.0.0 is a promise that scripts and run directories keep working; it is
    made when those formats have survived a release unchanged. Adopters pin
    an archive by SHA-256 meanwhile.
  - **The GUI is released with the CLI, and gets its own version for the
    merged source**, from (af)'s list: a minor release if anything
    user-visible was added, otherwise a patch. The manifest, `CHANGELOG-GUI.md`
    and the updater's own version agree, and `colosseum-release` validates
    the tag.
  - **The GUI tag scheme is `gui-v<semver>`, as designed**; the legacy
    `v<semver>` form ends with 1.0.2 (maintainer decision 2026-09-21, see
    (af)). `colosseum-release` and `release-gui.yml` accept exactly that.
  - GitHub's repository-wide "latest" belongs to the stable GUI release; a
    CLI release never claims it (`make_latest: false`, already so). README
    links reach the newest CLI through the `cli-v` release list.
- **(ah) One build entry point** (GUIDE 10.9y, Sol High). A `cargo xtask`
  crate, as Rarog uses, replaces `build_windows.ps1`, `build_linux.sh` and
  `build_macos.sh`: `build gui|cli`, `package gui|cli` producing the archives
  and their `SHA256SUMS` exactly as the release workflows stage them, and
  `release-check <tag>` wrapping `colosseum-release` and the documentation
  drift gate. The two workflows call the same commands, so a local package
  and a published one come from one recipe. The products stay separate: no
  command builds or packages both into one artifact, and the CLI archive
  never contains the GUI. Exit: on Windows, `cargo xtask package cli` and
  `package gui` produce archives whose contents equal the candidate's file
  lists, and each passes its `Smoke-*Archive.ps1`.
- **(ai) User-facing documentation for the release** (GUIDE 10.9z, Terra
  High). `README.md` is the front door for both products: what each is for,
  how to download it (the GUI from the latest release, the CLI from the
  newest `cli-v` release, per platform, with checksum verification), how to
  start, a first tournament in the GUI, and a first `match`, `sprt` and `spsa`
  in the CLI with a run file, then links to `docs/cli/`. `README-CLI.md`, both
  changelogs, `docs/cli/` and `docs/DEVELOPMENT.md` (the xtask, the tag
  contract) agree with it. No phase numbers or internal method in user
  documents. Every link is checked, including the release links that exist
  only after the tags, which are listed for the maintainer to open after
  publication.
- **(j) Release acceptance repeat.** Regenerate the command reference, update
  `CHANGELOG-CLI.md` under 0.1.0, run the Phase 4B oracle replay and the
  Phase 8.1 parity matrix on the corrected source, repeat the short third-party
  usability flows, build a fresh four-platform CI candidate and pass exact
  archive smoke. The maintainer then merges `cli` to `main` and tags
  `cli-v0.1.0`.

**Decisions 2026-09-19 (maintainer):** (w) closed as rejected: its study
traced the forfeits to Rarog's lazily built KPK bitbase, fixed in Rarog, and
the 10,000-game target is not pursued; (ae) carries its one harness change.
(r) and (ad) follow the release. (ab)'s symmetry run is its own step, 10.9v,
run overnight when the machine is free and not blocking `cli-v0.1.0`; the
earlier −1.1 ± 3.9 nElo predates the slot pool and kept engines, so it does
not qualify the code being released. The rest of (ab) runs on the candidate
with kept engines, the default, because the scale and verdict figures on
record were measured with fresh processes per game.

**Exit criterion:** every item above demonstrated by its tests and fixtures;
oracle replay and parity matrix agree on shared fields; the candidate's four
archives pass smoke; documentation, changelog and generated reference are
consistent; the tag contract validates.

