use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use colosseum_core::{PairGameResult, PentanomialVector, pentanomial_statistics};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::pgn_telemetry::{SearchTelemetryReport, analyze_pgn, unavailable};

const Z95: f64 = 1.959_963_984_540_054;

/// The `ColosseumSample` class of a game a run recorded but cannot score.
///
/// The runner abandons a game on an infrastructure fault — an engine that
/// never spawned, an affinity call the operating system refused — and still
/// writes it, because the abandoned game is the evidence for the abort. It
/// carries a result only because the report and PGN shapes require one, and
/// every driver leaves it out of its own statistics. A replay that scored it
/// would report a larger sample than the run ever had.
pub const UNSCORABLE_SAMPLE: &str = "unscorable";

/// The `ColosseumSample` class of a game that counts.
pub const OFFICIAL_SAMPLE: &str = "official";

/// The `ColosseumSample` class of a pair an SPRT finished after its boundary.
pub const POST_TERMINAL_SAMPLE: &str = "post-terminal";

/// The `ColosseumSample` class of a game of an invalidated SPSA iteration.
pub const INVALID_SAMPLE: &str = "invalid";

#[derive(Debug, Clone, Serialize)]
pub struct ReplayAttempt {
    pub authority: &'static str,
    pub path: PathBuf,
    pub accepted: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsReplayReport {
    pub authority: &'static str,
    pub source: PathBuf,
    pub perspective: String,
    pub pairing: &'static str,
    pub games: u32,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pentanomial: Option<[u32; 5]>,
    pub complete_pairs: u32,
    pub unpaired_games: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired_statistics: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired_statistics_unavailable: Option<String>,
    /// Games this source recorded that its own run did not count.
    pub excluded_games: u32,
    /// Those games by the sample class that excluded them.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub excluded_by_sample: BTreeMap<String, u32>,
    pub attempts: Vec<ReplayAttempt>,
    pub warnings: Vec<String>,
    pub telemetry: SearchTelemetryReport,
    /// Where each game's wall time went, from a run's journal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_phases: Option<GamePhaseReport>,
}

/// The distribution over a run's games of each phase of a game's wall time,
/// in milliseconds. Every journalled game that reached its first search
/// counts, in or out of the official sample: this is about the harness, not
/// the result.
#[derive(Debug, Clone, Serialize)]
pub struct GamePhaseReport {
    pub games: u32,
    pub phases: Vec<PhaseDistribution>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PhaseDistribution {
    pub phase: &'static str,
    pub mean_ms: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

/// The phases, in the order a game passes through them. `outside-runner` is
/// the slot span less start-up, play and teardown: what the driver spends on
/// a game before the runner starts and after it returns.
const PHASES: [&str; 9] = [
    "startup",
    "play",
    "charged",
    "uncharged-play",
    "between-searches",
    "position-write",
    "after-bestmove",
    "teardown",
    "outside-runner",
];

fn game_phase_report(journal: &Path) -> Option<GamePhaseReport> {
    let bytes = fs::read(journal).ok()?;
    let mut columns: [Vec<f64>; PHASES.len()] = Default::default();
    let mut games = 0_u32;
    let ms = |ns: u64| ns as f64 / 1e6;
    for record in crate::journal::read_journal_bytes(&bytes) {
        let Some(phases) = record.clock.phases else {
            continue;
        };
        games += 1;
        let values = [
            phases.startup_ns,
            phases.play_ns,
            phases.charged_ns,
            phases.uncharged_play_ns,
            phases.between_searches_ns,
            phases.position_write_ns,
            phases.after_bestmove_ns,
            phases.teardown_ns,
        ];
        for (column, value) in columns.iter_mut().zip(values) {
            column.push(ms(value));
        }
        if let Some(slot) = &record.slot {
            let span_ns = slot
                .ended_unix_us
                .saturating_sub(slot.started_unix_us)
                .saturating_mul(1_000);
            let runner_ns = phases
                .startup_ns
                .saturating_add(phases.play_ns)
                .saturating_add(phases.teardown_ns);
            // Wall-clock microseconds against monotonic nanoseconds: a small
            // negative residue is resolution, not time, and reads as zero.
            columns[8].push(ms(span_ns.saturating_sub(runner_ns)));
        }
    }
    (games > 0).then(|| GamePhaseReport {
        games,
        phases: PHASES
            .iter()
            .zip(columns)
            .filter(|(_, values)| !values.is_empty())
            .map(|(phase, values)| distribution(phase, values))
            .collect(),
    })
}

fn distribution(phase: &'static str, mut values: Vec<f64>) -> PhaseDistribution {
    values.sort_by(f64::total_cmp);
    let quantile = |fraction: f64| {
        let rank = (fraction * values.len() as f64).ceil() as usize;
        values[rank.clamp(1, values.len()) - 1]
    };
    PhaseDistribution {
        phase,
        mean_ms: values.iter().sum::<f64>() / values.len() as f64,
        p50_ms: quantile(0.5),
        p90_ms: quantile(0.9),
        p99_ms: quantile(0.99),
        max_ms: values[values.len() - 1],
    }
}

/// Games a source recorded outside its own official sample, counted by the
/// class that excluded them.
///
/// A replay that silently dropped them would report a smaller sample than the
/// file holds with nothing to explain the difference.
#[derive(Debug, Clone, Default)]
struct ExcludedGames(BTreeMap<String, u32>);

impl ExcludedGames {
    fn record(&mut self, class: &str) {
        *self.0.entry(class.to_ascii_lowercase()).or_default() += 1;
    }

    fn total(&self) -> u32 {
        self.0.values().sum()
    }

    fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// "1 unscorable game", "2 unscorable and 6 post-terminal games".
    fn summary(&self) -> String {
        let parts = self
            .0
            .iter()
            .map(|(class, count)| format!("{count} {class}"))
            .collect::<Vec<_>>();
        let counted = match parts.split_last() {
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
            None => "0".to_owned(),
        };
        if self.total() == 1 {
            format!("{counted} game")
        } else {
            format!("{counted} games")
        }
    }
}

/// Where a game sits in the schedule: which pair, and which colour assignment
/// of that pair.
///
/// A pentanomial unit is two consecutive assignments of one pair, so an
/// encounter played with four games per pair holds two units. Deriving that
/// from the game number instead would silently pair games from different
/// encounters whenever a pair is not exactly two games long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PairSlot {
    pair_number: u32,
    pair_game: u32,
}

impl PairSlot {
    /// The pentanomial unit this game belongs to.
    fn unit(self) -> (u32, u32) {
        (self.pair_number, self.pair_game.div_ceil(2))
    }

