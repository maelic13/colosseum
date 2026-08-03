# Statistics replay

`colosseum-cli stats <PATH>` reconstructs match results without launching an
engine. `PATH` may be a CLI run directory, structured JSON, PGN, JSON-lines log,
or plain console text.

For a run directory, authority is fixed and visible:

1. final structured `result.json`;
2. checksum-verified current checkpoint, then its previous generation;
3. portable `games.pgn`;
4. forensic `run.log`;
5. observational `console.txt`.

Every attempted source and rejection reason is included in JSON. A corrupt
stronger source therefore cannot silently pretend to be authoritative, while a
valid weaker artifact remains usable.

Structured match games carry schedule number, side and opening assignment.
Only exact odd/even colour-reversed companions with identical opening identity
enter the pentanomial vector. Incomplete or inconsistent games stay counted as
unpaired. The usual paired statistics block is calculated when the complete
sample is sufficient and non-degenerate; otherwise its precise reason is
reported.

PGN and console text do not prove Colosseum pair/opening identity, so replay
reports labelled unpaired W/D/L and never guesses pairs from file order. Pass
`--subject "Engine name"` to select an engine perspective for PGN; without it,
PGN/console results use White's perspective. JSON-lines `game-completed` events
retain structured match identity and can reconstruct pairs when complete.

## Prospective experiment planning

Planning is engine-free and requires the statistical assumptions on the
command line. A fixed-sample difference design can also describe the interval
resolution already achieved by an observed pentanomial sample:

```text
colosseum-cli stats plan fixed --objective difference --model normalized \
  --effect-or-margin 5 --significance 0.05 --power 0.8 \
  --distribution 0.05,0.20,0.50,0.20,0.05 \
  --observed-pentanomial 5,20,50,20,5
```

Use `--objective equivalence` for a symmetric two-one-sided-test (TOST)
approximation. Here `--effect-or-margin` is the positive equivalence margin,
the assumed true effect is zero, and both one-sided tests must pass during the
actual fixed-sample analysis. Difference planning uses a two-sided test around
zero. Both calculations use a normal approximation and the supplied
pentanomial distribution for pair-score variance; output is in complete pairs
and twice as many games.

Expected SPRT length is a seeded Monte Carlo planning aid:

```text
colosseum-cli stats plan sprt --model normalized --elo0 0 --elo1 5 \
  --alpha 0.05 --beta 0.05 \
  --distribution 0.05,0.20,0.50,0.20,0.05 \
  --simulations 1000 --max-pairs 100000 --seed 42
```

The distribution is the assumed true pentanomial distribution. The report
retains it together with the hypotheses, error rates, seed, simulation cap,
stable named RNG stream and sampling algorithm. Capped trials are reported,
not discarded. The resulting length distribution is neither an SPRT stopping
rule nor a guarantee for the eventual engines or workload. Use `--json` for
the complete machine-readable reports.

## PGN search telemetry

When the authoritative source is PGN, `stats` reads search annotations from
mainline move comments. It supports exactly these forms:

```text
{[%depth 18] [%emt 0.250] [%nodes 500000]}
{depth=18 time=250ms nodes=500000}
{d=18 t=0.250s n=500000}
```

`%emt` is elapsed move time in seconds (a `H:M:S` value is also accepted).
Key/value `time`/`t` requires an explicit `ms` or `s` suffix. Depth and nodes
must be positive integers. Other comments and annotation tags are left
untouched and ignored by telemetry analysis.

Each engine receives an eligible post-opening move count, an annotated-move
coverage fraction, and separate coverage/mean/median reports for depth,
elapsed seconds, nodes and implied NPS. A metric with no valid samples is
labelled `unavailable`; missing data is never converted to zero. Implied NPS
requires nodes and positive elapsed time on the same move.

Colosseum-generated PGNs record the non-standard `OpeningPlyCount` tag whenever
the harness pre-plays book moves. Those plies are excluded. For PGNs without
that tag, individual comments containing the word `book` are excluded. If an
external producer records opening moves in neither way, they cannot be
identified and the coverage denominator includes them.

Node accounting is engine-defined. Compare implied NPS only when node semantics
are compatible—normally versions or builds from the same engine lineage. The
JSON report and stderr warning retain this limitation whenever telemetry is
available.
