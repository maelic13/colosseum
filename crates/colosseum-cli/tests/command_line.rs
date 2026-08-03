use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

#[test]
fn version_reports_cli_package_version() {
    let output = cli().arg("--version").output().unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        format!("colosseum-cli {}", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_is_headless_and_names_the_product() {
    let output = cli().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("ordinary UCI chess-engine executables"));
    assert!(stdout.contains("Usage: colosseum-cli"));
    assert!(stdout.contains("capabilities"));
    assert!(stdout.contains("match"));
    assert!(stdout.contains("sprt"));
    assert!(stdout.contains("spsa"));
    assert!(stdout.contains("nps"));
    assert!(stdout.contains("book"));
    assert!(stdout.contains("stats"));
    assert!(stdout.contains("calibrate"));
    assert!(output.stderr.is_empty());
}

#[test]
fn stats_replay_obeys_source_authority_and_never_pairs_pgn_by_guessing() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("match");
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let played = cli()
        .args(["match", "--games", "4"])
        .arg(binary)
        .arg(binary)
        .args([
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            "--max-moves",
            "2",
            "--placement",
            "off",
            "--dir",
        ])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        played.status.success(),
        "{}",
        String::from_utf8_lossy(&played.stderr)
    );

    let structured = cli().arg("stats").arg(&run).arg("--json").output().unwrap();
    assert!(structured.status.success());
    let structured: serde_json::Value = serde_json::from_slice(&structured.stdout).unwrap();
    assert_eq!(structured["report"]["authority"], "structured-run-store");
    assert_eq!(structured["report"]["pairing"], "paired");
    assert_eq!(structured["report"]["complete_pairs"], 2);
    assert_eq!(structured["report"]["unpaired_games"], 0);
    assert_eq!(
        structured["report"]["pentanomial"],
        serde_json::json!([0, 0, 2, 0, 0])
    );

    std::fs::write(run.join("result.json"), "not json").unwrap();
    let checkpoint = cli().arg("stats").arg(&run).arg("--json").output().unwrap();
    assert!(checkpoint.status.success());
    let checkpoint: serde_json::Value = serde_json::from_slice(&checkpoint.stdout).unwrap();
    assert_eq!(checkpoint["report"]["attempts"][0]["accepted"], false);
    assert_eq!(checkpoint["report"]["attempts"][1]["accepted"], true);
    assert_eq!(checkpoint["report"]["complete_pairs"], 2);

    std::fs::remove_file(run.join("result.json")).unwrap();
    std::fs::remove_file(run.join("checkpoint.json")).unwrap();
    if run.join("checkpoint.previous.json").exists() {
        std::fs::remove_file(run.join("checkpoint.previous.json")).unwrap();
    }
    let pgn = cli().arg("stats").arg(&run).arg("--json").output().unwrap();
    assert!(pgn.status.success());
    let pgn: serde_json::Value = serde_json::from_slice(&pgn.stdout).unwrap();
    assert_eq!(pgn["report"]["authority"], "pgn-export");
    assert_eq!(pgn["report"]["pairing"], "unpaired");
    assert_eq!(pgn["report"]["complete_pairs"], 0);
    assert_eq!(pgn["report"]["unpaired_games"], 4);
}

