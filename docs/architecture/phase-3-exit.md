# Phase 3 exit — CPU topology and affinity

Phase 3 is accepted when topology selection is deterministic over the required
machine shapes, requested hard affinity is either verified or rejected, and the
independent CLI accurately reports platform capabilities.

| Criterion | Evidence | Result |
|---|---|---|
| Exact OS topology identities; no numbering inference | Windows group masks, Linux sibling lists and macOS count-only behavior have parser/unit coverage | Pass |
| Current-process restrictions constrain planning | Recorded restricted-cpuset selection plus host allowed-set validation | Pass |
| SMT, hybrid, processor-group, no-SMT and dual-socket allocation | [`topologies.json`](../fixtures/phase3/topologies.json) is replayed with exact expected A/B CPU lists and asymmetry records | Pass |
| Physical cores allocated separately per engine | Slot fixtures and unit tests enforce `slots × 2 × cores-per-engine`, complete available siblings and disjointness | Pass |
| Class/NUMA symmetry is auditable | Hybrid and dual-socket fixtures prove same-class preference, per-engine locality and explicit unavoidable node mismatch | Pass |
| Requested affinity is fail-closed | Windows process masks and Linux per-thread masks are read back; invalid, mismatched and unavailable requests are typed errors | Pass |
| Actual residency where enforceable | Two busy fixture children repeatedly sample only the logical CPU assigned to each on Windows/Linux; macOS returns a documented unavailable capability and explicit test skip | Pass on enforceable platforms |
| Capability reporting | `colosseum-cli capabilities` text/JSON tests cover topology, restrictions, metadata, mechanisms, constraints and reasons | Pass |
| Independent CLI remains lightweight | Feature-minimal adapter dependency graph excludes GUI/windowing, SQLite, tournament and chess-position packages | Pass |
| Existing product behavior remains intact | Full workspace check, clippy and all-target test baseline | Pass |

Windows group-relative masks are applied only after inspecting the target
process's current thread primary groups. An allocation that spans groups or
names a different group fails; it is never relabelled. This is an explicit
current limitation rather than a correctness compromise. Linux applies the
mask to every current thread, rescans until the thread set is stable, then
verifies every mask. Engine worker threads created later inherit from their
already-restricted creator; Phase 4 composes this adapter before UCI setup can
create the configured worker pool.

macOS provides scheduler affinity tags but no supported verifiable logical-CPU
pinning contract for an external process. Colosseum therefore reports hard
affinity unavailable. Explicit placement `off` remains a valid recorded choice
and does not prohibit clock matches.

The machine-readable gate ownership is in
[`acceptance.json`](../fixtures/phase3/acceptance.json). Required CI executes the
same test targets on Windows, Linux and macOS; local acceptance evidence for
this change was produced on Windows.
