//! Progress blocks: what the commands print, log and show through `status`.
//!
//! When a block is due is `ProgressSchedule`'s unit tests, driven by instants
//! they choose, and what each command's block says is the unit test of that
//! command's block function. These cases cover only what needs a real run:
//! the final block, the log and `status` agreeing, the reporting options
//! staying out of the run identity, and each command's closing report.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn engine() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_colosseum-cli"))
}

/// An ordinary UCI executable that answers every search with `e2e4`, legal
/// only from the initial position.
fn one_move_engine() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The progress blocks in an output stream, each as its own text.
fn blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    for line in text.lines() {
        if line.starts_with("progress [") {
            blocks.push(format!("{line}\n"));
        } else if line.starts_with("  ")
            && !line.trim().is_empty()
            && let Some(block) = blocks.last_mut()
        {
            block.push_str(line);
            block.push('\n');
        }
    }
    blocks
}

/// A match of `games` games between two conforming stubs.
fn stub_match(run: &Path, games: u32, progress: &[&str]) -> Output {
    cli()
        .arg("match")
        .arg(engine())
        .arg(engine())
        .args(["--games", &games.to_string()])
        .args([
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            "--a-movetime-ms",
            "10",
            "--b-movetime-ms",
            "10",
            "--max-moves",
            "2",
            "--max-engine-faults",
            "999",
            "--max-time-losses",
            "999",
        ])
        .args(progress)
        .arg("--dir")
        .arg(run)
        .arg("--json")
        .output()
        .unwrap()
}

/// Termination prints the last block, `run.log` keeps every block the run
/// printed, and `status` prints the one the console last showed.
///
/// Whether the floor withholds a boundary block depends on how fast the games
/// ran, so the run prints one block or two; every assertion holds for either.
#[test]
fn status_prints_the_last_block_and_the_log_keeps_them_all() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let output = stub_match(
        &run,
        2,
        &["--progress-every", "1", "--progress-min-secs", "1"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stderr(&output);
    let printed = blocks(&text);
    let last = printed.last().expect("a final block");
    assert!(
        last.starts_with("progress [match]: 2/2 games (100%),"),
        "{last}"
    );
    for field in [
        "players",
        "score",
        "W/D/L",
        "Elo",
        "faults",
        "rate",
        "time remaining",
    ] {
        assert!(last.contains(field), "{field} missing from:\n{last}");
    }
    // A finished run has nothing left to wait for.
    assert!(last.contains("time remaining  0s"), "{last}");
    // One rule between blocks, and one before whatever follows them.
    assert_eq!(
        text.lines()
            .filter(|line| !line.is_empty() && line.chars().all(|c| c == '-'))
            .count(),
        printed.len(),
        "{text}"
    );

    let log = std::fs::read_to_string(run.join("run.log")).unwrap();
    let logged = log
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|event| event["event"] == "progress")
        .count();
    assert_eq!(logged, printed.len(), "{log}");

    let status = cli().arg("status").arg(&run).output().unwrap();
    assert!(status.status.success());
    let shown = String::from_utf8_lossy(&status.stdout).into_owned();
    assert!(
        shown.contains(last.trim_end()),
        "status did not print the last block:\n{shown}"
    );

    // The same block, machine-readable, for a caller that does not parse text.
    let machine = cli()
        .arg("status")
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    let record: Value = serde_json::from_slice(&machine.stdout).unwrap();
    let block = &record["record"]["progress"];
    assert_eq!(block["command"], "match", "{record}");
    assert_eq!(block["unit"], "games");
    assert_eq!(block["done"], 2);
    assert_eq!(block["total"], 2);
    // The schedule is recorded, which is what `status` reads to say when the
    // first block is due before one has been printed.
    assert_eq!(
        record["record"]["workflow"]["progress"],
        serde_json::json!({"every": 1, "unit": "games", "min_secs": 1}),
        "{record}"
    );
}

/// Both flags are diagnostics, so neither is part of the run identity and a
/// resumed run may be told to report differently.
#[test]
fn neither_progress_flag_enters_the_hashed_configuration() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    // The same conditions twice; only the reporting options differ.
    let stopped = stub_match(
        &run,
        2,
        &["--progress-every", "4", "--__stop-after-units", "1"],
    );
    assert_eq!(stopped.status.code(), Some(6), "{}", stderr(&stopped));

    let resolved = std::fs::read_to_string(run.join("resolved-config.json")).unwrap();
    assert!(
        !resolved.contains("progress"),
        "a reporting option reached the hashed configuration: {resolved}"
    );

    let resumed = stub_match(
        &run,
        2,
        &["--progress-every", "1", "--progress-min-secs", "2"],
    );
    assert!(
        resumed.status.success(),
        "the run did not resume with different reporting options: {}",
        stderr(&resumed)
    );
}