    /// True for the assignment that played the pair's first engine as White.
    fn is_first_assignment(self) -> bool {
        self.pair_game % 2 == 1
    }
}

#[derive(Debug, Clone)]
struct RawGame {
    slot: Option<PairSlot>,
    opening: Option<String>,
    outcome: PairGameResult,
}

pub fn replay(path: &Path, subject: Option<&str>) -> Result<StatsReplayReport, String> {
    if path.is_dir() {
        let mut report = replay_directory(path, subject)?;
        report.game_phases = game_phase_report(&path.join(crate::journal::JOURNAL_FILE));
        Ok(report)
    } else if let Some(directory) = directory_of_named_journal(path) {
        replay(directory, subject)
    } else {
        let mut report = replay_file(path, subject)?;
        let journal = path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value.eq_ignore_ascii_case("jsonl"));
        if journal {
            report.game_phases = game_phase_report(path);
        }
        Ok(report)
    }
}

fn replay_directory(path: &Path, subject: Option<&str>) -> Result<StatsReplayReport, String> {
    // The final result, then the journal, which is the structured record of a
    // run still going or stopped. A checkpoint holds aggregates, not games, so
    // it is not a source of statistics.
    let candidates = [
        ("structured-run-store", path.join("result.json")),
        (
            "structured-run-store",
            path.join(crate::journal::JOURNAL_FILE),
        ),
        ("pgn-export", path.join("games.pgn")),
        ("forensic-log", path.join("run.log")),
        ("console", path.join("console.txt")),
    ];
    let mut attempts = Vec::new();
    for (authority, candidate) in candidates {
        if !candidate.is_file() {
            attempts.push(ReplayAttempt {
                authority,
                path: candidate,
                accepted: false,
                detail: "not present".into(),
            });
            continue;
        }
        match read_source(authority, &candidate, subject) {
            Ok(mut source) if !source.games.is_empty() => {
                attempts.push(ReplayAttempt {
                    authority,
                    path: candidate.clone(),
                    accepted: true,
                    detail: format!("replayed {} scored games", source.games.len()),
                });
                source.telemetry = directory_telemetry(path, source.telemetry);
                return Ok(build_report(authority, candidate, source, attempts));
            }
            Ok(source) => attempts.push(ReplayAttempt {
                authority,
                path: candidate,
                accepted: false,
                detail: empty_detail(&source.excluded),
            }),
            Err(error) => attempts.push(ReplayAttempt {
                authority,
                path: candidate,
                accepted: false,
                detail: error,
            }),
        }
    }
    Err(format!(
        "no replayable source found in {}; attempted structured store, PGN, forensic log and console",
        path.display()
    ))
}

/// Say why a source yielded nothing to score.
///
/// "No games" and "games the run itself did not count" are different facts,
/// and a reader looking at a file full of games deserves the second one.
fn empty_detail(excluded: &ExcludedGames) -> String {
    if excluded.is_empty() {
        "contains no scored games".to_owned()
    } else {
        format!(
            "contains {} and nothing that belongs to the official sample",
            excluded.summary()
        )
    }
}

/// A run directory is one evidence set.
///
/// Statistics keep structured authority, but the annotations live in the
/// directory's own `games.pgn`, so reading the journal is no reason to report
/// telemetry as unavailable when the PGN beside it carries the moves.
fn directory_telemetry(
    directory: &Path,
    from_source: SearchTelemetryReport,
) -> SearchTelemetryReport {
    if from_source.status == "available" {
        return from_source;
    }
    let pgn = directory.join("games.pgn");
    let Ok(text) = fs::read_to_string(&pgn) else {
        return from_source;
    };
    let from_pgn = analyze_pgn(&text);
    if from_pgn.status == "available" {
        from_pgn
    } else {
        from_source
    }
}

/// The run directory whose journal a result names in place of its games, as
/// a tune's `result.json` does.
fn directory_of_named_journal(path: &Path) -> Option<&Path> {
    let document: serde_json::Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let directory = path.parent()?;
    (document.get("games")?.as_str()? == crate::journal::JOURNAL_FILE
        && directory.join(crate::journal::JOURNAL_FILE).is_file())
    .then_some(directory)
}

fn replay_file(path: &Path, subject: Option<&str>) -> Result<StatsReplayReport, String> {
    let authority = match path.extension().and_then(|value| value.to_str()) {
        Some(value) if value.eq_ignore_ascii_case("json") => "structured-run-store",
        Some(value) if value.eq_ignore_ascii_case("jsonl") => "structured-run-store",
        Some(value) if value.eq_ignore_ascii_case("pgn") => "pgn-export",
        Some(value) if value.eq_ignore_ascii_case("log") => "forensic-log",
        _ => "console",
    };
    let source = read_source(authority, path, subject)?;
    if source.games.is_empty() {
        return Err(format!(
            "{} {}",
            path.display(),
            empty_detail(&source.excluded)
        ));
    }
    let attempts = vec![ReplayAttempt {
        authority,
        path: path.to_owned(),
        accepted: true,
        detail: format!("replayed {} scored games", source.games.len()),
    }];
    Ok(build_report(authority, path.to_owned(), source, attempts))
}

/// What one evidence source yielded.
struct SourceGames {
    games: Vec<RawGame>,
    perspective: String,
    paired_capable: bool,
    /// Games the source recorded but did not count towards its official
    /// sample, so a reader can see they were left out rather than lost.
    excluded: ExcludedGames,
    telemetry: SearchTelemetryReport,
}

fn read_source(
    authority: &'static str,
    path: &Path,
    subject: Option<&str>,
) -> Result<SourceGames, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let journal = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("jsonl"));
    if journal && authority == "structured-run-store" {
        let (games, excluded) = journal_games(text.as_bytes());
        return Ok(SourceGames {
            games,
            perspective: "engine A".into(),
            paired_capable: true,
            excluded,
            telemetry: unavailable("the journal holds no PGN move annotations"),
        });
    }
    match authority {
        "structured-run-store" => structured_games(&text).map(|(games, excluded)| SourceGames {
            games,
            perspective: "engine A".into(),
            paired_capable: true,
            excluded,
            telemetry: unavailable("structured source has no PGN move annotations"),
        }),
        "pgn-export" => {
            let (games, paired_capable, excluded) = pgn_games(&text, subject);
            let perspective = match (subject, paired_capable) {
                (Some(value), _) => value.to_owned(),
                (None, true) => "first engine of each pair".into(),
                (None, false) => "White side".into(),
            };
            Ok(SourceGames {
                games,
                perspective,
                paired_capable,
                excluded,
                telemetry: analyze_pgn(&text),
            })
        }
        "forensic-log" => Ok(SourceGames {
            games: log_games(&text),
            perspective: "engine A".into(),
            paired_capable: true,
            excluded: ExcludedGames::default(),
            telemetry: unavailable("forensic log has no PGN move annotations"),
        }),
        _ => Ok(SourceGames {
            games: result_tokens(&text),
            perspective: "White side (console tokens)".into(),
            paired_capable: false,
            excluded: ExcludedGames::default(),
            telemetry: unavailable("console source has no PGN move annotations"),
        }),
    }
}

