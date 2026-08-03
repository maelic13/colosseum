use std::io::Read;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde_json::Value;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn write_tune(root: &std::path::Path) -> std::path::PathBuf {
    write_tune_contents(
        root,
        r#"
[[parameters]]
name = "Hash"
initial = 16
min = 1
max = 1024
c_end = 1.0
"#,
    )
}

fn write_tune_contents(root: &std::path::Path, contents: &str) -> std::path::PathBuf {
    let path = root.join("tune.toml");
    std::fs::write(&path, contents).unwrap();
    path
}

fn stub_command(tune: &std::path::Path, run: &std::path::Path) -> Command {
    let mut command = cli();
    command
        .arg("spsa")
        .arg(env!("CARGO_BIN_EXE_colosseum-cli"))
        .arg("--engine-arg=__uci-stub")
        .arg("--tune")
        .arg(tune)
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
            "--dir",
        ])
        .arg(run);
    command
}

fn checkpoint_payload(run: &std::path::Path) -> Value {
    let envelope: Value =
        serde_json::from_slice(&std::fs::read(run.join("checkpoint.json")).unwrap()).unwrap();
    envelope["payload"].clone()
}

#[test]
fn spsa_dry_run_resolves_defaults_and_schedule_without_launching_an_engine() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let output = cli()
        .args(["spsa", "definitely-missing-engine", "--tune"])
        .arg(&tune)
        .args(["--r-end", "0.002", "--dry-run", "--json", "--seed", "7"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "dry-run");
    assert_eq!(value["command"], "spsa");
    let config = &value["resolved_configuration"];
    assert_eq!(config["settings"]["iterations"], 5_000);
    assert_eq!(config["settings"]["games_per_iteration"], 32);
    assert_eq!(config["schedule"]["perturbations"]["master_seed"], 7);
    assert_eq!(config["tune"]["live_schema"], "verified-before-game-launch");
}

#[test]
fn spsa_configuration_audit_refuses_unmeasurable_vectors_before_engine_launch() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune_contents(
        root.path(),
        r#"
[[parameters]]
name = "Hash"
initial = 16
min = 1
max = 1024
c_end = 0.49
"#,
    );
    let output = cli()
        .args(["spsa", "definitely-missing-engine", "--tune"])
        .arg(&tune)
        .args(["--r-end", "0.002", "--dry-run"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stdout).unwrap().is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("rounds to zero at the end of the schedule")
    );
}

#[test]
fn spsa_configuration_audit_records_nonfatal_live_schema_warnings() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune_contents(
        root.path(),
        r#"
[[parameters]]
name = "Hash"
initial = 1
min = 1
max = 1024
c_end = 1.0
"#,
    );
    let run = root.path().join("warnings");
    let output = stub_command(&tune, &run).arg("--json").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("starts at 1, but the engine advertises default 16"));
    assert!(stderr.contains("starts on its lower rail"));
    let output: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        output["report"]["tune_audit"]["warnings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(
        record["workflow"]["tune_audit"]["warnings"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn spsa_resource_plan_uses_the_tuned_hash_rail_and_rejects_ambiguous_direct_cores() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let too_small = cli()
        .args(["spsa", "missing-engine", "--tune"])
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--dry-run",
            "--seed",
            "7",
            "--memory-budget-mb",
            "2047",
        ])
        .output()
        .unwrap();
    assert_eq!(too_small.status.code(), Some(2));
    assert!(
        String::from_utf8(too_small.stderr)
            .unwrap()
            .contains("2048 MB exceeds trusted budget 2047 MB")
    );

    let direct = cli()
        .args(["spsa", "missing-engine", "--tune"])
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--dry-run",
            "--seed",
            "7",
            "--cores",
            "0",
        ])
        .output()
        .unwrap();
    assert_eq!(direct.status.code(), Some(2));
    assert!(
        String::from_utf8(direct.stderr)
            .unwrap()
            .contains("cannot describe two disjoint arms")
    );
}

