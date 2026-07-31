# CLI output contract

`colosseum-cli --json …` writes exactly one JSON value to standard output on
success. Progress, warnings and failure diagnostics are written to standard
error; a failed command leaves standard output empty. The top-level `type`
field identifies the document schema.

`--dry-run` is a global option. It resolves configuration paths and prints the
configuration hash, the complete resolved configuration and every exact process
invocation without starting an engine or playing a game. An invocation is
represented by separate executable, argument-vector, working-directory,
environment, UCI-option and CPU-allocation fields. It is deliberately not a
shell command string: shell quoting is neither exact nor portable.

The currently emitted `type` values are:

- `dry-run`
- `engine-inspection`
- `engine-compliance`

Human-readable mode is the default. Automation should select `--json` and use
the process exit status as the primary success/failure signal.
