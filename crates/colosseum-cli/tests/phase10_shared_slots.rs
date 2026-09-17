//! Shared game slots: both engines of a game on one core set by default.
//!
//! Without pondering only one engine of a game searches at any moment and the
//! other waits on a pipe, so pinning them separately leaves half the pool
//! idle. These cases pin the contract that made a one-thread gate at
//! concurrency 14 fit on a 16-core host instead of being refused.

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

    // With the disjoint mode named, the same invocation resolves.
    let value = dry_run(&[
        "match",
        "--games",
        "2",
        "a",
        "b",
        "--ponder",
        "--cores-per-engine",
        "1",
    ]);
    assert_eq!(value["resolved_configuration"]["ponder"], true);
    assert_eq!(
        value["resolved_configuration"]["execution"]["allocation"]["mode"],
        "per-engine"
    );
}

/// The pool arithmetic each mode uses, named in the refusal when it does not
/// fit. How many physical cores a logical-CPU list covers depends on the host,
/// so the exact slot division is asserted against recorded topologies in
/// `colosseum-engine` rather than against whichever machine runs this.
#[test]
fn a_pool_too_small_refuses_and_names_the_arithmetic_it_applied() {
    let shared = refusal(&[
        "match",
        "--games",
        "2",
        "a",
        "b",
        "--placement",
        "0-1",
        "--concurrency",
        "100000",
    ]);
    assert!(
        shared.contains("game-slots × cores-per-game"),
        "shared refusal must name its arithmetic: {shared}"
    );

    let disjoint = refusal(&[
        "match",
        "--games",
        "2",
        "a",
        "b",
        "--placement",
        "0-1",
        "--concurrency",
        "100000",
        "--cores-per-engine",
        "1",
    ]);
    assert!(
        disjoint.contains("game-slots × 2 × cores-per-engine"),
        "disjoint refusal must name its arithmetic: {disjoint}"
    );
}

#[test]
fn the_run_record_carries_the_mode_and_every_allocation() {
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
            "--placement",
            "0-1",
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
    let slot = &execution["slots"][0];
    assert_eq!(
        slot["engine_a"]["allocation"],
        slot["engine_b"]["allocation"]
    );
    assert_eq!(slot["engine_a"]["physical_core_count"], 1);
}