#[test]
fn complete_mini_match_is_one_durable_gradient_commit() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let run = root.path().join("run");
    let output = stub_command(&tune, &run).arg("--json").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "spsa");
    assert_eq!(value["report"]["driver"]["status"], "completed");
    assert_eq!(
        value["report"]["driver"]["completed_iterations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let iteration = &value["report"]["driver"]["completed_iterations"][0];
    assert_eq!(iteration["pairs"].as_array().unwrap().len(), 1);
    assert_eq!(iteration["score"]["difference"], 0);
    assert_eq!(iteration["centers_before"][0], 16.0);
    assert_eq!(iteration["centers_after"][0], 16.0);
    assert_eq!(iteration["prepared"]["plus"][0]["sent"], 17);
    assert_eq!(iteration["prepared"]["minus"][0]["sent"], 15);

    for artifact in [
        "spsa-schedule.json",
        "checkpoint.json",
        "run.log",
        "games.pgn",
        "result.json",
        "run-record.json",
        "tuned-options.json",
        "tuned-options.txt",
        "tuned-options.toml",
    ] {
        assert!(run.join(artifact).is_file(), "missing {artifact}");
    }
    assert_eq!(value["report"]["tuned_result"]["window"]["percent"], 10);
    assert_eq!(
        value["report"]["tuned_result"]["parameters"][0]["tuned"],
        16
    );
    assert!(
        std::fs::read_to_string(run.join("tuned-options.txt"))
            .unwrap()
            .contains("setoption name Hash value 16")
    );
    assert!(
        std::fs::read_to_string(run.join("tuned-options.toml"))
            .unwrap()
            .contains("[engine.options]")
    );
    let checkpoint = checkpoint_payload(&run);
    assert_eq!(
        checkpoint["completed_iterations"].as_array().unwrap().len(),
        1
    );
    assert!(checkpoint.get("invalid_iteration").is_none());
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["status"], "completed");
    assert_eq!(record["official_sample"]["committed_units"], 1);
    assert_eq!(record["official_sample"]["completed_pairs"], 1);
    assert_eq!(record["official_sample"]["scored_games"], 2);
}

#[test]
fn sprt_apply_consumes_the_unedited_spsa_result_and_verifies_executable_content() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let tune_run = root.path().join("tune");
    let tune_output = stub_command(&tune, &tune_run).output().unwrap();
    assert!(
        tune_output.status.success(),
        "{}",
        String::from_utf8_lossy(&tune_output.stderr)
    );
    let result = tune_run.join("result.json");
    let gate_run = root.path().join("gate");
    let gate = cli()
        .arg("sprt")
        .arg("--apply")
        .arg(&result)
        .args([
            "--max-pairs",
            "1",
            "--preset",
            "gainer",
            "--a-depth",
            "1",
            "--b-depth",
            "1",
            "--max-moves",
            "2",
            "--seed",
            "7",
            "--dir",
        ])
        .arg(&gate_run)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        gate.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&gate.stderr)
    );
    assert!(gate.stderr.is_empty());
    let gate: Value = serde_json::from_slice(&gate.stdout).unwrap();
    assert_eq!(gate["report"]["apply"]["identity"]["status"], "verified");
    assert_eq!(gate["report"]["apply"]["parameters"][0]["name"], "Hash");
    let resolved: Value =
        serde_json::from_slice(&std::fs::read(gate_run.join("resolved-config.json")).unwrap())
            .unwrap();
    assert_eq!(resolved["engine_a"]["options"]["Hash"]["value"], 16);
    assert_eq!(resolved["engine_b"]["options"]["Hash"]["value"], 16);
    let record: Value =
        serde_json::from_slice(&std::fs::read(gate_run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(
        record["workflow"]["apply"]["identity"]["status"],
        "verified"
    );
}

#[test]
fn sprt_apply_refuses_hash_mismatch_unless_the_override_is_prominent() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let tune_run = root.path().join("tune");
    assert!(
        stub_command(&tune, &tune_run)
            .output()
            .unwrap()
            .status
            .success()
    );
    let result = tune_run.join("result.json");
    let different = env!("CARGO_BIN_EXE_colosseum-uci-fixture");

    let refused = cli()
        .arg("sprt")
        .arg("--apply")
        .arg(&result)
        .arg("--apply-executable")
        .arg(different)
        .args(["--max-pairs", "1", "--preset", "gainer", "--dry-run"])
        .output()
        .unwrap();
    assert_eq!(refused.status.code(), Some(2));
    assert!(
        String::from_utf8(refused.stderr)
            .unwrap()
            .contains("executable SHA-256 mismatch")
    );

    let overridden = cli()
        .arg("sprt")
        .arg("--apply")
        .arg(&result)
        .arg("--apply-executable")
        .arg(different)
        .arg("--allow-executable-mismatch")
        .args([
            "--max-pairs",
            "1",
            "--preset",
            "gainer",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(overridden.status.success());
    assert!(String::from_utf8_lossy(&overridden.stderr).contains("WARNING"));
    let overridden: Value = serde_json::from_slice(&overridden.stdout).unwrap();
    assert_eq!(
        overridden["resolved_configuration"]["apply"]["identity"]["status"],
        "mismatch-overridden"
    );
}

#[test]
fn engine_fault_commits_invalid_evidence_but_never_a_gradient() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let run = root.path().join("invalid");
    let output = cli()
        .arg("spsa")
        .arg(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
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
            "--dir",
        ])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let driver = &value["report"]["driver"];
    assert_eq!(driver["status"], "invalid");
    assert!(
        driver["completed_iterations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(driver["final_centers"][0], 16.0);
    assert_eq!(driver["invalid_iteration"]["centers_before"][0], 16.0);
    assert!(driver["invalid_iteration"].get("centers_after").is_none());
    assert!(
        driver["invalid_iteration"]["faults"]["engine_a"]
            .as_u64()
            .unwrap()
            + driver["invalid_iteration"]["faults"]["engine_b"]
                .as_u64()
                .unwrap()
            > 0
    );
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["status"], "invalid");
    assert_eq!(record["official_sample"]["committed_units"], 0);
}

#[test]
fn killed_tune_resumes_the_exact_rng_iteration_and_durable_prefix() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let run = root.path().join("resume");
    let pid_file = root.path().join("engine.pid");
    let mut child = long_tune(&tune, &run, &pid_file, "3", "2", "0.002")
        .spawn()
        .unwrap();
    wait_for_first_commit(&mut child, &run);
    let before = checkpoint_payload(&run)["completed_iterations"][0].clone();
    let active_engines = wait_for_active_engines(&mut child, &pid_file);
    child.kill().unwrap();
    child.wait().unwrap();
    assert_processes_reaped(active_engines);

    let output = long_tune(&tune, &run, &pid_file, "99", "4", "1")
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("stored SPSA horizon: 3 iterations, 2 games per iteration, r_end 0.002")
    );
    let completed = value["report"]["driver"]["completed_iterations"]
        .as_array()
        .unwrap();
    assert_eq!(completed.len(), 3);
    assert_eq!(completed[0], before);
    assert_eq!(completed[0]["iteration"], 0);
    assert_eq!(completed[1]["iteration"], 1);
    assert_eq!(completed[2]["iteration"], 2);
    let log = std::fs::read_to_string(run.join("run.log")).unwrap();
    assert_eq!(log.matches("spsa-iteration-committed").count(), 3);
}

