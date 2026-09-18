use std::collections::BTreeMap;
use std::io::Read;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde_json::Value;

/// A durable run reports its progress on standard error, so "quiet" means
/// nothing there but progress blocks and the rules between them: no warning,
/// no diagnostic, no error.
fn only_progress(output: &std::process::Output) -> bool {
    String::from_utf8_lossy(&output.stderr).lines().all(|line| {
        line.is_empty()
            || line.starts_with("progress [")
            || line.starts_with("  ")
            || line.chars().all(|character| character == '-')
    })
}

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
        .args([
            "--r-end",
            "0.002",
            "--ponder",
            "--cores-per-engine",
            "1",
            "--dry-run",
            "--json",
            "--seed",
            "7",
        ])
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
    assert_eq!(config["ponder"], true);
    assert_eq!(value["invocations"][0]["options"]["Ponder"]["value"], true);
}

#[test]
fn spsa_plan_is_offline_and_reports_exact_schedule_cost_and_timing() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let output = cli()
        .args(["spsa", "plan", "--tune"])
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            "4",
            "--games-per-iteration",
            "6",
            "--concurrency",
            "4",
            "--seconds-per-game-low",
            "2",
            "--seconds-per-game-high",
            "3",
            "--compare-iterations",
            "2",
            "--compare-iterations",
            "8",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "spsa-plan");
    let report = &value["report"];
    assert_eq!(report["total_games"], 24);
    assert_eq!(report["total_pairs"], 12);
    assert_eq!(report["checkpoint_publications"], 4);
    assert_eq!(report["wall_time"]["waves_per_iteration"], 2);
    assert_eq!(report["wall_time"]["lower_seconds"], 16.0);
    assert_eq!(report["wall_time"]["upper_seconds"], 24.0);
    assert_eq!(
        report["knobs"][0]["trajectory"].as_array().unwrap().len(),
        4
    );
    assert_eq!(report["knobs"][0]["trajectory"][3]["c"], 1.0);
    assert_eq!(
        report["knobs"][0]["first_rounding_resolution_hazard"],
        Value::Null
    );
    assert_eq!(report["horizon_comparisons"][0]["iterations"], 2);
    assert_eq!(report["horizon_comparisons"][1]["iterations"], 8);
}

#[test]
fn spsa_plan_pilot_timing_is_a_labelled_observation_not_a_convergence_claim() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let output = cli()
        .args(["spsa", "plan", "--tune"])
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            "3",
            "--games-per-iteration",
            "2",
            "--concurrency",
            "2",
            "--pilot-game-seconds",
            "1.25",
            "--pilot-game-seconds",
            "2.0",
            "--pilot-game-seconds",
            "1.5",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["report"]["wall_time"]["basis"]["source"],
        "pilot-games"
    );
    assert_eq!(value["report"]["wall_time"]["lower_seconds"], 3.75);
    assert_eq!(value["report"]["wall_time"]["upper_seconds"], 6.0);
    assert!(
        value["report"]["interpretation"]
            .as_str()
            .unwrap()
            .contains("not a chess-convergence forecast")
    );
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

