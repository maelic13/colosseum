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
    // Book moves, both score signs, both mate signs, and a move whose engine
    // reported no node count so the field is absent rather than zero.
    assert!(ANNOTATED.contains("1. e4 {book} e5 {book}"));
    assert!(ANNOTATED.contains("{s=24 d=14 t=97ms n=1204513}"));
    assert!(ANNOTATED.contains("{s=-18 d=15 t=103ms n=1550922}"));
    assert!(ANNOTATED.contains("{s=31 d=13 t=88ms}"));
    assert!(ANNOTATED.contains("{s=#1 d=6 t=12ms n=41233}"));
    assert!(ANNOTATED.contains("{s=#-1 d=12 t=74ms n=901233}"));
    assert!(!ANNOTATED.contains("n=0"));
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
    // Alpha searched Bc4, Qh5, Qxf7# as White and Qh4# as Black.
    assert_eq!(alpha["eligible_moves"], 4);
    assert_eq!(alpha["annotation_coverage"], 1.0);
    assert_eq!(alpha["score_cp"]["coverage"], 1.0);
    // Two of Alpha's four scores are mates, which are counted as covered but
    // excluded from the centipawn values.
    assert_eq!(alpha["score_cp"]["samples"], 2);
    assert_eq!(alpha["score_cp"]["mean"], 27.5);
    assert_eq!(alpha["mean_absolute_score_cp"]["mean"], 27.5);
    // Qh5 reported no nodes, so nodes are covered for three of four moves.
    assert_eq!(alpha["nodes"]["samples"], 3);
    assert_eq!(alpha["nodes"]["coverage"], 0.75);
    assert_eq!(alpha["depth"]["coverage"], 1.0);
    assert_eq!(alpha["elapsed_seconds"]["coverage"], 1.0);

    let beta = engines
        .iter()
        .find(|engine| engine["engine"] == "Beta 2.3.1")
        .unwrap();
    assert_eq!(beta["score_cp"]["mean"], -210.0);
    assert_eq!(beta["mean_absolute_score_cp"]["mean"], 210.0);
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
