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
    assert!(output.stderr.is_empty());
}

#[test]
fn fixed_match_accepts_the_same_ordinary_uci_path_with_different_side_options() {
    let fixture = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"));
    let output = cli()
        .args(["match", "--games", "2"])
        .arg(fixture)
        .arg(fixture)
        .args(["--a-option", "Hash=16", "--b-option", "Hash=32", "--json"])
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
    assert_eq!(value["report"]["games_requested"], 2);
    assert_eq!(value["report"]["games_completed"], 2);
    assert_eq!(value["report"]["games"][0]["white"], "a");
    assert_eq!(value["report"]["games"][1]["white"], "b");
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
fn fixed_match_rejects_cpu_controls_until_placement_is_composed() {
    let output = cli()
        .args([
            "match",
            "--games",
            "1",
            "missing-a",
            "missing-b",
            "--a-cores",
            "0",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("CPU placement")
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
