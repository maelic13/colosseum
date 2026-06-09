//! Integration tests for the tournament scheduler, driven by real engines (copied to
//! a temp dir). Cover a full round-robin, Stop→drain→resume, Force-Stop→discard, and
//! the engine-failure path (a bogus executable). Skips when no engine is available.

mod common;

use std::path::Path;
use std::time::{Duration, Instant};

use colosseum_core::{
    AdjudicationConfig, EngineConfig, Format, TimeControl, TournamentConfig, TournamentEvent,
    UciOptionValue,
};
use colosseum_engine::scheduler::{TournamentStatus, create_tournament, resume_tournament};
use colosseum_engine::store::{self, Store};

fn engine_cfg(name: &str, exe: &Path, opts: &[(&str, &str)]) -> EngineConfig {
    let mut cfg = EngineConfig::new(exe.to_path_buf());
    cfg.meta.name = name.to_string();
    for (key, value) in opts {
        cfg.options.insert(
            (*key).to_string(),
            UciOptionValue::Str((*value).to_string()),
        );
    }
    cfg
}

fn fast_config(concurrency: usize, max_moves: u32, movetime_ms: u64) -> TournamentConfig {
    TournamentConfig {
        format: Format::RoundRobin { cycles: 1 },
        games_per_pair: 2,
        time_control: TimeControl::PerMove { ms: movetime_ms },
        concurrency,
        adjudication: AdjudicationConfig {
            max_moves: Some(max_moves),
            ..Default::default()
        },
        ..Default::default()
    }
}

