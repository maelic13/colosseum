//! What every durable command shares: resuming from its journal, and when to
//! write a checkpoint.

use std::time::Instant;

use super::*;
use crate::journal::{self, GameRecord, JournalAnchor, JournalResume, LoadMode, LoadedJournal};
use crate::run_writer::RunWriter;

/// Units committed between two checkpoints at most.
pub(crate) const CHECKPOINT_EVERY_UNITS: u64 = 50;
/// Time between two checkpoints at most, while units are being committed.
pub(crate) const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(5);

/// When a checkpoint is due: every [`CHECKPOINT_EVERY_UNITS`] units or every
/// [`CHECKPOINT_INTERVAL`], whichever comes first. A checkpoint is constant in
/// size, so its cost does not grow with the run; only how often it is paid
/// is bounded here.
#[derive(Debug)]
pub(crate) struct CheckpointCadence {
    since: u64,
    last: Instant,
}

impl CheckpointCadence {
    pub(crate) fn new() -> Self {
        Self {
            since: 0,
            last: Instant::now(),
        }
    }

    /// Record `units` newly committed; true when a checkpoint is due now.
    pub(crate) fn record(&mut self, units: u64) -> bool {
        self.since += units;
        if self.since >= CHECKPOINT_EVERY_UNITS || self.last.elapsed() >= CHECKPOINT_INTERVAL {
            self.since = 0;
            self.last = Instant::now();
            true
        } else {
            false
        }
    }
}

/// The games a run directory already holds, and where appending continues.
pub(crate) struct OpenedJournal {
    pub(crate) records: Vec<GameRecord>,
    pub(crate) resume: JournalResume,
}

/// Read a resumed run's journal, verified against its checkpoint, on a
/// blocking thread. A fresh run has nothing to read.
///
/// A torn last line, and games whose moves never reached `games.pgn`, are cut
/// off and said so: they were inside the last sync window, and the resumed
/// run plays them again. A journal that does not match its checkpoint is a
/// refusal.
pub(crate) async fn open_journal(
    directory: &Arc<RunDirectory>,
    resumed: bool,
) -> Result<OpenedJournal, String> {
    if !resumed {
        return Ok(OpenedJournal {
            records: Vec::new(),
            resume: JournalResume::fresh(),
        });
    }
    let root = directory.paths().root.clone();
    let loaded = tokio::task::spawn_blocking(move || -> Result<LoadedJournal, String> {
        let anchor = read_anchor(&root)?;
        journal::load_journal(&root, anchor.as_ref(), LoadMode::Repair)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())??;
    let discarded = loaded.discarded;
    if discarded.torn_bytes > 0 {
        eprintln!(
            "resume: dropped an unfinished last journal line ({} bytes); its game is played again",
            discarded.torn_bytes
        );
    }
    if discarded.games_without_moves > 0 {
        eprintln!(
            "resume: {} journalled game(s) had not reached games.pgn when the run stopped; they are played again",
            discarded.games_without_moves
        );
    }
    Ok(OpenedJournal {
        records: loaded.records,
        resume: loaded.resume,
    })
}

/// The journal position the newest valid checkpoint covers. `None` when no
/// checkpoint was ever written, as in a run killed before its first: then
/// every journal line is verified on its own.
pub(crate) fn read_anchor(root: &Path) -> Result<Option<JournalAnchor>, String> {
    if !root.join("checkpoint.json").exists() && !root.join("checkpoint.previous.json").exists() {
        return Ok(None);
    }
    let payload: Value =
        RunDirectory::read_checkpoint_snapshot(root).map_err(|error| error.to_string())?;
    let anchor = payload.get("journal").cloned().ok_or(
        "the checkpoint names no journal position; this run directory was written by an earlier Colosseum version and cannot be resumed; run the same command with --restart, which archives this directory and starts afresh",
    )?;
    serde_json::from_value(anchor)
        .map(Some)
        .map_err(|error| format!("the checkpoint's journal position is unreadable: {error}"))
}

/// A faulted game as a human event: which game, who faulted and how. The game
/// itself is in the journal; this is the line a person scanning `run.log`
/// needs.
pub(crate) fn log_fault(writer: &RunWriter, record: &GameRecord) {
    if let Some(fault) = &record.fault {
        log_event(
            writer,
            &json!({
                "event": "fault",
                "game": record.number,
                "white": record.white,
                "black": record.black,
                "termination": record.termination,
                "fault": fault,
            }),
        );
    }
}

/// The complete colour-reversed pairs of one sample class, in pair order.
///
/// A pair's two games are journalled one after the other, so a kill can leave
/// only the first of them. That half pair is not evidence of anything: it is
/// dropped here and played again, and because pairs are journalled in pair
/// order nothing after it can exist.
pub(crate) fn pairs_from_journal(
    records: &[GameRecord],
    class: &str,
) -> Vec<CompletePair<match_runner::MatchGame>> {
    let mut halves = BTreeMap::<u32, match_runner::MatchGame>::new();
    let mut pairs = Vec::new();
    for record in records.iter().filter(|record| record.sample == class) {
        let Some(game) = match_runner::MatchGame::from_journal(record) else {
            continue;
        };
        let pair_id = record.pair_number;
        match halves.remove(&pair_id) {
            Some(other) => {
                let (first, second) = if other.number < game.number {
                    (other, game)
                } else {
                    (game, other)
                };
                pairs.push(CompletePair {
                    pair_id,
                    first,
                    second,
                });
            }
            None => {
                halves.insert(pair_id, game);
            }
        }
    }
    pairs.sort_by_key(|pair| pair.pair_id);
    pairs
}

/// A tune's committed games, as the complete pairs of each iteration.
pub(crate) fn iterations_from_journal(
    records: &[GameRecord],
) -> BTreeMap<u32, Vec<CompletePair<match_runner::MatchGame>>> {
    let mut by_iteration = BTreeMap::<u32, Vec<GameRecord>>::new();
    for record in records
        .iter()
        .filter(|record| record.sample == OFFICIAL_SAMPLE)
    {
        if let Some(iteration) = record.iteration {
            by_iteration
                .entry(iteration)
                .or_default()
                .push(record.clone());
        }
    }
    by_iteration
        .into_iter()
        .map(|(iteration, records)| (iteration, pairs_from_journal(&records, OFFICIAL_SAMPLE)))
        .collect()
}

/// Wait for every write the run has made, and say so if one failed.
pub(crate) async fn settle(writer: &RunWriter) -> Result<(), String> {
    writer.barrier().await.map(|_| ())
}