/// Several iterations, so a staged stop has a boundary to land on and a tail
/// window has more than one sample.
fn multi_iteration_command(
    tune: &std::path::Path,
    run: &std::path::Path,
    iterations: &str,
) -> Command {
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
            iterations,
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

#[test]
fn the_default_estimator_is_the_final_centre_and_the_tail_window_is_opt_in() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());

    let default_run = root.path().join("default");
    let default_output = multi_iteration_command(&tune, &default_run, "4")
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        default_output.status.success(),
        "{}",
        String::from_utf8_lossy(&default_output.stderr)
    );
    let default: Value = serde_json::from_slice(&default_output.stdout).unwrap();
    let estimator = &default["report"]["tuned_result"]["estimator"];
    assert_eq!(estimator["kind"], "final-center");
    assert_eq!(estimator["iteration"], 3);
    assert_eq!(default["report"]["tuned_result"]["completed_iterations"], 4);
    assert_eq!(default["report"]["tuned_result"]["schema_version"], 3);

    let window_run = root.path().join("window");
    let window_output = multi_iteration_command(&tune, &window_run, "4")
        .args(["--final-window-percent", "50", "--json"])
        .output()
        .unwrap();
    assert!(
        window_output.status.success(),
        "{}",
        String::from_utf8_lossy(&window_output.stderr)
    );
    let window: Value = serde_json::from_slice(&window_output.stdout).unwrap();
    let estimator = &window["report"]["tuned_result"]["estimator"];
    assert_eq!(estimator["kind"], "tail-window-mean");
    assert_eq!(estimator["percent"], 50);
    assert_eq!(estimator["samples_used"], 2);
    // The selected estimator is frozen into the run record exactly as the
    // default is, so a reader knows which one produced the vector.
    let record: Value =
        serde_json::from_slice(&std::fs::read(window_run.join("run-record.json")).unwrap())
            .unwrap();
    assert_eq!(record["workflow"]["final_window_percent"], 50);
    let default_record: Value =
        serde_json::from_slice(&std::fs::read(default_run.join("run-record.json")).unwrap())
            .unwrap();
    assert!(default_record["workflow"]["final_window_percent"].is_null());
}

