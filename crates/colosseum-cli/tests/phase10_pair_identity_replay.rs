//! Regression tests for the pair-identity replay defects.
//!
//! All four share one shape: the written identity of a game was there, and the
//! reader guessed instead of using it. `stats` derived the pentanomial unit
//! from the game number, the PGN said nothing about the pairs a run had
//! stopped counting, a tournament's games all claimed opening zero, and an
//! artifact field named a statistics version while holding a random-stream
//! one.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn engine() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_colosseum-cli"))
}

/// An ordinary UCI executable that answers every search with `e2e4`.
///
/// It is legal from the initial position and illegal from everything else, so
/// against the conforming stub it loses whenever it has to move twice — and
/// wins whenever its opponent is the side that must move into an opening where
/// the canned reply does not fit. That asymmetry is what gives a stub SPRT a
/// pentanomial sample with variance, and so a boundary it can actually cross.
fn one_move_engine() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
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

fn replay(target: &Path) -> Value {
    let output = cli()
        .arg("stats")
        .arg(target)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stats {} failed: {}",
        target.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    json(&output)["report"].clone()
}

/// Two openings: one with White to move, one with Black to move.
///
/// The second misaligns both engines' canned replies, so the side that has to
/// move first loses the game — which makes the pair's two assignments cancel
/// out, while the first opening's pair is won twice.
fn mixed_book(root: &Path) -> PathBuf {
    let book = root.join("openings.epd");
    std::fs::write(
        &book,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\n\
         rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1\n",
    )
    .unwrap();
    book
}

fn three_opening_book(root: &Path) -> PathBuf {
    let book = root.join("three.epd");
    std::fs::write(
        &book,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\n\
         rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1\n\
         rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2\n",
    )
    .unwrap();
    book
}

/// One rendered game with the identity tags a Colosseum export carries.
fn game(number: u32, pair: u32, assignment: u32, white: &str, result: &str) -> String {
    let black = if white == "Alpha 1.0.0" {
        "Beta 1.0.0"
    } else {
        "Alpha 1.0.0"
    };
    format!(
        "[Event \"Colosseum CLI fixed match\"]\n\
         [Site \"?\"]\n\
         [Date \"????.??.??\"]\n\
         [Round \"{number}\"]\n\
         [White \"{white}\"]\n\
         [Black \"{black}\"]\n\
         [Result \"{result}\"]\n\
         [GameNumber \"{number}\"]\n\
         [PairNumber \"{pair}\"]\n\
         [PairGame \"{assignment}\"]\n\
         [OpeningIndex \"0\"]\n\
         [OpeningLabel \"e4 e5\"]\n\
         \n\
         1. e4 e5 {result}\n\n"
    )
}

/// Two encounters of one game each, on the same opening.
///
/// Numbering them one and two is enough for game-number arithmetic to call
/// them a colour-reversed pair, but they are two different contests and their
/// scores must never be added into one pentanomial unit.
#[test]
fn one_game_per_pair_is_never_joined_across_encounters() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("games.pgn");
    std::fs::write(
        &path,
        format!(
            "{}{}",
            game(1, 1, 1, "Alpha 1.0.0", "1-0"),
            game(2, 2, 1, "Alpha 1.0.0", "0-1"),
        ),
    )
    .unwrap();

    let report = replay(&path);
    assert_eq!(report["complete_pairs"], 0, "{report}");
    assert_eq!(report["unpaired_games"], 2, "{report}");
    assert_eq!(report["pentanomial"], Value::Null, "{report}");
}

/// Four games of one encounter are two pentanomial units, and the colour
/// inversion belongs to every even assignment, not only to assignment two.
#[test]
fn four_games_per_pair_are_two_units_and_invert_every_even_assignment() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("games.pgn");
    std::fs::write(
        &path,
        format!(
            "{}{}{}{}",
            // The first unit is shared: Alpha wins as White, Beta wins as White.
            game(1, 1, 1, "Alpha 1.0.0", "1-0"),
            game(2, 1, 2, "Beta 1.0.0", "1-0"),
            // The second unit is Alpha's twice: as White, then as Black.
            game(3, 1, 3, "Alpha 1.0.0", "1-0"),
            game(4, 1, 4, "Beta 1.0.0", "0-1"),
        ),
    )
    .unwrap();

    let report = replay(&path);
    assert_eq!(report["complete_pairs"], 2, "{report}");
    assert_eq!(report["unpaired_games"], 0, "{report}");
    assert_eq!(
        report["pentanomial"],
        serde_json::json!([0, 0, 1, 0, 1]),
        "{report}"
    );
}

