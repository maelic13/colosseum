//! What an engine process has consumed, precisely enough to set against one
//! search: CPU time, the part of it spent in the kernel, and page faults.
//!
//! A search's charged time is wall time. When the engine's process consumed
//! less CPU than that, it was not running for the difference: held off its
//! processors, or waiting inside the operating system. When it consumed all
//! of it but its search still overran, the kernel time and the page faults
//! over the search say whether the operating system's memory manager was
//! doing the work in the process's name. These are the readings every search
//! provides about a stall, where a forfeit happens once in thousands of games.
//!
//! Windows accounts CPU in time-stamp-counter cycles per thread
//! (`QueryProcessCycleTime` sums them for a process); its tick-based
//! `GetProcessTimes` moves in 15.6 ms steps and cannot resolve one search, so
//! it is used only for the kernel share, where a burst of tens of
//! milliseconds is what is looked for. The page-fault count is exact. The
//! cycle rate is calibrated once against the monotonic clock. Other
//! platforms report nothing.

/// One reading of a process's counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessSample {
    /// CPU time consumed, user and kernel, in nanoseconds (cycle-exact).
    pub cpu_ns: u64,
    /// Kernel-mode CPU time, in nanoseconds (tick-granular).
    pub kernel_ns: u64,
    /// Page faults, soft and hard.
    pub page_faults: u64,
}

/// A handle for reading one process's counters.
pub(crate) struct ProcessCpu {
    #[cfg(all(windows, target_arch = "x86_64"))]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: the handle is a process handle with query access only; it is used
// by one owner at a time and closed exactly once by Drop.
#[cfg(all(windows, target_arch = "x86_64"))]
unsafe impl Send for ProcessCpu {}

impl ProcessCpu {
    /// Open the process for queries; `None` where the platform offers no
    /// cycle-exact account or the process cannot be opened.
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn open(process_id: u32) -> Option<Self> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
        };
        cycles_per_second()?;
        // SAFETY: a plain query-access open; the handle is checked.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                0,
                process_id,
            )
        };
        (!handle.is_null()).then_some(Self { handle })
    }

    #[cfg(not(all(windows, target_arch = "x86_64")))]
    pub(crate) fn open(_process_id: u32) -> Option<Self> {
        None
    }

    /// The process's counters now.
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn sample(&self) -> Option<ProcessSample> {
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetProcessTimes;
        use windows_sys::Win32::System::WindowsProgramming::QueryProcessCycleTime;
        let mut cycles = 0_u64;
        // SAFETY: the handle is open with query access; the out-pointer is
        // a live u64.
        if unsafe { QueryProcessCycleTime(self.handle, &mut cycles) } == 0 {
            return None;
        }
        let mut times = [FILETIME::default(); 4];
        let [created, exited, kernel, user] = &mut times;
        // SAFETY: four live FILETIME out-pointers and an open handle.
        if unsafe { GetProcessTimes(self.handle, created, exited, kernel, user) } == 0 {
            return None;
        }
        let kernel_100ns =
            u64::from(times[2].dwHighDateTime) << 32 | u64::from(times[2].dwLowDateTime);
        // SAFETY: zeroed plain-data counters sized for the call.
        let mut memory: PROCESS_MEMORY_COUNTERS = unsafe { std::mem::zeroed() };
        memory.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        // SAFETY: the handle has query and VM-read access; the counters are
        // live and their size is given.
        if unsafe { K32GetProcessMemoryInfo(self.handle, &mut memory, memory.cb) } == 0 {
            return None;
        }
        let rate = cycles_per_second()?;
        Some(ProcessSample {
            cpu_ns: (cycles as f64 / rate * 1e9) as u64,
            kernel_ns: kernel_100ns.saturating_mul(100),
            page_faults: u64::from(memory.PageFaultCount),
        })
    }

    #[cfg(not(all(windows, target_arch = "x86_64")))]
    pub(crate) fn sample(&self) -> Option<ProcessSample> {
        None
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
impl Drop for ProcessCpu {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `open` and is closed once.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.handle);
        }
    }
}

/// The time-stamp counter's rate, measured once against the monotonic clock
/// over a 20 ms spin. Windows counts thread cycles in this counter's units.
#[cfg(all(windows, target_arch = "x86_64"))]
fn cycles_per_second() -> Option<f64> {
    static RATE: std::sync::OnceLock<Option<f64>> = std::sync::OnceLock::new();
    *RATE.get_or_init(|| {
        use std::time::{Duration, Instant};
        // SAFETY: `_rdtsc` has no preconditions on x86_64.
        let read = || unsafe { core::arch::x86_64::_rdtsc() };
        let (start, first) = (Instant::now(), read());
        while start.elapsed() < Duration::from_millis(20) {
            std::hint::spin_loop();
        }
        let (elapsed, last) = (start.elapsed(), read());
        let rate = last.wrapping_sub(first) as f64 / elapsed.as_secs_f64();
        (rate.is_finite() && rate > 1e8).then_some(rate)
    })
}

#[cfg(all(test, windows, target_arch = "x86_64"))]
mod tests {
    use super::*;

    /// The adapter reads the process's own CPU counter: it never runs
    /// backwards, and work done on this thread shows up in it. How much CPU a
    /// wall-clock interval costs is left alone — other test threads share the
    /// process and the scheduler shares the host, so no ratio holds reliably.
    #[test]
    fn the_cpu_counter_is_monotonic_and_counts_work_done_by_this_process() {
        let own = ProcessCpu::open(std::process::id()).expect("own process opens");
        let before = own.sample().unwrap().cpu_ns;
        let mut latest = before;
        let mut work = 0_u64;
        // Only a broken counter reaches this bound; it keeps that a failure
        // rather than a hang.
        let give_up = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while latest == before {
            assert!(
                std::time::Instant::now() < give_up,
                "the counter never moved"
            );
            for _ in 0..1_000_000 {
                work = std::hint::black_box(work.wrapping_add(1));
            }
            let now = own.sample().unwrap().cpu_ns;
            assert!(
                now >= latest,
                "the counter ran backwards: {latest} then {now}"
            );
            latest = now;
        }
        assert!(latest > before);
    }

    #[test]
    fn touching_fresh_memory_is_counted_in_page_faults() {
        let own = ProcessCpu::open(std::process::id()).expect("own process opens");
        let before = own.sample().unwrap().page_faults;
        // 16 MiB of fresh pages, each touched once: a demand-zero fault each.
        let mut pages = vec![0_u8; 16 << 20];
        for page in pages.chunks_mut(4096) {
            page[0] = 1;
        }
        std::hint::black_box(&pages);
        let faults = own.sample().unwrap().page_faults - before;
        assert!(
            faults >= 2_000,
            "touching 4,096 fresh pages faulted {faults} times"
        );
    }
}
