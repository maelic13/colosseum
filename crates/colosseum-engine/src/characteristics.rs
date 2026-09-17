//! OS-native core-class and NUMA characteristics used by placement policy.

#[cfg(any(windows, target_os = "linux", test))]
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

/// One last-level cache domain as the operating system reports it.
///
/// On a multi-die part this is the chiplet boundary: two cores in different
/// domains do not share their last-level cache, so a game slot split across
/// them is not the same measurement as one kept inside a domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CacheDomainId {
    /// The cache level the operating system reported as the last one.
    pub level: u8,
    /// Stable index assigned in ascending order of the domain's lowest CPU.
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalCoreCharacteristics {
    pub logical_cpus: Vec<LogicalCpuId>,
    pub core_class: CoreClass,
    pub numa_node: Option<NumaNodeId>,
    /// `None` when the operating system reported no cache topology for this
    /// core. It is never inferred from CPU numbering or from another core.
    #[serde(default)]
    pub last_level_cache: Option<CacheDomainId>,
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
                    last_level_cache: None,
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

#[cfg(any(windows, target_os = "linux", test))]
#[derive(Debug, Clone, Copy)]
struct LogicalCharacteristics {
    core_class: CoreClass,
    numa_node: Option<NumaNodeId>,
    last_level_cache: Option<CacheDomainId>,
}

pub fn detect_cpu_characteristics(
    topology: &CpuTopology,
) -> Result<CpuCharacteristics, CharacteristicsError> {
    detect_platform(topology)
}

/// One operating-system cache report: the sharing CPU set at a cache level.
#[cfg(any(windows, target_os = "linux", test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheReport {
    pub(crate) level: u8,
    pub(crate) shared_cpus: BTreeSet<LogicalCpuId>,
}

/// Reduce raw cache reports to one last-level domain per logical CPU.
///
/// A CPU's last level is the highest level the operating system reported for
/// it; domains are the distinct sharing sets at that level, indexed in
/// ascending order of their lowest member so the identity is stable across
/// runs. A CPU that appears in no report has no domain, and nothing is
/// inferred for it from a neighbour.
#[cfg(any(windows, target_os = "linux", test))]
pub(crate) fn last_level_cache_domains(
    reports: &[CacheReport],
) -> BTreeMap<LogicalCpuId, CacheDomainId> {
    let mut best: BTreeMap<LogicalCpuId, (u8, &BTreeSet<LogicalCpuId>)> = BTreeMap::new();
    for report in reports {
        for cpu in &report.shared_cpus {
            let entry = best
                .entry(*cpu)
                .or_insert((report.level, &report.shared_cpus));
            if report.level > entry.0 {
                *entry = (report.level, &report.shared_cpus);
            }
        }
    }
    let mut domains: BTreeSet<(LogicalCpuId, u8, Vec<LogicalCpuId>)> = BTreeSet::new();
    for (level, shared) in best.values() {
        let members = shared.iter().copied().collect::<Vec<_>>();
        let Some(lowest) = members.first().copied() else {
            continue;
        };
        domains.insert((lowest, *level, members));
    }
    let mut assigned = BTreeMap::new();
    for (index, (_, level, members)) in domains.into_iter().enumerate() {
        let identity = CacheDomainId {
            level,
            index: u32::try_from(index).unwrap_or(u32::MAX),
        };
        for cpu in members {
            // Only the CPUs whose own last level is this one adopt the domain.
            if best
                .get(&cpu)
                .is_some_and(|(cpu_level, _)| *cpu_level == level)
            {
                assigned.insert(cpu, identity);
            }
        }
    }
    assigned
}

#[cfg(any(windows, target_os = "linux", test))]
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
            let caches = observations
                .iter()
                .map(|item| item.last_level_cache)
                .collect::<BTreeSet<_>>();
            if caches.len() != 1 {
                return Err(CharacteristicsError::InconsistentCore {
                    field: "last-level cache domain",
                });
            }
            Ok(PhysicalCoreCharacteristics {
                logical_cpus: core.logical_cpus.clone(),
                core_class: *classes.first().expect("one observation per physical core"),
                numa_node: *nodes.first().expect("one observation per physical core"),
                last_level_cache: *caches.first().expect("one observation per physical core"),
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
    let caches = last_level_cache_domains(&windows_cache::query()?);
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
                    last_level_cache: caches.get(&entry.cpu).copied(),
                },
            )
        })
        .collect();
    assemble(topology, CharacteristicsSource::WindowsCpuSets, &by_cpu)
}

/// `GetLogicalProcessorInformationEx(RelationCache)` — the only portable
/// Windows source for which cores share a last-level cache.
#[cfg(windows)]
mod windows_cache {
    use std::mem::{offset_of, size_of};

    use windows_sys::Win32::System::SystemInformation::{
        CACHE_RELATIONSHIP, CacheData, CacheUnified, GROUP_AFFINITY,
        GetLogicalProcessorInformationEx, RelationCache, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };

