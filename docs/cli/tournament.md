# Tournaments

`tournament plan` produces the exact static schedule for a round-robin or
gauntlet without launching engines. It accepts ordinary executable paths; no
engine manifest is required.

```text
colosseum-cli tournament plan \
  --engine ./engine-a --engine ./engine-b --engine ./engine-c

colosseum-cli tournament plan --format gauntlet --seeds 2 \
  --engine ./seed-a --engine ./seed-b \
  --engine ./opponent-a --engine ./opponent-b
```

Round-robin is the default. `--cycles` repeats the complete pairing design and
`--games-per-pair` controls each encounter; colours alternate within an
encounter. In a gauntlet, the first `--seeds` engines play every remaining
engine, while seeds do not play one another and opponents do not play one
another.

`gauntlet` is a convenience alias for the same planner:

```text
colosseum-cli gauntlet --seeds 2 \
  --engine ./seed-a --engine ./seed-b \
  --engine ./opponent-a --engine ./opponent-b
```

Use `--json` for stable structured output. Participant IDs are derived from
the supplied order, so the same arguments produce the same schedule. Optional
`--rating` values must either be omitted or supplied once per engine; omitted
ratings default to 1500. Ratings are schedule metadata at this stage and do
not affect pairings.

## Play a tournament

`tournament run` plays the same static schedule and accepts ordinary UCI
executables directly:

```text
colosseum-cli tournament run \
  --engine ./engine-a --engine ./engine-b --engine ./engine-c \
  --games-per-pair 2 --concurrency 2 --placement auto \
  --book ./openings.epd --seed 42 --dir ./runs/comparison
```

The time-control choices are `--movetime-ms`, `--base-ms` with optional
`--increment-ms`, `--nodes`, or `--depth`; when omitted the default is
`3+0.03`. Draw, two-sided resignation and maximum-move adjudication use the
same controls and conservative defaults as fixed matches. The opening book is
optional. Its order, start, PGN ply count and seed have the same meaning as in
the match runner.

`--ponder` enables and records UCI pondering for every participant. It is off
by default and requires a base/increment clock; it is not accepted with
movetime, fixed nodes or fixed depth.

Common process and UCI controls apply to every participant: `--engine-arg`,
`--cwd`, `--env`, `--option`, and `--button`. A one-based indexed form overrides
or extends one participant where engines differ:

```text
--engine-option 1:EvalFile=candidate.nnue
--engine-option 2:EvalFile=baseline.nnue
--engine-arg-at=3:--uci
--engine-cwd 3:./engine-three
--engine-env 3:RUST_LOG=warn
--engine-button 3:ClearHash
```

`--dry-run` resolves all invocations and settings without launching an engine.
Use `--label` once per engine for readable names; otherwise executable file
stems are used.

## Ratings and artifacts

Ratings are recomputed jointly from all scored games with the shared
maximum-likelihood implementation. Each JSON/text row includes its asymptotic
95% error half-width. `--anchor N` fixes the one-based participant at its
initial `--rating` and estimates every other participant against that scale.
Without an anchor, the participant priors center the otherwise relative rating
scale.

Every run directory contains checksum-protected current/previous checkpoints,
append-only `run.log`, `games.pgn`, `standings.csv`, `crosstable.csv`,
`result.json`, the resolved configuration, and the common run record. Repeating
the same command with an existing `--dir` resumes only schedule games absent
from the durable checkpoint. `--restart` archives the old directory first.

Engine-attributable faults remain scored forfeits. Tournaments are exploratory
and non-strict by default; use `--max-engine-faults N` when the whole run should
be invalidated after more than N such faults. Infrastructure failures are
never scored and stop new scheduling.
