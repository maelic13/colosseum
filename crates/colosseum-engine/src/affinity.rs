//! Child-process CPU-affinity application and capability reporting.
//!
//! Placement policy decides which CPUs an engine should receive. This module
//! is the outer OS adapter that either applies that request and verifies the
//! result, or returns a typed failure. It never silently degrades a hard
//! request to ordinary scheduling.

use std::collections::BTreeSet;

use colosseum_application::{CpuAllocation, LogicalCpuId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AffinitySupportLevel {
    Enforced,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AffinityCapability {
    pub level: AffinitySupportLevel,
    pub mechanism: Option<String>,
    pub constraints: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AffinityOutcome {
    Off,
    Enforced,
}

/// Durable evidence returned after an affinity request is handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedAffinity {
    pub process_id: u32,
    pub outcome: AffinityOutcome,
    pub cpus: Vec<LogicalCpuId>,
    pub mechanism: Option<String>,
}

#[derive(Debug, Error)]
pub enum AffinityError {
    #[error("hard CPU affinity is unavailable: {reason}")]
    Unavailable { reason: String },
    #[error("an enforced CPU allocation must contain at least one logical CPU")]
    EmptyCpuSet,
    #[error("advisory CPU allocation is not supported by this platform adapter")]
    AdvisoryUnsupported,
    #[error("CPU allocation contains logical CPU {0:?} more than once")]
    DuplicateCpu(LogicalCpuId),
    #[error("{platform} cannot apply one engine allocation across processor groups {groups:?}")]
    MultipleProcessorGroups {
        platform: &'static str,
        groups: Vec<u16>,
    },
    #[error(
        "the Windows hard-affinity adapter currently supports processor group zero, not group {0}"
    )]
    WindowsUnsupportedGroup(u16),
    #[error("Linux logical CPU identities must use group zero, found {0:?}")]
    LinuxNonzeroGroup(LogicalCpuId),
    #[error("{operation} failed: {source}")]
    Platform {
        operation: &'static str,
        source: std::io::Error,
    },
    #[error(
        "affinity verification for process {process_id} returned {actual:?}, expected {expected:?}"
    )]
    VerificationFailed {
        process_id: u32,
        expected: Vec<LogicalCpuId>,
        actual: Vec<LogicalCpuId>,
    },
    #[error("process {process_id} kept creating threads while affinity was being applied")]
    UnstableThreadSet { process_id: u32 },
}

/// Report the current platform's affinity implementation independently of a
/// particular CPU request.
#[must_use]
pub fn affinity_capability() -> AffinityCapability {
    platform::capability()
}

/// Apply a resolved allocation to an already spawned child process.
///
/// `Unrestricted` is an explicit, successful no-op and is returned as `off`.
/// An enforced request is canonicalized, applied, then read back. Failure at
/// any point is returned to the caller; it is never converted to advisory or
/// unrestricted placement.
pub fn apply_process_affinity(
    process_id: u32,
    allocation: &CpuAllocation,
) -> Result<AppliedAffinity, AffinityError> {
    match allocation {
        CpuAllocation::Unrestricted => Ok(AppliedAffinity {
            process_id,
            outcome: AffinityOutcome::Off,
            cpus: Vec::new(),
            mechanism: None,
        }),
        CpuAllocation::Advisory(_) => Err(AffinityError::AdvisoryUnsupported),
        CpuAllocation::Enforced(cpus) => {
            let cpus = validate_cpus(cpus)?;
            let capability = affinity_capability();
            if capability.level == AffinitySupportLevel::Unavailable {
                return Err(AffinityError::Unavailable {
                    reason: capability
                        .reason
                        .unwrap_or_else(|| "platform does not expose a supported mechanism".into()),
                });
            }
            platform::apply_and_verify(process_id, &cpus)?;
            Ok(AppliedAffinity {
                process_id,
                outcome: AffinityOutcome::Enforced,
                cpus,
                mechanism: capability.mechanism,
            })
        }
    }
}

fn validate_cpus(cpus: &[LogicalCpuId]) -> Result<Vec<LogicalCpuId>, AffinityError> {
    if cpus.is_empty() {
        return Err(AffinityError::EmptyCpuSet);
    }
    let mut unique = BTreeSet::new();
    for cpu in cpus {
        if !unique.insert(*cpu) {
            return Err(AffinityError::DuplicateCpu(*cpu));
        }
    }
    Ok(unique.into_iter().collect())
}

#[cfg(windows)]
mod platform {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Threading::{
        GetProcessAffinityMask, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_INFORMATION, SetProcessAffinityMask,
    };

    use super::*;

    pub(super) fn capability() -> AffinityCapability {
        AffinityCapability {
            level: AffinitySupportLevel::Enforced,
            mechanism: Some("SetProcessAffinityMask".into()),
            constraints: vec![
                "each engine allocation must fit Windows processor group zero".into(),
            ],
            reason: None,
        }
    }

