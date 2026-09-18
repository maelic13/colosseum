//! The `status` command: the official state of any run, unmodified.

use super::*;
use crate::journal::{self, JOURNAL_FILE, LoadMode};

pub(crate) fn run_status(run_directory: &Path, machine: bool) -> ExitCode {
    let record = match RunRecord::read(run_directory) {
        Ok(record) => record,
        Err(error) => {
            eprintln!("status failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let durable = durable_status(run_directory);
    if machine {
        print_json(&MachineOutput::RunStatus {
            run_directory,
            record,
            durable: durable.map(|durable| durable.to_json()),
        });
    } else {
        println!("command: {}", record.command);
        println!("status: {:?}", record.status);
        println!("config: {}", record.config_sha256);
        println!(
            "committed units: {}",
            record.official_sample.committed_units
        );
        println!("scored games: {}", record.official_sample.scored_games);
        println!("anomalies: {}", record.anomalies.len());
        if let Some(durable) = &durable {
            durable.print();
        }
        // The last block the run published, verbatim: a closed console loses
        // nothing, and `status` never invents a second account of the run.
        match &record.progress {
            Some(block) => print!("{}", block.render()),
            None => println!("progress: {}", first_block_due(&record)),
        }
    }
    ExitCode::SUCCESS
}

/// What the run directory holds on disk: the newest valid checkpoint's
/// aggregates, and the journal verified against it with its tail read line by
/// line. Reading it repairs nothing; a journal that does not match its
/// checkpoint is reported as the refusal a resume would meet.
pub(crate) struct DurableStatus {
    checkpoint: Option<Value>,
    journal: Result<JournalStatus, String>,
}

struct JournalStatus {
    games: usize,
    covered: usize,
    torn_bytes: u64,
    games_without_moves: u64,
}

impl DurableStatus {
    fn to_json(&self) -> Value {
        json!({
            "checkpoint": self.checkpoint,
            "journal": match &self.journal {
                Ok(journal) => json!({
                    "games": journal.games,
                    "covered_by_checkpoint": journal.covered,
                    "after_checkpoint": journal.games - journal.covered,
                    "torn_bytes": journal.torn_bytes,
                    "games_without_moves": journal.games_without_moves,
                }),
                Err(error) => json!({ "refused": error }),
            },
        })
    }

    fn print(&self) {
        match &self.journal {
            Ok(journal) => {
                println!(
                    "journal: {} committed, {} after the last checkpoint",
                    progress::plural(journal.games as u64, "game"),
                    journal.games - journal.covered
                );
                if journal.torn_bytes > 0 || journal.games_without_moves > 0 {
                    println!(
                        "journal tail: {} unfinished bytes and {} without moves; a resume plays them again",
                        journal.torn_bytes,
                        progress::plural(journal.games_without_moves, "game")
                    );
                }
            }
            Err(error) => println!("journal: refused: {error}"),
        }
    }
}

/// `None` for a run that plays no games: it keeps no journal, and its
/// checkpoint, if it has one, is its own business.
fn durable_status(root: &Path) -> Option<DurableStatus> {
    if !root.join(JOURNAL_FILE).exists() {
        return None;
    }
    let has_checkpoint =
        root.join("checkpoint.json").exists() || root.join("checkpoint.previous.json").exists();
    let checkpoint = if has_checkpoint {
        RunDirectory::read_checkpoint_snapshot::<Value>(root)
            .map_err(|error| error.to_string())
            .map(Some)
    } else {
        Ok(None)
    };
    let journal = checkpoint.clone().and_then(|payload| {
        let anchor = match payload.as_ref().and_then(|payload| payload.get("journal")) {
            Some(anchor) => Some(serde_json::from_value(anchor.clone()).map_err(|error| {
                format!("the checkpoint's journal position is unreadable: {error}")
            })?),
            None if payload.is_some() => {
                return Err("the checkpoint names no journal position".to_owned());
            }
            None => None,
        };
        let loaded = journal::load_journal(root, anchor.as_ref(), LoadMode::ReadOnly)
            .map_err(|error| error.to_string())?;
        Ok(JournalStatus {
            games: loaded.records.len(),
            covered: loaded.covered,
            torn_bytes: loaded.discarded.torn_bytes,
            games_without_moves: loaded.discarded.games_without_moves,
        })
    });
    Some(DurableStatus {
        checkpoint: checkpoint.ok().flatten().map(|mut payload| {
            // The anchor is the journal's business, reported above in words.
            if let Some(object) = payload.as_object_mut() {
                object.remove("journal");
            }
            payload
        }),
        journal,
    })
}

/// When a run that has printed no block yet will print its first: after the
/// number of units its `--progress-every` names, and never sooner than its
/// `--progress-min-secs` floor. A run that recorded no schedule says only that
/// nothing has been published.
pub(crate) fn first_block_due(record: &RunRecord) -> String {
    let progress = &record.workflow["progress"];
    let (Some(every), Some(unit), Some(min_secs)) = (
        progress["every"].as_u64(),
        progress["unit"].as_str(),
        progress["min_secs"].as_u64(),
    ) else {
        return "none published yet".to_owned();
    };
    format!(
        "none published yet; the first block is due once {every} {unit} have been committed in this invocation ({} committed in total so far), and no sooner than {min_secs} s after it started",
        record.official_sample.committed_units
    )
}
