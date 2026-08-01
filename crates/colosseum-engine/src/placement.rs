//! CPU-placement policy resolution over an already discovered topology.
//!
//! This module decides *which available* logical processors a placement mode
//! names. Applying affinity remains a separate platform-adapter responsibility.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use colosseum_application::CpuAllocation;

use crate::allowed_cpus::AllowedCpuSet;
use crate::topology::{CpuTopology, LogicalCpuId, PhysicalCore};

/// The default number of whole physical cores left for the harness and host.
pub const DEFAULT_AUTO_HEADROOM_PHYSICAL_CORES: usize = 2;

/// User-selected CPU placement policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum CpuPlacementPolicy {
    /// Select all but `headroom_physical_cores` physical cores.
    Auto {
        #[serde(default = "default_auto_headroom_physical_cores")]
        headroom_physical_cores: usize,
    },
    /// Do not select or restrict any CPUs.
    Off,
    /// Use exactly these operating-system logical CPU identities.
    Explicit { cpus: Vec<LogicalCpuId> },
}

impl Default for CpuPlacementPolicy {
    fn default() -> Self {
        Self::Auto {
            headroom_physical_cores: DEFAULT_AUTO_HEADROOM_PHYSICAL_CORES,
        }
    }
}

/// The resolved CPU selection. Applying it to a process is intentionally a
/// separate concern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CpuPlacementPlan {
    /// The caller deliberately requested normal operating-system scheduling.
    Unrestricted,
    /// `auto` selected complete physical cores, including every available SMT
    /// sibling reported for each core.
    WholePhysicalCores {
        cores: Vec<PhysicalCore>,
        headroom_physical_cores: usize,
    },
    /// An explicit request selects the exact supplied logical CPU identities.
    ExplicitLogicalCpus {
        cpus: Vec<LogicalCpuId>,
        physical_cores: Vec<PhysicalCore>,
    },
}

impl CpuPlacementPlan {
    /// Logical CPUs selected by this plan, in stable identity order.
    #[must_use]
    pub fn logical_cpus(&self) -> Option<Vec<LogicalCpuId>> {
        match self {
            Self::Unrestricted => None,
            Self::WholePhysicalCores { cores, .. } => Some(
                cores
                    .iter()
                    .flat_map(|core| core.logical_cpus.iter().copied())
                    .collect(),
            ),
            Self::ExplicitLogicalCpus { cpus, .. } => Some(cpus.clone()),
        }
    }

    fn physical_cores(&self) -> Option<&[PhysicalCore]> {
        match self {
            Self::Unrestricted => None,
            Self::WholePhysicalCores { cores, .. } => Some(cores),
            Self::ExplicitLogicalCpus { physical_cores, .. } => Some(physical_cores),
        }
    }
}

/// Disjoint CPU allocations for the two engines occupying one concurrent game
/// slot. These are process allocations, independent of UCI worker options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameSlotCpuAllocation {
    pub slot_index: usize,
    pub engine_a: CpuAllocation,
    pub engine_b: CpuAllocation,
}

