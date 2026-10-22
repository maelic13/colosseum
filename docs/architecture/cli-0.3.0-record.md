# Colosseum CLI 0.3.0 record

Two defects a user reported against `colosseum-cli 0.2.0` (`ca05dfa`) while
moving training-data generation onto the harness, fixed on a branch from
`main`, 2026-10-14. Host: AMD Ryzen 9 5950X (16 cores, 32 threads), 128 GB,
Windows 11 Pro 10.0.26300. Engine on both sides: the reporter's engine build
(`E73E3230…`), 8,000 nodes, `Threads=1`,
`Hash=16`; book `phase_book_v1-s10403.order.epd` (`F0CF3E07…`), first 900
openings in order; no adjudication, 200-move cap, placement off. fastchess
1.8.0 alpha (`8444E739…`) as the reference.

## 1. The setup position was never counted for threefold repetition

**Cause.** `runner::play` counted a position after each book move and each
engine move, never the position the game starts from. Under the Laws of
Chess (Article 9.2) that position is the first occurrence, so a recurring
start position needed a fourth occurrence where it should need a third.
No other code keeps a repetition count: adjudication is score-based, the
PGN termination text comes from the runner's `Termination`, and the GUI's
live view hashes positions only for its ECO lookup.

**Fix.** The start position (EPD/FEN, or `startpos` before a PGN opening's
moves) is counted once before any move (`adf077c`).

**Evidence.** Benchmark games compared move by move with fastchess's
(`same_games.py`, keyed by start FEN): 0.2.0 differs in exactly 2 of 900, both
draws of 9 plies where fastchess has 8 (the first two cases of the report);
0.3.0 is identical in 900 of 900 at concurrency 30 and at 14, and its ply
total equals fastchess's (71,523; 0.2.0: 71,525).

**Tests** (`crates/colosseum-cli/tests/cli/threefold.rs`, through the real
game loop with the `colosseum-uci-fixture --script=` mode, which plays a
scripted line from a named start position):
`a_match_draws_on_the_third_occurrence_of_an_epd_start_position` (the five
reported FEN cases, both colours, 8 plies each),
`a_match_draws_on_the_third_occurrence_of_startpos_without_a_book`,
`the_position_before_a_pgn_openings_first_move_counts` (4 book plies + 4
engine plies) and
`a_tournament_draws_on_the_third_occurrence_of_an_epd_start_position`. All
four fail without the fix. Every command reaches the same `runner::play`.

## 2. Colosseum took about 36% more wall time than fastchess

**Reproduced** with the reporter's harness-speed script on the pinned 0.2.0
exe:
35.3 / 34.4 / 33.9 s against fastchess 25.9 / 25.9 / 24.8 s (medians 34.4 and
25.9 s); mean charged 8.53–8.61 ms a move, mean `h` 2.12–2.15 ms.

**Where the time went.** Every game's run record already carries its phases
(`clock.phases`). At concurrency 30 a 0.2.0 game spent 339.6 ms in start-up
(spawn, handshake, options, `isready`, `ucinewgame`), 768 ms in play and
18.9 ms in teardown: a tournament started two fresh processes for every game,
so each slot paid the engine's start-up about 30 times; fastchess keeps its
engines. Start-up is not only waiting: 30 slots starting engines at once
loaded the host the searches ran on, which is why the charged time per move
was 9.15 ms at concurrency 30 against 3.16 ms at 1.

