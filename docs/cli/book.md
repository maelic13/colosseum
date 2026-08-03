# Opening-book utilities

`colosseum-cli book` works on EPD or PGN files without launching an engine.
The format follows the file extension; use `--format epd|pgn` to override it.
For PGN, `--plies` selects how many half-moves form each opening.

```powershell
colosseum-cli book verify openings.epd
colosseum-cli book stats openings.pgn --plies 12 --json
colosseum-cli book hash openings.epd
colosseum-cli book slice openings.pgn subset.epd --plies 12 --count 200 --order random --seed 42
```

`verify` accounts for every non-comment EPD line or PGN game. It exits with 1
and lists one-based rejected candidate indices when any candidate is malformed,
or when none is usable. Normal parsing never turns rejection into startpos.

`hash` reports SHA-256 over the exact input bytes. `stats` reports that identity
alongside byte size, candidate/usable/rejected counts, unique and duplicate
resolved openings, min/mean/max retained plies, and an evaluation band when EPD
`ce` (centipawns) or PGN `[%eval ...]` (pawns) annotations are present. Units
remain explicit and are never mixed.

`slice` first requires a clean verification, then applies the requested start,
count and sequential or versioned named-stream random order. The output is
canonical EPD: a PGN opening is replayed and materialized as its resulting
position. JSON records input/output hashes, seed, order, requested range and
actual count. Existing output is refused unless `--force` is explicit; input
and output must differ.
