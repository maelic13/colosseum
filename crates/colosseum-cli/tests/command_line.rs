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
    assert!(output.stderr.is_empty());
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
