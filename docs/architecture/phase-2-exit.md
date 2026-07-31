# Phase 2 exit review

This review records the implemented architecture/durability foundation and the
evidence used to accept it. It does not claim that later chess-experiment
workflows already exist.

| Exit condition | Evidence | Result |
|---|---|---|
| Ordinary engines need only executable paths | Two independently copied repository UCI fixtures pass `engine check` with no descriptor, options or arguments; locally discovered Rarog and Basilisk executables also passed the same path-only command | Pass |
| Run-file, flattening, unset and path origins are deterministic | `config_resolution` covers inherited/flattened and file/all-CLI canonical byte/hash equality, recursive table merge, replacement, RFC 6901 unset, depth/cycle and Windows aliases | Pass |
| Randomness is stable and independent by consumer | Domain golden vectors cover every built-in stream and sampling primitive; adding a named consumer cannot move any old stream | Pass |
| Dry-run/JSON streams are automation-safe | Command-line tests prove no launch during dry-run, one typed JSON value on successful stdout and empty stdout on failure | Pass |
| Process I/O and ownership are bounded | The exact executable self-test floods both pipes, rejects a line above 64 KiB and reaps an ignored-quit engine plus descendant through Job Object/process-group containment | Pass |
| Durable state recovers without inventing results | Run-directory tests cover unique create, exact-hash resume refusal, archive restart, append-only logs, atomic checksummed two-generation checkpoints and fallback | Pass |
| Every started workflow has readable official state | Recorder tests cover running/terminal samples and ownership-drop abort; status is proven byte-for-byte read-only | Pass |
| CLI is independent from GUI state and presentation | Cargo/source tests reject GUI/windowing dependencies and GUI app-data access; an isolated sentinel integration test proves the CLI does not touch GUI files | Pass |
| Inner layers point inward | Core/application manifest tests exclude runtime, database, GUI, entropy and adapter dependencies; fake-port tests pin orchestration and commit-before-progress | Pass |
| Published-style binary is self-contained | A copied `colosseum-cli` executable passes help/version, exact-executable self-test and JSON workflows from an isolated directory | Pass |
| Existing desktop behavior remains compatible | The full workspace all-target test suite, including GUI configuration/preset/runtime-mapping tests, is green | Pass |

The CLI’s unused dependency on the legacy engine/SQLite package was removed at
exit. Later match execution will introduce only the adapter surface it needs;
it must not import GUI persistence or saved engine-library policy.

CPU topology/affinity, full game adjudication, match/SPRT orchestration,
calibration, SPSA and data generation remain explicitly owned by later phases.
The four-ply self-test verifies executable/process/protocol integration; it is
not presented as a substitute for the later chess-game runner acceptance.
