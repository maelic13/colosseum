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

Matches currently use the standard start position and one game at a time.
Adjudication, CPU placement, concurrency, books, durable run output and
statistical commands are added separately. `--a-cores` and `--b-cores` are
parsed for consistency with direct controls but rejected until CPU placement
is composed into matches.

Use `--dry-run` to resolve and print both invocations without launching either
engine. `--json` emits one match result document on stdout.
