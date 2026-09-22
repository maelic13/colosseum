//! A game's wall time is accounted for by phase.
//!
//! A real game journals its per-game phases, play is its charged time plus
//! the uncharged part, and `stats` reports the distribution of every phase.
//! Which stretch of a game lands in which phase is unit-tested on the phase
//! clock in `colosseum_engine::runner`, with synthetic instants.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn fixture() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

#[test]
fn a_game_journals_its_phases_and_stats_reports_every_phase() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let output = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .args(["match", "--games", "1"])
        .arg(fixture())
        .arg(fixture())
        .args([
            "--a-engine-arg=--legal-sequence",
            "--b-engine-arg=--legal-sequence",
            "--max-moves",
            "4",
            "--seed",
            "7",
            "--placement",
            "off",
            // Teardown here is the fresh processes' exit, which kept engines
            // never reach between games.
            "--engine-processes",
            "per-game",
            "--json",
            "--dir",
        ])
        .arg(&run)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let journal = std::fs::read_to_string(run.join("games.jsonl")).unwrap();
    let games = journal
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap()["game"].clone())
        .collect::<Vec<_>>();
    assert_eq!(games.len(), 1);
    let phases = &games[0]["clock"]["phases"];
    assert!(phases.is_object(), "{}", games[0]);
    let phase = |name: &str| {
        phases[name]
            .as_u64()
            .unwrap_or_else(|| panic!("{name} missing from {phases}"))
    };
    for name in ["startup_ns", "teardown_ns"] {
        phase(name);
    }
    // Play is charged time and the uncharged part, exactly, and the three
    // parts of uncharged play are parts of it.
    let uncharged = phase("uncharged_play_ns");
    assert_eq!(
        phase("play_ns"),
        phase("charged_ns") + uncharged,
        "{phases}"
    );
    let parts =
        phase("between_searches_ns") + phase("position_write_ns") + phase("after_bestmove_ns");
    assert!(parts <= uncharged, "{phases}");

    let stats = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .arg("stats")
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(stats.status.success());
    let report = &serde_json::from_slice::<Value>(&stats.stdout).unwrap()["report"];
    let phases = &report["game_phases"];
    assert_eq!(phases["games"], 1, "{report}");
    let names = phases["phases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|phase| phase["phase"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            "startup",
            "play",
            "charged",
            "uncharged-play",
            "between-searches",
            "position-write",
            "after-bestmove",
            "teardown",
            "outside-runner"
        ]
    );
    let text = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .arg("stats")
        .arg(&run)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("game phases over 1 game"), "{text}");
}