fn structured_games(text: &str) -> Result<(Vec<RawGame>, ExcludedGames), String> {
    let mut value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if value.get("payload").is_some() {
        let payload = value
            .get("payload")
            .cloned()
            .ok_or_else(|| "checkpoint envelope has no payload".to_owned())?;
        let expected = value
            .get("payload_sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| "checkpoint envelope has no payload checksum".to_owned())?;
        let actual = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&payload).expect("JSON checkpoint payload is serializable")
            )
        );
        if actual != expected {
            return Err("checkpoint payload checksum mismatch".into());
        }
        value = payload;
    }
    let mut candidates = Vec::<(Vec<RawGame>, ExcludedGames)>::new();
    collect_structured_candidates(&value, &mut candidates);
    candidates
        .into_iter()
        .max_by_key(|(games, _)| games.len())
        .ok_or_else(|| "structured document contains no game array".into())
}

fn collect_structured_candidates(value: &Value, output: &mut Vec<(Vec<RawGame>, ExcludedGames)>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "official_pairs"
                    && let Some(pairs) = child.as_array()
                {
                    let candidate = structured_candidate(
                        pairs
                            .iter()
                            .flat_map(|pair| [pair.get("first"), pair.get("second")])
                            .flatten(),
                    );
                    if !candidate.0.is_empty() {
                        output.push(candidate);
                    }
                }
                if key == "games"
                    && let Some(games) = child.as_array()
                {
                    let candidate = structured_candidate(games.iter());
                    if !candidate.0.is_empty() {
                        output.push(candidate);
                    }
                }
                collect_structured_candidates(child, output);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_structured_candidates(child, output);
            }
        }
        _ => {}
    }
}

