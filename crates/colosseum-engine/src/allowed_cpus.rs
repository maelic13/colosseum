//! Current-process CPU availability adapters.
//!
//! Topology describes the machine. Availability describes the subset the
//! operating system currently permits the calling process/thread to use.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::topology::{CpuTopology, LogicalCpuId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AllowedCpuSource {
    LinuxSchedulerAffinity,
    WindowsProcessAffinity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum AllowedCpuSet {
    Known {
        source: AllowedCpuSource,
        cpus: Vec<LogicalCpuId>,
    },
    Unavailable {
        reason: String,
    },
}

impl AllowedCpuSet {
    #[must_use]
    pub fn cpus(&self) -> Option<&[LogicalCpuId]> {
        match self {
            Self::Known { cpus, .. } => Some(cpus),
            Self::Unavailable { .. } => None,
        }
    }

    fn known(
        topology: &CpuTopology,
        source: AllowedCpuSource,
        cpus: impl IntoIterator<Item = LogicalCpuId>,
    ) -> Result<Self, AllowedCpuError> {
        let known = topology
            .cores()
            .ok_or(AllowedCpuError::TopologyIdentityUnavailable)?
            .iter()
            .flat_map(|core| core.logical_cpus.iter().copied())
            .collect::<BTreeSet<_>>();
        let cpus = cpus.into_iter().collect::<BTreeSet<_>>();
        if cpus.is_empty() {
            return Err(AllowedCpuError::EmptyAllowedSet);
        }
        if let Some(cpu) = cpus.iter().find(|cpu| !known.contains(cpu)) {
            return Err(AllowedCpuError::CpuMissingFromTopology(*cpu));
        }
        Ok(Self::Known {
            source,
            cpus: cpus.into_iter().collect(),
        })
    }
}

