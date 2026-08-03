# Fixed-node NPS measurement

`colosseum-cli nps` measures one bounded UCI search without trusting the
engine's own clock or NPS calculation:

```powershell
colosseum-cli nps D:\chess\engines\engine.exe --nodes 10000000
```

The charged interval starts after the complete `go nodes` command has been
written and ends when the complete `bestmove` line has been read. Colosseum
uses a monotonic clock and computes authoritative NPS as requested nodes divided
by that wall time. The last `info nodes` value must be present and at least the
requested limit; otherwise the sample is refused as unverifiable or incomplete.
Node overshoot is recorded but does not let an engine change the fixed workload.

The engine's `info time` and `info nps` values are displayed as diagnostics
only. They cannot affect `authoritative_nps`.

Use `--position "<FEN>"` and repeat `--move <uci-move>` when appropriate. An
omitted position searches `startpos`; the command warns because one initial
position is a weak basis for performance decisions.

Add `--against <EXE>` for a comparison. Repeat `--position` for the position
suite and use `--a-build` / `--b-build` for additional executables in either
arm. Each build gets its own median so pooling cannot conceal non-overlap.
Colosseum shuffles the position and build-pair order from `--seed`, then runs
each pair in strict A/B or B/A alternation. The resolved design and complete
schedule are included in JSON output.

`--state warm` (the default) keeps every engine process alive, sending
`ucinewgame` and `isready` before each search. `--state cold` restarts the
engine for every search; handshake and startup occur before the charged search
interval. `--warmup` repetitions are run but excluded from summaries. Hash is
not assumed to be clear in either mode; an explicitly configured button is sent
only at process preparation.

Arm output includes the median, the best per-build median, a seeded 95%
bootstrap interval for the median, and the standard deviation of per-round B/A
ratios as a machine-noise diagnostic. `--self-pair` compares the primary binary
with itself. It is recommended, not required; a matching self pair outside
`--self-tolerance-percent` (default ±0.5%) emits a warning.

The normal direct engine controls apply: `--engine-arg`, `--cwd`, `--env`,
`--option`, and `--button`. `--deadline-ms` bounds the search (default 60000).
Use `--json` for one machine-readable document or `--dry-run` to resolve the
workload and invocation without launching the engine.
