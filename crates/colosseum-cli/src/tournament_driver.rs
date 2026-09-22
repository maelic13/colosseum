//! Headless, durable execution adapter for the shared tournament use case.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use colosseum_application::{
    RateTournament, TournamentCompletedGame, TournamentFixedRating, TournamentPlan,
    TournamentResults, TournamentScheduleGame,
};
use colosseum_core::{AdjudicationConfig, GameResult, ParticipantId, Termination};
use colosseum_engine::{ClockAccountingReport, GameFault, GamePairIdentity};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cancellation::Cancellation;
use crate::journal::GameRecord;
use crate::match_runner::{
    ConfiguredTimeControl, EngineProcesses, FaultPolicy, FixedMatchRequest, MatchExecutionPlan,
    MatchOpenings, MatchProgress, OpeningAssignment, run_fixed_match,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TournamentRunStatus {
    Completed,
    /// Stopped cleanly on request with games left unplayed. Standings cover
    /// the games actually played and the run directory resumes.
    Cancelled,
    Invalid,
    InfrastructureError,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TournamentGame {
    pub number: u32,
    pub encounter: u32,
    pub game_in_encounter: u32,
    pub round: u32,
    pub white: ParticipantId,
    pub black: ParticipantId,
    pub result: GameResult,
    pub scorable: bool,
    pub termination: Termination,
    pub clock_accounting: ClockAccountingReport,
    pub opening: OpeningAssignment,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault: Option<GameFault>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The game's moves, held only until the observer commits them to
    /// `games.pgn`; never stored in a checkpoint, a report or the driver.
    #[serde(skip)]
    pub pgn: String,
    /// The CPU slot the game ran on and when it held it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<crate::match_runner::SlotOccupancy>,
}

/// A participant identity as the journal writes it.
fn participant(text: &str) -> Option<ParticipantId> {
    serde_json::from_value(serde_json::Value::String(text.to_owned())).ok()
}

impl TournamentGame {
    /// The journal record of this game.
    #[must_use]
    pub fn journal_record(&self, sample: &str) -> GameRecord {
        GameRecord {
            number: self.number,
            pair_number: self.encounter,
            pair_game: self.game_in_encounter,
            white: self.white.to_string(),
            black: self.black.to_string(),
            result: self.result,
            scorable: self.scorable,
            termination: self.termination,
            opening: self.opening.clone(),
            fault: self.fault.clone(),
            sample: sample.to_owned(),
            clock: self.clock_accounting.clone(),
            iteration: None,
            round: Some(self.round),
            error: self.error.clone(),
            slot: self.slot,
        }
    }

    /// The game a journal record describes, without its moves.
    #[must_use]
    pub fn from_journal(record: &GameRecord) -> Option<Self> {
        Some(Self {
            number: record.number,
            encounter: record.pair_number,
            game_in_encounter: record.pair_game,
            round: record.round?,
            white: participant(&record.white)?,
            black: participant(&record.black)?,
            result: record.result,
            scorable: record.scorable,
            termination: record.termination,
            clock_accounting: record.clock.clone(),
            opening: record.opening.clone(),
            fault: record.fault.clone(),
            error: record.error.clone(),
            pgn: String::new(),
            slot: record.slot,
        })
    }

    /// The scoring evidence of this game, which is all the rating step reads.
    pub fn evidence(&self) -> TournamentCompletedGame {
        TournamentCompletedGame {
            number: self.number,
            white: self.white,
            black: self.black,
            result: self.result,
            scorable: self.scorable,
            termination: self.termination,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TournamentReport {
    pub status: TournamentRunStatus,
    pub plan: TournamentPlan,
    pub results: TournamentResults,
    pub time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub ponder: bool,
    pub execution: MatchExecutionPlan,
    pub master_seed: u64,
    pub master_seed_generated: bool,
    pub opening_policy: crate::match_runner::OpeningPolicyReport,
    pub engine_faults: u32,
    /// The engine faults the run tolerated before it would be invalid.
    pub max_engine_faults: u32,
    pub infrastructure_faults: u32,
    pub games: Vec<TournamentGame>,
}

pub trait TournamentObserver: Send + Sync {
    fn game_completed(&self, game: &TournamentGame) -> Result<(), String>;
}

#[derive(Clone)]
pub struct TournamentRunRequest {
    pub plan: TournamentPlan,
    pub anchor: Option<ParticipantId>,
    /// Participants pinned at supplied ratings; only the rest are estimated.
    pub fixed_ratings: Vec<TournamentFixedRating>,
    pub time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub ponder: bool,
    pub execution: MatchExecutionPlan,
    pub master_seed: u64,
    pub master_seed_generated: bool,
    pub openings: MatchOpenings,
    /// Invalidate after more engine faults than this; a time loss is one.
    pub max_engine_faults: u32,
    pub completed_games: Vec<TournamentGame>,
    pub cancellation: Cancellation,
    pub observer: Option<Arc<dyn TournamentObserver>>,
}

impl std::fmt::Debug for TournamentRunRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TournamentRunRequest")
            .field("plan", &self.plan)
            .field("completed_games", &self.completed_games.len())
            .field("observer", &self.observer.as_ref().map(|_| "configured"))
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Error)]
pub enum TournamentRunError {
    #[error("tournament execution needs at least one game slot")]
    NoExecutionSlots,
    #[error("tournament checkpoint has duplicate game {0}")]
    DuplicateCheckpointGame(u32),
    #[error("tournament checkpoint failed validation: {0}")]
    InvalidCheckpoint(String),
    #[error("tournament participant {0} is missing")]
    MissingParticipant(ParticipantId),
    #[error("tournament worker failed: {0}")]
    Worker(String),
    #[error("tournament game runner failed: {0}")]
    Game(String),
    #[error("durable tournament output failed: {0}")]
    Output(String),
    #[error("tournament result calculation failed: {0}")]
    Results(String),
}

/// The schedule identity a tournament game is written with.
///
/// The encounter is the pair, whatever its length, and its opening index is
/// the one the tournament chose from the whole book, not the single entry the
/// one-game match that plays it is handed.
fn encounter_identity(
    scheduled: &TournamentScheduleGame,
    opening: &OpeningAssignment,
) -> GamePairIdentity {
    GamePairIdentity {
        game_number: scheduled.number,
        pair_number: scheduled.encounter,
        pair_game: scheduled.game_in_encounter,
        opening_index: opening.book_index,
        opening_label: opening.label.clone(),
    }
}

pub async fn run_tournament(
    request: TournamentRunRequest,
) -> Result<TournamentReport, TournamentRunError> {
    if request.execution.slots.is_empty() {
        return Err(TournamentRunError::NoExecutionSlots);
    }
    let evidence = request
        .completed_games
        .iter()
        .map(TournamentGame::evidence)
        .collect::<Vec<_>>();
    RateTournament::execute_with_fixed_field(
        &request.plan,
        &evidence,
        request.anchor,
        &request.fixed_ratings,
    )
    .map_err(|error| TournamentRunError::InvalidCheckpoint(error.to_string()))?;
    let mut seen = BTreeSet::new();
    for game in &request.completed_games {
        if !seen.insert(game.number) {
            return Err(TournamentRunError::DuplicateCheckpointGame(game.number));
        }
    }
    let participants = request
        .plan
        .participants
        .iter()
        .map(|participant| {
            (
                participant.participant.id,
                participant.participant.launch.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut pending = request
        .plan
        .schedule
        .iter()
        .filter(|game| !seen.contains(&game.number))
        .cloned()
        .collect::<VecDeque<_>>();
    let mut games = request.completed_games;
    let mut workers = tokio::task::JoinSet::new();
    let mut infrastructure_error = false;
    let mut cancelled = false;
    let mut pool = request
        .execution
        .slot_pool()
        .map_err(|error| TournamentRunError::Game(error.to_string()))?;
    while (!pending.is_empty() && !request.cancellation.stopping()) || !workers.is_empty() {
        while !infrastructure_error
            && !request.cancellation.stopping()
            && !pending.is_empty()
            && workers.len() < request.execution.concurrency
        {
            let scheduled = pending.pop_front().expect("pending is not empty");
            let white = participants
                .get(&scheduled.white)
                .cloned()
                .ok_or(TournamentRunError::MissingParticipant(scheduled.white))?;
            let black = participants
                .get(&scheduled.black)
                .cloned()
                .ok_or(TournamentRunError::MissingParticipant(scheduled.black))?;
            let position = pool
                .take()
                .expect("a tournament keeps no more games live than it has slots");
            debug_assert_eq!(pool.held(), workers.len() + 1);
            let slot = request.execution.slots[position].clone();
            let execution = MatchExecutionPlan {
                concurrency: 1,
                allocation: request.execution.allocation,
                placement_policy: request.execution.placement_policy.clone(),
                slots: vec![slot],
                hash_memory: request.execution.hash_memory.clone(),
            };
            let (openings, opening) = request.openings.select_encounter(scheduled.encounter);
            let time_control = request.time_control;
            let adjudication = request.adjudication;
            let master_seed = request.master_seed;
            let game = async move {
                let report = run_fixed_match(FixedMatchRequest {
                    engine_a: white,
                    engine_b: black,
                    games: 1,
                    engine_a_time_control: time_control,
                    engine_b_time_control: time_control,
                    adjudication,
                    ponder: request.ponder,
                    // A tournament's slot changes engines from game to game.
                    engine_processes: EngineProcesses::PerGame,
                    fault_policy: FaultPolicy {
                        max_engine_faults: u32::MAX,
                        max_time_losses: u32::MAX,
                        rate: None,
                    },
                    execution,
                    master_seed,
                    master_seed_generated: false,
                    openings,
                    completed_games: Vec::new(),
                    progress: MatchProgress::default(),
                    // The outer schedule owns the stop; a single game either
                    // finishes or is abandoned with the rest.
                    cancellation: Cancellation::inactive(),
                    identity_override: Some(encounter_identity(&scheduled, &opening)),
                    observer: None,
                })
                .await
                .map_err(|error| error.to_string())?;
                let game = report
                    .games
                    .into_iter()
                    .next()
                    .ok_or_else(|| "one-game runner returned no game".to_owned())?;
                let pgn = game
                    .pgn
                    .replacen(
                        "[Event \"Colosseum CLI fixed match\"]",
                        "[Event \"Colosseum CLI tournament\"]",
                        1,
                    )
                    .replacen(
                        "[Round \"1\"]",
                        &format!("[Round \"{}\"]", scheduled.round),
                        1,
                    );
                Ok::<_, String>(TournamentGame {
                    number: scheduled.number,
                    encounter: scheduled.encounter,
                    game_in_encounter: scheduled.game_in_encounter,
                    round: scheduled.round,
                    white: scheduled.white,
                    black: scheduled.black,
                    result: game.result,
                    scorable: game.scorable,
                    termination: game.termination,
                    clock_accounting: game.clock_accounting,
                    opening,
                    fault: game.fault,
                    error: game.error,
                    pgn,
                    slot: game.slot,
                })
            };
            workers.spawn(async move { (position, game.await) });
        }
        let joined = tokio::select! {
            joined = workers.join_next() => joined,
            () = request.cancellation.abandon() => {
                workers.abort_all();
                cancelled = true;
                break;
            }
        };
        let Some(joined) = joined else {
            break;
        };
        let (position, game) =
            joined.map_err(|error| TournamentRunError::Worker(error.to_string()))?;
        // The one-game match returned: its engines have exited.
        pool.give_back(position);
        let game = game.map_err(TournamentRunError::Game)?;
        infrastructure_error |= matches!(game.fault, Some(GameFault::Infrastructure { .. }));
        if let Some(observer) = &request.observer {
            observer
                .game_completed(&game)
                .map_err(TournamentRunError::Output)?;
        }
        // Committed; the driver keeps the summary.
        let mut game = game;
        game.pgn = String::new();
        request.cancellation.record_committed_unit();
        games.push(game);
    }
    games.sort_by_key(|game| game.number);
    let engine_faults = games
        .iter()
        .filter(|game| matches!(game.fault, Some(GameFault::Engine { .. })))
        .count() as u32;
    let infrastructure_faults = games
        .iter()
        .filter(|game| matches!(game.fault, Some(GameFault::Infrastructure { .. })))
        .count() as u32;
    let status = if infrastructure_error || infrastructure_faults > 0 {
        TournamentRunStatus::InfrastructureError
    } else if engine_faults > request.max_engine_faults {
        TournamentRunStatus::Invalid
    } else if cancelled || request.cancellation.stopping() {
        TournamentRunStatus::Cancelled
    } else {
        TournamentRunStatus::Completed
    };
    let evidence = games
        .iter()
        .map(TournamentGame::evidence)
        .collect::<Vec<_>>();
    let results = RateTournament::execute_with_fixed_field(
        &request.plan,
        &evidence,
        request.anchor,
        &request.fixed_ratings,
    )
    .map_err(|error| TournamentRunError::Results(error.to_string()))?;
    Ok(TournamentReport {
        status,
        plan: request.plan,
        results,
        time_control: request.time_control,
        adjudication: request.adjudication,
        ponder: request.ponder,
        execution: request.execution,
        master_seed: request.master_seed,
        master_seed_generated: request.master_seed_generated,
        opening_policy: request.openings.report().clone(),
        engine_faults,
        max_engine_faults: request.max_engine_faults,
        infrastructure_faults,
        games,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use colosseum_application::{
        EngineLaunchSpec, PlanTournament, RuntimeParticipant, TournamentDesign,
        TournamentParticipant,
    };
    use colosseum_core::Format;
    use colosseum_engine::pgn::{PgnTags, build_pgn};

    use super::*;

    fn plan(games_per_pair: u32) -> TournamentPlan {
        let participants = (0..3)
            .map(|index| TournamentParticipant {
                participant: RuntimeParticipant {
                    id: ParticipantId::from_u128(index + 1),
                    launch: EngineLaunchSpec::path_only(format!("engine-{index}").into()),
                },
                initial_rating: 1_500.0,
            })
            .collect();
        PlanTournament::execute(
            participants,
            TournamentDesign {
                format: Format::RoundRobin { cycles: 1 },
                games_per_pair,
            },
        )
        .unwrap()
    }

    /// A tournament encounter is the pair, whatever its length: one game per
    /// encounter is no pentanomial unit at all, two is one unit per encounter
    /// and four is two. What the tournament writes has to say so, or a replay
    /// of its PGN would pair games from different encounters.
    #[test]
    fn a_tournament_pgn_names_each_encounter_as_its_pair_for_any_games_per_pair() {
        for (games_per_pair, expected_units) in [(1, 0), (2, 3), (4, 6)] {
            let plan = plan(games_per_pair);
            let mut pgn = String::new();
            let mut encounters = BTreeMap::<u32, Vec<u32>>::new();
            for scheduled in &plan.schedule {
                let opening = OpeningAssignment {
                    book_index: Some(scheduled.encounter as usize - 1),
                    label: format!("opening {}", scheduled.encounter),
                };
                let identity = encounter_identity(scheduled, &opening);
                assert_eq!(identity.game_number, scheduled.number);
                assert_eq!(identity.opening_index, opening.book_index);
                encounters
                    .entry(identity.pair_number)
                    .or_default()
                    .push(identity.pair_game);
                pgn.push_str(&build_pgn(
                    &PgnTags {
                        event: "Colosseum CLI tournament".into(),
                        site: "?".into(),
                        date: "2026.09.22".into(),
                        round: scheduled.round,
                        white: scheduled.white.to_string(),
                        black: scheduled.black.to_string(),
                        result: GameResult::WhiteWin,
                        time_control: String::new(),
                        termination: Some(Termination::Checkmate),
                        fen: None,
                        opening_plies: 0,
                        identity: Some(identity),
                        time_margins_ms: None,
                        slot: None,
                        forfeited_search: None,
                    },
                    &[],
                    &[],
                ));
                pgn.push('\n');
            }
            // Three encounters, each numbering its own games from one.
            let expected = (1..=games_per_pair).collect::<Vec<_>>();
            assert_eq!(encounters.len(), 3, "{games_per_pair} games per pair");
            assert!(encounters.values().all(|games| *games == expected));

            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("games.pgn");
            std::fs::write(&path, pgn).unwrap();
            let report = crate::stats_replay::replay(&path, None).unwrap();
            assert_eq!(report.games, 3 * games_per_pair);
            assert_eq!(
                report.complete_pairs, expected_units,
                "{games_per_pair} games per pair"
            );
            assert_eq!(
                report.unpaired_games,
                3 * games_per_pair - 2 * expected_units
            );
        }
    }
}