/// Read one array of structured games, counting what it cannot score.
///
/// A driver records the game it abandoned on an infrastructure fault and then
/// leaves it out of its own statistics. The replay does the same, and says how
/// many it left out so the count agrees with the run's own PGN.
fn structured_candidate<'a>(
    values: impl Iterator<Item = &'a Value>,
) -> (Vec<RawGame>, ExcludedGames) {
    let mut games = Vec::new();
    let mut excluded = ExcludedGames::default();
    for value in values {
        if value.get("scorable").and_then(Value::as_bool) == Some(false) {
            excluded.record(UNSCORABLE_SAMPLE);
            continue;
        }
        if let Some(game) = parse_structured_game(value) {
            games.push(game);
        }
    }
    (games, excluded)
}

fn parse_structured_game(value: &Value) -> Option<RawGame> {
    // The forensic log reads games one event at a time and has no array to
    // count exclusions against, so the guard stays here as well.
    if value.get("scorable").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let result = value.get("result")?.as_str()?;
    let white = value.get("white").and_then(Value::as_str);
    let white_score = match result {
        "WhiteWin" | "white-win" | "1-0" => PairGameResult::Win,
        "BlackWin" | "black-win" | "0-1" => PairGameResult::Loss,
        "Draw" | "draw" | "1/2-1/2" => PairGameResult::Draw,
        _ => return None,
    };
    let slot = structured_slot(value);
    let outcome = match white {
        // A match or an SPRT names the two sides of its own pair, so the
        // record says outright which of them had White.
        Some(side) if side.eq_ignore_ascii_case("a") => white_score,
        Some(side) if side.eq_ignore_ascii_case("b") => invert(white_score),
        // A tournament names participants by identity instead, and its
        // encounter alternates colours: the odd assignment gave White to the
        // engine the pair is scored from. Taking every game from White's side
        // would score a pair as its two colours rather than as one contest.
        _ => match slot {
            Some(slot) if !slot.is_first_assignment() => invert(white_score),
            _ => white_score,
        },
    };
    Some(RawGame {
        slot,
        opening: value
            .get("opening")
            .map(|opening| serde_json::to_string(opening).unwrap_or_default()),
        outcome,
    })
}

