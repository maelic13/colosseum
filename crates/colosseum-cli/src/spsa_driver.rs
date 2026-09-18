//! Long-lived, pair-atomic SPSA execution adapter.
//!
//! The CLI process and parsed opening set remain alive across the complete
//! tune. Engine processes still retain the per-game isolation required by the
//! shared runner. Only a complete, fault-free mini-match advances the floating
//! centre vector and becomes a durable iteration.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use colosseum_application::{
    CompletePair, PairCommitQueue, SpsaCommittedUpdate, SpsaIterationTransition,
    SpsaMiniMatchScore, SpsaRunSettings, SpsaTuningState, VerifiedSpsaSchedule,
};
use colosseum_core::{GameResult, PairGameResult, SpsaIteration};
use colosseum_engine::GameFault;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cancellation::Cancellation;
use crate::match_runner::{
    FaultPolicy, MatchError, MatchExecutionPlan, MatchFaultCounts, MatchGame, MatchSide,
    PairGameSettings, pair_game_numbers, play_pair_game, record_fault,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpsaStatus {
    Completed,
    /// Stopped cleanly at an iteration boundary. The stored horizon is
    /// untouched, so the same run directory resumes where it left off.
    Cancelled,
    Invalid,
}

/// What a tune keeps of a committed iteration once its games are in the
/// journal: the centres before and after, the two arm vectors, the pair score
/// and its faults. The games themselves are the journal's lines named by
/// `games`; carrying every game's record through the whole run made a
/// 60-iteration result 6 MB and a real tune's hundreds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaCommittedIteration {
    /// Zero-based schedule iteration.
    pub iteration: u32,
    pub centers_before: Vec<f64>,
    pub prepared: SpsaIteration,
    pub score: SpsaMiniMatchScore,
    pub centers_after: Vec<f64>,
    pub faults: MatchFaultCounts,
    pub games: SpsaJournalGames,
}

/// An iteration that completed but may not update SPSA, summarised the same
/// way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaInvalidIteration {
    /// Zero-based schedule iteration that completed but may not update SPSA.
    pub iteration: u32,
    pub centers_before: Vec<f64>,
    pub prepared: SpsaIteration,
    pub faults: MatchFaultCounts,
    pub reason: String,
    pub games: SpsaJournalGames,
}

/// Where an iteration's games are: the lines of the run's `games.jsonl` whose
/// game numbers run from `first` to `last`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpsaJournalGames {
    pub first: u32,
    pub last: u32,
}

