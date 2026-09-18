# Fixed matches

`colosseum-cli match` plays exactly the requested number of games between two
ordinary UCI executables. It does not use sequential stopping. Colours alternate
by game, starting with engine A as White.

```text
colosseum-cli match --games 100 ./candidate ./baseline
```

The two paths may be the same executable. Give each side its own options to
compare configurations of one engine:

```text
colosseum-cli match --games 200 ./my-engine ./my-engine \
  --a-label candidate --a-option Hash=64 \
  --b-label baseline --b-option Hash=32
```

Side-specific direct controls use an `a-` or `b-` prefix:

| Control | Meaning |
|---|---|
| `--a-label` / `--b-label` | Display label |
| `--a-engine-arg` / `--b-engine-arg` | Engine process argument; repeat as needed |
| `--a-cwd` / `--b-cwd` | Engine working directory |
| `--a-env` / `--b-env` | `KEY=VALUE` environment override |
| `--a-option` / `--b-option` | `NAME=VALUE` UCI option |
| `--a-button` / `--b-button` | UCI button option |

Each side has an independent time control. Select at most one mode per side:

| Mode | Engine A | Engine B |
|---|---|---|
| Time per move | `--a-movetime-ms N` | `--b-movetime-ms N` |
| Sudden death | `--a-base-ms N` | `--b-base-ms N` |
| Base plus increment | `--a-base-ms N --a-increment-ms I` | `--b-base-ms N --b-increment-ms I` |
| Fixed nodes | `--a-nodes N` | `--b-nodes N` |
| Fixed depth | `--a-depth N` | `--b-depth N` |

With no selection, that side uses `3000 ms + 30 ms` per move. The per-side
`--a-margin-ms` and `--b-margin-ms` values are forfeit tolerances only; they are
not sent to the engines. This permits odds matches, including one engine at a
different clock or search limit.

`--ponder` enables the ordinary UCI pondering protocol for both engines and
sets their `Ponder` option. It is off by default and requires base/increment
game clocks on both sides. Pondering is rejected with fixed movetime, nodes or
depth because those modes have no opponent-clock budget and a ponder hit would
change the requested fixed work. The flag, resolved engine options and final
report all record the condition; use the flag rather than forwarding `Ponder`
as a generic per-engine option.

Clocked searches use the recorded `go-write-to-bestmove-arrival` model
version 2. The charged interval begins as the `go` command is written and ends
at the instant the complete `bestmove` line arrived on the engine's output
pipe, using a monotonic clock. A thread that does nothing but read that pipe
takes the arrival instant, so nothing the harness does after the line lands
— committing another game, a slow disk — can be charged to the engine.
Position setup is outside that interval. Increment is credited only
after an accepted move: an elapsed time greater than remaining time plus margin
forfeits; exact equality is accepted. Structured results include the model,
version, both margins, measured monotonic resolution and per-side charged-time
sample count/minimum/median/maximum. These figures intentionally do not claim
to separate engine work from scheduler or pipe latency inside the interval.

Adjudication is off. Games are played out under the rules of chess unless you
ask for a rule, because every adjudication rule ends games the engines would
otherwise have had to convert, and conversion is part of what a strength
measurement measures. Each rule is enabled by its own flag and carries its own
parameters:

| Policy | Enable with | Parameters and defaults |
|---|---|---|
| Draw | `--draw-adjudication` | `--draw-move 40`, `--draw-moves 8`, `--draw-score-cp 10` |
| Resignation | `--resign-adjudication` | `--resign-moves 3`, `--resign-score-cp 600`, two-sided unless `--one-sided-resign-adjudication` |
| Maximum moves | `--max-moves N` | none; omitting the flag leaves the cap off |

A parameter without its enabling flag is refused rather than silently ignored,
so `--resign-score-cp 900` alone is an error and never a run that resigned at
600. Enabling a rule is part of the resolved configuration, the run identity
and the resume comparison.

The shipped parameters are conservative on purpose: a draw needs agreement
within ±10 cp for 8 consecutive moves and never before move 40, because early
evaluations agree by coincidence rather than because a position is drawn.
Resignation is two-sided because a one-sided rule adjudicates on the losing
side's own evaluation alone, which is a measurable asymmetry whenever the two
sides score differently — most sharply in SPSA, where both arms are the same
binary. `--one-sided-resign-adjudication` exists to reproduce a harness policy
deliberately and requires `--resign-adjudication`.

Throughput is the reason to enable these rules, and it is a legitimate one.
Settings in common use elsewhere, for users who want to match them:

