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
position is a weak basis for performance decisions. Multi-position A/B design,
repetitions, warm-up, estimators, and state policy are not yet part of this
single-sample command.

The normal direct engine controls apply: `--engine-arg`, `--cwd`, `--env`,
`--option`, and `--button`. `--deadline-ms` bounds the search (default 60000).
Use `--json` for one machine-readable document or `--dry-run` to resolve the
workload and invocation without launching the engine.