    pub(super) fn apply_and_verify(
        process_id: u32,
        cpus: &[LogicalCpuId],
    ) -> Result<(), AffinityError> {
        let groups = cpus.iter().map(|cpu| cpu.group).collect::<BTreeSet<_>>();
        if groups.len() != 1 {
            return Err(AffinityError::MultipleProcessorGroups {
                platform: "Windows",
                groups: groups.into_iter().collect(),
            });
        }
        let group = *groups.first().expect("validated non-empty allocation");
        if group != 0 {
            return Err(AffinityError::WindowsUnsupportedGroup(group));
        }
        let mask = cpus.iter().try_fold(0_usize, |mask, cpu| {
            let bit = usize::try_from(cpu.number)
                .ok()
                .and_then(|number| (number < usize::BITS as usize).then_some(1_usize << number));
            bit.map(|bit| mask | bit)
                .ok_or_else(|| AffinityError::Platform {
                    operation: "build Windows process affinity mask",
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("logical CPU {} does not fit a group mask", cpu.number),
                    ),
                })
        })?;
        let handle = unsafe {
            OpenProcess(
                PROCESS_SET_INFORMATION | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                process_id,
            )
        };
        if handle.is_null() {
            return Err(last_error("OpenProcess"));
        }
        let handle = OwnedHandle(handle);
        if unsafe { SetProcessAffinityMask(handle.0, mask) } == 0 {
            return Err(last_error("SetProcessAffinityMask"));
        }
        let mut actual = 0_usize;
        let mut system = 0_usize;
        if unsafe { GetProcessAffinityMask(handle.0, &mut actual, &mut system) } == 0 {
            return Err(last_error("GetProcessAffinityMask"));
        }
        if actual != mask {
            return Err(AffinityError::VerificationFailed {
                process_id,
                expected: cpus.to_vec(),
                actual: mask_cpus(group, actual),
            });
        }
        Ok(())
    }

    fn mask_cpus(group: u16, mask: usize) -> Vec<LogicalCpuId> {
        (0..usize::BITS)
            .filter(|bit| mask & (1_usize << bit) != 0)
            .map(|number| LogicalCpuId { group, number })
            .collect()
    }

    fn last_error(operation: &'static str) -> AffinityError {
        AffinityError::Platform {
            operation,
            source: std::io::Error::last_os_error(),
        }
    }

    struct OwnedHandle(HANDLE);

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::fs;
    use std::mem::size_of;
    use std::path::PathBuf;

    use super::*;

    const MAX_THREAD_SCANS: usize = 8;

    pub(super) fn capability() -> AffinityCapability {
        AffinityCapability {
            level: AffinitySupportLevel::Enforced,
            mechanism: Some("sched_setaffinity".into()),
            constraints: Vec::new(),
            reason: None,
        }
    }

    pub(super) fn apply_and_verify(
        process_id: u32,
        cpus: &[LogicalCpuId],
    ) -> Result<(), AffinityError> {
        if let Some(cpu) = cpus.iter().find(|cpu| cpu.group != 0) {
            return Err(AffinityError::LinuxNonzeroGroup(*cpu));
        }
        let mask = cpu_mask(process_id, cpus)?;
        let mut prior = BTreeSet::new();
        for _ in 0..MAX_THREAD_SCANS {
            let tids = thread_ids(process_id)?;
            for tid in &tids {
                set_affinity(*tid, &mask)?;
            }
            let after = thread_ids(process_id)?;
            if after == tids && after == prior {
                for tid in after {
                    verify_affinity(process_id, tid, cpus, &mask)?;
                }
                return Ok(());
            }
            prior = after;
        }
        Err(AffinityError::UnstableThreadSet { process_id })
    }

    fn cpu_mask(process_id: u32, cpus: &[LogicalCpuId]) -> Result<Vec<usize>, AffinityError> {
        let highest = cpus.iter().map(|cpu| cpu.number as usize).max().unwrap();
        let bytes = (highest / 8 + 1).max(size_of::<libc::cpu_set_t>());
        let words = affinity_word_count(process_id, bytes.div_ceil(size_of::<usize>()))?;
        let mut mask = vec![0_usize; words];
        for cpu in cpus {
            let number = cpu.number as usize;
            mask[number / usize::BITS as usize] |= 1_usize << (number % usize::BITS as usize);
        }
        Ok(mask)
    }

    fn affinity_word_count(process_id: u32, minimum: usize) -> Result<usize, AffinityError> {
        let mut words = minimum;
        loop {
            let mut probe = vec![0_usize; words];
            let result = unsafe {
                libc::sched_getaffinity(
                    process_id as libc::pid_t,
                    std::mem::size_of_val(probe.as_slice()),
                    probe.as_mut_ptr().cast::<libc::cpu_set_t>(),
                )
            };
            if result == 0 {
                return Ok(words);
            }
            let source = std::io::Error::last_os_error();
            if source.raw_os_error() == Some(libc::EINVAL)
                && words
                    .checked_mul(2)
                    .is_some_and(|next| next * size_of::<usize>() <= 1024 * 1024)
            {
                words *= 2;
                continue;
            }
            return Err(AffinityError::Platform {
                operation: "probe sched_getaffinity mask size",
                source,
            });
        }
    }

    fn thread_ids(process_id: u32) -> Result<BTreeSet<libc::pid_t>, AffinityError> {
        let path = PathBuf::from(format!("/proc/{process_id}/task"));
        fs::read_dir(&path)
            .map_err(|source| AffinityError::Platform {
                operation: "read /proc process thread list",
                source,
            })?
            .map(|entry| {
                let entry = entry.map_err(|source| AffinityError::Platform {
                    operation: "read /proc process thread entry",
                    source,
                })?;
                entry
                    .file_name()
                    .to_string_lossy()
                    .parse::<libc::pid_t>()
                    .map_err(|source| AffinityError::Platform {
                        operation: "parse /proc thread ID",
                        source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
                    })
            })
            .collect()
    }

    fn set_affinity(tid: libc::pid_t, mask: &[usize]) -> Result<(), AffinityError> {
        let result = unsafe {
            libc::sched_setaffinity(
                tid,
                std::mem::size_of_val(mask),
                mask.as_ptr().cast::<libc::cpu_set_t>(),
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(AffinityError::Platform {
                operation: "sched_setaffinity",
                source: std::io::Error::last_os_error(),
            })
        }
    }

    fn verify_affinity(
        process_id: u32,
        tid: libc::pid_t,
        cpus: &[LogicalCpuId],
        expected_mask: &[usize],
    ) -> Result<(), AffinityError> {
        let mut actual = vec![0_usize; expected_mask.len()];
        let result = unsafe {
            libc::sched_getaffinity(
                tid,
                std::mem::size_of_val(actual.as_slice()),
                actual.as_mut_ptr().cast::<libc::cpu_set_t>(),
            )
        };
        if result != 0 {
            return Err(AffinityError::Platform {
                operation: "sched_getaffinity",
                source: std::io::Error::last_os_error(),
            });
        }
        if actual != expected_mask {
            return Err(AffinityError::VerificationFailed {
                process_id,
                expected: cpus.to_vec(),
                actual: mask_cpus(&actual),
            });
        }
        Ok(())
    }

    fn mask_cpus(mask: &[usize]) -> Vec<LogicalCpuId> {
        mask.iter()
            .enumerate()
            .flat_map(|(word, value)| {
                (0..usize::BITS).filter_map(move |bit| {
                    (value & (1_usize << bit) != 0).then_some(LogicalCpuId {
                        group: 0,
                        number: (word * usize::BITS as usize + bit as usize) as u32,
                    })
                })
            })
            .collect()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;

    pub(super) fn capability() -> AffinityCapability {
        AffinityCapability {
            level: AffinitySupportLevel::Unavailable,
            mechanism: None,
            constraints: Vec::new(),
            reason: Some(
                "macOS exposes affinity tags as scheduler hints, not hard logical-CPU pinning"
                    .into(),
            ),
        }
    }

    pub(super) fn apply_and_verify(
        _process_id: u32,
        _cpus: &[LogicalCpuId],
    ) -> Result<(), AffinityError> {
        unreachable!("unavailable capability is rejected before platform application")
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod platform {
    use super::*;

    pub(super) fn capability() -> AffinityCapability {
        AffinityCapability {
            level: AffinitySupportLevel::Unavailable,
            mechanism: None,
            constraints: Vec::new(),
            reason: Some(format!("no affinity adapter for {}", std::env::consts::OS)),
        }
    }

    pub(super) fn apply_and_verify(
        _process_id: u32,
        _cpus: &[LogicalCpuId],
    ) -> Result<(), AffinityError> {
        unreachable!("unavailable capability is rejected before platform application")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(number: u32) -> LogicalCpuId {
        LogicalCpuId { group: 0, number }
    }

    #[test]
    fn off_is_a_recorded_no_op_even_without_a_real_process() {
        assert_eq!(
            apply_process_affinity(u32::MAX, &CpuAllocation::Unrestricted).unwrap(),
            AppliedAffinity {
                process_id: u32::MAX,
                outcome: AffinityOutcome::Off,
                cpus: Vec::new(),
                mechanism: None,
            }
        );
    }

    #[test]
    fn hard_requests_reject_empty_and_duplicate_cpu_sets() {
        assert!(matches!(
            apply_process_affinity(1, &CpuAllocation::Enforced(Vec::new())),
            Err(AffinityError::EmptyCpuSet)
        ));
        assert!(matches!(
            apply_process_affinity(1, &CpuAllocation::Enforced(vec![cpu(2), cpu(2)])),
            Err(AffinityError::DuplicateCpu(id)) if id == cpu(2)
        ));
    }

    #[test]
    fn advisory_is_never_silently_treated_as_enforced_or_off() {
        assert!(matches!(
            apply_process_affinity(1, &CpuAllocation::Advisory(vec![cpu(0)])),
            Err(AffinityError::AdvisoryUnsupported)
        ));
    }

    #[test]
    fn capability_is_self_consistent() {
        let capability = affinity_capability();
        match capability.level {
            AffinitySupportLevel::Enforced => {
                assert!(capability.mechanism.is_some());
                assert!(capability.reason.is_none());
            }
            AffinitySupportLevel::Unavailable => {
                assert!(capability.mechanism.is_none());
                assert!(capability.reason.is_some());
            }
        }
    }
}
