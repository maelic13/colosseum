use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use colosseum_application::{
    CalibrationDesign, CalibrationInterval, CalibrationStatus, classify_calibration,
};
use serde_json::Value;

const ACCEPTANCE: &str = include_str!("../../../docs/fixtures/phase4c/acceptance.json");
const CALIBRATION_SOURCE: &str = include_str!("../../colosseum-application/src/calibration.rs");
const COMMAND_LINE: &str = include_str!("command_line.rs");
const MAIN: &str = include_str!("../src/main.rs");

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn cli_executable() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn calibration(run: &Path, tolerance_nelo: f64) -> Command {
    let mut command = cli();
    command
        .arg("calibrate")
        .arg(cli_executable())
        .arg(cli_executable())
        .args([
            "--games",
            "8",
            "--confidence",
            "0.9",
            "--tolerance-nelo",
            &tolerance_nelo.to_string(),
            "--a-engine-arg=__uci-stub",
            "--a-engine-arg=--sleep-ms=50",
            "--b-engine-arg=__uci-stub",
            "--b-engine-arg=--sleep-ms=50",
            "--max-moves",
            "2",
            "--no-draw-adjudication",
            "--no-resign-adjudication",
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

fn calibration_with_pid_files(
    run: &Path,
    tolerance_nelo: f64,
    a_pid_file: &Path,
    b_pid_file: &Path,
) -> Command {
    let mut command = calibration(run, tolerance_nelo);
    command
        .arg(format!(
            "--a-engine-arg=--pid-file={}",
            a_pid_file.display()
        ))
        .arg(format!(
            "--b-engine-arg=--pid-file={}",
            b_pid_file.display()
        ));
    command
}

fn read_pid(path: &Path) -> Option<u32> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn wait_for_pid_pair(child: &mut Child, a: &Path, b: &Path) -> (u32, u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        assert!(child.try_wait().unwrap().is_none());
        if let (Some(a), Some(b)) = (read_pid(a), read_pid(b)) {
            return (a, b);
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("calibration engine PID pair was not observed");
}

fn wait_for_new_pid_pair(child: &mut Child, a: &Path, b: &Path, old: (u32, u32)) -> (u32, u32) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        assert!(child.try_wait().unwrap().is_none());
        if let (Some(a), Some(b)) = (read_pid(a), read_pid(b))
            && (a, b) != old
        {
            return (a, b);
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("next calibration engine PID pair was not observed");
}

fn assert_processes_reaped(pids: (u32, u32)) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if !colosseum_uci::process_is_alive(pids.0) && !colosseum_uci::process_is_alive(pids.1) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("calibration engines remained after owner death: {pids:?}");
}

fn wait_for_checkpoint(child: &mut Child, checkpoint: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        assert!(
            child.try_wait().unwrap().is_none(),
            "calibration ended before its resume checkpoint was observed"
        );
        if checkpoint.is_file() {
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
    panic!("calibration did not create a checkpoint within ten seconds");
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
fn acceptance_manifest_names_every_phase_4c_exit_gate_and_test_owner() {
    let manifest: Value = serde_json::from_str(ACCEPTANCE).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["phase"], "4C");
    let gates = manifest["gates"].as_array().unwrap();
    let ids = gates
        .iter()
        .map(|gate| gate["id"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ids,
        BTreeSet::from([
            "binary-content-identity",
            "configuration-and-resume",
            "deterministic-outcome-classification",
            "distinct-automation-exits",
            "fault-invalidity",
            "real-machine-smoke",
            "workspace-regression",
        ])
    );
    assert_eq!(ids.len(), gates.len(), "duplicate Phase 4C gate ID");
    for gate in gates {
        assert!(!gate["evidence"].as_str().unwrap().trim().is_empty());
    }
    for symbol in [
        "calibration_refuses_nonidentical_executable_content_before_launch",
        "calibration_marks_any_engine_fault_invalid_even_when_the_match_policy_allows_it",
    ] {
        assert!(COMMAND_LINE.contains(symbol), "missing CLI test {symbol}");
    }
    assert!(CALIBRATION_SOURCE.contains("classify_calibration"));
    assert!(MAIN.contains("calibration_terminal_classes_have_distinct_automation_exit_codes"));
    let smoke = &manifest["real_machine_smoke"];
    assert_eq!(smoke["engine"], "Basilisk 1.9.0");
    assert_eq!(smoke["result"], "inconclusive");
    assert_eq!(smoke["engine_faults"], 0);
    assert_eq!(smoke["infrastructure_faults"], 0);
    assert_eq!(smoke["affinity"], "enforced");
}

#[test]
fn every_calibration_outcome_is_deterministic_at_its_exact_boundaries() {
    let design = CalibrationDesign::new(8, 0.95, 5.0).unwrap();
    let interval = |lower_nelo, upper_nelo| CalibrationInterval {
        confidence: 0.95,
        estimate_nelo: (lower_nelo + upper_nelo) / 2.0,
        lower_nelo,
        upper_nelo,
    };
    for (observed, expected) in [
        (
            classify_calibration(design, Some(interval(-5.0, 5.0)), 0),
            CalibrationStatus::Pass,
        ),
        (
            classify_calibration(design, Some(interval(5.000_001, 8.0)), 0),
            CalibrationStatus::Fail,
        ),
        (
            classify_calibration(design, Some(interval(-8.0, -5.000_001)), 0),
            CalibrationStatus::Fail,
        ),
        (
            classify_calibration(design, Some(interval(-4.0, 6.0)), 0),
            CalibrationStatus::Inconclusive,
        ),
        (
            classify_calibration(design, None, 0),
            CalibrationStatus::Inconclusive,
        ),
        (
            classify_calibration(design, Some(interval(-1.0, 1.0)), 1),
            CalibrationStatus::Invalid,
        ),
    ] {
        assert_eq!(observed, expected);
    }
}

#[test]
fn killed_calibration_resumes_exact_configuration_and_refuses_mismatch() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("calibration");
    let a_pid_file = root.path().join("a.pid");
    let b_pid_file = root.path().join("b.pid");
    let mut first = calibration_with_pid_files(&run, 12.5, &a_pid_file, &b_pid_file);
    first.stdout(Stdio::null()).stderr(Stdio::null());
    let mut child = first.spawn().unwrap();
    let initial = wait_for_pid_pair(&mut child, &a_pid_file, &b_pid_file);
    wait_for_checkpoint(&mut child, &run.join("checkpoint.json"));
    let active = wait_for_new_pid_pair(&mut child, &a_pid_file, &b_pid_file, initial);
    child.kill().unwrap();
    child.wait().unwrap();
    assert_processes_reaped(active);

    let resumed = calibration_with_pid_files(&run, 12.5, &a_pid_file, &b_pid_file)
        .output()
        .unwrap();
    assert_eq!(
        resumed.status.code(),
        Some(4),
        "stderr: {}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let output = json(&resumed);
    assert_eq!(output["report"]["status"], "inconclusive");
    assert_eq!(output["report"]["fixed_match"]["games_attempted"], 8);
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
    assert_eq!(record["official_sample"]["completed_pairs"], 4);
    assert!(
        record["anomalies"]
            .as_array()
            .unwrap()
            .iter()
            .any(|anomaly| { anomaly["code"] == "run-resumed" })
    );

    let mismatch = calibration_with_pid_files(&run, 13.0, &a_pid_file, &b_pid_file)
        .output()
        .unwrap();
    assert_eq!(mismatch.status.code(), Some(2));
    assert!(mismatch.stdout.is_empty());
    assert!(
        String::from_utf8(mismatch.stderr)
            .unwrap()
            .contains("run configuration mismatch")
    );
}
