# Phase 4C exit — optional calibration

Phase 4C is accepted as an optional, durable identical-binary symmetry
measurement over ordinary UCI executables. It is evidence about one machine
and resolved experiment configuration, never a prerequisite or proof of
runner correctness. The acceptance manifest is
[`docs/fixtures/phase4c/acceptance.json`](../fixtures/phase4c/acceptance.json).

## Gate result

| Gate | Evidence | Result |
|---|---|---|
| Binary identity | Both executable contents are hashed before launch; unequal SHA-256 identities are refused | Pass |
| Configuration durability | Design, binary identities, clocks, adjudication, openings, concurrency, placement and seed round-trip through resolved config, checkpoint, result and run record | Pass |
| Resume identity | A killed hermetic run resumes only its missing games with the exact stored configuration; a changed tolerance is refused | Pass |
| Outcome classification | Exact-boundary fixtures cover PASS, either-sided FAIL, overlapping and non-estimable INCONCLUSIVE, and fault-dominant INVALID | Pass |
| Automation exits | PASS=0, FAIL=1, configuration=2, infrastructure/runtime=3, INCONCLUSIVE=4 and INVALID=5 are distinct | Pass |
| Real-engine path | Basilisk 1.9.0 completed four depth-1 games with equal binary hashes, two concurrent slots, enforced disjoint CPU affinity and no faults | Pass |
| Workspace regression | Required check, clippy and all-target test suites pass | Pass |

## Real-machine smoke interpretation

The Windows development-host smoke deliberately used only two complete pairs,
no book, depth 1 and a two-move cap. Both identical Basilisk sides completed
four draws. The resulting all-central pentanomial sample has zero variance, so
`INCONCLUSIVE` is the only valid statistical result. This is successful smoke
evidence for UCI launch, content hashing, colour reversal, concurrent
scheduling, enforced placement and durable output; it is not a claim that the
full ±5 nElo default tolerance was measured.

The exact content hash, seed, CPU allocation, sample and result are retained in
the acceptance manifest. No required test reads the developer's engine folder
or launches a real engine.

## Remaining boundary

Calibration remains optional. A full representative run may be useful after a
material change to clock accounting, scheduling or affinity, but it does not
gate another workflow. Phase 5 may now build SPSA over the accepted paired,
durable runner and statistical boundaries.
