# Statistics fixture corpus

This directory is the reproducible evidence set for `colosseum-core` paired
statistics. It has two deliberately separate sources of truth:

| Source | Purpose | CI role |
|---|---|---|
| `analytic-pentanomial.toml` | Hand-derived inputs and expected values for every Colosseum formula and error boundary | Hermetic regression oracle |
| `external/` | Raw output from current `fastchess` and `cutechess-cli` runs, plus immutable tool/engine provenance | Compatibility evidence; never required by a test that launches an external program |

The oracle matrix in [oracle-matrix.md](oracle-matrix.md) says exactly which
fields may be compared. A runner cannot become an oracle for a model it does
not expose. In particular, external output that prints `NaN` or infinity for a
clean sweep is retained as forensic evidence, not accepted as Colosseum output:
Colosseum returns `StatisticsError` for such a sample.

## Recreating an external observation

Use `scripts/Generate-StatisticsFixture.ps1`. It accepts ordinary UCI
executables, never source trees or manifests, and records the SHA-256 and
licence/source provenance supplied by the caller. It writes a new directory and
refuses to overwrite old evidence.

```powershell
pwsh -File scripts/Generate-StatisticsFixture.ps1 `
  -Fastchess C:\tools\fastchess.exe `
  -Cutechess C:\tools\cutechess-cli.exe `
  -EngineA C:\engines\candidate.exe -EngineAName Candidate `
  -EngineASource https://example.invalid/candidate -EngineALicense GPL-3.0-or-later `
  -EngineB C:\engines\baseline.exe -EngineBName Baseline `
  -EngineBSource https://example.invalid/baseline -EngineBLicense GPL-3.0-or-later `
  -OutputDirectory tests/fixtures/statistics/external/my-run
```

The resulting directory contains both runner consoles, PGNs and
`provenance.json`. Copy only reviewed, useful evidence into `external/`; do
not replace a fixture merely because a newer runner is available. A changed
tool/engine hash, command condition or result changes the fixture identity.

The supplied generator intentionally runs a small no-book smoke match. It
proves output shape and captures provenance, not engine strength. Before a
field is promoted to an oracle comparison, generate a representative,
non-degenerate pair sample and document its exact book, time control,
adjudication and command conditions in a new fixture directory.

## Fixture rules

- Preserve runner stdout/stderr and PGN content as generated; put explanation
  in this document or the accompanying provenance file, never by editing a
  reviewed artifact.
- Record tool version **and executable SHA-256**, engine identity/version and
  SHA-256, source URL/revision where available, and licence provenance.
- Do not commit runner or engine binaries. The hashes identify them without
  making Colosseum a distributor.
- Never make an external engine, local path, or runner installation a required
  CI input. Phase 1.8 owns that enforcement.
- Analytic fixture values are independently derived from the formulas in
  `PLAN.md` §5.1. Phase 1.9 wires every analytic case and compatible matrix
  cell into the automated acceptance check.
