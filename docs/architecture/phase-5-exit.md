# Phase 5 exit — SPSA

Phase 5 is accepted as an exact, durable SPSA workflow over numeric UCI `spin`
options exposed by an ordinary engine executable. The binding gate inventory is
[`docs/fixtures/phase5/acceptance.json`](../fixtures/phase5/acceptance.json).

## Gate result

| Gate | Evidence | Result |
|---|---|---|
| Schedule, RNG and rounding | Hand-computed gain points, multi-horizon terminal invariants, named-stream replay, arm symmetry, clipping and half-away-from-zero fixtures | Pass |
| Written schedule authority | Mutated or differently derived persisted schedules cannot produce the launch token | Pass |
| Configuration audit | Missing/non-spin options, duplicates, invalid tuning bounds, requested/live range violations and sub-half-unit terminal perturbations are typed refusals | Pass |
| Pair-atomic durability | Only complete fault-free mini-matches update the floating centre; engine faults retain evidence but produce no gradient | Pass |
| Exact recovery | A killed tune resumes the stored horizon, gain iteration, perturbation stream, centre prefix and append-only evidence | Pass |
| Synthetic convergence | A seeded noisy two-dimensional quadratic starting far from its known optimum finishes within the declared 5.0 RMSE band | Pass |
| Planning | Exact schedule/workload arithmetic, comparison horizons and explicit range/pilot timing estimates match hand calculations | Pass |
| Diagnostics | Normalized trajectories, thirds, ETA and fixed heuristics match hand calculations; short history remains insufficient; live status is bounded and read-only | Pass |
| Loop closure | Stub tune output feeds `sprt --apply` unedited; executable content is verified and a mismatch is refused unless prominently overridden | Pass |
| Workspace regression | Check, clippy and every all-target workspace test are green | Pass |

## Scope of the convergence smoke

The synthetic test exists to catch schedule direction, gain, perturbation,
rounding and update regressions under reproducible noise. It is not evidence
that any chess objective will converge. Real tuning depends on unknown
curvature, sensitivity, interactions, starting distance, match noise and range
selection. `spsa plan` therefore remains factual workload arithmetic and
`spsa status` remains explicitly heuristic.

## Runtime evidence boundary

The complete loop uses the cross-platform internal UCI stub and finishes in the
required hermetic suite. No long real-engine SPSA or SPRT is required to accept
the implementation. Rarog and Basilisk remain interoperability engines, not
hidden prerequisites or statistical oracles. A real gate through the released
artifact remains Phase 9.7 evidence.
