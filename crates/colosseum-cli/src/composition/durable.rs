//! What every durable command shares: resuming from its journal, and when to
//! write a checkpoint.

use std::time::Instant;

use super::*;
use crate::journal::{self, GameRecord, JournalAnchor, JournalResume, LoadMode, LoadedJournal};
use crate::progress::RunClock;
use crate::run_writer::{RUN_ELAPSED_FIELD, RunWriter};

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
    /// The time earlier invocations spent on the run; zero for a fresh one.
    pub(crate) prior_elapsed: Duration,
}

impl OpenedJournal {
    /// The run's clock for this invocation, continuing the earlier ones.
    pub(crate) fn clock(&self) -> RunClock {
        RunClock::resumed_after(self.prior_elapsed)
    }
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
            prior_elapsed: Duration::ZERO,
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
    let prior_elapsed = run_elapsed(&directory.paths().root, &loaded.records);
    Ok(OpenedJournal {
        records: loaded.records,
        resume: loaded.resume,
        prior_elapsed,
    })
}

/// Where a resumed run stood when this invocation started, in the run's own
/// unit: what the operator reads on resume, what `run.log` records, and what
/// the `--json` value carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub(crate) struct ResumeFacts {
    pub(crate) unit: ProgressUnit,
    pub(crate) completed_units: u64,
    /// Units still to play, where the run has a fixed horizon or a cap.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) remaining_units: Option<u64>,
}

impl ResumeFacts {
    /// The facts of a resumed run; `None` for a fresh one.
    pub(crate) fn of(
        resumed: bool,
        unit: ProgressUnit,
        completed: u64,
        total: Option<u64>,
    ) -> Option<Self> {
        resumed.then(|| Self {
            unit,
            completed_units: completed,
            remaining_units: total.map(|total| total.saturating_sub(completed)),
        })
    }

    /// The note an operator reads: "resuming: 1240 of 2000 games complete, 760
    /// to play".
    pub(crate) fn note(&self) -> String {
        let unit = self.unit.plural();
        match self.remaining_units {
            Some(remaining) => format!(
                "resuming: {} of {} {unit} complete, {remaining} to play",
                self.completed_units,
                self.completed_units + remaining
            ),
            None => format!("resuming: {} {unit} complete", self.completed_units),
        }
    }
}

/// Say that a run resumed: the note on standard error, in every output mode,
/// and the same facts as one `run.log` event.
pub(crate) fn announce_resume(writer: &RunWriter, facts: Option<ResumeFacts>) {
    if let Some(facts) = facts {
        eprintln!("{}", facts.note());
        log_event(
            writer,
            &json!({
                "event": "resumed",
                "unit": facts.unit,
                "completed_units": facts.completed_units,
                "remaining_units": facts.remaining_units,
            }),
        );
    }
}

/// Record a clean stop in `run.log`: how far the run got and the exit code it
/// stopped with, so the log shows where one invocation ended and the next
/// began.
pub(crate) fn log_stop(writer: &RunWriter, unit: ProgressUnit, completed: u64, exit_code: u8) {
    log_event(
        writer,
        &json!({
            "event": "stopped",
            "unit": unit,
            "completed_units": completed,
            "exit_code": exit_code,
        }),
    );
}

/// The time a run's earlier invocations spent, as its checkpoint recorded it.
///
/// A run directory written before the checkpoint carried it has only its
/// games' own spans to go on. Their union is the time some game was being
/// played: it leaves out the gaps between invocations, as it should, and
/// the seconds between games, which are small beside a game.
pub(crate) fn run_elapsed(root: &Path, records: &[GameRecord]) -> Duration {
    recorded_run_elapsed(root).unwrap_or_else(|| played_time(records))
}

/// The run's elapsed time as its newest readable checkpoint recorded it, when
/// it recorded one.
pub(crate) fn recorded_run_elapsed(root: &Path) -> Option<Duration> {
    RunDirectory::read_checkpoint_snapshot::<Value>(root)
        .ok()
        .and_then(|payload| payload.get(RUN_ELAPSED_FIELD)?.as_f64())
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(Duration::from_secs_f64)
}

/// The union of the games' slot spans.
fn played_time(records: &[GameRecord]) -> Duration {
    let mut spans = records
        .iter()
        .filter_map(|record| record.slot.as_ref())
        .map(|slot| {
            (
                slot.started_unix_us,
                slot.ended_unix_us.max(slot.started_unix_us),
            )
        })
        .collect::<Vec<_>>();
    spans.sort_unstable();
    let mut total = 0_u64;
    let mut current: Option<(u64, u64)> = None;
    for (start, end) in spans {
        current = match current {
            Some((open, close)) if start <= close => Some((open, close.max(end))),
            Some((open, close)) => {
                total += close - open;
                Some((start, end))
            }
            None => Some((start, end)),
        };
    }
    if let Some((open, close)) = current {
        total += close - open;
    }
    Duration::from_micros(total)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::match_runner::SlotOccupancy;
    use crate::run_writer::tests::{directory, record};

    const SECOND: u64 = 1_000_000;

    fn played(number: u32, from: u64, to: u64) -> GameRecord {
        GameRecord {
            slot: Some(SlotOccupancy {
                index: 0,
                started_unix_us: from * SECOND,
                ended_unix_us: to * SECOND,
            }),
            ..record(number)
        }
    }

    #[test]
    fn played_time_counts_overlapping_games_once_and_the_gaps_between_invocations_not_at_all() {
        let records = [
            played(1, 100, 110),
            played(2, 105, 115),
            // A stop, then a resumed invocation twenty minutes later.
            played(3, 1_300, 1_310),
            record(4),
        ];
        assert_eq!(played_time(&records), Duration::from_secs(25));
        assert_eq!(played_time(&[]), Duration::ZERO);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_checkpoint_carries_the_run_time_into_the_next_invocation() {
        let root = tempfile::tempdir().unwrap();
        let directory = directory(root.path());
        let run = directory.paths().root.clone();
        let records = [played(1, 100, 110)];

        // Before any checkpoint recorded it, the games are all there is.
        assert_eq!(run_elapsed(&run, &records), Duration::from_secs(10));

        let earlier = Duration::from_secs(3_600);
        let writer = RunWriter::start(Arc::clone(&directory), JournalResume::fresh())
            .await
            .unwrap()
            .on_clock(RunClock::resumed_after(earlier));
        writer.checkpoint(json!({"command": "match"})).unwrap();
        writer.barrier().await.unwrap();

        let carried = run_elapsed(&run, &records);
        assert!(carried >= earlier, "{carried:?}");
        assert!(carried < earlier + Duration::from_secs(60), "{carried:?}");
        assert_eq!(recorded_run_elapsed(&run), Some(carried));
    }
}
