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
Colosseum does not ship one. This includes explicit `--ponder` for a
base/increment-clock calibration; it remains off by default.

`--dry-run` resolves the complete configuration and hashes the supplied
executables without launching them. Unlike a regular match dry run, the paths
must therefore name readable executable files.

Every live calibration writes the standard self-contained run directory,
including the resolved configuration, binary identities, checkpoint, PGN,
JSON-lines log, run record and final result. It can resume only with the same
resolved configuration.

The result uses a fixed two-sided normalized-Elo interval:

| Result | Meaning | Exit code |
|---|---|---:|
| `pass` | The entire interval is within the configured tolerance. | 0 |
| `fail` | The entire interval is above or below one tolerance edge. | 1 |
| `inconclusive` | The interval overlaps an edge, or the sample has no estimable interval. | 4 |
| `invalid` | Any engine-attributable fault occurred. | 5 |

Infrastructure, persistence and runtime failures use exit code 3 and do not
claim a calibration result. An inconclusive calibration is evidence of an
unresolved measurement, not a failed engine or a reason to block another
workflow.
