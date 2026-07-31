# ADR-0003: Inject runtime-neutral ports and commit before publication

- **Status:** Accepted
- **Date:** 2026-07-31
- **Relates to:** PLAN §S4 and §5.11; findings CS-01 and CS-04 through CS-10

## Context

Current workflows accept or construct `EngineProcess`, `SpawnOptions`, `Store`,
Tokio channels/tasks, crossbeam senders, filesystem paths, clocks and random
UUIDs directly. Driver-loop writes can be ignored while in-memory standings
advance, and a panicking task can disappear without a typed result. That is not
safe for statistical commands whose official sample must be durable and
replayable.

The application needs testable seams without abstracting away the important
failure semantics of engines, processes, storage, clocks and cancellation.

## Decision

`colosseum-application` declares the driven ports. Concrete adapters implement
them outward of the application boundary.

| Port | Application-owned responsibility |
|---|---|
| `EngineSessionFactory` | Create isolated, cancellable UCI sessions from `EngineLaunchSpec` and expose handshake/search/shutdown observations |
| `GameExecutor` | Execute one immutable game with fresh sessions and return one typed report/failure; never persist or score it |
| `ExecutionPool` | Run bounded game/work units, return every stable unit identity, and turn panic/lost-task state into infrastructure failure |
| `RunRepository` | Create/resume, verify configuration, persist schedule and atomically commit complete units/checkpoints; provide atomic read snapshots |
| `ArtifactSink` | Write logical named artifacts under a root selected by composition, with explicit append/atomic and required/best-effort semantics |
| `OpeningSource` | Read, validate and hash optional opening/position input and return deterministic path-free values |
| `CpuPlacement` | Report capability, resolve/apply/verify CPU placement and preserve explicit off/advisory states |
| `Clock` | Supply monotonic instants/resolution for decisions and UTC only for metadata |
| `IdGenerator` | Supply typed identities; never generate them in domain constructors |
| `MasterSeedSource` | Supply a master seed only when absent; named deterministic derivation stays pure |
| `ProgressSink` | Receive bounded, presentation-neutral committed progress; it cannot determine official state |
| `Cancellation` | Expose cooperative stop state and wake blocked adapters without leaking a runtime token type |

Each use case receives a named dependency structure containing only the ports it
uses. There is no global registry, service locator or process-wide application
context. Shareable production adapters are held as `Arc<dyn Port + Send + Sync>`;
exclusive state such as an engine session remains an owned boxed value.

### Async representation

Port traits are object-safe and runtime-neutral. Asynchronous methods return an
application alias equivalent to:

```rust
type PortFuture<'a, T> =
    Pin<Box<dyn Future<Output = T> + Send + 'a>>;
```

Traits use object-safe methods over `&self`/`&mut self`; no Tokio handle,
Tokio channel, `async_trait` macro expansion or adapter-specific associated type
appears in the public application contract. This makes dynamic CLI/GUI/test
composition straightforward at a small allocation cost per port operation,
which is negligible beside process and game I/O. A later ADR may adopt a
language-level object-safe async representation without changing port
semantics.

Synchronous ports remain synchronous. Cancellation and progress methods must be
bounded/non-blocking by contract; an adapter that needs asynchronous draining
does so behind its boundary.

### Failures

Adapters map concrete errors once into application-owned categories with
operation, participant/unit and retry/recovery context:

- `ConfigurationFault` refuses work before it can be scored;
- `EngineFault` identifies the participant and attributable behavior;
- `InfrastructureFault` covers harness/OS, repository/artifact and lost-task
  failures and is never scored;
- `Cancelled` is a supported terminal transition;
- domain validation errors remain typed domain/application errors.

Concrete `UciError`, `std::io::Error`, `rusqlite::Error`, Tokio join errors and
OS handles stay in adapters. Diagnostic source chains may be logged or stored
opaquely, but application policy never switches on a concrete adapter error.
Fault attribution is fixed by tested classification tables in the relevant
implementation steps.

