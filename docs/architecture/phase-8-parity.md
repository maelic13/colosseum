# Phase 8.1 — release-candidate runner parity

The controlled Phase-4B live comparison was repeated on 2026-08-03 against
current FastChess and Cute Chess releases and the exact Colosseum CLI release
candidate. Each run used the same Rarog 2.3.1 executable on both arms, depth 1,
four colour-reversed pairs, no opening book, concurrency 1 and deliberately
early draw adjudication. Each runner finished in seconds.

## Reproducible identities

| Component | Version / commit | SHA-256 |
|---|---|---|
| FastChess | 1.8.0-alpha, CI commit `072859b` | `8444e73965ae44e716cde1bb546a7d7c8c9fc7a442a44194a0c71a3bffa7dd0d` |
| Cute Chess CLI | 1.5.1, Qt 6.8.3 | `8889f9582dc688c567704cf083f6025baf77f791cde903698c70b3420caf5d7e` |
| Colosseum CLI | 0.1.0 candidate at `86fc42b442d0f2a354a1fcc1ec5c09cad47a0f43` | `652e1c41cb16261c15a07cdcd1f18cfbf855957b0b7e57794006eee80e97a16f` |
| Rarog | 2.3.1 Windows AVX2 | `2a95390ddff846ffcc132494d1a28dcde8b703d1d9ec57aa7199a3b856ab916b` |

The versions were checked against the official
[FastChess 1.8.0-alpha release](https://github.com/Disservin/fastchess/releases/tag/v1.8.0-alpha)
and [Cute Chess 1.5.1 release](https://github.com/cutechess/cutechess/releases/tag/v1.5.1).

## Commands

Paths are represented by stable placeholders because executable location is
not part of runner semantics.

```text
<fastchess> -engine cmd=<rarog> name=A -engine cmd=<rarog> name=B -each depth=1 -rounds 4 -repeat -concurrency 1 -draw movenumber=5 movecount=2 score=10000 -ratinginterval 8

<cutechess-cli> -engine cmd=<rarog> name=A -engine cmd=<rarog> name=B -each proto=uci tc=inf depth=1 -rounds 4 -games 2 -repeat -concurrency 1 -draw movenumber=5 movecount=2 score=10000 -ratinginterval 8

<colosseum-cli> sprt <rarog> <rarog> --max-pairs 4 --preset gainer --a-depth 1 --b-depth 1 --draw-move 5 --draw-moves 2 --draw-score-cp 10000 --dir <run-dir> --json --seed 123 --placement off
```

## Result and boundary

All three runners agree on every shared oracle field: eight games, four
complete colour-reversed pairs, 0/8/0 W/D/L, 100% draws, draw-adjudication
termination and zero engine faults. FastChess and Colosseum additionally agree
on pentanomial `[0, 0, 4, 0, 0]`.

Two differences are expected and are not parity failures:

- FastChess and Cute Chess completed fixed matches with exit 0. Colosseum ran a
  deliberately capped SPRT, so it correctly returned `INCONCLUSIVE` and exit 4.
- The all-draw sample has zero variance. External display of NaN/infinity and
  Colosseum's typed unavailability are presentation differences, not finite
  statistics to compare.

The exact evidence, hashes, commands and exclusions live in
[`docs/fixtures/phase8/parity.json`](../fixtures/phase8/parity.json). The
Colosseum projection is
[`tests/fixtures/statistics/external/phase8-colosseum.json`](../../tests/fixtures/statistics/external/phase8-colosseum.json).
The acceptance test verifies both external transcripts, colour reversal,
artifact hashes, candidate identity and every recorded decision without
requiring external binaries during ordinary test runs.
