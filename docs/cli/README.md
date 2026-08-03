# Colosseum CLI guide

Colosseum CLI is the independent, headless side of Colosseum for chess-engine
development and reproducible experiments. The current development version can
inspect and compliance-check ordinary UCI executables, run a fixed-N direct
match, capped pair-atomic SPRT, or optional identical-binary calibration,
resolve reproducible configuration, validate its own installed executable, and
read durable run status, tune UCI parameters with SPSA, and make an authoritative
single-search fixed-node speed measurement.

Start with an executable path; no manifest or engine-side integration is
required:

```text
colosseum-cli engine inspect ./my-engine
colosseum-cli engine check ./my-engine
colosseum-cli match --games 100 ./candidate ./baseline
colosseum-cli sprt ./candidate ./baseline --max-pairs 5000 --preset gainer
colosseum-cli calibrate ./my-engine ./my-engine --book ./openings.epd
colosseum-cli nps ./my-engine --nodes 10000000
colosseum-cli tournament plan --engine ./engine-a --engine ./engine-b --engine ./engine-c
colosseum-cli capabilities
colosseum-cli self-test
colosseum-cli status ./colosseum-runs/my-run
```

Detailed contracts:

- [Direct engine controls and compliance checks](engine-controls.md)
- [Fixed direct-engine matches](match.md)
- [Capped pair-based SPRT designs](sprt.md)
- [Optional identical-binary calibration](calibration.md)
- [SPSA tuning, planning and diagnostics](spsa.md)
- [Fixed-node NPS measurement](nps.md)
- [Opening-book utilities](book.md)
- [Statistics replay and evidence authority](stats.md)
- [Fixed-work EPD/FEN position suites](suite.md)
- [Round-robin and gauntlet tournament planning](tournament.md)
- [Configuration files, inheritance and path origins](run-files.md)
- [Master seed and named random streams](randomness.md)
- [Human, JSON and dry-run output](output.md)
- [Installed-executable self-test](self-test.md)
- [Run directories and checkpoint recovery](run-directories.md)
- [Official run records and read-only status](status.md)
- [CPU topology, restrictions and affinity capabilities](cpu-topology.md)
