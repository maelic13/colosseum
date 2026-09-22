//! The commit path's cost does not grow with the run.
//!
//! Three checkpoint cadences of games are committed through the observer a
//! fixed match uses, into a real run directory, with a stub game of realistic
//! size. What a run directory costs per game is the writer's work behind the
//! observer and the checkpoint it rewrites; neither may depend on how many
//! games came before. Before the journal, the last commit of a 30,000-game run
//! rewrote a 167 MB checkpoint and a 167 MB PGN. That is checked structurally
//! rather than by timing: the checkpoint stays the same size at every cadence,
//! and the journal and the PGN are only ever appended to.

use colosseum_core::{GameResult, Termination};
use colosseum_engine::ClockAccountingReport;

use super::*;
use crate::match_runner::{MatchGame, MatchObserver, MatchSide, OpeningAssignment};

const WINDOW: u32 = CHECKPOINT_EVERY_UNITS as u32;
const GAMES: u32 = 3 * WINDOW;

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
            phases: None,
        },
        opening: OpeningAssignment {
            book_index: Some(number.div_ceil(2) as usize - 1),
            label: "book".into(),
        },
        fault: None,
        error: None,
        pgn,

        slot: None,
    }
}

fn checkpoint_size(root: &Path) -> u64 {
    fs::metadata(root.join("checkpoint.json")).unwrap().len()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_growing_run_keeps_its_checkpoint_constant_and_only_appends_its_journal_and_pgn() {
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

    let mut checkpoints = Vec::new();
    let mut journal_before = Vec::new();
    let mut pgn_before = Vec::new();
    for number in 1..=GAMES {
        observer.game_completed(&stub_game(number)).unwrap();
        if number % WINDOW == 0 {
            // Every byte of the window on disk and synced, and the cadence's
            // checkpoint written.
            writer.barrier().await.unwrap();
            checkpoints.push(checkpoint_size(&run));
            // What was on disk before is untouched: a commit appends, it
            // never rewrites what earlier games wrote.
            let journal = fs::read(run.join("games.jsonl")).unwrap();
            let pgn = fs::read(run.join("games.pgn")).unwrap();
            assert!(
                journal.len() > journal_before.len() && journal.starts_with(&journal_before),
                "game {number}: the journal was rewritten instead of appended to"
            );
            assert!(
                pgn.len() > pgn_before.len() && pgn.starts_with(&pgn_before),
                "game {number}: the PGN was rewritten instead of appended to"
            );
            journal_before = journal;
            pgn_before = pgn;
        }
    }

    // The checkpoint holds aggregates: its size is the same whether it
    // summarises one cadence of games or three, give or take the digits of
    // the counts in it.
    let first = checkpoints[0];
    for (cadence, size) in checkpoints.iter().enumerate() {
        assert!(
            *size <= first + 64,
            "the checkpoint grew from {first} to {size} bytes by cadence {}",
            cadence + 1
        );
    }
    let (sample, _) = observer.sample();
    assert_eq!(sample.scored_games, GAMES);
    // And every game is there, once, in the journal and in the PGN.
    let journal = crate::journal::read_journal_bytes(&journal_before);
    let numbers: Vec<u32> = journal.iter().map(|record| record.number).collect();
    assert_eq!(numbers, (1..=GAMES).collect::<Vec<_>>());
    let pgn = String::from_utf8(pgn_before).unwrap();
    assert_eq!(pgn.matches("[Event ").count(), GAMES as usize);
}
