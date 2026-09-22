//! The round-trip instrument names the phase a search's time went to.
//!
//! A fixture engine is told to spend a known delay in each engine-side phase —
//! before its first `info`, between its `info` lines, and between its last
//! `info` and its `bestmove` — and to report a known search time. The harness
//! side is exercised by blocking the game task while the `bestmove` is on its
//! way. Each delay must land in its own phase, in the session's stamps, the
//! journal's per-side maxima, the PGN's `h=` and the forfeit forensic.
//!
//! Only lower bounds are asserted on measured time: a busy host can add to a
//! delay, never take from it. The phase arithmetic itself is unit-tested in
//! `colosseum-uci` with synthetic stamps.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use colosseum_uci::{EngineProcess, GoLimits, RoundTripPhase, SpawnOptions, UciPosition};
use serde_json::Value;

const FIRST_INFO_MS: u64 = 30;
const BETWEEN_INFO_MS: u64 = 40;
const AFTER_INFO_MS: u64 = 50;
const REPORTED_MS: u64 = 35;

fn fixture() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

fn phase_arguments(first_info_ms: u64, between_info_ms: u64, after_info_ms: u64) -> Vec<String> {
    vec![
        format!("--first-info-ms={first_info_ms}"),
        format!("--between-info-ms={between_info_ms}"),
        format!("--bestmove-after-info-ms={after_info_ms}"),
        format!("--report-time-ms={REPORTED_MS}"),
    ]
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// A delay the fixture slept is at least that long: timer granularity and a
/// busy host can add to it, never subtract.
fn assert_injected(label: &str, measured: Duration, injected: u64) {
    assert!(
        measured >= ms(injected.saturating_sub(2)),
        "{label}: injected {injected} ms, measured {measured:?}"
    );
}

#[test]
fn each_injected_delay_lands_in_its_own_phase() {
    // How long the game task is kept from reading once the last `info` has
    // been handed to it.
    const HARNESS_BLOCK_MS: u64 = 100;
    // One thread, so blocking it inside the `info` callback delays the game
    // task exactly as a busy runtime would, while the reader thread keeps
    // stamping each line as it arrives.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut options = SpawnOptions::new(fixture());
        options.args = phase_arguments(FIRST_INFO_MS, BETWEEN_INFO_MS, AFTER_INFO_MS);
        let mut engine = EngineProcess::spawn(options).await.unwrap();
        engine.handshake(Duration::from_secs(5)).await.unwrap();
        engine.is_ready(Duration::from_secs(5)).await.unwrap();

        let mut infos = 0;
        let output = engine
            .search(
                &UciPosition::StartPos { moves: Vec::new() },
                &GoLimits::MoveTime(ms(500)),
                Duration::from_secs(5),
                |_| {
                    infos += 1;
                    if infos == 2 {
                        std::thread::sleep(ms(HARNESS_BLOCK_MS));
                    }
                },
            )
            .await
            .unwrap();
        assert_eq!(infos, 2);
        let timing = engine.take_search_timing().expect("the search was timed");
        assert!(engine.take_search_timing().is_none(), "taken once");

        assert!(timing.phase(RoundTripPhase::GoWrite).is_some());
        assert_injected(
            "to first info",
            timing.phase(RoundTripPhase::ToFirstInfo).unwrap(),
            FIRST_INFO_MS,
        );
        assert_injected(
            "between info",
            timing.phase(RoundTripPhase::BetweenInfo).unwrap(),
            BETWEEN_INFO_MS,
        );
        let tail = timing.phase(RoundTripPhase::LastInfoToBestmove).unwrap();
        assert_injected("last info to bestmove", tail, AFTER_INFO_MS);
        // The game task took nothing off the channel for the block after the
        // last info, so the block is behind that info's stamp: in the tail if
        // the answer came late, and otherwise after the answer, where it is
        // not charged.
        let consumed = timing.phase(RoundTripPhase::BestmoveToConsumed).unwrap();
        assert!(
            tail + consumed >= ms(HARNESS_BLOCK_MS),
            "tail {tail:?}, consume delay {consumed:?}"
        );
        assert_eq!(Some(output.elapsed), timing.charged());
        assert!(output.elapsed <= timing.since_go(timing.consumed.unwrap()));

        // The engine's own account is the time on its last info line; the
        // overhead is everything charged beyond it.
        assert_eq!(timing.engine_time_ms, Some(REPORTED_MS));
        let overhead = timing.overhead_ns().unwrap();
        assert_eq!(
            overhead,
            i64::try_from(output.elapsed.as_nanos()).unwrap() - 35_000_000
        );
        let _ = engine.quit(Duration::from_secs(1)).await;
    });
}

