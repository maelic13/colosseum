//! Integration test for the single-game runner using a real engine. Strong vs.
//! deliberately weakened Stockfish, with adjudication, so the game ends quickly.

mod common;

use std::time::Duration;

use colosseum_core::{
    AdjudicationConfig, DrawAdjudication, EngineId, GameId, GameResult, ResignAdjudication,
    TimeControl,
};
use colosseum_engine::runner::{EngineGameSpec, GameSpec, run_game};
use colosseum_uci::SpawnOptions;

fn live_for(game: &GameSpec) -> colosseum_engine::LiveGameHandle {
    colosseum_engine::LiveGameState::new_handle(
        game.game_id,
        game.round,
        (game.white.id, game.white.name.clone()),
        (game.black.id, game.black.name.clone()),
        game.start_fen.clone(),
        game.time_control,
    )
}

fn spec(
    id: EngineId,
    name: &str,
    exe: &std::path::Path,
    options: Vec<(&str, &str)>,
) -> EngineGameSpec {
    EngineGameSpec {
        id,
        name: name.to_string(),
        spawn: SpawnOptions::new(exe),
        options: options
            .into_iter()
            .map(|(k, v)| (k.to_string(), Some(v.to_string())))
            .collect(),
    }
}

#[tokio::test]
async fn stockfish_self_play_one_game() {
    let Some((_guard, exe)) = common::engine_or_skip() else {
        eprintln!("skipping stockfish_self_play_one_game: no engine");
        return;
    };

    let white = spec(
        EngineId::new(),
        "SF-Strong",
        &exe,
        vec![("Threads", "1"), ("Hash", "16")],
    );
    let black = spec(
        EngineId::new(),
        "SF-Weak",
        &exe,
        vec![
            ("Threads", "1"),
            ("Hash", "16"),
            ("UCI_LimitStrength", "true"),
            ("UCI_Elo", "1320"),
        ],
    );

    let game = GameSpec {
        game_id: GameId::new(),
        event: "Colosseum Test".into(),
        site: "Local".into(),
        date: "2026.06.08".into(),
        round: 1,
        white,
        black,
        start_fen: None,
        opening_moves: Vec::new(),
        time_control: TimeControl::PerMove { ms: 30 },
        time_control_label: "movetime/30ms".into(),
        adjudication: AdjudicationConfig {
            max_moves: Some(40),
            draw: Some(DrawAdjudication {
                min_ply: 20,
                move_count: 8,
                score_cp: 10,
            }),
            resign: Some(ResignAdjudication {
                move_count: 4,
                score_cp: 900,
            }),
        },
        ponder: false,
        timeout_tolerance: Duration::from_secs(2),
        handshake_timeout: Duration::from_secs(5),
    };

    let live = live_for(&game);
    let report = run_game(game, live).await;

    assert!(
        report.error.is_none(),
        "unexpected engine error: {:?}",
        report.error
    );
    assert!(!report.san_moves.is_empty(), "no moves were played");
    assert_eq!(report.san_moves.len(), report.uci_moves.len());
    assert_eq!(report.stats.plies as usize, report.san_moves.len());
    assert!(report.pgn.contains("[White \"SF-Strong\"]"));
    assert!(report.pgn.contains("[Black \"SF-Weak\"]"));
    assert!(report.pgn.contains(report.result.pgn()));
    // nps should have been observed for at least the side that moved first.
    assert!(report.stats.white_nps.is_some());

    eprintln!(
        "result={:?} termination={:?} plies={}",
        report.result, report.termination, report.stats.plies
    );
    // The strong side should not lose to a 1320-rated opponent.
    assert_ne!(report.result, GameResult::BlackWin);
}

/// A game with assigned opening moves should pre-play them, then continue, and
/// surface the opening in the PGN movetext.
#[tokio::test]
async fn game_pre_plays_opening_moves() {
    let Some((_guard, exe)) = common::engine_or_skip() else {
        eprintln!("skipping game_pre_plays_opening_moves: no engine");
        return;
    };

    let white = spec(EngineId::new(), "SF-W", &exe, vec![("Threads", "1")]);
    let black = spec(EngineId::new(), "SF-B", &exe, vec![("Threads", "1")]);

    let game = GameSpec {
        game_id: GameId::new(),
        event: "Opening Test".into(),
        site: "Local".into(),
        date: "2026.06.09".into(),
        round: 1,
        white,
        black,
        start_fen: None,
        opening_moves: vec!["e2e4".into(), "e7e5".into(), "g1f3".into()],
        time_control: TimeControl::PerMove { ms: 20 },
        time_control_label: "movetime/20ms".into(),
        adjudication: AdjudicationConfig {
            max_moves: Some(30),
            draw: None,
            resign: Some(ResignAdjudication {
                move_count: 3,
                score_cp: 600,
            }),
        },
        ponder: false,
        timeout_tolerance: Duration::from_secs(2),
        handshake_timeout: Duration::from_secs(5),
    };

    let live = live_for(&game);
    let report = run_game(game, live).await;
    assert!(report.error.is_none(), "engine error: {:?}", report.error);
    // The first three plies are exactly the assigned opening.
    assert!(
        report.uci_moves.len() >= 3,
        "expected at least the opening plies, got {}",
        report.uci_moves.len()
    );
    assert_eq!(&report.uci_moves[0..3], &["e2e4", "e7e5", "g1f3"]);
    assert_eq!(&report.san_moves[0..3], &["e4", "e5", "Nf3"]);
    // The game continued past the opening.
    assert!(report.san_moves.len() > 3, "game did not continue");
}