fn long_tune(
    tune: &std::path::Path,
    run: &std::path::Path,
    pid_file: &std::path::Path,
    iterations: &str,
    games_per_iteration: &str,
    r_end: &str,
) -> Command {
    let mut command = cli();
    command
        .arg("spsa")
        .arg(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
        .args([
            "--engine-arg=--legal-sequence",
            "--engine-arg=--sleep-ms=75",
            "--engine-arg=--append-pid-file",
        ])
        .arg(format!("--engine-arg=--pid-file={}", pid_file.display()))
        .arg("--tune")
        .arg(tune)
        .args([
            "--r-end",
            r_end,
            "--iterations",
            iterations,
            "--games-per-iteration",
            games_per_iteration,
            "--depth",
            "1",
            "--max-moves",
            "2",
            "--seed",
            "7",
            "--dir",
        ])
        .arg(run)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    command
}

fn wait_for_active_engines(child: &mut Child, pid_file: &std::path::Path) -> [u32; 2] {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        assert!(
            child.try_wait().unwrap().is_none(),
            "SPSA tune ended before an active post-checkpoint engine was observed"
        );
        if let Ok(contents) = std::fs::read_to_string(pid_file) {
            let mut active = contents
                .lines()
                .filter_map(|line| line.trim().parse::<u32>().ok())
                .filter(|pid| colosseum_uci::process_is_alive(*pid))
                .collect::<Vec<_>>();
            active.sort_unstable();
            active.dedup();
            if active.len() >= 2 {
                return [active[0], active[1]];
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for an active SPSA engine process");
}

fn assert_processes_reaped(pids: [u32; 2]) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if pids
            .iter()
            .all(|pid| !colosseum_uci::process_is_alive(*pid))
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("SPSA engine processes remained after their CLI owner was killed: {pids:?}");
}

fn wait_for_first_commit(child: &mut Child, run: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            let mut stdout = String::new();
            let mut stderr = String::new();
            child
                .stdout
                .take()
                .unwrap()
                .read_to_string(&mut stdout)
                .unwrap();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .unwrap();
            panic!(
                "SPSA fixture exited before it could be interrupted\nstdout: {stdout}\nstderr: {stderr}"
            );
        }
        let has_checkpoint = run.join("checkpoint.json").is_file();
        let has_log_commit = std::fs::read_to_string(run.join("run.log"))
            .is_ok_and(|log| log.contains("spsa-iteration-committed"));
        if has_checkpoint && has_log_commit {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for the first durable SPSA iteration");
}
