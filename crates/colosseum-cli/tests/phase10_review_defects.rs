//! Regression tests for the Phase 10 review defects that need a real run.
//!
//! Most were about an interrupt arriving at an awkward moment. The common
//! shape of the bug was treating "a stop was asked for" as "the run did not
//! finish", which turns a completed match or a cap-reached SPRT into a
//! cancellation and loses the verdict it had already earned. The SPRT cases
//! are `sprt_runner`'s unit tests, and a cancelled suite's exit code is
//! asserted with its clean stop in `phase10_graceful_stop`.

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
            .arg(engine())
            .arg("--engine-arg=__uci-stub")
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

/// `--anchor` pins a participant at its own rating and `--fixed` at a supplied
/// one. Naming the same participant through both is a contradiction, refused
/// before a run directory exists.
#[test]
fn anchor_and_fixed_on_one_participant_are_refused_before_any_run_directory() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("never-created");

    for extra in [vec!["--dry-run"], vec![]] {
        let output = cli()
            .args(["tournament", "run", "--engine", "a", "--engine", "b"])
            .args(["--engine", "c", "--anchor", "2", "--fixed", "2:2400"])
            .args(&extra)
            .args(["--json", "--dir"])
            .arg(&run)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{extra:?} was accepted");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("--anchor") && stderr.contains("--fixed"),
            "{extra:?} refusal must name both flags: {stderr}"
        );
        assert!(
            !run.exists(),
            "{extra:?} created a run directory for a configuration it refused"
        );
    }

    // Pinning different participants through the two flags stays valid.
    let output = cli()
        .args(["tournament", "run", "--engine", "a", "--engine", "b"])
        .args(["--engine", "c", "--anchor", "1", "--fixed", "2:2400"])
        .args(["--dry-run", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