/// A tournament encounter is the pair, whatever its length.
#[test]
fn a_tournament_replays_from_its_pgn_for_any_games_per_pair() {
    let root = tempfile::tempdir().unwrap();
    let book = three_opening_book(root.path());
    // One game per encounter is no pentanomial unit at all; two is one unit
    // per encounter; four is two.
    for (games_per_pair, expected_pairs) in [(1, 0), (2, 3), (4, 6)] {
        let run = root.path().join(format!("run-{games_per_pair}"));
        let played = cli()
            .args(["tournament", "run"])
            .args(["--engine".as_ref(), engine().as_os_str()])
            .args(["--engine".as_ref(), engine().as_os_str()])
            .args(["--engine".as_ref(), engine().as_os_str()])
            .args(["--games-per-pair", &games_per_pair.to_string()])
            .args([
                "--engine-arg=__uci-stub",
                "--movetime-ms",
                "10",
                "--max-moves",
                "4",
                "--max-engine-faults",
                "999",
                "--book",
            ])
            .arg(&book)
            .arg("--dir")
            .arg(&run)
            .arg("--json")
            .output()
            .unwrap();
        assert!(
            played.status.success(),
            "{games_per_pair} games per pair: {}",
            String::from_utf8_lossy(&played.stderr)
        );

        let directory = replay(&run);
        let pgn = replay(&run.join("games.pgn"));
        assert_eq!(directory["authority"], "structured-run-store");
        assert_eq!(pgn["authority"], "pgn-export");
        for field in ["games", "complete_pairs", "unpaired_games", "pentanomial"] {
            assert_eq!(
                directory[field], pgn[field],
                "{field} differs at {games_per_pair} games per pair"
            );
        }
        assert_eq!(
            directory["complete_pairs"], expected_pairs,
            "{games_per_pair} games per pair: {directory}"
        );
    }
}

/// The tournament chose the opening, so the written game must name the one it
/// chose — not the sole entry of the one-game book its inner match was handed.
#[test]
fn tournament_games_carry_the_encounters_own_opening() {
    let root = tempfile::tempdir().unwrap();
    let book = three_opening_book(root.path());
    let run = root.path().join("run");
    let played = cli()
        .args(["tournament", "run"])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args([
            "--engine-arg=__uci-stub",
            "--movetime-ms",
            "10",
            "--max-moves",
            "4",
            "--max-engine-faults",
            "999",
            "--book",
        ])
        .arg(&book)
        .arg("--dir")
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        played.status.success(),
        "{}",
        String::from_utf8_lossy(&played.stderr)
    );

    let pgn = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    let written = pgn
        .lines()
        .filter_map(|line| line.strip_prefix("[OpeningIndex \""))
        .filter_map(|rest| rest.strip_suffix("\"]"))
        .collect::<Vec<_>>();
    // Three encounters of two games each, in encounter order.
    assert_eq!(written, ["0", "0", "1", "1", "2", "2"], "{pgn}");

    // The label follows the same index, and the checkpoint agrees.
    let result: Value =
        serde_json::from_slice(&std::fs::read(run.join("result.json")).unwrap()).unwrap();
    for game in result["games"].as_array().unwrap() {
        let index = game["opening"]["book_index"].as_u64().unwrap();
        let label = game["opening"]["label"].as_str().unwrap();
        assert!(
            pgn.contains(&format!(
                "[OpeningIndex \"{index}\"]\n[OpeningLabel \"{label}\"]"
            )),
            "opening {index} is not written beside its label"
        );
    }
}

