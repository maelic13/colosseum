//! Pair-atomic SPRT execution adapter.
#![allow(dead_code)] // Composed into the live stopping loop in step 4B.3.

use std::sync::Arc;

use colosseum_application::{CompletePair, PairCommitQueue};
use serde::Serialize;
use thiserror::Error;

use crate::match_runner::{MatchError, MatchExecutionPlan, MatchGame, PairGameSettings, play_pair};

pub trait PairObserver: Send + Sync {
    fn pair_committed(&self, pair: &CompletePair<MatchGame>) -> Result<(), String>;
}

#[derive(Clone)]
pub struct PairScheduleRequest {
    pub settings: PairGameSettings,
    pub execution: MatchExecutionPlan,
    pub max_pairs: u32,
    /// Durable pairs must be the contiguous schedule prefix `1..=N`.
    pub completed_pairs: Vec<CompletePair<MatchGame>>,
    pub observer: Option<Arc<dyn PairObserver>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairScheduleReport {
    pub max_pairs: u32,
    pub committed_pairs: Vec<CompletePair<MatchGame>>,
}

pub async fn run_pair_schedule(
    request: PairScheduleRequest,
) -> Result<PairScheduleReport, PairScheduleError> {
    validate_completed_prefix(&request.completed_pairs, request.max_pairs)?;
    let next_pair_id = request.completed_pairs.len() as u32 + 1;
    let mut commit_queue = PairCommitQueue::new(next_pair_id, request.max_pairs)?;
    let mut committed_pairs = request.completed_pairs;
    let mut workers = tokio::task::JoinSet::new();
    let mut next_to_schedule = next_pair_id;
    while next_to_schedule <= request.max_pairs || !workers.is_empty() {
        while next_to_schedule <= request.max_pairs && workers.len() < request.execution.concurrency
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
            if let Some(observer) = &request.observer {
                observer
                    .pair_committed(&released)
                    .map_err(PairScheduleError::Output)?;
            }
            committed_pairs.push(released);
        }
    }
    Ok(PairScheduleReport {
        max_pairs: request.max_pairs,
        committed_pairs,
    })
}

fn validate_completed_prefix(
    completed: &[CompletePair<MatchGame>],
    max_pairs: u32,
) -> Result<(), PairScheduleError> {
    if completed.len() > max_pairs as usize {
        return Err(PairScheduleError::InvalidResumePrefix);
    }
    if completed
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
    #[error("durable SPRT pairs are not a contiguous schedule prefix")]
    InvalidResumePrefix,
    #[error("a pair worker failed: {0}")]
    Worker(String),
    #[error("pair output failed: {0}")]
    Output(String),
    #[error(transparent)]
    Match(#[from] MatchError),
    #[error(transparent)]
    Commit(#[from] colosseum_application::PairCommitError),
}
