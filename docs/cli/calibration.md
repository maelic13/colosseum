# Optional calibration

`colosseum-cli calibrate` is an optional fixed-N identical-binary symmetry
measurement. It is useful evidence after changing clocks, scheduling, affinity
or runner code. It is not a prerequisite for a match, SPRT, tuning run or
release.

Both executable files must have the same SHA-256 digest. This is a content
check, not an engine manifest or build-system requirement: each side remains
an ordinary UCI process and may use its normal arguments and UCI options.

```text
colosseum-cli calibrate ./my-engine ./my-engine \
  --games 30000 --confidence 0.95 --tolerance-nelo 5 \
  --book ./openings.epd --concurrency 4 --placement auto
```

The defaults are 30,000 games, 95% confidence and an inclusive ±5 normalized
Elo tolerance. The game count must be even so both colours of every opening
pair are retained. The same time-control, book, adjudication, concurrency and
CPU-placement controls as `match` are available; choose values representative
of the experiments whose symmetry you want to observe. A book is optional and
Colosseum does not ship one.

`--dry-run` resolves the complete configuration and hashes the supplied
executables without launching them. Unlike a regular match dry run, the paths
must therefore name readable executable files.

Every live calibration writes the standard self-contained run directory,
including the resolved configuration, binary identities, checkpoint, PGN,
JSON-lines log, run record and final result. It can resume only with the same
resolved configuration.