#[test]
fn a_ponderhit_search_has_no_overhead_against_a_clock_started_at_go_ponder() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut options = SpawnOptions::new(env!("CARGO_BIN_EXE_colosseum-cli"));
        options.args = vec!["__uci-stub".into(), "--ponder-hints".into()];
        let mut engine = EngineProcess::spawn(options).await.unwrap();
        engine.handshake(Duration::from_secs(5)).await.unwrap();
        engine.is_ready(Duration::from_secs(5)).await.unwrap();
        engine
            .start_ponder(
                &UciPosition::StartPos {
                    moves: vec!["e2e4".into()],
                },
                &GoLimits::MoveTime(ms(500)),
            )
            .await
            .unwrap();
        // The stub ponders until told otherwise, so the hit can follow at once.
        engine
            .ponderhit(Duration::from_secs(5), |_| {})
            .await
            .unwrap();
        let timing = engine
            .take_search_timing()
            .expect("the ponderhit was timed");
        // The engine's clock started at `go ponder`, the charge at
        // `ponderhit`: there is no overhead to speak of, and none is claimed.
        assert!(timing.earlier_origin);
        assert!(timing.charged().is_some());
        assert_eq!(timing.overhead_ns(), None);
        let _ = engine.quit(Duration::from_secs(1)).await;
    });
}

/// One game: engine A, with `a_arguments`, plays white.
fn run_game(run: &Path, a_arguments: &[String], extra: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    command
        .args(["match", "--games", "1"])
        .arg(fixture())
        .arg(fixture())
        .arg("--a-engine-arg=--legal-sequence")
        .arg("--b-engine-arg=--legal-sequence")
        .args(
            a_arguments
                .iter()
                .map(|argument| format!("--a-engine-arg={argument}")),
        )
        .args([
            "--seed",
            "7",
            "--max-engine-faults",
            "100",
            "--json",
            "--dir",
        ])
        .arg(run)
        .args(extra);
    command.output().unwrap()
}

/// The one game of the run.
fn journalled_game(run: &Path) -> Value {
    let journal = std::fs::read_to_string(run.join("games.jsonl")).unwrap();
    let mut games = journal
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["game"].clone())
        .collect::<Vec<_>>();
    assert_eq!(games.len(), 1, "{journal}");
    let game = games.remove(0);
    assert_eq!(game["white"], "a", "{game}");
    game
}

fn millis(value: &Value) -> Duration {
    Duration::from_nanos(
        value
            .as_u64()
            .unwrap_or_else(|| panic!("not a phase in nanoseconds: {value}")),
    )
}

