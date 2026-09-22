# CLI quickstart

Colosseum CLI launches ordinary UCI executables directly. It does not inspect
their compiler, source tree or build flags, and it requires no engine manifest.

After unpacking a release archive, verify the exact binary:

```text
colosseum-cli --version
colosseum-cli self-test
colosseum-cli capabilities
```

`self-test` uses an internal deterministic UCI fixture; it needs no engine or
network. `capabilities` reports topology, process restrictions and whether hard
CPU affinity can be enforced on this host.

Check each engine before spending games:

```text
colosseum-cli engine inspect ./candidate
colosseum-cli engine check ./candidate
colosseum-cli engine check ./baseline
```

Plan the exact invocation without launching either process:

```text
colosseum-cli match ./candidate ./baseline --games 100 --a-movetime-ms 100 --b-movetime-ms 100 --dry-run --json
```

Then remove `--dry-run`. An opening book is optional; without one, every game
starts from the normal initial position and the result records the lack of
opening diversity. For a durable location that can be resumed, add
`--dir ./runs/first-match`.

```text
colosseum-cli match ./candidate ./baseline --games 100 --a-movetime-ms 100 --b-movetime-ms 100 --dir ./runs/first-match
colosseum-cli status ./runs/first-match --json
```

Use [run files](run-files.md) when conditions should be reviewed and reused.
Before interpreting Elo or an SPRT verdict, read [how to trust a
result](trust-results.md).
