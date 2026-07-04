//! On-disk timing check: bulk schedule insertion must be fast enough that
//! starting a large tournament never freezes the UI.

use colosseum_core::{EngineId, GameId, TournamentConfig, TournamentId};
use colosseum_engine::{PendingGame, Store};

#[test]
fn bulk_insert_of_10500_games_is_fast_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("timing.sqlite")).unwrap();
    let tid = TournamentId::new();
    store
        .create_tournament(tid, "Timing", &TournamentConfig::default())
        .unwrap();

    let (a, b) = (EngineId::new(), EngineId::new());
    let opening = vec!["e2e4".to_string(), "e7e5".to_string()];
    let ids: Vec<GameId> = (0..10_500).map(|_| GameId::new()).collect();
    let rows: Vec<PendingGame<'_>> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| PendingGame {
            id: *id,
            round: (i / 2 + 1) as u32,
            white: a,
            black: b,
            start_fen: None,
            opening_moves: &opening,
        })
        .collect();

    let start = std::time::Instant::now();
    store.insert_pending_games(tid, &rows).unwrap();
    let elapsed = start.elapsed();
    println!("inserted 10500 games in {elapsed:?}");
    // Generous bound: the old per-row path took minutes; one transaction
    // should be well under a second even on slow disks.
    assert!(elapsed.as_secs() < 5, "bulk insert too slow: {elapsed:?}");
    assert_eq!(store.list_games(tid).unwrap().len(), 10_500);
}
