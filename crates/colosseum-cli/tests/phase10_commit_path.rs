//! A hard kill costs at most the last sync window, and resume says what it
//! dropped.
//!
//! A run appends each game to `games.jsonl` and `games.pgn` without syncing
//! and syncs both together every fifty games or one second. A kill inside that
//! window can leave a torn last journal line, and journal lines whose moves the
//! operating system never wrote to `games.pgn`. These cases produce exactly
//! those states deterministically — a run directory whose journal runs past
//! its checkpoint, then torn the way a lost write tears it — resume it, and
//! require the complete run. A journal that no longer matches its checkpoint
//! is refused, never repaired.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const GAMES: &str = "8";

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

/// The match, stopped cleanly by the internal hook after `units` games of
/// this invocation.
fn stopped_run(run: &Path, units: u64) {
    let mut command = cli();
    command.args(["--__stop-after-units", &units.to_string()]);
    // The same match, with the stop hook ahead of its own `--json`.
    let template = match_command(run);
    let stopped = command.args(template.get_args().skip(1)).output().unwrap();
    assert_eq!(
        stopped.status.code(),
        Some(6),
        "{}",
        String::from_utf8_lossy(&stopped.stderr)
    );
}

/// What a kill between two checkpoints leaves: a checkpoint, and journal and
/// PGN records of at least three games committed after it.
///
/// Rather than racing a kill against the run, the run is stopped cleanly,
/// its checkpoints are set aside, the run is resumed and stopped again, and
/// the older checkpoints are put back. The journal then runs past its
/// checkpoint exactly as it does when the process dies before the next one.
fn killed_run(run: &Path) {
    stopped_run(run, 3);
    let checkpoints = ["checkpoint.json", "checkpoint.previous.json"]
        .map(|name| (run.join(name), std::fs::read(run.join(name)).ok()));
    stopped_run(run, 3);
    for (path, bytes) in checkpoints {
        match bytes {
            Some(bytes) => std::fs::write(&path, bytes).unwrap(),
            None => {
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

fn truncate(path: &Path, length: usize) {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(length as u64)
        .unwrap();
}

#[test]
fn a_kill_inside_the_sync_window_loses_only_that_window() {
    let root = tempfile::tempdir().unwrap();
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
    let games = GAMES.parse::<u64>().unwrap();
    assert_eq!(resumed["report"]["games_attempted"], games, "{resumed}");
    assert_eq!(resumed["report"]["games_completed"], games, "{resumed}");

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
    assert_eq!(
        numbers,
        (1..=games).collect::<Vec<_>>(),
        "a game was journalled twice or never"
    );
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
    stopped_run(&run, 3);
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

#[test]
fn a_run_directory_from_before_the_journal_is_refused_with_the_restart_guidance() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("old");
    stopped_run(&run, 3);

    // What a directory written before the journal looks like to this
    // version: checkpoints in the first schema, and no journal beside them.
    for name in ["checkpoint.json", "checkpoint.previous.json"] {
        let path = run.join(name);
        if let Ok(bytes) = std::fs::read(&path) {
            let mut envelope: Value = serde_json::from_slice(&bytes).unwrap();
            envelope["schema_version"] = 1.into();
            std::fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        }
    }
    std::fs::remove_file(run.join("games.jsonl")).unwrap();

    let resumed = match_command(&run).output().unwrap();
    assert!(!resumed.status.success());
    let stderr = String::from_utf8_lossy(&resumed.stderr);
    assert!(
        stderr.contains("written by an earlier Colosseum version") && stderr.contains("--restart"),
        "{stderr}"
    );
    assert!(
        !stderr.contains("unsupported schema version"),
        "the refusal fell back to a bare schema message: {stderr}"
    );

    // The guidance works: a restart archives the old directory and plays.
    let mut restarted = match_command(&run);
    restarted.arg("--restart");
    let restarted = restarted.output().unwrap();
    assert!(
        restarted.status.success(),
        "{}",
        String::from_utf8_lossy(&restarted.stderr)
    );
}