    use super::*;

    pub(super) fn query() -> Result<Vec<CacheReport>, CharacteristicsError> {
        let mut byte_length = 0_u32;
        // The first call obtains the required variable-length buffer size.
        unsafe {
            GetLogicalProcessorInformationEx(RelationCache, std::ptr::null_mut(), &mut byte_length);
        }
        if byte_length == 0 {
            // A host that reports no cache relationships at all is reported as
            // such; placement decides what to do about it.
            return Ok(Vec::new());
        }
        let words = (byte_length as usize).div_ceil(size_of::<usize>());
        let mut buffer = vec![0_usize; words];
        let mut returned = byte_length;
        let success = unsafe {
            GetLogicalProcessorInformationEx(
                RelationCache,
                buffer.as_mut_ptr().cast(),
                &mut returned,
            )
        };
        if success == 0 {
            return Err(CharacteristicsError::Io {
                path: PathBuf::from("GetLogicalProcessorInformationEx(RelationCache)"),
                source: std::io::Error::last_os_error(),
            });
        }
        parse_buffer(buffer.as_ptr().cast(), returned as usize)
    }

    fn parse_buffer(
        bytes: *const u8,
        length: usize,
    ) -> Result<Vec<CacheReport>, CharacteristicsError> {
        let mut offset = 0;
        let mut reports = Vec::new();
        while offset < length {
            if length - offset < 8 {
                return Err(malformed("truncated record header"));
            }
            let record = unsafe {
                &*bytes
                    .add(offset)
                    .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()
            };
            let size = record.Size as usize;
            if size < 8 || size > length - offset {
                return Err(malformed(&format!("invalid record size {size}")));
            }
            if record.Relationship != RelationCache {
                return Err(malformed(&format!(
                    "unexpected relationship {}",
                    record.Relationship
                )));
            }
            let cache = unsafe { &record.Anonymous.Cache };
            // An instruction or trace cache says nothing about data locality.
            if cache.Type == CacheUnified || cache.Type == CacheData {
                reports.push(CacheReport {
                    level: cache.Level,
                    shared_cpus: group_masks(bytes, offset, size, cache)?,
                });
            }
            offset += size;
        }
        Ok(reports)
    }

