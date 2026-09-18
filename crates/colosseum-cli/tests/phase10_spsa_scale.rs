//! A tune at the scale of a real one: thousands of iterations.
//!
//! Games are played in-process as instant results (a hidden test switch), so
//! the run exercises everything around the games — the driver, the journal,
//! the checkpoint, the progress blocks and the result — at a length where a
//! cost that grows with the run shows. Per-iteration commit time must be flat
//! from the first iterations to the last, the process's resident memory must
//! not grow with the iterations committed, and the result must stay small:
//! each iteration is a summary, and the games are in the journal.

use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const ITERATIONS: u64 = 5_000;
const GAMES_PER_ITERATION: u64 = 4;
/// About 1.1 KB of pretty-printed summary per iteration of a one-knob tune;
/// the old result carried every game, about 100 KB per iteration.
const RESULT_LIMIT_BYTES: u64 = 8 * 1024 * 1024;
/// Resident growth tolerated from early in the run to its end.
const MEMORY_GROWTH_LIMIT_BYTES: u64 = 16 * 1024 * 1024;

#[cfg(windows)]
fn resident_bytes(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };
    // SAFETY: the handle is checked and closed; the counters are plain data
    // sized for the call.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        let ok = K32GetProcessMemoryInfo(process, &mut counters, counters.cb);
        CloseHandle(process);
        (ok != 0).then_some(counters.WorkingSetSize as u64)
    }
}

#[cfg(target_os = "linux")]
fn resident_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kilobytes = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(kilobytes * 1024)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn resident_bytes(_pid: u32) -> Option<u64> {
    None
}

fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

#[test]
fn a_five_thousand_iteration_tune_commits_its_last_iteration_as_cheaply_as_its_first() {
    let root = tempfile::tempdir().unwrap();
    let tune = root.path().join("tune.toml");
    std::fs::write(
        &tune,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    let run = root.path().join("run");
    let executable = env!("CARGO_BIN_EXE_colosseum-cli");
    let mut child = Command::new(executable)
        .arg("--json")
        .arg("spsa")
        .arg(executable)
        .arg("--engine-arg=__uci-stub")
        .arg("--tune")
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            &ITERATIONS.to_string(),
            "--games-per-iteration",
            &GAMES_PER_ITERATION.to_string(),
            "--depth",
            "1",
            "--progress-every",
            "1000",
            "--__synthetic-games",
            "--dir",
        ])
        .arg(&run)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // Resident memory, sampled while the tune runs.
    let started = Instant::now();
    let mut samples = Vec::new();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if let Some(bytes) = resident_bytes(child.id()) {
            samples.push((started.elapsed(), bytes));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(status.success(), "the synthetic tune failed");
    let wall = started.elapsed();

    // Commit time per iteration, from the journal: each iteration ends when
    // its last game does, and the next begins after the commit.
    let mut ends = BTreeMap::<u64, u64>::new();
    let journal = std::fs::read_to_string(run.join("games.jsonl")).unwrap();
    for line in journal.lines() {
        let game = &serde_json::from_str::<Value>(line).unwrap()["game"];
        let iteration = game["iteration"].as_u64().unwrap();
        let ended = game["slot"]["ended_unix_us"].as_u64().unwrap();
        let end = ends.entry(iteration).or_insert(0);
        *end = (*end).max(ended);
    }
    assert_eq!(ends.len() as u64, ITERATIONS);
    assert_eq!(
        journal.lines().count() as u64,
        ITERATIONS * GAMES_PER_ITERATION
    );
    let ends = ends.into_values().collect::<Vec<_>>();
    let cycles = ends
        .windows(2)
        .map(|pair| pair[1].saturating_sub(pair[0]))
        .collect::<Vec<_>>();
    let tenth = cycles.len() / 10;
    let first = median(cycles[..tenth].to_vec());
    let last = median(cycles[cycles.len() - tenth..].to_vec());

    let result_bytes = std::fs::metadata(run.join("result.json")).unwrap().len();
    let result: Value =
        serde_json::from_slice(&std::fs::read(run.join("result.json")).unwrap()).unwrap();
    let completed = result["driver"]["completed_iterations"].as_array().unwrap();

    let growth = if samples.len() >= 20 {
        // Early: the second tenth of the run, once the tune is under way.
        // Late: its last fifth.
        let early = samples
            .iter()
            .filter(|(at, _)| *at >= wall / 10 && *at < wall / 5)
            .map(|(_, bytes)| *bytes)
            .collect::<Vec<_>>();
        let late = samples
            .iter()
            .filter(|(at, _)| *at >= wall * 4 / 5)
            .map(|(_, bytes)| *bytes)
            .collect::<Vec<_>>();
        (!early.is_empty() && !late.is_empty()).then(|| (median(early), median(late)))
    } else {
        None
    };
    eprintln!(
        "{ITERATIONS} iterations in {wall:?}; median cycle {first} us early, {last} us late; result {result_bytes} bytes; resident {growth:?}"
    );

    assert!(
        last <= first * 3 + 2_000,
        "an iteration took {first} us to commit at the start and {last} us at the end"
    );
    assert_eq!(completed.len() as u64, ITERATIONS);
    assert!(
        completed
            .iter()
            .all(|iteration| iteration.get("pairs").is_none()),
        "the result still carries games"
    );
    assert!(
        result_bytes < RESULT_LIMIT_BYTES,
        "result.json is {result_bytes} bytes"
    );
    if let Some((early, late)) = growth {
        assert!(
            late <= early + MEMORY_GROWTH_LIMIT_BYTES,
            "resident memory grew from {early} to {late} bytes"
        );
    }
}

