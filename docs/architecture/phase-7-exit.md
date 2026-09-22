# Phase 7 exit — tournaments

Phase 7 is accepted by one hermetic gate over the independent CLI composition
root. No test imports GUI state, the GUI executable, SQLite, or the legacy
tournament scheduler.

## Accepted evidence

| Criterion | Evidence |
|---|---|
| Shared round-robin schedule | The frozen GUI-origin fixture matches every game number, round, colour and participant ID produced by `PlanTournament` |
| Shared multi-seed gauntlet schedule | The same fixture covers two seeds against two opponents and excludes seed/seed and opponent/opponent games |
| GUI rating parity | A stored result vector and non-uniform priors reproduce the GUI/shared-core joint ML ratings within 0.01 Elo |
| Durable round-robin | A deterministic four-engine run is killed after a partial checkpoint, resumed, and compared with an uninterrupted run |
| Durable multi-seed gauntlet | The same kill/resume comparison covers a two-seed gauntlet |
| Deterministic result | Resumed schedule, standings, error bars and crosstable equal uninterrupted output; every schedule number occurs exactly once |

The durable fixture is
[`docs/fixtures/phase7/gui-parity.json`](../fixtures/phase7/gui-parity.json).
Gate ownership is recorded in
[`docs/fixtures/phase7/acceptance.json`](../fixtures/phase7/acceptance.json),
and executed by
[`crates/colosseum-cli/tests/phase7_acceptance.rs`](../../crates/colosseum-cli/tests/phase7_acceptance.rs).

## Evidence boundary

Deterministic UCI stubs prove scheduling, process driving, atomic persistence,
resume and result aggregation without machine-dependent chess outcomes. They
do not claim that a particular real engine is stable, correctly configured, or
strong. Real-engine tournaments remain useful smoke evidence, not a prerequisite
for the correctness of the generic tournament mechanism.
