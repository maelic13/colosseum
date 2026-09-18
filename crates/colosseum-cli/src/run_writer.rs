//! The one writer of a run directory while its run is going.
//!
//! Every write a running command makes to its directory goes through here, on
//! a blocking thread: journal lines, PGN games, `run.log` lines, checkpoints,
//! the run record and the final artifacts. None of it happens on a runtime
//! worker. That is not tidiness: a worker blocked in `fsync` or a rename is a
//! worker not driving a game, and the time a game waits there used to be
//! charged to its engine.
//!
//! Durability is group-committed. Appends are not synced one by one; the
//! journal, `games.pgn` and `run.log` are synced together every
//! [`SYNC_EVERY_GAMES`] games or [`SYNC_INTERVAL`], whichever comes first, and
//! at every checkpoint and every barrier. A hard kill loses at most the games
//! of that window. A resume does not have them and plays them again.
//!
//! The queue in front of the writer is bounded. A run whose games finish
//! faster than the disk takes them waits for room instead of holding an
//! unbounded backlog in memory, and it waits off the runtime: the committing
//! task gives up its worker first, so the other games' tasks keep running and
//! their clocks keep being read.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::RunDirectory;
use crate::journal::{
    GameRecord, JOURNAL_FILE, JournalAnchor, JournalResume, PGN_FILE, PgnSpan, encode_line, hex,
};

/// Games appended between two syncs at most.
pub const SYNC_EVERY_GAMES: u32 = 50;
/// Time between two syncs at most, while anything is unsynced.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(1);
/// Commands the writer may be behind by before a committing task waits.
pub const WRITER_QUEUE_CAPACITY: usize = 256;

enum Command {
    Game {
        record: Box<GameRecord>,
        pgn: String,
    },
    Log(Vec<u8>),
    Checkpoint(Value),
    Replace {
        path: PathBuf,
        bytes: Vec<u8>,
    },
    /// A replace whose sender waits to learn whether it was written.
    ReplaceAndReport {
        path: PathBuf,
        bytes: Vec<u8>,
        reply: mpsc::SyncSender<Result<(), String>>,
    },
    /// Stop writing until the sender releases the writer. Tests use it to
    /// stand in for a disk that has fallen behind.
    #[cfg(test)]
    Hold(mpsc::Receiver<()>),
    Barrier(tokio::sync::oneshot::Sender<Result<JournalAnchor, String>>),
}

/// The handle a run holds. Cloning it shares the one writer.
#[derive(Clone)]
pub struct RunWriter {
    sender: mpsc::SyncSender<Command>,
    failure: Arc<Mutex<Option<String>>>,
    root: PathBuf,
}

impl RunWriter {
    /// Open the run directory's append-only files at the position a load left
    /// them and start the writer.
    pub async fn start(
        directory: Arc<RunDirectory>,
        resume: JournalResume,
    ) -> Result<Self, String> {
        Self::start_with_capacity(directory, resume, WRITER_QUEUE_CAPACITY).await
    }

