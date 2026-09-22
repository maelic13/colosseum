# Colosseum CLI changelog

All notable released changes to Colosseum CLI will be documented here.
The CLI follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-09-22

First public release.

**What `0.x` means here.** The command surface, run-file schema, run-directory
layout, JSON report schemas and exit codes may still change in a `0.x` minor
release; when one does, this changelog says so. Pin an archive by its SHA-256
if your automation depends on them. Version `1.0.0` is the promise that they
keep working.

Download an archive from the
[Colosseum CLI 0.1.0 release](https://github.com/maelic13/colosseum/releases/tag/cli-v0.1.0),
then follow the
[version-matched CLI guide](https://github.com/maelic13/colosseum/blob/cli-v0.1.0/docs/cli/README.md).

- Added an independent headless harness for ordinary UCI executables, with
  engine inspection/compliance checks, fixed matches, pair-atomic capped SPRT
  gates and optional fixed-sample calibration.
- Added reproducible run records with executable/input hashes, one master seed,
  resolved configuration, explicit statistical and clock models, fault
  evidence, bounded CPU placement and durable resume for long workflows.
- Games are played out: draw and resignation adjudication are off unless
  `--draw-adjudication` or `--resign-adjudication` asks for them, because every
  adjudication rule ends games the engines would otherwise have had to convert.
  A rule parameter supplied without its enabling flag is refused rather than
  ignored. The guide names settings in common use elsewhere for anyone who
  wants the throughput instead.
- Added `--placement auto` that reads core class, NUMA node and last-level
  cache domain from the operating system: it leaves one whole physical core
  free, uses only the highest-performance class on a host whose classes differ,
  and keeps each game slot inside one cache domain and one node when the pool
  allows it. Where the operating system's own evidence cannot decide, it
  refuses and names the topology instead of guessing.
- Added a clean stop for every long-running command. One interrupt stops
  launching new work, lets the games in flight finish within a bounded grace
  period, checkpoints, records the run cancelled and exits with code `6`; a
  second interrupt abandons them at once. Either way the run directory resumes.
- Opening books are consumed in order from `--book-start`, and a schedule that
  needs more entries than remain is refused with the shortfall named rather
  than quietly replaying openings it has already played. `--book-wrap` opts
  into reuse; `--dry-run` reports the exact index range a run will consume.
- Added optional inheritable TOML run files that map directly to public CLI
  commands/options, resolve paths relative to their declaring file and allow
  command-line replacement or strict unsetting of inherited options.
- Added versioned offline user documentation with a parser-generated command
  reference, quickstart, worked examples, format references, compatibility
  contract and result-trust guidance.
- Added durable pair-atomic SPSA tuning over ordinary UCI spin options, with
  exact schedule preflight, optional books, engine-fault invalidation and
  kill/resume recovery at complete-iteration boundaries.
- Added live-schema SPSA configuration audits with durable warnings for
  non-default or rail-seeded parameters.
- Added SPSA result artifacts naming the estimator that produced them — the
  final centre vector by default, or an optional tail-window mean — and
  SHA-256-verified `sprt --apply` gating of original versus tuned UCI vectors.
- Added `spsa --stop-after-iteration N` to stop a tune cleanly at a planned
  iteration without changing the horizon it resumes towards.
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
- Added per-move search annotations to every game record: score, depth,
  charged time and nodes after each engine move, `{book}` on pre-played ones,
  and a field the engine did not report left out rather than written as zero.
- Added PGN search telemetry that reads those annotations back, with per-engine
  score, mean absolute score, depth, time and node coverage, opening exclusion
  and compatibility-labelled implied NPS.
- Added resumable fixed-time/node/depth EPD/FEN suites with legal `bm`/`am`,
  deterministic malformed/unscored outcomes and compatible baseline compare.
- Added one deterministic tournament planner for round-robin and multi-seed
  gauntlet schedules, with `gauntlet` as an alias of the same implementation.
- Added durable round-robin and gauntlet execution with bounded concurrency,
  optional openings/affinity, joint or anchored ML ratings with error bars,
  standings/crosstable CSV, direct engine controls and per-game resume.
- Added `tournament run --fixed <index>:<rating>` to pin an established field
  at ratings you supply and estimate only the remaining participants against
  it. A pinned rating reports no error bar, because it is an input rather than
  something the tournament measured.
- Added explicit, recorded UCI pondering for clock-based match, SPRT,
  calibration, SPSA and tournament workflows.
- Standard chess is the supported ruleset. A Chess960 request is refused rather
  than attempted: castling there is encoded as king-onto-rook, which a standard
  reader would score as a different move.
