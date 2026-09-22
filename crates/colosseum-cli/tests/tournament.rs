use std::path::Path;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn tournament_command(run: &Path) -> Command {
    let binary = Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let mut command = cli();
    command
        .args(["tournament", "run", "--engine"])
        .arg(binary)
        .arg("--engine")
        .arg(binary)
        .arg("--engine")
        .arg(binary)
        .args([
            "--label",
            "Alpha",
            "--label",
            "Beta",
            "--label",
            "Gamma",
            "--engine-arg=__uci-stub",
            "--games-per-pair",
            "2",
            "--max-moves",
            "1",
            "--placement",
            "off",
            "--concurrency",
            "1",
            "--anchor",
            "1",
            "--seed",
            "7",
            "--dir",
        ])
        .arg(run)
        .arg("--json");
    command
}

/// Games the last checkpoint counted. A checkpoint holds aggregates, not
/// games, so this is a number and not the length of a list.
fn checkpoint_games(run: &Path) -> Option<usize> {
    let bytes = std::fs::read(run.join("checkpoint.json")).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    value["payload"]["games_attempted"]
        .as_u64()
        .and_then(|games| usize::try_from(games).ok())
}

#[test]
fn tournament_dry_run_resolves_common_and_indexed_engine_controls() {
    let output = cli()
        .args([
            "tournament",
            "run",
            "--engine",
            "missing-a",
            "--engine",
            "missing-b",
            "--option",
            "Hash=32",
            "--engine-option",
            "2:EvalFile=beta.nnue",
            "--engine-env",
            "1:RUST_LOG=warn",
            "--engine-arg-at=2:--uci",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let participants = output["resolved_configuration"]["plan"]["participants"]
        .as_array()
        .unwrap();
    assert_eq!(
        participants[0]["participant"]["launch"]["options"]["Hash"]["value"],
        "32"
    );
    assert_eq!(
        participants[1]["participant"]["launch"]["options"]["EvalFile"]["value"],
        "beta.nnue"
    );
    assert_eq!(
        participants[0]["participant"]["launch"]["environment"]["RUST_LOG"],
        "warn"
    );
    assert_eq!(
        participants[1]["participant"]["launch"]["arguments"][0],
        "--uci"
    );
}

/// A tournament whose field is pinned at supplied ratings rather than anchored
/// on one participant's prior.
fn fixed_field_command(run: &Path, fixed: &[&str]) -> Command {
    let binary = Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let mut command = cli();
    command
        .args(["tournament", "run", "--engine"])
        .arg(binary)
        .arg("--engine")
        .arg(binary)
        .arg("--engine")
        .arg(binary)
        .args([
            "--label",
            "Alpha",
            "--label",
            "Beta",
            "--label",
            "Gamma",
            "--engine-arg=__uci-stub",
        ]);
    for entry in fixed {
        command.args(["--fixed", entry]);
    }
    command
        .args([
            "--games-per-pair",
            "1",
            "--max-moves",
            "1",
            "--placement",
            "off",
            "--concurrency",
            "1",
            "--seed",
            "7",
            "--dir",
        ])
        .arg(run);
    command
}

#[test]
fn a_fixed_field_pins_its_members_and_estimates_only_the_newcomer() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("fixed");
    let output = fixed_field_command(&run, &["1:2400", "2:2200.5"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // The text report marks a pinned rating and gives it no error, rather
    // than calling a measurement it never made unavailable.
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("2400.0 [fixed];"), "{text}");
    assert!(text.contains("2200.5 [fixed];"), "{text}");
    assert!(!text.contains("unavailable"), "{text}");

    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run.join("result.json")).unwrap()).unwrap();
    let results = &value["results"];

    // The pinned ratings are retained as run inputs, in participant order.
    assert_eq!(results["fixed_ratings"][0]["rating"], 2400.0);
    assert_eq!(results["fixed_ratings"][1]["rating"], 2200.5);
    assert_eq!(results["fixed_ratings"].as_array().unwrap().len(), 2);

    let standings = results["standings"].as_array().unwrap();
    let row = |name: &str| {
        standings
            .iter()
            .find(|row| row["name"] == name)
            .unwrap_or_else(|| panic!("{name} missing from {standings:?}"))
    };
    for (name, rating) in [("Alpha", 2400.0), ("Beta", 2200.5)] {
        assert_eq!(row(name)["fixed"], true, "{name}");
        assert_eq!(row(name)["rating"], rating, "{name}");
        assert!(
            row(name)["error_95"].is_null(),
            "{name} reported an interval"
        );
    }
    let gamma = row("Gamma");
    assert_eq!(gamma["fixed"], false);
    assert!(gamma["error_95"].is_number(), "{gamma}");
    // Gamma is placed against the fixed field, not left on its own prior.
    assert_ne!(gamma["rating"], gamma["initial_rating"]);

    // The CSV labels pinned rows and leaves them no delta.
    let csv = std::fs::read_to_string(run.join("standings.csv")).unwrap();
    let mut lines = csv.lines();
    assert_eq!(
        lines.next().unwrap(),
        "Rank,Engine,Version,Elo,EloDelta,Fixed,Points,Games,Wins,Draws,Losses,AvgNps"
    );
    for line in lines {
        let fields = line.split(',').collect::<Vec<_>>();
        let pinned = fields[5] == "yes";
        assert_eq!(pinned, matches!(fields[1], "Alpha" | "Beta"), "{line}");
        assert_eq!(fields[4].is_empty(), pinned, "{line}");
    }

    // The fixed ratings are part of the record, not only of the console output.
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    assert_eq!(record["workflow"]["fixed_ratings"][0]["rating"], 2400.0);
}

