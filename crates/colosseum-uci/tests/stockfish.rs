//! Explicitly opt-in UCI interoperability smoke coverage.
//!
//! Cargo compiles this target only with `real-engine-smoke`; it requires
//! `COLOSSEUM_SMOKE_ENGINE` and is never part of required CI or release evidence.

use std::path::{Path, PathBuf};
use std::time::Duration;

use colosseum_uci::{EngineProcess, GoLimits, SpawnOptions, UciPosition};

/// Resolve an explicitly supplied UCI executable for this opt-in test target.
fn smoke_engine_path() -> PathBuf {
    let path = PathBuf::from(
        std::env::var("COLOSSEUM_SMOKE_ENGINE")
            .expect("UCI smoke test requires COLOSSEUM_SMOKE_ENGINE to name a UCI executable"),
    );
    assert!(
        path.is_file(),
        "COLOSSEUM_SMOKE_ENGINE must name an existing executable file: {}",
        path.display()
    );
    path
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
    let src = smoke_engine_path();
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
            |_| {},
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
            |_| {},
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
            |_| {},
        )
        .await
        .expect("search after e4");
    assert_ne!(reply.best_move, "(none)");

    engine.quit(Duration::from_secs(2)).await.expect("quit");
}
