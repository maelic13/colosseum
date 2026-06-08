//! Integration tests against a real engine. The engine is copied into a temp dir
//! first so the original is never touched or locked. If no engine is available the
//! test skips (so CI on machines without the engine stays green). Point the tests at
//! any UCI engine with `COLOSSEUM_TEST_ENGINE=/path/to/engine`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use colosseum_uci::{EngineProcess, GoLimits, SpawnOptions, UciPosition};

/// Locate a UCI engine: `COLOSSEUM_TEST_ENGINE` env, else the known dev path.
fn locate_engine() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("COLOSSEUM_TEST_ENGINE") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }
    let default = PathBuf::from(r"D:\chess\engines\stockfish.exe");
    default.exists().then_some(default)
}

/// Copy the engine into a fresh temp dir; returns the guard (kept alive) and the path.
fn copy_to_temp(src: &Path) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let dest = dir.path().join(src.file_name().expect("engine file name"));
    std::fs::copy(src, &dest).expect("copy engine");
    (dir, dest)
}

#[tokio::test]
async fn stockfish_full_cycle() {
    let Some(src) = locate_engine() else {
        eprintln!("skipping stockfish_full_cycle: no engine (set COLOSSEUM_TEST_ENGINE)");
        return;
    };
    let (_guard, exe) = copy_to_temp(&src);

    let mut engine = EngineProcess::spawn(SpawnOptions::new(&exe))
        .await
        .expect("spawn engine");

    // Handshake: name + options must be discovered.
    engine
        .handshake(Duration::from_secs(5))
        .await
        .expect("handshake");
    let name = engine.name().unwrap_or_default();
    assert!(!name.is_empty(), "engine reported no id name");

    let option_names: Vec<String> = engine
        .options()
        .iter()
        .map(|o| o.name().to_string())
        .collect();
    assert!(
        option_names.iter().any(|n| n == "Threads"),
        "no Threads option"
    );
    assert!(option_names.iter().any(|n| n == "Hash"), "no Hash option");

    // Configure and ready.
    engine.set_option("Threads", Some("1")).await.unwrap();
    engine.set_option("Hash", Some("16")).await.unwrap();
    engine.is_ready(Duration::from_secs(5)).await.unwrap();
    engine.new_game().await.unwrap();
    engine.is_ready(Duration::from_secs(5)).await.unwrap();

    let startpos = UciPosition::StartPos { moves: vec![] };

    // A normal 100ms search yields a legal-looking first move and a score.
    let out = engine
        .search(
            &startpos,
            &GoLimits::MoveTime(Duration::from_millis(100)),
            Duration::from_secs(3),
        )
        .await
        .expect("search 100ms");
    assert_ne!(out.best_move, "(none)");
    assert!(
        out.best_move.len() == 4 || out.best_move.len() == 5,
        "unexpected bestmove {:?}",
        out.best_move
    );
    assert!(out.score.is_some(), "no score reported");

    // The critical fast path: 10ms/move must work and stay within the deadline.
    let fast = engine
        .search(
            &startpos,
            &GoLimits::MoveTime(Duration::from_millis(10)),
            Duration::from_secs(1),
        )
        .await
        .expect("search 10ms");
    assert!(!fast.best_move.is_empty());

    // Continuing from a move list also works.
    let after_e4 = UciPosition::StartPos {
        moves: vec!["e2e4".into()],
    };
    let reply = engine
        .search(
            &after_e4,
            &GoLimits::MoveTime(Duration::from_millis(50)),
            Duration::from_secs(2),
        )
        .await
        .expect("search after e4");
    assert_ne!(reply.best_move, "(none)");

    engine.quit(Duration::from_secs(2)).await.expect("quit");
}