/// A budget in games derives the iteration count, is stored in the resolved
/// configuration, and is refused when the mini-match does not divide it.
#[test]
fn a_game_budget_sets_the_horizon_and_is_refused_when_it_does_not_divide() {
    let root = tempfile::tempdir().unwrap();
    let tune = root.path().join("tune.toml");
    std::fs::write(
        &tune,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    let executable = env!("CARGO_BIN_EXE_colosseum-cli");
    let spsa = |extra: &[&str]| {
        Command::new(executable)
            .args(["--json", "--dry-run", "spsa"])
            .arg(executable)
            .arg("--engine-arg=__uci-stub")
            .arg("--tune")
            .arg(&tune)
            .args(["--r-end", "0.002", "--depth", "1"])
            .args(extra)
            .output()
            .unwrap()
    };
    let planned = spsa(&["--total-games", "168000", "--games-per-iteration", "42"]);
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    let value: Value = serde_json::from_slice(&planned.stdout).unwrap();
    let configuration = &value["resolved_configuration"];
    assert_eq!(configuration["settings"]["iterations"], 4_000);
    assert_eq!(configuration["total_games"], 168_000);

    let refused = spsa(&["--total-games", "160000", "--games-per-iteration", "42"]);
    assert_eq!(refused.status.code(), Some(2));
    let message = String::from_utf8_lossy(&refused.stderr);
    assert!(
        message.contains("159978 (3809 iterations) or 160020 (3810 iterations)"),
        "{message}"
    );

    let both = spsa(&["--total-games", "168000", "--iterations", "10"]);
    assert_eq!(both.status.code(), Some(2));

    // The plan takes the same budget and reports hours from the wave model.
    let plan = Command::new(executable)
        .args(["spsa", "plan", "--tune"])
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--total-games",
            "168000",
            "--games-per-iteration",
            "42",
            "--concurrency",
            "14",
            "--seconds-per-game-low",
            "8",
            "--seconds-per-game-high",
            "10",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&plan.stdout);
    assert!(plan.status.success(), "{text}");
    // 4,000 iterations of three full waves of 14: 12,000 waves of 8..10 s.
    assert!(
        text.contains("estimated wall time: 26.7..33.3 hours"),
        "{text}"
    );
}
