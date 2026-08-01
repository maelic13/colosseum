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

Topology discovery alone does not promise that every discovered CPU is
available to the current process, nor does it apply affinity. Colosseum reports
allowed-CPU restrictions, placement modes and enforcement capabilities through
the corresponding placement functionality.