/// Read the pair slot a structured game records.
///
/// A tournament game names its encounter and its position inside it. A match
/// or SPRT game names only its number, and there the pair is the two
/// consecutive games that share an opening, odd first.
fn structured_slot(value: &Value) -> Option<PairSlot> {
    let field = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
    };
    // A journal record names its pair directly; a tournament result names its
    // encounter; a match result names only the game number.
    for (pair, game) in [
        ("pair_number", "pair_game"),
        ("encounter", "game_in_encounter"),
    ] {
        if let (Some(pair_number), Some(pair_game)) = (field(pair), field(game)) {
            return Some(PairSlot {
                pair_number,
                pair_game,
            });
        }
    }
    let number = field("number").filter(|number| *number > 0)?;
    Some(PairSlot {
        pair_number: number.div_ceil(2),
        pair_game: if number % 2 == 1 { 1 } else { 2 },
    })
}

/// The games of a run's journal, `games.jsonl`: every verified line, with the
/// games the run did not count set aside by the class it wrote them with.
///
/// A game nobody could score keeps its pair's or iteration's class in the
/// journal, marked `scorable: false`; it is set aside as `unscorable`, which
/// is how the run's own PGN tags it, so the two sources agree.
fn journal_games(bytes: &[u8]) -> (Vec<RawGame>, ExcludedGames) {
    let mut games = Vec::new();
    let mut excluded = ExcludedGames::default();
    for record in crate::journal::read_journal_bytes(bytes) {
        if !record.scorable {
            excluded.record(UNSCORABLE_SAMPLE);
            continue;
        }
        if !record.sample.eq_ignore_ascii_case(OFFICIAL_SAMPLE) {
            excluded.record(&record.sample);
            continue;
        }
        let value = serde_json::to_value(&record).expect("a journal record is serializable");
        if let Some(game) = parse_structured_game(&value) {
            games.push(game);
        }
    }
    (games, excluded)
}

fn log_games(text: &str) -> Vec<RawGame> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value.get("event").and_then(Value::as_str) == Some("game-completed"))
        .filter_map(|value| parse_structured_game(&value["game"]))
        .collect()
}

/// Read games from a PGN, using the schedule identity tags when they are there.
///
/// A Colosseum export names the pair each game belongs to and which colour
/// assignment it is, so the pentanomial unit survives the round trip. Without
/// a subject the outcome is taken from the pair's first engine — the one that
/// had White in assignment 1 — which is the perspective the journal uses,
/// so the two sources agree. A PGN without the tags yields no identity and the
/// caller falls back to labelled unpaired statistics.
fn pgn_games(text: &str, subject: Option<&str>) -> (Vec<RawGame>, bool, ExcludedGames) {
    let mut paired_capable = true;
    let mut excluded = ExcludedGames::default();
    let games = split_pgn(text)
        .into_iter()
        .filter_map(|game| {
            // A run's own export keeps the games it played but did not count:
            // a game abandoned on an infrastructure fault, the pairs an SPRT
            // finished after its boundary, and the games of an invalidated
            // SPSA iteration. The official sample excludes them exactly as the
            // journal does.
            if let Some(class) = pgn_tag(game, "ColosseumSample")
                .filter(|class| !class.eq_ignore_ascii_case(OFFICIAL_SAMPLE))
            {
                excluded.record(&class);
                return None;
            }
            let result = pgn_tag(game, "Result")?;
            let white = pgn_tag(game, "White").unwrap_or_default();
            let black = pgn_tag(game, "Black").unwrap_or_default();
            let white_score = token_result(&result)?;
            let identity = pgn_identity(game);
            if identity.is_none() {
                paired_capable = false;
            }
            let outcome = match subject {
                Some(name) if white == name => white_score,
                Some(name) if black == name => invert(white_score),
                Some(_) => return None,
                // An even assignment is the same opening with the colours
                // reversed, so its White is the pair's second engine.
                None => match identity.as_ref() {
                    Some(identity) if !identity.slot.is_first_assignment() => invert(white_score),
                    _ => white_score,
                },
            };
            Some(RawGame {
                slot: identity.as_ref().map(|identity| identity.slot),
                opening: identity.as_ref().map(|identity| identity.opening.clone()),
                outcome,
            })
        })
        .collect::<Vec<_>>();
    let paired_capable = paired_capable && !games.is_empty();
    (games, paired_capable, excluded)
}

