# Worked command examples

These examples show one valid shape for every public command. Paths and budgets
are illustrative; inspect the [generated reference](command-reference.md) and
use `--dry-run --json` before a costly live workflow.

| Command | Example |
|---|---|
| `capabilities` | `colosseum-cli capabilities --json` |
| `engine inspect` | `colosseum-cli engine inspect ./engine` |
| `engine check` | `colosseum-cli engine check ./engine --option Hash=64` |
| `match` | `colosseum-cli match ./candidate ./baseline --games 100 --a-movetime-ms 100 --b-movetime-ms 100` |
| `sprt` | `colosseum-cli sprt ./candidate ./baseline --max-pairs 5000 --preset gainer` |
| `calibrate` | `colosseum-cli calibrate ./engine ./engine-copy --games 100 --a-movetime-ms 100 --b-movetime-ms 100` |
| `spsa` | `colosseum-cli spsa ./engine --tune ./tune.toml --r-end 0.002 --iterations 100 --games-per-iteration 8` |
| `spsa plan` | `colosseum-cli spsa plan --tune ./tune.toml --r-end 0.002 --pilot-game-seconds 3.2 --pilot-game-seconds 3.6` |
| `spsa status` | `colosseum-cli spsa status ./runs/tune --json` |
| `nps` | `colosseum-cli nps ./engine --nodes 10000000` |
| `book verify` | `colosseum-cli book verify ./openings.epd` |
| `book hash` | `colosseum-cli book hash ./openings.epd` |
| `book stats` | `colosseum-cli book stats ./openings.pgn --plies 12` |
| `book slice` | `colosseum-cli book slice ./openings.pgn ./subset.epd --plies 12 --count 200 --order random --seed 42` |
| `stats` | `colosseum-cli stats ./runs/gate --json` |
| `stats plan fixed` | `colosseum-cli stats plan fixed --objective difference --model normalized --effect-or-margin 3 --distribution 0.05,0.15,0.6,0.15,0.05` |
| `stats plan sprt` | `colosseum-cli stats plan sprt --model normalized --elo0 0 --elo1 3 --distribution 0.05,0.15,0.6,0.15,0.05 --max-pairs 5000` |
| `suite` | `colosseum-cli suite ./engine ./positions.epd --nodes 1000000` |
| `tournament plan` | `colosseum-cli tournament plan --engine ./a --engine ./b --engine ./c` |
| `tournament run` | `colosseum-cli tournament run --engine ./a --engine ./b --games-per-pair 2 --movetime-ms 100` |
| `gauntlet` | `colosseum-cli gauntlet --engine ./seed --engine ./challenger-a --engine ./challenger-b --games-per-pair 4` |
| `self-test` | `colosseum-cli self-test --json` |
| `status` | `colosseum-cli status ./runs/gate --json` |

An SPSA result can also define an exact gate without copying its option vector:

```text
colosseum-cli sprt --apply ./runs/tune/result.json --max-pairs 5000 --preset gainer
```
