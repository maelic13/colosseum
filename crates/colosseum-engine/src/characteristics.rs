//! OS-native core-class and NUMA characteristics used by placement policy.

use std::collections::{BTreeMap, BTreeSet};
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::topology::{CpuTopology, LogicalCpuId};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "source", content = "value", rename_all = "kebab-case")]
pub enum CoreClass {
    #[default]
    Unknown,
    WindowsEfficiencyClass(u8),
    LinuxCapacity(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct NumaNodeId {
    /// Windows processor group; zero on Linux.
    pub group: u16,
    /// OS node index within that namespace.
    pub number: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalCoreCharacteristics {
    pub logical_cpus: Vec<LogicalCpuId>,
    pub core_class: CoreClass,
    pub numa_node: Option<NumaNodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CharacteristicsSource {
    WindowsCpuSets,
    LinuxSysfs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuCharacteristics {
    pub source: CharacteristicsSource,
    pub cores: Vec<PhysicalCoreCharacteristics>,
}

impl CpuCharacteristics {
    /// Deterministic unknown metadata for fixtures or platforms without a
    /// quality signal. Production composition should prefer OS detection.
    #[must_use]
    pub fn unknown(topology: &CpuTopology) -> Option<Self> {
        Some(Self {
            source: if cfg!(windows) {
                CharacteristicsSource::WindowsCpuSets
            } else {
                CharacteristicsSource::LinuxSysfs
            },
            cores: topology
                .cores()?
                .iter()
                .map(|core| PhysicalCoreCharacteristics {
                    logical_cpus: core.logical_cpus.clone(),
                    core_class: CoreClass::Unknown,
                    numa_node: None,
                })
                .collect(),
        })
    }
}

#[derive(Debug, Error)]
pub enum CharacteristicsError {
    #[error("logical CPU identities are unavailable for this topology")]
    TopologyIdentityUnavailable,
    #[error("characteristics are missing for logical CPU {0:?}")]
    CpuMissing(LogicalCpuId),
    #[error("SMT siblings of one physical core report inconsistent {field}")]
    InconsistentCore { field: &'static str },
    #[error("could not read CPU characteristic at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid CPU characteristic {value:?} at {path}")]
    InvalidValue { path: PathBuf, value: String },
    #[error("CPU characteristic detection is unsupported on {0}")]
    UnsupportedPlatform(&'static str),
}

#[derive(Debug, Clone, Copy)]
struct LogicalCharacteristics {
    core_class: CoreClass,
    numa_node: Option<NumaNodeId>,
}

pub fn detect_cpu_characteristics(
    topology: &CpuTopology,
) -> Result<CpuCharacteristics, CharacteristicsError> {
    detect_platform(topology)
}

fn assemble(
    topology: &CpuTopology,
    source: CharacteristicsSource,
    by_cpu: &BTreeMap<LogicalCpuId, LogicalCharacteristics>,
) -> Result<CpuCharacteristics, CharacteristicsError> {
    let cores = topology
        .cores()
        .ok_or(CharacteristicsError::TopologyIdentityUnavailable)?
        .iter()
        .map(|core| {
            let observations = core
                .logical_cpus
                .iter()
                .map(|cpu| {
                    by_cpu
                        .get(cpu)
                        .copied()
                        .ok_or(CharacteristicsError::CpuMissing(*cpu))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let classes = observations
                .iter()
                .map(|item| item.core_class)
                .collect::<BTreeSet<_>>();
            if classes.len() != 1 {
                return Err(CharacteristicsError::InconsistentCore {
                    field: "core class",
                });
            }
            let nodes = observations
                .iter()
                .map(|item| item.numa_node)
                .collect::<BTreeSet<_>>();
            if nodes.len() != 1 {
                return Err(CharacteristicsError::InconsistentCore { field: "NUMA node" });
            }
            Ok(PhysicalCoreCharacteristics {
                logical_cpus: core.logical_cpus.clone(),
                core_class: *classes.first().expect("one observation per physical core"),
                numa_node: *nodes.first().expect("one observation per physical core"),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CpuCharacteristics { source, cores })
}

#[cfg(windows)]
fn detect_platform(topology: &CpuTopology) -> Result<CpuCharacteristics, CharacteristicsError> {
    let entries = crate::allowed_cpus::windows::query_cpu_set_entries(unsafe {
        windows_sys::Win32::System::Threading::GetCurrentProcess()
    })
    .map_err(|error| CharacteristicsError::Io {
        path: PathBuf::from("GetSystemCpuSetInformation"),
        source: std::io::Error::other(error.to_string()),
    })?;
    let by_cpu = entries
        .into_iter()
        .map(|entry| {
            (
                entry.cpu,
                LogicalCharacteristics {
                    core_class: CoreClass::WindowsEfficiencyClass(entry.efficiency_class),
                    numa_node: Some(NumaNodeId {
                        group: entry.cpu.group,
                        number: u32::from(entry.numa_node_index),
                    }),
                },
            )
        })
        .collect();
    assemble(topology, CharacteristicsSource::WindowsCpuSets, &by_cpu)
}

#[cfg(target_os = "linux")]
fn detect_platform(topology: &CpuTopology) -> Result<CpuCharacteristics, CharacteristicsError> {
    linux::detect(topology, Path::new("/sys/devices/system/cpu"))
}

#[cfg(target_os = "macos")]
fn detect_platform(_topology: &CpuTopology) -> Result<CpuCharacteristics, CharacteristicsError> {
    Err(CharacteristicsError::TopologyIdentityUnavailable)
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn detect_platform(_topology: &CpuTopology) -> Result<CpuCharacteristics, CharacteristicsError> {
    Err(CharacteristicsError::UnsupportedPlatform(
        std::env::consts::OS,
    ))
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;

    use super::*;

    pub(super) fn detect(
        topology: &CpuTopology,
        root: &Path,
    ) -> Result<CpuCharacteristics, CharacteristicsError> {
        let mut observations = BTreeMap::new();
        for cpu in topology
            .cores()
            .ok_or(CharacteristicsError::TopologyIdentityUnavailable)?
            .iter()
            .flat_map(|core| core.logical_cpus.iter().copied())
        {
            let cpu_root = root.join(format!("cpu{}", cpu.number));
            let capacity_path = cpu_root.join("cpu_capacity");
            let core_class = match fs::read_to_string(&capacity_path) {
                Ok(value) => CoreClass::LinuxCapacity(parse_u32(&capacity_path, &value)?),
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => CoreClass::Unknown,
                Err(source) => {
                    return Err(CharacteristicsError::Io {
                        path: capacity_path,
                        source,
                    });
                }
            };
            let mut nodes = fs::read_dir(&cpu_root)
                .map_err(|source| CharacteristicsError::Io {
                    path: cpu_root.clone(),
                    source,
                })?
                .filter_map(Result::ok)
                .filter_map(|entry| {
                    entry
                        .file_name()
                        .to_str()
                        .and_then(|name| name.strip_prefix("node"))
                        .and_then(|number| number.parse::<u32>().ok())
                })
                .collect::<BTreeSet<_>>();
            let numa_node = match nodes.len() {
                0 => None,
                1 => Some(NumaNodeId {
                    group: 0,
                    number: nodes.pop_first().expect("one node"),
                }),
                _ => {
                    return Err(CharacteristicsError::InconsistentCore {
                        field: "logical CPU NUMA membership",
                    });
                }
            };
            observations.insert(
                cpu,
                LogicalCharacteristics {
                    core_class,
                    numa_node,
                },
            );
        }
        assemble(topology, CharacteristicsSource::LinuxSysfs, &observations)
    }

    fn parse_u32(path: &Path, value: &str) -> Result<u32, CharacteristicsError> {
        value
            .trim()
            .parse()
            .map_err(|_| CharacteristicsError::InvalidValue {
                path: path.to_path_buf(),
                value: value.into(),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::{PhysicalCore, SiblingMapping, TopologySource};

    fn topology() -> CpuTopology {
        CpuTopology {
            source: TopologySource::LinuxThreadSiblingsList,
            physical_core_count: 2,
            logical_cpu_count: 4,
            sibling_mapping: SiblingMapping::Known {
                cores: vec![
                    PhysicalCore {
                        logical_cpus: vec![LogicalCpuId::from(0), LogicalCpuId::from(2)],
                    },
                    PhysicalCore {
                        logical_cpus: vec![LogicalCpuId::from(1), LogicalCpuId::from(3)],
                    },
                ],
            },
        }
    }

    #[test]
    fn assembly_records_class_and_numa_for_each_physical_core() {
        let observations = [(0, 1024, 0), (2, 1024, 0), (1, 512, 1), (3, 512, 1)]
            .into_iter()
            .map(|(cpu, capacity, node)| {
                (
                    LogicalCpuId::from(cpu),
                    LogicalCharacteristics {
                        core_class: CoreClass::LinuxCapacity(capacity),
                        numa_node: Some(NumaNodeId {
                            group: 0,
                            number: node,
                        }),
                    },
                )
            })
            .collect();
        let characteristics = assemble(
            &topology(),
            CharacteristicsSource::LinuxSysfs,
            &observations,
        )
        .unwrap();
        assert_eq!(
            characteristics.cores[0].core_class,
            CoreClass::LinuxCapacity(1024)
        );
        assert_eq!(characteristics.cores[1].numa_node.unwrap().number, 1);
    }

    #[test]
    fn assembly_rejects_sibling_disagreement() {
        let observations = [(0, 1024), (2, 512), (1, 512), (3, 512)]
            .into_iter()
            .map(|(cpu, capacity)| {
                (
                    LogicalCpuId::from(cpu),
                    LogicalCharacteristics {
                        core_class: CoreClass::LinuxCapacity(capacity),
                        numa_node: None,
                    },
                )
            })
            .collect();
        assert!(matches!(
            assemble(
                &topology(),
                CharacteristicsSource::LinuxSysfs,
                &observations
            ),
            Err(CharacteristicsError::InconsistentCore {
                field: "core class"
            })
        ));
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn host_characteristics_cover_detected_topology() {
        let topology = crate::topology::detect_cpu_topology().unwrap();
        let characteristics = detect_cpu_characteristics(&topology).unwrap();
        assert_eq!(characteristics.cores.len(), topology.physical_core_count);
    }
}