/// Divide a resolved placement pool between concurrent game slots.
///
/// Each engine receives `cores_per_engine` physical cores. A physical core's
/// available SMT siblings always stay together. Slots never share a logical
/// CPU. With placement `off`, every engine remains unrestricted.
pub fn allocate_game_slots(
    plan: &CpuPlacementPlan,
    game_slots: usize,
    cores_per_engine: usize,
) -> Result<Vec<GameSlotCpuAllocation>, CpuPlacementError> {
    if game_slots == 0 {
        return Err(CpuPlacementError::ZeroGameSlots);
    }
    if cores_per_engine == 0 {
        return Err(CpuPlacementError::ZeroCoresPerEngine);
    }
    let Some(cores) = plan.physical_cores() else {
        return Ok((0..game_slots)
            .map(|slot_index| GameSlotCpuAllocation {
                slot_index,
                engine_a: CpuAllocation::Unrestricted,
                engine_b: CpuAllocation::Unrestricted,
            })
            .collect());
    };
    let required = game_slots
        .checked_mul(2)
        .and_then(|engines| engines.checked_mul(cores_per_engine))
        .ok_or(CpuPlacementError::AllocationSizeOverflow)?;
    if required > cores.len() {
        return Err(CpuPlacementError::InsufficientPhysicalCores {
            required,
            available: cores.len(),
            game_slots,
            cores_per_engine,
        });
    }

    let allocation_for = |engine_index: usize| {
        let start = engine_index * cores_per_engine;
        let cpus = cores[start..start + cores_per_engine]
            .iter()
            .flat_map(|core| core.logical_cpus.iter().copied())
            .collect();
        CpuAllocation::Enforced(cpus)
    };
    Ok((0..game_slots)
        .map(|slot_index| GameSlotCpuAllocation {
            slot_index,
            engine_a: allocation_for(slot_index * 2),
            engine_b: allocation_for(slot_index * 2 + 1),
        })
        .collect())
}

/// Resolve a placement policy against an exact sibling map and the current
/// process's allowed CPU set.
///
/// `auto` counts physical cores only after applying the allowed set, and keeps
/// every allowed SMT sibling belonging to each chosen core. An explicit list
/// remains an exact user request and may name a subset of a physical core, but
/// every identity must exist in the topology and be allowed to this process.
pub fn plan_cpu_placement(
    topology: &CpuTopology,
    allowed: &AllowedCpuSet,
    policy: &CpuPlacementPolicy,
) -> Result<CpuPlacementPlan, CpuPlacementError> {
    match policy {
        CpuPlacementPolicy::Off => Ok(CpuPlacementPlan::Unrestricted),
        CpuPlacementPolicy::Auto {
            headroom_physical_cores,
        } => {
            let cores = available_cores(topology, allowed)?;
            if *headroom_physical_cores >= cores.len() {
                return Err(CpuPlacementError::HeadroomExhaustsTopology {
                    headroom_physical_cores: *headroom_physical_cores,
                    physical_core_count: cores.len(),
                });
            }
            let selected_count = cores.len() - headroom_physical_cores;
            Ok(CpuPlacementPlan::WholePhysicalCores {
                cores: cores[..selected_count].to_vec(),
                headroom_physical_cores: *headroom_physical_cores,
            })
        }
        CpuPlacementPolicy::Explicit { cpus } => {
            if cpus.is_empty() {
                return Err(CpuPlacementError::EmptyExplicitList);
            }
            let known = known_cores(topology)?
                .iter()
                .flat_map(|core| core.logical_cpus.iter().copied())
                .collect::<BTreeSet<_>>();
            let allowed = known_allowed_cpus(allowed)?;
            let mut selected = BTreeSet::new();
            for cpu in cpus {
                if !selected.insert(*cpu) {
                    return Err(CpuPlacementError::DuplicateExplicitCpu(*cpu));
                }
                if !known.contains(cpu) {
                    return Err(CpuPlacementError::UnknownExplicitCpu(*cpu));
                }
                if !allowed.contains(cpu) {
                    return Err(CpuPlacementError::ExplicitCpuNotAllowed(*cpu));
                }
            }
            let cpus = selected.into_iter().collect::<Vec<_>>();
            let physical_cores = known_cores(topology)?
                .iter()
                .filter_map(|core| {
                    let logical_cpus = core
                        .logical_cpus
                        .iter()
                        .filter(|cpu| cpus.binary_search(cpu).is_ok())
                        .copied()
                        .collect::<Vec<_>>();
                    (!logical_cpus.is_empty()).then_some(PhysicalCore { logical_cpus })
                })
                .collect();
            Ok(CpuPlacementPlan::ExplicitLogicalCpus {
                cpus,
                physical_cores,
            })
        }
    }
}

fn default_auto_headroom_physical_cores() -> usize {
    DEFAULT_AUTO_HEADROOM_PHYSICAL_CORES
}

