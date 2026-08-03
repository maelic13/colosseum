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
    assert!(stdout.contains("calibrate"));
    assert!(output.stderr.is_empty());
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
