# Phase 8 exit — external parity and remaining gaps

Phase 8 is accepted. The exact 0.1.0 candidate was checked against current
FastChess and Cute Chess releases on the oracle matrix's compatible fields,
and every identified runner gap has one explicit adopt, decline or defer
decision.

## Accepted evidence

| Criterion | Evidence |
|---|---|
| Current external runners | Official FastChess 1.8.0-alpha and Cute Chess 1.5.1 release sources, versions and executable hashes are frozen |
| Exact candidate | Commit, release executable hash and raw result hash are frozen |
| Shared-field parity | All three runners agree on game/pair count, colour reversal, W/D/L, draw ratio, termination and faults; compatible pentanomial outputs agree |
| Divergences | Fixed-match versus capped-SPRT exit and zero-variance presentation are classified and excluded for stated reasons |
| Complete gap audit | Ponder, Chess960, harness Syzygy, tournament formats, output formats and datagen each have one reasoned decision and revisit boundary |
| Adopted behavior | A hermetic engine drives a real `go ponder`/`ponderhit` exchange and every gameplay workflow records the same explicit control |
| Regression | Workspace check, clippy and all-target tests pass |

The detailed parity record is
[`docs/fixtures/phase8/parity.json`](../fixtures/phase8/parity.json), the gap
record is [`docs/fixtures/phase8/gaps.json`](../fixtures/phase8/gaps.json), and
gate ownership is
[`docs/fixtures/phase8/acceptance.json`](../fixtures/phase8/acceptance.json).

## Product boundary after Phase 8

Colosseum CLI has independent, ordinary-UCI engine inspection, fixed matches,
pair-atomic SPRT, optional calibration, durable SPSA, speed/scaling experiments,
book tooling, statistics replay/planning/telemetry, position suites and static
multi-engine tournaments. It does not require engine manifests or source/build
metadata.

The remaining deferred items are intentional rather than accidental claims:
Chess960 and harness-side Syzygy probing need new correctness boundaries after
1.0. Additional event formats, interchange formats and a dedicated datagen
command have no demonstrated generic requirement beyond the current product.
The 1.0 documentation must state these boundaries plainly.
