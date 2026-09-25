//! One clean-stop case per durable command.
//!
//! A clean stop takes the same cancellation path a console interrupt takes;
//! only the trigger differs, so these cases exercise the interrupt behaviour
//! without needing a signal a test can portably deliver to one child process.
//! What each case asserts is the contract: stop launching new units, write the
//! checkpoint, record the run `cancelled`, exit with the documented code, and
//! resume to statistics identical to an uninterrupted run.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The documented exit status of a run that stopped cleanly on request.
const CANCELLED: i32 = 6;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    command.arg("--json");
    command
}

fn engine() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

/// A small ordinary UCI executable with a `Hash` option, for a tune whose
/// games are synthetic and whose engine is only probed and hashed.
fn probed_engine() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

fn json(output: std::process::Output) -> Value {
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

fn status_of(run: &Path) -> Value {
    let output = cli().arg("status").arg(run).output().unwrap();
    json(output)["record"]["status"].clone()
}

fn stub_pair(command: &mut Command) {
    command.args([
        "--a-engine-arg=__uci-stub",
        "--b-engine-arg=__uci-stub",
        "--a-movetime-ms",
        "5",
        "--b-movetime-ms",
        "5",
        "--max-moves",
        "2",
        "--seed",
        "11",
    ]);
}

fn write_book(root: &Path) -> PathBuf {
    let book = root.join("openings.epd");
    std::fs::write(
        &book,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1\n\
         rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1\n\
         rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2\n\
         rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2\n",
    )
    .unwrap();
    book
}

#[test]
fn a_fixed_match_stops_cleanly_and_resumes_to_the_uninterrupted_result() {
    let root = tempfile::tempdir().unwrap();

    let uninterrupted = root.path().join("straight");
    let mut command = cli();
    command.arg("match").arg(engine()).arg(engine());
    stub_pair(&mut command);
    let expected = json(
        command
            .args(["--games", "4", "--dir"])
            .arg(&uninterrupted)
            .output()
            .unwrap(),
    );

    let run = root.path().join("stopped");
    let mut command = cli();
    command
        .args(["--__stop-after-units", "2"])
        .arg("match")
        .arg(engine())
        .arg(engine());
    stub_pair(&mut command);
    let stopped = command
        .args(["--games", "4", "--dir"])
        .arg(&run)
        .output()
        .unwrap();
    assert_eq!(stopped.status.code(), Some(CANCELLED));
    let stopped = json(stopped);
    assert_eq!(stopped["report"]["status"], "cancelled");
    let played = stopped["report"]["games"].as_array().unwrap().len();
    assert!(
        (2..4).contains(&played),
        "a clean stop should keep the games already played and launch no more, got {played}"
    );
    assert_eq!(record(&run)["status"], "cancelled");
    assert!(
        run.join("checkpoint.json").is_file(),
        "no checkpoint written"
    );
    assert_eq!(status_of(&run), "cancelled");

    let mut command = cli();
    command.arg("match").arg(engine()).arg(engine());
    stub_pair(&mut command);
    let resumed = command
        .args(["--games", "4", "--dir"])
        .arg(&run)
        .output()
        .unwrap();
    assert!(resumed.status.success());
    let resumed = json(resumed);
    assert_eq!(resumed["report"]["status"], "completed");
    for field in ["games_attempted", "games_completed", "engine_a", "engine_b"] {
        assert_eq!(
            resumed["report"][field], expected["report"][field],
            "{field} differs from the uninterrupted run"
        );
    }
    assert_eq!(record(&run)["status"], "completed");
}

#[test]
fn an_sprt_stops_cleanly_without_claiming_a_verdict_and_resumes() {
    let root = tempfile::tempdir().unwrap();
    let book = write_book(root.path());

    let uninterrupted = root.path().join("straight");
    let mut command = cli();
    command.arg("sprt").arg(engine()).arg(engine());
    stub_pair(&mut command);
    let expected = json(
        command
            .args([
                "--preset",
                "gainer",
                "--max-pairs",
                "2",
                "--max-engine-faults",
                "1000",
                "--max-time-losses",
                "1000",
                "--book",
            ])
            .arg(&book)
            .arg("--dir")
            .arg(&uninterrupted)
            .output()
            .unwrap(),
    );

    let run = root.path().join("stopped");
    let mut command = cli();
    command
        .args(["--__stop-after-units", "1"])
        .arg("sprt")
        .arg(engine())
        .arg(engine());
    stub_pair(&mut command);
    let stopped = command
        .args([
            "--preset",
            "gainer",
            "--max-pairs",
            "2",
            "--max-engine-faults",
            "1000",
            "--max-time-losses",
            "1000",
            "--book",
        ])
        .arg(&book)
        .arg("--dir")
        .arg(&run)
        .output()
        .unwrap();
    assert_eq!(stopped.status.code(), Some(CANCELLED));
    let stopped = json(stopped);
    // A stop is not an inconclusive verdict: no boundary and no cap was reached.
    assert_eq!(stopped["report"]["status"], "cancelled");
    assert_eq!(stopped["report"]["schedule"]["cancelled"], true);
    assert_eq!(record(&run)["status"], "cancelled");
    assert_eq!(status_of(&run), "cancelled");

    let mut command = cli();
    command.arg("sprt").arg(engine()).arg(engine());
    stub_pair(&mut command);
    let resumed = json(
        command
            .args([
                "--preset",
                "gainer",
                "--max-pairs",
                "2",
                "--max-engine-faults",
                "1000",
                "--max-time-losses",
                "1000",
                "--book",
            ])
            .arg(&book)
            .arg("--dir")
            .arg(&run)
            .output()
            .unwrap(),
    );
    assert_eq!(
        resumed["report"]["schedule"]["pentanomial"], expected["report"]["schedule"]["pentanomial"],
        "resumed statistics differ from the uninterrupted run"
    );
    assert_eq!(resumed["report"]["status"], expected["report"]["status"]);
}

#[test]
fn a_calibration_stops_cleanly_rather_than_classifying_a_short_sample() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("stopped");
    let mut command = cli();
    command
        .args(["--__stop-after-units", "1"])
        .arg("calibrate")
        .arg(engine())
        .arg(engine());
    stub_pair(&mut command);
    let stopped = command
        .args(["--games", "4", "--dir"])
        .arg(&run)
        .output()
        .unwrap();
    assert_eq!(stopped.status.code(), Some(CANCELLED));
    let stopped = json(stopped);
    assert_eq!(stopped["report"]["status"], "inconclusive");
    assert!(
        stopped["report"]["statistics_unavailable"]
            .as_str()
            .unwrap()
            .contains("stopped cleanly"),
        "{stopped}"
    );
    assert_eq!(record(&run)["status"], "cancelled");
}