#[test]
fn a_match_journals_each_sides_phase_maxima_and_writes_the_overhead_beside_the_time() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let output = run_game(
        &run,
        &phase_arguments(FIRST_INFO_MS, BETWEEN_INFO_MS, AFTER_INFO_MS),
        &[
            "--a-movetime-ms",
            "1000",
            "--a-margin-ms",
            "20",
            "--b-movetime-ms",
            "1000",
            // End inside the fixture's scripted moves: its closing illegal
            // move is timed into the journal's maxima but never annotated in
            // the PGN, so when it was the slowest search the two could not
            // agree.
            "--max-moves",
            "2",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let game = journalled_game(&run);
    let a = &game["clock"]["white_round_trip"];
    assert!(a["searches"].as_u64().unwrap() > 0, "{game}");
    assert_injected(
        "to first info",
        millis(&a["to_first_info_ns"]),
        FIRST_INFO_MS,
    );
    assert_injected(
        "between info",
        millis(&a["between_info_ns"]),
        BETWEEN_INFO_MS,
    );
    assert_injected(
        "last info to bestmove",
        millis(&a["last_info_to_bestmove_ns"]),
        AFTER_INFO_MS,
    );
    // Engine B was not delayed, and is journalled on its own side.
    let b = &game["clock"]["black_round_trip"];
    assert!(b["searches"].as_u64().unwrap() > 0, "{game}");
    for phase in [
        "go_write_ns",
        "to_first_info_ns",
        "last_info_to_bestmove_ns",
    ] {
        assert!(b[phase].is_u64(), "{phase}: {game}");
    }

    // Every searched move carries h= beside t=, and h is the charged time
    // minus the 35 ms engine A reported: `t` is truncated to the millisecond
    // and `h` rounded to the nearest, so the two agree to within one.
    let pgn = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    assert!(pgn.contains("[WhiteTimeMarginMs \""), "{pgn}");
    let comments = pgn
        .split('{')
        .skip(1)
        .map(|comment| &comment[..comment.find('}').unwrap()])
        .collect::<Vec<_>>();
    let field = |comment: &str, key: &str| {
        comment
            .split_whitespace()
            .find_map(|field| field.strip_prefix(key))
            .map(|value| value.trim_end_matches("ms").parse::<i64>().unwrap())
    };
    // Engine A is white: its moves are the even-numbered comments.
    let a_overheads = comments
        .iter()
        .step_by(2)
        .map(|comment| {
            let time = field(comment, "t=").unwrap_or_else(|| panic!("no t=: {comment}"));
            let overhead = field(comment, "h=").unwrap_or_else(|| panic!("no h=: {comment}"));
            assert!((overhead - (time - 35)).abs() <= 1, "{comment}");
            overhead
        })
        .collect::<Vec<_>>();
    assert!(!a_overheads.is_empty(), "no move by engine A:\n{pgn}");

    // One rounding: the game's largest h= for engine A is its journal
    // maximum in nanoseconds, to the nearest millisecond.
    let nanos = a["overhead_ns"].as_i64().unwrap();
    assert_eq!(
        a_overheads.iter().max().copied(),
        Some((nanos + 500_000).div_euclid(1_000_000)),
        "PGN and journal round differently:\n{pgn}"
    );
}

#[test]
fn a_forfeit_forensic_prints_the_last_searches_and_when_the_late_answer_came() {
    const LATE_MS: u64 = 800;
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    // Engine A reports at once and answers 800 ms later, far past its 300 ms
    // move and 20 ms margin, and well inside the second the harness waits to
    // learn when a late answer came: its first search forfeits. Just before
    // the late answer it writes a line longer than the protocol allows; that
    // fails one read, and the wait for the answer goes on past it.
    let mut arguments = phase_arguments(0, 0, LATE_MS);
    arguments.push("--overlong-before-bestmove".into());
    let output = run_game(
        &run,
        &arguments,
        &[
            "--a-movetime-ms",
            "300",
            "--a-margin-ms",
            "20",
            "--b-movetime-ms",
            "300",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let forfeit = journalled_game(&run);
    assert_eq!(forfeit["termination"], "TimeForfeit", "{forfeit}");
    assert_eq!(forfeit["fault"]["side"], "white", "A did not forfeit");
    // The late answer was waited for and timed: the search that forfeited
    // counts, and its tail phase holds the late answer.
    let a = &forfeit["clock"]["white_round_trip"];
    assert_injected("late tail", millis(&a["last_info_to_bestmove_ns"]), LATE_MS);

    let forensic = std::fs::read_dir(run.join("failed-games"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .find(|text| text.contains("termination: TimeForfeit"))
        .expect("a forfeit forensic was written");
    assert!(forensic.contains("round trip, last"), "{forensic}");
    assert!(forensic.contains("\nslot:        0\n"), "{forensic}");
    for column in [
        "written", "1st info", "lst info", "engine t", "bestmove", "consumed", "overhead",
        "deadline",
    ] {
        assert!(forensic.contains(column), "missing {column}:\n{forensic}");
    }
    // The forfeited search's bestmove is marked as having come after the
    // deadline, 800 ms after the go.
    let late = forensic
        .lines()
        .find(|line| line.contains('!') && !line.starts_with('('))
        .unwrap_or_else(|| panic!("no late answer marked:\n{forensic}"));
    let arrived = late
        .split_whitespace()
        .find_map(|field| field.strip_suffix('!'))
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(arrived >= (LATE_MS - 2) as f64, "{late}");

    // The forfeited search played no move, but its overhead is in the PGN,
    // on the side that forfeited.
    let pgn = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    let forfeit_comment = pgn
        .split('{')
        .find_map(|comment| comment.strip_prefix("forfeit "))
        .map(|comment| &comment[..comment.find('}').unwrap()])
        .unwrap_or_else(|| panic!("no forfeit comment:\n{pgn}"));
    let overhead = forfeit_comment
        .split_whitespace()
        .find_map(|field| field.strip_prefix("h="))
        .unwrap()
        .trim_end_matches("ms")
        .parse::<u64>()
        .unwrap();
    assert!(overhead >= LATE_MS - REPORTED_MS - 2, "{forfeit_comment}");
}
