//! Pair-atomic SPRT execution adapter.

use std::sync::Arc;

use colosseum_application::{CompletePair, PairCommitQueue, SprtDesign};
use colosseum_core::{
    AdjudicationConfig, GameResult, PairGameResult, PentanomialSprtResult, PentanomialVector,
    SprtDecision, StatisticsError, pentanomial_sprt,
};
use serde::Serialize;
use thiserror::Error;

use crate::match_runner::{
    ConfiguredTimeControl, FaultPolicy, MatchError, MatchExecutionPlan, MatchFaultCounts,
    MatchGame, MatchSide, OpeningPolicyReport, PairGameSettings, play_pair, record_fault,
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
    pub fault_policy: FaultPolicy,
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
    pub invalid_pair: Option<u32>,
    pub faults: MatchFaultCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SprtStatus {
    H1,
    H0,
    Inconclusive,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SprtReport {
    pub status: SprtStatus,
    pub design: SprtDesign,
    pub engine_a_time_control: ConfiguredTimeControl,
    pub engine_b_time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub fault_policy: FaultPolicy,
    pub execution: MatchExecutionPlan,
    pub master_seed: u64,
    pub master_seed_generated: bool,
    pub openings: OpeningPolicyReport,
    pub schedule: PairScheduleReport,
}

impl PairScheduleReport {
    #[must_use]
    pub fn status(&self) -> SprtStatus {
        if self.invalid_pair.is_some() {
            return SprtStatus::Invalid;
        }
        match self.statistics.map(|statistics| statistics.decision) {
            Some(SprtDecision::AcceptH1) => SprtStatus::H1,
            Some(SprtDecision::AcceptH0) => SprtStatus::H0,
            Some(SprtDecision::Continue) | None => SprtStatus::Inconclusive,
        }
    }
}

pub async fn run_pair_schedule(
    request: PairScheduleRequest,
) -> Result<PairScheduleReport, PairScheduleError> {
    validate_completed_prefix(&request.completed_pairs, request.design.max_pairs)?;
    let next_pair_id = request.completed_pairs.len() as u32 + 1;
    let mut commit_queue = PairCommitQueue::new(next_pair_id, request.design.max_pairs)?;
    let mut accumulator = SprtAccumulator::new(request.design, request.fault_policy);
    for pair in &request.completed_pairs {
        if accumulator.admit(pair)? == PairDisposition::PostTerminal {
            return Err(PairScheduleError::InvalidResumePrefix);
        }
    }
    let mut official_pairs = request.completed_pairs;
    let mut post_terminal_pairs = Vec::new();
    let mut workers = tokio::task::JoinSet::new();
    let mut next_to_schedule = next_pair_id;

    while (!accumulator.stopped() && next_to_schedule <= request.design.max_pairs)
        || !workers.is_empty()
    {
        while !accumulator.stopped()
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
        invalid_pair: accumulator.invalid_pair,
        faults: accumulator.faults,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PairDisposition {
    Official,
    PostTerminal,
}

struct SprtAccumulator {
    design: SprtDesign,
    fault_policy: FaultPolicy,
    sample: PentanomialVector,
    statistics: Option<PentanomialSprtResult>,
    terminal_pair: Option<u32>,
    invalid_pair: Option<u32>,
    faults: MatchFaultCounts,
}

impl SprtAccumulator {
    fn new(design: SprtDesign, fault_policy: FaultPolicy) -> Self {
        Self {
            design,
            fault_policy,
            sample: PentanomialVector::default(),
            statistics: None,
            terminal_pair: None,
            invalid_pair: None,
            faults: MatchFaultCounts::default(),
        }
    }

    fn stopped(&self) -> bool {
        self.terminal_pair.is_some() || self.invalid_pair.is_some()
    }

    fn admit(
        &mut self,
        pair: &CompletePair<MatchGame>,
    ) -> Result<PairDisposition, PairScheduleError> {
        if self.stopped() {
            return Ok(PairDisposition::PostTerminal);
        }
        if !pair.first.scorable || !pair.second.scorable {
            return Err(PairScheduleError::UnscorablePair(pair.pair_id));
        }
        record_fault(
            &mut self.faults,
            pair.first.white,
            pair.first.fault.as_ref(),
        );
        record_fault(
            &mut self.faults,
            pair.second.white,
            pair.second.fault.as_ref(),
        );
        if self.faults.engine_total() > self.fault_policy.max_engine_faults
            || self.faults.time_total() > self.fault_policy.max_time_losses
        {
            self.invalid_pair = Some(pair.pair_id);
            self.sample
                .record_pair(result_for_a(&pair.first), result_for_a(&pair.second));
            return Ok(PairDisposition::Official);
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
        if self.stopped() {
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
    use colosseum_engine::{ClockAccountingReport, EngineFaultKind, GameFault, GameSide};

    #[derive(Debug, PartialEq)]
    struct CompletionReplay {
        terminal_pair: u32,
        decision: SprtDecision,
        counts: [u32; 5],
        official_pairs: u32,
        post_terminal_pairs: u32,
    }

    fn replay_worker_completion_order(order: &[u32], winning: bool) -> CompletionReplay {
        let max_pairs = u32::try_from(order.len()).unwrap();
        let design = SprtDesign::new(SprtBundle::Gainer.defaults(), max_pairs, None).unwrap();
        let mut accumulator = SprtAccumulator::new(design, FaultPolicy::default());
        let mut queue = PairCommitQueue::new(1, max_pairs).unwrap();
        let mut official_pairs = 0;
        let mut post_terminal_pairs = 0;
        for &pair_id in order {
            let decisive = if winning {
                PairGameResult::Win
            } else {
                PairGameResult::Loss
            };
            let second = if pair_id % 2 == 0 {
                PairGameResult::Draw
            } else {
                decisive
            };
            for pair in queue
                .complete(CompletePair {
                    pair_id,
                    first: decisive,
                    second,
                })
                .unwrap()
            {
                match accumulator
                    .admit_results(pair.pair_id, pair.first, pair.second)
                    .unwrap()
                {
                    PairDisposition::Official => official_pairs += 1,
                    PairDisposition::PostTerminal => post_terminal_pairs += 1,
                }
            }
        }
        let statistics = accumulator.statistics.unwrap();
        CompletionReplay {
            terminal_pair: accumulator.terminal_pair.unwrap(),
            decision: statistics.decision,
            counts: accumulator.sample.counts(),
            official_pairs,
            post_terminal_pairs,
        }
    }

    fn fixture_game(
        number: u32,
        white: MatchSide,
        result: GameResult,
        termination: colosseum_core::Termination,
        scorable: bool,
        fault: Option<GameFault>,
    ) -> MatchGame {
        MatchGame {
            number,
            white,
            result,
            scorable,
            termination,
            clock_accounting: ClockAccountingReport {
                model: "test".into(),
                version: 1,
                white_margin_ms: 0,
                black_margin_ms: 0,
                monotonic_resolution_ns: 1,
                white_charged_elapsed: None,
                black_charged_elapsed: None,
            },
            opening: crate::match_runner::OpeningAssignment {
                book_index: None,
                label: "test".into(),
            },
            fault,
            error: None,
            pgn: String::new(),
        }
    }

    fn fault_pair(kind: EngineFaultKind) -> CompletePair<MatchGame> {
        use colosseum_core::Termination;

        let termination = match kind {
            EngineFaultKind::Timeout => Termination::TimeForfeit,
            EngineFaultKind::IllegalMove => Termination::IllegalMove,
            EngineFaultKind::Crash | EngineFaultKind::Disconnect | EngineFaultKind::Protocol => {
                Termination::EngineCrash
            }
        };
        CompletePair {
            pair_id: 1,
            first: fixture_game(
                1,
                MatchSide::A,
                GameResult::BlackWin,
                termination,
                true,
                Some(GameFault::Engine {
                    side: GameSide::White,
                    kind,
                    message: "injected".into(),
                }),
            ),
            second: fixture_game(
                2,
                MatchSide::B,
                GameResult::Draw,
                Termination::AdjudicatedDraw,
                true,
                None,
            ),
        }
    }

    #[test]
    fn boundary_pair_is_official_and_every_later_completion_is_separate() {
        let design = SprtDesign::new(SprtBundle::Gainer.defaults(), 1_000, None).unwrap();
        let mut accumulator = SprtAccumulator::new(design, FaultPolicy::default());
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

    #[test]
    fn losing_stream_crosses_h0() {
        let design = SprtDesign::new(SprtBundle::Gainer.defaults(), 1_000, None).unwrap();
        let mut accumulator = SprtAccumulator::new(design, FaultPolicy::default());
        for pair_id in 1..=1_000 {
            let second = if pair_id % 2 == 0 {
                PairGameResult::Draw
            } else {
                PairGameResult::Loss
            };
            accumulator
                .admit_results(pair_id, PairGameResult::Loss, second)
                .unwrap();
            if accumulator.terminal_pair.is_some() {
                break;
            }
        }
        assert_eq!(
            accumulator.statistics.unwrap().decision,
            SprtDecision::AcceptH0
        );
    }

    #[test]
    fn worker_completion_order_cannot_change_either_terminal_sample() {
        let ascending = (1..=1_000).collect::<Vec<_>>();
        let mut interleaved = (2..=1_000).step_by(2).collect::<Vec<_>>();
        interleaved.extend((1..=1_000).step_by(2));
        let descending = (1..=1_000).rev().collect::<Vec<_>>();

        for winning in [true, false] {
            let expected = replay_worker_completion_order(&ascending, winning);
            assert_eq!(expected.official_pairs, expected.terminal_pair);
            assert_eq!(
                expected.official_pairs + expected.post_terminal_pairs,
                1_000
            );
            assert_eq!(
                replay_worker_completion_order(&interleaved, winning),
                expected
            );
            assert_eq!(
                replay_worker_completion_order(&descending, winning),
                expected
            );
        }
    }

    #[test]
    fn strict_sprt_fault_policy_covers_every_engine_fault_and_rejects_infrastructure() {
        for kind in [
            EngineFaultKind::Timeout,
            EngineFaultKind::Crash,
            EngineFaultKind::Disconnect,
            EngineFaultKind::Protocol,
            EngineFaultKind::IllegalMove,
        ] {
            let design = SprtDesign::new(SprtBundle::Gainer.defaults(), 10, None).unwrap();
            let mut accumulator = SprtAccumulator::new(design, FaultPolicy::default());
            assert_eq!(
                accumulator.admit(&fault_pair(kind)).unwrap(),
                PairDisposition::Official
            );
            assert_eq!(accumulator.invalid_pair, Some(1), "{kind:?}");
            assert_eq!(accumulator.faults.engine_a, 1, "{kind:?}");
            assert_eq!(accumulator.faults.engine_b, 0, "{kind:?}");
            assert_eq!(
                accumulator.faults.time_losses_a,
                u32::from(kind == EngineFaultKind::Timeout),
                "{kind:?}"
            );
            assert_eq!(accumulator.sample.pairs(), 1, "{kind:?}");
        }

        let design = SprtDesign::new(SprtBundle::Gainer.defaults(), 10, None).unwrap();
        let mut accumulator = SprtAccumulator::new(design, FaultPolicy::default());
        let pair = CompletePair {
            pair_id: 1,
            first: fixture_game(
                1,
                MatchSide::A,
                GameResult::Draw,
                colosseum_core::Termination::Aborted,
                false,
                Some(GameFault::Infrastructure {
                    operation: "artifact".into(),
                    message: "injected".into(),
                }),
            ),
            second: fixture_game(
                2,
                MatchSide::B,
                GameResult::Draw,
                colosseum_core::Termination::AdjudicatedDraw,
                true,
                None,
            ),
        };
        assert!(matches!(
            accumulator.admit(&pair),
            Err(PairScheduleError::UnscorablePair(1))
        ));
        assert_eq!(accumulator.sample.pairs(), 0);
        assert_eq!(accumulator.invalid_pair, None);
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
