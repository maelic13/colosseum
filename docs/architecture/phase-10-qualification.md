# Phase 10 qualification on a real engine

Release acceptance for Colosseum CLI 0.1.0 (PLAN §Phase 10(ab), GUIDE 10.9r):
the harness measured against a second runner, fastchess 1.8.0-alpha
(`072859b`), on the reference host, with Rarog as the validation engine. Each
check was run by the maintainer, one command at a time, and analysed from its
run directory before the next. An adopting project trusts the released binary
and repeats none of this. Symmetry is its own step, 10.9v.

**Reference host:** AMD Ryzen 9 5950X (16 cores, 32 threads), 128 GB,
Windows 11 build 26200. **Common conditions:** 3+0.03 with a 20 ms margin,
Hash 64, Threads 1, 14 concurrent games with `--placement auto` (cores 1–14,
both SMT siblings per game), engine processes kept per slot (the default), the
UHO_Lichess_4852_v1 book in random order (sha256 `7A7F6470…35644A`), no
adjudication.

## Candidates

| Colosseum source | CLI binary sha256 | Used for |
|---|---|---|
| `22782ec` | `cb324c221edfe4be438204c815bd05e28ddd36417de81bd6e22e4b70d80f77c1` | scale |
| `9f7fafc` | `6998bf8dde5a88fac971f4d5f7a3bdeaa342892df250a0b099c710db6428bc85` | verdict |
| `99091bc` | `9bb751e045654413881c35f2e5776e4cc3dd9cfb27214450714e9b4ed79344d8` | ratings onward |

Between them: the text of the `faults` line (`time: 0-2` for `time: b22core
0, b22base 2`), at the maintainer's request; the SPRT progress block's time
remaining once the LLR has crossed a bound (it showed the whole cap); and the
default time margin, 2,000 ms to 20 ms, by maintainer decision. Every run here
states its 20 ms margin explicitly, so none of these changes what they
measured.

## 1. Scale — passed

Two builds whose difference fastchess has measured, a fixed 2,000 games.

```text
colosseum-cli match rarog-b22core-pext-pgo.exe rarog-b22base-pext-pgo.exe \
  --a-label b22core --b-label b22base --games 2000 \
  --a-base-ms 3000 --a-increment-ms 30 --b-base-ms 3000 --b-increment-ms 30 \
  --a-option Hash=64 --a-option Threads=1 --b-option Hash=64 --b-option Threads=1 \
  --a-margin-ms 20 --b-margin-ms 20 --book UHO_Lichess_4852_v1.epd \
  --book-order random --seed 44 --placement auto --concurrency 14 \
  --dir colosseum-qual-scale
```

Engines: `rarog-b22core-pext-pgo.exe` (sha256 `C51476EBE81E…`, bench
4,706,910) and `rarog-b22base-pext-pgo.exe` (`CDAF2AE5090D…`, 7,601,220),
both matching their sidecars.

| | Colosseum 0.1.0 | fastchess, same binaries |
|---|---|---|
| Elo | **+54.3 ± 10.6** | +52.2 ± 10.7, +54.3 ± 11.1 |
| nElo | +79.5 ± 15.2 | +75.6 ± 15.2 (second run) |
| Ptnml | [31, 155, 391, 319, 104] | |
| Time losses | **0** | 0, 0 |

The intervals overlap almost exactly. From the run directory: 2,000 official
games, no fault of any kind, 14 slots with no overlapping slot span, 5,579
games per hour. With engines kept per slot, start-up is 7.3 ms per game at
the median (a slot's first game about 200 ms) and teardown 0.1 ms, against
42.0 ms and 10.7 ms with fresh processes per game in the 10.9m baseline on
the same binaries. The closest any move came to its deadline was 51 ms (18 ms
in the baseline), and no move was charged past Rarog's own hard-limit bound
by more than 10 ms (21 in the baseline): the KPK bitbase these pre-fix
binaries build on first use is now built at most once per process. The
largest time a search was held off its CPU was 27.7 ms (p99 of the per-side
maxima 8.2 ms).

## 2. Verdict — passed

An SPRT of a pair fastchess has gated, under its conditions: normalized
`[0, 10]`, α = β = 0.05, at most 8,000 pairs.

```text
colosseum-cli sprt rarog-b24a-core-pext-pgo.exe rarog-b24a-base-pext-pgo.exe \
  --a-label b24a-core --b-label b24a-base --model normalized --elo0 0 --elo1 10 \
  --alpha 0.05 --beta 0.05 --max-pairs 8000 \
  --a-base-ms 3000 --a-increment-ms 30 --b-base-ms 3000 --b-increment-ms 30 \
  --a-option Hash=64 --a-option Threads=1 --b-option Hash=64 --b-option Threads=1 \
  --a-margin-ms 20 --b-margin-ms 20 --book UHO_Lichess_4852_v1.epd \
  --book-order random --seed 45 --placement auto --concurrency 14 \
  --dir colosseum-qual-verdict
```

Engines: `rarog-b24a-core-pext-pgo.exe` (sha256 `9206A59884D0…`) and
`rarog-b24a-base-pext-pgo.exe` (`4EC72F0FA40F…`), the hashes fastchess's
manifest records.

| | Colosseum 0.1.0 | fastchess, same binaries and design |
|---|---|---|
| Verdict | **H1 accepted** | H1 accepted |
| Pairs to the decision | 241 | 216 |
| nElo | +83.0 ± 31.0 | +94.0 ± 32.8 |
| Elo | +60.4 ± 23.1 | |
| LLR at the stop | 2.951 of 2.944 | |
| Time losses | **0** | 0 |

Same decision, intervals overlapping; where a sequential test stops depends
on the openings drawn. Sixteen pairs finished after the terminal pair and are
kept outside the official sample. 5,402 games per hour, a pair holding its
slot for both colours. The run's last progress block showed "time remaining
2h52m" beside "accept H1": a display defect, fixed in `22637fd`.
