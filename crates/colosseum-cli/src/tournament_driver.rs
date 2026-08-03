//! Headless, durable execution adapter for the shared tournament use case.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use colosseum_application::{
    RateTournament, TournamentCompletedGame, TournamentPlan, TournamentResults,
};
use colosseum_core::{AdjudicationConfig, GameResult, ParticipantId, Termination};
use colosseum_engine::{ClockAccountingReport, GameFault};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::match_runner::{
    ConfiguredTimeControl, FaultPolicy, FixedMatchRequest, MatchExecutionPlan, MatchOpenings,
    MatchProgress, OpeningAssignment, run_fixed_match,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TournamentRunStatus {
    Completed,
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
    pub pgn: String,
}

impl TournamentGame {
    fn evidence(&self) -> TournamentCompletedGame {
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TournamentCheckpoint {
    pub games: Vec<TournamentGame>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TournamentReport {
    pub status: TournamentRunStatus,
    pub plan: TournamentPlan,
    pub results: TournamentResults,
    pub time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub execution: MatchExecutionPlan,
    pub master_seed: u64,
    pub master_seed_generated: bool,
    pub opening_policy: crate::match_runner::OpeningPolicyReport,
    pub engine_faults: u32,
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
    pub time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub execution: MatchExecutionPlan,
    pub master_seed: u64,
    pub master_seed_generated: bool,
    pub openings: MatchOpenings,
    pub max_engine_faults: Option<u32>,
    pub completed_games: Vec<TournamentGame>,
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
    RateTournament::execute(&request.plan, &evidence, request.anchor)
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
    while !pending.is_empty() || !workers.is_empty() {
        while !infrastructure_error
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
            let slot = request.execution.slots
                [(scheduled.number as usize - 1) % request.execution.slots.len()]
            .clone();
            let execution = MatchExecutionPlan {
                concurrency: 1,
                cores_per_engine: request.execution.cores_per_engine,
                placement_policy: request.execution.placement_policy.clone(),
                slots: vec![slot],
                hash_memory: request.execution.hash_memory.clone(),
            };
            let (openings, opening) = request.openings.select_encounter(scheduled.encounter);
            let time_control = request.time_control;
            let adjudication = request.adjudication;
            let master_seed = request.master_seed;
            workers.spawn(async move {
                let report = run_fixed_match(FixedMatchRequest {
                    engine_a: white,
                    engine_b: black,
                    games: 1,
                    engine_a_time_control: time_control,
                    engine_b_time_control: time_control,
                    adjudication,
                    fault_policy: FaultPolicy {
                        max_engine_faults: u32::MAX,
                        max_time_losses: u32::MAX,
                    },
                    execution,
                    master_seed,
                    master_seed_generated: false,
                    openings,
                    completed_games: Vec::new(),
                    progress: MatchProgress::default(),
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
                })
            });
        }
        let Some(joined) = workers.join_next().await else {
            break;
        };
        let game = joined
            .map_err(|error| TournamentRunError::Worker(error.to_string()))?
            .map_err(TournamentRunError::Game)?;
        infrastructure_error |= matches!(game.fault, Some(GameFault::Infrastructure { .. }));
        if let Some(observer) = &request.observer {
            observer
                .game_completed(&game)
                .map_err(TournamentRunError::Output)?;
        }
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
    } else if request
        .max_engine_faults
        .is_some_and(|limit| engine_faults > limit)
    {
        TournamentRunStatus::Invalid
    } else {
        TournamentRunStatus::Completed
    };
    let evidence = games
        .iter()
        .map(TournamentGame::evidence)
        .collect::<Vec<_>>();
    let results = RateTournament::execute(&request.plan, &evidence, request.anchor)
        .map_err(|error| TournamentRunError::Results(error.to_string()))?;
    Ok(TournamentReport {
        status,
        plan: request.plan,
        results,
        time_control: request.time_control,
        adjudication: request.adjudication,
        execution: request.execution,
        master_seed: request.master_seed,
        master_seed_generated: request.master_seed_generated,
        opening_policy: request.openings.report().clone(),
        engine_faults,
        infrastructure_faults,
        games,
    })
}
