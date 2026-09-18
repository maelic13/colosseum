# fastchess mechanics, and what Colosseum does differently

Research record for Phase 10.9m (PLAN §Phase 10(w)). It sets out how
fastchess runs an engine on Windows, which of its mechanisms Colosseum CLI
lacks, and for each one whether to adopt it, decline it with a reason, or
test it. It also records what the forfeits measured so far say about where
the time goes.

**Source read:** fastchess at commit `072859b` ("chore: bump to
v1.8.0-alpha"), the exact build the reference host runs (`fastchess.exe
--version`: `alpha 1.8.0 compiled for windows-latest 20260128-072859b`).
Paths below are relative to its `app/src`. **Invocation read:** Rarog's
`tools/sprt.ps1`, which is how fastchess has been run on this host: 3+0.03,
`timemargin=20`, concurrency 14, `-use-affinity 2,4,6,…,28`.

## What the forfeits say

Every Colosseum time forfeit with the round-trip instrument (10.9j) was
parsed from its forensic table: 14 in `colosseum-latency-b22` (before the
slot pool) and the two since (`colosseum-verify-sprt`,
`colosseum-verify-sprt-3`). All sixteen have one shape:

| | range over the 16 forfeits |
|---|---|
| last `info` arrival minus the `time` it reported | 0.1–1.1 ms |
| `bestmove` arrival minus the last `info` arrival | 59–89 ms |
| `bestmove` arrival minus the engine's own hard cap (`0.8097 × R − 10` for Rarog) | 42–64 ms |

The engine received `go` promptly and reported on time: its own clock and
the harness's agree to about a millisecond at its last report. The time is
lost after that, inside the engine's process. Rarog polls a precise clock
every 2,048 nodes and aborts at its hard cap, so a `bestmove` 42–64 ms past
the cap means its search thread did not run, or ran far slower, for about
that long. The delivery of `go`, the pipes and the harness's reading are
ruled out as carriers of this time.

**`h=` is not harness overhead for such an engine.** `h=` is charged time
minus the engine's last reported `time`. Rarog prints `info` only after a
completed iteration, so when the clock aborts an iteration it prints
nothing more, and `h=` then holds the engine's own search time since its
last report. In the maintainer's 15-slot tune (`colosseum-spsa-15x30-s`,
1,800 games) 1,087 of 3,600 game-sides had a largest `h=` above 20 ms, and
in every one of them all of it lay between the last `info` and the
`bestmove`; the last `info` arrived within 3.0 ms of the time it reported
in every game-side. The `stats` count of moves "over the time margin" is
therefore a count of aborted iterations for such an engine, not of harness
delays. The harness's own share is the last-`info` lag, 1.1 ms typical.

**The desktop application's record is not evidence at a 20 ms margin.** Its
database holds 251,192 finished games with 435 time forfeits, every one
lost by Rybka 2.3.2a; its incident directory holds 756 forfeit reports, all
Rybka. Rarog played 67,820 game-sides and Basilisk 97,816 without a
forfeit. But the desktop scheduler has always charged with a 2 s margin
(`TIMEOUT_TOLERANCE`, unchanged since 0.1.0), and its PGNs carry no
per-move times from which a 20 ms margin could be replayed. It says only
that nothing but Rybka ever stalled for two seconds.

## Mechanisms compared

### Process lifetime and reuse

fastchess keeps engines alive between games. With affinity on, each
tournament worker thread has its own engine cache
(`matchmaking/tournament/base/tournament.cpp`, `getEngineCache`), and the
same two processes play every game on that thread unless an engine is
configured `restart=on`, or stalled or disconnected with `recover` set. A
game refreshes a cached engine with `ucinewgame`, `isready` and the options
again (`engine/uci_engine.cpp`, `refreshUci`). Colosseum starts two fresh
processes for every game and quits them after it, deliberately: a fault
cannot leak into the next game and each game has its own forensics.

Relevance: a fresh process allocates and touches its hash, loads its image
and is new to the operating system (and to any real-time scanner) every
game; a reused one is warm. The stall happens late in games (plies 93–180),
long after start-up, which argues against start-up effects, and the one
post-pool forfeit checked had no other game boundary within 950 ms. **Test**
(persistent processes per slot), after the cheaper tests below, because it
is the one expensive to build.

### Starting engines

fastchess starts at most 16 engines at once (a counting semaphore in
`engine/uci_engine.cpp`). Colosseum had no such limit; its start-up cost
came from a processor-group check that snapshotted every thread on the
machine, fixed in 10.9s, after which a burst of thirty starts costs what the
engines themselves cost. **Decline:** nothing points at start-up in the
forfeits.

### Process creation flags, job object and priority

fastchess: `CreateProcessA` with `CREATE_NEW_PROCESS_GROUP` only, no job
object, priority inherited (normal) (`engine/process/process_win.hpp`).
Colosseum: `CREATE_NO_WINDOW | CREATE_SUSPENDED`, a kill-on-close job object
attached before the process resumes, priority normal. Neither changes a
priority class. A job with only a kill-on-close limit places no scheduling
limit on its processes. **Decline for now:** no mechanism connects either
to a thread held mid-search; revisit only if the tests below leave a gap.

### Affinity: mechanism and CPUs per game

fastchess on Windows 11 applies `SetProcessDefaultCpuSetMasks` (CPU Sets)
and falls back to `SetProcessAffinityMask` only where that function is
absent (`affinity/affinity.hpp`, `setProcessAffinity`); the reference host
runs Windows 11 build 26200, so fastchess has used CPU Sets there. It gives
each game one logical CPU: `-use-affinity 2,4,…,28` makes each listed CPU
its own slot (`affinity/affinity_manager.hpp`, `setupSelectedCores`), so
both engines of a game and the worker thread that runs it share one logical
CPU, and the other sibling of every game core is left to everything else.
It also pins that worker thread to the game's CPU before starting engines
(`tournament.cpp`, `setThreadAffinity`).

Colosseum applies the hard `SetProcessAffinityMask` and gives each game a
whole core, both siblings, with the harness's threads unpinned.

Relevance: an engine thread restricted to two logical CPUs can be held off
them for a scheduling quantum or more when other threads the scheduler
places there run at an equal or boosted priority; how the two mechanisms,
and one CPU against two, change that is exactly the open question. The
measured overruns of 42–64 ms are of the order of the scheduler's time
quantum, a few clock ticks of 15.6 ms, which fits a thread waiting its turn
better than any engine-side cause; that is a hypothesis for the
measurement below to confirm or refute, not a finding. **Test:** one logical CPU per game needs no code
(`--placement 2,4,…,28` at concurrency 14 puts one listed CPU in each
slot); CPU Sets need a recorded, selectable placement mechanism.

### Pipes and readers

fastchess: named pipes (`\\.\Pipe\RemoteExeAnon.…`) created overlapped with
4 KB buffers, `stderr` merged into `stdout`, the game's own thread reading
with overlapped `ReadFile` and a fresh event per read, and synchronous
`WriteFile` for input (`engine/process/anon_pipe.hpp`, `process_win.hpp`).
Colosseum: the standard library's pipes, `stderr` drained separately, one
dedicated reader thread per engine stamping each line on arrival, writes
through the runtime. **Decline:** the forfeit forensics put the carrier
after the engine's last report, with the last `info` 0.1–1.1 ms behind its
own time; the pipes deliver promptly.

### Where the clock starts and stops

fastchess stamps `t0` after `position` and `go` have been written and `t1`
when the game thread has read the `bestmove` line, and truncates the
interval to whole milliseconds (`matchmaking/match/match.cpp`, `playMove`).
Colosseum stamps before the `go` write and charges to the reader thread's
arrival stamp, in nanoseconds (clock model `go-write-to-bestmove-arrival`
2). The two differ by the write (0.2 ms typical) and up to a millisecond of
truncation in the engine's favour. **Decline:** Colosseum's interval is the
stricter and more exact one, and the difference is two orders of magnitude
below the forfeit overruns.

### `timemargin` and the clock trajectory

The rule is the same: a move forfeits when its elapsed time exceeds the
time left plus the margin, time left is floored at zero and the increment
added after the move (`game/timecontrol/timecontrol.cpp`, `updateTime`).
Two details differ. fastchess starts the clock at base plus one increment
(3,030 ms at 3+0.03), so every `wtime`/`btime` it sends is one increment
higher than Colosseum's at the same history; at the forfeit point that is
about 0.19 × 30 ≈ 6 ms more slack for an engine that stops at 0.8097 of its
time. And it waits for a `bestmove` until time left plus margin plus 100 ms
(`MARGIN`) before declaring the timeout, where Colosseum stops at time left
plus margin and then reads for a second only to time the late answer; the
verdict is the same. **Decline** as a change to Colosseum: the base clock is
the time control as configured, and the 6 ms is too small to explain an
overrun of 42–64 ms. **Recorded** because fastchess's forfeit counts are
measured on a 30 ms richer clock.

### Traffic between moves

Before every move fastchess checks the engine with `isready`/`readyok`
(`validConnection`) before sending `position`, again between `position`
and `go`, and again after the `bestmove`, all outside the clock. Colosseum
sends `position` and `go` back to back. The ping guarantees that the engine
has finished handling everything before the clock starts. **Test, low
prior:** the stall lies inside the search, after the engine's own clock
had started, which a ping before `go` cannot reach; cheap to build if the
other tests leave a gap.

### Traffic between games

With reuse, fastchess sends `ucinewgame`, `isready` and the options (with
`Threads` first) per game; Colosseum's fresh processes receive the full
handshake. Part of the persistent-process test.

## What to measure, and how

A forfeit happens about once in 2,000–3,000 games, so a 2,000-game run per
difference would mostly compare zero with zero or one. The discriminating
runs need a reading that every search provides. Two are available or
cheap:

- **Held time per search, engine-agnostic (to build first).** At `go` and
  at the `bestmove`'s arrival the harness reads the engine process's
  consumed CPU cycles (`QueryProcessCycleTime`, cycle-exact where
  `GetProcessTimes` is tick-granular) and converts them to time with a
  calibrated cycle rate. Charged time minus CPU time is the time the
  engine's process was not running while its clock ran. It needs nothing
  from the engine, costs two system calls per search, and turns every one
  of the ~110,000 searches per side of a 2,000-game run into a sample:
  the distribution's 99.9th percentile and the count above 25 ms compare
  directly between runs.
- **Overrun past the engine's own cap, Rarog-specific.** Charged time minus
  `0.8097 × R − 10` for moves in scramble, the late-mode metric already in
  use; it fires only when the engine is near its cap.

If the held time is confirmed as the carrier, a Windows Performance
Recorder trace (`wpr -start CPU`, stopped right after a held search) names
the thread that ran instead; that is the decisive follow-up, not the first
step.

## Order of the tests

Each run: 2,000 games of b22core against b22base at 3+0.03, 14 slots, 20 ms
margin, Hash 64, Threads 1, the UHO book, one thing changed at a time, read
by held time per search, the late-mode metric and the forfeit count.

1. Baseline: `--placement auto`, as today.
2. One logical CPU per game, as fastchess: `--placement 2,4,…,28`.
3. CPU Sets instead of hard affinity, on the better of 1 and 2.
4. Persistent engine processes per slot, if 1–3 leave a gap.
5. `isready` before `go`, and fastchess's creation flags without the job
   object, only if a gap remains after 4.

Target, unchanged: 0 time losses in 10,000 games at 14 slots, or a
documented operating-system floor that fastchess shares. The fix is its own
step.
