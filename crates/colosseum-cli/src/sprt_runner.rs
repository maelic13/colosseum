//! Pair-atomic SPRT execution adapter.
#![allow(dead_code)] // Composed into the public command/report in step 4B.4.

use std::sync::Arc;

use colosseum_application::{CompletePair, PairCommitQueue, SprtDesign};
use colosseum_core::{
    GameResult, PairGameResult, PentanomialSprtResult, PentanomialVector, SprtDecision,
    StatisticsError, pentanomial_sprt,
};
use serde::Serialize;
use thiserror::Error;

use crate::match_runner::{
    MatchError, MatchExecutionPlan, MatchGame, MatchSide, PairGameSettings, play_pair,
};

pub trait PairObserver: Send + Sync {
    fn official_pair(&self, pair: &CompletePair<MatchGame>) -> Result<(), String>;
    fn post_terminal_pair(&self, pair: &CompletePair<MatchGame>) -> Result<(), String>;
}

#[derive(Clone)]
pub struct PairScheduleRequest {
    pub settings: PairGameSettings,
    pub execution: MatchExecutionPlan,
    pub design: SprtDesign,
    /// Durable pairs must be the contiguous official prefix `1..=N`.
    pub completed_pairs: Vec<CompletePair<MatchGame>>,
    pub observer: Option<Arc<dyn PairObserver>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PairScheduleReport {
    pub max_pairs: u32,
    pub official_pairs: Vec<CompletePair<MatchGame>>,
    pub post_terminal_pairs: Vec<CompletePair<MatchGame>>,
    pub pentanomial: [u32; 5],
    pub statistics: Option<PentanomialSprtResult>,
    pub terminal_pair: Option<u32>,
}

pub async fn run_pair_schedule(
    request: PairScheduleRequest,
) -> Result<PairScheduleReport, PairScheduleError> {
    validate_completed_prefix(&request.completed_pairs, request.design.max_pairs)?;
    let next_pair_id = request.completed_pairs.len() as u32 + 1;
    let mut commit_queue = PairCommitQueue::new(next_pair_id, request.design.max_pairs)?;
    let mut accumulator = SprtAccumulator::new(request.design);
    for pair in &request.completed_pairs {
        if accumulator.admit(pair)? == PairDisposition::PostTerminal {
            return Err(PairScheduleError::InvalidResumePrefix);
        }
    }
    let mut official_pairs = request.completed_pairs;
    let mut post_terminal_pairs = Vec::new();
    let mut workers = tokio::task::JoinSet::new();
    let mut next_to_schedule = next_pair_id;

    while (accumulator.terminal_pair.is_none() && next_to_schedule <= request.design.max_pairs)
        || !workers.is_empty()
    {
        while accumulator.terminal_pair.is_none()
            && next_to_schedule <= request.design.max_pairs
            && workers.len() < request.execution.concurrency
        {
            let pair_id = next_to_schedule;
            next_to_schedule += 1;
            let slot = request.execution.slots
                [(pair_id as usize - 1) % request.execution.slots.len()]
            .clone();
            let settings = request.settings.clone();
            workers.spawn(async move { play_pair(pair_id, &slot, settings).await });
        }
        let Some(joined) = workers.join_next().await else {
            break;
        };
        let pair = joined.map_err(|error| PairScheduleError::Worker(error.to_string()))??;
        for released in commit_queue.complete(pair)? {
            match accumulator.admit(&released)? {
                PairDisposition::Official => {
                    if let Some(observer) = &request.observer {
                        observer
                            .official_pair(&released)
                            .map_err(PairScheduleError::Output)?;
                    }
                    official_pairs.push(released);
                }
                PairDisposition::PostTerminal => {
                    if let Some(observer) = &request.observer {
                        observer
                            .post_terminal_pair(&released)
                            .map_err(PairScheduleError::Output)?;
                    }
                    post_terminal_pairs.push(released);
                }
            }
        }
    }
    Ok(PairScheduleReport {
        max_pairs: request.design.max_pairs,
        official_pairs,
        post_terminal_pairs,
        pentanomial: accumulator.sample.counts(),
        statistics: accumulator.statistics,
        terminal_pair: accumulator.terminal_pair,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairDisposition {
    Official,
    PostTerminal,
}

struct SprtAccumulator {
    design: SprtDesign,
    sample: PentanomialVector,
    statistics: Option<PentanomialSprtResult>,
    terminal_pair: Option<u32>,
}

impl SprtAccumulator {
    fn new(design: SprtDesign) -> Self {
        Self {
            design,
            sample: PentanomialVector::default(),
            statistics: None,
            terminal_pair: None,
        }
    }

    fn admit(
        &mut self,
        pair: &CompletePair<MatchGame>,
    ) -> Result<PairDisposition, PairScheduleError> {
        if self.terminal_pair.is_some() {
            return Ok(PairDisposition::PostTerminal);
        }
        if !pair.first.scorable || !pair.second.scorable {
            return Err(PairScheduleError::UnscorablePair(pair.pair_id));
        }
        self.admit_results(
            pair.pair_id,
            result_for_a(&pair.first),
            result_for_a(&pair.second),
        )
    }

    fn admit_results(
        &mut self,
        pair_id: u32,
        first: PairGameResult,
        second: PairGameResult,
    ) -> Result<PairDisposition, PairScheduleError> {
        if self.terminal_pair.is_some() {
            return Ok(PairDisposition::PostTerminal);
        }
        self.sample.record_pair(first, second);
        match pentanomial_sprt(
            &self.sample,
            self.design.parameters.model,
            self.design.parameters.elo0,
            self.design.parameters.elo1,
            self.design.parameters.alpha,
            self.design.parameters.beta,
        ) {
            Ok(statistics) => {
                if !matches!(statistics.decision, SprtDecision::Continue) {
                    self.terminal_pair = Some(pair_id);
                }
                self.statistics = Some(statistics);
            }
            Err(StatisticsError::InsufficientPairs { .. } | StatisticsError::ZeroVariance) => {}
            Err(error) => return Err(PairScheduleError::Statistics(error)),
        }
        Ok(PairDisposition::Official)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use colosseum_application::{SprtBundle, SprtDesign};

    #[test]
    fn boundary_pair_is_official_and_every_later_completion_is_separate() {
        let design = SprtDesign::new(SprtBundle::Gainer.defaults(), 1_000, None).unwrap();
        let mut accumulator = SprtAccumulator::new(design);
        for pair_id in 1..=1_000 {
            let second = if pair_id % 2 == 0 {
                PairGameResult::Draw
            } else {
                PairGameResult::Win
            };
            assert_eq!(
                accumulator
                    .admit_results(pair_id, PairGameResult::Win, second)
                    .unwrap(),
                PairDisposition::Official
            );
            if accumulator.terminal_pair.is_some() {
                break;
            }
        }
        let terminal = accumulator
            .terminal_pair
            .expect("strong fixture crosses H1 before its cap");
        let counts_at_boundary = accumulator.sample.counts();
        assert_eq!(
            accumulator
                .admit_results(terminal + 1, PairGameResult::Loss, PairGameResult::Loss)
                .unwrap(),
            PairDisposition::PostTerminal
        );
        assert_eq!(accumulator.sample.counts(), counts_at_boundary);
        assert_eq!(
            accumulator.statistics.unwrap().decision,
            SprtDecision::AcceptH1
        );
    }
}

fn result_for_a(game: &MatchGame) -> PairGameResult {
    match (game.white, game.result) {
        (MatchSide::A, GameResult::WhiteWin) | (MatchSide::B, GameResult::BlackWin) => {
            PairGameResult::Win
        }
        (MatchSide::A, GameResult::BlackWin) | (MatchSide::B, GameResult::WhiteWin) => {
            PairGameResult::Loss
        }
        (_, GameResult::Draw) => PairGameResult::Draw,
    }
}

fn validate_completed_prefix(
    completed: &[CompletePair<MatchGame>],
    max_pairs: u32,
) -> Result<(), PairScheduleError> {
    if completed.len() > max_pairs as usize
        || completed
            .iter()
            .enumerate()
            .any(|(index, pair)| pair.pair_id != index as u32 + 1)
    {
        return Err(PairScheduleError::InvalidResumePrefix);
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum PairScheduleError {
    #[error("durable SPRT pairs are not a contiguous official schedule prefix")]
    InvalidResumePrefix,
    #[error("a pair worker failed: {0}")]
    Worker(String),
    #[error("pair output failed: {0}")]
    Output(String),
    #[error("pair {0} contains a non-scorable infrastructure game")]
    UnscorablePair(u32),
    #[error(transparent)]
    Statistics(#[from] StatisticsError),
    #[error(transparent)]
    Match(#[from] MatchError),
    #[error(transparent)]
    Commit(#[from] colosseum_application::PairCommitError),
}
