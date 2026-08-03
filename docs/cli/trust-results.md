# How to trust a result

Colosseum makes experiment inputs and failure states visible, but it cannot
choose sound scientific policy for an engine project. Use this checklist before
acting on a number.

1. Treat a declared sequential test as the strength verdict. Evaluation loss,
   depth, nodes and NPS are diagnostics and can move opposite to playing
   strength.
2. Do not interpret a null result as proof of no effect. Plan fixed-N work from
   significance, power and an assumed outcome distribution, then report the
   achieved interval. An SPRT H0 applies only to its printed hypotheses and
   model.
3. Choose the right question. A normalized-Elo `[-3, +3]` SPRT is not an
   equivalence test; non-inferiority is conventionally `[-3, 0]`, while
   equivalence uses a fixed-N interval-containment rule.
4. Prefer colour-reversed opening pairs and pentanomial statistics. The two
   games from one opening are correlated; treating them as independent makes
   uncertainty look smaller than it is. Normalized Elo remains more comparable
   across different draw rates than logistic Elo.
5. Validate the instrument. Run a same-binary self pair after material timing,
   scheduler or affinity changes. Optional `calibrate` evidence should be near
   zero; calibration is useful diagnosis, not a prerequisite for every run.
6. Keep tuning and gating conditions identical: time control, book,
   adjudication, engine options, concurrency and placement. Otherwise the gate
   measures a different objective from the tune.
7. Treat the opening set as part of the measurement. Unbalanced openings are
   unbiased when colour-reversed and often resolve faster; balanced openings
   compare more naturally with rating lists. Avoid silent reuse and inspect the
   reported uniqueness/reuse evidence.
8. Control CPU placement for clock-based work. Do not oversubscribe physical
   cores, and allocate enough cores per engine for its configured threads.
   Retain the reported topology and enforcement status with the result.
9. Report speed as speed. Never convert an NPS percentage into Elo without a
   separately measured, engine- and condition-specific relationship.

For a result you intend to publish or use as a merge gate, also:

- commit or archive the exact run/tune files and master seed;
- retain the self-contained run directory, `run-record.json`, result,
  checkpoints, PGN and configuration hash;
- verify that the run reached its intended terminal state rather than a cap,
  invalid state, crash threshold or infrastructure failure;
- name the Elo model, hypotheses or fixed-N objective, error rates, game/pair
  count, time control, adjudication, book hash/reuse and engine options;
- inspect anomalies and incomplete pairs instead of quoting only the headline
  Elo value;
- reproduce surprising conclusions with an independent run or external runner
  before making a costly decision.

`stats plan fixed`, `stats plan sprt`, `stats`, `status`, `book verify` and
`calibrate` provide the supporting evidence; none can turn an underspecified
experiment into a universal claim.
