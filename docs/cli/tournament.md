# Tournament planning

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

Live play, joint ratings, CSV output and durable resume are not yet exposed by
the development build.