/// A game starting from an EPD FEN should embed that FEN in the PGN and report
/// no error.
#[tokio::test]
async fn game_starts_from_fen() {
    let Some((_guard, exe)) = common::engine_or_skip() else {
        eprintln!("skipping game_starts_from_fen: no engine");
        return;
    };

    // Position after 1.e4 e5 2.Nf3 (Black to move).
    let fen = "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2";

    let white = spec(EngineId::new(), "SF-W", &exe, vec![("Threads", "1")]);
    let black = spec(EngineId::new(), "SF-B", &exe, vec![("Threads", "1")]);

    let game = GameSpec {
        game_id: GameId::new(),
        event: "FEN Start".into(),
        site: "Local".into(),
        date: "2026.06.09".into(),
        round: 1,
        white,
        black,
        start_fen: Some(fen.to_string()),
        opening_moves: Vec::new(),
        time_control: TimeControl::PerMove { ms: 20 },
        time_control_label: "movetime/20ms".into(),
        adjudication: AdjudicationConfig {
            max_moves: Some(20),
            draw: None,
            resign: Some(ResignAdjudication {
                move_count: 3,
                score_cp: 600,
            }),
        },
        ponder: false,
        timeout_tolerance: Duration::from_secs(2),
        handshake_timeout: Duration::from_secs(5),
    };

    let live = live_for(&game);
    let report = run_game(game, live).await;
    assert!(report.error.is_none(), "engine error: {:?}", report.error);
    assert!(!report.san_moves.is_empty(), "no moves were played");
    // The PGN carries the start FEN tag for faithful replay.
    assert!(report.pgn.contains(&format!("[FEN \"{fen}\"]")));
    assert!(report.pgn.contains("[SetUp \"1\"]"));
}

/// An engine that spawns but never speaks UCI must be reported as a setup
/// crash *and* leave a forensic incident file — the gap that hid Deep
/// Junior's 17 startup crashes. Uses `cmd /c exit` as a process that starts
/// and immediately closes its pipes (handshake sees EOF).
#[cfg(windows)]
#[tokio::test]
async fn setup_failure_writes_incident() {
    let dir = tempfile::tempdir().unwrap();
    colosseum_engine::incidents::set_dir(dir.path().to_path_buf());

    let bogus = |name: &str| EngineGameSpec {
        id: EngineId::new(),
        name: name.to_string(),
        spawn: SpawnOptions {
            path: "cmd".into(),
            args: vec!["/c".into(), "exit".into()],
            working_dir: None,
            env: Default::default(),
        },
        options: vec![("Threads".to_string(), Some("1".to_string()))],
    };

    let game = GameSpec {
        game_id: GameId::new(),
        event: "Setup Fail".into(),
        site: "Local".into(),
        date: "2026.07.04".into(),
        round: 7,
        white: bogus("BrokenWhite"),
        black: bogus("BrokenBlack"),
        start_fen: None,
        opening_moves: Vec::new(),
        time_control: TimeControl::PerMove { ms: 20 },
        time_control_label: "movetime/20ms".into(),
        adjudication: AdjudicationConfig::default(),
        ponder: false,
        timeout_tolerance: Duration::from_secs(2),
        handshake_timeout: Duration::from_secs(3),
    };

    let live = live_for(&game);
    let report = run_game(game, live).await;
    // White takes precedence when both fail → Black wins by White's crash.
    assert_eq!(report.result, GameResult::BlackWin);
    assert!(
        report.error.as_deref().unwrap_or("").contains("incidents/"),
        "error should reference the incident file: {:?}",
        report.error
    );

    let files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .collect();
    assert_eq!(files.len(), 1, "exactly one incident file expected");
    let name = files[0].file_name().into_string().unwrap();
    assert!(name.contains("SetupCrash"), "unexpected name: {name}");
    let text = std::fs::read_to_string(files[0].path()).unwrap();
    assert!(text.contains("EngineCrash (during setup)"));
    assert!(text.contains("BrokenWhite"));
}
