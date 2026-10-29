//! What a two-engine run reports at its end beyond its statistics: each
//! engine's official games by colour and its search telemetry, in
//! `results.txt`, in its `--json` value and in `run-record.json`, in the
//! figures `stats` replays from the same directory. The console keeps the
//! command's own report and names the file.
//!
//! How the games are counted is unit-tested in `stats_replay`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn stub() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn record(run: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap()
}

fn replay(run: &Path) -> Value {
    let output = cli().arg("stats").arg(run).arg("--json").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    json(&output)["report"].clone()
}

/// A match between two labelled stub engines, `games` long.
fn labelled_match(run: &Path, games: u32) -> Command {
    let mut command = cli();
    command
        .arg("match")
        .arg(stub())
        .arg(stub())
        .args([
            "--games",
            &games.to_string(),
            "--a-label",
            "Alpha",
            "--b-label",
            "Beta",
            "--a-engine-arg=__uci-stub",
            "--a-engine-arg=--sleep-ms=2",
            "--b-engine-arg=__uci-stub",
            "--b-engine-arg=--sleep-ms=2",
            "--a-movetime-ms",
            "50",
            "--b-movetime-ms",
            "50",
            "--max-moves",
            "4",
            "--dir",
        ])
        .arg(run);
    command
}

fn count(record: &Value, field: &str) -> u64 {
    record[field].as_u64().unwrap()
}

/// Each engine's record by colour is the other's from across the board, and
/// together they are the run's W/D/L.
fn assert_mirrored(by_colour: &Value, games: u64) {
    let [first, second] = [&by_colour[0], &by_colour[1]];
    for (colour, other) in [("as_white", "as_black"), ("as_black", "as_white")] {
        let (mine, theirs) = (&first[colour], &second[other]);
        assert_eq!(count(mine, "games"), count(theirs, "games"), "{by_colour}");
        assert_eq!(count(mine, "wins"), count(theirs, "losses"), "{by_colour}");
        assert_eq!(count(mine, "draws"), count(theirs, "draws"), "{by_colour}");
        assert_eq!(count(mine, "losses"), count(theirs, "wins"), "{by_colour}");
    }
    assert_eq!(
        count(&first["as_white"], "games") + count(&first["as_black"], "games"),
        games,
        "{by_colour}"
    );
}

#[test]
fn a_match_ends_with_each_engines_games_by_colour_and_its_search() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let played = labelled_match(&run, 4).arg("--json").output().unwrap();
    assert!(
        played.status.success(),
        "{}",
        String::from_utf8_lossy(&played.stderr)
    );
    let output = json(&played);
    let results = &output["results"];
    let by_colour = &results["by_colour"];
    assert_eq!(by_colour[0]["engine"], "Alpha");
    assert_eq!(by_colour[1]["engine"], "Beta");
    // Colours alternate, so each engine had White in half the games.
    assert_eq!(by_colour[0]["as_white"]["games"], 2);
    assert_mirrored(by_colour, 4);
    let engine_a = &output["report"]["engine_a"];
    for field in ["wins", "draws", "losses"] {
        assert_eq!(
            count(&by_colour[0]["as_white"], field) + count(&by_colour[0]["as_black"], field),
            count(engine_a, field),
            "{field}: {by_colour} against {engine_a}"
        );
    }

    // Both engines' searches, read from the run's PGN.
    let engines = results["telemetry"]["engines"].as_array().unwrap();
    let names = engines
        .iter()
        .map(|engine| engine["engine"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["Alpha", "Beta"]);
    for engine in engines {
        assert_eq!(engine["annotation_coverage"], 1.0, "{engine}");
        assert!(engine["nodes"]["mean"].is_number(), "{engine}");
        assert!(engine["nodes_per_second"].is_number(), "{engine}");
    }

    // The run record keeps exactly what the run printed, and a later replay
    // of the directory reaches the same figures.
    let stored = record(&run);
    assert_eq!(stored["schema_version"], 6);
    assert_eq!(&stored["results"], results);
    let replayed = replay(&run);
    assert_eq!(&replayed["by_colour"], by_colour);
    assert_eq!(replayed["telemetry"], results["telemetry"]);
}

#[test]
fn the_results_file_has_the_colours_and_the_search_of_each_engine() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let played = labelled_match(&run, 2).output().unwrap();
    assert!(
        played.status.success(),
        "{}",
        String::from_utf8_lossy(&played.stderr)
    );
    let stdout = String::from_utf8(played.stdout).unwrap();
    // The console keeps to the match's own report and names the file.
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("results: ") && line.ends_with("results.txt")),
        "{stdout}"
    );
    assert!(!stdout.contains(": as White "), "{stdout}");
    let text = std::fs::read_to_string(run.join("results.txt")).unwrap();
    for engine in ["Alpha", "Beta"] {
        assert!(
            text.contains(&format!("{engine}: as White ")),
            "{engine} has no colour line:\n{text}"
        );
        let search = text
            .lines()
            .find(|line| line.starts_with(&format!("{engine}: ")) && line.contains("annotated"))
            .unwrap_or_else(|| panic!("{engine} has no search line:\n{text}"));
        for figure in ["depth", "nodes per move", "ms per move", "nodes per second"] {
            assert!(search.contains(figure), "{figure} missing: {search}");
        }
    }
}

