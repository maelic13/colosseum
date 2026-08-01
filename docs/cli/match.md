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

Clocked searches use the recorded `go-write-to-bestmove-read` model version 1.
The charged interval begins after the complete `go` command has been flushed
and ends after the complete `bestmove` line has been read, using a monotonic
clock. Position setup is outside that interval. Increment is credited only
after an accepted move: an elapsed time greater than remaining time plus margin
forfeits; exact equality is accepted. Structured results include the model,
version, both margins, measured monotonic resolution and per-side charged-time
sample count/minimum/median/maximum. These figures intentionally do not claim
to separate engine work from scheduler or pipe latency inside the interval.

Draw and resignation adjudication are enabled by default with conservative
settings. All settings are ordinary policy and can be changed or disabled:

| Policy | Default | Controls |
|---|---|---|
| Draw | from move 40, 8 moves, ±10 cp | `--draw-move`, `--draw-moves`, `--draw-score-cp`, `--no-draw-adjudication` |
| Resignation | 3 moves, ±600 cp, both engines agreeing | `--resign-moves`, `--resign-score-cp`, `--no-resign-adjudication` |
| Maximum moves | disabled | `--max-moves N` |

Natural mate and draw rules always apply. Tablebase-related UCI options can be
forwarded through `--a-option` and `--b-option`; Colosseum does not inspect the
tablebase files or perform harness-side tablebase adjudication.

Engine and infrastructure faults are different outcomes. An engine timeout,
disconnect, protocol failure or illegal move is a scored forfeit with explicit
side and kind metadata. The match becomes `invalid` after more than
`--max-engine-faults N` or `--max-time-losses N`; both limits default to zero.
A pre-play spawn or harness/infrastructure failure is marked non-scorable,
stops the match as `infrastructure-error`, and never changes W/L/D. Colosseum
does not offer selective retry or discard of already-started statistical games.

Matches currently use the standard start position and one game at a time. CPU
placement, concurrency, books, durable run output and
statistical commands are added separately. `--a-cores` and `--b-cores` are
parsed for consistency with direct controls but rejected until CPU placement
is composed into matches.

Use `--dry-run` to resolve and print both invocations without launching either
engine. `--json` emits one match result document on stdout.
