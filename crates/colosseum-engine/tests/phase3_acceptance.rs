use std::collections::BTreeSet;
use std::process::{Child, Command, Stdio};

use colosseum_application::CpuAllocation;
use colosseum_engine::{
    AffinitySupportLevel, AllowedCpuSet, CacheDomainId, CharacteristicsSource, CoreClass,
    CpuCharacteristics, CpuPlacementPolicy, CpuTopology, LogicalCpuId, NumaNodeId, PhysicalCore,
    PhysicalCoreCharacteristics, PlacementAsymmetry, SiblingMapping, SlotAllocation,
    TopologySource, affinity_capability, allocate_game_slots, apply_process_affinity,
    detect_allowed_cpu_set, detect_cpu_topology, plan_cpu_placement, process_affinity_groups,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct RecordedFixture {
    name: String,
    source: TopologySource,
    cores: Vec<RecordedCore>,
    allowed: Vec<[u32; 2]>,
    game_slots: usize,
    /// Cores each engine gets separately; absent selects the shared mode.
    #[serde(default)]
    cores_per_engine: Option<usize>,
    /// Cores the two engines of a game share; absent means one.
    #[serde(default)]
    cores_per_game: Option<usize>,
    /// Whole physical cores `auto` leaves free.
    #[serde(default)]
    headroom_physical_cores: usize,
    /// The logical CPUs `auto` selects before any slot is carved out of them.
    expected_pool: Vec<[u32; 2]>,
    expected: Vec<ExpectedSlot>,
}

impl RecordedFixture {
    fn allocation(&self) -> SlotAllocation {
        match self.cores_per_engine {
            Some(cores_per_engine) => SlotAllocation::PerEngine { cores_per_engine },
            None => SlotAllocation::Shared {
                cores_per_game: self.cores_per_game.unwrap_or(1),
            },
        }
    }
}

#[derive(Debug, Deserialize)]
struct RecordedCore {
    cpus: Vec<[u32; 2]>,
    core_class: CoreClass,
    numa_node: Option<NumaNodeId>,
    last_level_cache: Option<CacheDomainId>,
}

#[derive(Debug, Deserialize)]
struct ExpectedSlot {
    engine_a: Vec<[u32; 2]>,
    engine_b: Vec<[u32; 2]>,
    asymmetries: Vec<PlacementAsymmetry>,
}

#[test]
fn recorded_topology_corpus_selects_exact_expected_cpu_lists() {
    let fixtures: Vec<RecordedFixture> = serde_json::from_str(include_str!(
        "../../../docs/fixtures/phase3/topologies.json"
    ))
    .unwrap();
    let names = fixtures
        .iter()
        .map(|fixture| fixture.name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "dual-socket",
            "hybrid-performance-efficiency",
            "dual-cache-domain",
            "no-smt",
            "processor-groups",
            "smt-16c-32t-shared-one-thread",
            "smt-16c-32t-disjoint-one-thread",
            "restricted-cpuset",
            "smt-16c-32t",
        ])
    );

    for fixture in fixtures {
        let cores = fixture
            .cores
            .iter()
            .map(|core| PhysicalCore {
                logical_cpus: ids(&core.cpus),
            })
            .collect::<Vec<_>>();
        let topology = CpuTopology {
            source: fixture.source,
            physical_core_count: cores.len(),
            logical_cpu_count: cores.iter().map(|core| core.logical_cpus.len()).sum(),
            sibling_mapping: SiblingMapping::Known {
                cores: cores.clone(),
            },
        };
        let allowed = AllowedCpuSet::Known {
            source: match fixture.source {
                TopologySource::WindowsLogicalProcessorInformation => {
                    colosseum_engine::AllowedCpuSource::WindowsProcessAffinity
                }
                TopologySource::LinuxThreadSiblingsList => {
                    colosseum_engine::AllowedCpuSource::LinuxSchedulerAffinity
                }
                TopologySource::MacOsSysctlCounts => unreachable!("no macOS identity fixture"),
            },
            cpus: ids(&fixture.allowed),
        };
        let characteristics = CpuCharacteristics {
            source: CharacteristicsSource::WindowsCpuSets,
            cores: fixture
                .cores
                .iter()
                .map(|core| PhysicalCoreCharacteristics {
                    logical_cpus: ids(&core.cpus),
                    core_class: core.core_class,
                    numa_node: core.numa_node,
                    last_level_cache: core.last_level_cache,
                })
                .collect(),
        };
        let plan = plan_cpu_placement(
            &topology,
            &allowed,
            &characteristics,
            &CpuPlacementPolicy::Auto {
                headroom_physical_cores: fixture.headroom_physical_cores,
            },
        )
        .unwrap_or_else(|error| panic!("{} planning failed: {error}", fixture.name));
        assert_eq!(
            plan.logical_cpus(),
            Some(ids(&fixture.expected_pool)),
            "{} selected an unexpected placement pool",
            fixture.name
        );
        let actual = allocate_game_slots(
            &plan,
            &characteristics,
            fixture.game_slots,
            fixture.allocation(),
        )
        .unwrap_or_else(|error| panic!("{} allocation failed: {error}", fixture.name));
        assert_eq!(actual.len(), fixture.expected.len(), "{}", fixture.name);
        for (actual, expected) in actual.iter().zip(&fixture.expected) {
            assert_eq!(
                actual.engine_a.allocation,
                CpuAllocation::Enforced(ids(&expected.engine_a)),
                "{} slot {} engine A",
                fixture.name,
                actual.slot_index
            );
            assert_eq!(
                actual.engine_b.allocation,
                CpuAllocation::Enforced(ids(&expected.engine_b)),
                "{} slot {} engine B",
                fixture.name,
                actual.slot_index
            );
            assert_eq!(
                actual.asymmetries, expected.asymmetries,
                "{} slot {} asymmetry",
                fixture.name, actual.slot_index
            );
        }
    }
}

