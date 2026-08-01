//! OS-reported physical-core and SMT-sibling topology.
//!
//! Logical processor numbering is identity only. It is never used to infer
//! which processors share a physical core.

use std::collections::BTreeSet;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::path::PathBuf;

pub use colosseum_application::LogicalCpuId;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhysicalCore {
    /// Exact logical processors reported as siblings by the operating system.
    pub logical_cpus: Vec<LogicalCpuId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum SiblingMapping {
    Known { cores: Vec<PhysicalCore> },
    Unavailable { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TopologySource {
    WindowsLogicalProcessorInformation,
    LinuxThreadSiblingsList,
    MacOsSysctlCounts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuTopology {
    pub source: TopologySource,
    pub physical_core_count: usize,
    pub logical_cpu_count: usize,
    pub sibling_mapping: SiblingMapping,
}

impl CpuTopology {
    fn from_known(
        source: TopologySource,
        mut cores: Vec<PhysicalCore>,
    ) -> Result<Self, TopologyError> {
        if cores.is_empty() {
            return Err(TopologyError::EmptyTopology);
        }
        for core in &mut cores {
            core.logical_cpus.sort_unstable();
            core.logical_cpus.dedup();
            if core.logical_cpus.is_empty() {
                return Err(TopologyError::CoreWithoutLogicalCpu);
            }
        }
        cores.sort_by_key(|core| core.logical_cpus[0]);
        let mut logical_cpus = BTreeSet::new();
        for core in &cores {
            for cpu in &core.logical_cpus {
                if !logical_cpus.insert(*cpu) {
                    return Err(TopologyError::LogicalCpuInMultipleCores(*cpu));
                }
            }
        }
        Ok(Self {
            source,
            physical_core_count: cores.len(),
            logical_cpu_count: logical_cpus.len(),
            sibling_mapping: SiblingMapping::Known { cores },
        })
    }

    #[cfg(any(target_os = "macos", test))]
    fn from_counts(
        source: TopologySource,
        physical_core_count: usize,
        logical_cpu_count: usize,
        reason: impl Into<String>,
    ) -> Result<Self, TopologyError> {
        if physical_core_count == 0
            || logical_cpu_count == 0
            || logical_cpu_count < physical_core_count
        {
            return Err(TopologyError::InvalidCounts {
                physical: physical_core_count,
                logical: logical_cpu_count,
            });
        }
        Ok(Self {
            source,
            physical_core_count,
            logical_cpu_count,
            sibling_mapping: SiblingMapping::Unavailable {
                reason: reason.into(),
            },
        })
    }

    #[must_use]
    pub fn cores(&self) -> Option<&[PhysicalCore]> {
        match &self.sibling_mapping {
            SiblingMapping::Known { cores } => Some(cores),
            SiblingMapping::Unavailable { .. } => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum TopologyError {
    #[error("CPU topology contains no physical cores")]
    EmptyTopology,
    #[error("physical core contains no logical processor")]
    CoreWithoutLogicalCpu,
    #[error("logical CPU {0:?} is reported in more than one physical core")]
    LogicalCpuInMultipleCores(LogicalCpuId),
    #[error("invalid CPU counts: {physical} physical, {logical} logical")]
    InvalidCounts { physical: usize, logical: usize },
    #[error("could not enumerate CPU topology at {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid Linux sibling list {value:?} for CPU {cpu}: {reason}")]
    InvalidLinuxSiblingList {
        cpu: u32,
        value: String,
        reason: String,
    },
    #[error("inconsistent Linux sibling reports for logical CPU {cpu}")]
    InconsistentLinuxSiblings { cpu: u32 },
    #[error("Windows topology record is malformed: {0}")]
    MalformedWindowsRecord(String),
    #[error("Windows topology query failed: {0}")]
    Windows(#[source] std::io::Error),
    #[error("macOS sysctl {name} failed: {source}")]
    MacOsSysctl {
        name: &'static str,
        source: std::io::Error,
    },
    #[error("CPU topology detection is unsupported on {0}")]
    UnsupportedPlatform(&'static str),
}

/// Detect the host topology through the platform's authoritative interface.
pub fn detect_cpu_topology() -> Result<CpuTopology, TopologyError> {
    detect_platform()
}

#[cfg(windows)]
fn detect_platform() -> Result<CpuTopology, TopologyError> {
    windows::detect()
}

#[cfg(target_os = "linux")]
fn detect_platform() -> Result<CpuTopology, TopologyError> {
    linux::detect(Path::new("/sys/devices/system/cpu"))
}

#[cfg(target_os = "macos")]
fn detect_platform() -> Result<CpuTopology, TopologyError> {
    macos::detect()
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn detect_platform() -> Result<CpuTopology, TopologyError> {
    Err(TopologyError::UnsupportedPlatform(std::env::consts::OS))
}

#[cfg(windows)]
mod windows {
    use std::mem::{offset_of, size_of};

    use windows_sys::Win32::System::SystemInformation::{
        GROUP_AFFINITY, GetLogicalProcessorInformationEx, PROCESSOR_RELATIONSHIP,
        RelationProcessorCore, SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX,
    };

    use super::*;

    pub(super) fn detect() -> Result<CpuTopology, TopologyError> {
        let mut byte_length = 0_u32;
        // The first call obtains the required variable-length buffer size.
        unsafe {
            GetLogicalProcessorInformationEx(
                RelationProcessorCore,
                std::ptr::null_mut(),
                &mut byte_length,
            );
        }
        if byte_length == 0 {
            return Err(TopologyError::Windows(std::io::Error::last_os_error()));
        }
        let words = (byte_length as usize).div_ceil(size_of::<usize>());
        let mut buffer = vec![0_usize; words];
        let mut returned = byte_length;
        let success = unsafe {
            GetLogicalProcessorInformationEx(
                RelationProcessorCore,
                buffer.as_mut_ptr().cast(),
                &mut returned,
            )
        };
        if success == 0 {
            return Err(TopologyError::Windows(std::io::Error::last_os_error()));
        }
        parse_buffer(buffer.as_ptr().cast(), returned as usize)
    }

    fn parse_buffer(bytes: *const u8, length: usize) -> Result<CpuTopology, TopologyError> {
        let mut offset = 0;
        let mut cores = Vec::new();
        while offset < length {
            if length - offset < 8 {
                return Err(TopologyError::MalformedWindowsRecord(
                    "truncated record header".into(),
                ));
            }
            let record = unsafe {
                &*bytes
                    .add(offset)
                    .cast::<SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX>()
            };
            let size = record.Size as usize;
            if size < 8 || size > length - offset {
                return Err(TopologyError::MalformedWindowsRecord(format!(
                    "invalid record size {size}"
                )));
            }
            if record.Relationship != RelationProcessorCore {
                return Err(TopologyError::MalformedWindowsRecord(format!(
                    "unexpected relationship {}",
                    record.Relationship
                )));
            }
            let processor = unsafe { &record.Anonymous.Processor };
            let group_count = usize::from(processor.GroupCount);
            let masks_offset = offset_of!(SYSTEM_LOGICAL_PROCESSOR_INFORMATION_EX, Anonymous)
                + offset_of!(PROCESSOR_RELATIONSHIP, GroupMask);
            let required = masks_offset
                .checked_add(group_count.saturating_mul(size_of::<GROUP_AFFINITY>()))
                .ok_or_else(|| {
                    TopologyError::MalformedWindowsRecord("group-mask size overflow".into())
                })?;
            if group_count == 0 || required > size {
                return Err(TopologyError::MalformedWindowsRecord(format!(
                    "record has {group_count} group masks but size {size}"
                )));
            }
            let masks = unsafe { bytes.add(offset + masks_offset).cast::<GROUP_AFFINITY>() };
            let mut logical_cpus = Vec::new();
            for index in 0..group_count {
                let affinity = unsafe { &*masks.add(index) };
                for bit in 0..usize::BITS {
                    if affinity.Mask & (1_usize << bit) != 0 {
                        logical_cpus.push(LogicalCpuId {
                            group: affinity.Group,
                            number: bit,
                        });
                    }
                }
            }
            cores.push(PhysicalCore { logical_cpus });
            offset += size;
        }
        CpuTopology::from_known(TopologySource::WindowsLogicalProcessorInformation, cores)
    }
}

#[cfg(any(target_os = "linux", test))]
mod linux {
    use std::collections::{BTreeMap, BTreeSet};
    #[cfg(target_os = "linux")]
    use std::fs;

    use super::*;

    #[cfg(target_os = "linux")]
    pub(super) fn detect(root: &Path) -> Result<CpuTopology, TopologyError> {
        let entries = fs::read_dir(root).map_err(|source| TopologyError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let mut reports = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|source| TopologyError::Io {
                path: root.to_path_buf(),
                source,
            })?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some(number) = name.strip_prefix("cpu") else {
                continue;
            };
            let Ok(cpu) = number.parse::<u32>() else {
                continue;
            };
            let path = entry.path().join("topology/thread_siblings_list");
            let value = fs::read_to_string(&path).map_err(|source| TopologyError::Io {
                path: path.clone(),
                source,
            })?;
            reports.push((cpu, value));
        }
        from_reports(reports)
    }

    pub(super) fn from_reports(
        reports: impl IntoIterator<Item = (u32, String)>,
    ) -> Result<CpuTopology, TopologyError> {
        let mut by_cpu = BTreeMap::new();
        let mut unique = BTreeSet::new();
        for (cpu, value) in reports {
            let siblings = parse_list(cpu, &value)?;
            if !siblings.contains(&cpu) {
                return Err(TopologyError::InvalidLinuxSiblingList {
                    cpu,
                    value,
                    reason: "the reporting CPU is absent from its sibling set".into(),
                });
            }
            if let Some(previous) = by_cpu.insert(cpu, siblings.clone())
                && previous != siblings
            {
                return Err(TopologyError::InconsistentLinuxSiblings { cpu });
            }
            unique.insert(siblings);
        }
        for siblings in &unique {
            for cpu in siblings {
                if let Some(report) = by_cpu.get(cpu)
                    && report != siblings
                {
                    return Err(TopologyError::InconsistentLinuxSiblings { cpu: *cpu });
                }
            }
        }
        let cores = unique
            .into_iter()
            .map(|siblings| PhysicalCore {
                logical_cpus: siblings
                    .into_iter()
                    .map(|number| LogicalCpuId { group: 0, number })
                    .collect(),
            })
            .collect();
        CpuTopology::from_known(TopologySource::LinuxThreadSiblingsList, cores)
    }

    fn parse_list(cpu: u32, value: &str) -> Result<BTreeSet<u32>, TopologyError> {
        let value = value.trim();
        let mut output = BTreeSet::new();
        if value.is_empty() {
            return invalid(cpu, value, "empty list");
        }
        for component in value.split(',') {
            let (start, end) = if let Some((start, end)) = component.split_once('-') {
                let start = parse_cpu(cpu, value, start)?;
                let end = parse_cpu(cpu, value, end)?;
                if start > end {
                    return invalid(cpu, value, "descending range");
                }
                if end - start > 1_000_000 {
                    return invalid(cpu, value, "excessive range");
                }
                (start, end)
            } else {
                let number = parse_cpu(cpu, value, component)?;
                (number, number)
            };
            output.extend(start..=end);
        }
        Ok(output)
    }

    fn parse_cpu(cpu: u32, whole: &str, value: &str) -> Result<u32, TopologyError> {
        value
            .parse()
            .map_err(|_| TopologyError::InvalidLinuxSiblingList {
                cpu,
                value: whole.into(),
                reason: format!("invalid logical CPU {value:?}"),
            })
    }

    fn invalid<T>(cpu: u32, value: &str, reason: &str) -> Result<T, TopologyError> {
        Err(TopologyError::InvalidLinuxSiblingList {
            cpu,
            value: value.into(),
            reason: reason.into(),
        })
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use std::ffi::CString;
    use std::mem::size_of;

    use super::*;

    const REASON: &str = "macOS sysctl reports physical/logical counts but does not expose a public logical-ID sibling map";

    pub(super) fn detect() -> Result<CpuTopology, TopologyError> {
        from_counts(
            usize::try_from(sysctl_u32("hw.physicalcpu")?).unwrap_or(usize::MAX),
            usize::try_from(sysctl_u32("hw.logicalcpu")?).unwrap_or(usize::MAX),
        )
    }

    fn from_counts(physical: usize, logical: usize) -> Result<CpuTopology, TopologyError> {
        CpuTopology::from_counts(TopologySource::MacOsSysctlCounts, physical, logical, REASON)
    }

    fn sysctl_u32(name: &'static str) -> Result<u32, TopologyError> {
        let name_c = CString::new(name).expect("static sysctl name contains no nul");
        let mut value = 0_u32;
        let mut length = size_of::<u32>();
        let result = unsafe {
            libc::sysctlbyname(
                name_c.as_ptr(),
                std::ptr::addr_of_mut!(value).cast(),
                &mut length,
                std::ptr::null_mut(),
                0,
            )
        };
        if result != 0 || length != size_of::<u32>() {
            return Err(TopologyError::MacOsSysctl {
                name,
                source: std::io::Error::last_os_error(),
            });
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_smt_fixture_uses_reported_non_adjacent_siblings() {
        let reports = (0..32).map(|cpu| {
            let sibling = if cpu < 16 { cpu + 16 } else { cpu - 16 };
            (cpu, format!("{},{}", cpu.min(sibling), cpu.max(sibling)))
        });
        let topology = linux::from_reports(reports).unwrap();
        assert_eq!(topology.physical_core_count, 16);
        assert_eq!(topology.logical_cpu_count, 32);
        let cores = topology.cores().unwrap();
        assert_eq!(
            cores[0].logical_cpus,
            [
                LogicalCpuId {
                    group: 0,
                    number: 0
                },
                LogicalCpuId {
                    group: 0,
                    number: 16
                }
            ]
        );
    }

    #[test]
    fn linux_no_smt_fixture_keeps_one_logical_cpu_per_core() {
        let topology = linux::from_reports((0..8).map(|cpu| (cpu, cpu.to_string()))).unwrap();
        assert_eq!(topology.physical_core_count, 8);
        assert_eq!(topology.logical_cpu_count, 8);
        assert!(
            topology
                .cores()
                .unwrap()
                .iter()
                .all(|core| core.logical_cpus.len() == 1)
        );
    }

    #[test]
    fn linux_rejects_overlapping_or_inconsistent_sibling_sets() {
        let error = linux::from_reports([
            (0, "0,7".into()),
            (7, "7".into()),
            (2, "2,5".into()),
            (5, "2,5".into()),
        ])
        .unwrap_err();
        assert!(matches!(
            error,
            TopologyError::InconsistentLinuxSiblings { cpu: 7 }
        ));
    }

    #[test]
    fn known_topology_rejects_one_logical_cpu_in_two_cores() {
        let cpu = LogicalCpuId {
            group: 0,
            number: 3,
        };
        let error = CpuTopology::from_known(
            TopologySource::LinuxThreadSiblingsList,
            vec![
                PhysicalCore {
                    logical_cpus: vec![cpu],
                },
                PhysicalCore {
                    logical_cpus: vec![cpu],
                },
            ],
        )
        .unwrap_err();
        assert!(matches!(
            error,
            TopologyError::LogicalCpuInMultipleCores(id) if id == cpu
        ));
    }

    #[test]
    fn windows_group_qualified_ids_do_not_collapse_equal_cpu_numbers() {
        let topology = CpuTopology::from_known(
            TopologySource::WindowsLogicalProcessorInformation,
            vec![
                PhysicalCore {
                    logical_cpus: vec![
                        LogicalCpuId {
                            group: 0,
                            number: 0,
                        },
                        LogicalCpuId {
                            group: 0,
                            number: 9,
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
                            number: 9,
                        },
                    ],
                },
            ],
        )
        .unwrap();
        assert_eq!(topology.physical_core_count, 2);
        assert_eq!(topology.logical_cpu_count, 4);
    }

    #[test]
    fn macos_count_fixture_does_not_invent_sibling_ids() {
        let topology = CpuTopology::from_counts(
            TopologySource::MacOsSysctlCounts,
            10,
            10,
            "public API exposes counts only",
        )
        .unwrap();
        assert_eq!(topology.physical_core_count, 10);
        assert_eq!(topology.logical_cpu_count, 10);
        assert!(topology.cores().is_none());
        assert!(matches!(
            topology.sibling_mapping,
            SiblingMapping::Unavailable { .. }
        ));
    }

    #[test]
    fn invalid_count_fixture_is_rejected() {
        assert!(matches!(
            CpuTopology::from_counts(TopologySource::MacOsSysctlCounts, 8, 4, "invalid"),
            Err(TopologyError::InvalidCounts {
                physical: 8,
                logical: 4
            })
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_host_reports_exact_group_qualified_sibling_sets() {
        let topology = detect_cpu_topology().unwrap();
        assert!(topology.physical_core_count > 0);
        assert!(topology.logical_cpu_count >= topology.physical_core_count);
        assert_eq!(
            topology.source,
            TopologySource::WindowsLogicalProcessorInformation
        );
        assert!(topology.cores().is_some());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_host_reads_sysfs_sibling_sets() {
        let topology = detect_cpu_topology().unwrap();
        assert!(topology.physical_core_count > 0);
        assert!(topology.logical_cpu_count >= topology.physical_core_count);
        assert_eq!(topology.source, TopologySource::LinuxThreadSiblingsList);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_host_reports_counts_without_fabricated_mapping() {
        let topology = detect_cpu_topology().unwrap();
        assert!(topology.physical_core_count > 0);
        assert!(topology.logical_cpu_count >= topology.physical_core_count);
        assert!(topology.cores().is_none());
    }
}
