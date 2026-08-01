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

The placement-policy resolver has three modes:

| Mode | Selection |
|---|---|
| `auto` | Complete physical cores, leaving two physical cores free by default; the headroom is configurable |
| `off` | No CPU selection or affinity request |
| explicit CPU list | Exactly the named group-qualified logical CPU identities |

`auto` never divides an SMT sibling set. Explicit lists are canonicalized and
validated against the discovered logical identities, but may intentionally name
part of a physical core. Both modes require the exact sibling map; on macOS
they therefore report that the selection cannot yet be resolved rather than
guessing CPU identities. `off` remains available without a sibling map.

This policy is only a deterministic selection. It does not yet account for the
CPUs available to the current process and does not apply affinity; those are
separate platform responsibilities.
