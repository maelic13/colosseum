//! The per-game journal, `games.jsonl`, and the PGN export appended beside it.
//!
//! A run directory keeps four files with four jobs. The journal is the record
//! of every game: one checksummed line per game, appended and never
//! rewritten, holding the game's identity, result, fault and clock summary
//! but never its moves. `games.pgn` holds the moves, appended one game at a
//! time. The checkpoint holds aggregates and the journal position they cover.
//! `run.log` holds what a person reads.
//!
//! Rewriting any of them per game made a commit cost grow with the run: late
//! in a 30,000-game calibration each game rewrote 167 MB, on the thread whose
//! delay was then charged to the next engine's move. Appending costs the same
//! at the first game and the last.
//!
//! Every journal line also names the byte range its game occupies in
//! `games.pgn` and what that range hashes to. The two files are synced
//! together, but between syncs the operating system may keep either one's
//! latest bytes, so after a hard kill the line alone cannot say whether its
//! game's moves reached the disk. The hash can.

use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

use colosseum_core::{GameResult, Termination};
use colosseum_engine::{ClockAccountingReport, GameFault};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::match_runner::OpeningAssignment;

pub const JOURNAL_FILE: &str = "games.jsonl";
pub const PGN_FILE: &str = "games.pgn";
/// Version of one journal line. A reader refuses a line of another version.
pub const JOURNAL_LINE_VERSION: u32 = 1;

/// One game as the journal records it: everything a statistic, a resume or a
/// replay needs, and nothing it does not. The moves are in `games.pgn`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameRecord {
    /// Harness game number in schedule order, counting from one.
    pub number: u32,
    /// The colour-reversed pair or tournament encounter.
    pub pair_number: u32,
    /// Which colour assignment of that pair.
    pub pair_game: u32,
    /// The side or participant that had White: `a`/`b` for a two-engine
    /// command, the participant identity for a tournament.
    pub white: String,
    pub black: String,
    pub result: GameResult,
    pub scorable: bool,
    pub termination: Termination,
    pub opening: OpeningAssignment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fault: Option<GameFault>,
    /// The `ColosseumSample` class the game was written with.
    pub sample: String,
    /// Charged time per side: min, median and max, with the margins used.
    pub clock: ClockAccountingReport,
    /// The SPSA iteration the game belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iteration: Option<u32>,
    /// The tournament round.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Where a game's moves sit in `games.pgn`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PgnSpan {
    pub offset: u64,
    pub length: u64,
    pub sha256: String,
}

/// One line of the journal before its checksum is attached.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LineBody {
    v: u32,
    seq: u64,
    game: GameRecord,
    pgn: PgnSpan,
}

/// The journal position a checkpoint covers: every byte before `offset`, what
/// those bytes hash to, how many records they hold, and where `games.pgn`
/// ended at the same moment. Both files were synced before it was written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalAnchor {
    pub offset: u64,
    pub sha256: String,
    pub records: u64,
    pub pgn_offset: u64,
}

impl JournalAnchor {
    /// The anchor of an empty journal.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            offset: 0,
            sha256: hex(&Sha256::new().finalize()),
            records: 0,
            pgn_offset: 0,
        }
    }
}

/// Render one game as its journal line, newline-terminated.
pub fn encode_line(seq: u64, game: &GameRecord, pgn: &PgnSpan) -> Vec<u8> {
    let body = LineBody {
        v: JOURNAL_LINE_VERSION,
        seq,
        game: game.clone(),
        pgn: pgn.clone(),
    };
    let mut value = serde_json::to_value(&body).expect("a journal line is serializable");
    let checksum = line_checksum(&value);
    value
        .as_object_mut()
        .expect("a journal line is an object")
        .insert("sha256".into(), Value::String(checksum));
    let mut bytes = serde_json::to_vec(&value).expect("a journal line is serializable");
    bytes.push(b'\n');
    bytes
}

/// The checksum of a line: SHA-256 of its body in canonical key order.
fn line_checksum(body: &Value) -> String {
    hex(&Sha256::digest(
        serde_json::to_vec(body).expect("JSON value is serializable"),
    ))
}

