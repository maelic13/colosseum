# Run directories and recovery

Every long-running CLI workflow uses a self-contained run directory. Without an
explicit path, the CLI allocates a collision-safe directory beneath
`./colosseum-runs`. A supplied `--dir` is stable: an existing directory means
resume, and its stored configuration hash must match exactly.

Selecting restart never deletes or overwrites an earlier attempt. The complete
old directory is renamed to a unique adjacent `.archive-…` path before a fresh
directory is initialized. Logs are opened append-only; resume does not
truncate prior diagnostics.

The run stores canonical `resolved-config.json`, `config.sha256` and
`config-origins.json`, and `run-record.json` (see [status](status.md)).

## What a run writes while it plays

Four files carry a running `match`, `calibrate`, `sprt`, `spsa` or
`tournament run`, and each has one job (a position `suite` plays no games and
keeps its completed positions in its checkpoint):

| File | Written | Holds |
|---|---|---|
| `games.jsonl` | appended, one line per game | the journal: every game's record |
| `games.pgn` | appended, one game at a time | every game's moves, for other tools |
| `checkpoint.json` | replaced every 50 units or 5 seconds, and on stop | aggregates only, and how much of the journal they cover |
| `run.log` | appended | human events: progress blocks, faults, stop and resume |

**`games.jsonl`** is the run's evidence. Each line is one JSON object: a line
version `v`, a sequence number `seq` counting from 1, the `game` record, the
position of its moves in `games.pgn` (`pgn`: `offset`, `length` and the
`sha256` of those bytes) and a `sha256` of the line itself, computed over the
line's other fields. The game record names the game (`number`, `pair_number`,
`pair_game`, `white`, `black`, the `opening`, and for a tournament its
`round`, for a tune its `iteration`), its `result`, `termination`, whether it
is `scorable`, its `fault` with the fault kind, its `sample` class (the same
class `games.pgn` carries as `ColosseumSample`: `official`, `post-terminal` or
`invalid`) and its clock accounting. A game nobody could score keeps the class
of the pair or iteration it was played in, with `scorable: false` as its mark,
so a resume rebuilds that pair or iteration whole; its PGN tag says
`unscorable`, which is what a reader of the export must know. The clock
accounting includes, per side, where its searches' time went: the largest
value each phase of a search's round trip reached in the game, in
nanoseconds — `go_write_ns` (the harness writing `go`), `to_first_info_ns`
(until the engine's first `info` arrived), `between_info_ns` (first to last
`info`), `last_info_to_bestmove_ns` (last `info` until `bestmove` arrived),
`bestmove_to_consumed_ns` (until the game task took it; not charged),
`last_info_lag_ns` (the last `info`'s arrival minus the time it reported) and
`overhead_ns` (charged time minus the engine's reported time). `slot` names
the CPU slot the game ran on (`index`, counting from zero) and when it held
it, in microseconds since the Unix epoch: `started_unix_us` before its first
engine was spawned, `ended_unix_us` after both had exited. Two games' spans on
one slot never overlap. It never holds moves.
Nothing is ever rewritten: a game is committed by appending its line.

**`games.pgn`** is appended one game at a time and never rewritten. Its tags
and annotations are the ones described in [output](output.md). A game's moves
are written before its journal line, so a journal line always points at moves
that were written first.

**`checkpoint.json`** is small and stays the same size however long the run
is. It holds what the command's summary needs — games attempted and scored,
W/D/L, the pentanomial counts and faults; for an SPRT the official pair count
and the post-terminal count; for a tune the completed iterations, the current
centre vector and the last iteration; for a tournament the attempted, scored
and faulted games — and `journal`: the byte `offset` of the journal it covers,
the `sha256` of those bytes, the number of `records` in them, and the
`pgn_offset` where `games.pgn` ended at the same moment. It is an atomic JSON
envelope with its schema version, payload and payload checksum, kept in two
generations: `checkpoint.json` is the current one and
`checkpoint.previous.json` the one before. If the current file is missing,
torn or fails its checksum, the previous generation is used. If neither
validates, resume fails rather than inventing state.

**`run.log`** is an append-only JSON-lines stream of what a person wants to
read: the progress blocks as printed, each fault with the game it ended, and
the stop, resume and finish of the run. It does not repeat games; the journal
has them.

When the run finishes, `result.json` holds the final structured report, and
`failed-games/` keeps a forensic of every abnormal game: the round-trip stamps
of each side's last five searches — when `go` was written, when the first and
last `info` arrived with the time the engine reported, when `bestmove` arrived
and when the game task took it, the overhead and the deadline — then the UCI
traffic and stderr. When a search misses its deadline the harness waits up to
one more second, only to time its `bestmove`; the forensic marks that late
answer with `!`, or prints `never`.

### Durability

Appends are not synced one by one. The journal, `games.pgn` and `run.log` are
synced together every 50 games or every second, whichever comes first, and at
every checkpoint and every stop. **A hard kill — a power cut, a killed
process, a crashed host — loses at most the games of that last window.** A
resume does not have those games and plays them again; nothing else is lost,
and nothing already synced changes.

All of this writing happens off the thread that drives games, so a slow disk
delays the files, never an engine's clock. The writer may fall a bounded
number of writes behind; past that, the game whose result is being committed
waits for the disk, and it waits without holding up the others, so no other
game's clock is read late on its account. If a write fails, the writer writes
nothing more and the run stops at its next commit; its final `run-record.json`
is then written directly, so a run that ended never reads `running`, and it
carries a `writer-failed` anomaly naming the failure.

### Resume

A resume loads the newest valid checkpoint and verifies the journal up to the
offset it names against the hash it recorded. It then reads the lines after
that offset one by one, each against its own checksum:

- an unfinished or unverifiable **last** line is a write the kill interrupted.
  It is dropped and its game is played again;
- a journal line whose moves are not in `games.pgn` with the hash the line
  recorded belongs to the unsynced window. It and everything after it are
  dropped and played again;
- anything in `games.pgn` after the last kept game is cut off.

The resume says on standard error what it dropped. What it keeps is replayed
from the journal: the SPRT official prefix and a tune's iteration boundaries
are recomputed from the kept games, never taken from memory or from the
checkpoint. A resumed run appends after the kept bytes and changes none of
them.

**A journal that does not match its checkpoint is refused, not repaired.** A
hash mismatch inside the covered bytes, a damaged line followed by a valid
one, or a `games.pgn` shorter than the checkpoint says was synced means the
directory changed after the checkpoint was written; the resume stops and says
so, and leaves the directory as it found it. A run directory written by an
earlier version, without a journal, is refused with an explanation: run the
same command with `--restart`, which archives the old directory and starts
afresh.

A resumed match schedules only the game numbers absent from the journal and
keeps deterministic report order. SPRT journals complete official and
post-terminal pairs with their class; only the official prefix enters
statistics, and a run that already reached its terminal state cannot be
extended.

A game the harness abandoned on an infrastructure fault — an engine that never
spawned, a processor-affinity call the operating system refused — is still
journalled and written to `games.pgn`, because the abandoned game is the
evidence for the abort. It is marked `ColosseumSample "unscorable"` and no
statistic counts it; see [statistics replay](stats.md).

Completed SPSA runs additionally write the verified schedule and three views of
the frozen final-window vector: `tuned-options.txt`, `tuned-options.json` and
`tuned-options.toml`. The main `result.json` retains the source launch, tune
conditions, schedule/version metadata, original values, floating means and
rounded tuned values so it can feed `sprt --apply` without editing. Each
iteration appears in it as a summary — the centres before and after, the plus
and minus vectors sent, the pair score, the faults, and `games`, the first and
last game numbers of its lines in `games.jsonl` — and never as its games: a
tune of thousands of iterations writes a result of a few megabytes, and a
running tune holds only the iteration it is playing. The document carries its
own `schema_version` (2; version 1 carried every game).

## Stopping a run

Interrupting a long run is a supported operation. Press Ctrl-C once and the run
stops launching new work, gives the games already in flight up to
`--stop-grace-secs` seconds (30 by default) to finish on their own, writes its
checkpoint, records itself `cancelled` and exits with code `6`. Press Ctrl-C
again and the games in flight are abandoned immediately instead of waiting.

Either way the run is recoverable, and every command stops the same way:
`match`, `sprt`, `calibrate`, `spsa`, `tournament run` and `suite` share one
cancellation path rather than each inventing its own. A stop syncs the journal
and writes the checkpoint, so every committed game is kept; work that had not
been committed is simply replayed when you resume the same `--dir`. For SPSA that means the mini-match in flight
is replayed as a whole, which is what keeps a gain schedule from advancing on a
partial iteration.

The grace period is one bounded period, measured from the interrupt rather
than from each game that finishes inside it, so a busy run with several slots
still stops when the period is up.

A cancelled run is not a failure and not a statistical conclusion. An SPRT that
stopped before reaching a boundary or its cap reports `cancelled` rather than
`inconclusive`, because it never sampled far enough to say anything; a
calibration that stopped short of its fixed sample reports that instead of
classifying a partial interval. `spsa --stop-after-iteration N` asks for the
same clean stop at a planned point rather than at an interrupt.