    async fn start_with_capacity(
        directory: Arc<RunDirectory>,
        resume: JournalResume,
        capacity: usize,
    ) -> Result<Self, String> {
        let root = directory.paths().root.clone();
        let run_root = root.clone();
        let log = directory.paths().log.clone();
        let files = tokio::task::spawn_blocking(move || -> std::io::Result<_> {
            let append = |path: PathBuf| OpenOptions::new().create(true).append(true).open(path);
            Ok((
                append(root.join(JOURNAL_FILE))?,
                append(root.join(PGN_FILE))?,
                append(log)?,
            ))
        })
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| format!("could not open the run directory's files: {error}"))?;
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let failure = Arc::new(Mutex::new(None));
        let mut state = WriterState {
            directory,
            journal: files.0,
            pgn: files.1,
            log: files.2,
            records: resume.next_seq - 1,
            resume,
            dirty: false,
            unsynced_games: 0,
            last_sync: Instant::now(),
        };
        let shared = Arc::clone(&failure);
        tokio::task::spawn_blocking(move || state.run(&receiver, &shared));
        Ok(Self {
            sender,
            failure,
            root: run_root,
        })
    }

    /// The run directory this writer writes.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The first write that failed, if one has. A run stops on it at its next
    /// commit rather than keep playing games nothing can record.
    #[must_use]
    pub fn failure(&self) -> Option<String> {
        self.failure.lock().ok().and_then(|failure| failure.clone())
    }

    fn send(&self, command: Command) -> Result<(), String> {
        if let Some(failure) = self.failure() {
            return Err(failure);
        }
        let stopped = || "the run directory writer has stopped".to_owned();
        let command = match self.sender.try_send(command) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(_)) => return Err(stopped()),
            Err(TrySendError::Full(command)) => command,
        };
        // The disk is behind. Wait for room, but never on a runtime worker:
        // this task hands its worker over first, so every other game keeps
        // being driven while this one waits.
        off_the_runtime(|| self.sender.send(command)).map_err(|_| stopped())
    }

    /// Replace a whole file and wait to learn whether it was written. A run's
    /// final record goes this way: if the writer has failed it writes nothing
    /// more, and the caller must write the record itself.
    pub fn replace_and_report(&self, path: PathBuf, bytes: Vec<u8>) -> Result<(), String> {
        let (reply, answer) = mpsc::sync_channel(1);
        self.send(Command::ReplaceAndReport { path, bytes, reply })?;
        off_the_runtime(|| answer.recv())
            .unwrap_or_else(|_| Err("the run directory writer has stopped".to_owned()))
    }

    /// Append one game: its moves to `games.pgn`, its record to the journal.
    pub fn game(&self, record: GameRecord, pgn: String) -> Result<(), String> {
        self.send(Command::Game {
            record: Box::new(record),
            pgn,
        })
    }

    /// Append one line to `run.log`. The line must end in a newline.
    pub fn log(&self, bytes: Vec<u8>) -> Result<(), String> {
        self.send(Command::Log(bytes))
    }

    /// Sync, then write a checkpoint holding `aggregates` and the journal
    /// position they cover.
    pub fn checkpoint(&self, aggregates: Value) -> Result<(), String> {
        self.send(Command::Checkpoint(aggregates))
    }

    /// Replace a whole file atomically, such as the run record or the final
    /// result.
    pub fn replace(&self, path: PathBuf, bytes: Vec<u8>) -> Result<(), String> {
        self.send(Command::Replace { path, bytes })
    }

    /// Stop the writer until `release` is dropped or sent to.
    #[cfg(test)]
    fn hold(&self, release: mpsc::Receiver<()>) {
        self.send(Command::Hold(release)).unwrap();
    }

    /// Wait until everything sent so far is written and synced.
    pub async fn barrier(&self) -> Result<JournalAnchor, String> {
        let (reply, receive) = tokio::sync::oneshot::channel();
        self.send(Command::Barrier(reply))?;
        receive
            .await
            .map_err(|_| "the run directory writer has stopped".to_owned())?
    }
}

struct WriterState {
    directory: Arc<RunDirectory>,
    journal: File,
    pgn: File,
    log: File,
    resume: JournalResume,
    records: u64,
    dirty: bool,
    unsynced_games: u32,
    last_sync: Instant,
}

