# CPU topology detection

Run `colosseum-cli capabilities` to inspect this machine without launching an
engine or creating a run directory. Add `--json` for one stable machine-readable
document containing the topology, current-process restrictions, core class,
NUMA and last-level cache metadata, affinity mechanism, limitations and
unavailable reasons. Because this
is a read-only probe, `--dry-run` is rejected as meaningless.

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

Colosseum also records placement-quality metadata without estimating it from
clock frequency or CPU numbering:

| Platform | Core-class source | NUMA source | Last-level cache source |
|---|---|---|---|
| Windows | CPU Set `EfficiencyClass` | CPU Set `NumaNodeIndex`, qualified by processor group | `GetLogicalProcessorInformationEx(RelationCache)`, highest unified or data level |
| Linux | `cpu_capacity` when the kernel exports it; otherwise unknown | Per-CPU `nodeN` sysfs membership | `cpu*/cache/index*/shared_cpu_list` at the highest unified or data level, typically `index3` |
| macOS | Unavailable without a logical sibling map | Unavailable without a logical sibling map | Unavailable without a logical sibling map |

An unknown class is kept as unknown, and a core the operating system reported
no cache for keeps no cache domain. This avoids silently treating unlike cores
as equivalent when the operating system supplies no trustworthy signal.

Cache domains are the sharing sets at a core's last reported cache level,
numbered in ascending order of their lowest member CPU so the identity is
stable between runs. On a multi-die part this is the chiplet boundary: two
cores in different domains do not share a last-level cache, and a game slot
split across that boundary is not measuring the same thing as one kept inside
it.

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
| `auto` | Physical cores from the allowed set, highest-performance class only where classes differ, leaving one whole physical core free by default; the headroom is configurable |
| `off` | No CPU selection or affinity request |
| explicit CPU list | Exactly the named group-qualified logical CPU identities |

The default headroom is one whole physical core with all of its SMT siblings.
That is room for the harness and the operating system; a second free core costs
a game slot for no measured benefit.

Placement knows nothing about any particular processor. It reads core class,
NUMA node and last-level cache domain from the operating system and decides
from those alone. On a host whose classes differ, `auto` selects the
highest-performance class only, because mixed classes make game slots unequal.

Where the operating system's own evidence is too thin to decide, `auto`
refuses, names the detected topology and asks for an explicit CPU list rather
than guessing:

- the host reports mixed core classes and at least one of them is unknown;
- the host reports a cache domain for some cores and none for others;
- the host reports no cache topology at all for a part it also reports as
  spanning more than one NUMA node.

A host that reports neither a cache domain nor more than one node says nothing
that makes it multi-domain, so `auto` proceeds and both facts stay visible as
unreported in the run record. An explicit CPU list is always accepted; it is
your statement about the machine rather than the tool's inference.

`auto` counts cores only after applying the allowed set and keeps every allowed
SMT sibling belonging to a selected core. Explicit lists are canonicalized and
validated against both the discovered and allowed logical identities, but may
intentionally name part of a physical core. Both modes require the exact
sibling map; on macOS they therefore report that the selection cannot yet be
resolved rather than guessing CPU identities. `off` remains available without
a sibling map or allowed-set identity.

The selected pool is divided into disjoint concurrent game slots, and each
slot's cores carry every available SMT sibling belonging to them. How a slot's
cores are divided between its two engines is the allocation mode:

| Mode | Flag | Cores a slot consumes |
|---|---|---|
| Shared (default) | `--cores-per-game N` (default 1) | `game-slots × cores-per-game` |
| Disjoint | `--cores-per-engine N` | `game-slots × 2 × cores-per-engine` |

**Sharing is the default because it is what the games actually need.** Without
pondering, only one engine of a game searches at any moment; the other is
blocked reading a pipe. Pinning the two engines to separate cores therefore
leaves half the pool idle: a 16-core host with one core of headroom runs 15
one-thread games at once when they share, and 7 when they do not.

`--cores-per-engine N` selects the disjoint allocation, and the two flags are
mutually exclusive. It is required with `--ponder`, and a shared request with
`--ponder` is refused: a pondering engine searches on its opponent's time, so
both engines of a game can run at once and cannot share a core.

Either way the allocation is independent of the engine's UCI worker-thread
option: changing `Threads` never changes `cores-per-game` or
`cores-per-engine`, or vice versa. A request that does not fit the pool is
rejected, and the refusal names the arithmetic it applied. The mode and every
engine allocation are in the run record and in `--dry-run` output.

Allocation first looks for enough cores of one class, NUMA node and cache
domain for the whole slot, so a slot stays inside one domain and one node
whenever the pool allows it. A shared slot takes its cores from a single group
for the same reason. If that is impossible, the disjoint mode keeps each engine
within one group and prefers matching classes; only then does it fall back to
the remaining cores. Every engine allocation records its class, node and cache
domain sets, together with explicit flags for class, node or cache-domain
mismatch and for an engine spanning more than one of any of them. The run
record can therefore expose unavoidable asymmetry instead of hiding it.

The OS adapter has an explicit capability contract:

| Platform | Hard-affinity mechanism | Limitation |
|---|---|---|
| Windows | `SetProcessAffinityMask`, followed by group-aware thread inspection and mask read-back verification | One engine allocation must fit one current child-process primary group; mismatched group requests fail rather than applying group-relative CPU numbers to the wrong group |
| Linux | `sched_setaffinity` for every current process thread, followed by per-thread read-back verification | Newly observed threads are rescanned until the process thread set is stable |
| macOS | Unavailable | Public affinity tags are scheduler hints, not verifiable logical-CPU pinning |

Any requested hard placement that cannot be applied or verified is a run
error. It is never silently changed to advisory placement or normal scheduling.
The explicit `off` mode is different: it is a successful, recorded no-op and
leaves scheduling to the operating system. Consequently, a clock match remains
valid on macOS when placement is `off`; only a hard-placement request is
rejected there.