fn known_cores(topology: &CpuTopology) -> Result<&[PhysicalCore], CpuPlacementError> {
    topology
        .cores()
        .ok_or(CpuPlacementError::SiblingMappingUnavailable)
}

fn known_allowed_cpus(
    allowed: &AllowedCpuSet,
) -> Result<BTreeSet<LogicalCpuId>, CpuPlacementError> {
    match allowed {
        AllowedCpuSet::Known { cpus, .. } => {
            if cpus.is_empty() {
                return Err(CpuPlacementError::NoAllowedLogicalCpus);
            }
            Ok(cpus.iter().copied().collect())
        }
        AllowedCpuSet::Unavailable { reason } => Err(CpuPlacementError::AllowedCpuSetUnavailable {
            reason: reason.clone(),
        }),
    }
}

fn available_cores(
    topology: &CpuTopology,
    allowed: &AllowedCpuSet,
) -> Result<Vec<PhysicalCore>, CpuPlacementError> {
    let cores = known_cores(topology)?;
    let known = cores
        .iter()
        .flat_map(|core| core.logical_cpus.iter().copied())
        .collect::<BTreeSet<_>>();
    let allowed = known_allowed_cpus(allowed)?;
    if let Some(cpu) = allowed.iter().find(|cpu| !known.contains(cpu)) {
        return Err(CpuPlacementError::AllowedCpuMissingFromTopology(*cpu));
    }
    let cores = cores
        .iter()
        .filter_map(|core| {
            let logical_cpus = core
                .logical_cpus
                .iter()
                .filter(|cpu| allowed.contains(cpu))
                .copied()
                .collect::<Vec<_>>();
            (!logical_cpus.is_empty()).then_some(PhysicalCore { logical_cpus })
        })
        .collect::<Vec<_>>();
    if cores.is_empty() {
        return Err(CpuPlacementError::NoAllowedLogicalCpus);
    }
    Ok(cores)
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CpuPlacementError {
    #[error("CPU placement requires an exact logical-CPU sibling map, but it is unavailable")]
    SiblingMappingUnavailable,
    #[error("the allowed CPU set is unavailable: {reason}")]
    AllowedCpuSetUnavailable { reason: String },
    #[error("the operating system reports no allowed logical CPUs")]
    NoAllowedLogicalCpus,
    #[error("allowed logical CPU {0:?} is absent from the detected topology")]
    AllowedCpuMissingFromTopology(LogicalCpuId),
    #[error(
        "automatic headroom of {headroom_physical_cores} physical cores leaves no core to allocate from {physical_core_count}"
    )]
    HeadroomExhaustsTopology {
        headroom_physical_cores: usize,
        physical_core_count: usize,
    },
    #[error("explicit CPU placement requires at least one logical CPU")]
    EmptyExplicitList,
    #[error("explicit CPU list contains logical CPU {0:?} more than once")]
    DuplicateExplicitCpu(LogicalCpuId),
    #[error("explicit CPU list contains logical CPU {0:?}, which is not in the detected topology")]
    UnknownExplicitCpu(LogicalCpuId),
    #[error("explicit CPU list contains logical CPU {0:?}, which is not allowed to this process")]
    ExplicitCpuNotAllowed(LogicalCpuId),
    #[error("game-slot count must be at least one")]
    ZeroGameSlots,
    #[error("cores-per-engine must be at least one")]
    ZeroCoresPerEngine,
    #[error("CPU allocation size overflow")]
    AllocationSizeOverflow,
    #[error(
        "{game_slots} game slots at {cores_per_engine} physical cores per engine require {required} physical cores, but placement provides {available}"
    )]
    InsufficientPhysicalCores {
        required: usize,
        available: usize,
        game_slots: usize,
        cores_per_engine: usize,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::allowed_cpus::AllowedCpuSource;
    use crate::topology::{SiblingMapping, TopologySource};

    fn cpu(number: u32) -> LogicalCpuId {
        LogicalCpuId { group: 0, number }
    }

    fn smt_topology() -> CpuTopology {
        CpuTopology {
            source: TopologySource::LinuxThreadSiblingsList,
            physical_core_count: 4,
            logical_cpu_count: 8,
            sibling_mapping: SiblingMapping::Known {
                cores: vec![
                    PhysicalCore {
                        logical_cpus: vec![cpu(0), cpu(4)],
                    },
                    PhysicalCore {
                        logical_cpus: vec![cpu(1), cpu(5)],
                    },
                    PhysicalCore {
                        logical_cpus: vec![cpu(2), cpu(6)],
                    },
                    PhysicalCore {
                        logical_cpus: vec![cpu(3), cpu(7)],
                    },
                ],
            },
        }
    }

    fn all_allowed(topology: &CpuTopology) -> AllowedCpuSet {
        AllowedCpuSet::Known {
            source: AllowedCpuSource::LinuxSchedulerAffinity,
            cpus: topology
                .cores()
                .unwrap()
                .iter()
                .flat_map(|core| core.logical_cpus.iter().copied())
                .collect(),
        }
    }

    fn plan(
        topology: &CpuTopology,
        policy: &CpuPlacementPolicy,
    ) -> Result<CpuPlacementPlan, CpuPlacementError> {
        plan_cpu_placement(topology, &all_allowed(topology), policy)
    }

    #[test]
    fn auto_default_keeps_two_whole_physical_cores_free() {
        let plan = plan(&smt_topology(), &CpuPlacementPolicy::default()).unwrap();
        assert_eq!(
            plan,
            CpuPlacementPlan::WholePhysicalCores {
                cores: vec![
                    PhysicalCore {
                        logical_cpus: vec![cpu(0), cpu(4)],
                    },
                    PhysicalCore {
                        logical_cpus: vec![cpu(1), cpu(5)],
                    },
                ],
                headroom_physical_cores: 2,
            }
        );
        assert_eq!(
            plan.logical_cpus(),
            Some(vec![cpu(0), cpu(4), cpu(1), cpu(5)])
        );
    }

    #[test]
    fn auto_with_custom_headroom_selects_complete_non_smt_cores() {
        let topology = CpuTopology {
            source: TopologySource::LinuxThreadSiblingsList,
            physical_core_count: 3,
            logical_cpu_count: 3,
            sibling_mapping: SiblingMapping::Known {
                cores: vec![
                    PhysicalCore {
                        logical_cpus: vec![cpu(0)],
                    },
                    PhysicalCore {
                        logical_cpus: vec![cpu(1)],
                    },
                    PhysicalCore {
                        logical_cpus: vec![cpu(2)],
                    },
                ],
            },
        };
        let plan = plan(
            &topology,
            &CpuPlacementPolicy::Auto {
                headroom_physical_cores: 1,
            },
        )
        .unwrap();
        assert_eq!(plan.logical_cpus(), Some(vec![cpu(0), cpu(1)]));
    }

    #[test]
    fn auto_rejects_headroom_that_leaves_no_usable_core() {
        let error = plan(
            &smt_topology(),
            &CpuPlacementPolicy::Auto {
                headroom_physical_cores: 4,
            },
        )
        .unwrap_err();
        assert_eq!(
            error,
            CpuPlacementError::HeadroomExhaustsTopology {
                headroom_physical_cores: 4,
                physical_core_count: 4,
            }
        );
    }

    #[test]
    fn off_needs_no_sibling_mapping_and_selects_nothing() {
        let topology = CpuTopology {
            source: TopologySource::MacOsSysctlCounts,
            physical_core_count: 8,
            logical_cpu_count: 8,
            sibling_mapping: SiblingMapping::Unavailable {
                reason: "counts only".into(),
            },
        };
        let plan = plan_cpu_placement(
            &topology,
            &AllowedCpuSet::Unavailable {
                reason: "no logical IDs".into(),
            },
            &CpuPlacementPolicy::Off,
        )
        .unwrap();
        assert_eq!(plan, CpuPlacementPlan::Unrestricted);
        assert_eq!(plan.logical_cpus(), None);
    }

    #[test]
    fn explicit_list_is_validated_and_canonicalized_by_group_qualified_identity() {
        let topology = CpuTopology {
            source: TopologySource::WindowsLogicalProcessorInformation,
            physical_core_count: 2,
            logical_cpu_count: 4,
            sibling_mapping: SiblingMapping::Known {
                cores: vec![
                    PhysicalCore {
                        logical_cpus: vec![
                            LogicalCpuId {
                                group: 0,
                                number: 0,
                            },
                            LogicalCpuId {
                                group: 0,
                                number: 1,
                            },
                        ],
                    },
                    PhysicalCore {
                        logical_cpus: vec![
                            LogicalCpuId {
                                group: 1,
                                number: 0,
                            },
                            LogicalCpuId {
                                group: 1,
                                number: 1,
                            },
                        ],
                    },
                ],
            },
        };
        let plan = plan(
            &topology,
            &CpuPlacementPolicy::Explicit {
                cpus: vec![
                    LogicalCpuId {
                        group: 1,
                        number: 0,
                    },
                    LogicalCpuId {
                        group: 0,
                        number: 1,
                    },
                ],
            },
        )
        .unwrap();
        assert_eq!(
            plan.logical_cpus(),
            Some(vec![
                LogicalCpuId {
                    group: 0,
                    number: 1
                },
                LogicalCpuId {
                    group: 1,
                    number: 0
                },
            ])
        );
    }

    #[test]
    fn explicit_list_rejects_empty_duplicate_and_unknown_cpus() {
        let empty = plan(
            &smt_topology(),
            &CpuPlacementPolicy::Explicit { cpus: vec![] },
        )
        .unwrap_err();
        assert_eq!(empty, CpuPlacementError::EmptyExplicitList);

        let duplicate = plan(
            &smt_topology(),
            &CpuPlacementPolicy::Explicit {
                cpus: vec![cpu(1), cpu(1)],
            },
        )
        .unwrap_err();
        assert_eq!(duplicate, CpuPlacementError::DuplicateExplicitCpu(cpu(1)));

        let unknown = plan(
            &smt_topology(),
            &CpuPlacementPolicy::Explicit {
                cpus: vec![cpu(99)],
            },
        )
        .unwrap_err();
        assert_eq!(unknown, CpuPlacementError::UnknownExplicitCpu(cpu(99)));
    }

    #[test]
    fn auto_and_explicit_require_an_exact_sibling_map() {
        let topology = CpuTopology {
            source: TopologySource::MacOsSysctlCounts,
            physical_core_count: 8,
            logical_cpu_count: 8,
            sibling_mapping: SiblingMapping::Unavailable {
                reason: "counts only".into(),
            },
        };
        for policy in [
            CpuPlacementPolicy::Auto {
                headroom_physical_cores: 2,
            },
            CpuPlacementPolicy::Explicit { cpus: vec![cpu(0)] },
        ] {
            assert_eq!(
                plan_cpu_placement(
                    &topology,
                    &AllowedCpuSet::Unavailable {
                        reason: "no logical IDs".into(),
                    },
                    &policy,
                )
                .unwrap_err(),
                CpuPlacementError::SiblingMappingUnavailable
            );
        }
    }

    #[test]
    fn restricted_set_filters_smt_siblings_before_headroom_is_counted() {
        let topology = smt_topology();
        let allowed = AllowedCpuSet::Known {
            source: AllowedCpuSource::LinuxSchedulerAffinity,
            cpus: vec![cpu(1), cpu(2), cpu(5), cpu(7)],
        };
        let plan = plan_cpu_placement(
            &topology,
            &allowed,
            &CpuPlacementPolicy::Auto {
                headroom_physical_cores: 1,
            },
        )
        .unwrap();
        assert_eq!(plan.logical_cpus(), Some(vec![cpu(1), cpu(5), cpu(2)]));
    }

    #[test]
    fn explicit_cpu_must_be_in_the_process_allowed_set() {
        let topology = smt_topology();
        let allowed = AllowedCpuSet::Known {
            source: AllowedCpuSource::LinuxSchedulerAffinity,
            cpus: vec![cpu(0), cpu(4)],
        };
        assert_eq!(
            plan_cpu_placement(
                &topology,
                &allowed,
                &CpuPlacementPolicy::Explicit { cpus: vec![cpu(1)] },
            )
            .unwrap_err(),
            CpuPlacementError::ExplicitCpuNotAllowed(cpu(1))
        );
    }

    #[test]
    fn slot_allocation_assigns_configured_physical_cores_to_each_engine() {
        let topology = smt_topology();
        let placement = plan(
            &topology,
            &CpuPlacementPolicy::Auto {
                headroom_physical_cores: 0,
            },
        )
        .unwrap();
        let slots = allocate_game_slots(&placement, 1, 2).unwrap();
        assert_eq!(slots.len(), 1);
        assert_eq!(
            slots[0].engine_a,
            CpuAllocation::Enforced(vec![cpu(0), cpu(4), cpu(1), cpu(5)])
        );
        assert_eq!(
            slots[0].engine_b,
            CpuAllocation::Enforced(vec![cpu(2), cpu(6), cpu(3), cpu(7)])
        );
    }

    #[test]
    fn concurrent_slots_are_disjoint_and_capacity_is_physical_core_based() {
        let topology = smt_topology();
        let placement = plan(
            &topology,
            &CpuPlacementPolicy::Auto {
                headroom_physical_cores: 0,
            },
        )
        .unwrap();
        let slots = allocate_game_slots(&placement, 2, 1).unwrap();
        let allocations = slots
            .iter()
            .flat_map(|slot| [&slot.engine_a, &slot.engine_b])
            .map(|allocation| match allocation {
                CpuAllocation::Enforced(cpus) => cpus.clone(),
                _ => panic!("placement-on allocation must be enforced"),
            })
            .collect::<Vec<_>>();
        assert_eq!(allocations.len(), 4);
        assert_eq!(allocations[0], [cpu(0), cpu(4)]);
        assert_eq!(allocations[1], [cpu(1), cpu(5)]);
        assert_eq!(allocations[2], [cpu(2), cpu(6)]);
        assert_eq!(allocations[3], [cpu(3), cpu(7)]);

        assert_eq!(
            allocate_game_slots(&placement, 2, 2).unwrap_err(),
            CpuPlacementError::InsufficientPhysicalCores {
                required: 8,
                available: 4,
                game_slots: 2,
                cores_per_engine: 2,
            }
        );
    }

    #[test]
    fn explicit_partial_siblings_still_count_as_one_physical_core() {
        let topology = smt_topology();
        let placement = plan(
            &topology,
            &CpuPlacementPolicy::Explicit {
                cpus: vec![cpu(0), cpu(1)],
            },
        )
        .unwrap();
        let slots = allocate_game_slots(&placement, 1, 1).unwrap();
        assert_eq!(slots[0].engine_a, CpuAllocation::Enforced(vec![cpu(0)]));
        assert_eq!(slots[0].engine_b, CpuAllocation::Enforced(vec![cpu(1)]));
    }

    #[test]
    fn placement_off_keeps_every_slot_unrestricted() {
        let slots = allocate_game_slots(&CpuPlacementPlan::Unrestricted, 3, 8).unwrap();
        assert_eq!(slots.len(), 3);
        assert!(slots.iter().all(|slot| {
            slot.engine_a == CpuAllocation::Unrestricted
                && slot.engine_b == CpuAllocation::Unrestricted
        }));
    }

    #[test]
    fn slot_allocation_rejects_zero_dimensions() {
        assert_eq!(
            allocate_game_slots(&CpuPlacementPlan::Unrestricted, 0, 1).unwrap_err(),
            CpuPlacementError::ZeroGameSlots
        );
        assert_eq!(
            allocate_game_slots(&CpuPlacementPlan::Unrestricted, 1, 0).unwrap_err(),
            CpuPlacementError::ZeroCoresPerEngine
        );
    }
}
