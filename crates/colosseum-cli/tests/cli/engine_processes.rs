//! A slot keeps its two engine processes from game to game.
//!
//! The fixture engine logs every command it receives with its process
//! identifier, so the tests read from the engines' side what the harness did:
//! how many processes each side ran, which options each process was sent and
//! how often a new game was announced.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::{Command, Output};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

fn fixture() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn match_command(run: &Path, games: u32, extra: &[String]) -> Command {
    let mut command = cli();
    command
        .args(["match", "--games", &games.to_string()])
        .arg(fixture())
        .arg(fixture())
        .args([
            "--a-engine-arg=--legal-sequence",
            "--b-engine-arg=--legal-sequence",
            // Inside the fixture's scripted moves, so no game ends in a fault.
            "--max-moves",
            "2",
            "--seed",
            "7",
            "--concurrency",
            "1",
            "--placement",
            "off",
            "--max-engine-faults",
            "100",
            "--json",
        ])
        .args(extra)
        .arg("--dir")
        .arg(run);
    command
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The logged commands: `(process identifier, command)`.
fn commands(log: &Path) -> Vec<(u32, String)> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (pid, command) = line.split_once(' ')?;
            Some((pid.parse().ok()?, command.to_owned()))
        })
        .collect()
}

fn processes(log: &[(u32, String)]) -> usize {
    log.iter()
        .map(|(pid, _)| *pid)
        .collect::<BTreeSet<_>>()
        .len()
}

fn count(log: &[(u32, String)], command: &str) -> usize {
    log.iter().filter(|(_, line)| line == command).count()
}

#[test]
fn a_slot_keeps_its_engines_for_the_whole_match_and_resends_only_changed_options() {
    let root = tempfile::tempdir().unwrap();
    let play = |name: &str, mode: &str| {
        let (a_log, b_log) = (
            root.path().join(format!("{name}-a.log")),
            root.path().join(format!("{name}-b.log")),
        );
        let output = match_command(
            &root.path().join(name),
            3,
            &[
                format!("--a-engine-arg=--log-commands={}", a_log.display()),
                format!("--b-engine-arg=--log-commands={}", b_log.display()),
                "--a-option".into(),
                "Hash=32".into(),
                "--engine-processes".into(),
                mode.into(),
            ],
        )
        .output()
        .unwrap();
        succeeded(&output);
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["report"]["games_completed"], 3, "{report}");
        (commands(&a_log), commands(&b_log))
    };

    let (a, b) = play("kept", "per-slot");
    // One process per side for three games, each told of every new game; the
    // option set once, not again for games whose value did not change.
    assert_eq!((processes(&a), processes(&b)), (1, 1));
    assert_eq!(count(&a, "uci"), 1);
    assert_eq!(count(&a, "ucinewgame"), 3);
    assert_eq!(count(&a, "setoption name Hash value 32"), 1);
    assert_eq!(count(&b, "ucinewgame"), 3);

    let (a, b) = play("fresh", "per-game");
    assert_eq!((processes(&a), processes(&b)), (3, 3));
    assert_eq!(count(&a, "setoption name Hash value 32"), 3);

    // The mode is part of the resolved configuration.
    let resolved: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("kept").join("resolved-config.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(resolved["engine_processes"], "per-slot", "{resolved}");
}

#[test]
fn an_engine_that_faults_is_replaced_and_its_opponent_is_kept() {
    let root = tempfile::tempdir().unwrap();
    let (a_log, b_log) = (root.path().join("a.log"), root.path().join("b.log"));
    let output = match_command(
        &root.path().join("run"),
        2,
        &[
            format!("--a-engine-arg=--log-commands={}", a_log.display()),
            format!("--b-engine-arg=--log-commands={}", b_log.display()),
            "--b-engine-arg=--crash-on-go".into(),
        ],
    )
    .output()
    .unwrap();
    succeeded(&output);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["report"]["faults"]["engine_b"], 2, "{report}");
    // B crashed in every game and was started again for the next one; A,
    // which never faulted, played both in one process.
    assert_eq!(processes(&commands(&b_log)), 2);
    assert_eq!(processes(&commands(&a_log)), 1);
}

fn read_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// Killing the harness outright takes its engines with it, whether the slot
/// keeps them or each game starts its own.
///
/// Engine A never answers a search and the clock would give it ten minutes,
/// so both processes are certainly still running — and still the harness's —
/// when it is killed: nothing but the kill can end them.
#[test]
fn engines_are_reaped_when_the_harness_is_killed_whether_kept_or_per_game() {
    let root = tempfile::tempdir().unwrap();
    for mode in ["per-slot", "per-game"] {
        let pid_files = [
            root.path().join(format!("{mode}-a.pid")),
            root.path().join(format!("{mode}-b.pid")),
        ];
        let mut child = match_command(
            &root.path().join(mode),
            1,
            &[
                "--a-engine-arg=--hang-on-go".into(),
                format!("--a-engine-arg=--pid-file={}", pid_files[0].display()),
                format!("--b-engine-arg=--pid-file={}", pid_files[1].display()),
                "--a-movetime-ms".into(),
                "600000".into(),
                "--b-movetime-ms".into(),
                "600000".into(),
                "--engine-processes".into(),
                mode.into(),
            ],
        )
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
        // Each engine writes its pid file as it starts.
        let deadline = Instant::now() + Duration::from_secs(30);
        let pids = loop {
            if let [Some(a), Some(b)] = pid_files.each_ref().map(|path| read_pid(path)) {
                break [a, b];
            }
            assert!(
                child.try_wait().unwrap().is_none(),
                "{mode}: the match ended before both engines started"
            );
            assert!(Instant::now() < deadline, "{mode}: no engines started");
            thread::sleep(Duration::from_millis(10));
        };
        child.kill().unwrap();
        child.wait().unwrap();
        // The operating system ends them; wait for it to have done so.
        let deadline = Instant::now() + Duration::from_secs(30);
        while pids.iter().any(|pid| colosseum_uci::process_is_alive(*pid)) {
            assert!(
                Instant::now() < deadline,
                "{mode}: engines {pids:?} outlived the harness"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}
