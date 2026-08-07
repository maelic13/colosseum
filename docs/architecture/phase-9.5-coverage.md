# Phase 9.5 validation-engine coverage acceptance

Date: 2026-08-07

This acceptance audits the current Rarog and Basilisk development workflows
against PLAN §S5.14. Generic harness implementations were removed from both
working trees, not copied into an archive directory: their Git histories are
the archive, while current users see only supported commands.

| Engine | Accepted commit | Declarative policy retained |
|---|---|---|
| Rarog | `8f35647630eb57839f0435e94b3c58858bd91711` | Seven run profiles and twelve SPSA parameter vectors under `tools/colosseum/` |
| Basilisk | `3cbf90ba5f7b6c7aab2c25e8066ba1417b273447` | Nine run profiles and five active SPSA parameter vectors under `tools/colosseum/` |

Neither engine acquired a Colosseum manifest. Executable paths, books,
concurrency and run directories remain ordinary CLI arguments. Each engine's
guide states that Colosseum is installed independently and records the exact
mechanism/policy boundary.

## Coverage result

| Former engine-repository mechanism | Current owner | Acceptance evidence |
|---|---|---|
| Fixed matches, SPRT and fixed-N null calibration | `match`, `sprt`, `calibrate` | Gainer, simplify, 1T calibration and Basilisk 4T calibration profiles parse through the shipped command model |
| SPSA driver, schedule derivation, audit, progress, persistence and final-vector selection | `spsa`, `spsa plan`, `spsa status`, `sprt --apply` | All 17 active vectors converted to the ordered TOML schema and validated by `spsa plan` |
| Round-robin and gauntlet scheduling/rating | `tournament run` | Both repositories retain only declarative tournament conditions and participant paths stay arguments |
| NPS A/B, repeated builds and thread scaling | `nps` | Engine guides use `--against`, repeated build arguments and `--scale-threads`; no NPS estimator remains in either repository |
| UCI option probing and compliance | `engine inspect`, `engine check` | Both current engine executables passed the live Colosseum UCI check |
| Result recomputation and PGN diagnostics | `stats`, `stats telemetry` | Engine-local PGN result/depth scripts were removed; historical result citations remain evidence only |
| CPU topology, affinity, concurrency execution, master seeds and executable hashes | Colosseum match/tune drivers | Shared topology/affinity helpers were removed; profiles request `placement = "auto"` and leave host concurrency explicit |
| Console filtering, tee, liveness and resumable state | Durable run logging, checkpoints and status commands | Watch/filter scripts were removed; run directories are the supported observation and recovery surface |
| Opening parsing, ordering, slicing and non-reuse | Match/tune book controls and `book` | No book is bundled or hard-coded; engine guides pass optional project-selected books explicitly |
| Generic EPD suites | `suite` | No duplicate suite runner was retained |
| Self-play PGN generation | `match` recipe | Each Texel guide now consumes Colosseum `games.pgn`; label extraction remains engine-owned |
| FastChess/weather-factory download, patch and vendor logic | No owner; obsolete | Setup scripts, Basilisk's tracked tuner overlay and empty runner-bin placeholders were removed |

The converted SPSA vectors preserve the old 5,000-iteration terminal
perturbation as `legacy_step * 5000^-0.102`. Values below `0.5` are raised to
`0.5`, because sending the same rounded integer to both arms measures no knob;
this corrects two active Basilisk step-1 parameters instead of preserving an
invalid schedule.

## Intentional engine-owned residuals

| Residual | Classification |
|---|---|
| Rarog `xtask`/`build_test.ps1`, Basilisk CMake/PGO build script and local build manifests | Engine build, ISA/PGO, compiler/flag comparability and artifact policy |
| Rarog perft/search-quality/SMP diagnostics and profiling scripts | Engine-internal correctness and diagnostic counters; not generic UCI measurements |
| Each engine's move-generation, search, tactical, sanitizer and regression tests | Engine correctness |
| Texel sampling, extraction, filtering, fitting and value-baking tools | Non-UCI training-data and source-integration policy |
| `tools/colosseum/*.toml` plus concise invocation documentation | Declarative engine-project experiment policy explicitly allowed by S5.14 |
| Historical PLAN/EXPERIMENTS citations to retired scripts | Immutable provenance for old results, not executable current guidance |

## Gap found and closed

The audit found one generic mechanism gap. Colosseum's PLAN promised that its
two-sided resignation default was externally overridable, but the CLI could
only change its threshold or disable it. Rarog had real one-sided historical
conditions, so silently converting them would have changed the experiment.

Commit `89d24a0` adds `--one-sided-resign-adjudication` to fixed matches, SPRT,
calibration, tournaments and SPSA, records the choice in resolved
configuration, preserves two-sided deserialization as the compatibility
default, and tests both decision rules. Rarog profiles use the compatibility
mode explicitly; Basilisk retains the safer two-sided default. No generic CLI
mechanism gap remains after this correction.

## Verification

- Colosseum: focused core and CLI adjudication tests passed; the generated
  command reference was refreshed. `cargo check --workspace --tests`,
  `cargo clippy --workspace`, `cargo test --workspace --all-targets` and the
  generated-documentation drift check all passed.
- Rarog: `cargo fmt --check`, `cargo test`, live `engine check`, six profile
  dry runs and `spsa plan` for all twelve tune vectors passed.
- Basilisk: live `engine check`, seven profile dry runs, both calibration
  variants, and `spsa plan` for all five tune vectors passed. Thirteen retained
  Texel Python tests passed. CMake/CTest was unavailable in the current shell;
  no compiled C++ behaviour changed (only ownership comments), and the tested
  existing PGO executable supplied the live UCI acceptance.
- Both engine repositories were clean after their migration commits. No long
  SPRT, calibration, SPSA or tournament run was started in this step.

The §S5.14 success criterion is met: current engine repositories contain no
duplicate generic harness implementation, every demonstrated workflow has a
Colosseum command or recipe, and every residual is classified above.
