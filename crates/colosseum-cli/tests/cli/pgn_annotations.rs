//! Per-move search annotations in `games.pgn`, and what reads them back.
//!
//! The comment form itself and how `stats` reads the frozen fixture are
//! unit-tested beside the writer (`colosseum_engine::pgn`) and the readers
//! (`pgn_telemetry`, `stats_replay`).

use std::path::Path;
use std::process::Command;

use colosseum_core::{OpeningBook, OpeningFormat, OpeningOrder};
use colosseum_engine::summarize;

/// The committed specimen of the writer form. Freezing it means a change to
/// the comment shape has to be a deliberate edit here, not a silent drift that
/// breaks every reader a user already wrote.
const ANNOTATED: &str = include_str!("../../../../tests/fixtures/annotated-games.pgn");

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

#[test]
fn a_stub_match_annotates_every_post_opening_move_and_marks_book_moves() {
    let root = tempfile::tempdir().unwrap();
    let book_path = root.path().join("book.pgn");
    std::fs::write(&book_path, "1. e4 e5 2. Nf3 Nc6 *\n\n1. d4 d5 2. c4 e6 *\n").unwrap();
    let run = root.path().join("run");
    let binary = Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let played = cli()
        .arg("match")
        .arg(binary)
        .arg(binary)
        .args([
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
            "--book",
        ])
        .arg(&book_path)
        .args(["--book-plies", "4", "--dir"])
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

    // The run record names the comment form it was written in.
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(
        record["workflow"]["pgn_annotation_writer"],
        colosseum_engine::pgn::PGN_ANNOTATION_WRITER
    );
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
            "4",
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
    assert_eq!(directory["complete_pairs"], 2);

    // Reading the checkpoint is no reason to lose the annotations beside it,
    // and every engine move of the PGN is annotated.
    for (name, report) in [("run directory", &directory), ("PGN", &pgn)] {
        let engines = report["telemetry"]["engines"].as_array().unwrap();
        assert!(!engines.is_empty(), "{name} reported no telemetry engines");
        for engine in engines {
            assert_eq!(
                engine["annotation_coverage"], 1.0,
                "{name}: annotation coverage in {engine}"
            );
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
