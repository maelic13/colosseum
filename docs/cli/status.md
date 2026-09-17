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
- the most recent progress block the run published;
- an OS/architecture/visible-CPU and capability summary;
- structured anomalies, including invalidation or unexpected owner loss.

The workflow owns a recorder guard. Completing, cancelling or invalidating the
run writes the explicit terminal state. If ownership ends without one, the
guard records `aborted` and an anomaly, preserving even zero-sample attempts.

`cancelled` means the run stopped cleanly on request at a committed boundary
rather than reaching its terminal state. Its checkpoint is written, its exit
code is `6`, and the same run directory resumes towards the stored horizon. It
is neither a failure nor a statistical conclusion.

`colosseum-cli status <run-dir>` is common to all workflow types. It only reads
`run-record.json`; it never resumes, repairs, checkpoints or changes the run.
Use `--json` for the common single-document machine output.

It ends by printing the run's most recent progress block verbatim, exactly as
the console showed it, so closing a terminal costs nothing. `run.log` keeps
every block the run ever printed; the record keeps the latest. A run that has
not published one yet says so. See the
[output contract](output.md) for what a block contains and when it is printed.

SPSA tunes additionally support `colosseum-cli spsa status <run-dir>`. That
command reads the checksum-verified checkpoint generation and extends the
common lifecycle view with trajectory, thirds, ETA and explicitly heuristic
per-knob observations. It ends with the last committed iteration's own
trajectory — its mini-match score, the gain and perturbation scale it used and
what moved in it — which the progress blocks record but deliberately do not
print. It likewise never repairs or mutates the run.

Run-record schema history:

| Version | Change |
|---:|---|
| 1 | Common identity, lifecycle, host, sample and anomalies |
| 2 | Added required command-specific `workflow` evidence so the statistical model and experimental conditions are stored in the record itself |
| 3 | Added the last-level cache domain to every engine CPU placement, the versioned game-record annotation writer, and the SPSA estimator that produced a tuned vector |
| 4 | Replaced the execution plan's `cores_per_engine` count with the slot allocation mode, so a record says whether the two engines of a game shared their cores or had their own |
| 5 | Added the run's most recent progress block, so `status` reports what the console last showed |