```text
# Approximately the usual fastchess/cutechess draw and resign policy
--draw-adjudication --draw-move 40 --draw-moves 8 --draw-score-cp 10 --resign-adjudication --resign-moves 3 --resign-score-cp 600

# Fishtest-style: resignation off, draws adjudicated late and tightly
--draw-adjudication --draw-move 40 --draw-moves 8 --draw-score-cp 10

# A one-sided resign rule, as some older harnesses apply it
--resign-adjudication --resign-moves 3 --resign-score-cp 600 --one-sided-resign-adjudication
```

Whatever you choose, keep tuning and gating conditions identical: a tuner
optimising under different game-termination rules than the gate measures is
optimising a different objective.

Natural mate and draw rules always apply. Tablebase-related UCI options can be
forwarded through `--a-option` and `--b-option`; Colosseum does not inspect the
tablebase files or perform harness-side tablebase adjudication.

Every game-playing command writes the search evidence behind each move into
`games.pgn`, because a PGN of bare moves cannot feed a training-data extractor
or a tree-shape comparison and the runner already holds the values. After every
engine move the comment is:

```text
{s=<score> d=<depth> t=<ms>ms h=<ms>ms n=<nodes>}
```

`s` is the score from the mover’s own point of view, a signed integer in
centipawns or `#<n>` / `#-<n>` for mate in `n`. `d` is the reported depth, `t`
is the harness-charged elapsed milliseconds under the recorded clock model, `h`
is the harness overhead — the charged time minus the search time the engine
itself reported, rounded to the nearest millisecond, which can be slightly
negative — and `n` is the reported node count. A field the engine did not
report is left out entirely rather than written as zero, so an absent value and
a reported zero stay distinguishable. `h` is absent when the engine reported
no time, and after a `ponderhit` under `--ponder`, where the engine's clock
started at `go ponder` and the charge at `ponderhit`, so the two share no
origin. Moves pre-played from an opening book carry `{book}` instead, and the
`OpeningPlyCount` tag still marks how many there were.

A search that lost on time played no move, so it has no move to comment on.
It is written just before the result as `{forfeit t=<ms>ms h=<ms>ms}` with the
time its answer finally arrived, or `{forfeit}` when no answer came. The
`WhiteTimeMarginMs` and `BlackTimeMarginMs` tags name each side's time margin,
so a PGN alone says how close each move's overhead came to a forfeit, and
`GameSlot` names the CPU slot the game ran on, counting from zero.

The writer form is versioned and every run record names the version it used, so
a PGN read months later can be interpreted against the exact form that produced
it. [`stats`](stats.md) reads this form back and reports score, depth, time and
node coverage.

Engine and infrastructure faults are different outcomes. An engine timeout,
disconnect, protocol failure or illegal move is a scored forfeit with explicit
side and kind metadata. **A loss on time is an engine fault**: it counts
towards `--max-engine-faults` as well as towards `--max-time-losses`. The
match becomes `invalid` after more engine faults than `--max-engine-faults N`,
which defaults to 1% of the scheduled games and at least 5 — a 30,000-game
match tolerates 300, a 200-game match 5. `--max-time-losses N` defaults to the
same number, so by default it adds no separate limit. Set either to `0` to
invalidate on the first forfeit. The allowance is shown on the `faults` line of
every progress block and of the final report.
A pre-play spawn or harness/infrastructure failure is marked non-scorable,
makes the match `infrastructure-error`, and never changes W/L/D. Colosseum does
not offer selective retry or discard of already-started statistical games.

`--concurrency N` runs that many game slots while keeping final game records in
schedule order. CPU placement defaults to `--placement off`, which makes no
hard request and works on platforms without affinity. Use `--placement auto`
with optional `--headroom-cores N` to allocate whole physical cores through the
detected topology, or provide an explicit logical CPU pool such as
`--placement 0-7`.

Both engines of a game share one core set by default, `--cores-per-game N`
(default 1), because without pondering only one of them searches at a time and
the other is blocked reading a pipe. A 16-core host therefore runs 15
one-thread games at once rather than 7. `--cores-per-engine N` gives each
engine its own cores instead; the two flags are mutually exclusive, and the
disjoint one is required with `--ponder`, whose engines do search at once. `auto` leaves one whole
physical core free by default, selects only the highest-performance core class
on a host whose classes differ, and keeps each game slot inside one last-level
cache domain and one NUMA node when the pool allows it; see
[CPU topology](cpu-topology.md). Existing per-side
`--a-cores` / `--b-cores` lists are also enforced, but only for concurrency 1.
Any requested placement that cannot be applied and read back is an
infrastructure error, never a silent fallback.

When both sides have an explicit numeric Hash option, output reports the
conservative lower bound `concurrency × (A Hash + B Hash)`. It is informational
unless `--memory-budget-mb N` supplies a trusted hard budget; only that explicit
budget can cause memory refusal. Hash omits engine-private allocations, mapped
files and other process memory, so the reported value is never labelled total
memory use.

