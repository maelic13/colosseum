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
        timeout_tolerance: Duration::from_secs(2),
        handshake_timeout: Duration::from_secs(5),
    };

    let report = run_game(game).await;

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
