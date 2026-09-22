//! Shared game slots: both engines of a game on one core set by default.
//!
//! Without pondering only one engine of a game searches at any moment and the
//! other waits on a pipe, so pinning them separately leaves half the pool
//! idle. These cases pin the contract that made a one-thread gate at
//! concurrency 14 fit on a 16-core host instead of being refused.
//!
//! Only the command-line contract is asserted here, on every host alike: the
//! core sets a mode hands out, and the pool arithmetic it refuses with, depend
//! on the host's CPUs and are unit-tested against synthetic topologies in
//! `match_runner`.

use std::process::Command;

use serde_json::Value;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn dry_run(arguments: &[&str]) -> Value {
    let output = cli()
        .args(arguments)
        .args(["--dry-run", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{arguments:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn refusal(arguments: &[&str]) -> String {
    let output = cli()
        .args(arguments)
        .args(["--dry-run", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{arguments:?} was accepted");
    assert!(output.stdout.is_empty(), "{arguments:?} wrote to stdout");
    String::from_utf8(output.stderr).unwrap()
}

#[test]
fn the_default_allocation_shares_one_core_set_between_both_engines() {
    let value = dry_run(&["match", "--games", "2", "a", "b"]);
    let execution = &value["resolved_configuration"]["execution"];
    assert_eq!(execution["allocation"]["mode"], "shared");
    assert_eq!(execution["allocation"]["cores_per_game"], 1);
}

#[test]
fn cores_per_engine_selects_the_disjoint_allocation() {
    let value = dry_run(&["match", "--games", "2", "a", "b", "--cores-per-engine", "2"]);
    let execution = &value["resolved_configuration"]["execution"];
    assert_eq!(execution["allocation"]["mode"], "per-engine");
    assert_eq!(execution["allocation"]["cores_per_engine"], 2);

    let value = dry_run(&["match", "--games", "2", "a", "b", "--cores-per-game", "3"]);
    assert_eq!(
        value["resolved_configuration"]["execution"]["allocation"]["cores_per_game"],
        3
    );
}

#[test]
fn the_two_allocation_modes_are_mutually_exclusive() {
    let stderr = refusal(&[
        "match",
        "--games",
        "2",
        "a",
        "b",
        "--cores-per-game",
        "1",
        "--cores-per-engine",
        "1",
    ]);
    assert!(stderr.contains("cannot be used with"), "{stderr}");
}

/// A pondering engine searches on its opponent's time, so the two engines of a
/// game run at once and cannot share cores.
#[test]
fn ponder_requires_the_disjoint_allocation_on_every_command() {
    for arguments in [
        vec!["match", "--games", "2", "a", "b", "--ponder"],
        vec![
            "sprt",
            "a",
            "b",
            "--max-pairs",
            "2",
            "--preset",
            "gainer",
            "--ponder",
        ],
        vec!["calibrate", "a", "b", "--games", "2", "--ponder"],
        vec![
            "tournament",
            "run",
            "--engine",
            "a",
            "--engine",
            "b",
            "--ponder",
        ],
    ] {
        let stderr = refusal(&arguments);
        assert!(
            stderr.contains("--cores-per-engine"),
            "{arguments:?} refusal does not name the disjoint mode: {stderr}"
        );
    }
}

#[test]
fn the_run_record_carries_the_allocation_mode() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let output = cli()
        .arg("match")
        .arg(binary)
        .arg(binary)
        .args([
            "--games",
            "2",
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            "--a-movetime-ms",
            "5",
            "--b-movetime-ms",
            "5",
            "--max-moves",
            "2",
            "--max-engine-faults",
            "99",
            "--dir",
        ])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    let execution = &record["workflow"]["execution"];
    assert_eq!(execution["allocation"]["mode"], "shared");
    assert_eq!(execution["allocation"]["cores_per_game"], 1);
}
