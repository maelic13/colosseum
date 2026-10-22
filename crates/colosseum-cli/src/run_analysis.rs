//! What a run's games say beyond its statistics, read back from its run
//! directory: how the counted games ended, which faults were scored and how,
//! and a tournament's standings and its pairs.
//!
//! Everything here is computed from the files every run writes — the journal,
//! `games.pgn` and the resolved configuration — so a run's own end-of-run
//! summary, `stats` and `status` report the same figures, and a run directory
//! an earlier version left is read the same way.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use colosseum_application::{
    RateTournament, TournamentCompletedGame, TournamentFixedRating, TournamentPlan,
    TournamentStanding,
};
use colosseum_core::{
    Format, GameResult, PairGameResult, ParticipantId, PentanomialVector, ScoreElo, Termination,
    pentanomial_statistics, trinomial_elo,
};
use colosseum_engine::{GameFault, GameSide};
use serde::Serialize;
use serde_json::Value;

use crate::journal::GameRecord;

const Z95: f64 = 1.959_963_984_540_054;

/// The sample class of a game that counts.
const OFFICIAL: &str = "official";

/// The engines of a run, by the key its journal names them with — `a` and
/// `b` for a two-engine run, the participant identity for a tournament — in
/// the order a report lists them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participants {
    order: Vec<(String, String)>,
}

impl Participants {
    #[must_use]
    pub fn new(order: Vec<(String, String)>) -> Self {
        Self { order }
    }

    /// Engines A and B of a two-engine run.
    #[must_use]
    pub fn two_engines(names: [&str; 2]) -> Self {
        Self::new(vec![
            ("a".to_owned(), names[0].to_owned()),
            ("b".to_owned(), names[1].to_owned()),
        ])
    }

    /// The name a journal key stands for; an unknown key stands for itself.
    #[must_use]
    pub fn name(&self, key: &str) -> String {
        self.order
            .iter()
            .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
            .map_or_else(|| key.to_owned(), |(_, name)| name.clone())
    }

    #[must_use]
    pub fn key_of(&self, name: &str) -> Option<&str> {
        self.order
            .iter()
            .find(|(_, candidate)| candidate == name)
            .map(|(key, _)| key.as_str())
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.order.iter().map(|(_, name)| name.as_str())
    }
}

/// A game of the official sample that was scored.
fn counted(record: &GameRecord) -> bool {
    record.scorable && record.sample.eq_ignore_ascii_case(OFFICIAL)
}

fn termination_name(termination: Termination) -> String {
    format!("{termination:?}")
}

/// The counted games by how they ended, overall and for each engine.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TerminationReport {
    /// Every counted game, by termination.
    pub overall: BTreeMap<String, u32>,
    /// Each engine's counted games by its own outcome and termination: a
    /// decisive game is a win for one engine and a loss for the other.
    pub by_engine: Vec<EngineTerminations>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EngineTerminations {
    pub engine: String,
    pub wins: BTreeMap<String, u32>,
    pub draws: BTreeMap<String, u32>,
    pub losses: BTreeMap<String, u32>,
}

#[must_use]
pub fn terminations(records: &[GameRecord], participants: &Participants) -> TerminationReport {
    let mut report = TerminationReport {
        overall: BTreeMap::new(),
        by_engine: participants
            .names()
            .map(|engine| EngineTerminations {
                engine: engine.to_owned(),
                ..EngineTerminations::default()
            })
            .collect(),
    };
    for record in records.iter().filter(|record| counted(record)) {
        let kind = termination_name(record.termination);
        *report.overall.entry(kind.clone()).or_default() += 1;
        for (key, outcome) in [
            (&record.white, white_outcome(record.result)),
            (&record.black, invert(white_outcome(record.result))),
        ] {
            let name = participants.name(key);
            let Some(engine) = report
                .by_engine
                .iter_mut()
                .find(|engine| engine.engine == name)
            else {
                continue;
            };
            let bucket = match outcome {
                PairGameResult::Win => &mut engine.wins,
                PairGameResult::Draw => &mut engine.draws,
                PairGameResult::Loss => &mut engine.losses,
            };
            *bucket.entry(kind.clone()).or_default() += 1;
        }
    }
    report
}

