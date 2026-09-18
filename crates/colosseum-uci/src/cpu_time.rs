//! How much CPU time an engine process has consumed, precisely enough to set
//! against one search.
//!
//! A search's charged time is wall time. When the engine's process consumed
//! less CPU than that, it was not running for the difference: held off its
//! processors, or waiting inside the operating system. That difference is the
//! one reading every search provides about a stall, where a forfeit happens
//! once in thousands of games.
//!
//! Windows accounts CPU in time-stamp-counter cycles per thread
//! (`QueryProcessCycleTime` sums them for a process); its tick-based
//! `GetProcessTimes` moves in 15.6 ms steps and cannot resolve one search.
//! The cycle rate is calibrated once against the monotonic clock. Other
//! platforms report nothing.

/// A handle for reading one process's consumed CPU time.
pub(crate) struct ProcessCpu {
    #[cfg(all(windows, target_arch = "x86_64"))]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: the handle is a process handle with query access only; it is used
// by one owner at a time and closed exactly once by Drop.
#[cfg(all(windows, target_arch = "x86_64"))]
unsafe impl Send for ProcessCpu {}

impl ProcessCpu {
    /// Open the process for CPU-time queries; `None` where the platform
    /// offers no cycle-exact account or the process cannot be opened.
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn open(process_id: u32) -> Option<Self> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        cycles_per_second()?;
        // SAFETY: a plain query-access open; the handle is checked.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        (!handle.is_null()).then_some(Self { handle })
    }

    #[cfg(not(all(windows, target_arch = "x86_64")))]
    pub(crate) fn open(_process_id: u32) -> Option<Self> {
        None
    }

    /// CPU time the process has consumed so far, in nanoseconds.
    #[cfg(all(windows, target_arch = "x86_64"))]
    pub(crate) fn consumed_ns(&self) -> Option<u64> {
        use windows_sys::Win32::System::WindowsProgramming::QueryProcessCycleTime;
        let mut cycles = 0_u64;
        // SAFETY: the handle is open with query access; the out-pointer is
        // a live u64.
        if unsafe { QueryProcessCycleTime(self.handle, &mut cycles) } == 0 {
            return None;
        }
        let rate = cycles_per_second()?;
        Some((cycles as f64 / rate * 1e9) as u64)
    }

    #[cfg(not(all(windows, target_arch = "x86_64")))]
    pub(crate) fn consumed_ns(&self) -> Option<u64> {
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

    #[test]
    fn a_busy_process_consumes_about_its_wall_time_and_an_idle_one_none() {
        let own = ProcessCpu::open(std::process::id()).expect("own process opens");
        let spin = |duration: std::time::Duration| {
            let start = std::time::Instant::now();
            while start.elapsed() < duration {
                std::hint::spin_loop();
            }
        };
        let before = own.consumed_ns().unwrap();
        spin(std::time::Duration::from_millis(100));
        let busy = own.consumed_ns().unwrap() - before;
        // Other test threads may add CPU; a preempted spin may lose some.
        assert!(busy >= 60_000_000, "spinning 100 ms consumed {busy} ns");

        let before = own.consumed_ns().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(100));
        let idle = own.consumed_ns().unwrap() - before;
        assert!(
            idle < busy / 2,
            "sleeping 100 ms consumed {idle} ns against {busy} ns spinning"
        );
    }
}
