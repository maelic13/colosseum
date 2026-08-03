# Position suites

`colosseum-cli suite` runs one ordinary UCI engine over a line-oriented EPD or
FEN position set. Select exactly one fixed-work limit:

```text
colosseum-cli suite ./engine ./positions.epd --nodes 1000000
colosseum-cli suite ./engine ./positions.epd --depth 16 --dir ./suite-run
colosseum-cli suite ./engine ./positions.fen --movetime-ms 500 --format fen
```

The first positional argument is the engine executable and the second is the
position file. All common direct engine arguments, working-directory,
environment, UCI-option and CPU controls apply. `--deadline-ms` is a safety
deadline, not a fourth search limit; by default it is ten minutes for
node/depth work and twice the move time plus five seconds for fixed time.

## EPD expectations

EPD uses its standard four position fields. `bm` accepts one or more legal SAN
or UCI moves; the engine passes when its UCI `bestmove` is in that set. `am`
passes when `bestmove` is outside its set. A position without either operation
is searched and labelled `unscored`, so missing expectations never become a
failure or a pass.

```text
rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - bm e4 d4; id "opening";
8/8/8/8/8/8/K7/7k w - - am Ka3; id "avoid";
```

An entry with an illegal position, illegal expectation, duplicate `bm`/`am`, or
both `bm` and `am` is retained as `malformed` and is not sent to the engine.
Unknown operations are copied into that position's result and otherwise
ignored. Colosseum never infers a custom `bench`, `perft` or diagnostic command.
Line-oriented FEN files are legal-position workloads without `bm`/`am`, so all
their completed searches are unscored.

The report includes every input line's outcome, best move and harness latency,
plus searched/assessed/pass/fail/unscored/malformed counts. Pass rate is
`passed / (passed + failed)` and is `unavailable` when nothing is assessed. A
failed expectation or malformed entry returns exit code 1 after writing the
complete JSON artifacts.

## Durability and comparison

Every suite is a durable run. Without `--dir`, Colosseum creates a unique
directory below `./colosseum-runs/`. An existing matching explicit directory
resumes from its checksummed checkpoint, skipping every committed input index;
`--restart` archives the old directory. `result.json` is the portable final
suite report and `status <DIR>` reads the common run record.

Compare with a previous result using `--baseline PATH`. Comparison requires
the same versioned parsed position-set identity and the same fixed-work search
identity (limit and safety deadline); incompatible inputs are refused. The
engine executable is deliberately not part of that compatibility pair, since
comparing two engine versions is the purpose. The report shows pass-count/rate
deltas and the exact input indices whose outcomes changed.