/// Parse and verify one complete line (without its newline).
fn decode_line(bytes: &[u8]) -> Result<(u64, GameRecord, PgnSpan), String> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let expected = value
        .as_object_mut()
        .and_then(|object| object.remove("sha256"))
        .and_then(|checksum| checksum.as_str().map(str::to_owned))
        .ok_or("the line has no checksum")?;
    if line_checksum(&value) != expected {
        return Err("the line's checksum does not match its content".into());
    }
    let body: LineBody = serde_json::from_value(value).map_err(|error| error.to_string())?;
    if body.v != JOURNAL_LINE_VERSION {
        return Err(format!("unsupported journal line version {}", body.v));
    }
    Ok((body.seq, body.game, body.pgn))
}

/// Every game a run directory holds, as a resume or a reader finds it.
#[derive(Debug)]
pub struct LoadedJournal {
    /// The games, in the order they were committed.
    pub records: Vec<GameRecord>,
    /// Games the last checkpoint covered; the rest are the replayed tail.
    pub covered: usize,
    /// The journal state to keep appending from.
    pub resume: JournalResume,
    /// What resume had to discard: a torn last line, or the games of a sync
    /// window whose moves never reached `games.pgn`.
    pub discarded: Discarded,
}

/// Where appending continues after a load.
#[derive(Debug, Clone)]
pub struct JournalResume {
    pub offset: u64,
    pub hasher: Sha256,
    pub next_seq: u64,
    pub pgn_offset: u64,
}

impl JournalResume {
    #[must_use]
    pub fn fresh() -> Self {
        Self {
            offset: 0,
            hasher: Sha256::new(),
            next_seq: 1,
            pgn_offset: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Discarded {
    /// Bytes of an incomplete or unverifiable last journal line.
    pub torn_bytes: u64,
    /// Complete journal lines whose game's moves were not in `games.pgn`.
    pub games_without_moves: u64,
}

/// Whether a load may repair the files it read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadMode {
    /// Resume: truncate a torn tail and a PGN tail with no journal line, so
    /// appending continues from a consistent pair of files.
    Repair,
    /// Status and statistics: read what is there and change nothing.
    ReadOnly,
}

#[derive(Debug, Error)]
pub enum JournalError {
    #[error(
        "the journal does not match its checkpoint: the checkpoint covers {expected_records} games in {expected_offset} bytes hashing to {expected_sha256}, and the journal {found}. This is a refusal, not a repair: the run directory was changed after the checkpoint was written"
    )]
    AnchorMismatch {
        expected_offset: u64,
        expected_records: u64,
        expected_sha256: String,
        found: String,
    },
    #[error("journal line {line} is corrupt and is not the last line: {reason}")]
    Corrupt { line: u64, reason: String },
    #[error("journal line {line} has sequence {found}, expected {expected}")]
    Sequence {
        line: u64,
        expected: u64,
        found: u64,
    },
    #[error(
        "games.pgn is shorter than the checkpoint says was synced: {length} bytes, expected at least {expected}"
    )]
    PgnBehindCheckpoint { length: u64, expected: u64 },
    #[error("could not {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

