use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ApplicationError, EngineSessionFactory, PortFuture, RuntimeParticipant, SearchLimit,
    SearchRequest,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "moves", rename_all = "kebab-case")]
pub enum SuiteExpectation {
    Best(Vec<String>),
    Avoid(Vec<String>),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuitePosition {
    pub index: u32,
    pub id: String,
    pub fen: String,
    pub expectation: SuiteExpectation,
    pub unknown_operations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MalformedSuitePosition {
    pub index: u32,
    pub source: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "entry", rename_all = "kebab-case")]
pub enum SuiteEntry {
    Position(SuitePosition),
    Malformed(MalformedSuitePosition),
}

impl SuiteEntry {
    #[must_use]
    pub const fn index(&self) -> u32 {
        match self {
            Self::Position(position) => position.index,
            Self::Malformed(position) => position.index,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuiteDesign {
    pub position_set_sha256: String,
    pub search_sha256: String,
    pub limit: SearchLimit,
    pub deadline_ms: u64,
    pub entries: Vec<SuiteEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SuiteOutcome {
    Passed,
    Failed,
    Unscored,
    Malformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuitePositionResult {
    pub index: u32,
    pub id: String,
    pub outcome: SuiteOutcome,
    pub best_move: Option<String>,
    pub harness_elapsed_ns: Option<u64>,
    pub expectation: Option<SuiteExpectation>,
    pub unknown_operations: Vec<String>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuiteReport {
    pub position_set_sha256: String,
    pub search_sha256: String,
    pub limit: SearchLimit,
    pub deadline_ms: u64,
    pub total_entries: u32,
    pub searched: u32,
    pub assessed: u32,
    pub passed: u32,
    pub failed: u32,
    pub unscored: u32,
    pub malformed: u32,
    pub pass_rate: Option<f64>,
    pub results: Vec<SuitePositionResult>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuiteBaselineComparison {
    pub baseline_passed: u32,
    pub current_passed: u32,
    pub passed_delta: i64,
    pub baseline_pass_rate: Option<f64>,
    pub current_pass_rate: Option<f64>,
    pub pass_rate_delta: Option<f64>,
    pub changed_positions: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SuiteError {
    #[error("position suite requires at least one input entry and a positive deadline")]
    InvalidDesign,
    #[error("position suite entry indices must be unique")]
    DuplicateIndex,
    #[error("resume result {index} does not correspond to the current position set")]
    InvalidResumeResult { index: u32 },
    #[error("baseline position-set/search identity is incompatible")]
    IncompatibleBaseline,
}

pub trait SuiteProgress: Send + Sync {
    fn commit(&self, result: &SuitePositionResult) -> PortFuture<'_, Result<(), ApplicationError>>;
}

pub struct RunSuite;

impl RunSuite {
    pub fn execute<'a>(
        sessions: &'a dyn EngineSessionFactory,
        participant: &'a RuntimeParticipant,
        design: SuiteDesign,
        prior: Vec<SuitePositionResult>,
        progress: &'a dyn SuiteProgress,
    ) -> PortFuture<'a, Result<SuiteReport, ApplicationError>> {
        Box::pin(async move {
            validate(&design, &prior).map_err(domain_error)?;
            let mut completed = prior
                .into_iter()
                .map(|result| (result.index, result))
                .collect::<BTreeMap<_, _>>();

            for entry in &design.entries {
                if completed.contains_key(&entry.index()) {
                    continue;
                }
                if let SuiteEntry::Malformed(malformed) = entry {
                    let result = SuitePositionResult {
                        index: malformed.index,
                        id: format!("line {}", malformed.index),
                        outcome: SuiteOutcome::Malformed,
                        best_move: None,
                        harness_elapsed_ns: None,
                        expectation: None,
                        unknown_operations: Vec::new(),
                        detail: malformed.reason.clone(),
                    };
                    progress.commit(&result).await?;
                    completed.insert(result.index, result);
                }
            }

            let pending = design.entries.iter().any(|entry| {
                matches!(entry, SuiteEntry::Position(_)) && !completed.contains_key(&entry.index())
            });
            if pending {
                let mut session = sessions.open(participant).await?;
                let run = async {
                    let inspection = session.inspect().await?;
                    for (name, value) in &participant.launch.options {
                        if !inspection
                            .options
                            .iter()
                            .any(|schema| schema.name() == name)
                        {
                            return Err(ApplicationError::ConfigurationFault(format!(
                                "UCI option {name:?} was not advertised by the engine"
                            )));
                        }
                        let value = value.command_value();
                        session.set_option(name, value.as_deref()).await?;
                    }
                    session.is_ready().await?;
                    for entry in &design.entries {
                        let SuiteEntry::Position(position) = entry else {
                            continue;
                        };
                        if completed.contains_key(&position.index) {
                            continue;
                        }
                        session.new_game().await?;
                        session.is_ready().await?;
                        let observation = session
                            .search(SearchRequest {
                                position: position.fen.clone(),
                                moves: Vec::new(),
                                limit: design.limit.clone(),
                                deadline_ms: design.deadline_ms,
                            })
                            .await?;
                        let (outcome, detail) =
                            assess(&position.expectation, &observation.best_move);
                        let result = SuitePositionResult {
                            index: position.index,
                            id: position.id.clone(),
                            outcome,
                            best_move: Some(observation.best_move),
                            harness_elapsed_ns: Some(observation.harness_elapsed_ns),
                            expectation: Some(position.expectation.clone()),
                            unknown_operations: position.unknown_operations.clone(),
                            detail,
                        };
                        progress.commit(&result).await?;
                        completed.insert(result.index, result);
                    }
                    Ok(())
                }
                .await;
                let shutdown = session.shutdown().await;
                match (run, shutdown) {
                    (Err(error), _) | (Ok(()), Err(error)) => return Err(error),
                    (Ok(()), Ok(())) => {}
                }
            }
            Ok(summarize(design, completed))
        })
    }
}

pub fn compare_suite_baseline(
    current: &SuiteReport,
    baseline: &SuiteReport,
) -> Result<SuiteBaselineComparison, SuiteError> {
    if current.position_set_sha256 != baseline.position_set_sha256
        || current.search_sha256 != baseline.search_sha256
        || current.total_entries != baseline.total_entries
    {
        return Err(SuiteError::IncompatibleBaseline);
    }
    let old = baseline
        .results
        .iter()
        .map(|result| (result.index, result.outcome))
        .collect::<BTreeMap<_, _>>();
    let changed_positions = current
        .results
        .iter()
        .filter(|result| {
            old.get(&result.index)
                .is_some_and(|old| *old != result.outcome)
        })
        .map(|result| result.index)
        .collect();
    Ok(SuiteBaselineComparison {
        baseline_passed: baseline.passed,
        current_passed: current.passed,
        passed_delta: i64::from(current.passed) - i64::from(baseline.passed),
        baseline_pass_rate: baseline.pass_rate,
        current_pass_rate: current.pass_rate,
        pass_rate_delta: current
            .pass_rate
            .zip(baseline.pass_rate)
            .map(|(current, baseline)| current - baseline),
        changed_positions,
    })
}

fn validate(design: &SuiteDesign, prior: &[SuitePositionResult]) -> Result<(), SuiteError> {
    let zero_limit = matches!(
        &design.limit,
        SearchLimit::MoveTimeMs(0) | SearchLimit::Nodes(0) | SearchLimit::Depth(0)
    );
    if design.entries.is_empty() || design.deadline_ms == 0 || zero_limit {
        return Err(SuiteError::InvalidDesign);
    }
    let mut indices = design
        .entries
        .iter()
        .map(SuiteEntry::index)
        .collect::<Vec<_>>();
    indices.sort_unstable();
    if indices.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(SuiteError::DuplicateIndex);
    }
    let mut prior_indices = prior.iter().map(|result| result.index).collect::<Vec<_>>();
    prior_indices.sort_unstable();
    if prior_indices.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(SuiteError::DuplicateIndex);
    }
    for result in prior {
        let valid = design.entries.iter().any(|entry| match entry {
            SuiteEntry::Position(position) => {
                result.index == position.index
                    && result.id == position.id
                    && result.expectation.as_ref() == Some(&position.expectation)
            }
            SuiteEntry::Malformed(malformed) => {
                result.index == malformed.index
                    && result.id == format!("line {}", malformed.index)
                    && result.outcome == SuiteOutcome::Malformed
            }
        });
        if !valid {
            return Err(SuiteError::InvalidResumeResult {
                index: result.index,
            });
        }
    }
    Ok(())
}

fn assess(expectation: &SuiteExpectation, best_move: &str) -> (SuiteOutcome, String) {
    match expectation {
        SuiteExpectation::Best(moves) if moves.iter().any(|value| value == best_move) => (
            SuiteOutcome::Passed,
            "best move is in the accepted bm set".into(),
        ),
        SuiteExpectation::Best(_) => (
            SuiteOutcome::Failed,
            "best move is outside the accepted bm set".into(),
        ),
        SuiteExpectation::Avoid(moves) if moves.iter().any(|value| value == best_move) => (
            SuiteOutcome::Failed,
            "best move is in the forbidden am set".into(),
        ),
        SuiteExpectation::Avoid(_) => (
            SuiteOutcome::Passed,
            "best move avoids the forbidden am set".into(),
        ),
        SuiteExpectation::None => (
            SuiteOutcome::Unscored,
            "position has no bm/am expectation".into(),
        ),
    }
}

fn summarize(design: SuiteDesign, completed: BTreeMap<u32, SuitePositionResult>) -> SuiteReport {
    let results = completed.into_values().collect::<Vec<_>>();
    let count = |outcome| {
        results
            .iter()
            .filter(|result| result.outcome == outcome)
            .count() as u32
    };
    let passed = count(SuiteOutcome::Passed);
    let failed = count(SuiteOutcome::Failed);
    let assessed = passed + failed;
    SuiteReport {
        position_set_sha256: design.position_set_sha256,
        search_sha256: design.search_sha256,
        limit: design.limit,
        deadline_ms: design.deadline_ms,
        total_entries: design.entries.len() as u32,
        searched: results
            .iter()
            .filter(|result| result.best_move.is_some())
            .count() as u32,
        assessed,
        passed,
        failed,
        unscored: count(SuiteOutcome::Unscored),
        malformed: count(SuiteOutcome::Malformed),
        pass_rate: (assessed > 0).then(|| f64::from(passed) / f64::from(assessed)),
        results,
    }
}

fn domain_error(error: SuiteError) -> ApplicationError {
    ApplicationError::ConfigurationFault(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_refuses_incompatible_work_and_finds_changes() {
        let report = |hash: &str, outcomes: [SuiteOutcome; 2]| SuiteReport {
            position_set_sha256: hash.into(),
            search_sha256: "search".into(),
            limit: SearchLimit::Depth(1),
            deadline_ms: 100,
            total_entries: 2,
            searched: 2,
            assessed: 2,
            passed: outcomes
                .iter()
                .filter(|outcome| **outcome == SuiteOutcome::Passed)
                .count() as u32,
            failed: outcomes
                .iter()
                .filter(|outcome| **outcome == SuiteOutcome::Failed)
                .count() as u32,
            unscored: 0,
            malformed: 0,
            pass_rate: Some(
                outcomes
                    .iter()
                    .filter(|outcome| **outcome == SuiteOutcome::Passed)
                    .count() as f64
                    / 2.0,
            ),
            results: outcomes
                .into_iter()
                .enumerate()
                .map(|(index, outcome)| SuitePositionResult {
                    index: index as u32 + 1,
                    id: index.to_string(),
                    outcome,
                    best_move: Some("e2e4".into()),
                    harness_elapsed_ns: Some(1),
                    expectation: None,
                    unknown_operations: Vec::new(),
                    detail: String::new(),
                })
                .collect(),
        };
        let baseline = report("positions", [SuiteOutcome::Failed, SuiteOutcome::Passed]);
        let current = report("positions", [SuiteOutcome::Passed, SuiteOutcome::Passed]);
        let comparison = compare_suite_baseline(&current, &baseline).unwrap();
        assert_eq!(comparison.passed_delta, 1);
        assert_eq!(comparison.changed_positions, vec![1]);
        assert_eq!(
            compare_suite_baseline(
                &report("other", [SuiteOutcome::Passed, SuiteOutcome::Passed]),
                &baseline
            ),
            Err(SuiteError::IncompatibleBaseline)
        );
    }
}
