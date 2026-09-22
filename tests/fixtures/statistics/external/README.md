# External runner observations

These raw artifacts were generated on Windows 11 x86_64 on 2026-07-31. They
are small integration observations, not strength tests and not a source of
truth for unsupported or degenerate statistics. The full identities, hashes,
licence provenance and portable command templates are in
[provenance.json](provenance.json).

The Fastchess run intentionally used an unrealistically short clock and ended
in time forfeits. The Cutechess run used a longer short clock and ended in
mates. Both are clean sweeps from Rarog’s perspective, so their printed
`-inf`/`nan` Elo is **not** a compatible Colosseum oracle; it is retained to
prove that the fixture policy does not silently accept runner output outside
Colosseum’s typed-error domain.

The generated console logs and PGNs are preserved as reviewed runner artifacts.
The required `statistics_fixtures` test parses their complete colour pairs and
W/D/L fields—the compatible cells named in `phase-1-acceptance.toml`. It does
not treat their degenerate Elo, LOS or SPRT presentation as an oracle.

## Phase 4B parity evidence

`phase4b5-fastchess.console.txt` is a second, non-degenerate Fastchess
observation. Rarog depth 1 and Stockfish depth 2 replayed the committed
`phase4b5-openings.epd` sequentially, with a 40-pair cap and normalized
`[-50, 50]` hypotheses. Fastchess stopped at pair 10 with H0, `LLR=-3.14` and
the pair vector `[4, 4, 2, 0, 0]`; the hermetic replay test asserts that
Colosseum reaches the same first terminal prefix and matches values to the
external tool's two-decimal display precision.

The two `phase4b5-live-*.console.txt` files and the reviewed Colosseum JSON
projection record a separate same-Rarog, depth-1, four-pair smoke. All runners
completed eight adjudicated draws with colour reversal and no faults.
Fastchess and Colosseum also expose the same `[0, 0, 4, 0, 0]` pair vector.
Because the sample has zero variance, Elo, confidence, LOS and LLR presentation
are explicitly excluded. Exact versions, hashes, commands, conditions and the
raw Colosseum-output hash live in `../phase-4b-parity.toml`.