impl SpsaJournalGames {
    /// The game numbers of zero-based `iteration`.
    fn of(iteration: u32, settings: SpsaRunSettings) -> Result<Self, SpsaDriverError> {
        let games = settings
            .pairs_per_iteration()
            .checked_mul(2)
            .ok_or(SpsaDriverError::PairIdentityOverflow)?;
        let first = iteration
            .checked_mul(games)
            .and_then(|value| value.checked_add(1))
            .ok_or(SpsaDriverError::PairIdentityOverflow)?;
        Ok(Self {
            first,
            last: first + games - 1,
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpsaCheckpoint {
    pub completed_iterations: Vec<SpsaCommittedIteration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_iteration: Option<SpsaInvalidIteration>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SpsaDriverReport {
    pub status: SpsaStatus,
    pub settings: SpsaRunSettings,
    pub completed_iterations: Vec<SpsaCommittedIteration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_iteration: Option<SpsaInvalidIteration>,
    pub final_centers: Vec<f64>,
}

/// Receives each finished iteration with its games, once. The driver keeps only
/// the summary afterwards, so what it holds is bounded by the iteration in
/// flight, not by the length of the tune.
pub trait SpsaObserver: Send + Sync {
    fn iteration_committed(
        &self,
        iteration: &SpsaCommittedIteration,
        pairs: &[CompletePair<MatchGame>],
    ) -> Result<(), String>;
    fn iteration_invalid(
        &self,
        iteration: &SpsaInvalidIteration,
        pairs: &[CompletePair<MatchGame>],
    ) -> Result<(), String>;
}

#[derive(Clone)]
pub struct SpsaDriverRequest {
    pub schedule: VerifiedSpsaSchedule,
    pub settings: SpsaRunSettings,
    pub initial_centers: Vec<f64>,
    pub base_engine: colosseum_application::EngineLaunchSpec,
    pub game_settings: PairGameSettings,
    pub execution: MatchExecutionPlan,
    /// When engine faults void the tune. A forfeit within it is scored as the
    /// loss it is and moves the gradient like any other result.
    pub fault_policy: FaultPolicy,
    pub checkpoint: SpsaCheckpoint,
    pub progress: SpsaProgress,
    /// Stop cleanly once this many iterations are committed. This is a request
    /// about this invocation, never a change to the stored horizon.
    pub stop_after_iteration: Option<u32>,
    pub cancellation: Cancellation,
    pub observer: Option<Arc<dyn SpsaObserver>>,
}

impl std::fmt::Debug for SpsaDriverRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SpsaDriverRequest")
            .field("settings", &self.settings)
            .field("initial_centers", &self.initial_centers)
            .field(
                "completed_iterations",
                &self.checkpoint.completed_iterations.len(),
            )
            .field("progress", &self.progress.snapshot())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SpsaProgress {
    completed_iterations: Arc<AtomicU32>,
    completed_pairs: Arc<AtomicU32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SpsaProgressSnapshot {
    pub completed_iterations: u32,
    pub completed_pairs: u32,
}

impl SpsaProgress {
    #[must_use]
    pub fn snapshot(&self) -> SpsaProgressSnapshot {
        SpsaProgressSnapshot {
            completed_iterations: self.completed_iterations.load(Ordering::Relaxed),
            completed_pairs: self.completed_pairs.load(Ordering::Relaxed),
        }
    }

    fn initialize(&self, completed_iterations: u32, completed_pairs: u32) {
        self.completed_iterations
            .store(completed_iterations, Ordering::Relaxed);
        self.completed_pairs
            .store(completed_pairs, Ordering::Relaxed);
    }
}

pub async fn run_spsa(request: SpsaDriverRequest) -> Result<SpsaDriverReport, SpsaDriverError> {
    if request.checkpoint.invalid_iteration.is_some() {
        return Err(SpsaDriverError::TerminalCheckpoint);
    }
    let artifact = request.schedule.artifact().clone();
    let history = validate_checkpoint_evidence(&request.checkpoint, request.settings)?;
    let mut state = SpsaTuningState::resume(
        request.schedule,
        request.settings,
        request.initial_centers,
        &history,
    )?;
    let mut completed_iterations = request.checkpoint.completed_iterations;
    // Faults over every committed game, a resumed run's included: the
    // allowance is judged over the whole tune.
    let mut run_faults = MatchFaultCounts::default();
    for iteration in &completed_iterations {
        run_faults.absorb(iteration.faults);
    }
    let games_per_iteration = u64::from(request.settings.pairs_per_iteration()) * 2;
    let resumed_iterations = state.completed_iterations();
    request.progress.initialize(
        resumed_iterations,
        resumed_iterations
            .checked_mul(request.settings.pairs_per_iteration())
            .ok_or(SpsaDriverError::PairIdentityOverflow)?,
    );

    while let Some(prepared) = state.prepare_next()? {
        let iteration = prepared.iteration;
        let (plus, minus) = arm_launches(&request.base_engine, &artifact, &prepared)?;
        let mut game_settings = request.game_settings.clone();
        game_settings.engine_a = plus;
        game_settings.engine_b = minus;
        // Stop before committing the engines to another complete mini-match.
        // The staged limit counts committed iterations cumulatively, including
        // those a resume replayed, so repeating the same command after a stop
        // at N plays nothing rather than one more iteration each time.
        let staged_stop = request
            .stop_after_iteration
            .is_some_and(|limit| state.completed_iterations() >= limit);
        if request.cancellation.stopping() || staged_stop {
            return Ok(SpsaDriverReport {
                status: SpsaStatus::Cancelled,
                settings: request.settings,
                completed_iterations,
                invalid_iteration: None,
                final_centers: state.centers().to_vec(),
            });
        }
        let pairs = play_mini_match(
            iteration,
            request.settings,
            game_settings,
            &request.execution,
            &request.progress,
        )
        .await?;
        let faults = fault_counts(&pairs);
        if faults.infrastructure > 0 || pairs.iter().any(pair_is_unscorable) {
            return Err(SpsaDriverError::InfrastructureMiniMatch { iteration });
        }
        run_faults.absorb(faults);
        let games_played =
            (u64::from(state.completed_iterations()) + 1).saturating_mul(games_per_iteration);
        if request.fault_policy.exceeded(run_faults, games_played) {
            let SpsaIterationTransition::Invalid(policy) = state.commit_iteration(
                prepared,
                pairs.len() as u32,
                None,
                faults.engine_total().max(1),
            )?
            else {
                unreachable!("an engine fault cannot produce a committed update")
            };
            let invalid = SpsaInvalidIteration {
                iteration: policy.iteration,
                centers_before: policy.centers_before,
                prepared: policy.prepared,
                faults,
                reason: policy.reason,
                games: SpsaJournalGames::of(iteration, request.settings)?,
            };
            if let Some(observer) = &request.observer {
                observer
                    .iteration_invalid(&invalid, &pairs)
                    .map_err(SpsaDriverError::Output)?;
            }
            return Ok(SpsaDriverReport {
                status: SpsaStatus::Invalid,
                settings: request.settings,
                completed_iterations,
                invalid_iteration: Some(invalid),
                final_centers: state.centers().to_vec(),
            });
        }
        // Within the allowance a forfeit is a result: the game it ended is
        // scored as the loss it is and the gradient uses it.
        let score = score_mini_match(&pairs)?;
        let SpsaIterationTransition::Committed(update) =
            state.commit_iteration(prepared, pairs.len() as u32, Some(score), 0)?
        else {
            unreachable!("a scored complete mini-match cannot invalidate")
        };
        let committed = SpsaCommittedIteration {
            iteration: update.iteration,
            centers_before: update.centers_before,
            prepared: update.prepared,
            score: update.score,
            centers_after: update.centers_after,
            faults,
            games: SpsaJournalGames::of(iteration, request.settings)?,
        };
        if let Some(observer) = &request.observer {
            observer
                .iteration_committed(&committed, &pairs)
                .map_err(SpsaDriverError::Output)?;
        }
        // The observer committed the games to the journal; the driver keeps
        // the summary and lets the games go.
        drop(pairs);
        completed_iterations.push(committed);
        request.cancellation.record_committed_unit();
        request
            .progress
            .completed_iterations
            .store(state.completed_iterations(), Ordering::Relaxed);
    }

    Ok(SpsaDriverReport {
        status: SpsaStatus::Completed,
        settings: request.settings,
        completed_iterations,
        invalid_iteration: None,
        final_centers: state.centers().to_vec(),
    })
}

/// Rebuild the committed iterations of a tune from their journalled games.
///
/// Nothing about an iteration is stored except its games. Its perturbation,
/// its gains and the centres it moved to are recomputed from the verified
/// schedule exactly as the driver computed them the first time, so a resumed
/// tune and a status report stand on the same arithmetic as the run that
/// wrote the games. An iteration whose games are not all present was cut by a
/// kill: it and anything after it are dropped, to be played again. What is
/// returned is the summaries; the games stay in the journal.
pub fn replay_iterations(
    schedule: VerifiedSpsaSchedule,
    settings: SpsaRunSettings,
    initial_centers: Vec<f64>,
    iterations: &std::collections::BTreeMap<u32, Vec<CompletePair<MatchGame>>>,
) -> Result<Vec<SpsaCommittedIteration>, SpsaDriverError> {
    let mut state = SpsaTuningState::resume(schedule, settings, initial_centers, &[])?;
    let mut committed = Vec::new();
    for iteration in 0..settings.iterations {
        let Some(pairs) = iterations.get(&iteration) else {
            break;
        };
        if pairs.len() != settings.pairs_per_iteration() as usize {
            break;
        }
        validate_pair_ids(iteration, settings, pairs)?;
        if let Some((game, reason)) = unusable_game(pairs) {
            return Err(SpsaDriverError::UnusableJournalGame {
                iteration,
                game,
                reason,
            });
        }
        let Some(prepared) = state.prepare_next()? else {
            return Err(SpsaDriverError::CheckpointBeyondHorizon);
        };
        let score = score_mini_match(pairs)?;
        let SpsaIterationTransition::Committed(update) =
            state.commit_iteration(prepared, pairs.len() as u32, Some(score), 0)?
        else {
            return Err(SpsaDriverError::CheckpointMismatch { iteration });
        };
        committed.push(SpsaCommittedIteration {
            iteration: update.iteration,
            centers_before: update.centers_before,
            prepared: update.prepared,
            score: update.score,
            centers_after: update.centers_after,
            faults: fault_counts(pairs),
            games: SpsaJournalGames::of(iteration, settings)?,
        });
    }
    Ok(committed)
}

fn arm_launches(
    base: &colosseum_application::EngineLaunchSpec,
    artifact: &colosseum_core::SpsaScheduleArtifact,
    prepared: &SpsaIteration,
) -> Result<
    (
        colosseum_application::EngineLaunchSpec,
        colosseum_application::EngineLaunchSpec,
    ),
    SpsaDriverError,
> {
    if artifact.knobs.len() != prepared.plus.len() || artifact.knobs.len() != prepared.minus.len() {
        return Err(SpsaDriverError::PreparedDimensionMismatch);
    }
    let base_label = base.label.clone().unwrap_or_else(|| {
        base.executable
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("engine")
            .to_owned()
    });
    let mut plus = base.clone();
    let mut minus = base.clone();
    plus.label = Some(format!("{base_label} [SPSA plus]"));
    minus.label = Some(format!("{base_label} [SPSA minus]"));
    for ((knob, plus_value), minus_value) in artifact
        .knobs
        .iter()
        .zip(&prepared.plus)
        .zip(&prepared.minus)
    {
        plus.options.insert(
            knob.name.clone(),
            colosseum_application::UciOptionValue::Spin(plus_value.sent),
        );
        minus.options.insert(
            knob.name.clone(),
            colosseum_application::UciOptionValue::Spin(minus_value.sent),
        );
    }
    Ok((plus, minus))
}

async fn play_mini_match(
    iteration: u32,
    settings: SpsaRunSettings,
    game_settings: PairGameSettings,
    execution: &MatchExecutionPlan,
    progress: &SpsaProgress,
) -> Result<Vec<CompletePair<MatchGame>>, SpsaDriverError> {
    let pairs_per_iteration = settings.pairs_per_iteration();
    let first_pair = iteration
        .checked_mul(pairs_per_iteration)
        .and_then(|value| value.checked_add(1))
        .ok_or(SpsaDriverError::PairIdentityOverflow)?;
    let last_pair = first_pair
        .checked_add(pairs_per_iteration - 1)
        .ok_or(SpsaDriverError::PairIdentityOverflow)?;
    let mut queue = PairCommitQueue::new(first_pair, last_pair)?;
    // The slot-holding unit of a mini-match is the game, not the pair. Both
    // arms play in every game and every slot is equivalent, so holding a slot
    // for a whole pair buys nothing, and it costs a wave: sixteen pairs on
    // fourteen slots ran as fourteen pairs and then two more on an otherwise
    // idle machine. Games are launched in schedule order, each on the free
    // slot it finds; the two games of a pair may run at once on different
    // slots. The pairs are reassembled by identity before anything is scored.
    let (first_game, _) = pair_game_numbers(first_pair)?;
    let (_, last_game) = pair_game_numbers(last_pair)?;
    let mut workers = tokio::task::JoinSet::new();
    let mut next_game = first_game;
    let mut halves = std::collections::BTreeMap::<u32, MatchGame>::new();
    let mut pairs = Vec::with_capacity(pairs_per_iteration as usize);
    let mut pool = execution.slot_pool()?;
    while next_game <= last_game || !workers.is_empty() {
        while next_game <= last_game && workers.len() < execution.concurrency {
            let number = next_game;
            next_game += 1;
            let position = pool
                .take()
                .expect("a mini-match keeps no more games live than it has slots");
            debug_assert_eq!(pool.held(), workers.len() + 1);
            let slot = execution.slots[position].clone();
            let game_settings = game_settings.clone();
            workers.spawn(async move {
                (
                    position,
                    play_pair_game(number, &slot, &game_settings).await,
                )
            });
        }
        let Some(joined) = workers.join_next().await else {
            break;
        };
        let (position, game) =
            joined.map_err(|error| SpsaDriverError::Worker(error.to_string()))?;
        // The game's engines have exited: `play_pair_game` returns only then.
        pool.give_back(position);
        let pair_id = game.number.div_ceil(2);
        let Some(other) = halves.remove(&pair_id) else {
            halves.insert(pair_id, game);
            continue;
        };
        let (first, second) = if other.number < game.number {
            (other, game)
        } else {
            (game, other)
        };
        progress.completed_pairs.fetch_add(1, Ordering::Relaxed);
        pairs.extend(queue.complete(CompletePair {
            pair_id,
            first,
            second,
        })?);
    }
    if pairs.len() != pairs_per_iteration as usize {
        return Err(SpsaDriverError::IncompleteMiniMatch {
            iteration,
            expected_pairs: pairs_per_iteration,
            completed_pairs: pairs.len() as u32,
        });
    }
    Ok(pairs)
}

fn validate_checkpoint_evidence(
    checkpoint: &SpsaCheckpoint,
    settings: SpsaRunSettings,
) -> Result<Vec<SpsaCommittedUpdate>, SpsaDriverError> {
    if checkpoint.completed_iterations.len() > settings.iterations as usize {
        return Err(SpsaDriverError::CheckpointBeyondHorizon);
    }
    let mut history = Vec::with_capacity(checkpoint.completed_iterations.len());
    for (index, record) in checkpoint.completed_iterations.iter().enumerate() {
        let iteration =
            u32::try_from(index).map_err(|_| SpsaDriverError::IterationCountOverflow)?;
        // The games behind each summary were verified when it was rebuilt
        // from the journal; here the summaries must be the schedule's own
        // iterations, in order, over the games the schedule gives them.
        if record.iteration != iteration
            || record.games != SpsaJournalGames::of(iteration, settings)?
        {
            return Err(SpsaDriverError::CheckpointMismatch { iteration });
        }
        history.push(SpsaCommittedUpdate {
            iteration: record.iteration,
            centers_before: record.centers_before.clone(),
            prepared: record.prepared.clone(),
            score: record.score,
            centers_after: record.centers_after.clone(),
        });
    }
    Ok(history)
}

fn validate_pair_ids(
    iteration: u32,
    settings: SpsaRunSettings,
    pairs: &[CompletePair<MatchGame>],
) -> Result<(), SpsaDriverError> {
    let first = iteration
        .checked_mul(settings.pairs_per_iteration())
        .and_then(|value| value.checked_add(1))
        .ok_or(SpsaDriverError::PairIdentityOverflow)?;
    if pairs.len() != settings.pairs_per_iteration() as usize
        || pairs
            .iter()
            .enumerate()
            .any(|(offset, pair)| pair.pair_id != first + offset as u32)
    {
        return Err(SpsaDriverError::CheckpointMismatch { iteration });
    }
    Ok(())
}

fn score_mini_match(
    pairs: &[CompletePair<MatchGame>],
) -> Result<SpsaMiniMatchScore, SpsaDriverError> {
    let mut plus_wins = 0_u32;
    let mut plus_losses = 0_u32;
    let mut draws = 0_u32;
    for pair in pairs {
        for game in [&pair.first, &pair.second] {
            if !game.scorable {
                return Err(SpsaDriverError::UnscorableGame { game: game.number });
            }
            match result_for_plus(game) {
                PairGameResult::Win => plus_wins += 1,
                PairGameResult::Loss => plus_losses += 1,
                PairGameResult::Draw => draws += 1,
            }
        }
    }
    let difference = i32::try_from(plus_wins)
        .and_then(|wins| i32::try_from(plus_losses).map(|losses| wins - losses))
        .map_err(|_| SpsaDriverError::ScoreOverflow)?;
    Ok(SpsaMiniMatchScore {
        plus_wins,
        plus_losses,
        draws,
        difference,
    })
}

fn result_for_plus(game: &MatchGame) -> PairGameResult {
    match (game.white, game.result) {
        (_, GameResult::Draw) => PairGameResult::Draw,
        (MatchSide::A, GameResult::WhiteWin) | (MatchSide::B, GameResult::BlackWin) => {
            PairGameResult::Win
        }
        (MatchSide::A, GameResult::BlackWin) | (MatchSide::B, GameResult::WhiteWin) => {
            PairGameResult::Loss
        }
    }
}

fn fault_counts(pairs: &[CompletePair<MatchGame>]) -> MatchFaultCounts {
    let mut counts = MatchFaultCounts::default();
    for pair in pairs {
        for game in [&pair.first, &pair.second] {
            record_fault(&mut counts, game.white, game.fault.as_ref());
        }
    }
    counts
}

/// The first game of a rebuilt iteration that no committed iteration can hold,
/// and why.
fn unusable_game(pairs: &[CompletePair<MatchGame>]) -> Option<(u32, &'static str)> {
    pairs
        .iter()
        .flat_map(|pair| [&pair.first, &pair.second])
        .find_map(|game| {
            // An engine fault is a scored result and belongs in the iteration;
            // a game nobody could score does not.
            (!game.scorable || matches!(game.fault, Some(GameFault::Infrastructure { .. })))
                .then_some((game.number, "could not be scored"))
        })
}

fn pair_is_unscorable(pair: &CompletePair<MatchGame>) -> bool {
    !pair.first.scorable
        || !pair.second.scorable
        || matches!(pair.first.fault, Some(GameFault::Infrastructure { .. }))
        || matches!(pair.second.fault, Some(GameFault::Infrastructure { .. }))
}

#[derive(Debug, Error)]
pub enum SpsaDriverError {
    #[error(transparent)]
    Policy(#[from] colosseum_application::SpsaDriverPolicyError),
    #[error(transparent)]
    PairCommit(#[from] colosseum_application::PairCommitError),
    #[error(transparent)]
    Match(#[from] MatchError),
    #[error("SPSA checkpoint contains a terminal invalid iteration")]
    TerminalCheckpoint,
    #[error("SPSA checkpoint extends beyond the configured horizon")]
    CheckpointBeyondHorizon,
    #[error("SPSA checkpoint does not reproduce iteration {iteration}")]
    CheckpointMismatch { iteration: u32 },
    #[error(
        "SPSA iteration {iteration} in the journal holds game {game}, which {reason}; a tune commits no iteration with such a game, so this run directory cannot be resumed as it is: run the same command with --restart"
    )]
    UnusableJournalGame {
        iteration: u32,
        game: u32,
        reason: &'static str,
    },
    #[error("SPSA prepared arm vector does not match the persisted knob vector")]
    PreparedDimensionMismatch,
    #[error("SPSA pair identity overflow")]
    PairIdentityOverflow,
    #[error("SPSA iteration count is not representable")]
    IterationCountOverflow,
    #[error("SPSA mini-match {iteration} completed {completed_pairs}/{expected_pairs} pairs")]
    IncompleteMiniMatch {
        iteration: u32,
        expected_pairs: u32,
        completed_pairs: u32,
    },
    #[error("SPSA mini-match {iteration} had a non-scorable infrastructure failure")]
    InfrastructureMiniMatch { iteration: u32 },
    #[error("SPSA game {game} is not scorable")]
    UnscorableGame { game: u32 },
    #[error("SPSA score difference is outside the supported integer range")]
    ScoreOverflow,
    #[error("SPSA worker failed: {0}")]
    Worker(String),
    #[error("durable SPSA output failed: {0}")]
    Output(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use colosseum_core::Termination;
    use colosseum_engine::ClockAccountingReport;

    fn game(number: u32, scorable: bool, fault: Option<GameFault>) -> MatchGame {
        MatchGame {
            number,
            white: if number % 2 == 1 {
                MatchSide::A
            } else {
                MatchSide::B
            },
            result: GameResult::Draw,
            scorable,
            termination: Termination::FiftyMove,
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
            },
            opening: crate::match_runner::OpeningAssignment {
                book_index: None,
                label: "startpos".into(),
            },
            fault,
            error: None,
            pgn: String::new(),
            slot: None,
        }
    }

    #[test]
    fn a_rebuilt_iteration_with_an_unscorable_game_names_that_game() {
        let pairs = vec![
            CompletePair {
                pair_id: 1,
                first: game(1, true, None),
                second: game(2, true, None),
            },
            CompletePair {
                pair_id: 2,
                first: game(3, false, None),
                second: game(4, true, None),
            },
        ];
        let (number, reason) = unusable_game(&pairs).unwrap();
        assert_eq!((number, reason), (3, "could not be scored"));
        let message = SpsaDriverError::UnusableJournalGame {
            iteration: 0,
            game: number,
            reason,
        }
        .to_string();
        assert!(
            message.contains("iteration 0")
                && message.contains("game 3")
                && message.contains("--restart"),
            "{message}"
        );
        assert!(unusable_game(&pairs[..1]).is_none());
    }
}
