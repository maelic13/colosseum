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
Only the two consecutive colour assignments of one pair, with identical opening
identity, enter the pentanomial vector. Incomplete or inconsistent games stay
counted as unpaired. The usual paired statistics block is calculated when the complete
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
{s=24 d=18 t=250ms n=500000}
```

The last form is what Colosseum itself writes; see
[fixed matches](match.md) for the annotations it produces. `%emt` is
elapsed move time in seconds (a `H:M:S` value is also accepted). Key/value
`time`/`t` requires an explicit `ms` or `s` suffix, and with that suffix a
zero is read as a real sub-millisecond measurement rather than a placeholder.
Depth and nodes must be positive integers. `s`/`score` is the mover’s own
score in centipawns, or `#N` / `#-N` for mate in `N`. Other comments and
annotation tags are left untouched and ignored by telemetry analysis.

An explicit zero is a report, not a missing field: a move commented
`{s=18 d=0 t=1ms n=0}` counts towards depth, time and node coverage, because
the writer omits a field it has no value for. Only an absent field is absent.

Each engine receives an eligible post-opening move count, an annotated-move
coverage fraction, and separate coverage/mean/median reports for score,
mean absolute score, depth, elapsed seconds, nodes and implied NPS. A metric
with no valid samples is labelled `unavailable`; missing data is never
converted to zero. Implied NPS requires nodes and positive elapsed time on the
same move. A mate score counts towards score coverage but is deliberately left
out of the centipawn values: it is a claim about distance to mate, not an
evaluation on the same scale.

## Pair identity in a Colosseum PGN

The seven-tag roster says who played and how a game ended. It cannot say which
colour-reversed pair a game belongs to, and that pair is the unit every paired
statistic is computed over. Every game Colosseum writes therefore carries five
more tags beside `OpeningPlyCount`:

| Tag | Meaning |
|---|---|
| `GameNumber` | Harness game number in schedule order, counting from one |
| `PairNumber` | The colour-reversed pair, or the tournament encounter, counting from one |
| `PairGame` | Which colour assignment of that pair this is, counting from one; every even assignment is the odd one before it with the colours reversed |
| `OpeningIndex` | Zero-based index into the resolved opening order; absent without a book |
| `OpeningLabel` | The opening's label, `startpos` when no book supplied one |

`stats` reads them back, so replaying a Colosseum PGN reports the same
pentanomial vector as the checkpoint it came from. Pairing follows
`PairNumber` and `PairGame` alone, never the game numbers: a tournament
encounter played with four games per pair is two pentanomial units, and one
game per pair is none at all, whatever the games are numbered. Without a
subject the outcome is taken from the pair's first engine — the one that had
White in assignment `1` — which is the perspective the checkpoint uses.

## Games a run kept but did not count

A run's own `games.pgn` holds every game it played. Some of those games are
deliberately outside its official sample: a game the harness abandoned on an
infrastructure fault, the pairs an SPRT was still playing in its other slots
when it crossed a boundary, and the games of an SPSA iteration an engine fault
invalidated. They are kept because they are evidence, and they are marked so no
reader has to guess:

| Tag | Values |
|---|---|
| `ColosseumSample` | `official`, `unscorable` (abandoned on an infrastructure fault), `post-terminal` (played after an SPRT boundary), `invalid` (an invalidated SPSA iteration) |
| `ColosseumSpsaIteration` | Zero-based SPSA iteration the game belongs to |

An abandoned game carries a result only because the PGN shape requires one;
nobody may score it, and no run ever did. `unscorable` is per game, so it is
what a game of an otherwise counted pair carries.

`stats` counts only `official` games, so `stats <pgn>` reports the same pair
count and the same pentanomial vector as `stats <run-dir>` for the same run. It
reports how many games it left out in `excluded_games`, broken down by class in
`excluded_by_sample`, and warns about them in text. A file in which nothing is
official — the export of a single invalidated SPSA iteration, for example — is
refused with that reason rather than replayed as statistics. A game with no
`ColosseumSample` tag is official, so a PGN from any other source is unaffected.

A PGN that does not carry these tags is not paired by guesswork: the order
games appear in is not evidence that two of them share an opening, so such a
file falls back to labelled unpaired statistics. That includes exports from
other runners, and a Colosseum export whose tags were stripped.

Colosseum-generated PGNs record the non-standard `OpeningPlyCount` tag whenever
the harness pre-plays book moves. Those plies are excluded. For PGNs without
that tag, individual comments containing the word `book` are excluded. If an
external producer records opening moves in neither way, they cannot be
identified and the coverage denominator includes them.

Node accounting is engine-defined. Compare implied NPS only when node semantics
are compatible—normally versions or builds from the same engine lineage. The
JSON report and stderr warning retain this limitation whenever telemetry is
available.