    fn group_masks(
        bytes: *const u8,
        offset: usize,
        size: usize,
        cache: &CACHE_RELATIONSHIP,
    ) -> Result<BTreeSet<LogicalCpuId>, CharacteristicsError> {
        // Windows 10 1703 and later report GroupCount masks; earlier records
        // carry the single GroupMask in the same union slot.
        let group_count = usize::from(cache.GroupCount).max(1);
        let masks_offset = offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, Anonymous)
            + offset_of!(CACHE_RELATIONSHIP, Anonymous);
        let required = masks_offset
            .checked_add(group_count.saturating_mul(size_of::<GROUP_AFFINITY>()))
            .ok_or_else(|| malformed("group-mask size overflow"))?;
        if required > size {
            return Err(malformed(&format!(
                "cache record has {group_count} group masks but size {size}"
            )));
        }
        let masks = unsafe { bytes.add(offset + masks_offset).cast::<GROUP_AFFINITY>() };
        let mut cpus = BTreeSet::new();
        for index in 0..group_count {
            let affinity = unsafe { &*masks.add(index) };
            for bit in 0..usize::BITS {
                if affinity.Mask & (1_usize << bit) != 0 {
                    cpus.insert(LogicalCpuId {
                        group: affinity.Group,
                        number: bit,
                    });
                }
            }
        }
        Ok(cpus)
    }

    fn malformed(reason: &str) -> CharacteristicsError {
        CharacteristicsError::InvalidValue {
            path: PathBuf::from("GetLogicalProcessorInformationEx(RelationCache)"),
            value: reason.into(),
        }
    }
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
        let cpus = topology
            .cores()
            .ok_or(CharacteristicsError::TopologyIdentityUnavailable)?
            .iter()
            .flat_map(|core| core.logical_cpus.iter().copied())
            .collect::<Vec<_>>();
        let caches = last_level_cache_domains(&cache_reports(root, &cpus)?);
        for cpu in cpus {
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
                    last_level_cache: caches.get(&cpu).copied(),
                },
            );
        }
        assemble(topology, CharacteristicsSource::LinuxSysfs, &observations)
    }

    /// Read every `cpuN/cache/index*` entry the kernel exports. The last level
    /// is whichever level is highest here, typically `index3` on parts with an
    /// L3; a host that exports no cache directory yields no reports at all.
    fn cache_reports(
        root: &Path,
        cpus: &[LogicalCpuId],
    ) -> Result<Vec<CacheReport>, CharacteristicsError> {
        let mut reports = Vec::new();
        for cpu in cpus {
            let cache_root = root.join(format!("cpu{}", cpu.number)).join("cache");
            let entries = match fs::read_dir(&cache_root) {
                Ok(entries) => entries,
                Err(source) if source.kind() == std::io::ErrorKind::NotFound => continue,
                Err(source) => {
                    return Err(CharacteristicsError::Io {
                        path: cache_root,
                        source,
                    });
                }
            };
            for entry in entries.filter_map(Result::ok) {
                let index_root = entry.path();
                if !entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("index"))
                {
                    continue;
                }
                let Some(kind) = read_optional(&index_root.join("type"))? else {
                    continue;
                };
                // An instruction or trace cache says nothing about data locality.
                if !matches!(kind.trim(), "Unified" | "Data") {
                    continue;
                }
                let (Some(level), Some(shared)) = (
                    read_optional(&index_root.join("level"))?,
                    read_optional(&index_root.join("shared_cpu_list"))?,
                ) else {
                    continue;
                };
                let level_path = index_root.join("level");
                let level = u8::try_from(parse_u32(&level_path, &level)?).map_err(|_| {
                    CharacteristicsError::InvalidValue {
                        path: level_path,
                        value: level.clone(),
                    }
                })?;
                reports.push(CacheReport {
                    level,
                    shared_cpus: parse_cpu_list(&index_root.join("shared_cpu_list"), &shared)?,
                });
            }
        }
        Ok(reports)
    }

    fn read_optional(path: &Path) -> Result<Option<String>, CharacteristicsError> {
        match fs::read_to_string(path) {
            Ok(value) => Ok(Some(value)),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(source) => Err(CharacteristicsError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    fn parse_cpu_list(
        path: &Path,
        value: &str,
    ) -> Result<BTreeSet<LogicalCpuId>, CharacteristicsError> {
        let mut cpus = BTreeSet::new();
        for component in value.trim().split(',').filter(|part| !part.is_empty()) {
            let (start, end) = match component.split_once('-') {
                Some((start, end)) => (parse_u32(path, start)?, parse_u32(path, end)?),
                None => {
                    let number = parse_u32(path, component)?;
                    (number, number)
                }
            };
            if start > end || end - start > 1_000_000 {
                return Err(CharacteristicsError::InvalidValue {
                    path: path.to_path_buf(),
                    value: value.into(),
                });
            }
            cpus.extend((start..=end).map(|number| LogicalCpuId { group: 0, number }));
        }
        Ok(cpus)
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
                        last_level_cache: None,
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
                        last_level_cache: None,
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

    fn cpus(numbers: &[u32]) -> BTreeSet<LogicalCpuId> {
        numbers.iter().copied().map(LogicalCpuId::from).collect()
    }

    #[test]
    fn last_level_domains_index_by_lowest_member_and_ignore_lower_levels() {
        // One chiplet part: two L3 domains over four cores, each with its own
        // L2 pair. Only the last level decides the domain.
        let reports = vec![
            CacheReport {
                level: 2,
                shared_cpus: cpus(&[0, 1]),
            },
            CacheReport {
                level: 2,
                shared_cpus: cpus(&[2, 3]),
            },
            CacheReport {
                level: 3,
                shared_cpus: cpus(&[2, 3]),
            },
            CacheReport {
                level: 3,
                shared_cpus: cpus(&[0, 1]),
            },
        ];
        let domains = last_level_cache_domains(&reports);
        let identity = |number: u32| domains[&LogicalCpuId::from(number)];
        assert_eq!(identity(0), identity(1));
        assert_eq!(identity(2), identity(3));
        assert_ne!(identity(0), identity(2));
        // Index order follows the lowest member CPU, not report order.
        assert_eq!(identity(0), CacheDomainId { level: 3, index: 0 });
        assert_eq!(identity(2), CacheDomainId { level: 3, index: 1 });
    }

    #[test]
    fn a_cpu_with_no_cache_report_is_left_without_a_domain() {
        let reports = vec![CacheReport {
            level: 3,
            shared_cpus: cpus(&[0, 1]),
        }];
        let domains = last_level_cache_domains(&reports);
        assert!(domains.contains_key(&LogicalCpuId::from(0)));
        assert!(!domains.contains_key(&LogicalCpuId::from(2)));
    }

    #[test]
    fn a_host_reporting_one_shared_last_level_yields_one_domain() {
        let reports = vec![CacheReport {
            level: 3,
            shared_cpus: cpus(&[0, 1, 2, 3]),
        }];
        let domains = last_level_cache_domains(&reports);
        assert_eq!(domains.values().copied().collect::<BTreeSet<_>>().len(), 1);
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn host_characteristics_cover_detected_topology() {
        let topology = crate::topology::detect_cpu_topology().unwrap();
        let characteristics = detect_cpu_characteristics(&topology).unwrap();
        assert_eq!(characteristics.cores.len(), topology.physical_core_count);
    }
}
