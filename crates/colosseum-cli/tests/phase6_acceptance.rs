use std::path::Path;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

#[test]
fn cold_restarts_each_sample_while_warm_reuses_one_session_per_arm() {
    let root = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"));
    let run = |state: &str, pid_file: &Path| {
        let output = cli()
            .arg("nps")
            .arg(fixture)
            .args([
                "--nodes",
                "1",
                "--self-pair",
                "--repetitions",
                "2",
                "--warmup",
                "0",
                "--state",
                state,
                "--bootstrap-samples",
                "10",
                "--engine-arg=--legal-sequence",
                "--engine-arg=--append-pid-file",
            ])
            .arg(format!("--engine-arg=--pid-file={}", pid_file.display()))
            .arg("--json")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    let warm = root.path().join("warm.pids");
    let cold = root.path().join("cold.pids");
    run("warm", &warm);
    run("cold", &cold);
    let pid_count = |path: &Path| std::fs::read_to_string(path).unwrap().lines().count();
    assert_eq!(pid_count(&warm), 2, "warm keeps one session per A/B arm");
    assert_eq!(
        pid_count(&cold),
        4,
        "cold opens one session per measured sample"
    );
}
