//! A hard kill costs at most the last sync window, and resume says what it
//! dropped.
//!
//! A run appends each game to `games.jsonl` and `games.pgn` without syncing
//! and syncs both together every fifty games or one second. A kill inside that
//! window can leave a torn last journal line, and journal lines whose moves the
//! operating system never wrote to `games.pgn`. These cases produce exactly
//! those states from a real killed run, resume it, and require the result an
//! uninterrupted run produces. A journal that no longer matches its checkpoint
//! is refused, never repaired.

use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;

const GAMES: &str = "200";

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    command.arg("--json");
    command
}

fn engine() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn match_command(run: &Path) -> Command {
    let mut command = cli();
    command
        .arg("match")
        .arg(engine())
        .arg(engine())
        .args([
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            "--a-movetime-ms",
            "5",
            "--b-movetime-ms",
            "5",
            "--max-moves",
            "2",
            "--seed",
            "11",
            "--games",
            GAMES,
            "--dir",
        ])
        .arg(run);
    command
}

fn json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

/// Every complete journal line as (start offset, end offset, parsed line).
fn journal_lines(run: &Path) -> Vec<(usize, usize, Value)> {
    let bytes = std::fs::read(run.join("games.jsonl")).unwrap_or_default();
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            lines.push((
                start,
                index + 1,
                serde_json::from_slice(&bytes[start..index]).unwrap(),
            ));
            start = index + 1;
        }
    }
    lines
}

/// Games the newest checkpoint covers; zero before the first one.
fn covered(run: &Path) -> usize {
    std::fs::read(run.join("checkpoint.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .and_then(|value| value["payload"]["journal"]["records"].as_u64())
        .map_or(0, |records| records as usize)
}

/// Play part of the match and kill the process outright, with at least three
/// games committed after the last checkpoint.
fn killed_run(run: &Path) {
    let mut child = match_command(run)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        assert!(
            child.try_wait().unwrap().is_none(),
            "the match finished before it could be killed"
        );
        if journal_lines(run).len() >= covered(run) + 3 {
            break;
        }
        assert!(Instant::now() < deadline, "no games were committed");
        thread::sleep(Duration::from_millis(2));
    }
    child.kill().unwrap();
    child.wait().unwrap();
}

fn truncate(path: &Path, length: usize) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(length as u64)
        .unwrap();
}

fn report_counts(value: &Value) -> Vec<Value> {
    ["games_attempted", "games_completed", "engine_a", "engine_b"]
        .iter()
        .map(|field| value["report"][field].clone())
        .collect()
}

#[test]
fn a_kill_inside_the_sync_window_loses_only_that_window() {
    let root = tempfile::tempdir().unwrap();
    let straight = root.path().join("straight");
    let expected = match_command(&straight).output().unwrap();
    assert!(expected.status.success());
    let expected = json(&expected);

    let run = root.path().join("killed");
    killed_run(&run);
    let lines = journal_lines(&run);
    let covered = covered(&run);
    assert!(
        lines.len() >= covered + 3,
        "{} lines, {covered} covered",
        lines.len()
    );

    // What the operating system may lose of an unsynced window: the last
    // journal line half-written, and the moves of the game before it never
    // written at all. Everything the checkpoint covers is untouched.
    let (last_start, last_end, _) = lines[lines.len() - 1];
    truncate(
        &run.join("games.jsonl"),
        last_start + (last_end - last_start) / 2,
    );
    let span = &lines[lines.len() - 2].2["pgn"];
    let cut = span["offset"].as_u64().unwrap() + span["length"].as_u64().unwrap() / 2;
    truncate(&run.join("games.pgn"), cut as usize);
    let durable = lines.len() - 2;
    let prefix = std::fs::read(run.join("games.jsonl")).unwrap()[..lines[durable - 1].1].to_vec();

    // `status` reads the damage and repairs none of it.
    let status = json(&cli().arg("status").arg(&run).output().unwrap());
    let journal = &status["durable"]["journal"];
    assert_eq!(journal["games"], durable, "{status}");
    assert_eq!(journal["covered_by_checkpoint"], covered);
    assert!(journal["torn_bytes"].as_u64().unwrap() > 0);
    assert_eq!(journal["games_without_moves"], 1);
    assert_eq!(
        std::fs::metadata(run.join("games.pgn")).unwrap().len(),
        cut,
        "status changed the run directory"
    );

    let resumed = match_command(&run).output().unwrap();
    let stderr = String::from_utf8_lossy(&resumed.stderr).into_owned();
    assert!(resumed.status.success(), "{stderr}");
    assert!(
        stderr.contains("dropped an unfinished last journal line"),
        "{stderr}"
    );
    assert!(
        stderr.contains("1 journalled game(s) had not reached games.pgn"),
        "{stderr}"
    );
    let resumed = json(&resumed);
    assert_eq!(resumed["report"]["status"], "completed");
    assert_eq!(report_counts(&resumed), report_counts(&expected));

    // The resume appended to what was durable and rewrote none of it.
    let journal = std::fs::read(run.join("games.jsonl")).unwrap();
    assert!(journal.starts_with(&prefix));
    let lines = journal_lines(&run);
    assert_eq!(lines.len(), GAMES.parse::<usize>().unwrap());
    let mut numbers = lines
        .iter()
        .map(|(_, _, line)| line["game"]["number"].as_u64().unwrap())
        .collect::<Vec<_>>();
    numbers.sort_unstable();
    numbers.dedup();
    assert_eq!(numbers.len(), lines.len(), "a game was journalled twice");
    let pgn = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    assert_eq!(pgn.matches("[Event ").count(), lines.len());
    // Every line's moves are where the line says they are.
    for (_, _, line) in &lines {
        let span = &line["pgn"];
        let offset = span["offset"].as_u64().unwrap() as usize;
        let length = span["length"].as_u64().unwrap() as usize;
        assert!(pgn.as_bytes()[offset..offset + length].starts_with(b"[Event "));
    }
}

#[test]
fn a_journal_that_no_longer_matches_its_checkpoint_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("stopped");
    // A clean stop writes a checkpoint covering every committed game.
    let mut command = cli();
    command.args(["--__stop-after-units", "3"]);
    // The same match, with the stop hook ahead of its own `--json`.
    let template = match_command(&run);
    let stopped = command.args(template.get_args().skip(1)).output().unwrap();
    assert_eq!(stopped.status.code(), Some(6));
    let covered = covered(&run);
    assert!(covered >= 3);

    // Change one byte the checkpoint covers.
    let path = run.join("games.jsonl");
    let mut bytes = std::fs::read(&path).unwrap();
    let position = bytes.iter().position(|byte| *byte == b'"').unwrap() + 1;
    bytes[position] ^= 0x01;
    std::fs::write(&path, &bytes).unwrap();

    let status = json(&cli().arg("status").arg(&run).output().unwrap());
    assert!(
        status["durable"]["journal"]["refused"]
            .as_str()
            .is_some_and(|error| error.contains("does not match its checkpoint")),
        "{status}"
    );
    let resumed = match_command(&run).output().unwrap();
    assert!(!resumed.status.success());
    let stderr = String::from_utf8_lossy(&resumed.stderr);
    assert!(
        stderr.contains("This is a refusal, not a repair"),
        "{stderr}"
    );
    // The refusal left the directory exactly as it found it.
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
