use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Acceptance {
    schema_version: u32,
    phase: String,
    gates: Vec<Gate>,
}

#[derive(Debug, Deserialize)]
struct Gate {
    id: String,
    evidence: String,
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

#[test]
fn acceptance_manifest_names_every_phase_six_exit_gate_and_test_owner() {
    let acceptance: Acceptance = serde_json::from_str(include_str!(
        "../../../docs/fixtures/phase6/acceptance.json"
    ))
    .unwrap();
    assert_eq!(acceptance.schema_version, 1);
    assert_eq!(acceptance.phase, "6");
    let expected = BTreeSet::from([
        "authoritative-wall-clock-and-lie-resistance",
        "skew-robust-arm-estimator",
        "cold-warm-state-policy",
        "scaling-and-hash-policy",
        "book-reproducibility",
        "replay-authority",
        "experiment-planning",
        "pgn-telemetry",
        "position-suite-and-recovery",
        "workspace-regression",
    ]);
    let actual = acceptance
        .gates
        .iter()
        .map(|gate| gate.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert_eq!(
        actual.len(),
        acceptance.gates.len(),
        "duplicate Phase 6 gate ID"
    );
    assert!(
        acceptance
            .gates
            .iter()
            .all(|gate| !gate.evidence.trim().is_empty())
    );
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