/// Load the journal of a run directory, verified against the checkpoint that
/// covers it.
///
/// Everything the checkpoint covers must hash to what the checkpoint recorded;
/// a mismatch is refused, because it means the directory changed after the
/// checkpoint and nothing in it can be trusted to mean what it says. After the
/// covered prefix, each line is verified on its own. The first line that is
/// incomplete or fails its checksum ends the journal, provided nothing valid
/// follows it: that is a write the kill interrupted. A valid line after an
/// invalid one is corruption and is refused. Finally each tail game's moves
/// must be in `games.pgn` with the hash its line recorded; the first game
/// whose moves are missing ends the journal there, because those games were
/// inside the last sync window and the resume will play them again.
pub fn load_journal(
    root: &Path,
    anchor: Option<&JournalAnchor>,
    mode: LoadMode,
) -> Result<LoadedJournal, JournalError> {
    let journal_path = root.join(JOURNAL_FILE);
    let pgn_path = root.join(PGN_FILE);
    let bytes = read_all(&journal_path)?;
    let anchor = anchor.cloned().unwrap_or_else(JournalAnchor::empty);

    // The covered prefix: one hash, no per-line trust needed.
    let covered_end = usize::try_from(anchor.offset).unwrap_or(usize::MAX);
    if bytes.len() < covered_end {
        return Err(anchor_mismatch(
            &anchor,
            format!("holds only {} bytes", bytes.len()),
        ));
    }
    let mut hasher = Sha256::new();
    hasher.update(&bytes[..covered_end]);
    let prefix_sha256 = hex(&hasher.clone().finalize());
    if prefix_sha256 != anchor.sha256 {
        return Err(anchor_mismatch(
            &anchor,
            format!("hashes to {prefix_sha256} over those bytes"),
        ));
    }

    let mut records = Vec::new();
    let mut spans = Vec::new();
    let mut line_number = 0_u64;
    let mut expected_seq = 1_u64;
    for line in bytes[..covered_end].split_inclusive(|byte| *byte == b'\n') {
        line_number += 1;
        let content = line.strip_suffix(b"\n").ok_or_else(|| {
            anchor_mismatch(&anchor, "ends inside a line at the checkpoint".into())
        })?;
        let (seq, game, span) = decode_line(content).map_err(|reason| JournalError::Corrupt {
            line: line_number,
            reason,
        })?;
        if seq != expected_seq {
            return Err(JournalError::Sequence {
                line: line_number,
                expected: expected_seq,
                found: seq,
            });
        }
        expected_seq += 1;
        records.push(game);
        spans.push(span);
    }
    if records.len() as u64 != anchor.records {
        return Err(anchor_mismatch(
            &anchor,
            format!("holds {} games in those bytes", records.len()),
        ));
    }
    let covered = records.len();

    // The tail: each line on its own, a torn end tolerated.
    let mut valid_end = covered_end;
    let mut first_invalid: Option<(u64, String)> = None;
    let mut position = covered_end;
    for line in bytes[covered_end..].split_inclusive(|byte| *byte == b'\n') {
        line_number += 1;
        position += line.len();
        let decoded = match line.strip_suffix(b"\n") {
            Some(content) => decode_line(content),
            None => Err("the line was never finished".into()),
        };
        match (decoded, &first_invalid) {
            (Ok((seq, game, span)), None) => {
                if seq != expected_seq {
                    return Err(JournalError::Sequence {
                        line: line_number,
                        expected: expected_seq,
                        found: seq,
                    });
                }
                expected_seq += 1;
                hasher.update(line);
                records.push(game);
                spans.push(span);
                valid_end = position;
            }
            (Ok(_), Some((line, reason))) => {
                return Err(JournalError::Corrupt {
                    line: *line,
                    reason: reason.clone(),
                });
            }
            (Err(reason), None) => {
                first_invalid = Some((line_number, reason));
            }
            (Err(_), Some(_)) => {}
        }
    }
    let torn_bytes = (bytes.len() - valid_end) as u64;

    // The moves of every tail game must be where its line says they are.
    let pgn_length = file_length(&pgn_path)?;
    if pgn_length < anchor.pgn_offset {
        return Err(JournalError::PgnBehindCheckpoint {
            length: pgn_length,
            expected: anchor.pgn_offset,
        });
    }
    let mut kept = records.len();
    let mut pgn_end = anchor.pgn_offset;
    if records.len() > covered {
        let pgn = read_range(&pgn_path, anchor.pgn_offset, pgn_length)?;
        for (index, span) in spans.iter().enumerate().skip(covered) {
            let start = span.offset.checked_sub(anchor.pgn_offset);
            let present = start.and_then(|start| {
                let start = usize::try_from(start).ok()?;
                let end = start.checked_add(usize::try_from(span.length).ok()?)?;
                pgn.get(start..end)
            });
            match present {
                Some(moves) if hex(&Sha256::digest(moves)) == span.sha256 => {
                    pgn_end = span.offset + span.length;
                }
                _ => {
                    kept = index;
                    break;
                }
            }
        }
    }
    let games_without_moves = (records.len() - kept) as u64;
    if kept < records.len() {
        // Rehash the journal up to the last game whose moves are present.
        records.truncate(kept);
        let end = line_end_after(&bytes, kept);
        hasher = Sha256::new();
        hasher.update(&bytes[..end]);
        valid_end = end;
    }

    if mode == LoadMode::Repair {
        truncate_to(&journal_path, valid_end as u64, bytes.len() as u64)?;
        truncate_to(&pgn_path, pgn_end, pgn_length)?;
    }
    Ok(LoadedJournal {
        records,
        covered,
        resume: JournalResume {
            offset: valid_end as u64,
            hasher,
            next_seq: kept as u64 + 1,
            pgn_offset: pgn_end,
        },
        discarded: Discarded {
            torn_bytes,
            games_without_moves,
        },
    })
}

/// The byte offset just after the `count`th line.
fn line_end_after(bytes: &[u8], count: usize) -> usize {
    bytes
        .split_inclusive(|byte| *byte == b'\n')
        .take(count)
        .map(<[u8]>::len)
        .sum()
}

