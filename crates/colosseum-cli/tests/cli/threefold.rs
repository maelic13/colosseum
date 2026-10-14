//! The position a game starts from counts as its first occurrence.
//!
//! Each scripted game shuffles back to its start position every four plies,
//! so the start position occurs for the third time after ply 8, and the game
//! must end there as a threefold draw. A harness that does not count the
//! setup position plays a ninth ply and draws on the next repetition instead.
//! The FEN cases are the ones a fastchess comparison found.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

const FEN_CASES: [(&str, &str); 5] = [
    (
        "8/5K2/7P/p4k1n/1p1p4/1P6/1P1B4/8 b - - 0 1",
        "h5f6 f7g7 f6h5 g7f7 h5f6 f7g7 f6h5 g7f7",
    ),
    (
        "r4rk1/pb1pbppp/1p2p1n1/3N4/2qBP3/5BP1/1P3P1P/R2Q1RK1 w - - 0 1",
        "a1c1 c4a2 c1a1 a2c4 a1c1 c4a2 c1a1 a2c4",
    ),
    (
        "1r6/3n1pk1/1r1p1np1/q2Pp3/7p/R6P/NP3PP1/2QR1BK1 b - - 0 1",
        "a5c5 a3c3 c5a5 c3a3 a5c5 a3c3 c5a5 c3a3",
    ),
    (
        "2b1Q3/p1r3p1/1p2Bb1k/8/nP6/4PqP1/P4P1P/3R2K1 w - - 0 1",
        "e8h8 h6g6 h8e8 g6h6 e8h8 h6g6 h8e8 g6h6",
    ),
    (
        "8/2k1B3/pb2P1p1/2p2p1p/K1P2P1P/6P1/8/8 b - - 0 1",
        "c7c6 e7f8 c6c7 f8e7 c7c6 e7f8 c6c7 f8e7",
    ),
];

const STARTPOS_CYCLE: &str = "g1f3 g8f6 f3g1 f6g8 g1f3 g8f6 f3g1 f6g8";

fn fixture() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

/// Write the fixture's script and an EPD book of the FEN cases.
fn fen_inputs(root: &Path) -> (PathBuf, PathBuf) {
    let script = root.join("script.txt");
    let book = root.join("book.epd");
    std::fs::write(
        &script,
        FEN_CASES
            .iter()
            .map(|(fen, moves)| format!("{fen} | {moves}\n"))
            .collect::<String>(),
    )
    .unwrap();
    std::fs::write(
        &book,
        FEN_CASES
            .iter()
            .map(|(fen, _)| format!("{fen}\n"))
            .collect::<String>(),
    )
    .unwrap();
    (script, book)
}

fn startpos_script(root: &Path) -> PathBuf {
    let script = root.join("script.txt");
    std::fs::write(&script, format!("startpos | {STARTPOS_CYCLE}\n")).unwrap();
    script
}

fn match_command(run: &Path, script: &Path, games: u32) -> Command {
    let mut command = cli();
    command
        .args(["match", "--games", &games.to_string()])
        .arg(fixture())
        .arg(fixture())
        .arg(format!("--a-engine-arg=--script={}", script.display()))
        .arg(format!("--b-engine-arg=--script={}", script.display()))
        .args(["--seed", "7", "--placement", "off", "--json", "--dir"])
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

/// Every journaled game's termination.
fn terminations(run: &Path) -> Vec<String> {
    std::fs::read_to_string(run.join("games.jsonl"))
        .unwrap()
        .lines()
        .map(|line| {
            let record: Value = serde_json::from_str(line).unwrap();
            record["game"]["termination"].as_str().unwrap().to_owned()
        })
        .collect()
}

/// The engine moves of every game in `games.pgn`: one annotation each.
fn engine_plies(run: &Path) -> Vec<usize> {
    let pgn = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    pgn.split("[Event ")
        .skip(1)
        .map(|game| {
            game.split('{')
                .skip(1)
                .filter(|comment| !comment.starts_with("book}"))
                .count()
        })
        .collect()
}

fn assert_threefold_after(run: &Path, games: usize, engine_plies_per_game: usize) {
    let terminations = terminations(run);
    assert_eq!(terminations.len(), games);
    assert!(
        terminations.iter().all(|t| t == "Threefold"),
        "{terminations:?}"
    );
    assert_eq!(
        engine_plies(run),
        vec![engine_plies_per_game; games],
        "{}",
        std::fs::read_to_string(run.join("games.pgn")).unwrap()
    );
}

#[test]
fn a_match_draws_on_the_third_occurrence_of_an_epd_start_position() {
    let root = tempfile::tempdir().unwrap();
    let (script, book) = fen_inputs(root.path());
    let run = root.path().join("run");
    let output = match_command(&run, &script, 10)
        .arg("--book")
        .arg(&book)
        .output()
        .unwrap();
    succeeded(&output);
    assert_threefold_after(&run, 10, 8);
}

#[test]
fn a_match_draws_on_the_third_occurrence_of_startpos_without_a_book() {
    let root = tempfile::tempdir().unwrap();
    let script = startpos_script(root.path());
    let run = root.path().join("run");
    let output = match_command(&run, &script, 2).output().unwrap();
    succeeded(&output);
    assert_threefold_after(&run, 2, 8);
}

#[test]
fn the_position_before_a_pgn_openings_first_move_counts() {
    let root = tempfile::tempdir().unwrap();
    let script = startpos_script(root.path());
    // The book plays the first half of the cycle; the engines the rest.
    let book = root.path().join("book.pgn");
    std::fs::write(&book, "1. Nf3 Nf6 2. Ng1 Ng8 *\n").unwrap();
    let run = root.path().join("run");
    let output = match_command(&run, &script, 2)
        .arg("--book")
        .arg(&book)
        .args(["--book-plies", "4"])
        .output()
        .unwrap();
    succeeded(&output);
    assert_threefold_after(&run, 2, 4);
}

#[test]
fn a_tournament_draws_on_the_third_occurrence_of_an_epd_start_position() {
    let root = tempfile::tempdir().unwrap();
    let (script, book) = fen_inputs(root.path());
    let run = root.path().join("run");
    let output = cli()
        .args(["tournament", "run", "--engine"])
        .arg(fixture())
        .arg("--engine")
        .arg(fixture())
        .arg(format!("--engine-arg=--script={}", script.display()))
        .args(["--games-per-pair", "2", "--cycles", "5", "--book"])
        .arg(&book)
        .args(["--seed", "7", "--placement", "off", "--json", "--dir"])
        .arg(&run)
        .output()
        .unwrap();
    succeeded(&output);
    assert_threefold_after(&run, 10, 8);
}