#[derive(Debug, Error)]
pub enum AllowedCpuError {
    #[error("logical CPU identities are unavailable for this topology")]
    TopologyIdentityUnavailable,
    #[error("the operating system reports an empty allowed CPU set")]
    EmptyAllowedSet,
    #[error("allowed logical CPU {0:?} is absent from the detected topology")]
    CpuMissingFromTopology(LogicalCpuId),
    #[error("{operation} failed: {source}")]
    Platform {
        operation: &'static str,
        source: std::io::Error,
    },
    #[error("Windows returned malformed CPU-set information: {0}")]
    MalformedWindowsCpuSet(String),
    #[error("CPU availability detection is unsupported on {0}")]
    UnsupportedPlatform(&'static str),
}

/// Detect the CPUs available to the calling process/thread and validate them
/// against the previously detected machine topology.
pub fn detect_allowed_cpu_set(topology: &CpuTopology) -> Result<AllowedCpuSet, AllowedCpuError> {
    detect_platform(topology)
}

#[cfg(target_os = "linux")]
fn detect_platform(topology: &CpuTopology) -> Result<AllowedCpuSet, AllowedCpuError> {
    linux::detect(topology)
}

#[cfg(windows)]
fn detect_platform(topology: &CpuTopology) -> Result<AllowedCpuSet, AllowedCpuError> {
    windows::detect(topology)
}

#[cfg(target_os = "macos")]
fn detect_platform(_topology: &CpuTopology) -> Result<AllowedCpuSet, AllowedCpuError> {
    Ok(AllowedCpuSet::Unavailable {
        reason:
            "macOS does not expose the logical CPU identities needed to describe an allowed set"
                .into(),
    })
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn detect_platform(_topology: &CpuTopology) -> Result<AllowedCpuSet, AllowedCpuError> {
    Err(AllowedCpuError::UnsupportedPlatform(std::env::consts::OS))
}

#[cfg(any(target_os = "linux", test))]
mod linux {
    #[cfg(target_os = "linux")]
    use std::mem::size_of;

    use super::*;

    #[cfg(target_os = "linux")]
    const MAX_AFFINITY_BYTES: usize = 1024 * 1024;

    #[cfg(target_os = "linux")]
    pub(super) fn detect(topology: &CpuTopology) -> Result<AllowedCpuSet, AllowedCpuError> {
        let highest_cpu = topology
            .cores()
            .ok_or(AllowedCpuError::TopologyIdentityUnavailable)?
            .iter()
            .flat_map(|core| core.logical_cpus.iter())
            .map(|cpu| usize::try_from(cpu.number).unwrap_or(usize::MAX))
            .max()
            .ok_or(AllowedCpuError::EmptyAllowedSet)?;
        let minimum_bytes = highest_cpu
            .checked_div(8)
            .and_then(|bytes| bytes.checked_add(1))
            .ok_or(AllowedCpuError::Platform {
                operation: "size Linux scheduler affinity mask",
                source: std::io::Error::new(std::io::ErrorKind::InvalidData, "CPU ID overflow"),
            })?;
        let mut words = minimum_bytes
            .max(size_of::<libc::cpu_set_t>())
            .div_ceil(size_of::<usize>());

        loop {
            let mut mask = vec![0_usize; words];
            let bytes = mask.len() * size_of::<usize>();
            let result = unsafe {
                libc::sched_getaffinity(0, bytes, mask.as_mut_ptr().cast::<libc::cpu_set_t>())
            };
            if result == 0 {
                return from_words(topology, &mask);
            }
            let source = std::io::Error::last_os_error();
            if source.raw_os_error() == Some(libc::EINVAL) && bytes < MAX_AFFINITY_BYTES {
                words = words.saturating_mul(2);
                continue;
            }
            return Err(AllowedCpuError::Platform {
                operation: "sched_getaffinity",
                source,
            });
        }
    }

    pub(super) fn from_words(
        topology: &CpuTopology,
        words: &[usize],
    ) -> Result<AllowedCpuSet, AllowedCpuError> {
        let cpus = words.iter().enumerate().flat_map(|(word_index, word)| {
            (0..usize::BITS).filter_map(move |bit| {
                if word & (1_usize << bit) == 0 {
                    return None;
                }
                Some(LogicalCpuId {
                    group: 0,
                    number: u32::try_from(word_index * usize::BITS as usize + bit as usize).ok()?,
                })
            })
        });
        AllowedCpuSet::known(topology, AllowedCpuSource::LinuxSchedulerAffinity, cpus)
    }
}

#[cfg(any(windows, test))]
pub(crate) mod windows {
    use std::collections::{BTreeMap, BTreeSet};
    #[cfg(windows)]
    use std::mem::{size_of, size_of_val};

    use super::*;

    #[derive(Debug, Clone, Copy)]
    pub(crate) struct CpuSetEntry {
        pub(crate) id: u32,
        pub(crate) cpu: LogicalCpuId,
        pub(crate) efficiency_class: u8,
        pub(crate) numa_node_index: u8,
        pub(crate) allocated: bool,
        pub(crate) allocated_to_process: bool,
    }

    #[cfg(windows)]
    pub(super) fn detect(topology: &CpuTopology) -> Result<AllowedCpuSet, AllowedCpuError> {
        use windows_sys::Win32::System::Threading::{
            GetCurrentProcess, GetProcessAffinityMask, GetProcessDefaultCpuSets,
            GetProcessGroupAffinity,
        };

        let process = unsafe { GetCurrentProcess() };
        let groups = query_u16_list("GetProcessGroupAffinity", |buffer, count| unsafe {
            GetProcessGroupAffinity(process, count, buffer)
        })?;
        let default_cpu_set_ids = query_u32_list(
            "GetProcessDefaultCpuSets",
            |buffer, count, required| unsafe {
                GetProcessDefaultCpuSets(process, buffer, count, required)
            },
        )?;
        let entries = query_cpu_set_entries(process)?;
        let process_mask = if groups.len() == 1 {
            let mut process_mask = 0_usize;
            let mut system_mask = 0_usize;
            let success =
                unsafe { GetProcessAffinityMask(process, &mut process_mask, &mut system_mask) };
            if success == 0 {
                return Err(platform_error("GetProcessAffinityMask"));
            }
            Some((groups[0], process_mask))
        } else {
            None
        };
        resolve(
            topology,
            &groups,
            process_mask,
            &default_cpu_set_ids,
            &entries,
        )
    }

    fn resolve(
        topology: &CpuTopology,
        groups: &[u16],
        process_mask: Option<(u16, usize)>,
        default_cpu_set_ids: &[u32],
        entries: &[CpuSetEntry],
    ) -> Result<AllowedCpuSet, AllowedCpuError> {
        let group_set = groups.iter().copied().collect::<BTreeSet<_>>();
        let by_cpu = entries
            .iter()
            .map(|entry| (entry.cpu, entry))
            .collect::<BTreeMap<_, _>>();
        let default_ids = default_cpu_set_ids.iter().copied().collect::<BTreeSet<_>>();
        let cpus = topology
            .cores()
            .ok_or(AllowedCpuError::TopologyIdentityUnavailable)?
            .iter()
            .flat_map(|core| core.logical_cpus.iter().copied())
            .filter(|cpu| group_set.contains(&cpu.group))
            .filter(|cpu| {
                process_mask.is_none_or(|(group, mask)| {
                    cpu.group == group
                        && cpu.number < usize::BITS
                        && mask & (1_usize << cpu.number) != 0
                })
            })
            .filter(|cpu| {
                by_cpu
                    .get(cpu)
                    .is_none_or(|entry| !entry.allocated || entry.allocated_to_process)
            })
            .filter(|cpu| {
                default_ids.is_empty()
                    || by_cpu
                        .get(cpu)
                        .is_some_and(|entry| default_ids.contains(&entry.id))
            });
        AllowedCpuSet::known(topology, AllowedCpuSource::WindowsProcessAffinity, cpus)
    }

    #[cfg(windows)]
    fn query_u16_list(
        operation: &'static str,
        query: impl Fn(*mut u16, *mut u16) -> i32,
    ) -> Result<Vec<u16>, AllowedCpuError> {
        let mut required = 0_u16;
        let first = query(std::ptr::null_mut(), &mut required);
        if required == 0 {
            if first == 0 {
                return Err(platform_error(operation));
            }
            return Err(AllowedCpuError::EmptyAllowedSet);
        }
        let mut values = vec![0_u16; usize::from(required)];
        let success = query(values.as_mut_ptr(), &mut required);
        if success == 0 {
            return Err(platform_error(operation));
        }
        values.truncate(usize::from(required));
        Ok(values)
    }

    #[cfg(windows)]
    fn query_u32_list(
        operation: &'static str,
        query: impl Fn(*mut u32, u32, *mut u32) -> i32,
    ) -> Result<Vec<u32>, AllowedCpuError> {
        let mut required = 0_u32;
        let first = query(std::ptr::null_mut(), 0, &mut required);
        if required == 0 {
            if first == 0 {
                return Err(platform_error(operation));
            }
            return Ok(Vec::new());
        }
        let mut values = vec![0_u32; required as usize];
        let success = query(values.as_mut_ptr(), values.len() as u32, &mut required);
        if success == 0 {
            return Err(platform_error(operation));
        }
        values.truncate(required as usize);
        Ok(values)
    }

    #[cfg(windows)]
    pub(crate) fn query_cpu_set_entries(
        process: windows_sys::Win32::Foundation::HANDLE,
    ) -> Result<Vec<CpuSetEntry>, AllowedCpuError> {
        use windows_sys::Win32::System::SystemInformation::{
            CpuSetInformation, GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION,
            SYSTEM_CPU_SET_INFORMATION_ALLOCATED,
            SYSTEM_CPU_SET_INFORMATION_ALLOCATED_TO_TARGET_PROCESS,
        };

        let mut required = 0_u32;
        unsafe {
            GetSystemCpuSetInformation(std::ptr::null_mut(), 0, &mut required, process, 0);
        }
        if required == 0 {
            return Ok(Vec::new());
        }
        let words = (required as usize).div_ceil(size_of::<usize>());
        let mut buffer = vec![0_usize; words];
        let mut returned = required;
        let success = unsafe {
            GetSystemCpuSetInformation(
                buffer.as_mut_ptr().cast(),
                u32::try_from(size_of_val(buffer.as_slice())).unwrap_or(u32::MAX),
                &mut returned,
                process,
                0,
            )
        };
        if success == 0 {
            return Err(platform_error("GetSystemCpuSetInformation"));
        }

        let mut entries = Vec::new();
        let mut offset = 0_usize;
        while offset < returned as usize {
            if returned as usize - offset < 8 {
                return Err(AllowedCpuError::MalformedWindowsCpuSet(
                    "truncated record header".into(),
                ));
            }
            let record = unsafe {
                &*buffer
                    .as_ptr()
                    .cast::<u8>()
                    .add(offset)
                    .cast::<SYSTEM_CPU_SET_INFORMATION>()
            };
            let size = record.Size as usize;
            if size < size_of::<SYSTEM_CPU_SET_INFORMATION>() || size > returned as usize - offset {
                return Err(AllowedCpuError::MalformedWindowsCpuSet(format!(
                    "invalid record size {size}"
                )));
            }
            if record.Type == CpuSetInformation {
                let cpu_set = unsafe { record.Anonymous.CpuSet };
                let flags = unsafe { cpu_set.Anonymous1.AllFlags };
                entries.push(CpuSetEntry {
                    id: cpu_set.Id,
                    cpu: LogicalCpuId {
                        group: cpu_set.Group,
                        number: u32::from(cpu_set.LogicalProcessorIndex),
                    },
                    efficiency_class: cpu_set.EfficiencyClass,
                    numa_node_index: cpu_set.NumaNodeIndex,
                    allocated: flags & SYSTEM_CPU_SET_INFORMATION_ALLOCATED as u8 != 0,
                    allocated_to_process: flags
                        & SYSTEM_CPU_SET_INFORMATION_ALLOCATED_TO_TARGET_PROCESS as u8
                        != 0,
                });
            }
            offset += size;
        }
        Ok(entries)
    }

    #[cfg(windows)]
    fn platform_error(operation: &'static str) -> AllowedCpuError {
        AllowedCpuError::Platform {
            operation,
            source: std::io::Error::last_os_error(),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::topology::{PhysicalCore, SiblingMapping, TopologySource};

        fn topology() -> CpuTopology {
            CpuTopology {
                source: TopologySource::WindowsLogicalProcessorInformation,
                physical_core_count: 4,
                logical_cpu_count: 8,
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
                                    group: 0,
                                    number: 2,
                                },
                                LogicalCpuId {
                                    group: 0,
                                    number: 3,
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
                        PhysicalCore {
                            logical_cpus: vec![
                                LogicalCpuId {
                                    group: 1,
                                    number: 2,
                                },
                                LogicalCpuId {
                                    group: 1,
                                    number: 3,
                                },
                            ],
                        },
                    ],
                },
            }
        }

        fn entries() -> Vec<CpuSetEntry> {
            topology()
                .cores()
                .unwrap()
                .iter()
                .flat_map(|core| core.logical_cpus.iter().copied())
                .enumerate()
                .map(|(id, cpu)| CpuSetEntry {
                    id: id as u32,
                    cpu,
                    efficiency_class: 0,
                    numa_node_index: 0,
                    allocated: false,
                    allocated_to_process: false,
                })
                .collect()
        }

        #[test]
        fn processor_groups_remain_qualified_and_process_mask_is_respected() {
            let allowed = resolve(&topology(), &[0], Some((0, 0b0101)), &[], &entries()).unwrap();
            assert_eq!(
                allowed.cpus().unwrap(),
                [
                    LogicalCpuId {
                        group: 0,
                        number: 0
                    },
                    LogicalCpuId {
                        group: 0,
                        number: 2
                    },
                ]
            );

            let allowed = resolve(&topology(), &[0, 1], None, &[], &entries()).unwrap();
            assert_eq!(allowed.cpus().unwrap().len(), 8);
            assert!(allowed.cpus().unwrap().contains(&LogicalCpuId {
                group: 1,
                number: 0
            }));
        }

        #[test]
        fn process_default_and_exclusive_cpu_sets_are_intersected() {
            let mut cpu_sets = entries();
            cpu_sets[1].allocated = true;
            cpu_sets[1].allocated_to_process = false;
            cpu_sets[6].allocated = true;
            cpu_sets[6].allocated_to_process = true;
            let allowed = resolve(&topology(), &[0, 1], None, &[1, 6], &cpu_sets).unwrap();
            assert_eq!(
                allowed.cpus().unwrap(),
                [LogicalCpuId {
                    group: 1,
                    number: 2
                }]
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::{PhysicalCore, SiblingMapping, TopologySource};

    fn linux_topology() -> CpuTopology {
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
    fn linux_mask_words_are_decoded_and_validated_against_topology() {
        let allowed = linux::from_words(&linux_topology(), &[0b1010]).unwrap();
        assert_eq!(
            allowed.cpus().unwrap(),
            [LogicalCpuId::from(1), LogicalCpuId::from(3)]
        );
    }

    #[test]
    fn availability_rejects_a_cpu_absent_from_topology() {
        let error = linux::from_words(&linux_topology(), &[1 << 7]).unwrap_err();
        assert!(matches!(
            error,
            AllowedCpuError::CpuMissingFromTopology(LogicalCpuId {
                group: 0,
                number: 7
            })
        ));
    }

    #[cfg(any(windows, target_os = "linux"))]
    #[test]
    fn host_allowed_set_is_nonempty_and_within_detected_topology() {
        let topology = crate::topology::detect_cpu_topology().unwrap();
        let allowed = detect_allowed_cpu_set(&topology).unwrap();
        assert!(!allowed.cpus().unwrap().is_empty());
    }
}
