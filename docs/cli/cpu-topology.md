# CPU topology detection

Colosseum obtains physical-core and simultaneous-multithreading relationships
from operating-system topology interfaces. Logical CPU numbers are identifiers;
adjacent or otherwise patterned numbers are never assumed to share a core.

| Platform | Source | Sibling-map result |
|---|---|---|
| Windows | `GetLogicalProcessorInformationEx(RelationProcessorCore)` | Exact group-qualified logical CPU masks for every physical core |
| Linux | `/sys/devices/system/cpu/cpu*/topology/thread_siblings_list` | Exact kernel-reported logical CPU set for every physical core |
| macOS | `sysctl` physical/logical CPU counts | Counts only; sibling IDs are explicitly unavailable because the public interface does not expose that map |

Windows logical CPU identity includes both processor-group number and CPU
number, so equal CPU numbers in different groups remain distinct. Linux CPU
lists are parsed as reported by the kernel and checked for overlaps or
inconsistent sibling reports.

Colosseum separately detects the set available to the current process:

| Platform | Availability source |
|---|---|
| Windows | Process group membership and affinity masks, process-default CPU Sets, and CPU Sets reserved for this process rather than another process |
| Linux | `sched_getaffinity` for the calling thread, which already reflects scheduler affinity and cpuset/cgroup restrictions |
| macOS | Unavailable as logical identities because the public topology source provides counts only |

The detected set is validated against the topology snapshot. An empty set or
an operating-system CPU identity missing from that snapshot is an error, which
also makes hot-plug races visible instead of silently changing a run.

The placement-policy resolver has three modes:

| Mode | Selection |
|---|---|
| `auto` | Physical cores from the allowed set, leaving two physical cores free by default; the headroom is configurable |
| `off` | No CPU selection or affinity request |
| explicit CPU list | Exactly the named group-qualified logical CPU identities |

`auto` counts cores only after applying the allowed set and keeps every allowed
SMT sibling belonging to a selected core. Explicit lists are canonicalized and
validated against both the discovered and allowed logical identities, but may
intentionally name part of a physical core. Both modes require the exact
sibling map; on macOS they therefore report that the selection cannot yet be
resolved rather than guessing CPU identities. `off` remains available without
a sibling map or allowed-set identity.

This policy is only a deterministic selection. It does not yet divide CPUs
between concurrent game slots or apply affinity; those are separate steps.