impl WriterState {
    fn run(&mut self, receiver: &mpsc::Receiver<Command>, failure: &Mutex<Option<String>>) {
        let mut failed = false;
        loop {
            let wait = if self.dirty {
                SYNC_INTERVAL.saturating_sub(self.last_sync.elapsed())
            } else {
                Duration::from_secs(3600)
            };
            let outcome = match receiver.recv_timeout(wait) {
                Ok(Command::Barrier(reply)) => {
                    let result = if failed {
                        Err(failure
                            .lock()
                            .ok()
                            .and_then(|failure| failure.clone())
                            .unwrap_or_default())
                    } else {
                        self.sync().map(|()| self.anchor())
                    };
                    let _ = reply.send(result.clone());
                    result.map(|_| ())
                }
                Ok(Command::ReplaceAndReport { path, bytes, reply }) => {
                    let result = if failed {
                        Err(failure
                            .lock()
                            .ok()
                            .and_then(|failure| failure.clone())
                            .unwrap_or_default())
                    } else {
                        self.replace(&path, &bytes)
                    };
                    let _ = reply.send(result.clone());
                    result
                }
                // Once a write has failed, nothing more is written: a journal
                // line whose moves are missing is worse than no line at all.
                Ok(_) if failed => Ok(()),
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => self.sync(),
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = self.sync();
                    return;
                }
            };
            if let Err(error) = outcome
                && !failed
            {
                failed = true;
                if let Ok(mut slot) = failure.lock() {
                    *slot = Some(error);
                }
            }
        }
    }

    fn handle(&mut self, command: Command) -> Result<(), String> {
        match command {
            Command::Game { record, pgn } => self.append_game(&record, &pgn),
            Command::Log(bytes) => {
                self.log
                    .write_all(&bytes)
                    .map_err(|error| format!("could not append to run.log: {error}"))?;
                self.dirty = true;
                Ok(())
            }
            Command::Checkpoint(mut aggregates) => {
                self.sync()?;
                let anchor =
                    serde_json::to_value(self.anchor()).expect("a journal anchor is serializable");
                if let Some(object) = aggregates.as_object_mut() {
                    object.insert("journal".into(), anchor);
                }
                self.directory
                    .write_checkpoint(&aggregates)
                    .map_err(|error| format!("could not write the checkpoint: {error}"))
            }
            Command::Replace { path, bytes } => self.replace(&path, &bytes),
            #[cfg(test)]
            Command::Hold(release) => {
                let _ = release.recv();
                Ok(())
            }
            Command::ReplaceAndReport { .. } | Command::Barrier(_) => {
                unreachable!("answered by the loop")
            }
        }
    }

    fn replace(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        crate::run_directory::replace_file(path, bytes)
            .map_err(|error| format!("could not write {}: {error}", path.display()))
    }

    fn append_game(&mut self, record: &GameRecord, pgn: &str) -> Result<(), String> {
        let mut moves = pgn.trim_end().as_bytes().to_vec();
        moves.extend_from_slice(b"\n\n");
        let span = PgnSpan {
            offset: self.resume.pgn_offset,
            length: moves.len() as u64,
            sha256: hex(&Sha256::digest(&moves)),
        };
        // Moves first, then the line that points at them. A resume trusts a
        // line only if the moves it names are there.
        self.pgn
            .write_all(&moves)
            .map_err(|error| format!("could not append to games.pgn: {error}"))?;
        self.resume.pgn_offset += moves.len() as u64;
        let line = encode_line(self.resume.next_seq, record, &span);
        self.journal
            .write_all(&line)
            .map_err(|error| format!("could not append to games.jsonl: {error}"))?;
        self.resume.hasher.update(&line);
        self.resume.offset += line.len() as u64;
        self.resume.next_seq += 1;
        self.records += 1;
        self.dirty = true;
        self.unsynced_games += 1;
        if self.unsynced_games >= SYNC_EVERY_GAMES || self.last_sync.elapsed() >= SYNC_INTERVAL {
            self.sync()?;
        }
        Ok(())
    }

    fn sync(&mut self) -> Result<(), String> {
        if self.dirty {
            for (file, name) in [
                (&self.journal, JOURNAL_FILE),
                (&self.pgn, PGN_FILE),
                (&self.log, "run.log"),
            ] {
                file.sync_data()
                    .map_err(|error| format!("could not sync {name}: {error}"))?;
            }
        }
        self.dirty = false;
        self.unsynced_games = 0;
        self.last_sync = Instant::now();
        Ok(())
    }

    fn anchor(&self) -> JournalAnchor {
        JournalAnchor {
            offset: self.resume.offset,
            sha256: hex(&self.resume.hasher.clone().finalize()),
            records: self.records,
            pgn_offset: self.resume.pgn_offset,
        }
    }
}

