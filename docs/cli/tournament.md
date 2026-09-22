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
`3+0.03`. Draw, resignation and maximum-move adjudication are off unless
requested and use the same enabling flags, parameters and refusals as fixed
matches. The opening book is optional. Its order, start, PGN ply count and seed have the same meaning as in
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
95% error half-width. Without a fixed participant, the participant priors
center the otherwise relative rating scale.

`--fixed N:RATING` pins the one-based participant at a rating you supply, and
only the remaining participants are estimated — jointly, against the pinned
field and each other. Repeat it once per member of an established pool:

```text
colosseum-cli tournament run   --engine ./newcomer --engine ./known-a --engine ./known-b   --fixed 2:2480 --fixed 3:2315 --games-per-pair 20
```

That is how a newcomer is placed in a pool whose ratings you already trust,
without spending games re-measuring the pool. At least one participant must be
left free, or the tournament would estimate nothing and is refused.

A pinned participant reports no error bar: its rating is an input, not
something this tournament measured, and an interval would suggest otherwise.
Its standings row carries `fixed`, its CSV row says `yes` in the `Fixed`
column and leaves `EloDelta` empty, its text row is marked `[fixed]`, and the
supplied ratings are retained in the result and the run record as run inputs.

`--anchor N` is the degenerate case and stays: it pins one participant at its
own initial `--rating` instead of a separately supplied value. Naming the same
participant through both is refused rather than resolved silently.

Every run directory contains the append-only game journal `games.jsonl`,
`games.pgn` and `run.log`, checksum-protected current/previous checkpoints,
`standings.csv`, `crosstable.csv`, `result.json`, the resolved configuration,
and the common run record; see [run directories](run-directories.md).
Repeating the same command with an existing `--dir` resumes only schedule games
absent from the journal. `--restart` archives the old directory first.

Engine-attributable faults, losses on time among them, remain scored forfeits.
A tournament is invalidated after more of them than `--max-engine-faults N`,
which defaults to 1% of the scheduled games and at least 5. The count and the
allowance are on the `faults` line of every progress block and of the final
report. Infrastructure failures are never scored and stop new scheduling.