fn anchor_mismatch(anchor: &JournalAnchor, found: String) -> JournalError {
    JournalError::AnchorMismatch {
        expected_offset: anchor.offset,
        expected_records: anchor.records,
        expected_sha256: anchor.sha256.clone(),
        found,
    }
}

fn read_all(path: &Path) -> Result<Vec<u8>, JournalError> {
    match fs::read(path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(source) => Err(JournalError::Io {
            operation: "read the journal",
            path: path.to_owned(),
            source,
        }),
    }
}

fn file_length(path: &Path) -> Result<u64, JournalError> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(source) => Err(JournalError::Io {
            operation: "inspect games.pgn",
            path: path.to_owned(),
            source,
        }),
    }
}

fn read_range(path: &Path, from: u64, to: u64) -> Result<Vec<u8>, JournalError> {
    use std::io::{Seek, SeekFrom};
    if to <= from {
        return Ok(Vec::new());
    }
    let io = |operation, source| JournalError::Io {
        operation,
        path: path.to_owned(),
        source,
    };
    let mut file = File::open(path).map_err(|source| io("open games.pgn", source))?;
    file.seek(SeekFrom::Start(from))
        .map_err(|source| io("seek games.pgn", source))?;
    let mut bytes = Vec::with_capacity(usize::try_from(to - from).unwrap_or(0));
    file.take(to - from)
        .read_to_end(&mut bytes)
        .map_err(|source| io("read games.pgn", source))?;
    Ok(bytes)
}

fn truncate_to(path: &Path, length: u64, current: u64) -> Result<(), JournalError> {
    if length >= current {
        return Ok(());
    }
    let io = |operation, source| JournalError::Io {
        operation,
        path: path.to_owned(),
        source,
    };
    let file = OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|source| io("open for truncation", source))?;
    file.set_len(length)
        .and_then(|()| file.sync_all())
        .map_err(|source| io("truncate", source))
}

/// Lower-case hexadecimal.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}

