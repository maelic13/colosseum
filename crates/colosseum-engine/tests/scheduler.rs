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
use colosseum_engine::scheduler::{TournamentStatus, create_tournament};
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