/// Faulted games of the official sample, and the games lost on time.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FaultReport {
    /// Faulted games grouped by the engine at fault, how the game ended, the
    /// kind of fault, when it happened and how the game was scored.
    pub faults: Vec<FaultGroup>,
    /// Every engine's games lost on time, zeros included.
    pub time_losses: Vec<EngineCount>,
    /// Games the harness, not an engine, could not play.
    pub infrastructure_faults: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct FaultGroup {
    pub engine: String,
    pub termination: String,
    /// The fault as the runner classified it: `disconnect`, `crash`,
    /// `timeout`, `protocol` or `illegal-move`, or the operation the harness
    /// failed in.
    pub kind: String,
    /// `setup` when neither side made a search — the engine failed while it
    /// was started, configured or readied — and `play` otherwise.
    pub phase: &'static str,
    /// The faulted engine's result: `loss`, `draw` or `win` when the game was
    /// scored, `not scored` when it was not.
    pub scored_as: &'static str,
    pub games: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineCount {
    pub engine: String,
    pub games: u32,
}

#[must_use]
pub fn faults(records: &[GameRecord], participants: &Participants) -> FaultReport {
    let mut groups = BTreeMap::<FaultGroup, u32>::new();
    let mut time_losses = participants
        .names()
        .map(|engine| (engine.to_owned(), 0_u32))
        .collect::<Vec<_>>();
    let mut infrastructure_faults = 0;
    for record in records
        .iter()
        .filter(|record| record.sample.eq_ignore_ascii_case(OFFICIAL))
    {
        if record.scorable && record.termination == Termination::TimeForfeit {
            let loser = match record.result {
                GameResult::WhiteWin => Some(&record.black),
                GameResult::BlackWin => Some(&record.white),
                GameResult::Draw => None,
            };
            if let Some(loser) = loser {
                let name = participants.name(loser);
                if let Some(entry) = time_losses.iter_mut().find(|(engine, _)| *engine == name) {
                    entry.1 += 1;
                }
            }
        }
        let Some(fault) = &record.fault else {
            continue;
        };
        let phase = if record.clock.white_charged_elapsed.is_none()
            && record.clock.black_charged_elapsed.is_none()
        {
            "setup"
        } else {
            "play"
        };
        let (engine, kind, white) = match fault {
            GameFault::Engine { side, kind, .. } => {
                let white = matches!(side, GameSide::White);
                let key = if white { &record.white } else { &record.black };
                let kind = serde_json::to_value(kind)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| format!("{kind:?}"));
                (participants.name(key), kind, Some(white))
            }
            GameFault::Infrastructure { operation, .. } => {
                infrastructure_faults += 1;
                ("harness".to_owned(), operation.clone(), None)
            }
        };
        let scored_as = match (record.scorable, white) {
            (false, _) | (true, None) => "not scored",
            (true, Some(white)) => match (record.result, white) {
                (GameResult::Draw, _) => "draw",
                (GameResult::WhiteWin, true) | (GameResult::BlackWin, false) => "win",
                _ => "loss",
            },
        };
        *groups
            .entry(FaultGroup {
                engine,
                termination: termination_name(record.termination),
                kind,
                phase,
                scored_as,
                games: 0,
            })
            .or_default() += 1;
    }
    FaultReport {
        faults: groups
            .into_iter()
            .map(|(group, games)| FaultGroup { games, ..group })
            .collect(),
        time_losses: time_losses
            .into_iter()
            .map(|(engine, games)| EngineCount { engine, games })
            .collect(),
        infrastructure_faults,
    }
}

/// What a tournament needs to be rated again: its plan, anchor and pinned
/// ratings, as its resolved configuration keeps them.
#[derive(Debug, Clone)]
pub struct TournamentInputs {
    pub plan: TournamentPlan,
    pub anchor: Option<ParticipantId>,
    pub fixed_ratings: Vec<TournamentFixedRating>,
}

