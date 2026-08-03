# Phase 6 exit — speed, replay and position tools

Phase 6 is accepted as an engine-independent diagnostic/tooling layer over
ordinary UCI executables and standard EPD/FEN/PGN artifacts. The binding gate
inventory is
[`docs/fixtures/phase6/acceptance.json`](../fixtures/phase6/acceptance.json).

## Gate result

| Gate | Evidence | Result |
|---|---|---|
| Fixed-node authority | Requested nodes and harness monotonic elapsed time alone determine NPS; reported nodes only verify completed work and fake `info nps` is diagnostic | Pass |
| Robust A/B summary | A left-skew fixture strongly biases a naive mean of paired ratios while equal arm medians remain equal | Pass |
| Search state | Process-count fixtures prove cold restarts every sample and warm retains one session per arm with `ucinewgame`/`isready` | Pass |
| Scaling | Hand arithmetic covers speedup, efficiency and fixed-total/per-thread Hash; topology/affinity fixtures cover exact physical-core enforcement | Pass |
| Book tools | Hash/verify/stats/slice use legal parsed candidates and seeded slices are byte-identical | Pass |
| Statistics replay | Structured/checkpoint/PGN/log/console authority is audited and missing pair identity never becomes a guessed pair | Pass |
| Planning | Fixed-N analytic output and dedicated-stream capped SPRT simulation match deterministic fixtures | Pass |
| PGN telemetry | Both documented syntaxes match hand aggregates, opening plies are excluded and missing metrics stay unavailable | Pass |
| Position suites | Legal multi-move `bm`/`am`, unscored/malformed input, baseline identity and kill/resume without duplicate positions pass | Pass |
| Workspace regression | Check, clippy and every all-target workspace test are green | Pass |

## Evidence boundary

The required gate is hermetic. It validates workload accounting, scheduling,
statistics, parsing, persistence and protocol behavior with deterministic
fixtures; it does not claim a performance result for a particular chess
engine or machine. A real NPS/scaling result remains meaningful only on the
recorded host under controlled load. Real Rarog/Basilisk workflows remain
interoperability and release evidence, not hidden test dependencies.