/// A run's own PGN keeps every game it played, including the pairs it finished
/// after its boundary. Those are evidence, not sample, and the replay must
/// reach the same official vector as the checkpoint.
#[test]
fn a_boundary_crossing_sprt_reports_its_official_sample_from_its_pgn() {
    let root = tempfile::tempdir().unwrap();
    let book = mixed_book(root.path());
    let run = root.path().join("run");
    let played = cli()
        .arg("sprt")
        .arg(engine())
        .arg(one_move_engine())
        .args([
            "--max-pairs",
            "200",
            "--model",
            "normalized",
            "--elo0",
            "0",
            "--elo1",
            "10",
            "--alpha",
            "0.05",
            "--beta",
            "0.05",
            "--a-engine-arg=__uci-stub",
            "--a-movetime-ms",
            "10",
            "--b-movetime-ms",
            "10",
            "--max-engine-faults",
            "99999",
            "--max-time-losses",
            "99999",
            // More than one slot, so pairs are still in flight when the
            // boundary is crossed and become post-terminal.
            "--concurrency",
            "4",
            "--book-wrap",
            "--book",
        ])
        .arg(&book)
        .arg("--dir")
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        played.status.success(),
        "{}",
        String::from_utf8_lossy(&played.stderr)
    );
    let report = &json(&played)["report"];
    assert_eq!(report["status"], "h1", "{report}");
    let post_terminal = report["schedule"]["post_terminal_pairs"]
        .as_array()
        .map_or(0, Vec::len);
    assert!(
        post_terminal > 0,
        "no pair survived the boundary, so nothing marks the difference"
    );

    let pgn_text = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    let official = report["schedule"]["official_pairs"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(
        pgn_text.matches("[ColosseumSample \"official\"]").count(),
        official * 2
    );
    assert_eq!(
        pgn_text
            .matches("[ColosseumSample \"post-terminal\"]")
            .count(),
        post_terminal * 2
    );

    let directory = replay(&run);
    let pgn = replay(&run.join("games.pgn"));
    assert_eq!(directory["authority"], "structured-run-store");
    assert_eq!(pgn["authority"], "pgn-export");
    for field in ["games", "complete_pairs", "unpaired_games", "pentanomial"] {
        assert_eq!(
            directory[field], pgn[field],
            "{field} differs between the checkpoint and its own PGN"
        );
    }
    assert_eq!(directory["complete_pairs"], official as u64);
    // The replay says out loud what it left out rather than dropping it
    // silently.
    assert!(
        pgn["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .unwrap_or_default()
                .contains("excluded from the official sample")),
        "{pgn}"
    );
}

fn spsa_tune(root: &Path) -> PathBuf {
    let path = root.join("tune.toml");
    std::fs::write(
        &path,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    path
}

/// Every SPSA game names its iteration and whether that iteration counted.
#[test]
fn spsa_games_name_their_iteration_and_sample_class() {
    let root = tempfile::tempdir().unwrap();
    let tune = spsa_tune(root.path());

    let committed = root.path().join("committed");
    let played = cli()
        .arg("spsa")
        .arg(engine())
        .arg("--engine-arg=__uci-stub")
        .arg("--tune")
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            "2",
            "--games-per-iteration",
            "2",
            "--depth",
            "1",
            "--max-moves",
            "2",
            "--seed",
            "7",
            "--dir",
        ])
        .arg(&committed)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        played.status.success(),
        "{}",
        String::from_utf8_lossy(&played.stderr)
    );
    let pgn = std::fs::read_to_string(committed.join("games.pgn")).unwrap();
    assert_eq!(pgn.matches("[ColosseumSample \"official\"]").count(), 4);
    assert_eq!(pgn.matches("[ColosseumSpsaIteration \"0\"]").count(), 2);
    assert_eq!(pgn.matches("[ColosseumSpsaIteration \"1\"]").count(), 2);
    assert_eq!(replay(&committed.join("games.pgn"))["complete_pairs"], 2);

    // An engine-attributable fault invalidates the whole mini-match, and the
    // games it played are kept but marked.
    let invalid = root.path().join("invalid");
    let refused = cli()
        .arg("spsa")
        .arg(one_move_engine())
        .arg("--tune")
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            "1",
            "--games-per-iteration",
            "2",
            "--depth",
            "1",
            "--seed",
            "7",
            "--dir",
        ])
        .arg(&invalid)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(json(&refused)["report"]["driver"]["status"], "invalid");
    let pgn = std::fs::read_to_string(invalid.join("games.pgn")).unwrap();
    assert_eq!(pgn.matches("[ColosseumSample \"invalid\"]").count(), 2);
    assert!(!pgn.contains("[ColosseumSample \"official\"]"));

    // Nothing in that file belongs to an official sample, and the replay says
    // so instead of reporting the games as statistics.
    let output = cli()
        .arg("stats")
        .arg(invalid.join("games.pgn"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(
        message.contains("nothing that belongs to the official sample"),
        "{message}"
    );
}