/// The schedule identity a Colosseum export carries.
#[derive(Debug, Clone)]
struct PgnIdentity {
    slot: PairSlot,
    opening: String,
}

fn pgn_identity(game: &str) -> Option<PgnIdentity> {
    let pair_number: u32 = pgn_tag(game, "PairNumber")?.parse().ok()?;
    let pair_game: u32 = pgn_tag(game, "PairGame")?.parse().ok()?;
    if pair_number == 0 || pair_game == 0 {
        return None;
    }
    // Both games of a pair must agree on their opening for the pair to be a
    // pair at all, so the identity carries whichever the export named.
    let opening = pgn_tag(game, "OpeningIndex")
        .or_else(|| pgn_tag(game, "OpeningLabel"))
        .unwrap_or_else(|| format!("pair {pair_number}"));
    Some(PgnIdentity {
        slot: PairSlot {
            pair_number,
            pair_game,
        },
        opening,
    })
}

fn split_pgn(text: &str) -> Vec<&str> {
    let starts = text
        .match_indices("[Event ")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if starts.is_empty() {
        return Vec::new();
    }
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| &text[*start..starts.get(index + 1).copied().unwrap_or(text.len())])
        .collect()
}

fn pgn_tag(game: &str, name: &str) -> Option<String> {
    let prefix = format!("[{name} \"");
    game.lines().find_map(|line| {
        line.strip_prefix(&prefix)?
            .strip_suffix("\"]")
            .map(str::to_owned)
    })
}

fn result_tokens(text: &str) -> Vec<RawGame> {
    text.split_whitespace()
        .filter_map(token_result)
        .map(|outcome| RawGame {
            slot: None,
            opening: None,
            outcome,
        })
        .collect()
}

fn token_result(token: &str) -> Option<PairGameResult> {
    match token.trim_matches(|character: char| character == '"' || character == ',') {
        "1-0" => Some(PairGameResult::Win),
        "0-1" => Some(PairGameResult::Loss),
        "1/2-1/2" => Some(PairGameResult::Draw),
        _ => None,
    }
}

fn invert(result: PairGameResult) -> PairGameResult {
    match result {
        PairGameResult::Win => PairGameResult::Loss,
        PairGameResult::Draw => PairGameResult::Draw,
        PairGameResult::Loss => PairGameResult::Win,
    }
}