impl TournamentInputs {
    /// The inputs of the tournament in `root`; `None` when the directory is
    /// not a tournament's.
    pub fn read(root: &Path) -> Option<Result<Self, String>> {
        let config: Value =
            serde_json::from_slice(&fs::read(root.join("resolved-config.json")).ok()?).ok()?;
        if config.get("command").and_then(Value::as_str) != Some("tournament") {
            return None;
        }
        let field = |name: &str| config.get(name).cloned().unwrap_or(Value::Null);
        Some((|| -> Result<Self, String> {
            Ok(Self {
                plan: serde_json::from_value(field("plan")).map_err(|error| {
                    format!("the stored tournament plan is unreadable: {error}")
                })?,
                anchor: serde_json::from_value(field("anchor"))
                    .map_err(|error| format!("the stored anchor is unreadable: {error}"))?,
                fixed_ratings: serde_json::from_value(field("fixed_ratings")).unwrap_or_default(),
            })
        })())
    }

    /// Every participant by its journal key, in plan order.
    #[must_use]
    pub fn participants(&self) -> Participants {
        Participants::new(
            self.plan
                .participants
                .iter()
                .map(|participant| {
                    (
                        participant.participant.id.to_string(),
                        participant_name(&participant.participant.launch),
                    )
                })
                .collect(),
        )
    }

    /// The engines whose side a pair is reported from: a gauntlet's seeds,
    /// or every engine of a round robin.
    #[must_use]
    pub fn subjects(&self) -> Vec<String> {
        let names = self.participants();
        let seeds = match self.plan.design.format {
            Format::Gauntlet { seeds, .. } => seeds as usize,
            Format::RoundRobin { .. } => self.plan.participants.len(),
        };
        names.names().take(seeds).map(str::to_owned).collect()
    }
}

/// The name a participant is reported under: its label, or its executable's
/// file stem, as the rating step names it.
fn participant_name(launch: &colosseum_application::EngineLaunchSpec) -> String {
    launch.label.clone().unwrap_or_else(|| {
        launch
            .executable
            .file_stem()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("engine")
            .to_owned()
    })
}

/// An Elo estimate and its 95% interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct EloInterval {
    pub elo: f64,
    pub lower: f64,
    pub upper: f64,
}

/// One engine's games against one opponent, from that engine's side.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PairReport {
    pub engine: String,
    pub opponent: String,
    /// W/D/L, score and the trinomial Elo with its 95% interval.
    #[serde(flatten)]
    pub result: ScoreElo,
    /// Encounters played as two games of one opening with the colours
    /// reversed, and the games outside them.
    pub colour_pairs: u32,
    pub unpaired_games: u32,
    /// The colour pairs' pentanomial vector, absent without one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pentanomial: Option<[u32; 5]>,
    /// Its logistic Elo and 95% interval, absent when the sample is too small
    /// or degenerate to give one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pentanomial_elo: Option<EloInterval>,
    /// Scored games that ended on a fault.
    pub fault_scored_games: u32,
    /// The pair without those games, present when there were any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excluding_faults: Option<ScoreElo>,
}

/// A tournament's standings and pairs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TournamentAnalysis {
    /// `gauntlet` or `round-robin`.
    pub format: &'static str,
    /// The engines whose side the pairs are reported from: a gauntlet's
    /// seeds, every engine in a round robin.
    pub subjects: Vec<String>,
    /// Every participant, rated by the tournament's own joint recompute on
    /// the counted games; a pinned rating is marked fixed and has no
    /// interval.
    pub standings: Vec<TournamentStanding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub standings_unavailable: Option<String>,
    /// A gauntlet's pairs from each seed's side; a round robin's from both
    /// sides, the full matrix.
    pub pairs: Vec<PairReport>,
    /// Colour pairs and unpaired games over the tournament, each pair of
    /// engines counted once.
    pub colour_pairs: u32,
    pub unpaired_games: u32,
    /// Pairs of participants with a counted game between them. With more
    /// than one, the games are against several opponents.
    pub pairs_played: u32,
}

/// The participant a journal key names.
fn participant_id(key: &str) -> Option<ParticipantId> {
    serde_json::from_value(Value::String(key.to_owned())).ok()
}

