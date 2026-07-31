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
