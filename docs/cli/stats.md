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
