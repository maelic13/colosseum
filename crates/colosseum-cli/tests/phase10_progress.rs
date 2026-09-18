//! Progress blocks: what they say, and what makes one appear.
//!
//! The trigger is the run's own unit. The clock only ever withholds a block,
//! so a test can drive both halves of the contract without depending on how
//! fast the machine is: the stub's `--sleep-ms` sets how long a game takes,
//! and the unit counts are exact.

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
/// only from the initial position. Against the conforming stub it gives a
/// paired sample with variance, which is what an SPRT needs to report an LLR.
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

/// A match of `games` games where each game takes about `sleep_ms` times the
/// move cap, so the duration is the stub's and not the host's.
fn stub_match(run: &Path, games: u32, sleep_ms: u32, moves: u32, progress: &[&str]) -> Output {
    cli()
        .arg("match")
        .arg(engine())
        .arg(engine())
        .args(["--games", &games.to_string()])
        .args([
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            &format!("--a-engine-arg=--sleep-ms={sleep_ms}"),
            &format!("--b-engine-arg=--sleep-ms={sleep_ms}"),
        ])
        .args([
            "--a-movetime-ms",
            "10",
            "--b-movetime-ms",
            "10",
            "--max-moves",
            &moves.to_string(),
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

/// Crossing a unit boundary is what prints a block, and termination adds the
/// last one.
#[test]
fn a_match_prints_a_block_on_every_unit_boundary_it_crosses() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    // Eight games of four moves at 60 ms a move is about two seconds, so
    // every second boundary clears the one-second floor.
    let output = stub_match(
        &run,
        8,
        60,
        4,
        &["--progress-every", "2", "--progress-min-secs", "1"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stderr(&output);
    let blocks = blocks(&text);
    assert!(
        blocks.len() >= 2,
        "a run that crossed four boundaries printed {} blocks:\n{text}",
        blocks.len()
    );
    // Every block is a boundary or the end, never a per-game report.
    assert!(blocks.len() <= 4, "{text}");
    let last = blocks.last().unwrap();
    assert!(
        last.starts_with("progress [match]: 8/8 games (100%),"),
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
        blocks.len(),
        "{text}"
    );
}

/// The floor is the only thing the clock does: it withholds blocks, and the
/// next boundary after it expires prints one.
#[test]
fn the_floor_coalesces_boundaries_that_arrive_too_fast() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    // Every game is a boundary, and every one of them is inside the floor.
    let output = stub_match(
        &run,
        8,
        0,
        2,
        &["--progress-every", "1", "--progress-min-secs", "3600"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stderr(&output);
    let blocks = blocks(&text);
    assert_eq!(
        blocks.len(),
        1,
        "eight boundaries inside the floor printed {} blocks:\n{text}",
        blocks.len()
    );
    assert!(
        blocks[0].starts_with("progress [match]: 8/8 games (100%),"),
        "{text}"
    );
}

/// Time alone never prints a block. A run that takes seconds but never
/// reaches a boundary reports once, at the end.
#[test]
fn elapsed_time_alone_never_prints_a_block() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let output = stub_match(
        &run,
        8,
        60,
        4,
        &["--progress-every", "1000", "--progress-min-secs", "1"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stderr(&output);
    assert_eq!(blocks(&text).len(), 1, "{text}");
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
        8,
        0,
        2,
        &["--progress-every", "4", "--__stop-after-units", "4"],
    );
    assert_eq!(stopped.status.code(), Some(6), "{}", stderr(&stopped));

    let resolved = std::fs::read_to_string(run.join("resolved-config.json")).unwrap();
    assert!(
        !resolved.contains("progress"),
        "a reporting option reached the hashed configuration: {resolved}"
    );

    let resumed = stub_match(
        &run,
        8,
        0,
        2,
        &["--progress-every", "1", "--progress-min-secs", "2"],
    );
    assert!(
        resumed.status.success(),
        "the run did not resume with different reporting options: {}",
        stderr(&resumed)
    );
}

/// `status` prints the block the console last showed, and `run.log` keeps
/// every block the run ever printed.
#[test]
fn status_prints_the_last_block_and_the_log_keeps_them_all() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let output = stub_match(
        &run,
        8,
        60,
        4,
        &["--progress-every", "2", "--progress-min-secs", "1"],
    );
    assert!(output.status.success(), "{}", stderr(&output));
    let printed = blocks(&stderr(&output));

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
        shown.contains(printed.last().unwrap().trim_end()),
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
    assert_eq!(block["done"], 8);
    assert_eq!(block["total"], 8);
}

/// A sequential test reports the whole decision, not a pair count.
#[test]
fn an_sprt_block_carries_the_sample_both_models_and_the_llr() {
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
            "20",
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
            "--progress-every",
            "10",
            "--progress-min-secs",
            "1",
            "--book-wrap",
            "--book",
        ])
        .arg(&book)
        .arg("--dir")
        .arg(&run)
        // Human mode, because the closing report is what this test reads.
        .output()
        .unwrap();
    // Twenty pairs without a boundary is a capped inconclusive result.
    assert_eq!(output.status.code(), Some(4), "{}", stderr(&output));
    let text = stderr(&output);
    let last = blocks(&text).pop().expect("a final block");
    assert!(
        last.starts_with("progress [sprt]: 20/20 pairs (100%),"),
        "{last}"
    );
    for field in [
        "players",
        "games",
        "Elo",
        "nElo",
        "W/D/L",
        "Ptnml",
        "faults",
        "LLR",
        "rate",
        "time remaining",
    ] {
        assert!(last.contains(field), "{field} missing from:\n{last}");
    }
    // The pentanomial is the vector the run committed, and the LLR is stated
    // against its exact Wald bounds.
    assert!(last.contains("[0, 0, 10, 0, 10]"), "{last}");
    assert!(last.contains("in [-2.94, 2.94]"), "{last}");
    // Both estimates read as a value and a margin.
    assert!(last.contains("Elo") && last.contains("+/-"), "{last}");
    assert!(
        last.contains("colosseum-cli vs. colosseum-uci-fixture"),
        "{last}"
    );

    // The closing report names the hypotheses it tested, what it concluded,
    // and how long it took.
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(
        report.contains("SPRT [0.00, 10.00] inconclusive - the 20 pairs cap was reached"),
        "{report}"
    );
    assert!(report.contains("official sample: 20 pairs"), "{report}");
    assert!(report.contains("Finished match"), "{report}");
    assert!(report.contains("Total Time: "), "{report}");
}

/// A tournament reports who is ahead, with the error bars its own rating step
/// produced.
#[test]
fn a_tournament_block_names_the_standings_header() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("openings.epd");
    std::fs::write(
        &book,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\n\
         rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1\n\
         rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2\n",
    )
    .unwrap();
    let run = root.path().join("run");
    let output = cli()
        .args(["tournament", "run"])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args([
            "--games-per-pair",
            "2",
            "--engine-arg=__uci-stub",
            "--movetime-ms",
            "10",
            "--max-moves",
            "2",
            "--max-engine-faults",
            "999",
            "--progress-every",
            "4",
            "--progress-min-secs",
            "1",
            "--book",
        ])
        .arg(&book)
        .arg("--dir")
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let text = stderr(&output);
    let last = blocks(&text).pop().expect("a final block");
    assert!(
        last.starts_with("progress [tournament]: 6/6 games (100%),"),
        "{last}"
    );
    assert!(last.contains("standings"), "{last}");
    assert!(last.contains("+/- "), "no error bar in:\n{last}");
    assert!(last.contains("1. "), "no ranked row in:\n{last}");
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
            "4",
            "--games-per-iteration",
            "2",
            "--depth",
            "1",
            "--max-moves",
            "2",
            "--seed",
            "7",
            "--progress-every",
            "2",
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
        last.starts_with("progress [spsa]: 4/4 iterations (100%),"),
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
        report.contains("SPSA finished: 4 of 4 iterations"),
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

/// A calibration reports the paired sample it is measuring.
#[test]
fn a_calibration_block_reports_its_paired_sample() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let output = cli()
        .arg("calibrate")
        .arg(engine())
        .arg(engine())
        .args([
            "--games",
            "8",
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            "--a-movetime-ms",
            "10",
            "--b-movetime-ms",
            "10",
            "--max-moves",
            "2",
            "--progress-every",
            "2",
            "--progress-min-secs",
            "1",
            "--dir",
        ])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    // Identical stubs draw every game, which is a zero-variance sample and an
    // inconclusive calibration; the block still reports the sample.
    assert_eq!(output.status.code(), Some(4), "{}", stderr(&output));
    let text = stderr(&output);
    let last = blocks(&text).pop().expect("a final block");
    assert!(
        last.starts_with("progress [calibrate]: 4/4 pairs (100%),"),
        "{last}"
    );
    for field in [
        "players",
        "games",
        "W/D/L",
        "Ptnml",
        "faults",
        "time remaining",
    ] {
        assert!(last.contains(field), "{field} missing from:\n{last}");
    }
    assert!(last.contains("[0, 0, 4, 0, 0]"), "{last}");
    // A degenerate sample has no estimate in either model, and the block says
    // so for both rather than dropping a line a reader looks for.
    assert_eq!(last.matches("unavailable:").count(), 2, "{last}");
    assert!(last.contains("nElo"), "{last}");
}

/// A final report counts the games that ended badly; it never lists them.
#[test]
fn a_match_report_summarises_abnormal_games_instead_of_listing_them() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    // The stub runs out of canned moves and forfeits, so every game is
    // abnormal and the old per-game list would have been four lines long.
    let output = cli()
        .arg("match")
        .arg(engine())
        .arg(engine())
        .args([
            "--games",
            "4",
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
        report.contains("abnormal games: illegal move 4"),
        "{report}"
    );
    assert!(!report.contains("game 1:"), "{report}");
    assert!(!report.contains("game 4:"), "{report}");
    // Every result is still in the export beside it.
    let pgn = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    assert_eq!(pgn.matches("[Result ").count(), 4, "{pgn}");
}

/// A run that has printed no block yet says when its first is due, from the
/// schedule it recorded, rather than only that none was published.
#[test]
fn status_before_the_first_block_says_when_it_is_due() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let mut child = cli()
        .arg("match")
        .arg(engine())
        .arg(engine())
        .args([
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            "--a-engine-arg=--sleep-ms=200",
            "--b-engine-arg=--sleep-ms=200",
            "--a-movetime-ms",
            "300",
            "--b-movetime-ms",
            "300",
            "--max-moves",
            "2",
            "--games",
            "40",
            "--progress-every",
            "20",
            "--dir",
        ])
        .arg(&run)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while !run.join("run-record.json").is_file() {
        assert!(std::time::Instant::now() < deadline, "no run record");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    std::thread::sleep(std::time::Duration::from_millis(200));
    let status = cli().arg("status").arg(&run).output().unwrap();
    let _ = child.kill();
    let _ = child.wait();
    let shown = String::from_utf8_lossy(&status.stdout).into_owned();
    assert!(
        shown.contains("the first block is due once 20 games have been committed"),
        "{shown}"
    );
    assert!(
        shown.contains("no sooner than 5 s after it started"),
        "{shown}"
    );
}
