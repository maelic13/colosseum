use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

/// The documented exit status of a run that stopped cleanly on request.
const CANCELLED: i32 = 6;

fn fixture() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

/// A calibration of the fixture against itself. Both sides play the same
/// legal line to the move cap, so every game is a draw and the run has zero
/// variance: inconclusive by construction.
fn calibration(run: &Path, tolerance_nelo: f64, stop_after_units: Option<u64>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    if let Some(units) = stop_after_units {
        command.args(["--__stop-after-units", &units.to_string()]);
    }
    command
        .arg("calibrate")
        .arg(fixture())
        .arg(fixture())
        .args([
            "--games",
            "4",
            "--confidence",
            "0.9",
            "--tolerance-nelo",
            &tolerance_nelo.to_string(),
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
        .arg(run)
        .arg("--json");
    command
}

fn json(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid JSON ({error}); stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn read_json(path: PathBuf) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn stopped_calibration_resumes_exact_configuration_and_refuses_mismatch() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("calibration");
    let stopped = calibration(&run, 12.5, Some(1)).output().unwrap();
    assert_eq!(
        stopped.status.code(),
        Some(CANCELLED),
        "stderr: {}",
        String::from_utf8_lossy(&stopped.stderr)
    );
    let played = json(&stopped)["report"]["fixed_match"]["games_attempted"]
        .as_u64()
        .unwrap();
    assert!((1..4).contains(&played), "stopped after {played} games");

    let resumed = calibration(&run, 12.5, None).output().unwrap();
    assert_eq!(
        resumed.status.code(),
        Some(4),
        "stderr: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let output = json(&resumed);
    assert_eq!(output["report"]["status"], "inconclusive");
    assert_eq!(output["report"]["fixed_match"]["games_attempted"], 4);
    assert_eq!(output["report"]["design"]["confidence"], 0.9);
    assert_eq!(output["report"]["design"]["tolerance_nelo"], 12.5);

    let resolved = read_json(run.join("resolved-config.json"));
    let record = read_json(run.join("run-record.json"));
    let result = read_json(run.join("result.json"));
    let config_sha256 = std::fs::read_to_string(run.join("config.sha256")).unwrap();
    assert_eq!(resolved["design"], output["report"]["design"]);
    assert_eq!(resolved["binaries"], output["report"]["binaries"]);
    assert_eq!(result["design"], resolved["design"]);
    assert_eq!(result["binaries"], resolved["binaries"]);
    assert_eq!(
        record["config_sha256"],
        config_sha256.split_whitespace().next().unwrap()
    );
    assert_eq!(record["workflow"]["design"], resolved["design"]);
    assert_eq!(record["workflow"]["binaries"], resolved["binaries"]);
    assert_eq!(record["official_sample"]["completed_pairs"], 2);
    assert!(
        record["anomalies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|anomaly| anomaly["code"] == "run-resumed")
    );

    let mismatch = calibration(&run, 13.0, None).output().unwrap();
    assert_eq!(mismatch.status.code(), Some(2));
    assert!(mismatch.stdout.is_empty());
    assert!(
        String::from_utf8(mismatch.stderr)
            .unwrap()
            .contains("run configuration mismatch")
    );
}
