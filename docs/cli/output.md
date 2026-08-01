# CLI output contract

`colosseum-cli --json …` writes exactly one JSON value to standard output when
a command produces a final report. Progress, warnings and diagnostics are
written to standard error. A configuration or runtime failure before a final
report leaves standard output empty. A statistically invalid completed run is
still a report and therefore emits JSON with a nonzero exit status. The
top-level `type` field identifies the document schema.

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
- `fixed-match`

Human-readable mode is the default. Automation should select `--json` and use
the process exit status as the primary success/failure signal.

For `match`, exit `0` means completed/valid, `1` means completed/invalid under
the fault thresholds, `2` means command-line or configuration refusal, and `3`
means infrastructure, persistence or runtime error. Sequential commands add
their documented H0/H1/inconclusive distinctions without reusing the error
codes.
