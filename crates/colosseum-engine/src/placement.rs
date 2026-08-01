//! CPU-placement policy resolution over an already discovered topology.
//!
//! This module decides *which* logical processors a placement mode names. It
//! deliberately does not inspect process restrictions or apply affinity; those
//! are platform-adapter responsibilities added by later steps.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    /// `auto` selected complete physical cores, including every SMT sibling.
    WholePhysicalCores {
        cores: Vec<PhysicalCore>,
        headroom_physical_cores: usize,
    },
    /// An explicit request selects the exact supplied logical CPU identities.
    ExplicitLogicalCpus { cpus: Vec<LogicalCpuId> },
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
            Self::ExplicitLogicalCpus { cpus } => Some(cpus.clone()),
        }
    }
}

/// Resolve a placement policy against a topology with an exact sibling map.
///
/// `auto` always allocates whole physical cores, never a guessed subset of
/// SMT siblings. An explicit list remains an exact user request and may name a
/// subset of a physical core. It is nevertheless checked against the reported
/// logical-CPU identities so a typo cannot silently become a different plan.
pub fn plan_cpu_placement(
    topology: &CpuTopology,
    policy: &CpuPlacementPolicy,
) -> Result<CpuPlacementPlan, CpuPlacementError> {
    match policy {
        CpuPlacementPolicy::Off => Ok(CpuPlacementPlan::Unrestricted),
        CpuPlacementPolicy::Auto {
            headroom_physical_cores,
        } => {
            let cores = known_cores(topology)?;
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
            let mut selected = BTreeSet::new();
            for cpu in cpus {
                if !selected.insert(*cpu) {
                    return Err(CpuPlacementError::DuplicateExplicitCpu(*cpu));
                }
                if !known.contains(cpu) {
                    return Err(CpuPlacementError::UnknownExplicitCpu(*cpu));
                }
            }
            Ok(CpuPlacementPlan::ExplicitLogicalCpus {
                cpus: selected.into_iter().collect(),
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

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CpuPlacementError {
    #[error("CPU placement requires an exact logical-CPU sibling map, but it is unavailable")]
    SiblingMappingUnavailable,
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
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn auto_default_keeps_two_whole_physical_cores_free() {
        let plan = plan_cpu_placement(&smt_topology(), &CpuPlacementPolicy::default()).unwrap();
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
        let plan = plan_cpu_placement(
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
        let error = plan_cpu_placement(
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
        let plan = plan_cpu_placement(&topology, &CpuPlacementPolicy::Off).unwrap();
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
        let plan = plan_cpu_placement(
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
        let empty = plan_cpu_placement(
            &smt_topology(),
            &CpuPlacementPolicy::Explicit { cpus: vec![] },
        )
        .unwrap_err();
        assert_eq!(empty, CpuPlacementError::EmptyExplicitList);

        let duplicate = plan_cpu_placement(
            &smt_topology(),
            &CpuPlacementPolicy::Explicit {
                cpus: vec![cpu(1), cpu(1)],
            },
        )
        .unwrap_err();
        assert_eq!(duplicate, CpuPlacementError::DuplicateExplicitCpu(cpu(1)));

        let unknown = plan_cpu_placement(
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
                plan_cpu_placement(&topology, &policy).unwrap_err(),
                CpuPlacementError::SiblingMappingUnavailable
            );
        }
    }
}
