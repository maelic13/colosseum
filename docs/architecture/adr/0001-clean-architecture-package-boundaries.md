# ADR-0001: Introduce an application package and inward dependencies

- **Status:** Accepted
- **Date:** 2026-07-31
- **Relates to:** PLAN §S4; findings CS-01 and CS-03

## Context

The current Cargo graph points away from `colosseum-gui`, but package contents
do not form Clean Architecture layers. `colosseum-core` contains GUI/product
policy, and `colosseum-engine` combines use cases with Tokio, UCI processes,
SQLite, filesystem output and GUI-shaped state. A CLI built directly on those
surfaces would inherit GUI storage concepts and concrete infrastructure.

Both CLI and GUI must invoke the same experiment policy. Neither may own or
depend on the other, and working UCI/game functionality should not be rewritten
merely to improve package names.

## Decision

Add one workspace library, `colosseum-application`, as the owner of
presentation-independent use cases, input/output models, run invariants and
driven-port traits.

These package names are current/working identifiers. Phase 0.8 may rename them
as part of the shared product migration without changing the accepted package
roles or dependency edges.

The target package ownership is:

| Package | Ownership |
|---|---|
| `colosseum-core` | Side-effect-free domain values and deterministic chess/statistical rules |
| `colosseum-application` | Use cases, application models, ports and failure/commit policy |
| `colosseum-uci` | UCI parsing/process adapter implementing application engine-session ports |
| `colosseum-engine` | Headless runner, execution, persistence, artifact, input, topology and affinity adapters |
| CLI product package (`<name>-cli`) | Command adapter and CLI composition root |
| GUI product package (currently `colosseum-gui`) | Desktop adapter, GUI-owned persistence models and GUI composition root |

Allowed workspace dependency edges are:

```text
colosseum-core
    ↑
colosseum-application
    ↑              ↑
colosseum-uci   colosseum-engine  (engine may also depend on uci)
       ↑          ↑
       └── CLI and GUI composition roots ──┘
```

More precisely:

- `colosseum-application` may depend on `colosseum-core`, never on an adapter;
- `colosseum-uci` may depend on application/core;
- `colosseum-engine` may depend on application/core/UCI;
- CLI and GUI may depend on the inner packages and required adapters;
- CLI and GUI must not depend on each other;
- no inner package re-exports an outer framework type as part of its contract.

Pure UCI option schema/value and recognised-option policy move from core to the
application boundary because use cases must validate them and both UCI and
driving adapters consume them. GUI branding/events/library policy leave core.
CSV/PGN formatting and filesystem-oriented configuration remain outer adapter
concerns.

The application API may be asynchronous, but it must not expose Tokio or any
other runtime type. ADR-0003 fixes the object-safe port representation and
injection rules.

Each executable is a composition root. It owns its runtime, concrete adapters,
shutdown and top-level presentation. Reusable bootstrap helpers may construct
individual adapters, but no shared package becomes a hidden composition root or
reads GUI/CLI global configuration.

## Enforcement

Phase 2 adds dependency checks that reject:

- I/O, OS, entropy, clock acquisition or outer workspace dependencies in core;
- UCI/engine, Tokio, crossbeam, rusqlite, egui/eframe or OS-driver dependencies
  in application, including required test paths;
- GUI/windowing or GUI-configuration dependencies in CLI;
- a source dependency from either composition root to the other.

Application use-case tests compose fake ports without Tokio, SQLite, a GUI or a
real engine executable.

## Consequences

- Shared policies have one owner and can be tested deterministically.
- CLI and GUI remain independently composable even while sharing use cases.
- `colosseum-engine` remains a useful adapter package rather than being
  misleadingly promoted to the application layer.
- Moving existing public types causes temporary churn. ADR-0005 permits narrow
  compatibility shims only during Phase 2.1.
- The new package adds one Cargo boundary, but avoids a proliferation of one
  crate per adapter.

## Alternatives considered

### Keep use cases in `colosseum-engine`

Rejected. It would either keep concrete Tokio/SQLite/UCI dependencies in use
cases or require an internal layering convention that Cargo cannot enforce.

### Put shared workflows in the CLI product package

Rejected. The GUI would depend on the CLI or duplicate experiment policy, so
the command adapter would incorrectly own application behavior.

### Feature-gate GUI concepts out of the current packages

Rejected. Feature combinations obscure the dependency direction and do not
separate persisted GUI models from runtime input.

### Split every port/adapter into a separate crate

Rejected for now. It adds build and coordination cost without an identified
dependency cycle. A later ADR may split an adapter if concrete independent
reuse or platform packaging requires it.
