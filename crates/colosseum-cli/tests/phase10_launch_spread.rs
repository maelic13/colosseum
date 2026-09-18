//! The games of a wave start together, however large the book.
//!
//! Launch settings are cloned once per launched game (SPSA) or pair (SPRT). A
//! book carried in them by value was copied with every launch — about half a
//! second each with a 2.6-million-line book, on the one task that starts
//! games — so an iteration's fifteen games started over six seconds. With a
//! 200,000-entry book, the first wave's launches must all start within a few
//! milliseconds of each other: any large value copied per launch shows here.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

const SLOTS: usize = 4;
/// Launches of one wave may differ by this much at most.
const SPREAD_LIMIT_US: u64 = 10_000;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    command.arg("--json");
    command
}

fn engine() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

/// A book of 200,000 lines, every one the start position, which the stub
/// engine plays legally from.
fn large_book(root: &Path) -> PathBuf {
    let path = root.join("large.epd");
    let line = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -\n";
    std::fs::write(&path, line.repeat(200_000)).unwrap();
    path
}

/// Run to a report. A four-pair SPRT ends inconclusive, which is an exit
/// code of its own and not a failure here.
fn run(command: &mut Command) {
    let output = command.output().unwrap();
    assert!(
        matches!(output.status.code(), Some(0 | 1 | 4)),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The start stamps of the given games, from the journal.
fn starts(run: &Path, games: &[u64]) -> Vec<u64> {
    let journal = std::fs::read_to_string(run.join("games.jsonl")).unwrap();
    let mut starts = Vec::new();
    for line in journal.lines() {
        let game = &serde_json::from_str::<Value>(line).unwrap()["game"];
        if games.contains(&game["number"].as_u64().unwrap()) {
            starts.push(game["slot"]["started_unix_us"].as_u64().unwrap());
        }
    }
    assert_eq!(starts.len(), games.len(), "{journal}");
    starts
}

fn spread(starts: &[u64]) -> u64 {
    starts.iter().max().unwrap() - starts.iter().min().unwrap()
}

fn slow_stub(command: &mut Command, prefix: &str) {
    command.args([
        &format!("--{prefix}engine-arg=__uci-stub"),
        &format!("--{prefix}engine-arg=--sleep-ms=25"),
    ]);
}

#[test]
fn an_spsa_wave_launches_together_with_a_large_book() {
    let root = tempfile::tempdir().unwrap();
    let book = large_book(root.path());
    let tune = root.path().join("tune.toml");
    std::fs::write(
        &tune,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    let directory = root.path().join("spsa");
    let mut command = cli();
    command.arg("spsa").arg(engine());
    slow_stub(&mut command, "");
    run(command
        .arg("--tune")
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            "1",
            "--games-per-iteration",
            "8",
            "--concurrency",
            &SLOTS.to_string(),
            "--depth",
            "1",
            "--max-moves",
            "2",
            "--seed",
            "7",
            "--book",
        ])
        .arg(&book)
        .arg("--dir")
        .arg(&directory));
    // The first wave: games 1 to 4, each launched as the previous one was.
    let first_wave = starts(&directory, &[1, 2, 3, 4]);
    let spread = spread(&first_wave);
    eprintln!("spsa first wave launched within {spread} us");
    assert!(
        spread < SPREAD_LIMIT_US,
        "the first wave's launches spread over {spread} us"
    );
}

#[test]
fn an_sprt_wave_launches_together_with_a_large_book() {
    let root = tempfile::tempdir().unwrap();
    let book = large_book(root.path());
    let directory = root.path().join("sprt");
    let mut command = cli();
    command.arg("sprt").arg(engine()).arg(engine());
    slow_stub(&mut command, "a-");
    slow_stub(&mut command, "b-");
    run(command
        .args([
            "--preset",
            "gainer",
            "--max-pairs",
            "4",
            "--concurrency",
            &SLOTS.to_string(),
            "--a-movetime-ms",
            "100",
            "--b-movetime-ms",
            "100",
            "--max-moves",
            "2",
            "--seed",
            "7",
            "--book",
        ])
        .arg(&book)
        .arg("--dir")
        .arg(&directory));
    // The first wave: the first game of each of the four pairs.
    let first_wave = starts(&directory, &[1, 3, 5, 7]);
    let spread = spread(&first_wave);
    eprintln!("sprt first wave launched within {spread} us");
    assert!(
        spread < SPREAD_LIMIT_US,
        "the first wave's launches spread over {spread} us"
    );
}
