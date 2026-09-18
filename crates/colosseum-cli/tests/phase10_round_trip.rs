//! The round-trip instrument names the phase a search's time went to.
//!
//! A fixture engine is told to spend a known delay in each engine-side phase —
//! before its first `info`, between its `info` lines, and between its last
//! `info` and its `bestmove` — and to report a known search time. The harness
//! side is exercised by keeping the game task busy while the `bestmove`
//! arrives. Each delay must land in its own phase and nowhere else, in the
//! session's stamps, the journal's per-side maxima, the PGN's `h=` and the
//! forfeit forensic.

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

fn phase_arguments(after_info_ms: u64) -> Vec<String> {
    vec![
        format!("--first-info-ms={FIRST_INFO_MS}"),
        format!("--between-info-ms={BETWEEN_INFO_MS}"),
        format!("--bestmove-after-info-ms={after_info_ms}"),
        format!("--report-time-ms={REPORTED_MS}"),
    ]
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// A delay the fixture slept is at least that long, and not wildly more:
/// timer granularity and a busy test host can add, never subtract.
fn assert_injected(label: &str, measured: Duration, injected: u64) {
    assert!(
        measured >= ms(injected.saturating_sub(2)) && measured < ms(injected + 250),
        "{label}: injected {injected} ms, measured {measured:?}"
    );
}

#[test]
fn each_injected_delay_lands_in_its_own_phase() {
    // One thread, so a task that blocks it delays the game task exactly as a
    // busy runtime would, while the reader thread keeps stamping.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let mut options = SpawnOptions::new(fixture());
        options.args = phase_arguments(AFTER_INFO_MS);
        let mut engine = EngineProcess::spawn(options).await.unwrap();
        engine.handshake(Duration::from_secs(5)).await.unwrap();
        engine.is_ready(Duration::from_secs(5)).await.unwrap();

        // The bestmove arrives about 120 ms after `go`; from 100 ms to 200 ms
        // the game task's only thread is blocked, so it takes the line late.
        let busy = tokio::spawn(async {
            tokio::time::sleep(ms(100)).await;
            std::thread::sleep(ms(100));
        });
        let output = engine
            .search(
                &UciPosition::StartPos { moves: Vec::new() },
                &GoLimits::MoveTime(ms(500)),
                Duration::from_secs(5),
                |_| {},
            )
            .await
            .unwrap();
        busy.await.unwrap();
        let timing = engine.take_search_timing().expect("the search was timed");
        assert!(engine.take_search_timing().is_none(), "taken once");

        assert!(timing.phase(RoundTripPhase::GoWrite).unwrap() < ms(50));
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
        assert_injected(
            "last info to bestmove",
            timing.phase(RoundTripPhase::LastInfoToBestmove).unwrap(),
            AFTER_INFO_MS,
        );
        // The busy task held the thread until about 200 ms; the answer came
        // at about 120 ms. That wait is the harness's, and it is not charged.
        let consumed = timing.phase(RoundTripPhase::BestmoveToConsumed).unwrap();
        assert!(consumed >= ms(30), "consume delay measured {consumed:?}");
        assert_eq!(Some(output.elapsed), timing.charged());
        assert!(output.elapsed < timing.since_go(timing.consumed.unwrap()));

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

fn run_match(run: &Path, extra: &[&str]) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    command
        .args(["match", "--games", "2"])
        .arg(fixture())
        .arg(fixture())
        .arg("--a-engine-arg=--legal-sequence")
        .arg("--b-engine-arg=--legal-sequence")
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

fn journal(run: &Path) -> Vec<Value> {
    std::fs::read_to_string(run.join("games.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["game"].clone())
        .collect()
}

fn round_trip<'a>(game: &'a Value, side: &str) -> &'a Value {
    let colour = if game["white"] == side {
        "white"
    } else {
        "black"
    };
    &game["clock"][format!("{colour}_round_trip")]
}

fn millis(value: &Value) -> Duration {
    Duration::from_nanos(value.as_u64().unwrap())
}

#[test]
fn a_match_journals_each_sides_phase_maxima_and_writes_the_overhead_beside_the_time() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let phases = phase_arguments(AFTER_INFO_MS)
        .into_iter()
        .map(|argument| format!("--a-engine-arg={argument}"))
        .collect::<Vec<_>>();
    let mut extra = phases.iter().map(String::as_str).collect::<Vec<_>>();
    // A 20 ms margin for A: its roughly 85 ms of unreported time per move
    // would have forfeited any move played on the last of its clock.
    extra.extend([
        "--a-movetime-ms",
        "1000",
        "--a-margin-ms",
        "20",
        "--b-movetime-ms",
        "1000",
    ]);
    let output = run_match(&run, &extra);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    for game in journal(&run) {
        let a = round_trip(&game, "a");
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
        // Engine B was not delayed: its phases are all short.
        let b = round_trip(&game, "b");
        assert!(millis(&b["go_write_ns"]) < ms(50), "{game}");
        assert!(millis(&b["to_first_info_ns"]) < ms(20), "{game}");
        assert!(millis(&b["last_info_to_bestmove_ns"]) < ms(20), "{game}");
    }

    // Every searched move carries h= beside t=, and h is t minus the 35 ms
    // engine A reported.
    let pgn = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    assert!(pgn.contains("[WhiteTimeMarginMs \""), "{pgn}");
    let mut checked = 0;
    for comment in pgn.split('{').skip(1) {
        let comment = &comment[..comment.find('}').unwrap()];
        let field = |key: &str| {
            comment
                .split_whitespace()
                .find_map(|field| field.strip_prefix(key))
                .map(|value| value.trim_end_matches("ms").parse::<i64>().unwrap())
        };
        if let (Some(time), Some(overhead)) = (field("t="), field("h="))
            && time > 100
        {
            assert_eq!(overhead, time - 35, "{comment}");
            checked += 1;
        }
    }
    assert!(checked > 0, "no delayed move carried h=:\n{pgn}");

    // `stats` reads the h= back into a distribution per engine.
    let stats = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .args(["stats", "--json"])
        .arg(&run)
        .output()
        .unwrap();
    let value: Value = serde_json::from_slice(&stats.stdout).unwrap();
    let engines = value["report"]["telemetry"]["engines"].as_array().unwrap();
    let delayed = engines
        .iter()
        .map(|engine| &engine["harness_overhead_ms"])
        .max_by(|left, right| {
            left["p50"]
                .as_f64()
                .unwrap()
                .total_cmp(&right["p50"].as_f64().unwrap())
        })
        .unwrap();
    // Engine A's overhead is its unreported start and tail: at least the 30
    // ms before its first info and the 50 ms after its last.
    assert!(delayed["p50"].as_f64().unwrap() >= 78.0, "{delayed}");
    assert!(delayed["max"].as_f64().unwrap() >= delayed["p999"].as_f64().unwrap());
    // Every one of A's moves is over its 20 ms margin, and says so.
    assert!(delayed["moves_with_margin"].as_u64().unwrap() > 0);
    assert_eq!(delayed["over_margin"], delayed["moves_with_margin"]);
    let human = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .arg("stats")
        .arg(&run)
        .output()
        .unwrap();
    let human = String::from_utf8_lossy(&human.stdout);
    assert!(human.contains("harness overhead p50"), "{human}");
    assert!(human.contains("moves over the time margin"), "{human}");
}

#[test]
fn a_forfeit_forensic_prints_the_last_searches_and_when_the_late_answer_came() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    // Engine A answers 300 ms after its last info, far past a 100 ms move
    // with a 20 ms margin: its first search forfeits.
    let phases = phase_arguments(300)
        .into_iter()
        .map(|argument| format!("--a-engine-arg={argument}"))
        .collect::<Vec<_>>();
    let mut extra = phases.iter().map(String::as_str).collect::<Vec<_>>();
    extra.extend([
        "--a-movetime-ms",
        "100",
        "--a-margin-ms",
        "20",
        "--b-movetime-ms",
        "100",
    ]);
    let output = run_match(&run, &extra);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let forfeit = journal(&run)
        .into_iter()
        .find(|game| game["termination"] == "TimeForfeit")
        .expect("engine A forfeited");
    // The late answer was waited for and timed: the search that forfeited
    // counts, and its tail phase holds the 300 ms.
    let a = round_trip(&forfeit, "a");
    assert_injected("late tail", millis(&a["last_info_to_bestmove_ns"]), 300);

    let forensic = std::fs::read_dir(run.join("failed-games"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .find(|text| text.contains("termination: TimeForfeit"))
        .expect("a forfeit forensic was written");
    assert!(forensic.contains("round trip, last"), "{forensic}");
    for column in [
        "written", "1st info", "lst info", "engine t", "bestmove", "consumed", "overhead",
        "deadline",
    ] {
        assert!(forensic.contains(column), "missing {column}:\n{forensic}");
    }
    // The forfeited search's bestmove is marked as having come after the
    // deadline, at about 370 ms.
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
    assert!(arrived >= 360.0, "{late}");
}
