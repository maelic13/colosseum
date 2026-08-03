# Colosseum CLI changelog

All notable released changes to Colosseum CLI will be documented here.
The CLI follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

The first public CLI release is not yet published.

- Added durable pair-atomic SPSA tuning over ordinary UCI spin options, with
  exact schedule preflight, optional books, engine-fault invalidation and
  kill/resume recovery at complete-iteration boundaries.
- Added live-schema SPSA configuration audits with durable warnings for
  non-default or rail-seeded parameters.
- Added frozen final-window SPSA result artifacts and SHA-256-verified
  `sprt --apply` gating of original versus tuned UCI vectors.
- Added offline `spsa plan` schedule, workload, rounding-resolution, horizon
  comparison and evidence-based wall-time reporting.
- Added read-only `spsa status` trajectory, thirds, ETA and explicitly
  non-causal heuristic diagnostics over atomic durable snapshots.
- Added fixed-node `nps` measurement using harness monotonic wall time, with
  reported-node verification and engine time/NPS retained only as diagnostics.
- Added seeded multi-build NPS A/B schedules with explicit warm/cold state,
  strict alternation, warm-up, per-build and arm medians, best-of summaries,
  bootstrap intervals, optional self-pair checks and round-noise diagnostics.
- Added pinned fixed-node thread-scaling sweeps with explicit thread controls,
  fixed-total/per-thread Hash, topology evidence, speedup and efficiency.
- Added engine-free EPD/PGN `book hash`, `verify`, `stats` and deterministic
  canonical-EPD `slice` utilities.
- Added statistics replay with explicit structured/checkpoint/PGN/log/console
  authority and fail-closed unpaired fallback when pair identity is absent.
- Added explicit fixed-sample difference/equivalence planning, descriptive
  achieved resolution and seeded capped SPRT expected-length simulation.
- Added read-only PGN search telemetry with documented annotations, per-engine
  coverage, opening exclusion and compatibility-labelled implied NPS.
- Added resumable fixed-time/node/depth EPD/FEN suites with legal `bm`/`am`,
  deterministic malformed/unscored outcomes and compatible baseline compare.
- Added one deterministic tournament planner for round-robin and multi-seed
  gauntlet schedules, with `gauntlet` as an alias of the same implementation.