#[test]
fn statistics_plans_are_explicit_and_seed_reproducible() {
    let fixed = cli()
        .args([
            "stats",
            "plan",
            "fixed",
            "--objective",
            "difference",
            "--model",
            "normalized",
            "--effect-or-margin",
            "5",
            "--distribution",
            "0.05,0.2,0.5,0.2,0.05",
            "--observed-pentanomial",
            "5,20,50,20,5",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        fixed.status.success(),
        "{}",
        String::from_utf8_lossy(&fixed.stderr)
    );
    let fixed: serde_json::Value = serde_json::from_slice(&fixed.stdout).unwrap();
    assert_eq!(fixed["type"], "stats-fixed-plan");
    assert_eq!(
        fixed["report"]["required_games"].as_u64().unwrap(),
        fixed["report"]["required_pairs"].as_u64().unwrap() * 2
    );
    assert_eq!(fixed["report"]["achieved_resolution"]["pairs"], 100);

    let run = || {
        cli()
            .args([
                "stats",
                "plan",
                "sprt",
                "--model",
                "normalized",
                "--elo0",
                "0",
                "--elo1",
                "5",
                "--distribution",
                "0.05,0.2,0.5,0.2,0.05",
                "--simulations",
                "20",
                "--max-pairs",
                "20",
                "--seed",
                "42",
                "--json",
            ])
            .output()
            .unwrap()
    };
    let first = run();
    let second = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(first.stdout, second.stdout);
    let sprt: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(sprt["type"], "stats-sprt-plan");
    assert_eq!(
        sprt["report"]["accepted_h0"].as_u64().unwrap()
            + sprt["report"]["accepted_h1"].as_u64().unwrap()
            + sprt["report"]["capped"].as_u64().unwrap(),
        20
    );
    assert!(
        sprt["report"]["interpretation"]
            .as_str()
            .unwrap()
            .contains("not a stopping guarantee")
    );
}

#[test]
fn pgn_statistics_report_search_telemetry_without_counting_opening_moves() {
    let root = tempfile::tempdir().unwrap();
    let pgn = root.path().join("telemetry.pgn");
    std::fs::write(
        &pgn,
        r#"[Event "telemetry"]
[White "A"]
[Black "B"]
[Result "1/2-1/2"]
[OpeningPlyCount "2"]

1. e4 {[%depth 1] [%emt 1] [%nodes 1] unknown-preserved} e5 {d=1 t=1s n=1}
2. Nf3 {[%depth 10] [%emt 0.5] [%nodes 500]} Nc6 {d=20 t=250ms n=1000}
3. Bb5 {[%depth 30] [%emt 1.5] [%nodes 3000]} a6 {d=40 t=0.75s n=1500} 1/2-1/2
"#,
    )
    .unwrap();
    let output = cli().arg("stats").arg(&pgn).arg("--json").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let telemetry = &output["report"]["telemetry"];
    assert_eq!(telemetry["status"], "available");
    assert_eq!(telemetry["excluded_opening_moves"], 2);
    assert_eq!(telemetry["engines"][0]["engine"], "A");
    assert_eq!(telemetry["engines"][0]["eligible_moves"], 2);
    assert_eq!(telemetry["engines"][0]["depth"]["mean"], 20.0);
    assert_eq!(telemetry["engines"][0]["implied_nps"]["median"], 1500.0);
    assert!(
        output["report"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("node accounting"))
    );
}

#[test]
fn book_tools_hash_verify_stats_and_slice_without_an_engine() {
    let root = tempfile::tempdir().unwrap();
    let input = root.path().join("openings.epd");
    std::fs::write(
        &input,
        "8/8/8/8/8/8/K7/7k w - - ce 20;\n8/8/8/8/8/8/1K6/7k w - -\n8/8/8/8/8/8/2K5/7k w - -\n",
    )
    .unwrap();

    let hash = cli()
        .args(["book", "hash"])
        .arg(&input)
        .arg("--json")
        .output()
        .unwrap();
    assert!(hash.status.success());
    let hash: serde_json::Value = serde_json::from_slice(&hash.stdout).unwrap();
    assert_eq!(hash["type"], "book-hash");
    assert_eq!(hash["report"]["sha256"].as_str().unwrap().len(), 64);

    let stats = cli()
        .args(["book", "stats"])
        .arg(&input)
        .arg("--json")
        .output()
        .unwrap();
    assert!(stats.status.success());
    let stats: serde_json::Value = serde_json::from_slice(&stats.stdout).unwrap();
    assert_eq!(stats["report"]["usable"], 3);
    assert_eq!(stats["report"]["unique"], 3);
    assert_eq!(stats["report"]["mean_plies"], 0.0);
    assert_eq!(stats["report"]["eval_band"]["samples"], 1);
    assert_eq!(stats["report"]["eval_band"]["mean"], 20.0);

    let verified = cli()
        .args(["book", "verify"])
        .arg(&input)
        .arg("--json")
        .output()
        .unwrap();
    assert!(verified.status.success());
    let verified: serde_json::Value = serde_json::from_slice(&verified.stdout).unwrap();
    assert_eq!(verified["audit"]["rejected_indices"], serde_json::json!([]));

    let first = root.path().join("first.epd");
    let second = root.path().join("second.epd");
    for output in [&first, &second] {
        let sliced = cli()
            .args(["book", "slice"])
            .arg(&input)
            .arg(output)
            .args([
                "--count", "2", "--order", "random", "--seed", "42", "--json",
            ])
            .output()
            .unwrap();
        assert!(
            sliced.status.success(),
            "{}",
            String::from_utf8_lossy(&sliced.stderr)
        );
        let sliced: serde_json::Value = serde_json::from_slice(&sliced.stdout).unwrap();
        assert_eq!(sliced["report"]["written"], 2);
    }
    assert_eq!(
        std::fs::read(first).unwrap(),
        std::fs::read(second).unwrap()
    );

    let invalid = root.path().join("invalid.epd");
    std::fs::write(&invalid, "not an epd\n").unwrap();
    let rejected = cli()
        .args(["book", "verify"])
        .arg(&invalid)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(1));
    let rejected: serde_json::Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(
        rejected["audit"]["rejected_indices"],
        serde_json::json!([1])
    );
}