/// The records of a journal on its own, for a reader that has no checkpoint
/// to verify it against: every complete line that verifies, stopping at the
/// first one that does not.
#[must_use]
pub fn read_journal_bytes(bytes: &[u8]) -> Vec<GameRecord> {
    let mut records = Vec::new();
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        let Some(content) = line.strip_suffix(b"\n") else {
            break;
        };
        match decode_line(content) {
            Ok((_, game, _)) => records.push(game),
            Err(_) => break,
        }
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn record(number: u32) -> GameRecord {
        GameRecord {
            number,
            pair_number: number.div_ceil(2),
            pair_game: if number % 2 == 1 { 1 } else { 2 },
            white: if number % 2 == 1 { "a" } else { "b" }.into(),
            black: if number % 2 == 1 { "b" } else { "a" }.into(),
            result: GameResult::Draw,
            scorable: true,
            termination: Termination::MaxMoves,
            opening: OpeningAssignment {
                book_index: Some(0),
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

    /// Append `games` games to a fresh directory the way the writer does, and
    /// return the anchor after the first `anchored` of them.
    fn write_games(root: &Path, games: u32, anchored: u32) -> JournalAnchor {
        let mut journal = File::create(root.join(JOURNAL_FILE)).unwrap();
        let mut pgn = File::create(root.join(PGN_FILE)).unwrap();
        let mut hasher = Sha256::new();
        let (mut offset, mut pgn_offset) = (0_u64, 0_u64);
        let mut anchor = JournalAnchor::empty();
        for number in 1..=games {
            let moves = format!("[Event \"{number}\"]\n\n1. e4 *\n\n");
            let span = PgnSpan {
                offset: pgn_offset,
                length: moves.len() as u64,
                sha256: hex(&Sha256::digest(moves.as_bytes())),
            };
            pgn.write_all(moves.as_bytes()).unwrap();
            pgn_offset += moves.len() as u64;
            let line = encode_line(u64::from(number), &record(number), &span);
            journal.write_all(&line).unwrap();
            hasher.update(&line);
            offset += line.len() as u64;
            if number == anchored {
                anchor = JournalAnchor {
                    offset,
                    sha256: hex(&hasher.clone().finalize()),
                    records: u64::from(number),
                    pgn_offset,
                };
            }
        }
        anchor
    }

    #[test]
    fn a_clean_journal_loads_every_game_and_replays_only_the_tail() {
        let root = tempfile::tempdir().unwrap();
        let anchor = write_games(root.path(), 10, 6);
        let loaded = load_journal(root.path(), Some(&anchor), LoadMode::ReadOnly).unwrap();
        assert_eq!(loaded.records.len(), 10);
        assert_eq!(loaded.covered, 6);
        assert_eq!(loaded.resume.next_seq, 11);
        assert_eq!(loaded.discarded, Discarded::default());
    }

    #[test]
    fn a_torn_last_line_is_dropped_and_truncated_on_repair() {
        let root = tempfile::tempdir().unwrap();
        let anchor = write_games(root.path(), 5, 3);
        let path = root.path().join(JOURNAL_FILE);
        let complete = fs::metadata(&path).unwrap().len();
        // A kill in the middle of writing the next line.
        OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"{\"v\":1,\"seq\":6,\"game\":{\"num")
            .unwrap();
        let loaded = load_journal(root.path(), Some(&anchor), LoadMode::Repair).unwrap();
        assert_eq!(loaded.records.len(), 5);
        assert!(loaded.discarded.torn_bytes > 0);
        assert_eq!(fs::metadata(&path).unwrap().len(), complete);
        assert_eq!(loaded.resume.offset, complete);
    }

    #[test]
    fn a_game_whose_moves_never_reached_the_pgn_is_played_again() {
        let root = tempfile::tempdir().unwrap();
        let anchor = write_games(root.path(), 8, 4);
        // The sync window: the journal kept games 5-8, the PGN only 5 and 6.
        let pgn = root.path().join(PGN_FILE);
        let text = fs::read_to_string(&pgn).unwrap();
        let cut = text.match_indices("[Event \"7\"]").next().unwrap().0;
        fs::write(&pgn, &text[..cut]).unwrap();
        let loaded = load_journal(root.path(), Some(&anchor), LoadMode::Repair).unwrap();
        assert_eq!(loaded.records.len(), 6);
        assert_eq!(loaded.discarded.games_without_moves, 2);
        assert_eq!(loaded.resume.next_seq, 7);
        // The journal now ends where the moves do, so the pair is consistent.
        let again = load_journal(root.path(), Some(&anchor), LoadMode::ReadOnly).unwrap();
        assert_eq!(again.records.len(), 6);
        assert_eq!(again.discarded, Discarded::default());
    }

    #[test]
    fn a_changed_covered_prefix_is_refused_not_repaired() {
        let root = tempfile::tempdir().unwrap();
        let anchor = write_games(root.path(), 6, 4);
        let path = root.path().join(JOURNAL_FILE);
        let mut bytes = fs::read(&path).unwrap();
        // Flip a result inside a covered line; its own checksum no longer
        // matters, because the prefix hash catches it first.
        let at = bytes.windows(4).position(|w| w == b"Draw").unwrap();
        bytes[at] = b'd';
        fs::write(&path, &bytes).unwrap();
        let error = load_journal(root.path(), Some(&anchor), LoadMode::Repair).unwrap_err();
        assert!(
            matches!(error, JournalError::AnchorMismatch { .. }),
            "{error}"
        );
        // A refusal changes nothing.
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn a_bad_line_followed_by_a_good_one_is_corruption() {
        let root = tempfile::tempdir().unwrap();
        let anchor = write_games(root.path(), 6, 2);
        let path = root.path().join(JOURNAL_FILE);
        let text = fs::read_to_string(&path).unwrap();
        let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
        lines[3] = lines[3].replace("Draw", "draw");
        fs::write(&path, lines.join("\n") + "\n").unwrap();
        let error = load_journal(root.path(), Some(&anchor), LoadMode::Repair).unwrap_err();
        assert!(
            matches!(error, JournalError::Corrupt { line: 4, .. }),
            "{error}"
        );
    }

    #[test]
    fn a_journal_shorter_than_its_checkpoint_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let anchor = write_games(root.path(), 6, 6);
        let path = root.path().join(JOURNAL_FILE);
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..bytes.len() - 10]).unwrap();
        assert!(matches!(
            load_journal(root.path(), Some(&anchor), LoadMode::Repair),
            Err(JournalError::AnchorMismatch { .. })
        ));
    }

    #[test]
    fn a_line_never_carries_moves() {
        let line = encode_line(
            1,
            &record(1),
            &PgnSpan {
                offset: 0,
                length: 12,
                sha256: "00".into(),
            },
        );
        let text = String::from_utf8(line).unwrap();
        assert!(!text.contains("1. e4"), "{text}");
        assert!(text.ends_with('\n'));
        assert_eq!(text.matches('\n').count(), 1);
    }
}
