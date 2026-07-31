# Colosseum CLI guide

Colosseum CLI is the independent, headless side of Colosseum for chess-engine
development and reproducible experiments. The current development version can
inspect and compliance-check ordinary UCI executables, resolve reproducible
configuration, validate its own installed executable, and read durable run
status. Match, SPRT, calibration, SPSA and tuning workflows are being built on
this foundation and are not claimed as available yet.

Start with an executable path; no manifest or engine-side integration is
required:

```text
colosseum-cli engine inspect ./my-engine
colosseum-cli engine check ./my-engine
colosseum-cli self-test
colosseum-cli status ./colosseum-runs/my-run
```

Detailed contracts:

- [Direct engine controls and compliance checks](engine-controls.md)
- [Configuration files, inheritance and path origins](run-files.md)
- [Master seed and named random streams](randomness.md)
- [Human, JSON and dry-run output](output.md)
- [Installed-executable self-test](self-test.md)
- [Run directories and checkpoint recovery](run-directories.md)
- [Official run records and read-only status](status.md)