#[test]
fn nps_uses_fixed_nodes_and_exposes_engine_claim_as_diagnostic_only() {
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let output = cli()
        .arg("nps")
        .arg(binary)
        .args([
            "--nodes",
            "1000",
            "--engine-arg=__uci-stub",
            "--engine-arg=--reported-nps",
            "--engine-arg=18446744073709551615",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("startpos alone is a weak workload"));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "nps");
    assert_eq!(value["report"]["requested_nodes"], 1000);
    assert_eq!(value["report"]["reported_nodes"], 1000);
    assert_eq!(
        value["report"]["engine_reported_nps"],
        serde_json::Value::from(u64::MAX)
    );
    assert!(value["report"]["harness_elapsed_ns"].as_u64().unwrap() > 0);
    assert!(
        value["report"]["authoritative_nps"]
            .as_f64()
            .unwrap()
            .is_finite()
    );
}

#[test]
fn nps_comparison_records_seeded_warm_schedule_and_robust_summaries() {
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let output = cli()
        .arg("nps")
        .arg(binary)
        .args([
            "--nodes",
            "1000",
            "--self-pair",
            "--repetitions",
            "2",
            "--warmup",
            "1",
            "--state",
            "warm",
            "--seed",
            "42",
            "--bootstrap-samples",
            "100",
            "--engine-arg=__uci-stub",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "nps-comparison");
    assert_eq!(value["report"]["design"]["seed"], 42);
    assert_eq!(value["report"]["design"]["state_policy"], "warm");
    assert_eq!(value["report"]["schedule"].as_array().unwrap().len(), 6);
    assert_eq!(value["report"]["samples"].as_array().unwrap().len(), 4);
    assert_eq!(value["report"]["arms"].as_array().unwrap().len(), 2);
    assert!(
        value["report"]["arms"]
            .as_array()
            .unwrap()
            .iter()
            .all(|arm| arm["builds"][0]["samples"] == 2)
    );
}

#[test]
fn calibration_dry_run_records_the_optional_default_design_and_binary_identity() {
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let output = cli()
        .arg("calibrate")
        .arg(binary)
        .arg(binary)
        .args([
            "--dry-run",
            "--json",
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "dry-run");
    assert_eq!(value["command"], "calibrate");
    let config = &value["resolved_configuration"];
    assert_eq!(config["design"]["games"], 30_000);
    assert_eq!(config["design"]["confidence"], 0.95);
    assert_eq!(config["design"]["tolerance_nelo"], 5.0);
    assert_eq!(
        config["binaries"]["engine_a_sha256"],
        config["binaries"]["engine_b_sha256"]
    );
    assert_eq!(
        config["binaries"]["engine_a_sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
}

#[test]
fn calibration_refuses_nonidentical_executable_content_before_launch() {
    let cli_binary = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let fixture = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"));
    let output = cli()
        .args(["calibrate", "--dry-run"])
        .arg(cli_binary)
        .arg(fixture)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("byte-identical executables")
    );
}

#[test]
fn calibration_persists_a_degenerate_identical_binary_run_as_inconclusive() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("calibration");
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let output = cli()
        .arg("calibrate")
        .arg(binary)
        .arg(binary)
        .args([
            "--games",
            "4",
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            "--max-moves",
            "2",
            "--no-resign-adjudication",
            "--dir",
        ])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "calibration");
    assert_eq!(value["report"]["status"], "inconclusive");
    assert!(value["report"]["interval"].is_null());
    assert!(
        value["report"]["statistics_unavailable"]
            .as_str()
            .unwrap()
            .contains("zero variance")
    );
    for artifact in [
        "checkpoint.json",
        "run.log",
        "games.pgn",
        "result.json",
        "run-record.json",
    ] {
        assert!(run.join(artifact).is_file(), "missing {artifact}");
    }
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["command"], "calibrate");
    assert_eq!(record["status"], "completed");
    assert_eq!(record["official_sample"]["completed_pairs"], 2);
}

