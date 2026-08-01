# Run records and status

Every long-running workflow creates `run-record.json` immediately after its run
directory is initialized. It is the official status source and contains:

- run-record schema, statistics-contract and CLI product versions;
- command, exact configuration hash and lifecycle state;
- command-specific resolved evidence (for SPRT this includes model,
  hypotheses/error rates, cap, clocks, adjudication, resources, seed and
  opening policy);
- the official durably committed sample, including paired pentanomial bins and
  any unpaired games;
- an OS/architecture/visible-CPU and capability summary;
- structured anomalies, including invalidation or unexpected owner loss.

The workflow owns a recorder guard. Completing, cancelling or invalidating the
run writes the explicit terminal state. If ownership ends without one, the
guard records `aborted` and an anomaly, preserving even zero-sample attempts.

`colosseum-cli status <run-dir>` is common to all workflow types. It only reads
`run-record.json`; it never resumes, repairs, checkpoints or changes the run.
Use `--json` for the common single-document machine output.

Run-record schema history:

| Version | Change |
|---:|---|
| 1 | Common identity, lifecycle, host, sample and anomalies |
| 2 | Added required command-specific `workflow` evidence so the statistical model and experimental conditions are stored in the record itself |