### Authoritative commit boundary

The application owns this order:

1. receive a complete report bearing the scheduled unit identity;
2. validate completeness and fault classification;
3. atomically commit the command's unit through `RunRepository`;
4. update the official in-memory sample from the committed representation;
5. publish `CommittedRunSnapshot`/`ProgressEvent`;
6. append secondary artifacts under their declared durability rule.

The atomic unit is command-specific: game, colour pair, SPSA mini-match or suite
position. An incomplete pair/mini-match cannot enter an official paired sample.
Repository failure leaves the unit unofficial and makes the run
infrastructure-invalid. A required artifact failure stops the run and records
an infrastructure-invalid transition in the repository; the already committed
structured result remains authoritative. A best-effort forensic-report failure
is preserved as an anomaly and cannot replace or hide the primary failure.

An engine fault may be committed as an attributed forfeit/fault event and then
evaluated against the command's threshold. An infrastructure fault never
becomes a game result. A task panic or missing completion is an infrastructure
fault and prevents the run from satisfying its finished invariant.

### Cancellation

Cancellation first stops new scheduling. The application determines whether an
atomic unit must finish or remain explicitly incomplete. Adapters unblock
pending work, use bounded `stop`/`quit` escalation and reap every owned process.
The repository writes the last recoverable checkpoint and terminal
cancelled/aborted record when possible. Force-stop shortens cleanup deadlines;
it does not fabricate results or weaken persistence requirements.

## Adapter assignment

- `colosseum-uci` implements engine sessions/process lifecycle.
- `colosseum-engine` implements game execution, Tokio execution pool, SQLite
  and run-directory repositories, artifacts/openings and platform placement.
- `ucirig` and `colosseum-gui` implement presentation progress and
  cancellation sources and assemble adapters.
- application tests implement deterministic in-memory/fake ports.

An unavailable platform capability is returned explicitly. It is never hidden
behind a success no-op; the use case applies PLAN's fail/advisory/off policy.

## Verification

Contract and use-case tests must prove:

- application composition with fake ports and no runtime/framework dependency;
- out-of-order execution completion cannot change deterministic commit order;
- no official statistic or progress event precedes repository commit;
- every repository/required-artifact failure is surfaced and invalidates;
- engine and infrastructure faults cannot be interconverted accidentally;
- panic, lost completion, cancellation and forced cleanup leave no silently
  finished unit or owned process;
- ID, seed and clock fakes reproduce exact runs.

## Consequences

- Policy is deterministic and storage/runtime independent.
- Failure handling becomes more explicit and verbose at adapter boundaries.
- Object-safe boxed futures incur allocation/dynamic-dispatch overhead, but not
  on the engine's search hot path.
- SQLite GUI history and CLI run directories can implement one logical
  repository contract without sharing physical layout.
- The application cannot use a convenient concrete feature until a port and
  failure contract exist, which is intentional pressure toward correct
  responsibility placement.

## Alternatives considered

### Generic use cases over every adapter type

Rejected as the default composition mechanism. With this many independently
selectable ports it creates unwieldy types and makes runtime adapter selection
harder. Generic helpers remain allowed inside an implementation.

### Native `async fn` traits without boxing

Rejected for the initial boundary because the required trait objects are not a
stable object-safe composition surface at the workspace MSRV. Explicit boxed
futures keep the allocation and lifetime visible without a runtime dependency.

### `async_trait`

Rejected for the initial boundary. It would provide similar boxing through a
macro but hides the public transformation and adds a dependency without reducing
the architectural complexity.

### Concrete channels and stores as parameters

Rejected. Tokio/crossbeam/SQLite semantics would again become the application
contract and make CLI/GUI/test composition diverge.

### Best-effort persistence with in-memory state as authority

Rejected. It permits displayed/statistical results that resume and replay cannot
reproduce—the exact silent-wrong-result failure the durable contract prevents.