#[test]
fn stop_after_iteration_stops_on_a_boundary_and_leaves_the_horizon_alone() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let run = root.path().join("run");

    let stopped = multi_iteration_command(&tune, &run, "4")
        .args(["--stop-after-iteration", "2", "--json"])
        .output()
        .unwrap();
    assert_eq!(stopped.status.code(), Some(6));
    let value: Value = serde_json::from_slice(&stopped.stdout).unwrap();
    assert_eq!(value["report"]["driver"]["status"], "cancelled");
    assert_eq!(
        value["report"]["driver"]["completed_iterations"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    // The stored horizon is untouched by the stop request.
    assert_eq!(value["report"]["driver"]["settings"]["iterations"], 4);
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["status"], "cancelled");
    assert_eq!(record["workflow"]["settings"]["iterations"], 4);
    assert_eq!(record["workflow"]["stop_after_iteration"], 2);
    // A clean stop still produces an on-demand gate candidate.
    assert_eq!(value["report"]["tuned_result"]["estimator"]["iteration"], 1);

    let status = cli()
        .args(["status"])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["record"]["status"], "cancelled");

    // Resuming the same directory continues to the stored horizon.
    let resumed = multi_iteration_command(&tune, &run, "4")
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let resumed: Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(resumed["report"]["driver"]["status"], "completed");
    assert_eq!(
        resumed["report"]["driver"]["completed_iterations"]
            .as_array()
            .unwrap()
            .len(),
        4
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
    assert!(
        only_progress(&output),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
    // Without --final-window-percent the default estimator is the final
    // completed centre vector, not an average over a tail window.
    assert_eq!(
        value["report"]["tuned_result"]["estimator"]["kind"],
        "final-center"
    );
    assert_eq!(value["report"]["tuned_result"]["estimator"]["iteration"], 0);
    assert_eq!(value["report"]["tuned_result"]["completed_iterations"], 1);
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
    // The checkpoint is aggregates: how far the tune got and where it stands.
    let checkpoint = checkpoint_payload(&run);
    assert_eq!(checkpoint["completed_iterations"], 1);
    assert!(checkpoint["invalid_iteration"].is_null());
    assert_eq!(checkpoint["centers"].as_array().unwrap().len(), 1);
    // The games are in the journal, one line each, with their iteration.
    let journal = journal_lines(&run);
    assert_eq!(journal.len(), 2);
    assert!(journal.iter().all(|line| line["game"]["iteration"] == 0));
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["status"], "completed");
    assert_eq!(record["official_sample"]["committed_units"], 1);
    assert_eq!(record["official_sample"]["completed_pairs"], 1);
    assert_eq!(record["official_sample"]["scored_games"], 2);

    let status = cli()
        .args(["spsa", "status"])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(status.stderr.is_empty());
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["type"], "spsa-status");
    assert_eq!(status["report"]["diagnostics"]["completed_iterations"], 1);
    assert_eq!(status["report"]["diagnostics"]["percent_complete"], 100.0);
    assert_eq!(
        status["report"]["diagnostics"]["knobs"][0]["frequent_bound_contact"]["state"],
        "insufficient-history"
    );
    assert_eq!(
        status["report"]["candidate_result"]["parameters"][0]["tuned"],
        16
    );
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
    assert!(
        only_progress(&gate),
        "{}",
        String::from_utf8_lossy(&gate.stderr)
    );
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
    assert!(
        only_progress(&output),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
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
    let status_start = Instant::now();
    let live_status = cli()
        .args(["spsa", "status"])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        live_status.status.success(),
        "{}",
        String::from_utf8_lossy(&live_status.stderr)
    );
    assert!(status_start.elapsed() < Duration::from_secs(2));
    let live_status: Value = serde_json::from_slice(&live_status.stdout).unwrap();
    let durable_iterations = live_status["report"]["diagnostics"]["completed_iterations"]
        .as_u64()
        .unwrap();
    assert!((1..=3).contains(&durable_iterations));
    assert_eq!(
        live_status["report"]["diagnostics"]["knobs"][0]["recent_stability"]["state"],
        "insufficient-history"
    );
    // What was durable before the kill: the first iteration's journal lines.
    let durable_prefix = journal_prefix(&run, 2);
    let active_engines = wait_for_active_engines(&mut child, &pid_file);
    child.kill().unwrap();
    child.wait().unwrap();
    assert_processes_reaped(active_engines);

    let bytes_before_status = run_files(&run);
    let stopped_status = cli()
        .args(["spsa", "status"])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(stopped_status.status.success());
    assert_eq!(run_files(&run), bytes_before_status);

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
    assert_eq!(completed[0]["iteration"], 0);
    assert_eq!(completed[1]["iteration"], 1);
    assert_eq!(completed[2]["iteration"], 2);
    // The resumed journal begins with exactly the bytes that were durable: the
    // resume appended to them and changed none of them.
    let after = std::fs::read(run.join("games.jsonl")).unwrap();
    assert!(after.starts_with(&durable_prefix));
    // The iteration the resume replayed is the one those lines describe.
    let journal = journal_lines(&run);
    assert_eq!(journal.len(), 6);
    for (index, pair) in completed[0]["pairs"].as_array().unwrap().iter().enumerate() {
        for (offset, game) in [&pair["first"], &pair["second"]].into_iter().enumerate() {
            let line = &journal[index * 2 + offset]["game"];
            assert_eq!(line["number"], game["number"]);
            assert_eq!(line["result"], game["result"]);
            assert_eq!(line["white"], game["white"]);
        }
    }
    assert_eq!(
        journal
            .iter()
            .map(|line| line["game"]["iteration"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [0, 0, 1, 1, 2, 2]
    );
}

/// Every complete journal line, parsed.
fn journal_lines(run: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(run.join("games.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

/// The bytes of the journal's first `lines` complete lines.
fn journal_prefix(run: &std::path::Path, lines: usize) -> Vec<u8> {
    let bytes = std::fs::read(run.join("games.jsonl")).unwrap();
    let end = bytes
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == b'\n')
        .nth(lines - 1)
        .map(|(index, _)| index + 1)
        .expect("the journal holds the lines asked for");
    bytes[..end].to_vec()
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
        // An iteration is committed once its games are in the journal: two
        // complete lines for the two games of this fixture's mini-match. The
        // checkpoint that summarises them may not exist yet.
        let committed = std::fs::read_to_string(run.join("games.jsonl"))
            .is_ok_and(|journal| journal.matches('\n').count() >= 2);
        if committed {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("timed out waiting for the first durable SPSA iteration");
}

fn run_files(run: &std::path::Path) -> BTreeMap<std::ffi::OsString, Vec<u8>> {
    std::fs::read_dir(run)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| (entry.file_name(), std::fs::read(entry.path()).unwrap()))
        .collect()
}