#[test]
fn calibration_marks_any_engine_fault_invalid_even_when_the_match_policy_allows_it() {
    let root = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"));
    let output = cli()
        .arg("calibrate")
        .arg(fixture)
        .arg(fixture)
        .args(["--games", "2", "--max-engine-faults", "2", "--dir"])
        .arg(root.path().join("invalid"))
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["report"]["status"], "invalid");
    assert!(
        value["report"]["fixed_match"]["faults"]["engine_b"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn sprt_named_bundles_expand_to_explicit_finite_designs() {
    for (preset, elo0, elo1) in [("gainer", 0.0, 5.0), ("simplify", -5.0, 0.0)] {
        let output = cli()
            .args([
                "sprt",
                "a",
                "b",
                "--max-pairs",
                "1000",
                "--preset",
                preset,
                "--dry-run",
                "--json",
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let design = &value["resolved_configuration"]["design"];
        assert_eq!(design["parameters"]["model"], "normalized");
        assert_eq!(design["parameters"]["elo0"], elo0);
        assert_eq!(design["parameters"]["elo1"], elo1);
        assert_eq!(design["parameters"]["alpha"], 0.05);
        assert_eq!(design["parameters"]["beta"], 0.05);
        assert_eq!(design["max_pairs"], 1000);
        assert!(design["lower_bound"].as_f64().unwrap() < 0.0);
        assert!(design["upper_bound"].as_f64().unwrap() > 0.0);
    }
}

#[test]
fn sprt_custom_design_requires_and_reports_every_statistical_input() {
    let output = cli()
        .args([
            "sprt",
            "candidate",
            "baseline",
            "--max-pairs",
            "750",
            "--model",
            "logistic",
            "--elo0",
            "-2",
            "--elo1",
            "3",
            "--alpha",
            "0.01",
            "--beta",
            "0.1",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let design = &value["resolved_configuration"]["design"];
    assert_eq!(design["parameters"]["model"], "logistic");
    assert_eq!(design["parameters"]["elo0"], -2.0);
    assert_eq!(design["parameters"]["elo1"], 3.0);
    assert_eq!(design["parameters"]["alpha"], 0.01);
    assert_eq!(design["parameters"]["beta"], 0.1);
    assert!(design["bundle"].is_null());
}

#[test]
fn sprt_refuses_missing_or_invalid_design_before_launch() {
    for arguments in [
        vec!["sprt", "a", "b", "--max-pairs", "10", "--dry-run"],
        vec![
            "sprt",
            "a",
            "b",
            "--max-pairs",
            "10",
            "--preset",
            "gainer",
            "--elo1",
            "0",
            "--dry-run",
        ],
    ] {
        let output = cli().args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("configuration error")
        );
    }
}

#[test]
fn sprt_live_report_is_pair_atomic_durable_and_inconclusive_at_its_cap() {
    let root = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"));
    let run = root.path().join("sprt");
    let output = cli()
        .args(["sprt"])
        .arg(fixture)
        .arg(fixture)
        .args([
            "--max-pairs",
            "2",
            "--preset",
            "gainer",
            "--max-engine-faults",
            "4",
            "--seed",
            "7",
            "--dir",
        ])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(4));
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "sprt");
    assert_eq!(value["report"]["status"], "inconclusive");
    assert_eq!(
        value["report"]["design"]["parameters"]["model"],
        "normalized"
    );
    assert_eq!(
        value["report"]["schedule"]["official_pairs"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        value["report"]["schedule"]["post_terminal_pairs"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
    assert!(value["report"]["schedule"]["statistics"].is_null());
    for artifact in [
        "checkpoint.json",
        "run.log",
        "games.pgn",
        "result.json",
        "run-record.json",
    ] {
        assert!(run.join(artifact).is_file(), "missing {artifact}");
    }
}

#[test]
fn sprt_live_exit_distinguishes_invalid_and_infrastructure_error() {
    let root = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"));
    let invalid = cli()
        .arg("sprt")
        .arg(fixture)
        .arg(fixture)
        .args(["--max-pairs", "10", "--preset", "gainer", "--dir"])
        .arg(root.path().join("invalid"))
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(5));
    let value: serde_json::Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(value["report"]["status"], "invalid");
    assert_eq!(value["report"]["schedule"]["invalid_pair"], 1);

    let error = cli()
        .args([
            "sprt",
            "missing-a",
            "missing-b",
            "--max-pairs",
            "2",
            "--preset",
            "gainer",
            "--dir",
        ])
        .arg(root.path().join("error"))
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(error.status.code(), Some(3));
    assert!(error.stdout.is_empty());
    assert!(
        String::from_utf8(error.stderr)
            .unwrap()
            .contains("SPRT failed")
    );
}

#[test]
fn fixed_match_accepts_the_same_ordinary_uci_path_with_different_side_options() {
    let root = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"));
    let output = cli()
        .args(["match", "--games", "2"])
        .arg(fixture)
        .arg(fixture)
        .arg("--dir")
        .arg(root.path().join("run"))
        .args([
            "--a-option",
            "Hash=16",
            "--b-option",
            "Hash=32",
            "--max-engine-faults",
            "2",
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
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "fixed-match");
    assert_eq!(
        value["run_directory"],
        root.path().join("run").to_string_lossy().as_ref()
    );
    assert_eq!(value["report"]["games_requested"], 2);
    assert_eq!(value["report"]["games_completed"], 2);
    assert_eq!(value["report"]["status"], "completed");
    assert_eq!(value["report"]["games"][0]["white"], "a");
    assert_eq!(value["report"]["games"][1]["white"], "b");
    let clock = &value["report"]["games"][0]["clock_accounting"];
    assert_eq!(clock["model"], "go-write-to-bestmove-read");
    assert_eq!(clock["version"], 1);
    assert!(clock["monotonic_resolution_ns"].as_u64().unwrap() > 0);
    assert!(clock["white_charged_elapsed"]["samples"].as_u64().unwrap() > 0);
    let run = root.path().join("run");
    for artifact in [
        "resolved-config.json",
        "config.sha256",
        "run-record.json",
        "checkpoint.json",
        "run.log",
        "games.pgn",
        "result.json",
    ] {
        assert!(run.join(artifact).is_file(), "missing {artifact}");
    }
    assert!(
        std::fs::read_to_string(run.join("run.log"))
            .unwrap()
            .contains("match-finished")
    );
    assert!(
        std::fs::read_to_string(run.join("games.pgn"))
            .unwrap()
            .contains("[Event \"Colosseum CLI fixed match\"]")
    );
}

#[test]
fn fixed_match_strict_default_invalidates_on_the_first_engine_fault() {
    let root = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"));
    let output = cli()
        .args(["match", "--games", "2"])
        .arg(fixture)
        .arg(fixture)
        .arg("--dir")
        .arg(root.path().join("run"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["report"]["status"], "invalid");
    assert_eq!(value["report"]["games_attempted"], 2);
    assert_eq!(value["report"]["games_completed"], 2);
    assert_eq!(value["report"]["faults"]["engine_b"], 1);
    assert_eq!(value["report"]["games"][0]["scorable"], true);
    assert_eq!(value["report"]["games"][0]["fault"]["cause"], "engine");
}

#[test]
fn fixed_match_never_scores_an_engine_spawn_failure() {
    let root = tempfile::tempdir().unwrap();
    let output = cli()
        .args(["match", "--games", "2"])
        .arg(root.path().join("missing-a"))
        .arg(root.path().join("missing-b"))
        .arg("--dir")
        .arg(root.path().join("run"))
        .arg("--json")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["report"]["status"], "infrastructure-error");
    assert_eq!(value["report"]["games_attempted"], 2);
    assert_eq!(value["report"]["games_completed"], 0);
    assert_eq!(value["report"]["faults"]["infrastructure"], 2);
    assert_eq!(value["report"]["games"][0]["scorable"], false);
    assert_eq!(
        value["report"]["games"][0]["fault"]["cause"],
        "infrastructure"
    );
}

#[test]
fn fixed_match_dry_run_resolves_two_direct_engine_invocations_without_launching() {
    let root = tempfile::tempdir().unwrap();
    let missing_a = root.path().join("missing-a");
    let missing_b = root.path().join("missing-b");
    let output = cli()
        .args(["match", "--games", "3", "--dry-run", "--json"])
        .arg(&missing_a)
        .arg(&missing_b)
        .args(["--a-label", "candidate", "--b-option", "Hash=64"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "dry-run");
    assert_eq!(value["command"], "match");
    assert_eq!(value["invocations"].as_array().unwrap().len(), 2);
    assert_eq!(value["resolved_configuration"]["games"], 3);
    assert_eq!(value["invocations"][0]["label"], "candidate");
    assert_eq!(value["invocations"][1]["options"]["Hash"]["kind"], "string");
}

#[test]
fn fixed_match_resolves_independent_time_controls_and_margins() {
    let output = cli()
        .args([
            "match",
            "--games",
            "2",
            "missing-a",
            "missing-b",
            "--a-base-ms",
            "5000",
            "--a-increment-ms",
            "50",
            "--a-margin-ms",
            "25",
            "--b-nodes",
            "10000",
            "--b-margin-ms",
            "0",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let config = &value["resolved_configuration"];
    assert_eq!(
        config["engine_a_time_control"]["control"]["Increment"]["base_ms"],
        5000
    );
    assert_eq!(config["engine_a_time_control"]["margin_ms"], 25);
    assert_eq!(
        config["engine_b_time_control"]["control"]["Nodes"]["nodes"],
        10000
    );
    assert_eq!(config["engine_b_time_control"]["margin_ms"], 0);
}

#[test]
fn fixed_match_rejects_ambiguous_or_incomplete_time_controls() {
    for arguments in [
        vec![
            "match",
            "--games",
            "1",
            "a",
            "b",
            "--a-movetime-ms",
            "10",
            "--a-depth",
            "2",
        ],
        vec!["match", "--games", "1", "a", "b", "--b-increment-ms", "10"],
    ] {
        let output = cli().args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("configuration error")
        );
    }
}

#[test]
fn fixed_match_resolves_default_and_disableable_adjudication() {
    let default_output = cli()
        .args(["match", "--games", "1", "a", "b", "--dry-run", "--json"])
        .output()
        .unwrap();
    assert!(default_output.status.success());
    let default: serde_json::Value = serde_json::from_slice(&default_output.stdout).unwrap();
    let adjudication = &default["resolved_configuration"]["adjudication"];
    assert_eq!(adjudication["draw"]["min_ply"], 80);
    assert_eq!(adjudication["draw"]["move_count"], 8);
    assert_eq!(adjudication["draw"]["score_cp"], 10);
    assert_eq!(adjudication["resign"]["move_count"], 3);
    assert_eq!(adjudication["resign"]["score_cp"], 600);
    assert!(adjudication["max_moves"].is_null());

    let disabled_output = cli()
        .args([
            "match",
            "--games",
            "1",
            "a",
            "b",
            "--no-draw-adjudication",
            "--no-resign-adjudication",
            "--max-moves",
            "75",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(disabled_output.status.success());
    let disabled: serde_json::Value = serde_json::from_slice(&disabled_output.stdout).unwrap();
    let adjudication = &disabled["resolved_configuration"]["adjudication"];
    assert!(adjudication["draw"].is_null());
    assert!(adjudication["resign"].is_null());
    assert_eq!(adjudication["max_moves"], 75);
}

#[test]
fn fixed_match_resolves_direct_per_side_cpu_controls() {
    let output = cli()
        .args([
            "match",
            "--games",
            "1",
            "missing-a",
            "missing-b",
            "--a-cores",
            "0",
            "--b-cores",
            "1",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let slots = &value["resolved_configuration"]["execution"]["slots"];
    assert_eq!(slots[0]["engine_a"]["allocation"]["cpus"][0]["number"], 0);
    assert_eq!(slots[0]["engine_b"]["allocation"]["cpus"][0]["number"], 1);
}

#[test]
fn fixed_match_runs_explicit_concurrency_and_reports_hash_lower_bound() {
    let root = tempfile::tempdir().unwrap();
    let fixture = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"));
    let output = cli()
        .args(["match", "--games", "4"])
        .arg(fixture)
        .arg(fixture)
        .arg("--dir")
        .arg(root.path().join("run"))
        .args([
            "--concurrency",
            "2",
            "--placement",
            "off",
            "--a-option",
            "Hash=16",
            "--b-option",
            "Hash=32",
            "--memory-budget-mb",
            "96",
            "--max-engine-faults",
            "4",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["report"]["execution"]["concurrency"], 2);
    assert_eq!(
        value["report"]["execution"]["hash_memory"]["lower_bound_mb"],
        96
    );
    assert_eq!(
        value["report"]["games"]
            .as_array()
            .unwrap()
            .iter()
            .map(|game| game["number"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [1, 2, 3, 4]
    );
}

#[test]
fn fixed_match_pairs_optional_book_openings_and_reports_reuse() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("openings.epd");
    std::fs::write(
        &book,
        "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -\n\
rnbqkb1r/pppppppp/5n2/8/8/5N2/PPPPPPPP/RNBQKB1R w KQkq -\n",
    )
    .unwrap();
    let fixture = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"));
    let output = cli()
        .args(["match", "--games", "5"])
        .arg(fixture)
        .arg(fixture)
        .arg("--dir")
        .arg(root.path().join("run"))
        .arg("--book")
        .arg(&book)
        .args([
            "--book-start",
            "0",
            "--seed",
            "42",
            "--max-engine-faults",
            "5",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let report = &value["report"];
    assert_eq!(report["master_seed"], 42);
    assert_eq!(report["master_seed_generated"], false);
    assert_eq!(report["openings"]["mode"], "book");
    assert_eq!(report["openings"]["scheduled_pairs"], 3);
    assert_eq!(report["openings"]["reused_pair_assignments"], 1);
    assert_eq!(report["games"][0]["opening"]["book_index"], 0);
    assert_eq!(report["games"][1]["opening"]["book_index"], 0);
    assert_eq!(report["games"][2]["opening"]["book_index"], 1);
    assert_eq!(report["games"][3]["opening"]["book_index"], 1);
    assert_eq!(report["games"][4]["opening"]["book_index"], 0);
}

#[test]
fn fixed_match_no_book_reports_diversity_warning() {
    let output = cli()
        .args([
            "match",
            "--games",
            "1",
            "a",
            "b",
            "--seed",
            "7",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let openings = &value["resolved_configuration"]["openings"];
    assert_eq!(openings["mode"], "startpos");
    assert!(openings["warning"].as_str().unwrap().contains("diversity"));
}

#[test]
fn fixed_match_refuses_only_against_an_explicit_trusted_memory_budget() {
    let output = cli()
        .args([
            "match",
            "--games",
            "1",
            "a",
            "b",
            "--a-option",
            "Hash=64",
            "--b-option",
            "Hash=64",
            "--memory-budget-mb",
            "127",
            "--dry-run",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("exceeds trusted budget")
    );
}

#[test]
fn engine_subcommand_help_exposes_direct_controls() {
    for action in ["inspect", "check"] {
        let output = cli().args(["engine", action, "--help"]).output().unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        for control in [
            "<EXECUTABLE>",
            "--label",
            "--engine-arg",
            "--cwd",
            "--env",
            "--option",
            "--button",
            "--cores",
        ] {
            assert!(stdout.contains(control), "{action} help omitted {control}");
        }
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn inspect_launch_failure_is_nonzero_and_diagnostic() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing-uci-engine");
    let output = cli()
        .args(["engine", "inspect"])
        .arg(missing)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("engine inspect failed")
    );
}

#[test]
fn dry_run_resolves_a_missing_engine_without_launching_it() {
    let root = tempfile::tempdir().unwrap();
    let missing = root.path().join("missing-uci-engine");
    let output = cli()
        .args(["--json", "engine", "inspect", "--dry-run"])
        .arg(&missing)
        .args([
            "--engine-arg=--uci",
            "--env",
            "MODE=test",
            "--option",
            "Hash=32",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["type"], "dry-run");
    assert_eq!(value["command"], "engine-inspect");
    assert_eq!(value["invocations"][0]["arguments"][0], "--uci");
    assert_eq!(value["invocations"][0]["environment"]["MODE"], "test");
    assert!(value["config_sha256"].as_str().unwrap().len() == 64);
    assert!(
        value["resolved_configuration"]["engine"]["executable"]
            .as_str()
            .unwrap()
            .contains("missing-uci-engine")
    );
}

#[test]
fn json_failure_keeps_stdout_empty_and_diagnostics_on_stderr() {
    let root = tempfile::tempdir().unwrap();
    let output = cli()
        .args(["engine", "inspect", "--json"])
        .arg(root.path().join("missing"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("engine inspect failed")
    );
}

#[test]
fn dry_run_is_rejected_for_read_only_commands() {
    for arguments in [
        vec!["capabilities", "--dry-run", "--json"],
        vec!["self-test", "--dry-run", "--json"],
        vec!["status", "--dry-run", "--json", "missing-run"],
    ] {
        let output = cli().args(arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("not meaningful")
        );
    }
}

#[test]
fn capabilities_reports_platform_state_in_strict_json() {
    let output = cli().args(["capabilities", "--json"]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "capabilities");
    assert_eq!(value["report"]["schema_version"], 1);
    assert_eq!(value["report"]["platform"], std::env::consts::OS);
    assert!(matches!(
        value["report"]["topology"]["status"].as_str(),
        Some("available" | "unavailable")
    ));
    assert!(matches!(
        value["report"]["hard_affinity"]["level"].as_str(),
        Some("enforced" | "unavailable")
    ));
}

#[test]
fn capabilities_text_is_human_readable() {
    let output = cli().arg("capabilities").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for heading in [
        "platform:",
        "topology:",
        "allowed logical CPUs:",
        "hard affinity:",
    ] {
        assert!(stdout.contains(heading), "missing {heading} in {stdout}");
    }
    assert!(output.stderr.is_empty());
}

#[test]
fn copied_executable_passes_headless_self_test_in_isolated_directory() {
    let root = tempfile::tempdir().unwrap();
    let source = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let copied = root.path().join(if cfg!(windows) {
        "colosseum-cli.exe"
    } else {
        "colosseum-cli"
    });
    std::fs::copy(source, &copied).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&copied).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&copied, permissions).unwrap();
    }
    let output = Command::new(&copied)
        .args(["self-test", "--json"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "self-test");
    assert_eq!(value["report"]["success"], true);
    assert_eq!(value["report"]["checks"].as_array().unwrap().len(), 5);
}