#[test]
fn a_fixed_rating_is_refused_rather_than_silently_dropped() {
    let root = tempfile::tempdir().unwrap();
    for (arguments, expected) in [
        (vec!["1"], "must use INDEX:RATING"),
        (vec!["0:2400"], "outside the --engine list"),
        (vec!["9:2400"], "outside the --engine list"),
        (vec!["1:not-a-rating"], "invalid rating"),
        (vec!["x:2400"], "invalid engine index"),
        (vec!["1:2400", "1:2500"], "more than once"),
        (vec!["1:2400", "2:2300", "3:2200"], "pins every participant"),
    ] {
        let run = root.path().join(format!("refused-{}", arguments.join("_")));
        let output = fixed_field_command(&run, &arguments).output().unwrap();
        assert_eq!(output.status.code(), Some(2), "{arguments:?} was accepted");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains(expected),
            "{arguments:?} refusal did not say {expected:?}: {stderr}"
        );
    }
}

#[test]
fn live_tournament_writes_joint_ratings_and_both_csv_exports() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("round-robin");
    let output = tournament_command(&run).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["type"], "tournament");
    // The single anchor is the degenerate fixed field: one pinned rating, at
    // the anchor's own prior.
    let fixed = output["report"]["results"]["fixed_ratings"]
        .as_array()
        .unwrap();
    assert_eq!(fixed.len(), 1, "{fixed:?}");
    assert_eq!(fixed[0]["rating"], 1_500.0);
    assert_eq!(
        output["report"]["results"]["anchor"],
        fixed[0]["participant"]
    );
    assert_eq!(output["report"]["status"], "completed");
    assert_eq!(output["report"]["results"]["games_scored"], 6);
    let standings = output["report"]["results"]["standings"].as_array().unwrap();
    assert_eq!(standings.len(), 3);
    assert_eq!(standings[0]["fixed"], true);
    assert_eq!(standings[0]["rating"], 1_500.0);
    // A pinned rating is an input, so it carries no interval; the estimated
    // participants still do.
    assert!(standings[0]["error_95"].is_null());
    assert!(
        standings
            .iter()
            .skip(1)
            .all(|row| row["error_95"].is_number())
    );
    assert!(
        std::fs::read_to_string(run.join("standings.csv"))
            .unwrap()
            .starts_with("Rank,Engine,Version,Elo")
    );
    assert!(
        std::fs::read_to_string(run.join("crosstable.csv"))
            .unwrap()
            .starts_with(",Alpha,Beta,Gamma")
    );
    assert_eq!(checkpoint_games(&run), Some(6));
}
