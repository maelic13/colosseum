use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ApplicationError, EngineSessionFactory, PortFuture, RuntimeParticipant, SearchLimit,
    SearchObservation, SearchRequest,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpsRequest {
    pub nodes: u64,
    pub position: String,
    pub moves: Vec<String>,
    pub deadline_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpsReport {
    pub requested_nodes: u64,
    pub reported_nodes: u64,
    pub harness_elapsed_ns: u64,
    pub authoritative_nps: f64,
    pub engine_reported_time_ms: Option<u64>,
    pub engine_reported_nps: Option<u64>,
    pub best_move: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NpsError {
    #[error("fixed-node workload must contain at least one node")]
    ZeroNodes,
    #[error("fixed-node deadline must be at least one millisecond")]
    ZeroDeadline,
    #[error("engine did not report info nodes; fixed work cannot be verified")]
    MissingReportedNodes,
    #[error("engine stopped at {reported} reported nodes before the requested {requested}")]
    IncompleteWork { requested: u64, reported: u64 },
    #[error("the monotonic harness interval had zero duration")]
    ZeroElapsed,
}

pub struct MeasureNps;

impl MeasureNps {
    pub fn execute<'a>(
        sessions: &'a dyn EngineSessionFactory,
        participant: &'a RuntimeParticipant,
        request: NpsRequest,
    ) -> PortFuture<'a, Result<NpsReport, ApplicationError>> {
        Box::pin(async move {
            validate_request(&request).map_err(domain_error)?;
            let mut session = sessions.open(participant).await?;
            let measured = async {
                session.inspect().await?;
                for (name, value) in &participant.launch.options {
                    let value = value.command_value();
                    session.set_option(name, value.as_deref()).await?;
                }
                session.is_ready().await?;
                session.new_game().await?;
                session.is_ready().await?;
                let requested_nodes = request.nodes;
                let observation = session
                    .search(SearchRequest {
                        position: request.position,
                        moves: request.moves,
                        limit: SearchLimit::Nodes(requested_nodes),
                        deadline_ms: request.deadline_ms,
                    })
                    .await?;
                report(requested_nodes, observation).map_err(domain_error)
            }
            .await;
            let shutdown = session.shutdown().await;
            match (measured, shutdown) {
                (Err(error), _) | (Ok(_), Err(error)) => Err(error),
                (Ok(report), Ok(())) => Ok(report),
            }
        })
    }
}

fn validate_request(request: &NpsRequest) -> Result<(), NpsError> {
    if request.nodes == 0 {
        return Err(NpsError::ZeroNodes);
    }
    if request.deadline_ms == 0 {
        return Err(NpsError::ZeroDeadline);
    }
    Ok(())
}

fn report(requested_nodes: u64, observation: SearchObservation) -> Result<NpsReport, NpsError> {
    let reported_nodes = observation
        .reported_nodes
        .ok_or(NpsError::MissingReportedNodes)?;
    if reported_nodes < requested_nodes {
        return Err(NpsError::IncompleteWork {
            requested: requested_nodes,
            reported: reported_nodes,
        });
    }
    if observation.harness_elapsed_ns == 0 {
        return Err(NpsError::ZeroElapsed);
    }
    let authoritative_nps =
        requested_nodes as f64 * 1_000_000_000.0 / observation.harness_elapsed_ns as f64;
    Ok(NpsReport {
        requested_nodes,
        reported_nodes,
        harness_elapsed_ns: observation.harness_elapsed_ns,
        authoritative_nps,
        engine_reported_time_ms: observation.reported_time_ms,
        engine_reported_nps: observation.reported_nps,
        best_move: observation.best_move,
    })
}

fn domain_error(error: NpsError) -> ApplicationError {
    ApplicationError::DomainError(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(reported_nps: u64) -> SearchObservation {
        SearchObservation {
            best_move: "e2e4".into(),
            ponder: None,
            reported_nodes: Some(1_000_010),
            reported_time_ms: Some(1),
            reported_nps: Some(reported_nps),
            harness_elapsed_ns: 500_000_000,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn authoritative_speed_uses_requested_work_and_harness_time_only() {
        let low_claim = report(1_000_000, observation(1)).unwrap();
        let high_claim = report(1_000_000, observation(u64::MAX)).unwrap();
        assert_eq!(low_claim.authoritative_nps, 2_000_000.0);
        assert_eq!(low_claim.authoritative_nps, high_claim.authoritative_nps);
        assert_ne!(
            low_claim.engine_reported_nps,
            high_claim.engine_reported_nps
        );
    }

    #[test]
    fn reported_nodes_must_prove_the_fixed_work_completed() {
        let mut missing = observation(10);
        missing.reported_nodes = None;
        assert_eq!(
            report(1_000_000, missing),
            Err(NpsError::MissingReportedNodes)
        );

        let mut short = observation(10);
        short.reported_nodes = Some(999_999);
        assert_eq!(
            report(1_000_000, short),
            Err(NpsError::IncompleteWork {
                requested: 1_000_000,
                reported: 999_999,
            })
        );
    }
}
