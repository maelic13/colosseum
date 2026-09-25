use std::collections::BTreeMap;
use std::process::Command;

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
fn the_tail_window_estimator_is_opt_in_and_recorded() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let run = root.path().join("window");
    let output = multi_iteration_command(&tune, &run, "2")
        .args(["--final-window-percent", "50", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let estimator = &value["report"]["tuned_result"]["estimator"];
    assert_eq!(estimator["kind"], "tail-window-mean");
    assert_eq!(estimator["percent"], 50);
    assert_eq!(estimator["samples_used"], 1);
    assert_eq!(value["report"]["tuned_result"]["completed_iterations"], 2);
    assert_eq!(value["report"]["tuned_result"]["schema_version"], 3);
    // The selected estimator is frozen into the run record exactly as the
    // default is, so a reader knows which one produced the vector.
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["workflow"]["final_window_percent"], 50);
}

#[test]
fn stop_after_iteration_stops_on_a_boundary_and_leaves_the_horizon_alone() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let run = root.path().join("run");

    let stopped = multi_iteration_command(&tune, &run, "2")
        .args(["--stop-after-iteration", "1", "--json"])
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
        1
    );
    // The stored horizon is untouched by the stop request.
    assert_eq!(value["report"]["driver"]["settings"]["iterations"], 2);
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["status"], "cancelled");
    assert_eq!(record["workflow"]["settings"]["iterations"], 2);
    assert_eq!(record["workflow"]["stop_after_iteration"], 1);
    // A clean stop still produces an on-demand gate candidate.
    assert_eq!(value["report"]["tuned_result"]["estimator"]["iteration"], 0);

    let status = cli()
        .args(["status"])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["record"]["status"], "cancelled");

    // Observing a stopped tune changes none of its files.
    let before_status = run_files(&run);
    let spsa_status = cli()
        .args(["spsa", "status"])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        spsa_status.status.success(),
        "{}",
        String::from_utf8_lossy(&spsa_status.stderr)
    );
    let spsa_status: Value = serde_json::from_slice(&spsa_status.stdout).unwrap();
    assert_eq!(
        spsa_status["report"]["diagnostics"]["completed_iterations"],
        1
    );
    assert_eq!(run_files(&run), before_status);
    let durable_journal = std::fs::read(run.join("games.jsonl")).unwrap();

    // Resuming continues to the stored horizon, whatever the command line
    // now asks for.
    let resumed = multi_iteration_command(&tune, &run, "9")
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    assert!(
        String::from_utf8_lossy(&resumed.stderr)
            .contains("stored SPSA horizon: 2 iterations, 2 games per iteration, r_end 0.002"),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    let resumed: Value = serde_json::from_slice(&resumed.stdout).unwrap();
    assert_eq!(resumed["report"]["driver"]["status"], "completed");
    let completed = resumed["report"]["driver"]["completed_iterations"]
        .as_array()
        .unwrap();
    assert_eq!(completed.len(), 2);
    assert_eq!(completed[0]["games"]["first"], 1);
    assert_eq!(completed[0]["games"]["last"], 2);
    // The resume appended to the durable journal and changed none of it.
    assert!(
        std::fs::read(run.join("games.jsonl"))
            .unwrap()
            .starts_with(&durable_journal)
    );
    assert_eq!(
        journal_lines(&run)
            .iter()
            .map(|line| line["game"]["iteration"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [0, 0, 1, 1]
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
    // The result carries a summary per iteration and names the journal for
    // its games: one pair, games 1 and 2. What the schedule gives again (arm
    // vectors, signs, gains, the centres before) is not stored.
    assert_eq!(value["report"]["schema_version"], 3);
    assert_eq!(value["report"]["games"], "games.jsonl");
    let iteration = &value["report"]["driver"]["completed_iterations"][0];
    assert!(iteration.get("pairs").is_none(), "{iteration}");
    assert_eq!(iteration["games"]["first"], 1);
    assert_eq!(iteration["games"]["last"], 2);
    assert_eq!(iteration["score"]["difference"], 0);
    assert_eq!(iteration["centers_after"][0], 16.0);
    assert!(iteration["faults"].is_object(), "{iteration}");
    for derivable in ["centers_before", "prepared"] {
        assert!(iteration.get(derivable).is_none(), "{iteration}");
    }

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
    // The result names the journal, so `stats` on it reads the run it belongs
    // to, exactly as `stats` on the directory does.
    let stats = |path: &std::path::Path| {
        let output = cli().arg("stats").arg(path).arg("--json").output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["report"].clone()
    };
    let (from_result, from_directory) = (stats(&run.join("result.json")), stats(&run));
    assert_eq!(from_result, from_directory);
    assert!(
        from_result["source"]
            .as_str()
            .unwrap()
            .ends_with("games.jsonl"),
        "{from_result}"
    );
    assert_eq!(from_result["complete_pairs"], 1);
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["status"], "completed");
    assert!(record["workflow"]["final_window_percent"].is_null());
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
fn sprt_apply_consumes_the_unedited_spsa_result_and_refuses_other_executables() {
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

    // A different executable is refused unless the override is prominent.
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
    // A strict limit: the first fault voids the iteration and the tune.
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
            "--max-engine-faults",
            "0",
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
    for derivable in ["centers_before", "prepared", "centers_after"] {
        assert!(driver["invalid_iteration"].get(derivable).is_none());
    }
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

/// Under the default allowance a forfeit is a result: the iteration it lands in
/// is scored and committed, and the tune goes on until the faults outrun the
/// larger of 3 and 0.5% of the games played.
#[test]
fn a_rare_forfeit_is_scored_into_the_gradient_until_the_allowance_is_exceeded() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let run = root.path().join("allowed");
    // The fixture forfeits every game it plays as Black: two faults per
    // two-game iteration.
    let output = cli()
        .arg("spsa")
        .arg(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
        .arg("--tune")
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            "3",
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
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stderr)));
    let driver = &value["report"]["driver"];
    // Iteration 0: two faults in two games, within the floor of three. It is
    // committed with its forfeits scored as the losses they are.
    let completed = driver["completed_iterations"].as_array().unwrap();
    assert_eq!(completed.len(), 1, "{driver}");
    let score = &completed[0]["score"];
    assert_eq!(
        score["plus_wins"].as_u64().unwrap()
            + score["plus_losses"].as_u64().unwrap()
            + score["draws"].as_u64().unwrap(),
        2
    );
    assert!(completed[0].get("centers_after").is_some());
    // Its summary counts the two forfeits; the games are in the journal.
    let faults = &completed[0]["faults"];
    assert_eq!(
        faults["engine_a"].as_u64().unwrap() + faults["engine_b"].as_u64().unwrap(),
        2
    );
    let journal = std::fs::read_to_string(run.join("games.jsonl")).unwrap();
    assert!(
        journal
            .lines()
            .take(2)
            .all(|line| line.contains("\"fault\"")),
        "{journal}"
    );
    // Iteration 1 takes the count to four in four games: over the allowance.
    assert_eq!(driver["status"], "invalid");
    assert_eq!(driver["invalid_iteration"]["iteration"], 1);
    assert_eq!(output.status.code(), Some(5));
    assert_eq!(
        value["report"]["fault_policy"]["rate"]["per_mille"], 5,
        "{value}"
    );
    // The count, its rate and the allowance are in the progress block.
    let log = std::fs::read_to_string(run.join("run.log")).unwrap();
    assert!(log.contains("4 of 3 allowed"), "{log}");
}

/// Every complete journal line, parsed.
fn journal_lines(run: &std::path::Path) -> Vec<Value> {
    std::fs::read_to_string(run.join("games.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn run_files(run: &std::path::Path) -> BTreeMap<std::ffi::OsString, Vec<u8>> {
    std::fs::read_dir(run)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| (entry.file_name(), std::fs::read(entry.path()).unwrap()))
        .collect()
}

/// A budget in games derives the iteration count, is stored in the resolved
/// configuration, and cannot be given together with an iteration count.
#[test]
fn a_game_budget_sets_the_horizon_and_is_refused_when_it_does_not_divide() {
    let root = tempfile::tempdir().unwrap();
    let tune = root.path().join("tune.toml");
    std::fs::write(
        &tune,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    let executable = env!("CARGO_BIN_EXE_colosseum-cli");
    let spsa = |extra: &[&str]| {
        Command::new(executable)
            .args(["--json", "--dry-run", "spsa"])
            .arg(executable)
            .arg("--engine-arg=__uci-stub")
            .arg("--tune")
            .arg(&tune)
            .args(["--r-end", "0.002", "--depth", "1"])
            .args(extra)
            .output()
            .unwrap()
    };
    let planned = spsa(&["--total-games", "168000", "--games-per-iteration", "42"]);
    assert!(
        planned.status.success(),
        "{}",
        String::from_utf8_lossy(&planned.stderr)
    );
    let value: Value = serde_json::from_slice(&planned.stdout).unwrap();
    let configuration = &value["resolved_configuration"];
    assert_eq!(configuration["settings"]["iterations"], 4_000);
    assert_eq!(configuration["total_games"], 168_000);

    let refused = spsa(&["--total-games", "160000", "--games-per-iteration", "42"]);
    assert_eq!(refused.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("--total-games 160000"),
        "{}",
        String::from_utf8_lossy(&refused.stderr)
    );

    let both = spsa(&["--total-games", "168000", "--iterations", "10"]);
    assert_eq!(both.status.code(), Some(2));
}

/// `spsa history` rebuilds the centre vector after every iteration from the
/// journal: its rows are the centres the tune committed, in knob order, as
/// JSON, CSV or a table, thinned with `--every`.
#[test]
fn spsa_history_prints_the_centres_the_tune_committed_after_every_iteration() {
    let root = tempfile::tempdir().unwrap();
    let tune = write_tune(root.path());
    let run = root.path().join("run");
    let tuned = cli()
        .arg("--json")
        .arg("spsa")
        .arg(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
        .arg("--tune")
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            "3",
            "--games-per-iteration",
            "2",
            "--depth",
            "1",
            "--seed",
            "7",
            "--__synthetic-games",
            "--dir",
        ])
        .arg(&run)
        .output()
        .unwrap();
    assert!(
        tuned.status.success(),
        "{}",
        String::from_utf8_lossy(&tuned.stderr)
    );
    let committed: Value = serde_json::from_slice(&tuned.stdout).unwrap();
    let committed = committed["report"]["driver"]["completed_iterations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|iteration| iteration["centers_after"].clone())
        .collect::<Vec<_>>();
    assert_eq!(committed.len(), 3);

    let history = cli()
        .args(["--json", "spsa", "history"])
        .arg(&run)
        .output()
        .unwrap();
    assert!(
        history.status.success(),
        "{}",
        String::from_utf8_lossy(&history.stderr)
    );
    let history: Value = serde_json::from_slice(&history.stdout).unwrap();
    let report = &history["report"];
    assert_eq!(report["knobs"], serde_json::json!(["Hash"]));
    assert_eq!(report["completed_iterations"], 3);
    let rows = report["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["iteration"], 0);
    assert_eq!(rows[0]["centers"], serde_json::json!([16.0]));
    for (row, centers) in rows[1..].iter().zip(&committed) {
        assert_eq!(&row["centers"], centers, "{row}");
    }

    let csv = cli()
        .args(["spsa", "history", "--csv", "--every", "2"])
        .arg(&run)
        .output()
        .unwrap();
    assert!(csv.status.success());
    let csv = String::from_utf8(csv.stdout).unwrap();
    let lines = csv.lines().collect::<Vec<_>>();
    assert_eq!(lines[0], "iteration,Hash");
    let iterations = lines[1..]
        .iter()
        .map(|line| line.split(',').next().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(iterations, ["0", "2", "3"]);

    let both = cli()
        .args(["--json", "spsa", "history", "--csv"])
        .arg(&run)
        .output()
        .unwrap();
    assert_eq!(both.status.code(), Some(2));
}

/// A synthetic two-knob tune in `run`, run to its end or stopped after one
/// iteration. The stub engine advertises both knobs.
fn synthetic_two_knob_tune(root: &std::path::Path, run: &std::path::Path, stop: bool) {
    let tune = write_tune_contents(
        root,
        r#"
[[parameters]]
name = "Hash"
initial = 16
min = 1
max = 1024
c_end = 1.0

[[parameters]]
name = "Threads"
initial = 1
min = 1
max = 8
c_end = 1.0
"#,
    );
    let mut command = cli();
    if stop {
        command.args(["--__stop-after-units", "1"]);
    }
    let output = command
        .arg("--json")
        .arg("spsa")
        .arg(env!("CARGO_BIN_EXE_colosseum-cli"))
        .arg("--engine-arg=__uci-stub")
        .arg("--tune")
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            "3",
            "--games-per-iteration",
            "2",
            "--depth",
            "1",
            "--seed",
            "7",
            "--__synthetic-games",
            "--dir",
        ])
        .arg(run)
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(if stop { 6 } else { 0 }),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn seeded_dry_run(
    source: &std::path::Path,
    tune: Option<&std::path::Path>,
) -> std::process::Output {
    let mut command = cli();
    command
        .args(["--json", "--dry-run", "spsa"])
        .arg(env!("CARGO_BIN_EXE_colosseum-cli"))
        .arg("--engine-arg=__uci-stub")
        .arg("--seed-from")
        .arg(source);
    if let Some(tune) = tune {
        command.arg("--tune").arg(tune);
    }
    command
        .args(["--r-end", "0.002", "--iterations", "10", "--depth", "1"])
        .output()
        .unwrap()
}

/// A seeded tune starts from the completed tune's rounded final values on its
/// surface, and records where they came from.
#[test]
fn a_seeded_tune_starts_from_a_completed_tunes_rounded_final_values() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    synthetic_two_knob_tune(root.path(), &source, false);
    let result: Value =
        serde_json::from_slice(&std::fs::read(source.join("result.json")).unwrap()).unwrap();
    let tuned = result["tuned_result"]["parameters"]
        .as_array()
        .unwrap()
        .clone();

    let seeded = seeded_dry_run(&source, None);
    assert!(
        seeded.status.success(),
        "{}",
        String::from_utf8_lossy(&seeded.stderr)
    );
    let value: Value = serde_json::from_slice(&seeded.stdout).unwrap();
    let tune = &value["resolved_configuration"]["tune"];
    let parameters = tune["parameters"].as_array().unwrap();
    assert_eq!(parameters.len(), 2);
    for (parameter, result) in parameters.iter().zip(&tuned) {
        assert_eq!(parameter["name"], result["name"]);
        assert_eq!(parameter["initial"], result["tuned"], "{parameter}");
        assert_eq!(parameter["min"], result["min"]);
        assert_eq!(parameter["max"], result["max"]);
    }
    assert_eq!(tune["seeded_from"]["completed_iterations"], 3);
    assert_eq!(
        tune["seeded_from"]["result_sha256"].as_str().unwrap().len(),
        64
    );

    // A tune file with the same names replaces the surface and keeps the seed.
    let narrowed = root.path().join("narrowed.toml");
    std::fs::write(
        &narrowed,
        "[[parameters]]\nname = \"Hash\"\ninitial = 1\nmin = 1\nmax = 2048\nc_end = 2.0\n\n\
         [[parameters]]\nname = \"Threads\"\ninitial = 1\nmin = 1\nmax = 8\nc_end = 0.5\n",
    )
    .unwrap();
    let reseeded = seeded_dry_run(&source, Some(&narrowed));
    assert!(
        reseeded.status.success(),
        "{}",
        String::from_utf8_lossy(&reseeded.stderr)
    );
    let value: Value = serde_json::from_slice(&reseeded.stdout).unwrap();
    let parameters = &value["resolved_configuration"]["tune"]["parameters"];
    assert_eq!(parameters[0]["initial"], tuned[0]["tuned"]);
    assert_eq!(parameters[0]["max"], 2048);
    assert_eq!(parameters[1]["c_end"], 0.5);
}

/// Only a completed tune seeds another, and a tune file must name the seeded
/// parameters in the same order.
#[test]
fn seed_from_refuses_an_unfinished_tune_and_a_tune_file_with_other_names() {
    let root = tempfile::tempdir().unwrap();
    let stopped = root.path().join("stopped");
    synthetic_two_knob_tune(root.path(), &stopped, true);
    let refused = seeded_dry_run(&stopped, None);
    assert_eq!(refused.status.code(), Some(2));
    let message = String::from_utf8_lossy(&refused.stderr);
    assert!(
        message.contains("only a completed tune seeds a new one"),
        "{message}"
    );

    let source = root.path().join("source");
    synthetic_two_knob_tune(root.path(), &source, false);
    let reordered = root.path().join("reordered.toml");
    std::fs::write(
        &reordered,
        "[[parameters]]\nname = \"Threads\"\ninitial = 1\nmin = 1\nmax = 8\nc_end = 1.0\n\n\
         [[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    let refused = seeded_dry_run(&source, Some(&reordered));
    assert_eq!(refused.status.code(), Some(2));
    let message = String::from_utf8_lossy(&refused.stderr);
    assert!(
        message.contains("the names and their order must match"),
        "{message}"
    );
}