An opening book is optional; Colosseum does not ship one. `--book FILE`
accepts EPD or PGN (inferred from the extension). `--book-order sequential` is
the default; `random` uses the versioned `opening-order` named stream from the
run's `--seed`. If no seed is supplied, a master seed is generated and printed
in structured output. `--book-start N` selects the zero-based first opening and
`--book-plies N` controls how many PGN half-moves are pre-played. Each opening
is assigned to a two-game colour-reversed pair; odd fixed-N matches may end with
one final half-pair. Output records the assignment for every game. Without
`--book`, every game starts from startpos and output carries an
opening-diversity warning.

Openings are consumed **sequentially from `--book-start`** in the resolved
order, and a run that needs more than remain is refused at resolution time,
naming the shortfall. That refusal is the point: running out of openings makes
a run silently replay positions it has already played, which narrows its error
bars without narrowing the uncertainty, and it lets two segments of one book
replay each other. `--book-wrap` opts into modular reuse deliberately; it is
recorded in the resolved configuration and the reused fraction is reported.

`--dry-run` states the exact index range a run will consume — `first_index`,
`last_index` and how many openings that is — before it consumes any of them, so
a long schedule can be checked against its book without starting it. A
tournament consumes one opening per encounter rather than one per pair, because
every game of an encounter is played from the same opening.

## Generating a self-play corpus

There is no separate `datagen` command, because a fixed-node or fixed-depth
self-play match already is one. The same executable plays both sides, every
move carries its search evidence, and `games.pgn` is the corpus:

```text
colosseum-cli book slice ./master-book.epd ./shard-01.epd \
  --start 0 --count 20000 --order random --seed 4242

colosseum-cli match ./engine ./engine \
  --games 40000 --a-nodes 20000 --b-nodes 20000 \
  --book ./shard-01.epd --book-start 0 \
  --concurrency 12 --placement auto \
  --seed 4242 --dir ./corpus/shard-01
```

The range policy is what makes this shardable. 40,000 games are 20,000
colour-reversed pairs, so the shard must hold at least 20,000 openings; if it
does not, the run refuses before playing instead of quietly replaying the start
of the book into the corpus. To continue where a shard stopped, keep the same
book and advance `--book-start` by the number of openings the previous run
consumed, which its run record and dry run both state exactly. Never rely on
wraparound for a corpus: duplicated openings become duplicated positions.

Each shard is a self-contained run directory, so a shard can be stopped and
resumed, and its `run-record.json` carries the seed, book hash and index range
that produced it. Extract from `games.pgn`, whose per-move comments give score,
depth, charged time and nodes; positions before the first engine move are
marked `{book}` and counted by the `OpeningPlyCount` tag.

Every live match writes a self-contained run directory. Without `--dir`, a
unique path is created beneath `./colosseum-runs/`. `--dir PATH` selects a
stable directory and resumes matching durable state; `--restart` archives the
complete old directory before starting fresh. A materially different resolved
configuration is refused on resume. The directory contains:

| Artifact | Purpose |
|---|---|
| `resolved-config.json` and `config.sha256` | Canonical replay identity |
| `games.jsonl` | Append-only journal, one checksummed record per game |
| `games.pgn` | Portable game export, appended one game at a time |
| `checkpoint.json` and `checkpoint.previous.json` | Checksummed aggregates and the journal position they cover |
| `run-record.json` | Versioned lifecycle and official sample |
| `run.log` | Append-only JSON-lines log of progress blocks, faults, stop and resume |
| `result.json` | Final structured match report |
| `failed-games/` | Round-trip timing, UCI traffic and stderr for abnormal games |

Progress is written to stderr every `--progress-every N` games, and once more
at the end; a match block reports the score, the Elo estimate with its 95%
interval, faults against the allowance and the rate. See the
[output contract](output.md) for the shared rules. The
final human report names the artifact directory, the two engines' scores, the
fault counts and how many games ended abnormally and in which way. It does not
list the games: `games.pgn` holds every result, `run.log` every event, and
`failed-games/` the UCI traffic of each abnormal game. In `--json` mode stdout still
contains exactly one JSON document; it includes `run_directory`, while progress
and diagnostics remain confined to stderr.

Match exit codes are stable for automation: `0` means completed and valid, `1`
means completed but invalid under the configured fault policy, `2` means CLI or
configuration refusal, and `3` means an infrastructure, persistence or runtime
error. A failed command that cannot produce a final report leaves JSON stdout
empty.

`--a-cores` and `--b-cores` are direct hard-placement alternatives to the
global placement pool.

Use `--dry-run` to resolve and print both invocations without launching either
engine. `--json` emits one match result document on stdout.
