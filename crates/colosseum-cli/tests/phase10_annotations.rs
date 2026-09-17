//! Per-move search annotations in `games.pgn`, and what reads them back.

use std::path::Path;
use std::process::Command;

use colosseum_core::{OpeningBook, OpeningFormat, OpeningOrder};
use colosseum_engine::summarize;

/// The committed specimen of the writer form. Freezing it means a change to
/// the comment shape has to be a deliberate edit here, not a silent drift that
/// breaks every reader a user already wrote.
const ANNOTATED: &str = include_str!("../../../tests/fixtures/annotated-games.pgn");

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn stub_match(directory: &Path, book: Option<&Path>) -> serde_json::Value {
    let binary = Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let mut command = cli();
    command.arg("match").arg(binary).arg(binary).args([
        "--games",
        "2",
        "--a-engine-arg=__uci-stub",
        "--a-engine-arg=--sleep-ms=5",
        "--b-engine-arg=__uci-stub",
        "--b-engine-arg=--sleep-ms=5",
        "--a-movetime-ms",
        "50",
        "--b-movetime-ms",
        "50",
        "--max-moves",
        "8",
    ]);
    if let Some(book) = book {
        command.arg("--book").arg(book).args(["--book-plies", "4"]);
    }
    let output = command
        .arg("--dir")
        .arg(directory)
        .arg("--json")
        .output()
        .unwrap();
    serde_json::from_slice(&output.stdout).unwrap_or(serde_json::Value::Null)
}

#[test]
fn a_stub_match_annotates_every_post_opening_move_and_marks_book_moves() {
    let root = tempfile::tempdir().unwrap();
    let book_path = root.path().join("book.pgn");
    std::fs::write(&book_path, "1. e4 e5 2. Nf3 Nc6 *\n\n1. d4 d5 2. c4 e6 *\n").unwrap();
    stub_match(&root.path().join("run"), Some(&book_path));

    let pgn = std::fs::read_to_string(root.path().join("run/games.pgn")).unwrap();
    assert!(pgn.contains("[OpeningPlyCount \"4\"]"), "{pgn}");
    assert_eq!(pgn.matches("{book}").count(), 8, "{pgn}");
    // Every engine move carries score, depth, charged time and nodes.
    for comment in pgn
        .split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(body, _)| body))
        .filter(|body| *body != "book")
    {
        for field in ["s=", "d=", "t=", "n="] {
            assert!(
                comment.contains(field),
                "{field} missing from {{{comment}}}"
            );
        }
        assert!(comment.contains("ms"), "{{{comment}}} has no time unit");
    }
}

#[test]
fn stats_replay_of_an_annotated_match_reports_full_coverage() {
    let root = tempfile::tempdir().unwrap();
    stub_match(&root.path().join("run"), None);
    let pgn = root.path().join("run/games.pgn");

    let output = cli().arg("stats").arg(&pgn).arg("--json").output().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let engines = value["report"]["telemetry"]["engines"].as_array().unwrap();
    assert!(!engines.is_empty(), "{value}");
    for engine in engines {
        assert_eq!(engine["annotation_coverage"], 1.0, "{engine}");
        for metric in [
            "score_cp",
            "mean_absolute_score_cp",
            "depth",
            "elapsed_seconds",
            "nodes",
        ] {
            assert_eq!(engine[metric]["coverage"], 1.0, "{metric} in {engine}");
        }
    }
}

#[test]
fn the_frozen_fixture_keeps_the_documented_comment_form() {
    // Book moves, both score signs, both mate signs, a move whose engine
    // reported no node count so the field is absent, and a move that reported
    // depth and nodes as zero so the two cases stay distinguishable.
    assert!(ANNOTATED.contains("1. e4 {book} e5 {book}"));
    assert!(ANNOTATED.contains("{s=24 d=14 t=97ms n=1204513}"));
    assert!(ANNOTATED.contains("{s=-18 d=15 t=103ms n=1550922}"));
    assert!(ANNOTATED.contains("{s=30 d=13 t=88ms}"));
    assert!(ANNOTATED.contains("{s=#1 d=6 t=12ms n=41233}"));
    assert!(ANNOTATED.contains("{s=#-1 d=9 t=31ms n=210044}"));
    assert!(ANNOTATED.contains("{s=18 d=0 t=1ms n=0}"));
}

#[test]
fn the_frozen_fixture_carries_its_schedule_identity() {
    // One complete colour-reversed pair: the same opening played both ways.
    assert_eq!(ANNOTATED.matches("[PairNumber \"1\"]").count(), 2);
    assert!(ANNOTATED.contains("[GameNumber \"1\"]"));
    assert!(ANNOTATED.contains("[GameNumber \"2\"]"));
    assert!(ANNOTATED.contains("[PairGame \"1\"]"));
    assert!(ANNOTATED.contains("[PairGame \"2\"]"));
    assert_eq!(ANNOTATED.matches("[OpeningIndex \"0\"]").count(), 2);
    assert_eq!(ANNOTATED.matches("[OpeningLabel \"e4 e5\"]").count(), 2);
}

