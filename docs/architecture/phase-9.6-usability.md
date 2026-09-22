# Phase 9.6 release-candidate usability exercise

Date: 2026-08-07

This short, clean-room exercise used public UCI executables that the
Colosseum maintainers did not write.  It followed only the versioned
`docs/cli/` guides, beginning with `engine check`; no engine manifest, build
metadata, custom benchmark command or bundled opening book was used.

It is a workflow/usability smoke, not a strength measurement.  The tiny
limits intentionally force draws and an inconclusive SPRT result.

## Package and engines

The Windows x86-64 package was assembled from the normal release allowlist
from the source below, then passed `tools/release/Smoke-CliArchive.ps1` from an
isolated unpack directory.

| Field | Value |
|---|---|
| Product / version | `colosseum-cli` / `0.1.0` |
| Source | The Phase 9.5 acceptance commit (validation-engine coverage) |
| Local archive SHA-256 | `95f8c9174ca5ba799804f6854e87e147030daa1eaa1b12361fd31148f890cc9d` |
| Engine A | Stockfish development build (`stockfish.exe`) |
| Engine A SHA-256 | `c5d7dbd8842607df6508622c280b541512e637e5e2a54ca4321bf09552240422` |
| Engine B | Stockfish 18 Windows x86-64 BMI2 (`stockfish-windows-x86-64-bmi2.exe`) |
| Engine B SHA-256 | `bf2d8bf60ac6f3ba58df08b1b0c5f4dec759b994d8f4532b07fe62986dc03288` |

Both binaries passed the full published `engine check` protocol sequence.
They are separate public binaries, and are intentionally supplied only as
ordinary executable paths.

## Exercise

With `$cli` set to the unpacked `colosseum-cli.exe`, `$a` and `$b` set to the
two executable paths above, and `$run` set to a fresh writable directory, the
following documented command shapes completed:

| Workflow | Command | Observed result |
|---|---|---|
| Fixed match | `$cli match $a $b --games 2 --a-nodes 1000 --b-nodes 1000 --max-moves 4 --dir "$run/match" --json` | Completed: two colour-reversed draws at `MaxMoves`; zero engine, time and infrastructure faults. |
| SPRT | `$cli sprt $a $b --max-pairs 1 --preset gainer --a-nodes 1000 --b-nodes 1000 --max-moves 4 --dir "$run/sprt" --json` | One complete pair, pentanomial `[0, 0, 1, 0, 0]`, zero faults, capped **inconclusive** result (documented exit code 4). |
| SPSA | `$cli spsa $a --tune docs/fixtures/phase9.6/stockfish-threads.toml --r-end 0.002 --iterations 1 --games-per-iteration 2 --nodes 1000 --max-moves 4 --dir "$run/spsa" --json` | Completed one iteration / one pair with zero faults. The ordinary UCI `Threads` spin option was discovered and bound; the final smoke vector remained `Threads=1`. |

No book was supplied.  Each workflow explicitly reported that every game
started from `startpos` and that opening diversity was absent.  That warning,
the generated master seed, output directory, stored JSON result and the
documented capped-SPRT exit status were all discoverable without source
knowledge.

The SPSA fixture is intentionally non-promotional: its lower-rail starting
value produces the documented `initial-on-lower-rail` warning and makes no
claim about a useful tuned value.

## Triage

No CLI or documentation defect was found in the accepted exercise.

An additional compatibility probe used Fruit 2.1.  It passed `engine check`,
but did not finish a short `go nodes 1000` attempt before the outside test host
cancelled its owner process.  It therefore is not an acceptance result and
does not establish a Colosseum fault classification.  The existing
compatibility guide already distinguishes protocol acceptance from an
engine's internal observance of a selected limit, so this needs no product
change.

The Phase 9.4 GitHub candidate predates the Phase 9.5 commit that exposes the
resignation policy, which changes a CLI argument and package contents.  Per
the Phase 9.4 invalidation rule, this local archive is only usability
evidence.  Before Phase 9.7/final acceptance, the maintainer had to build a
fresh four-platform CI candidate from the intended source and retain it (at
the time by a push carrying a candidate marker in its commit subject, a
trigger since replaced by a manual dispatch of the release workflow).  That
remote operation is deliberately not performed here.

## Repeated for the release, 2026-09-22

The same three documented workflows were repeated for the release acceptance
(GUIDE 10.10) against the source being released, from an archive produced by
`cargo xtask package cli` and unpacked into an isolated directory. This section
is added rather than replacing the exercise above, which records what a
specific package did on a specific date.

| Field | Value |
|---|---|
| Archive | `colosseum-cli-0.1.0-windows-x64.zip` |
| Archive SHA-256 | `fb82e404f1f71aae4288f7c74cee704d31f6e4223a7f85f4d0fefb7db0f9a1c4` |
| Source | The Phase 10.10 commit (release acceptance repeat) |
| Engine A | Stockfish 19 Windows x86-64 universal, `45bc8e4969147db9c2eb533810637994619bff0eacc81ccfd9854394901bcbd0` |
| Engine B | Stockfish 18 Windows x86-64 BMI2, `bf2d8bf60ac6f3ba58df08b1b0c5f4dec759b994d8f4532b07fe62986dc03288` |

Engine A is a newer public Stockfish than the development build used in
August; engine B is the same binary. Both passed the full `engine check`
protocol sequence, ending `clean-shutdown`.

| Workflow | Observed result |
|---|---|
| Fixed match | Completed, exit 0: two colour-reversed draws at `MaxMoves`, zero engine, time and infrastructure faults. |
| SPRT | One complete pair, pentanomial `[0, 0, 1, 0, 0]`, zero faults, capped **inconclusive** with exit 4. |
| SPSA | Completed one iteration of one pair, exit 0, zero faults. `Threads` was discovered and bound, ended at `1`, and the run reported it sitting on a rail. |

Both game-playing workflows again reported `no opening book: every game starts
from startpos; opening diversity is absent`. No CLI or documentation defect was
found.
