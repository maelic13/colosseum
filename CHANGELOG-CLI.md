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
