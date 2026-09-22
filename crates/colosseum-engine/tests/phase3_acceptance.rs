use std::collections::BTreeSet;

use colosseum_application::CpuAllocation;
use colosseum_engine::{
    AllowedCpuSet, CacheDomainId, CharacteristicsSource, CoreClass, CpuCharacteristics,
    CpuPlacementPolicy, CpuTopology, LogicalCpuId, NumaNodeId, PhysicalCore,
    PhysicalCoreCharacteristics, PlacementAsymmetry, SiblingMapping, SlotAllocation,
    TopologySource, allocate_game_slots, plan_cpu_placement,
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

fn ids(values: &[[u32; 2]]) -> Vec<LogicalCpuId> {
    values
        .iter()
        .map(|[group, number]| LogicalCpuId {
            group: *group as u16,
            number: *number,
        })
        .collect()
}
