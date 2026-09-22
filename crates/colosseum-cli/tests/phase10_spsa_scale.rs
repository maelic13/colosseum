//! A tune at the scale of a real one: thousands of iterations.
//!
//! Games are played in-process as instant results (a hidden test switch), so
//! the run exercises everything around the games — the driver, the journal,
//! the checkpoint, the progress blocks and the result — at a length where a
//! cost that grows with the run shows. Per-iteration commit time must be flat
//! from the first iterations to the last, the memory the process has committed
//! must not grow with the iterations committed, and the result must stay
//! small: each iteration is a summary, and the games are in the journal. A
//! resumed tune must not keep the games it replayed.
//!
//! Memory is measured as private commit — what this process has asked the
//! operating system for and nobody else shares — rather than the resident set.
//! A resident set is what the OS has chosen to keep in RAM right now, which it
//! trims under pressure and grows when memory is plentiful, so comparing two
//! processes' resident sets measures the machine's mood as much as the
//! program's appetite: the comparison below failed intermittently under a
//! loaded test suite while the tune itself held exactly what it should.

use std::collections::BTreeMap;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const ITERATIONS: u64 = 5_000;
const GAMES_PER_ITERATION: u64 = 4;
/// About 0.5 KB of pretty-printed summary per iteration of a one-knob tune;
/// the old result carried every game, about 100 KB per iteration.
const RESULT_LIMIT_BYTES: u64 = 8 * 1024 * 1024;
/// Committed growth tolerated from early in the run to its end.
const MEMORY_GROWTH_LIMIT_BYTES: u64 = 16 * 1024 * 1024;
/// What a resumed tune may hold beyond one that never stopped: measured 2.8 MB
/// with the replayed games dropped and 13.7 MB with them kept.
const REPLAY_RESIDUE_LIMIT_BYTES: u64 = 8 * 1024 * 1024;

/// The private memory the process has committed, in bytes.
#[cfg(windows)]
fn committed_bytes(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };
    // SAFETY: the handle is checked and closed; the counters are plain data,
    // and the call is told the larger EX size it is given, which is the
    // documented way to read PrivateUsage.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let ok = K32GetProcessMemoryInfo(
            process,
            std::ptr::from_mut(&mut counters).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        );
        CloseHandle(process);
        (ok != 0).then_some(counters.PrivateUsage as u64)
    }
}

/// The private data segment, which is what this process asked for: `VmRSS`
/// would be the part of it the kernel currently keeps in RAM.
#[cfg(target_os = "linux")]
fn committed_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kilobytes = status
        .lines()
        .find_map(|line| line.strip_prefix("VmData:"))?
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(kilobytes * 1024)
}

#[cfg(not(any(windows, target_os = "linux")))]
fn committed_bytes(_pid: u32) -> Option<u64> {
    None
}

fn median(mut values: Vec<u64>) -> u64 {
    values.sort_unstable();
    values[values.len() / 2]
}

/// One invocation of the synthetic tune: its exit code, the committed memory
/// sampled while it ran, and its wall time.
struct Invocation {
    code: Option<i32>,
    samples: Vec<(Duration, u64)>,
    wall: Duration,
}

impl Invocation {
    /// The median committed memory over the part of the run between two
    /// fractions of its wall time, when enough samples were taken.
    fn committed(&self, from: u32, to: u32, of: u32) -> Option<u64> {
        if self.samples.len() < 20 {
            return None;
        }
        let window = self
            .samples
            .iter()
            .filter(|(at, _)| *at >= self.wall * from / of && *at < self.wall * to / of)
            .map(|(_, bytes)| *bytes)
            .collect::<Vec<_>>();
        (!window.is_empty()).then(|| median(window))
    }
}

fn synthetic_tune(root: &std::path::Path, run: &std::path::Path, extra: &[&str]) -> Invocation {
    let tune = root.join("tune.toml");
    std::fs::write(
        &tune,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
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
            "--seed",
            "7",
        ])
        .args(extra)
        .arg("--dir")
        .arg(run)
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
        if let Some(bytes) = committed_bytes(child.id()) {
            samples.push((started.elapsed(), bytes));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Invocation {
        code: status.code(),
        samples,
        wall: started.elapsed(),
    }
}

#[test]
fn a_five_thousand_iteration_tune_commits_its_last_iteration_as_cheaply_as_its_first() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let invocation = synthetic_tune(root.path(), &run, &[]);
    assert_eq!(invocation.code, Some(0), "the synthetic tune failed");
    let wall = invocation.wall;

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

    // Early: the second tenth of the run, once the tune is under way. Late:
    // its last fifth.
    let growth = invocation
        .committed(1, 2, 10)
        .zip(invocation.committed(4, 5, 5));
    eprintln!(
        "{ITERATIONS} iterations in {wall:?}; median cycle {first} us early, {last} us late; result {result_bytes} bytes; committed {growth:?}"
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
            "committed memory grew from {early} to {late} bytes"
        );
    }
}

/// A tune stopped late and resumed replays thousands of games from its
/// journal to rebuild its iterations. Those games are only an input to the
/// replay: once the summaries are rebuilt, the resumed process must hold no
/// more than a tune that never stopped, and must finish the same tune.
#[test]
fn a_resumed_tune_does_not_keep_the_games_it_replayed() {
    const STOP_AT: u64 = ITERATIONS * 9 / 10;
    let root = tempfile::tempdir().unwrap();
    let whole = synthetic_tune(root.path(), &root.path().join("whole"), &[]);
    assert_eq!(whole.code, Some(0), "the uninterrupted tune failed");

    let run = root.path().join("resumed");
    let stop = STOP_AT.to_string();
    let first = synthetic_tune(root.path(), &run, &["--stop-after-iteration", &stop]);
    assert_eq!(first.code, Some(6), "the tune did not stop at {STOP_AT}");
    let resumed = synthetic_tune(root.path(), &run, &[]);
    assert_eq!(resumed.code, Some(0), "the resumed tune failed");

    let journal = std::fs::read_to_string(run.join("games.jsonl")).unwrap();
    assert_eq!(
        journal.lines().count() as u64,
        ITERATIONS * GAMES_PER_ITERATION
    );
    let result = |name: &str| -> Value {
        serde_json::from_slice(&std::fs::read(root.path().join(name).join("result.json")).unwrap())
            .unwrap()
    };
    let (whole_result, resumed_result) = (result("whole"), result("resumed"));
    assert_eq!(
        resumed_result["driver"]["completed_iterations"],
        whole_result["driver"]["completed_iterations"],
        "the resumed tune is not the same tune"
    );

    // The resumed run's whole life against the uninterrupted run's last
    // fifth, which holds as many summaries.
    let committed = resumed.committed(0, 1, 1).zip(whole.committed(4, 5, 5));
    eprintln!(
        "resumed at {STOP_AT} of {ITERATIONS}: committed {committed:?} (resumed, uninterrupted) in {:?}",
        resumed.wall
    );
    if let Some((resumed, whole)) = committed {
        assert!(
            resumed <= whole + REPLAY_RESIDUE_LIMIT_BYTES,
            "the resumed tune holds {resumed} bytes against {whole} for one that never stopped"
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