/// A capped sequential test closes with a block of its sample and a report
/// naming the hypotheses it tested, what it concluded and how long it took.
/// What the block says is `sprt_progress_block`'s unit test.
#[test]
fn a_capped_sprt_reports_its_hypotheses_verdict_and_duration() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("openings.epd");
    std::fs::write(
        &book,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\n\
         rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1\n",
    )
    .unwrap();
    let run = root.path().join("run");
    let output = cli()
        .arg("sprt")
        .arg(engine())
        .arg(one_move_engine())
        .args([
            "--max-pairs",
            "2",
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
            "--book-wrap",
            "--book",
        ])
        .arg(&book)
        .arg("--dir")
        .arg(&run)
        // Human mode, because the closing report is what this test reads.
        .output()
        .unwrap();
    // Two pairs without a boundary is a capped inconclusive result.
    assert_eq!(output.status.code(), Some(4), "{}", stderr(&output));
    let text = stderr(&output);
    let last = blocks(&text).pop().expect("a final block");
    assert!(
        last.starts_with("progress [sprt]: 2/2 pairs (100%),"),
        "{last}"
    );
    assert!(
        last.contains("colosseum-cli vs. colosseum-uci-fixture"),
        "{last}"
    );

    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        report.contains("SPRT [0.00, 10.00] inconclusive - the 2 pairs cap was reached"),
        "{report}"
    );
    assert!(report.contains("official sample: 2 pairs"), "{report}");
    assert!(report.contains("Finished match"), "{report}");
    assert!(report.contains("Total Time: "), "{report}");
}

/// A tune's console block reports where it stands and what has moved; its
/// per-iteration trajectory is recorded rather than printed.
#[test]
fn a_tune_block_reports_progress_and_records_its_trajectory() {
    let root = tempfile::tempdir().unwrap();
    let tune = root.path().join("tune.toml");
    std::fs::write(
        &tune,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    let run = root.path().join("run");
    let output = cli()
        .arg("spsa")
        .arg(engine())
        .arg("--engine-arg=__uci-stub")
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
            "--max-moves",
            "2",
            "--seed",
            "7",
            "--progress-every",
            "1",
            "--progress-min-secs",
            "1",
            "--dir",
        ])
        .arg(&run)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stderr(&output);
    let last = blocks(&text).pop().expect("a final block");
    assert!(
        last.starts_with("progress [spsa]: 1/1 iterations (100%),"),
        "{last}"
    );
    for field in ["games", "faults", "time remaining"] {
        assert!(last.contains(field), "{field} missing from:\n{last}");
    }
    assert!(
        last.trim_end()
            .lines()
            .last()
            .unwrap()
            .contains("time remaining"),
        "time remaining is the last line of:\n{last}"
    );
    // The trajectory is not on the console, and a rail line appears only
    // when a centre is at one.
    for absent in [
        "last mini-match",
        "perturbation scale",
        "schedule",
        "moved most since start",
    ] {
        assert!(!last.contains(absent), "{absent} was printed:\n{last}");
    }

    // It is in the log, under the same block.
    let log = std::fs::read_to_string(run.join("run.log")).unwrap();
    let recorded = log
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .rfind(|event| event["event"] == "progress")
        .expect("a recorded block");
    let detail = recorded["progress"]["detail"].as_array().unwrap();
    let labels = detail
        .iter()
        .map(|field| field["label"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(
        labels,
        [
            "last mini-match",
            "schedule",
            "moved most",
            "moved most since start"
        ],
        "{recorded}"
    );
    assert!(
        detail[1]["value"]
            .as_str()
            .unwrap_or_default()
            .contains("perturbation scale c"),
        "{recorded}"
    );

    // And `spsa status` prints it.
    let status = cli().args(["spsa", "status"]).arg(&run).output().unwrap();
    assert!(status.status.success(), "{}", stderr(&status));
    let shown = String::from_utf8_lossy(&status.stdout).into_owned();
    for line in ["last mini-match:", "schedule:", "moved most:"] {
        assert!(shown.contains(line), "{line} missing from:\n{shown}");
    }

    // The final report is one table and a footer, with no per-parameter list.
    let report = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        report.contains("SPSA finished: 1 of 1 iterations"),
        "{report}"
    );
    // Column widths follow the values, so compare the header's words.
    assert!(
        report
            .lines()
            .any(|line| line.split_whitespace().collect::<Vec<_>>()
                == ["parameter", "start", "tuned", "exact", "change", "range"]),
        "{report}"
    );
    assert!(
        report.contains("the values the tune ended on, rounded to whole numbers"),
        "{report}"
    );
    assert!(report.contains("  at a rail  "), "{report}");
    assert!(report.contains("tuned-options.txt"), "{report}");
    assert!(!report.contains("setoption name"), "{report}");
}

/// A fixed match's final report carries both estimates and counts the games
/// that ended badly instead of listing them. The count's wording is
/// `abnormal_games`'s unit test.
#[test]
fn a_match_report_carries_both_estimates_and_counts_its_abnormal_games() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    // The stub runs out of canned moves and forfeits, so every game is
    // abnormal.
    let output = cli()
        .arg("match")
        .arg(engine())
        .arg(engine())
        .args([
            "--games",
            "2",
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            "--a-movetime-ms",
            "10",
            "--b-movetime-ms",
            "10",
            "--max-engine-faults",
            "99",
            "--max-time-losses",
            "99",
            "--dir",
        ])
        .arg(&run)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&output.stdout).into_owned();
    // A fixed match is pair-scheduled, so its report carries both estimates.
    assert!(report.contains("nElo"), "{report}");
    assert!(report.contains("Ptnml: "), "{report}");
    assert!(
        report.contains("abnormal games: illegal move 2"),
        "{report}"
    );
    assert!(!report.contains("game 1:"), "{report}");
    // Every result is still in the export beside it.
    let pgn = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    assert_eq!(pgn.matches("[Result ").count(), 2, "{pgn}");
}