/// Run a blocking wait without holding a runtime worker.
///
/// On a multi-threaded runtime the worker is handed to the other tasks first.
/// Outside a runtime, or on a single-threaded one, where there is no other
/// worker to hand over to, it simply waits: the writer runs on its own thread
/// and drains the queue whatever the runtime does.
fn off_the_runtime<T>(wait: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(wait)
        }
        _ => wait(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::journal::{LoadMode, load_journal};
    use crate::{built_in_defaults, resolve_config};
    use colosseum_core::{GameResult, Termination};
    use colosseum_engine::ClockAccountingReport;
    use serde_json::json;

    fn directory(root: &std::path::Path) -> Arc<RunDirectory> {
        let config = resolve_config(
            built_in_defaults(),
            None,
            json!({"command": "match"}),
            &[],
            root,
            &[],
        )
        .unwrap();
        Arc::new(
            RunDirectory::open_explicit(&root.join("run"), &config, false)
                .unwrap()
                .directory,
        )
    }

    fn record(number: u32) -> GameRecord {
        GameRecord {
            number,
            pair_number: number.div_ceil(2),
            pair_game: 2 - number % 2,
            white: "a".into(),
            black: "b".into(),
            result: GameResult::WhiteWin,
            scorable: true,
            termination: Termination::Checkmate,
            opening: crate::match_runner::OpeningAssignment {
                book_index: None,
                label: "startpos".into(),
            },
            fault: None,
            sample: "official".into(),
            clock: ClockAccountingReport {
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
            iteration: None,
            round: None,
            error: None,
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_checkpoint_names_exactly_the_journal_it_covers() {
        let root = tempfile::tempdir().unwrap();
        let directory = directory(root.path());
        let writer = RunWriter::start(Arc::clone(&directory), JournalResume::fresh())
            .await
            .unwrap();
        for number in 1..=7 {
            writer
                .game(record(number), format!("[Round \"{number}\"]\n\n1-0"))
                .unwrap();
        }
        writer
            .checkpoint(json!({"command": "match", "games": 7}))
            .unwrap();
        let anchor = writer.barrier().await.unwrap();
        assert_eq!(anchor.records, 7);

        let payload: Value = directory.read_checkpoint().unwrap();
        assert_eq!(payload["games"], 7);
        let stored: JournalAnchor = serde_json::from_value(payload["journal"].clone()).unwrap();
        assert_eq!(stored, anchor);

        let loaded =
            load_journal(&directory.paths().root, Some(&stored), LoadMode::Repair).unwrap();
        assert_eq!(loaded.records.len(), 7);
        assert_eq!(loaded.covered, 7);
        let pgn = std::fs::read_to_string(directory.paths().root.join(PGN_FILE)).unwrap();
        assert_eq!(pgn.matches("[Round ").count(), 7);
        assert!(
            !std::fs::read_to_string(directory.paths().root.join(JOURNAL_FILE))
                .unwrap()
                .contains("[Round")
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn appending_resumes_where_a_load_left_the_files() {
        let root = tempfile::tempdir().unwrap();
        let directory = directory(root.path());
        let writer = RunWriter::start(Arc::clone(&directory), JournalResume::fresh())
            .await
            .unwrap();
        for number in 1..=3 {
            writer.game(record(number), "1-0".into()).unwrap();
        }
        writer.checkpoint(json!({})).unwrap();
        writer.barrier().await.unwrap();
        drop(writer);

        let payload: Value = directory.read_checkpoint().unwrap();
        let anchor: JournalAnchor = serde_json::from_value(payload["journal"].clone()).unwrap();
        let loaded =
            load_journal(&directory.paths().root, Some(&anchor), LoadMode::Repair).unwrap();
        let writer = RunWriter::start(Arc::clone(&directory), loaded.resume)
            .await
            .unwrap();
        for number in 4..=5 {
            writer.game(record(number), "1-0".into()).unwrap();
        }
        let anchor = writer.barrier().await.unwrap();
        let loaded =
            load_journal(&directory.paths().root, Some(&anchor), LoadMode::ReadOnly).unwrap();
        assert_eq!(
            loaded
                .records
                .iter()
                .map(|game| game.number)
                .collect::<Vec<_>>(),
            [1, 2, 3, 4, 5]
        );
    }

    /// A disk that falls behind stalls the committing task, never the runtime.
    ///
    /// One worker thread: if the full queue blocked it, nothing else on the
    /// runtime could run, and the ticker below would stop. The writer is held,
    /// so the queue fills and the committing task has to wait.
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn a_full_queue_waits_off_the_runtime_and_the_other_tasks_keep_running() {
        use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
        let root = tempfile::tempdir().unwrap();
        let directory = directory(root.path());
        let writer = RunWriter::start_with_capacity(directory, JournalResume::fresh(), 2)
            .await
            .unwrap();
        let (release, held) = mpsc::channel();
        writer.hold(held);

        let ticks = Arc::new(AtomicU32::new(0));
        let ticker = {
            let ticks = Arc::clone(&ticks);
            tokio::spawn(async move {
                loop {
                    ticks.fetch_add(1, Ordering::Relaxed);
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
        };
        let committed = Arc::new(AtomicBool::new(false));
        let committer = {
            let writer = writer.clone();
            let committed = Arc::clone(&committed);
            tokio::spawn(async move {
                for line in 0..6 {
                    writer.log(format!("line {line}\n").into_bytes()).unwrap();
                }
                committed.store(true, Ordering::Relaxed);
            })
        };

        // The test body is not a worker, and it must not depend on one: if the
        // worker were blocked, a runtime timer here would never fire.
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !committed.load(Ordering::Relaxed),
            "six commands went into a queue of two while the writer was held"
        );
        let before = ticks.load(Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            ticks.load(Ordering::Relaxed) > before + 3,
            "the runtime stopped while a task waited for the writer"
        );

        release.send(()).unwrap();
        committer.await.unwrap();
        writer.barrier().await.unwrap();
        ticker.abort();
        let log = std::fs::read_to_string(root.path().join("run").join("run.log")).unwrap();
        assert_eq!(log.matches("line ").count(), 6, "{log}");
    }

    /// A writer that failed writes nothing more, and a run that ended must
    /// still say so on disk: the terminal record is written directly.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_writer_still_leaves_a_terminal_run_record() {
        use crate::{RunRecord, RunRecorder, RunStatus};
        let root = tempfile::tempdir().unwrap();
        let directory = directory(root.path());
        let run = directory.paths().root.clone();
        let writer = RunWriter::start(Arc::clone(&directory), JournalResume::fresh())
            .await
            .unwrap();
        let mut finished = RunRecorder::begin(&directory, "match").unwrap();
        finished.write_through(writer.clone());
        // A write the writer cannot make: it latches the failure.
        writer
            .replace(run.join("no-such-directory").join("file"), b"x".to_vec())
            .unwrap();
        assert!(writer.barrier().await.is_err());
        assert!(finished.update_progress(progress_block()).is_err());

        finished.finish(RunStatus::Completed).unwrap();
        let record = RunRecord::read(&run).unwrap();
        assert_eq!(record.status, RunStatus::Completed);
        assert!(
            record
                .anomalies
                .iter()
                .any(|anomaly| anomaly.code == "writer-failed"),
            "{record:?}"
        );

        // A run abandoned by its owner is recorded the same way.
        let other = root.path().join("other");
        let config = resolve_config(
            built_in_defaults(),
            None,
            json!({"command": "match"}),
            &[],
            root.path(),
            &[],
        )
        .unwrap();
        let other_directory = Arc::new(
            RunDirectory::open_explicit(&other, &config, false)
                .unwrap()
                .directory,
        );
        let other_writer = RunWriter::start(Arc::clone(&other_directory), JournalResume::fresh())
            .await
            .unwrap();
        let mut abandoned = RunRecorder::begin(&other_directory, "match").unwrap();
        abandoned.write_through(other_writer.clone());
        other_writer
            .replace(other.join("no-such-directory").join("file"), b"x".to_vec())
            .unwrap();
        assert!(other_writer.barrier().await.is_err());
        drop(abandoned);
        let record = RunRecord::read(&other).unwrap();
        assert_eq!(record.status, RunStatus::Aborted);
        assert!(
            record
                .anomalies
                .iter()
                .any(|anomaly| anomaly.code == "writer-failed")
        );
    }

    fn progress_block() -> crate::progress::ProgressBlock {
        crate::progress::ProgressBlock::new(
            "match",
            crate::progress::ProgressUnit::Games,
            1,
            Some(2),
            Duration::from_secs(1),
        )
    }
}