async fn wait_status(
    snapshot: &std::sync::Arc<std::sync::Mutex<colosseum_engine::TournamentSnapshot>>,
    want: TournamentStatus,
    timeout: Duration,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if snapshot.lock().unwrap().status == want {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn temp_db() -> (tempfile::TempDir, Store, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("colosseum.sqlite");
    let store = Store::open(&path).unwrap();
    (dir, store, path)
}

#[tokio::test]
async fn full_round_robin_completes() {
    let Some((_guard, exe)) = common::engine_or_skip() else {
        eprintln!("skipping full_round_robin_completes: no engine");
        return;
    };
    let (_dir, store, db_path) = temp_db();

    let engines = vec![
        engine_cfg("SF-A", &exe, &[("Hash", "16")]),
        engine_cfg("SF-B", &exe, &[("Hash", "16"), ("Skill Level", "8")]),
        engine_cfg("SF-C", &exe, &[("Hash", "16"), ("Skill Level", "3")]),
    ];
    let ids: Vec<_> = engines.iter().map(|e| e.id).collect();

    let (events_tx, events_rx) = crossbeam_channel::unbounded();
    let (tournament, driver) =
        create_tournament("RR", fast_config(2, 8, 10), engines, store, events_tx).unwrap();
    let handle = tokio::spawn(driver);

    tournament.go();
    let snapshot = tournament.snapshot_handle();
    assert!(
        wait_status(
            &snapshot,
            TournamentStatus::Finished,
            Duration::from_secs(120)
        )
        .await,
        "tournament did not finish in time"
    );

    // 3 engines, double round robin => C(3,2)=3 pairs * 2 games = 6 games.
    let snap = snapshot.lock().unwrap().clone();
    assert_eq!(snap.games_total, 6);
    assert_eq!(snap.games_finished, 6);
    // Each engine played 4 games (2 opponents * 2 games).
    for id in &ids {
        assert_eq!(snap.standings.standing(*id).games(), 4, "engine {id}");
        assert!(snap.elo.contains_key(id));
    }

    // Persisted: every game finished.
    let reopened = Store::open(&db_path).unwrap();
    let tournaments = reopened.list_tournaments().unwrap();
    assert_eq!(tournaments.len(), 1);
    assert_eq!(tournaments[0].status, store::STATUS_FINISHED);
    let games = reopened.list_games(tournaments[0].id).unwrap();
    assert_eq!(games.len(), 6);
    assert!(games.iter().all(|g| g.status == store::GAME_FINISHED));

    // Exactly 6 GameFinished events were emitted.
    let finished_events = events_rx
        .try_iter()
        .filter(|e| matches!(e, TournamentEvent::GameFinished { .. }))
        .count();
    assert_eq!(finished_events, 6);

    handle.abort();
}

#[tokio::test]
async fn stop_drains_then_resume_completes() {
    let Some((_guard, exe)) = common::engine_or_skip() else {
        eprintln!("skipping stop_drains_then_resume_completes: no engine");
        return;
    };
    let (_dir, store, _db_path) = temp_db();

    let engines = vec![
        engine_cfg("A", &exe, &[("Hash", "16")]),
        engine_cfg("B", &exe, &[("Hash", "16")]),
        engine_cfg("C", &exe, &[("Hash", "16")]),
    ];
    let (events_tx, _events_rx) = crossbeam_channel::unbounded();
    // Concurrency 1 + longer games so Stop reliably catches the tournament mid-way.
    let (tournament, driver) =
        create_tournament("StopGo", fast_config(1, 24, 20), engines, store, events_tx).unwrap();
    let handle = tokio::spawn(driver);

    tournament.go();
    tokio::time::sleep(Duration::from_millis(150)).await;
    tournament.stop();

    let snapshot = tournament.snapshot_handle();
    assert!(
        wait_status(
            &snapshot,
            TournamentStatus::Stopped,
            Duration::from_secs(30)
        )
        .await,
        "did not reach Stopped"
    );
    let after_stop = snapshot.lock().unwrap().games_finished;
    assert!(
        after_stop < 6,
        "stop should leave games unplayed, got {after_stop}"
    );

    // Resume and finish.
    tournament.go();
    assert!(
        wait_status(
            &snapshot,
            TournamentStatus::Finished,
            Duration::from_secs(120)
        )
        .await,
        "did not finish after resume"
    );
    assert_eq!(snapshot.lock().unwrap().games_finished, 6);

    handle.abort();
}

#[tokio::test]
async fn force_stop_discards_in_flight() {
    let Some((_guard, exe)) = common::engine_or_skip() else {
        eprintln!("skipping force_stop_discards_in_flight: no engine");
        return;
    };
    let (_dir, store, db_path) = temp_db();

    let engines = vec![
        engine_cfg("A", &exe, &[("Hash", "16")]),
        engine_cfg("B", &exe, &[("Hash", "16")]),
        engine_cfg("C", &exe, &[("Hash", "16")]),
    ];
    let (events_tx, _events_rx) = crossbeam_channel::unbounded();
    // Long games so they are still running when we force-stop.
    let (tournament, driver) =
        create_tournament("Force", fast_config(2, 200, 50), engines, store, events_tx).unwrap();
    let handle = tokio::spawn(driver);

    tournament.go();
    tokio::time::sleep(Duration::from_millis(200)).await;
    tournament.force_stop();

    let snapshot = tournament.snapshot_handle();
    assert!(
        wait_status(
            &snapshot,
            TournamentStatus::Stopped,
            Duration::from_secs(30)
        )
        .await,
        "did not reach Stopped after force-stop"
    );
    assert!(snapshot.lock().unwrap().games_finished < 6);

    // At least the in-flight games were discarded (not finished).
    let reopened = Store::open(&db_path).unwrap();
    let tid = reopened.list_tournaments().unwrap()[0].id;
    let games = reopened.list_games(tid).unwrap();
    let discarded = games
        .iter()
        .filter(|g| g.status == store::GAME_DISCARDED)
        .count();
    assert!(discarded >= 1, "expected at least one discarded game");

    handle.abort();
}

#[tokio::test]
async fn failed_engine_loses_with_error() {
    let Some((_guard, exe)) = common::engine_or_skip() else {
        eprintln!("skipping failed_engine_loses_with_error: no engine");
        return;
    };
    let (_dir, store, _db_path) = temp_db();

    let good = engine_cfg("Good", &exe, &[("Hash", "16")]);
    let good_id = good.id;
    let bogus = engine_cfg("Bogus", Path::new("definitely-not-a-real-engine.exe"), &[]);
    let (events_tx, events_rx) = crossbeam_channel::unbounded();

    let (tournament, driver) = create_tournament(
        "Crash",
        fast_config(2, 8, 10),
        vec![good, bogus],
        store,
        events_tx,
    )
    .unwrap();
    let handle = tokio::spawn(driver);

    tournament.go();
    let snapshot = tournament.snapshot_handle();
    assert!(
        wait_status(
            &snapshot,
            TournamentStatus::Finished,
            Duration::from_secs(30)
        )
        .await,
        "crash tournament did not finish"
    );

    let snap = snapshot.lock().unwrap().clone();
    // 2 engines, double round robin => 2 games; the good engine wins both.
    assert_eq!(snap.games_finished, 2);
    assert_eq!(snap.standings.standing(good_id).wins, 2);
    assert!(
        !snap.recent_errors.is_empty(),
        "expected engine errors recorded"
    );

    let error_events = events_rx
        .try_iter()
        .filter(|e| matches!(e, TournamentEvent::EngineError { .. }))
        .count();
    assert!(error_events >= 1);

    handle.abort();
}

/// Simulates an app restart: starts a tournament, stops it mid-way, drops the
/// handle (simulating an app exit), reopens the database, calls
/// `resume_tournament`, and verifies the full schedule completes.
#[tokio::test]
async fn resume_across_restart() {
    let Some((_guard, exe)) = common::engine_or_skip() else {
        eprintln!("skipping resume_across_restart: no engine");
        return;
    };

    // ── Phase 1: start tournament, let at least 1 game finish, then stop ──
    let (_dir, store1, db_path) = temp_db();
    let engines = vec![
        engine_cfg("A", &exe, &[("Hash", "16")]),
        engine_cfg("B", &exe, &[("Hash", "16")]),
        engine_cfg("C", &exe, &[("Hash", "16")]),
    ];
    let (events_tx, _events_rx) = crossbeam_channel::unbounded();
    let (t1, driver1) =
        create_tournament("Restart", fast_config(1, 8, 10), engines, store1, events_tx).unwrap();
    let tid = t1.id;
    let handle1 = tokio::spawn(driver1);
    t1.go();

    // Wait until at least 1 game finishes before stopping.
    let snap1 = t1.snapshot_handle();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if snap1.lock().unwrap().games_finished >= 1 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no game finished before deadline"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    t1.stop();
    assert!(
        wait_status(&snap1, TournamentStatus::Stopped, Duration::from_secs(30)).await,
        "did not reach Stopped"
    );
    let finished_before = snap1.lock().unwrap().games_finished;
    assert!(
        finished_before >= 1,
        "expected ≥1 game finished before restart"
    );

    // Simulate app exit: drop the handle so the driver's command channel closes
    // (driver receives None and exits the loop cleanly).
    drop(t1);
    let _ = tokio::time::timeout(Duration::from_secs(5), handle1).await;

    // ── Phase 2: reopen database and resume ──
    let store2 = Store::open(&db_path).unwrap();
    let row = store2
        .load_tournament(tid)
        .unwrap()
        .expect("tournament should still be in the database");
    let (events_tx2, _events_rx2) = crossbeam_channel::unbounded();
    let (t2, driver2) = resume_tournament(row, store2, events_tx2).unwrap();
    let handle2 = tokio::spawn(driver2);

    // The resumed snapshot should already reflect the pre-restart finished games.
    let snap2 = t2.snapshot_handle();
    assert_eq!(
        snap2.lock().unwrap().games_finished,
        finished_before,
        "resumed snapshot should start at the pre-restart finished count"
    );
    assert_eq!(
        snap2.lock().unwrap().games_total,
        6,
        "total should be the full tournament schedule"
    );
    assert_eq!(
        snap2.lock().unwrap().status,
        TournamentStatus::Stopped,
        "resumed tournament should start as Stopped, not Idle"
    );

    t2.go();
    assert!(
        wait_status(&snap2, TournamentStatus::Finished, Duration::from_secs(60)).await,
        "resumed tournament did not finish"
    );

    let final_snap = snap2.lock().unwrap().clone();
    assert_eq!(
        final_snap.games_finished, 6,
        "all 6 games should be finished"
    );
    assert_eq!(final_snap.games_total, 6);
    // Each of the 3 engines should have played exactly 4 games.
    for id in final_snap.standings.engines() {
        assert_eq!(
            final_snap.standings.standing(id).games(),
            4,
            "each engine should have played 4 games"
        );
    }

    // Verify DB: all games finished.
    let store3 = Store::open(&db_path).unwrap();
    let db_games = store3.list_games(tid).unwrap();
    assert_eq!(db_games.len(), 6);
    assert!(
        db_games.iter().all(|g| g.status == store::GAME_FINISHED),
        "all DB games should be finished after resume"
    );

    handle2.abort();
}

/// Opening assignment is engine-independent: one opening per *encounter*, both
/// colours sharing it, cycling when there are more encounters than openings.
/// This exercises `create_tournament`'s scheduling/persistence without engines.
#[test]
fn openings_assigned_per_encounter_and_persisted() {
    use colosseum_core::{OpeningBook, StartPosition};

    let dir = tempfile::tempdir().unwrap();
    let epd = dir.path().join("book.epd");
    // Two distinct positions (1.e4 and 1.d4 reached as Black-to-move EPDs).
    std::fs::write(
        &epd,
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3\n\
         rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq d3\n",
    )
    .unwrap();

    let db = dir.path().join("colosseum.sqlite");
    let store = Store::open(&db).unwrap();

    let mut config = TournamentConfig {
        format: Format::RoundRobin { cycles: 1 },
        games_per_pair: 2,
        ..Default::default()
    };
    config.start_position = StartPosition::Book(OpeningBook::new(epd));

    // 3 engines -> 3 encounters; with 2 openings the third encounter cycles back.
    let engines = vec![
        EngineConfig::new("/nonexistent/a".into()),
        EngineConfig::new("/nonexistent/b".into()),
        EngineConfig::new("/nonexistent/c".into()),
    ];
    let (events_tx, _rx) = crossbeam_channel::unbounded();
    let (tournament, _driver) =
        create_tournament("Book", config, engines, store, events_tx).unwrap();

    let reopened = Store::open(&db).unwrap();
    let games = reopened.list_games(tournament.id).unwrap();
    assert_eq!(games.len(), 6, "3 pairs * 2 games");

    // Every game has an assigned opening FEN.
    assert!(games.iter().all(|g| g.start_fen.is_some()));
    // Both games of an encounter share an opening (colours swap, position is the same).
    assert_eq!(games[0].start_fen, games[1].start_fen);
    assert_eq!(games[2].start_fen, games[3].start_fen);
    assert_eq!(games[4].start_fen, games[5].start_fen);
    // Distinct encounters draw distinct openings...
    assert_ne!(games[0].start_fen, games[2].start_fen);
    // ...and the book cycles: encounter 3 reuses opening 1.
    assert_eq!(games[4].start_fen, games[0].start_fen);
}

/// Capstone: a full tournament with an EPD opening book, driven by real engines,
/// runs to completion and every game starts from a book position (FEN-tagged PGN).
#[tokio::test]
async fn tournament_with_openings_runs_to_completion() {
    use colosseum_core::{OpeningBook, StartPosition};

    let Some((_guard, exe)) = common::engine_or_skip() else {
        eprintln!("skipping tournament_with_openings_runs_to_completion: no engine");
        return;
    };
    let (_dir, store, db_path) = temp_db();

    let book_path = _dir.path().join("book.epd");
    std::fs::write(
        &book_path,
        "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3\n\
         rnbqkbnr/pppppppp/8/8/3P4/8/PPP1PPPP/RNBQKBNR b KQkq d3\n",
    )
    .unwrap();

    let engines = vec![
        engine_cfg("SF-A", &exe, &[("Hash", "16")]),
        engine_cfg("SF-B", &exe, &[("Hash", "16"), ("Skill Level", "4")]),
    ];

    let mut config = fast_config(2, 8, 10);
    config.start_position = StartPosition::Book(OpeningBook::new(book_path));

    let (events_tx, _rx) = crossbeam_channel::unbounded();
    let (tournament, driver) =
        create_tournament("Booked", config, engines, store, events_tx).unwrap();
    let handle = tokio::spawn(driver);

    tournament.go();
    let snapshot = tournament.snapshot_handle();
    assert!(
        wait_status(
            &snapshot,
            TournamentStatus::Finished,
            Duration::from_secs(120)
        )
        .await,
        "tournament did not finish in time"
    );

    // 2 engines, double round robin => 1 pair * 2 games.
    let reopened = Store::open(&db_path).unwrap();
    let tid = reopened.list_tournaments().unwrap()[0].id;
    let games = reopened.list_games(tid).unwrap();
    assert_eq!(games.len(), 2);
    assert!(games.iter().all(|g| g.status == store::GAME_FINISHED));
    // Both games started from a book position and recorded it in the PGN.
    for g in &games {
        assert!(g.start_fen.is_some(), "game missing opening FEN");
        assert!(
            g.pgn.as_deref().unwrap().contains("[FEN \""),
            "PGN should carry the opening FEN tag"
        );
    }

    handle.abort();
}
