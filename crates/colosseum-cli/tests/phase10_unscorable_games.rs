//! Regression tests for the games a run records but cannot score, and for the
//! versioned refusals that had stopped naming a version.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn engine() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_colosseum-cli"))
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

fn replay(target: &Path) -> Value {
    let output = cli()
        .arg("stats")
        .arg(target)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stats {} failed: {}",
        target.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    json(&output)["report"].clone()
}

fn tune(root: &Path) -> PathBuf {
    let path = root.join("tune.toml");
    std::fs::write(
        &path,
        "[[parameters]]\nname = \"Hash\"\ninitial = 16\nmin = 1\nmax = 1024\nc_end = 1.0\n",
    )
    .unwrap();
    path
}

/// A run that abandoned a game reports the same sample from either evidence.
///
/// The gauntlet's seed meets the second engine first and plays its encounter
/// out, then meets an executable that does not exist. That game is written —
/// it is the evidence for the abort — with a result it only has because the
/// PGN shape needs one. The checkpoint has always left it out; now the PGN
/// says so too.
#[test]
fn an_aborted_game_is_outside_the_sample_in_the_checkpoint_and_in_the_pgn() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let played = cli()
        .args(["tournament", "run", "--format", "gauntlet", "--seeds", "1"])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args(["--engine".as_ref(), engine().as_os_str()])
        .args(["--engine".as_ref(), root.path().join("absent").as_os_str()])
        .args([
            "--games-per-pair",
            "2",
            "--engine-arg=__uci-stub",
            "--movetime-ms",
            "10",
            "--max-moves",
            "2",
            "--max-engine-faults",
            "999",
            "--dir",
        ])
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    // An infrastructure fault is exit 3, and the report is still a report.
    assert_eq!(played.status.code(), Some(3));
    let report = &json(&played)["report"];
    assert_eq!(report["status"], "infrastructure-error");
    let games = report["games"].as_array().unwrap();
    assert_eq!(games.len(), 3, "{report}");
    assert_eq!(games[2]["scorable"], false, "{report}");

    let pgn_text = std::fs::read_to_string(run.join("games.pgn")).unwrap();
    assert_eq!(
        pgn_text.matches("[ColosseumSample \"unscorable\"]").count(),
        1,
        "{pgn_text}"
    );
    // The two games that counted are left exactly as they were written.
    assert_eq!(pgn_text.matches("[ColosseumSample ").count(), 1);

    let directory = replay(&run);
    let pgn = replay(&run.join("games.pgn"));
    assert_eq!(directory["authority"], "structured-run-store");
    assert_eq!(pgn["authority"], "pgn-export");
    for field in [
        "games",
        "complete_pairs",
        "unpaired_games",
        "pentanomial",
        "wins",
        "draws",
        "losses",
        "excluded_games",
        "excluded_by_sample",
    ] {
        assert_eq!(
            directory[field], pgn[field],
            "{field} differs between the checkpoint and its own PGN"
        );
    }
    // Two games played, one pair, and the third game accounted for rather
    // than quietly dropped.
    assert_eq!(directory["games"], 2);
    assert_eq!(directory["complete_pairs"], 1);
    assert_eq!(directory["unpaired_games"], 0);
    assert_eq!(directory["excluded_games"], 1);
    assert_eq!(
        directory["excluded_by_sample"],
        serde_json::json!({"unscorable": 1})
    );
    for (name, report) in [("run directory", &directory), ("PGN", &pgn)] {
        assert!(
            report["warnings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|warning| warning
                    .as_str()
                    .unwrap_or_default()
                    .contains("1 unscorable game was excluded from the official sample")),
            "{name} never said what it left out: {report}"
        );
    }
}

/// A stale schedule artifact is refused by its version.
///
/// The artifact denies unknown fields, so deserializing before asking for the
/// version reported the field that a version bump had renamed and never named
/// the version. The refusal has to come first.
#[test]
fn an_old_spsa_schedule_is_refused_by_its_version_and_not_by_a_field() {
    let root = tempfile::tempdir().unwrap();
    let tune = tune(root.path());
    let run = root.path().join("run");
    let command = |stop: Option<&str>| {
        let mut command = cli();
        command
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
                "--__synthetic-games",
                "--dir",
            ])
            .arg(&run)
            .arg("--json");
        if let Some(stop) = stop {
            command.args(["--stop-after-iteration", stop]);
        }
        command.output().unwrap()
    };

    // Stop the run with iterations left, so resuming reads the stored
    // schedule instead of refusing a finished run.
    let stopped = command(Some("1"));
    assert_eq!(stopped.status.code(), Some(6));

    let path = run.join("spsa-schedule.json");
    let current: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(current["schema_version"], 2);
    // Exactly what version 1 wrote: the same numbers under the name it used.
    let mut stale = current.clone();
    let object = stale.as_object_mut().unwrap();
    let rng_version = object.remove("rng_version").unwrap();
    object.insert("schema_version".into(), 1.into());
    object.insert("stats_version".into(), rng_version);
    std::fs::write(&path, serde_json::to_vec(&stale).unwrap()).unwrap();

    let resumed = command(None);
    assert_eq!(resumed.status.code(), Some(3));
    let message = String::from_utf8_lossy(&resumed.stderr);
    assert!(
        message.contains("unsupported SPSA schedule artifact schema version 1"),
        "{message}"
    );
    assert!(
        !message.contains("unknown field"),
        "the version, not the field it renamed: {message}"
    );
}

/// The same rule for a tune result offered to `sprt --apply`.
#[test]
fn an_old_spsa_result_is_refused_by_its_version_and_not_by_a_field() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("old-result.json");
    // Version 2 of the result carried `stats_version`, which this build
    // denies. Reaching the refusal must not depend on any other field.
    std::fs::write(
        &path,
        r#"{"tuned_result":{"schema_version":2,"stats_version":1}}"#,
    )
    .unwrap();

    let output = cli()
        .arg("sprt")
        .args(["--apply".as_ref(), path.as_os_str()])
        .args(["--max-pairs", "4", "--preset", "gainer", "--json"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(
        message.contains("unsupported SPSA tune-result schema version 2"),
        "{message}"
    );
    assert!(
        !message.contains("unknown field"),
        "the version, not the field it renamed: {message}"
    );
}

/// The plan report's own schema version moved with the field it renamed.
#[test]
fn the_spsa_plan_report_names_the_rng_version_under_its_own_schema_version() {
    let root = tempfile::tempdir().unwrap();
    let tune = tune(root.path());
    let output = cli()
        .args(["spsa", "plan", "--tune"])
        .arg(&tune)
        .args([
            "--r-end",
            "0.002",
            "--iterations",
            "10",
            "--games-per-iteration",
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
    let report = &json(&output)["report"];
    assert_eq!(report["schema_version"], 2, "{report}");
    assert_eq!(report["schedule_schema_version"], 2, "{report}");
    assert_eq!(report["rng_version"], 1, "{report}");
    assert!(report.get("stats_version").is_none(), "{report}");
}