fn build_report(
    authority: &'static str,
    path: PathBuf,
    source: SourceGames,
    attempts: Vec<ReplayAttempt>,
) -> StatsReplayReport {
    let SourceGames {
        games,
        perspective,
        paired_capable,
        excluded,
        telemetry,
    } = source;
    let mut wins = 0;
    let mut draws = 0;
    let mut losses = 0;
    for game in &games {
        match game.outcome {
            PairGameResult::Win => wins += 1,
            PairGameResult::Draw => draws += 1,
            PairGameResult::Loss => losses += 1,
        }
    }
    let mut sample = PentanomialVector::default();
    let mut paired_games = 0;
    if paired_capable {
        let mut by_unit = BTreeMap::<(u32, u32), Vec<&RawGame>>::new();
        for game in &games {
            if let Some(slot) = game.slot {
                by_unit.entry(slot.unit()).or_default().push(game);
            }
        }
        for unit in by_unit.values_mut() {
            unit.sort_by_key(|game| game.slot.map(|slot| slot.pair_game));
            let assignments = unit
                .iter()
                .filter_map(|game| game.slot.map(|slot| slot.pair_game))
                .collect::<Vec<_>>();
            if unit.len() == 2
                && unit[0].slot.is_some_and(PairSlot::is_first_assignment)
                && assignments[1] == assignments[0] + 1
                && unit[0].opening == unit[1].opening
            {
                sample.record_pair(unit[0].outcome, unit[1].outcome);
                paired_games += 2;
            }
        }
    }
    for _ in paired_games..games.len() {
        sample.record_unpaired_game();
    }
    let paired_statistics = pentanomial_statistics(&sample, Z95).ok().map(|stats| {
        json!({
            "score": stats.score,
            "variance": stats.variance,
            "standard_error": stats.standard_error,
                "logistic_elo": {
                    "elo": stats.logistic_elo.elo,
                    "score": stats.logistic_elo.score,
                    "lower": stats.logistic_elo.lower,
                    "upper": stats.logistic_elo.upper,
                },
                "normalized_elo": {
                    "elo": stats.normalized_elo.elo,
                    "lower": stats.normalized_elo.lower,
                    "upper": stats.normalized_elo.upper,
                },
            "los": stats.los,
            "draw_ratio": stats.draw_ratio,
            "pairs_ratio": stats.pairs_ratio,
            "win_loss_to_double_draw_ratio": stats.win_loss_to_double_draw_ratio,
        })
    });
    let unavailable = (sample.pairs() > 0 && paired_statistics.is_none()).then(|| {
        pentanomial_statistics(&sample, Z95)
            .expect_err("statistics were unavailable")
            .to_string()
    });
    let games_count = games.len() as u32;
    let mut warnings = Vec::new();
    if !excluded.is_empty() {
        let (verb, pronoun) = if excluded.total() == 1 {
            ("was", "it")
        } else {
            ("were", "them")
        };
        warnings.push(format!(
            "{} {verb} excluded from the official sample; the run recorded {pronoun} without scoring {pronoun}",
            excluded.summary()
        ));
    }
    if sample.unpaired_games() > 0 {
        warnings.push(
            "pair/opening identity is absent or incomplete for some games; unpaired W/D/L is reported without invented pentanomial statistics"
                .into(),
        );
    }
    if telemetry.status == "available" {
        warnings.push(telemetry.node_semantics_warning.into());
    }
    StatsReplayReport {
        authority,
        source: path,
        perspective,
        pairing: if sample.pairs() > 0 {
            "paired"
        } else {
            "unpaired"
        },
        games: games_count,
        wins,
        draws,
        losses,
        score: (f64::from(wins) + 0.5 * f64::from(draws)) / f64::from(games_count),
        pentanomial: (sample.pairs() > 0).then(|| sample.counts()),
        complete_pairs: sample.pairs(),
        unpaired_games: sample.unpaired_games(),
        paired_statistics,
        paired_statistics_unavailable: unavailable,
        excluded_games: excluded.total(),
        excluded_by_sample: excluded.0,
        attempts,
        warnings,
        telemetry,
        game_phases: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pgn_without_pair_identity_is_never_guessed_into_pairs() {
        let pgn = "[Event \"x\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n1-0\n\n[Event \"x\"]\n[White \"B\"]\n[Black \"A\"]\n[Result \"0-1\"]\n\n0-1\n";
        let (games, paired_capable, excluded) = pgn_games(pgn, Some("A"));
        assert!(excluded.is_empty());
        assert!(
            !paired_capable,
            "a PGN without identity tags must not claim pairs"
        );
        let report = build_report(
            "pgn-export",
            "x.pgn".into(),
            SourceGames {
                games,
                perspective: "A".into(),
                paired_capable,
                excluded,
                telemetry: unavailable("fixture has no annotations"),
            },
            vec![],
        );
        assert_eq!(report.wins, 2);
        assert_eq!(report.complete_pairs, 0);
        assert_eq!(report.unpaired_games, 2);
        assert_eq!(report.pairing, "unpaired");
    }
}