/// Stop a three-engine tournament of `format` after two games, resume it, and
/// compare it with the same tournament played straight through.
fn assert_tournament_stops_and_resumes(format: &[&str]) {
    let root = tempfile::tempdir().unwrap();

    let arguments = |directory: &Path| {
        let mut command = cli();
        command
            .args(["tournament", "run"])
            .args(format)
            .arg("--engine")
            .arg(engine())
            .arg("--engine")
            .arg(engine())
            .arg("--engine")
            .arg(engine())
            .args([
                "--label",
                "a",
                "--label",
                "b",
                "--label",
                "c",
                "--engine-arg=__uci-stub",
                "--movetime-ms",
                "5",
                "--max-moves",
                "2",
                "--games-per-pair",
                "2",
                "--seed",
                "5",
                "--dir",
            ])
            .arg(directory);
        command
    };

    let uninterrupted = root.path().join("straight");
    let expected = json(arguments(&uninterrupted).output().unwrap());

    let run = root.path().join("stopped");
    let mut command = arguments(&run);
    let stopped = command
        .args(["--__stop-after-units", "2"])
        .output()
        .unwrap();
    assert_eq!(stopped.status.code(), Some(CANCELLED));
    let stopped = json(stopped);
    assert_eq!(stopped["report"]["status"], "cancelled");
    assert_eq!(record(&run)["status"], "cancelled");
    assert_eq!(status_of(&run), "cancelled");

    let resumed = json(arguments(&run).output().unwrap());
    assert_eq!(resumed["report"]["status"], "completed");
    assert_eq!(
        resumed["report"]["plan"]["schedule"], expected["report"]["plan"]["schedule"],
        "the resumed run planned a different schedule"
    );
    assert_eq!(
        resumed["report"]["results"], expected["report"]["results"],
        "resumed standings differ from the uninterrupted run"
    );
    let record = record(&run);
    assert_eq!(record["status"], "completed");
    assert!(
        record["anomalies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|anomaly| anomaly["code"] == "run-resumed"),
        "the resume is not on the record: {record}"
    );
}

#[test]
fn a_tournament_stops_cleanly_and_resumes_to_the_uninterrupted_standings() {
    assert_tournament_stops_and_resumes(&[]);
}

/// Two seeds against one opponent: a multi-seed gauntlet resumes onto the
/// schedule and standings it would have reached uninterrupted.
#[test]
fn a_gauntlet_stops_cleanly_and_resumes_to_the_uninterrupted_standings() {
    assert_tournament_stops_and_resumes(&["--format", "gauntlet", "--seeds", "2"]);
}

#[test]
fn a_position_suite_stops_cleanly_and_resumes_without_repeating_a_position() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("positions.epd");
    std::fs::write(
        &input,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1 ; id \"one\"\n\
         rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq - 0 2 ; id \"two\"\n\
         rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2 ; id \"three\"\n\
         r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3 ; id \"four\"\n",
    )
    .unwrap();

    let arguments = |directory: &Path| {
        let mut command = cli();
        command
            .arg("suite")
            .arg(engine())
            .arg("--engine-arg=__uci-stub")
            .arg(&input)
            .args(["--movetime-ms", "5", "--dir"])
            .arg(directory);
        command
    };

    let run = root.path().join("stopped");
    let mut command = arguments(&run);
    let stopped = command
        .args(["--__stop-after-units", "2"])
        .output()
        .unwrap();
    // Every driver that writes `cancelled` exits with the same code, so a
    // script does not have to know which command it interrupted.
    assert_eq!(
        stopped.status.code(),
        Some(CANCELLED),
        "a cancelled suite must exit like every other cancelled run"
    );
    let stopped_value = json(stopped);
    let searched = stopped_value["report"]["results"].as_array().unwrap().len();
    assert!(
        searched < 4,
        "the suite did not stop early: {stopped_value}"
    );
    assert_eq!(record(&run)["status"], "cancelled");
    assert_eq!(status_of(&run), "cancelled");

    let resumed = json(arguments(&run).output().unwrap());
    let results = resumed["report"]["results"].as_array().unwrap();
    assert_eq!(results.len(), 4);
    let indexes = results
        .iter()
        .map(|result| result["index"].as_u64().unwrap())
        .collect::<Vec<_>>();
    let mut unique = indexes.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(indexes.len(), unique.len(), "a position was searched twice");
    assert_eq!(record(&run)["status"], "completed");
}

