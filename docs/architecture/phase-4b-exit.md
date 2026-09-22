# Phase 4B exit — pair-atomic SPRT

Phase 4B is accepted as a bounded, independent SPRT workflow over ordinary UCI
executables. The acceptance manifest is
[`docs/fixtures/phase4b/acceptance.json`](../fixtures/phase4b/acceptance.json).

## Gate result

| Gate | Evidence | Result |
|---|---|---|
| Analytic statistics | Hand-derived normalized/logistic pentanomial and trinomial fixtures, bounds, symmetry and typed degeneracy tests | Pass |
| Compatible external oracle | Reviewed Fastchess 1.8.0 normalized stream reaches H0 at the same first terminal pair (10), with matching displayed LLR and bounds | Pass |
| Completion-order invariance | Ascending, interleaved and reverse worker-completion orders produce identical H1 and H0 terminal samples; later pairs are post-terminal | Pass |
| Finite cap | A live hermetic run reaches its pair cap as `INCONCLUSIVE`, preserving complete official pairs and durable artifacts | Pass |
| Fault policy | Timeout, crash, disconnect, protocol and illegal-move faults invalidate under the strict default; non-scorable infrastructure failure is rejected without entering the sample | Pass |
| Automation exits | H1=0, H0=1, configuration=2, infrastructure/runtime error=3, inconclusive=4 and invalid=5 are distinct and tested | Pass |
| Controlled live parity | Fastchess, Cute Chess and Colosseum agree on the shared same-binary scheduling, outcome, adjudication and fault fields | Pass |

## Root-caused live differences

The live smoke intentionally used the same Rarog executable on both sides at
depth 1, no book, four reversed-colour pairs, one worker and permissive draw
adjudication. Every runner produced eight adjudicated draws and no faults.

The sample has zero variance. Fastchess and Cute Chess print combinations of
zero, `NaN` and infinity for Elo/LOS/sequential presentation; Colosseum retains
typed non-estimability and reports no statistics. These values are different
interfaces to an undefined estimate, not a result disagreement, and the oracle
matrix forbids comparing them. Cute Chess also did not run the same normalized
pentanomial design, so its LLR/bounds are excluded. Fastchess's no-book
`Unknown opening format, 2` warning is its default-format presentation; all
eight scheduled start-position games completed as requested.

The exact external versions, hashes, commands, output hashes and compared or
excluded fields are fixed in
[`tests/fixtures/statistics/phase-4b-parity.toml`](../../tests/fixtures/statistics/phase-4b-parity.toml).
No required test launches an external runner or reads outside the repository.

## Remaining boundary

This exit establishes runner/statistical mechanics, not machine neutrality or
engine strength. Optional calibration remains Phase 4C. Release-candidate
parity on current external versions remains Phase 8.1. A user opening book is
still optional and is never shipped or silently required by the CLI.