Measured per build at concurrency 1, 14 and 30 (60, 420, 900 games; same
command as the benchmark; harness CPU is the `colosseum-cli` process's own):

| build | c | wall | harness CPU | start-up/game | position write/move | charged/move |
|---|---|---|---|---|---|---|
| 0.2.0 | 1 | 21.2 s | 3.8 s | 82.3 ms | 0.095 ms | 3.16 ms |
| A | 1 | 17.5 s | 3.3 s | 5.3 ms | 0.090 ms | 3.40 ms |
| B | 1 | 17.0 s | 2.7 s | 5.4 ms | 0.039 ms | 3.35 ms |
| fastchess | 1 | 18.5 s | 1.2 s | | | |
| 0.2.0 | 14 | 18.9 s | 36.0 s | 152.4 ms | 0.146 ms | 5.50 ms |
| A | 14 | 13.6 s | 21.4 s | 13.5 ms | 0.110 ms | 5.10 ms |
| B | 14 | 13.4 s | 17.4 s | 13.5 ms | 0.029 ms | 5.10 ms |
| fastchess | 14 | 14.3 s | 9.5 s | | | |
| 0.2.0 | 30 | 34.4 s | 74.8 s | 339.6 ms | 0.304 ms | 9.15 ms |
| A | 30 | 18.5 s | 42.4 s | 23.5 ms | 0.165 ms | 6.89 ms |
| B | 30 | 18.0 s | 31.5 s | 23.9 ms | 0.030 ms | 6.84 ms |
| fastchess | 30 | 24.9 s | 27.1 s | | | |

A is the threefold fix plus per-slot tournament engines, B adds the stdin
writer thread.

**Candidates, ruled in or out:**

- *Fresh engine processes per tournament game* — **in**, the dominant cause
  (above). Fixed by keeping engines per slot (`71b9a27`, maintainer decision
  2026-10-14): `tournament`/`gauntlet --engine-processes per-slot`, the
  default as for `match`/`sprt`/`spsa`; each side gets its own participant's
  kept engine, else any kept engine (kept when its launch fits, quit when not);
  `per-game` restores fresh processes. Tests
  `a_tournament_slot_keeps_its_engines_across_games_unless_asked_not_to` and
  `a_tournament_slot_hands_each_participant_its_own_kept_engine`.
- *Writes to engine stdin* — **in**, smaller. tokio's Windows `ChildStdin`
  runs every write and every flush on its blocking pool, and `send` made three
  of them (line, newline, flush), so a command reached the engine only after
  two pool hand-offs, each a thread to schedule on a saturated host. A
  dedicated writer thread per engine now takes one hand-off and one write
  (`153ebbf`): position write per move 0.165 → 0.030 ms and harness CPU 42.4 →
  31.5 s at concurrency 30. The charged interval still runs from the `go`
  stamp to the `bestmove` arrival (`CLOCK_MODEL_VERSION` 2 unchanged);
  `write_returned` is now the writer thread's completion stamp. Writes were
  always flushed; unflushed input was not a cause.
- *Async runtime scheduling with 60 engines* — **out** for wall time. An
  experimental build with 4 or 1 worker threads lowered harness CPU (31 →
  25 s at 1 worker) without changing wall time beyond run-to-run noise
  (±0.5 s at concurrency 30).
- *Live-view updates under a lock* — **out**: disabling the `info` sink saved
  about 1 s of harness CPU at concurrency 30, wall within noise.
- *Engine CPU sampling at `go` and `bestmove`* — **out**: disabling it saved
  about 3 s of harness CPU, wall within noise; it is the evidence behind the
  forensic `held`/kernel fields and stays.
- *SAN/PGN building, journaling and fsync, progress rendering* — **out**:
  together inside `between_searches` (0.027 ms a move) and per-game commit,
  outside any engine's clock; they do not reach wall time measurably.
- *Timer granularity on Windows* — **out**: at concurrency 1 the charged time
  per move (2.86 ms, B) equals fastchess's recorded time (2.82 ms); `h` there
  (1.07 ms) is the engine's whole-millisecond `info time` and its work after
  the last `info`, not harness latency.

**Acceptance** (`harness_speed.ps1`, 0.3.0 exe `AC56265A…`):

| c | Colosseum 0.3.0 wall | fastchess wall | plies (C / f) | charged / `h` | fastchess recorded |
|---|---|---|---|---|---|
| 30 | 18.0 / 17.9 / 17.8 s | 24.9 / 24.9 / 24.9 s | 71,523 / 71,523 | 6.27–6.31 / 1.70–1.71 ms | 6.01–6.09 ms |
| 14 | 22.8 / 22.8 / 22.8 s | 25.4 / 25.4 / 25.4 s | 71,523 / 71,523 | 3.69–3.70 / 1.23–1.24 ms | 3.28–3.30 ms |

Median at concurrency 30: 17.9 s against 24.9 s, 28% below fastchess (the
target was at most +2%). The charged time per move remains a little above
fastchess's recorded time at both concurrencies while the wall time is
lower; the two harnesses do not stamp the same interval, and wall time is the
measure.

**Timed games.** `3+0.03`, 300 games at concurrency 30, same book and
engine: 0.2.0 — 0 time losses, `h` median 1 ms, p99 23 ms, max 597 ms,
start-up 210 ms a game; 0.3.0 — 0 time losses, `h` median 1 ms, p99 22 ms,
max 579 ms, start-up 59 ms a game. No change in clock behaviour beyond the
shorter start-up.

## Compatibility

- The resolved tournament configuration gains `engine_processes`, so a
  tournament run directory from 0.2.0 is refused on resume as a changed
  configuration. Match, SPRT, SPSA and calibration run directories resume;
  their remaining games are played under the corrected repetition rule.
- The PGN annotation format and the journal schema are unchanged.
