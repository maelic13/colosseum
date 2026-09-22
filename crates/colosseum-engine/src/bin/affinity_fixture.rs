//! Internal child used by the Phase 3 residency acceptance test.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::{Duration, Instant};

fn main() {
    let gate = std::env::args_os().nth(1).expect("gate path");
    let gate = Path::new(&gate);
    let wait_until = Instant::now() + Duration::from_secs(10);
    while !gate.exists() {
        if Instant::now() >= wait_until {
            eprintln!("timed out waiting for residency gate");
            std::process::exit(2);
        }
        std::thread::sleep(Duration::from_millis(2));
    }

    let sample_until = Instant::now() + Duration::from_millis(300);
    let mut observed = BTreeSet::new();
    let mut accumulator = 1_u64;
    while Instant::now() < sample_until {
        for value in 1..=4096_u64 {
            accumulator = accumulator
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(value);
        }
        if let Some((group, number)) = current_logical_cpu() {
            observed.insert((group, number));
        }
    }
    std::hint::black_box(accumulator);
    if observed.is_empty() {
        eprintln!("logical CPU residency sampling is unsupported");
        std::process::exit(3);
    }
    for (group, number) in observed {
        println!("{group}:{number}");
    }
}

#[cfg(windows)]
fn current_logical_cpu() -> Option<(u16, u32)> {
    use windows_sys::Win32::System::Kernel::PROCESSOR_NUMBER;
    use windows_sys::Win32::System::Threading::GetCurrentProcessorNumberEx;

    let mut processor = PROCESSOR_NUMBER::default();
    unsafe {
        GetCurrentProcessorNumberEx(&mut processor);
    }
    Some((processor.Group, u32::from(processor.Number)))
}

#[cfg(target_os = "linux")]
fn current_logical_cpu() -> Option<(u16, u32)> {
    let number = unsafe { libc::sched_getcpu() };
    (number >= 0).then_some((0, number as u32))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn current_logical_cpu() -> Option<(u16, u32)> {
    None
}
