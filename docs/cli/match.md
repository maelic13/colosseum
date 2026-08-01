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

This first match surface uses the current 100 ms-per-move default, standard
start position, and one game at a time. Time-control selection, adjudication,
CPU placement, concurrency, books, durable run output and statistical commands
are added separately. `--a-cores` and `--b-cores` are parsed for consistency
with direct controls but rejected until CPU placement is composed into matches.

Use `--dry-run` to resolve and print both invocations without launching either
engine. `--json` emits one match result document on stdout.