/// One binary run as both engines with different options and no labels: the
/// two sides keep their own search, named by side, and between them they
/// account for every engine move the run played.
#[test]
fn one_binary_with_two_option_sets_reports_two_searches() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let played = cli()
        .arg("match")
        .arg(stub())
        .arg(stub())
        .args([
            "--games",
            "4",
            "--a-engine-arg=__uci-stub",
            "--a-engine-arg=--sleep-ms=2",
            "--b-engine-arg=__uci-stub",
            "--b-engine-arg=--sleep-ms=6",
            "--a-movetime-ms",
            "50",
            "--b-movetime-ms",
            "50",
            "--max-moves",
            "4",
            "--dir",
        ])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        played.status.success(),
        "{}",
        String::from_utf8_lossy(&played.stderr)
    );
    let stderr = String::from_utf8_lossy(&played.stderr);
    assert!(
        stderr.contains("colosseum-cli (A) vs. colosseum-cli (B)"),
        "{stderr}"
    );
    let output = json(&played);
    let engines = output["results"]["telemetry"]["engines"]
        .as_array()
        .unwrap();
    let names = engines
        .iter()
        .map(|engine| engine["engine"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["colosseum-cli (A)", "colosseum-cli (B)"]);

    // Every engine move in the PGN carries one annotation; the two searches
    // split them between them, and colours alternate, so evenly.
    let pgn = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    assert!(pgn.contains("[White \"colosseum-cli (A)\"]"), "{pgn}");
    assert!(pgn.contains("[White \"colosseum-cli (B)\"]"), "{pgn}");
    let total = pgn
        .split('{')
        .skip(1)
        .filter(|comment| !comment.starts_with("book}"))
        .count() as u64;
    let moves = engines
        .iter()
        .map(|engine| count(engine, "eligible_moves"))
        .collect::<Vec<_>>();
    assert!(moves.iter().all(|&count| count > 0), "{engines:?}");
    assert_eq!(moves.iter().sum::<u64>(), total, "{engines:?}");

    let text = std::fs::read_to_string(run.join("results.txt")).unwrap();
    let searches = text
        .lines()
        .filter(|line| line.contains("annotated"))
        .count();
    assert_eq!(searches, 2, "{text}");
    // A later replay of the directory keeps them apart the same way.
    assert_eq!(
        replay(&run)["telemetry"]["engines"],
        output["results"]["telemetry"]["engines"]
    );

    // The dry run says how the two will be named before anything is played.
    let dry = cli()
        .arg("match")
        .arg(stub())
        .arg(stub())
        .args(["--games", "2", "--dry-run"])
        .output()
        .unwrap();
    assert!(dry.status.success(), "{dry:?}");
    let note = String::from_utf8_lossy(&dry.stderr);
    assert!(
        note.contains(
            "engines A and B are both named colosseum-cli; they are reported as colosseum-cli (A) and colosseum-cli (B)"
        ),
        "{note}"
    );
}

/// An SPRT counts its official pairs by colour; the pairs it finished after
/// its boundary are evidence, not sample, and stay out.
#[test]
fn an_sprt_splits_only_its_official_pairs_by_colour() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("openings.epd");
    std::fs::write(
        &book,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\n\
         rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1\n",
    )
    .unwrap();
    let run = root.path().join("run");
    let played = cli()
        .arg("sprt")
        .arg(stub())
        .arg(fixture())
        .args([
            "--max-pairs",
            "200",
            "--model",
            "normalized",
            "--elo0",
            "0",
            "--elo1",
            "100",
            "--alpha",
            "0.2",
            "--beta",
            "0.2",
            "--a-engine-arg=__uci-stub",
            "--a-movetime-ms",
            "10",
            "--b-movetime-ms",
            "10",
            "--max-engine-faults",
            "99999",
            "--max-time-losses",
            "99999",
            "--concurrency",
            "2",
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
    let output = json(&played);
    let official = output["report"]["schedule"]["official_pairs"]
        .as_array()
        .unwrap()
        .len() as u64;
    let by_colour = &output["results"]["by_colour"];
    assert_eq!(by_colour[0]["engine"], "colosseum-cli");
    assert_eq!(by_colour[1]["engine"], "colosseum-uci-fixture");
    assert_mirrored(by_colour, official * 2);
    // A pair is one game with each colour.
    assert_eq!(count(&by_colour[0]["as_white"], "games"), official);
    assert_eq!(record(&run)["results"], output["results"]);
    assert_eq!(&replay(&run)["by_colour"], by_colour);
}

/// A calibration plays one executable against itself: its two sides share a
/// name, so the colour split names them by side.
#[test]
fn a_calibration_tells_its_two_identical_sides_apart_by_side() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let played = cli()
        .arg("calibrate")
        .arg(fixture())
        .arg(fixture())
        .args([
            "--games",
            "4",
            "--confidence",
            "0.9",
            "--tolerance-nelo",
            "5",
            "--a-engine-arg=--legal-sequence",
            "--b-engine-arg=--legal-sequence",
            "--max-moves",
            "2",
            "--concurrency",
            "1",
            "--placement",
            "off",
            "--seed",
            "987654321",
            "--dir",
        ])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    // Every game is a draw, so the calibration is inconclusive by design.
    assert_eq!(played.status.code(), Some(4), "{played:?}");
    let output = json(&played);
    let by_colour = &output["results"]["by_colour"];
    assert_eq!(by_colour[0]["engine"], "colosseum-uci-fixture (A)");
    assert_eq!(by_colour[1]["engine"], "colosseum-uci-fixture (B)");
    assert_mirrored(by_colour, 4);
    assert_eq!(by_colour[0]["as_white"]["draws"], 2);
    assert_eq!(record(&run)["results"], output["results"]);
}
