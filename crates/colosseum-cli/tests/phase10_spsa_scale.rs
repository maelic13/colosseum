//! A tune's horizon given as a budget in games.
//!
//! The arithmetic, the refusal of a budget the mini-match does not divide and
//! the plan's wall-time estimate are unit-tested in the `spsa` command; this
//! is the command-line contract around them.

use std::process::Command;

use serde_json::Value;

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
