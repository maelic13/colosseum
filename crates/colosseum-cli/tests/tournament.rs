use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn tournament_command(run: &Path, sleep_ms: u64) -> Command {
    let binary = Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let mut command = cli();
    command
        .args(["tournament", "run", "--engine"])
        .arg(binary)
        .arg("--engine")
        .arg(binary)
        .arg("--engine")
        .arg(binary)
        .args([
            "--label",
            "Alpha",
            "--label",
            "Beta",
            "--label",
            "Gamma",
            "--engine-arg=__uci-stub",
        ])
        .arg(format!("--engine-arg=--sleep-ms={sleep_ms}"))
        .args([
            "--games-per-pair",
            "2",
            "--max-moves",
            "1",
            "--placement",
            "off",
            "--concurrency",
            "1",
            "--anchor",
            "1",
            "--seed",
            "7",
            "--dir",
        ])
        .arg(run)
        .arg("--json");
    command
}

fn checkpoint_games(run: &Path) -> Option<usize> {
    let bytes = std::fs::read(run.join("checkpoint.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value["payload"]["games"].as_array().map(Vec::len)
}

#[test]
fn tournament_dry_run_resolves_common_and_indexed_engine_controls() {
    let output = cli()
        .args([
            "tournament",
            "run",
            "--engine",
            "missing-a",
            "--engine",
            "missing-b",
            "--option",
            "Hash=32",
            "--engine-option",
            "2:EvalFile=beta.nnue",
            "--engine-env",
            "1:RUST_LOG=warn",
            "--engine-arg-at=2:--uci",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let participants = output["resolved_configuration"]["plan"]["participants"]
        .as_array()
        .unwrap();
    assert_eq!(
        participants[0]["participant"]["launch"]["options"]["Hash"]["value"],
        "32"
    );
    assert_eq!(
        participants[1]["participant"]["launch"]["options"]["EvalFile"]["value"],
        "beta.nnue"
    );
    assert_eq!(
        participants[0]["participant"]["launch"]["environment"]["RUST_LOG"],
        "warn"
    );
    assert_eq!(
        participants[1]["participant"]["launch"]["arguments"][0],
        "--uci"
    );
}

#[test]
fn live_tournament_writes_joint_ratings_and_both_csv_exports() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("round-robin");
    let output = tournament_command(&run, 0).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["type"], "tournament");
    assert_eq!(output["report"]["status"], "completed");
    assert_eq!(output["report"]["results"]["games_scored"], 6);
    let standings = output["report"]["results"]["standings"].as_array().unwrap();
    assert_eq!(standings.len(), 3);
    assert_eq!(standings[0]["anchored"], true);
    assert_eq!(standings[0]["rating"], 1_500.0);
    assert!(standings.iter().all(|row| row["error_95"].is_number()));
    assert!(
        std::fs::read_to_string(run.join("standings.csv"))
            .unwrap()
            .starts_with("Rank,Engine,Version,Elo")
    );
    assert!(
        std::fs::read_to_string(run.join("crosstable.csv"))
            .unwrap()
            .starts_with(",Alpha,Beta,Gamma")
    );
    assert_eq!(checkpoint_games(&run), Some(6));
}

#[test]
fn killed_tournament_resumes_only_its_missing_schedule_games() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("resume");
    let mut child = tournament_command(&run, 100)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let completed_before_kill = loop {
        if let Some(completed @ 1..=5) = checkpoint_games(&run) {
            break completed;
        }
        assert!(
            Instant::now() < deadline,
            "tournament did not publish a partial checkpoint"
        );
        thread::sleep(Duration::from_millis(20));
    };
    child.kill().unwrap();
    child.wait().unwrap();

    let resumed = tournament_command(&run, 100).output().unwrap();
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let resumed: serde_json::Value = serde_json::from_slice(&resumed.stdout).unwrap();
    let games = resumed["report"]["games"].as_array().unwrap();
    assert_eq!(games.len(), 6);
    let numbers = games
        .iter()
        .map(|game| game["number"].as_u64().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(numbers.len(), 6);
    assert_eq!(checkpoint_games(&run), Some(6));
    assert!(completed_before_kill < games.len());

    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["status"], "completed");
    assert!(
        record["anomalies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|anomaly| anomaly["code"] == "run-resumed")
    );
}