#[test]
fn an_spsa_tune_stops_cleanly_between_iterations_and_resumes_the_schedule() {
    let root = tempfile::tempdir().unwrap();
    let tune = root.path().join("tune.toml");
    std::fs::write(
        &tune,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();

    let arguments = |directory: &Path| {
        let mut command = cli();
        command
            .arg("spsa")
            .arg(probed_engine())
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
                // Instant in-process games: the stop and the resumed schedule
                // are the driver's, not the engines'.
                "--__synthetic-games",
                "--dir",
            ])
            .arg(directory);
        command
    };

    let uninterrupted = root.path().join("straight");
    let expected = json(arguments(&uninterrupted).output().unwrap());

    let run = root.path().join("stopped");
    let mut command = arguments(&run);
    let stopped = command
        .args(["--__stop-after-units", "1"])
        .output()
        .unwrap();
    assert_eq!(stopped.status.code(), Some(CANCELLED));
    let stopped = json(stopped);
    assert_eq!(stopped["report"]["driver"]["status"], "cancelled");
    assert_eq!(
        stopped["report"]["driver"]["completed_iterations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(record(&run)["status"], "cancelled");
    assert_eq!(status_of(&run), "cancelled");

    let resumed = json(arguments(&run).output().unwrap());
    assert_eq!(resumed["report"]["driver"]["status"], "completed");
    assert_eq!(
        resumed["report"]["driver"]["final_centers"], expected["report"]["driver"]["final_centers"],
        "a resumed tune must land where the uninterrupted one did"
    );
    assert_eq!(
        resumed["report"]["tuned_result"]["parameters"],
        expected["report"]["tuned_result"]["parameters"]
    );
}

fn log_events(run: &Path, event: &str) -> Vec<Value> {
    std::fs::read_to_string(run.join("run.log"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .filter(|line| line["event"] == event)
        .collect()
}

/// A resumed run says where it stood — on stderr, in `--json` mode too, and in
/// the JSON value — and `run.log` records where one invocation stopped and the
/// next resumed.
#[test]
fn a_resumed_match_names_its_completed_and_remaining_games_and_logs_the_stop_and_the_resume() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let invoke = |stop: Option<&str>| {
        let mut command = cli();
        if let Some(units) = stop {
            command.args(["--__stop-after-units", units]);
        }
        command.arg("match").arg(engine()).arg(engine());
        stub_pair(&mut command);
        command
            .args(["--games", "4", "--dir"])
            .arg(&run)
            .output()
            .unwrap()
    };

    let stopped = invoke(Some("2"));
    assert_eq!(stopped.status.code(), Some(CANCELLED));
    let stopped = json(stopped);
    // A fresh run carries no resume facts at all.
    assert!(stopped.get("resume").is_none(), "{stopped}");
    let played = stopped["report"]["games"].as_array().unwrap().len() as u64;
    let stops = log_events(&run, "stopped");
    assert_eq!(stops.len(), 1, "{stops:?}");
    assert_eq!(stops[0]["unit"], "games");
    assert_eq!(stops[0]["completed_units"], played);
    assert_eq!(stops[0]["exit_code"], CANCELLED);
    assert!(log_events(&run, "resumed").is_empty());

    let resumed = invoke(None);
    assert!(resumed.status.success());
    let note = String::from_utf8_lossy(&resumed.stderr).into_owned();
    let expected = format!(
        "resuming: {played} of 4 games complete, {} to play",
        4 - played
    );
    assert!(note.contains(&expected), "{note}");
    let value = json(resumed);
    assert_eq!(value["resume"]["unit"], "games");
    assert_eq!(value["resume"]["completed_units"], played);
    assert_eq!(value["resume"]["remaining_units"], 4 - played);
    let resumes = log_events(&run, "resumed");
    assert_eq!(resumes.len(), 1, "{resumes:?}");
    assert_eq!(resumes[0]["completed_units"], played);
    assert_eq!(resumes[0]["remaining_units"], 4 - played);
}

/// Once a tune has been stopped and resumed, `spsa status` still has the
/// tune's whole elapsed time to estimate from.
#[test]
fn a_resumed_tune_keeps_a_finite_eta_in_spsa_status() {
    let root = tempfile::tempdir().unwrap();
    let tune = root.path().join("tune.toml");
    std::fs::write(
        &tune,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    let run = root.path().join("run");
    let stopped_after = |units: &str| {
        cli()
            .args(["--__stop-after-units", units])
            .arg("spsa")
            .arg(probed_engine())
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
                "--__synthetic-games",
                "--dir",
            ])
            .arg(&run)
            .output()
            .unwrap()
    };
    assert_eq!(stopped_after("1").status.code(), Some(CANCELLED));
    assert_eq!(stopped_after("1").status.code(), Some(CANCELLED));
    assert_eq!(log_events(&run, "resumed").len(), 1);

    let status = cli().args(["spsa", "status"]).arg(&run).output().unwrap();
    let status = json(status);
    let eta = &status["report"]["diagnostics"]["eta"];
    assert!(
        eta["remaining_seconds"]
            .as_f64()
            .is_some_and(f64::is_finite),
        "{eta}"
    );
}
