//! The commit path's cost does not grow with the run.
//!
//! Thirty thousand games are committed through the observer a fixed match
//! uses, into a real run directory, with a stub game of realistic size. What
//! a game loop waits for is the observer call; what a run directory costs is
//! the writer's work behind it and the checkpoint it rewrites. None of the
//! three may grow from the first games to the last: before the journal, the
//! last commit of such a run rewrote a 167 MB checkpoint and a 167 MB PGN.

use std::time::Instant;

use colosseum_core::{GameResult, Termination};
use colosseum_engine::ClockAccountingReport;

use super::*;
use crate::match_runner::{MatchGame, MatchObserver, MatchSide, OpeningAssignment};

const GAMES: u32 = 30_000;
const WINDOW: u32 = 1_000;

fn stub_game(number: u32) -> MatchGame {
    let white = if number % 2 == 1 {
        MatchSide::A
    } else {
        MatchSide::B
    };
    // About the size of a short engine game with clock comments.
    let mut pgn = format!(
        "[Event \"Colosseum CLI fixed match\"]\n[Round \"{number}\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n"
    );
    for ply in 1..=60 {
        pgn.push_str(&format!(
            "{ply}. e4 {{+0.12/14 0.021s}} e5 {{-0.10/13 0.019s}} "
        ));
    }
    pgn.push_str("1/2-1/2\n\n");
    MatchGame {
        number,
        white,
        result: GameResult::WhiteWin,
        scorable: true,
        termination: Termination::Checkmate,
        clock_accounting: ClockAccountingReport {
            model: "test".into(),
            version: 1,
            white_margin_ms: 0,
            black_margin_ms: 0,
            monotonic_resolution_ns: 1,
            white_charged_elapsed: None,
            black_charged_elapsed: None,
            white_round_trip: None,
            black_round_trip: None,
        },
        opening: OpeningAssignment {
            book_index: Some(number.div_ceil(2) as usize - 1),
            label: "book".into(),
        },
        fault: None,
        error: None,
        pgn,
    }
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn checkpoint_size(root: &Path) -> u64 {
    fs::metadata(root.join("checkpoint.json")).unwrap().len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_thirty_thousand_game_run_commits_its_last_game_as_cheaply_as_its_first() {
    let root = tempfile::tempdir().unwrap();
    let config = resolve_config(
        built_in_defaults(),
        None,
        json!({"command": "match"}),
        &[],
        root.path(),
        &[],
    )
    .unwrap();
    let directory = Arc::new(
        RunDirectory::open_explicit(&root.path().join("run"), &config, false)
            .unwrap()
            .directory,
    );
    let run = directory.paths().root.clone();
    let writer = RunWriter::start(directory, crate::journal::JournalResume::fresh())
        .await
        .unwrap();
    let observer = DurableMatchOutput::new(writer.clone(), ProgressUnit::Games, &[]);

    let mut calls = Vec::with_capacity(GAMES as usize);
    let mut windows = Vec::new();
    let mut first_checkpoint = None;
    let mut window_start = Instant::now();
    for number in 1..=GAMES {
        let game = stub_game(number);
        let started = Instant::now();
        observer.game_completed(&game).unwrap();
        calls.push(started.elapsed());
        if number % WINDOW == 0 {
            // A window is the game loop's calls plus the writer catching up
            // with them: every byte of the window on disk and synced.
            writer.barrier().await.unwrap();
            windows.push(window_start.elapsed());
            window_start = Instant::now();
            if first_checkpoint.is_none() {
                first_checkpoint = Some(checkpoint_size(&run));
            }
        }
    }
    // Game 30,000 is a multiple of the checkpoint cadence, so the checkpoint
    // on disk now summarises the whole run.
    writer.barrier().await.unwrap();

    // What the game loop waits for.
    let first_calls = median(calls[..WINDOW as usize].to_vec());
    let last_calls = median(calls[calls.len() - WINDOW as usize..].to_vec());
    let first_window = windows[0];
    let last_window = *windows.last().unwrap();
    let first = first_checkpoint.unwrap();
    let last = checkpoint_size(&run);
    eprintln!(
        "median commit {first_calls:?} -> {last_calls:?}; {WINDOW} commits synced {first_window:?} -> {last_window:?}; checkpoint {first} -> {last} bytes"
    );
    assert!(
        last_calls <= first_calls * 4 + Duration::from_micros(200),
        "the median commit grew from {first_calls:?} to {last_calls:?}"
    );
    // What the writer does behind it, synced to disk.
    assert!(
        last_window <= first_window * 4 + Duration::from_millis(500),
        "a thousand commits took {first_window:?} at the start and {last_window:?} at the end"
    );
    // The checkpoint holds aggregates: its size is the same whether it
    // summarises a thousand games or thirty thousand, give or take the digits
    // of the counts in it.
    let (sample, _) = observer.sample();
    assert_eq!(sample.scored_games, GAMES);
    assert!(
        last <= first + 64,
        "the checkpoint grew from {first} to {last} bytes"
    );
    // And every game is there, once, in the journal and in the PGN.
    let journal = crate::journal::read_journal_bytes(&fs::read(run.join("games.jsonl")).unwrap());
    assert_eq!(journal.len(), GAMES as usize);
    let pgn = fs::read_to_string(run.join("games.pgn")).unwrap();
    assert_eq!(pgn.matches("[Event ").count(), GAMES as usize);
}
