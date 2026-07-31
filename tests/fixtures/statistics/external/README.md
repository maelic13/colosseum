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
