# Run directories and recovery

Every long-running CLI workflow uses a self-contained run directory. Without an
explicit path, the CLI allocates a collision-safe directory beneath
`./colosseum-runs`. A supplied `--dir` is stable: an existing directory means
resume, and its stored configuration hash must match exactly.

Selecting restart never deletes or overwrites an earlier attempt. The complete
old directory is renamed to a unique adjacent `.archive-…` path before a fresh
directory is initialized. Logs are opened append-only and synced; resume does
not truncate prior diagnostics.

The run stores canonical `resolved-config.json`, `config.sha256` and
`config-origins.json`. Checkpoints are atomic JSON envelopes containing their
schema version, payload and SHA-256 payload checksum. `checkpoint.json` is the
current generation and `checkpoint.previous.json` is the last generation. If
the current file is missing, torn, invalid or fails its checksum, recovery uses
the previous generation. If neither validates, resume fails rather than
inventing state.

For fixed matches, `run.log` is an append-only JSON-lines event stream,
`games.pgn` is rebuilt from the authoritative checkpoint, `result.json` is the
final structured report, and `failed-games/` retains UCI stdout/stderr traffic
for abnormal games. A resumed match schedules only game numbers absent from the
verified checkpoint and retains deterministic report order.

A game the harness abandoned on an infrastructure fault — an engine that never
spawned, a processor-affinity call the operating system refused — is still
written to `games.pgn` and the checkpoint, because the abandoned game is the
evidence for the abort. It is marked `ColosseumSample "unscorable"` and no
statistic counts it; see [statistics replay](stats.md).

SPRT checkpoints store complete official and post-terminal pairs in separate
arrays. Only the official prefix enters statistics. Its PGN labels both sample
classes, while the run record and final result retain the explicit model,
hypotheses, error rates, cap, terminal pair and fault policy.

Completed SPSA runs additionally write the verified schedule and three views of
the frozen final-window vector: `tuned-options.txt`, `tuned-options.json` and
`tuned-options.toml`. The main `result.json` retains the source launch, tune
conditions, schedule/version metadata, original values, floating means and
rounded tuned values so it can feed `sprt --apply` without editing.

## Stopping a run

Interrupting a long run is a supported operation. Press Ctrl-C once and the run
stops launching new work, gives the games already in flight up to
`--stop-grace-secs` seconds (30 by default) to finish on their own, writes its
checkpoint, records itself `cancelled` and exits with code `6`. Press Ctrl-C
again and the games in flight are abandoned immediately instead of waiting.

Either way the run is recoverable, and every command stops the same way:
`match`, `sprt`, `calibrate`, `spsa`, `tournament run` and `suite` share one
cancellation path rather than each inventing its own. Work already committed to
the checkpoint is kept; work that had not been committed is simply replayed
when you resume the same `--dir`. For SPSA that means the mini-match in flight
is replayed as a whole, which is what keeps a gain schedule from advancing on a
partial iteration.

The grace period is one bounded period, measured from the interrupt rather
than from each game that finishes inside it, so a busy run with several slots
still stops when the period is up.

A cancelled run is not a failure and not a statistical conclusion. An SPRT that
stopped before reaching a boundary or its cap reports `cancelled` rather than
`inconclusive`, because it never sampled far enough to say anything; a
calibration that stopped short of its fixed sample reports that instead of
classifying a partial interval. `spsa --stop-after-iteration N` asks for the
same clean stop at a planned point rather than at an interrupt.
