//! A game's wall time is accounted for by phase.
//!
//! A fixture engine is told to spend a known delay starting up (before its
//! `uciok`), searching (before each `bestmove`) and shutting down (after
//! `quit`). Each delay must land in its own phase of the journal's per-game
//! phases, play must be its charged time plus the uncharged part, and
//! `stats` must report the distribution of every phase.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const UCIOK_AFTER_MS: u64 = 150;
const EXIT_AFTER_QUIT_MS: u64 = 120;
const SEARCH_MS: u64 = 20;

fn fixture() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
}

fn ms(value: &Value) -> f64 {
    value.as_u64().expect("a phase in nanoseconds") as f64 / 1e6
}

#[test]
fn each_injected_delay_lands_in_its_own_game_phase() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let output = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .args(["match", "--games", "2"])
        .arg(fixture())
        .arg(fixture())
        .args([
            "--a-engine-arg=--legal-sequence",
            "--b-engine-arg=--legal-sequence",
        ])
        .arg(format!("--a-engine-arg=--uciok-after-ms={UCIOK_AFTER_MS}"))
        .arg(format!(
            "--a-engine-arg=--exit-after-quit-ms={EXIT_AFTER_QUIT_MS}"
        ))
        .arg(format!("--a-engine-arg=--sleep-ms={SEARCH_MS}"))
        .arg(format!("--b-engine-arg=--sleep-ms={SEARCH_MS}"))
        .args([
            "--max-moves",
            "6",
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
    assert_eq!(games.len(), 2);
    for game in &games {
        let phases = &game["clock"]["phases"];
        assert!(phases.is_object(), "{game}");
        // Both engines start together; the slow one sets start-up.
        let startup = ms(&phases["startup_ns"]);
        assert!(
            startup >= (UCIOK_AFTER_MS - 2) as f64 && startup < (UCIOK_AFTER_MS + 1_000) as f64,
            "start-up {startup} ms against {UCIOK_AFTER_MS} ms injected"
        );
        // Teardown waits for both processes to have exited.
        let teardown = ms(&phases["teardown_ns"]);
        assert!(
            teardown >= (EXIT_AFTER_QUIT_MS - 2) as f64
                && teardown < (EXIT_AFTER_QUIT_MS + 1_000) as f64,
            "teardown {teardown} ms against {EXIT_AFTER_QUIT_MS} ms injected"
        );
        // Play is charged time and the uncharged part, exactly; every search
        // slept, so charged time is at least the moves times the sleep.
        let play = phases["play_ns"].as_u64().unwrap();
        let charged = phases["charged_ns"].as_u64().unwrap();
        let uncharged = phases["uncharged_play_ns"].as_u64().unwrap();
        assert_eq!(play, charged + uncharged, "{phases}");
        assert!(
            ms(&phases["charged_ns"]) >= (6 * (SEARCH_MS - 2)) as f64,
            "{phases}"
        );
        // The three parts of uncharged play are parts of it: without
        // pondering nothing else happens between searches.
        let parts = [
            "between_searches_ns",
            "position_write_ns",
            "after_bestmove_ns",
        ]
        .iter()
        .map(|part| phases[part].as_u64().unwrap())
        .sum::<u64>();
        assert!(parts <= uncharged, "{phases}");
        // Neither start-up nor teardown leaked into play's uncharged part.
        assert!(
            ms(&phases["uncharged_play_ns"]) < SEARCH_MS as f64 * 6.0,
            "{phases}"
        );
    }

    let stats = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .arg("stats")
        .arg(&run)
        .arg("--json")
        .output()
        .unwrap();
    assert!(stats.status.success());
    let report = &serde_json::from_slice::<Value>(&stats.stdout).unwrap()["report"];
    let phases = &report["game_phases"];
    assert_eq!(phases["games"], 2, "{report}");
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
    let startup = &phases["phases"][0];
    assert!(startup["p50_ms"].as_f64().unwrap() >= (UCIOK_AFTER_MS - 2) as f64);
    let text = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .arg("stats")
        .arg(&run)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&text.stdout);
    assert!(text.contains("game phases over 2 games"), "{text}");
}