#[test]
fn busy_children_reside_only_on_their_enforced_cpu() {
    let capability = affinity_capability();
    if capability.level != AffinitySupportLevel::Enforced {
        eprintln!(
            "SKIP residency: {}",
            capability
                .reason
                .as_deref()
                .unwrap_or("hard affinity unavailable")
        );
        return;
    }
    let topology = detect_cpu_topology().unwrap();
    let allowed = detect_allowed_cpu_set(&topology).unwrap();
    let allowed_cpus = match allowed {
        AllowedCpuSet::Known { cpus, .. } => cpus,
        AllowedCpuSet::Unavailable { reason } => {
            eprintln!("SKIP residency: {reason}");
            return;
        }
    };

    let root = tempfile::tempdir().unwrap();
    let mut children = (0..2)
        .map(|index| {
            let gate = root.path().join(format!("gate-{index}"));
            let child = Command::new(env!("CARGO_BIN_EXE_colosseum-affinity-fixture"))
                .arg(&gate)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let groups = process_affinity_groups(child.id()).unwrap();
            let candidates = allowed_cpus
                .iter()
                .filter(|cpu| groups.contains(&cpu.group))
                .copied()
                .collect::<Vec<_>>();
            assert!(
                !candidates.is_empty(),
                "child groups {groups:?} have no CPU in the harness allowed set"
            );
            let cpu = candidates[index % candidates.len()];
            (child, gate, cpu)
        })
        .collect::<Vec<_>>();

    for (child, gate, cpu) in &children {
        apply_process_affinity(child.id(), &CpuAllocation::Enforced(vec![*cpu])).unwrap();
        std::fs::write(gate, b"sample").unwrap();
    }
    for (child, _, expected) in children.drain(..) {
        assert_child_residency(child, expected);
    }
}

fn ids(values: &[[u32; 2]]) -> Vec<LogicalCpuId> {
    values
        .iter()
        .map(|[group, number]| LogicalCpuId {
            group: *group as u16,
            number: *number,
        })
        .collect()
}

fn assert_child_residency(child: Child, expected: LogicalCpuId) {
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "residency fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let observed = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| {
            let (group, number) = line.split_once(':').unwrap();
            LogicalCpuId {
                group: group.parse().unwrap(),
                number: number.parse().unwrap(),
            }
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(observed, BTreeSet::from([expected]));
}
