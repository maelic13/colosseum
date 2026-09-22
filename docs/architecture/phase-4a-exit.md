# Phase 4A exit — fixed matches

Phase 4A is accepted when an ordinary pair of UCI executable paths can run a
fixed match with deterministic scheduling, correct clock/fault accounting and
recoverable self-contained output.

| Criterion | Evidence | Result |
|---|---|---|
| Path-only, no-book match | Command-line acceptance launches the repository UCI fixture twice with no descriptor or manifest and reports the explicit startpos-diversity warning | Pass |
| Optional paired EPD/PGN books | Book tests pin one assignment per colour-reversed pair, seeded order, start/ply controls and reuse fraction | Pass |
| Per-side controls and adjudication | Unit and command-line tests cover every clock/search mode, independent margins, defaults, disable switches and maximum moves | Pass |
| Clock model and real elapsed charging | A cross-platform fixture sleeps 80 ms; its `go`-flush-to-`bestmove`-read charge is asserted within a documented 50–500 ms scheduler/pipe tolerance | Pass |
| Margin boundary and attribution | A sub-margin overrun is accepted; a clear super-margin overrun forfeits the correct named side. Pure clock fixtures pin below/equal/above `R + M` and deduction-before-increment exactly | Pass |
| Wall-clock independence | The production elapsed function accepts only monotonic `Instant` values; a regression fixture moves wall time backwards during the represented interval without changing the charge | Pass |
| Fault classification | Engine crash/protocol/illegal/timeout paths are typed forfeits; missing executable and hard-placement failure are non-scorable infrastructure outcomes | Pass |
| Concurrency-independent schedule | The same seed/book produces identical game numbers, colours, opening assignments and outcomes at concurrency 1 and 3 | Pass |
| Durable interruption and resume | A spawned 20-game match is killed after its first checkpoint, resumed from the same directory and finishes with each scheduled game exactly once in order | Pass |
| Output and automation contract | Tests assert resolved config, checksummed checkpoint generations, append-only log, PGN, final JSON, run record, strict JSON stdout and exit codes 0/1/2/3 | Pass |
| Existing product behavior | Full workspace check, clippy and all-target tests remain green | Pass |

The required suite is hermetic and uses only repository-built executables and
temporary directories. It does not claim live strength parity with an external
runner; pair-atomic SPRT and the external-statistics/live-parity gate are Phase
4B.

Local acceptance evidence for this change was produced on Windows. The same
tests are included in the required Windows/Linux/macOS debug and release CI
matrix.