#[must_use]
pub fn tournament_analysis(
    records: &[GameRecord],
    inputs: &TournamentInputs,
) -> TournamentAnalysis {
    let participants = inputs.participants();
    let official = records
        .iter()
        .filter(|record| record.sample.eq_ignore_ascii_case(OFFICIAL))
        .collect::<Vec<_>>();
    let evidence = official
        .iter()
        .filter_map(|record| {
            Some(TournamentCompletedGame {
                number: record.number,
                white: participant_id(&record.white)?,
                black: participant_id(&record.black)?,
                result: record.result,
                scorable: record.scorable,
                termination: record.termination,
            })
        })
        .collect::<Vec<_>>();
    let (standings, standings_unavailable) = match RateTournament::execute_with_fixed_field(
        &inputs.plan,
        &evidence,
        inputs.anchor,
        &inputs.fixed_ratings,
    ) {
        Ok(results) => (results.standings, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let subjects = inputs.subjects();
    let gauntlet = matches!(inputs.plan.design.format, Format::Gauntlet { .. });
    let keys = participants
        .order
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>();
    let mut pairs = Vec::new();
    let mut colour_pairs = 0;
    let mut unpaired_games = 0;
    let mut pairs_played = 0;
    for (first_index, first) in keys.iter().enumerate() {
        for second in &keys[first_index + 1..] {
            let games = official
                .iter()
                .copied()
                .filter(|record| {
                    (record.white.eq_ignore_ascii_case(first)
                        && record.black.eq_ignore_ascii_case(second))
                        || (record.white.eq_ignore_ascii_case(second)
                            && record.black.eq_ignore_ascii_case(first))
                })
                .collect::<Vec<_>>();
            if !games.iter().any(|record| record.scorable) {
                continue;
            }
            pairs_played += 1;
            let first_name = participants.name(first);
            let second_name = participants.name(second);
            let orientations = if gauntlet {
                if subjects.contains(&second_name) && !subjects.contains(&first_name) {
                    vec![(*second, *first)]
                } else {
                    vec![(*first, *second)]
                }
            } else {
                vec![(*first, *second), (*second, *first)]
            };
            for (index, (engine, opponent)) in orientations.into_iter().enumerate() {
                let report = pair_report(engine, opponent, &games, &participants);
                if index == 0 {
                    colour_pairs += report.colour_pairs;
                    unpaired_games += report.unpaired_games;
                }
                pairs.push(report);
            }
        }
    }
    if !gauntlet {
        // The matrix row by row: each engine's pairs together.
        pairs.sort_by_key(|pair| {
            (
                keys.iter()
                    .position(|key| participants.name(key) == pair.engine),
                keys.iter()
                    .position(|key| participants.name(key) == pair.opponent),
            )
        });
    }
    TournamentAnalysis {
        format: if gauntlet { "gauntlet" } else { "round-robin" },
        subjects,
        standings,
        standings_unavailable,
        pairs,
        colour_pairs,
        unpaired_games,
        pairs_played,
    }
}

/// `engine`'s outcome in `record`.
fn outcome_for(record: &GameRecord, engine: &str) -> PairGameResult {
    let white = white_outcome(record.result);
    if record.white.eq_ignore_ascii_case(engine) {
        white
    } else {
        invert(white)
    }
}

fn wdl(outcomes: impl Iterator<Item = PairGameResult>) -> ScoreElo {
    let (mut wins, mut draws, mut losses) = (0, 0, 0);
    for outcome in outcomes {
        match outcome {
            PairGameResult::Win => wins += 1,
            PairGameResult::Draw => draws += 1,
            PairGameResult::Loss => losses += 1,
        }
    }
    trinomial_elo(wins, draws, losses)
}

fn pair_report(
    engine: &str,
    opponent: &str,
    games: &[&GameRecord],
    participants: &Participants,
) -> PairReport {
    let scored = games
        .iter()
        .copied()
        .filter(|record| record.scorable)
        .collect::<Vec<_>>();
    let result = wdl(scored.iter().map(|record| outcome_for(record, engine)));
    let fault_scored_games = scored
        .iter()
        .filter(|record| record.fault.is_some())
        .count() as u32;
    let excluding_faults = (fault_scored_games > 0).then(|| {
        wdl(scored
            .iter()
            .filter(|record| record.fault.is_none())
            .map(|record| outcome_for(record, engine)))
    });
    // A colour pair is two consecutive assignments of one encounter, the
    // odd one first, on one opening.
    let mut units = BTreeMap::<(u32, u32), Vec<&GameRecord>>::new();
    for record in &scored {
        units
            .entry((record.pair_number, record.pair_game.div_ceil(2)))
            .or_default()
            .push(record);
    }
    let mut vector = PentanomialVector::default();
    let mut paired_games = 0;
    for unit in units.values_mut() {
        unit.sort_by_key(|record| record.pair_game);
        if unit.len() == 2
            && unit[0].pair_game % 2 == 1
            && unit[1].pair_game == unit[0].pair_game + 1
            && unit[0].opening == unit[1].opening
        {
            vector.record_pair(outcome_for(unit[0], engine), outcome_for(unit[1], engine));
            paired_games += 2;
        }
    }
    let colour_pairs = vector.pairs();
    let pentanomial_elo = (colour_pairs > 0)
        .then(|| pentanomial_statistics(&vector, Z95).ok())
        .flatten()
        .map(|statistics| EloInterval {
            elo: statistics.logistic_elo.elo,
            lower: statistics.logistic_elo.lower,
            upper: statistics.logistic_elo.upper,
        });
    PairReport {
        engine: participants.name(engine),
        opponent: participants.name(opponent),
        result,
        colour_pairs,
        unpaired_games: scored.len() as u32 - paired_games,
        pentanomial: (colour_pairs > 0).then(|| vector.counts()),
        pentanomial_elo,
        fault_scored_games,
        excluding_faults,
    }
}

fn white_outcome(result: GameResult) -> PairGameResult {
    match result {
        GameResult::WhiteWin => PairGameResult::Win,
        GameResult::BlackWin => PairGameResult::Loss,
        GameResult::Draw => PairGameResult::Draw,
    }
}

fn invert(result: PairGameResult) -> PairGameResult {
    match result {
        PairGameResult::Win => PairGameResult::Loss,
        PairGameResult::Draw => PairGameResult::Draw,
        PairGameResult::Loss => PairGameResult::Win,
    }
}

/// What a run directory's games say beyond its statistics.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DirectoryAnalysis {
    pub terminations: TerminationReport,
    pub faults: FaultReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tournament: Option<TournamentAnalysis>,
}

/// The analysis of the run in `root` from its journal: a tournament's by its
/// resolved configuration, a two-engine run's under `names`. `None` when the
/// directory has no journal.
pub fn analyse_directory(
    root: &Path,
    names: Option<[&str; 2]>,
) -> Option<Result<DirectoryAnalysis, String>> {
    let bytes = fs::read(root.join(crate::journal::JOURNAL_FILE)).ok()?;
    let records = crate::journal::read_journal_bytes(&bytes);
    Some(match TournamentInputs::read(root) {
        Some(Ok(inputs)) => {
            let participants = inputs.participants();
            Ok(DirectoryAnalysis {
                terminations: terminations(&records, &participants),
                faults: faults(&records, &participants),
                tournament: Some(tournament_analysis(&records, &inputs)),
            })
        }
        Some(Err(error)) => Err(error),
        None => {
            let participants = Participants::two_engines(names.unwrap_or(["engine A", "engine B"]));
            Ok(DirectoryAnalysis {
                terminations: terminations(&records, &participants),
                faults: faults(&records, &participants),
                tournament: None,
            })
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use colosseum_core::GameResult::{BlackWin, Draw, WhiteWin};
    use colosseum_engine::{ChargedElapsedSummary, EngineFaultKind};

    use crate::run_writer::tests::record;

    fn played(number: u32, white: &str, result: GameResult) -> GameRecord {
        let mut game = GameRecord {
            white: white.into(),
            black: if white == "a" { "b" } else { "a" }.into(),
            result,
            ..record(number)
        };
        let searched = Some(ChargedElapsedSummary {
            samples: 10,
            min_ns: 1,
            median_ns: 1,
            max_ns: 1,
        });
        game.clock.white_charged_elapsed = searched.clone();
        game.clock.black_charged_elapsed = searched;
        game
    }

    fn names() -> Participants {
        Participants::two_engines(["Alpha", "Beta"])
    }

    /// Terminations count the counted games only; each engine's decisive
    /// games are a win for one and a loss for the other.
    #[test]
    fn terminations_are_counted_overall_and_for_each_engine() {
        let mut stalemate = played(2, "b", Draw);
        stalemate.termination = Termination::Stalemate;
        let mut uncounted = played(3, "a", BlackWin);
        uncounted.sample = "post-terminal".into();
        let report = terminations(&[played(1, "a", WhiteWin), stalemate, uncounted], &names());
        assert_eq!(
            report.overall,
            BTreeMap::from([("Checkmate".to_owned(), 1), ("Stalemate".to_owned(), 1)])
        );
        assert_eq!(report.by_engine[0].wins.get("Checkmate"), Some(&1));
        assert_eq!(report.by_engine[1].losses.get("Checkmate"), Some(&1));
        assert_eq!(report.by_engine[1].draws.get("Stalemate"), Some(&1));
    }

    /// A crash before either side searched is a setup fault, and the game
    /// scored as the crashed engine's loss says so; a time loss is counted
    /// for the engine that lost it.
    #[test]
    fn faults_say_when_they_happened_and_how_they_were_scored() {
        let mut setup_crash = record(1);
        setup_crash.white = "b".into();
        setup_crash.black = "a".into();
        setup_crash.result = BlackWin;
        setup_crash.termination = Termination::EngineCrash;
        setup_crash.fault = Some(GameFault::Engine {
            side: GameSide::White,
            kind: EngineFaultKind::Disconnect,
            message: "terminated".into(),
        });
        let mut time_loss = played(2, "a", BlackWin);
        time_loss.termination = Termination::TimeForfeit;
        time_loss.fault = Some(GameFault::Engine {
            side: GameSide::White,
            kind: EngineFaultKind::Timeout,
            message: "flagged".into(),
        });
        let report = faults(&[setup_crash, time_loss, played(3, "a", Draw)], &names());
        assert_eq!(
            report.faults,
            [
                FaultGroup {
                    engine: "Alpha".into(),
                    termination: "TimeForfeit".into(),
                    kind: "timeout".into(),
                    phase: "play",
                    scored_as: "loss",
                    games: 1,
                },
                FaultGroup {
                    engine: "Beta".into(),
                    termination: "EngineCrash".into(),
                    kind: "disconnect".into(),
                    phase: "setup",
                    scored_as: "loss",
                    games: 1,
                },
            ]
        );
        assert_eq!(
            report.time_losses,
            [
                EngineCount {
                    engine: "Alpha".into(),
                    games: 1
                },
                EngineCount {
                    engine: "Beta".into(),
                    games: 0
                },
            ]
        );
    }

    /// One encounter of four games is two colour pairs; a fault-scored game
    /// is counted, and set aside in the figure without faults.
    #[test]
    fn a_pair_reports_its_trinomial_elo_its_pentanomial_and_its_result_without_faults() {
        let game = |number: u32, white: &str, result: GameResult| GameRecord {
            pair_number: 1,
            pair_game: number,
            ..played(number, white, result)
        };
        let mut faulted = game(4, "b", BlackWin);
        faulted.fault = Some(GameFault::Engine {
            side: GameSide::White,
            kind: EngineFaultKind::Crash,
            message: "crashed".into(),
        });
        let games = [
            game(1, "a", WhiteWin),
            game(2, "b", Draw),
            game(3, "a", Draw),
            faulted,
        ];
        let games = games.iter().collect::<Vec<_>>();
        let pair = pair_report("a", "b", &games, &names());
        assert_eq!(
            (pair.engine.as_str(), pair.opponent.as_str()),
            ("Alpha", "Beta")
        );
        assert_eq!(
            (pair.result.wins, pair.result.draws, pair.result.losses),
            (2, 2, 0)
        );
        assert_eq!(pair.result, trinomial_elo(2, 2, 0));
        assert_eq!((pair.colour_pairs, pair.unpaired_games), (2, 0));
        // 1.5 points, then 1.5 points: two pairs in the 3/4 bin.
        assert_eq!(pair.pentanomial, Some([0, 0, 0, 2, 0]));
        assert_eq!(pair.fault_scored_games, 1);
        assert_eq!(pair.excluding_faults, Some(trinomial_elo(1, 2, 0)));
    }
}
