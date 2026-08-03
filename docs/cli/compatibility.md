# Engine compatibility and failure behavior

Colosseum CLI accepts an executable path plus optional process arguments,
working directory, environment and UCI options. The engine is always a separate
process. Colosseum does not require a manifest, compiler identity, benchmark
command, source checkout, embedded library or engine-specific adapter.

## Required UCI behavior

The exact commands exercised depend on the workflow. A conforming engine used
for games must support:

- `uci` and a terminating `uciok`, followed by `isready` / `readyok`;
- advertised `option` declarations for every requested UCI value or button;
- `ucinewgame` and readiness before a new game;
- `position startpos` or `position fen …`, plus the accumulated legal moves;
- the selected `go` limit: clock, movetime, nodes or depth;
- a syntactically valid, legal `bestmove` within the applicable deadline;
- prompt `stop` handling when the harness ends a search;
- bounded shutdown after `quit` (Colosseum escalates containment if needed).

Pondering is off by default. With `--ponder`, the engine must advertise and
accept the standard `Ponder` option and correctly handle `go ponder`,
`ponderhit` and `stop`. Ponder is available only for base/increment clocks.

The `engine check` command reports handshake, readiness, requested-option
schema validation, option command plus readiness, bounded legal search, stop,
new-game readiness and shutdown separately. UCI provides no option read-back;
a successful check proves protocol acceptance, not that an engine internally
used the value as intended.

## Limits and unsupported variants

- Protocol lines are limited to 64 KiB excluding newline. Oversized lines are
  protocol faults rather than unbounded memory input.
- Standard chess is the supported ruleset. Chess960 is not silently attempted.
- Colosseum does not probe Syzygy itself. An engine may use its ordinary
  advertised tablebase options; those options remain engine-owned.
- The CLI ships no opening book or engine. Books are optional EPD/PGN inputs.
- Process affinity is enforced only where the reported host capability allows
  it; an explicit hard-placement request fails rather than pretending success.

## Failures are evidence

Engine exits, disconnects, time losses, illegal moves, protocol faults and
shutdown failures are classified and retained. Match/tournament policy may
forfeit an engine-attributable game up to its explicit threshold; sequential
and SPSA workflows preserve pair/iteration atomicity and do not turn incomplete
evidence into a statistical sample. Infrastructure or persistence failures are
never scored as chess results.

Machine mode writes one JSON document only when a final report exists;
diagnostics and progress stay on standard error. See the individual workflow
page and [output contract](output.md) for terminal states and exit codes.

Engine executables may have licences and redistribution conditions independent
of Colosseum. Consult the terms applicable to every engine and data file you
choose to run or distribute; separate-process execution is not a blanket legal
conclusion.
