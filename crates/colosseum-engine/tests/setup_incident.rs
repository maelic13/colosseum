//! An engine that spawns but never speaks UCI must be reported as a setup
//! crash *and* leave a forensic incident file — the gap that hid Deep Junior's
//! 17 startup crashes.
//!
//! The incident directory is a process-wide, set-once setting, so this test
//! sets it for its own test binary and finds its report by the name the error
//! message gives.

use std::time::Duration;

use colosseum_core::{AdjudicationConfig, EngineId, GameId, GameResult, TimeControl};
use colosseum_engine::runner::{EngineGameSpec, GameSpec, run_game};
use colosseum_uci::SpawnOptions;

/// A repository-owned process that starts and exits at once without writing
/// anything: the affinity fixture given no gate argument. The handshake sees
/// its pipes close.
fn silent_process() -> SpawnOptions {
    SpawnOptions::new(env!("CARGO_BIN_EXE_colosseum-affinity-fixture"))
}

#[tokio::test]
async fn setup_failure_writes_incident() {
    let dir = tempfile::tempdir().unwrap();
    colosseum_engine::incidents::set_dir(dir.path().to_path_buf());

    let bogus = |name: &str| EngineGameSpec {
        id: EngineId::from_uuid(uuid::Uuid::new_v4()),
        name: name.to_string(),
        spawn: silent_process(),
        options: vec![("Threads".to_string(), Some("1".to_string()))],
        allocated_cpus: colosseum_application::CpuAllocation::Unrestricted,
    };

    let game = GameSpec {
        game_id: GameId::from_uuid(uuid::Uuid::new_v4()),
        event: "Setup Fail".into(),
        site: "Local".into(),
        date: "2026.07.04".into(),
        round: 7,
        white: bogus("BrokenWhite"),
        black: bogus("BrokenBlack"),
        start_fen: None,
        opening_moves: Vec::new(),
        white_time_control: TimeControl::PerMove { ms: 20 },
        black_time_control: TimeControl::PerMove { ms: 20 },
        time_control_label: "movetime/20ms".into(),
        adjudication: AdjudicationConfig::default(),
        ponder: false,
        white_time_margin: Duration::from_secs(2),
        black_time_margin: Duration::from_secs(2),
        // Generous: the pipes close at once, so this only bounds a hang.
        handshake_timeout: Duration::from_secs(60),
        identity: None,
        slot: None,
    };

    let live = colosseum_engine::LiveGameState::new_handle(
        game.game_id,
        game.round,
        (game.white.id, game.white.name.clone()),
        (game.black.id, game.black.name.clone()),
        game.start_fen.clone(),
        game.white_time_control,
    );
    let report = run_game(game, live).await;
    // White takes precedence when both fail → Black wins by White's crash.
    assert_eq!(report.result, GameResult::BlackWin);
    let error = report.error.expect("a setup crash is reported");
    let (_, file) = error
        .split_once("logs/incidents/")
        .unwrap_or_else(|| panic!("error should reference the incident file: {error}"));

    let name = file.trim();
    assert!(name.contains("SetupCrash"), "unexpected name: {name}");
    let text = std::fs::read_to_string(dir.path().join(name)).unwrap();
    assert!(text.contains("EngineCrash (during setup)"), "{text}");
    assert!(text.contains("BrokenWhite"), "{text}");
}
