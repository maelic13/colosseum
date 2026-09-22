//! A stop request that lands on a boundary must not cost the run its outcome.
//!
//! The shape of the defect these guard against is treating "a stop was asked
//! for" as "the run did not finish", which turns a completed match into a
//! cancellation, or lets a repeated staged stop play one more unit each time.
//! When a stop cancels an SPRT is `sprt_runner`'s unit tests, and which status
//! a fixed match reports is `match_runner`'s.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// The documented exit status of a run that stopped cleanly on request.
const CANCELLED: i32 = 6;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn engine() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

/// A small ordinary UCI executable with a `Hash` option, for a tune whose
/// games are synthetic and whose engine is only probed and hashed.
fn probed_engine() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

fn json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn stub_pair(command: &mut Command) {
    command.args([
        "--a-engine-arg=__uci-stub",
        "--b-engine-arg=__uci-stub",
        "--a-movetime-ms",
        "5",
        "--b-movetime-ms",
        "5",
        "--max-moves",
        "2",
        "--seed",
        "11",
        "--max-engine-faults",
        "999",
        "--max-time-losses",
        "999",
    ]);
}

/// An interrupt that arrives while the last game is being joined still leaves
/// every requested game played, and that is a completed match.
#[test]
fn a_match_whose_last_game_was_scored_reports_completed_not_cancelled() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let mut command = cli();
    command
        .args(["--json", "--__stop-after-units", "2"])
        .arg("match")
        .arg(engine())
        .arg(engine());
    stub_pair(&mut command);
    let output = command
        .args(["--games", "2", "--dir"])
        .arg(&run)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "a match that played every game must not exit as cancelled: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value = json(&output);
    assert_eq!(value["report"]["status"], "completed");
    assert_eq!(value["report"]["games_attempted"], 2);
    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["status"], "completed");
}

/// The staged limit counts committed iterations cumulatively, so repeating the
/// command after a stop at N plays nothing instead of one more each time.
#[test]
fn stop_after_iteration_is_cumulative_across_resumes() {
    let root = tempfile::tempdir().unwrap();
    let tune = root.path().join("tune.toml");
    std::fs::write(
        &tune,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    let run = root.path().join("run");

    let invoke = || {
        let mut command = cli();
        command
            .arg("--json")
            .arg("spsa")
            .arg(probed_engine())
            .arg("--tune")
            .arg(&tune)
            .args([
                "--r-end",
                "0.002",
                "--iterations",
                "2",
                "--games-per-iteration",
                "2",
                "--depth",
                "1",
                "--max-moves",
                "2",
                "--seed",
                "7",
                "--stop-after-iteration",
                "1",
                // Instant in-process games: the limit is the driver's.
                "--__synthetic-games",
                "--dir",
            ])
            .arg(&run);
        command.output().unwrap()
    };

    let first = invoke();
    assert_eq!(first.status.code(), Some(CANCELLED));
    let first = json(&first);
    assert_eq!(
        first["report"]["driver"]["completed_iterations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Repeating the identical command must be a no-op, not another iteration.
    let again = invoke();
    assert_eq!(again.status.code(), Some(CANCELLED));
    let again = json(&again);
    assert_eq!(
        again["report"]["driver"]["completed_iterations"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "repeating --stop-after-iteration 1 played a further iteration"
    );
    assert_eq!(
        again["report"]["driver"]["final_centers"],
        first["report"]["driver"]["final_centers"]
    );
}