#[test]
fn the_telemetry_parser_reads_the_frozen_fixture() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("annotated-games.pgn");
    std::fs::write(&path, ANNOTATED).unwrap();

    let output = cli()
        .arg("stats")
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let telemetry = &value["report"]["telemetry"];
    assert_eq!(telemetry["status"], "available", "{value}");
    // Four book plies over two games are excluded from search telemetry.
    assert_eq!(telemetry["excluded_opening_moves"], 4);

    let engines = telemetry["engines"].as_array().unwrap();
    let alpha = engines
        .iter()
        .find(|engine| engine["engine"] == "Alpha 1.0.0")
        .unwrap();
    // Alpha searched three moves as White and two as Black.
    assert_eq!(alpha["eligible_moves"], 5);
    assert_eq!(alpha["annotation_coverage"], 1.0);
    assert_eq!(alpha["score_cp"]["coverage"], 1.0);
    // Two of Alpha's five scores are mates, counted as covered but excluded
    // from the centipawn values.
    assert_eq!(alpha["score_cp"]["samples"], 3);
    assert_eq!(alpha["score_cp"]["mean"], 14.0);
    assert_eq!(alpha["mean_absolute_score_cp"]["mean"], 22.0);
    // One Alpha move reported no nodes, so that field is covered for four of
    // five; depth and time are complete.
    assert_eq!(alpha["nodes"]["samples"], 4);
    assert_eq!(alpha["nodes"]["coverage"], 0.8);
    assert_eq!(alpha["depth"]["coverage"], 1.0);
    assert_eq!(alpha["elapsed_seconds"]["coverage"], 1.0);

    let beta = engines
        .iter()
        .find(|engine| engine["engine"] == "Beta 2.3.1")
        .unwrap();
    assert_eq!(beta["score_cp"]["mean"], -89.5);
    assert_eq!(beta["mean_absolute_score_cp"]["mean"], 120.5);
    // One Beta move reported depth 0 and 0 nodes. Those are reports, not
    // missing fields, so both stay fully covered.
    assert_eq!(beta["depth"]["samples"], 5);
    assert_eq!(beta["depth"]["coverage"], 1.0);
    assert_eq!(beta["nodes"]["samples"], 5);
    assert_eq!(beta["nodes"]["coverage"], 1.0);
}

/// A PGN alone reproduces the pentanomial vector, because it says which pair
/// each game belongs to and which colour assignment it is.
#[test]
fn the_frozen_fixture_replays_as_one_complete_pair() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("annotated-games.pgn");
    std::fs::write(&path, ANNOTATED).unwrap();

    let output = cli()
        .arg("stats")
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let report = &value["report"];
    assert_eq!(report["pairing"], "paired");
    assert_eq!(report["complete_pairs"], 1);
    assert_eq!(report["unpaired_games"], 0);
    // Alpha won its White game and lost the reversed one: one point of two.
    assert_eq!(report["pentanomial"], serde_json::json!([0, 0, 1, 0, 0]));
}

#[test]
fn the_workspace_pgn_reader_still_reads_the_frozen_fixture_as_openings() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("annotated-games.pgn");
    std::fs::write(&path, ANNOTATED).unwrap();

    let book = OpeningBook {
        path,
        format: OpeningFormat::Pgn,
        order: OpeningOrder::Sequential,
        plies: 4,
        count: None,
        seed: 0,
    };
    let summary = summarize(&book).unwrap();
    assert_eq!(summary.count, 2);
    // Comments never leak into the replayed opening line.
    assert!(
        !summary
            .first_label
            .clone()
            .unwrap_or_default()
            .contains('{'),
        "{summary:?}"
    );
}

#[test]
fn the_run_record_names_the_writer_form() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    stub_match(&run, None);
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(
        record["workflow"]["pgn_annotation_writer"],
        colosseum_engine::pgn::PGN_ANNOTATION_WRITER
    );
}

/// A run directory is one evidence set: the checkpoint is the authority for
/// statistics and its own PGN supplies the telemetry, and replaying that PGN
/// on its own must reach the same pentanomial vector.
#[test]
fn a_run_directory_and_its_pgn_replay_to_the_same_statistics() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let binary = Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let played = cli()
        .arg("match")
        .arg(binary)
        .arg(binary)
        .args([
            "--games",
            "6",
            "--a-engine-arg=__uci-stub",
            "--a-engine-arg=--sleep-ms=2",
            "--b-engine-arg=__uci-stub",
            "--b-engine-arg=--sleep-ms=2",
            "--a-movetime-ms",
            "50",
            "--b-movetime-ms",
            "50",
            "--max-moves",
            "2",
            "--max-engine-faults",
            "99",
            "--max-time-losses",
            "99",
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

    let replay = |target: &Path| -> serde_json::Value {
        let output = cli()
            .arg("stats")
            .arg(target)
            .arg("--json")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["report"].clone()
    };

    let directory = replay(&run);
    let pgn = replay(&run.join("games.pgn"));

    assert_eq!(directory["authority"], "structured-run-store");
    assert_eq!(pgn["authority"], "pgn-export");
    for field in ["pentanomial", "complete_pairs", "unpaired_games", "games"] {
        assert_eq!(
            directory[field], pgn[field],
            "{field} differs between the checkpoint and its own PGN"
        );
    }
    assert_eq!(directory["complete_pairs"], 3);

    // Reading the checkpoint is no reason to lose the annotations beside it.
    for (name, report) in [("run directory", &directory), ("PGN", &pgn)] {
        let engines = report["telemetry"]["engines"].as_array().unwrap();
        assert!(!engines.is_empty(), "{name} reported no telemetry engines");
        for engine in engines {
            for metric in [
                "score_cp",
                "mean_absolute_score_cp",
                "depth",
                "elapsed_seconds",
                "nodes",
            ] {
                assert_eq!(
                    engine[metric]["coverage"], 1.0,
                    "{name}: {metric} is not fully covered"
                );
            }
        }
    }
}
