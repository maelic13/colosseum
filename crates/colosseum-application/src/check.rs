use serde::{Deserialize, Serialize};

use crate::{
    ApplicationError, EngineInspection, EngineSessionFactory, PortFuture, RuntimeParticipant,
    SearchRequest, UciOptionSchema, UciOptionValue,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ComplianceStatus {
    Pass,
    Fail,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplianceCheck {
    pub requirement: String,
    pub status: ComplianceStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplianceReport {
    pub inspection: Option<EngineInspection>,
    pub checks: Vec<ComplianceCheck>,
    pub success: bool,
}

impl ComplianceReport {
    fn finish(mut self) -> Self {
        self.success = self
            .checks
            .iter()
            .all(|check| check.status == ComplianceStatus::Pass);
        self
    }

    fn push(&mut self, requirement: &str, status: ComplianceStatus, detail: impl Into<String>) {
        self.checks.push(ComplianceCheck {
            requirement: requirement.into(),
            status,
            detail: detail.into(),
        });
    }
}

pub struct CheckEngine;

impl CheckEngine {
    pub fn execute<'a>(
        sessions: &'a dyn EngineSessionFactory,
        participant: &'a RuntimeParticipant,
    ) -> PortFuture<'a, Result<ComplianceReport, ApplicationError>> {
        Box::pin(async move {
            let mut session = sessions.open(participant).await?;
            let mut report = ComplianceReport {
                inspection: None,
                checks: Vec::new(),
                success: false,
            };

            let inspection = match session.inspect().await {
                Ok(inspection) => {
                    report.push("handshake", ComplianceStatus::Pass, "received uciok");
                    report.inspection = Some(inspection.clone());
                    inspection
                }
                Err(error) => {
                    report.push("handshake", ComplianceStatus::Fail, error.to_string());
                    for requirement in [
                        "synchronization",
                        "option-schema-validation",
                        "option-acceptance",
                        "bounded-legal-search",
                        "stop",
                        "new-game",
                    ] {
                        report.push(
                            requirement,
                            ComplianceStatus::Skipped,
                            "handshake did not complete",
                        );
                    }
                    shutdown(&mut report, &mut *session).await;
                    return Ok(report.finish());
                }
            };

            let ready = record_result(
                &mut report,
                "synchronization",
                "received readyok",
                session.is_ready().await,
            );

            let schema = validate_options(&participant.launch.options, &inspection.options);
            match &schema {
                Ok(()) => report.push(
                    "option-schema-validation",
                    ComplianceStatus::Pass,
                    "all requested values match advertised option schemas",
                ),
                Err(detail) => {
                    report.push("option-schema-validation", ComplianceStatus::Fail, detail)
                }
            }

            let mut options_accepted = false;
            if ready && schema.is_ok() {
                let mut acceptance = Ok(());
                for (name, value) in &participant.launch.options {
                    let command_value = value.command_value();
                    if let Err(error) = session.set_option(name, command_value.as_deref()).await {
                        acceptance = Err(error);
                        break;
                    }
                }
                if acceptance.is_ok() {
                    acceptance = session.is_ready().await;
                }
                options_accepted = record_result(
                    &mut report,
                    "option-acceptance",
                    "setoption commands caused no failure and were followed by readyok; UCI provides no read-back",
                    acceptance,
                );
            } else {
                report.push(
                    "option-acceptance",
                    ComplianceStatus::Skipped,
                    "readiness or schema validation failed",
                );
            }

            let usable = ready && options_accepted;
            let mut search_ok = false;
            if usable {
                match session.search(short_search()).await {
                    Ok(observation) if is_legal_start_move(&observation.best_move) => {
                        report.push(
                            "bounded-legal-search",
                            ComplianceStatus::Pass,
                            format!("legal bestmove {}", observation.best_move),
                        );
                        search_ok = true;
                    }
                    Ok(observation) => report.push(
                        "bounded-legal-search",
                        ComplianceStatus::Fail,
                        format!("illegal start-position bestmove {}", observation.best_move),
                    ),
                    Err(error) => report.push(
                        "bounded-legal-search",
                        ComplianceStatus::Fail,
                        error.to_string(),
                    ),
                }
            } else {
                report.push(
                    "bounded-legal-search",
                    ComplianceStatus::Skipped,
                    "engine is not ready with validated options",
                );
            }

            if usable && search_ok {
                let stop_result = match session.start_search(long_search()).await {
                    Ok(()) => session.stop(2_000).await.map(|_| ()),
                    Err(error) => Err(error),
                };
                record_result(
                    &mut report,
                    "stop",
                    "stop produced bestmove within 2000 ms",
                    stop_result,
                );
            } else {
                report.push(
                    "stop",
                    ComplianceStatus::Skipped,
                    "bounded search did not pass",
                );
            }

            if usable {
                let new_game = match session.new_game().await {
                    Ok(()) => session.is_ready().await,
                    Err(error) => Err(error),
                };
                record_result(
                    &mut report,
                    "new-game",
                    "ucinewgame was followed by readyok",
                    new_game,
                );
            } else {
                report.push(
                    "new-game",
                    ComplianceStatus::Skipped,
                    "engine is not ready with validated options",
                );
            }

            shutdown(&mut report, &mut *session).await;
            Ok(report.finish())
        })
    }
}

async fn shutdown(report: &mut ComplianceReport, session: &mut dyn crate::EngineSession) {
    record_result(
        report,
        "clean-shutdown",
        "quit completed within the adapter deadline",
        session.shutdown().await,
    );
}

fn record_result(
    report: &mut ComplianceReport,
    requirement: &str,
    success: &str,
    result: Result<(), ApplicationError>,
) -> bool {
    match result {
        Ok(()) => {
            report.push(requirement, ComplianceStatus::Pass, success);
            true
        }
        Err(error) => {
            report.push(requirement, ComplianceStatus::Fail, error.to_string());
            false
        }
    }
}

fn short_search() -> SearchRequest {
    SearchRequest {
        position: "startpos".into(),
        moves: Vec::new(),
        move_time_ms: 25,
        deadline_ms: 2_000,
    }
}

fn long_search() -> SearchRequest {
    SearchRequest {
        move_time_ms: 10_000,
        ..short_search()
    }
}

fn validate_options(
    requested: &std::collections::BTreeMap<String, UciOptionValue>,
    schemas: &[UciOptionSchema],
) -> Result<(), String> {
    let mut failures = Vec::new();
    for (name, value) in requested {
        let Some(schema) = schemas.iter().find(|schema| schema.name() == name) else {
            failures.push(format!("option {name:?} was not advertised"));
            continue;
        };
        let valid = match (value, schema) {
            (UciOptionValue::Check(_), UciOptionSchema::Check { .. })
            | (UciOptionValue::String(_), UciOptionSchema::String { .. })
            | (UciOptionValue::Button, UciOptionSchema::Button { .. }) => true,
            (UciOptionValue::Spin(value), UciOptionSchema::Spin { min, max, .. }) => {
                value >= min && value <= max
            }
            (UciOptionValue::Combo(value), UciOptionSchema::Combo { values, .. }) => {
                values.contains(value)
            }
            // CLI values begin as strings because the live schema is the
            // authority. Parse those strings according to that schema here.
            (UciOptionValue::String(value), UciOptionSchema::Check { .. }) => {
                matches!(value.as_str(), "true" | "false")
            }
            (UciOptionValue::String(value), UciOptionSchema::Spin { min, max, .. }) => value
                .parse::<i64>()
                .is_ok_and(|value| value >= *min && value <= *max),
            (UciOptionValue::String(value), UciOptionSchema::Combo { values, .. }) => {
                values.contains(value)
            }
            _ => false,
        };
        if !valid {
            failures.push(format!(
                "value for option {name:?} does not match advertised schema"
            ));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

fn is_legal_start_move(best_move: &str) -> bool {
    const KNIGHT_MOVES: &[&str] = &["b1a3", "b1c3", "g1f3", "g1h3"];
    KNIGHT_MOVES.contains(&best_move)
        || (best_move.len() == 4 && {
            let bytes = best_move.as_bytes();
            bytes[0].is_ascii_lowercase()
                && (b'a'..=b'h').contains(&bytes[0])
                && bytes[1] == b'2'
                && bytes[2] == bytes[0]
                && matches!(bytes[3], b'3' | b'4')
        })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    use colosseum_core::ParticipantId;

    use super::*;
    use crate::{EngineLaunchSpec, EngineSession, PortFuture, SearchObservation};

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        let mut future = Box::pin(future);
        let mut context = Context::from_waker(Waker::noop());
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    struct FakeFactory {
        inspection: EngineInspection,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl EngineSessionFactory for FakeFactory {
        fn open(
            &self,
            _participant: &RuntimeParticipant,
        ) -> PortFuture<'_, Result<Box<dyn EngineSession>, ApplicationError>> {
            let session = FakeSession {
                inspection: self.inspection.clone(),
                calls: Arc::clone(&self.calls),
            };
            Box::pin(async move { Ok(Box::new(session) as Box<dyn EngineSession>) })
        }
    }

    struct FakeSession {
        inspection: EngineInspection,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl FakeSession {
        fn call(&self, name: impl Into<String>) {
            self.calls.lock().unwrap().push(name.into());
        }
    }

    impl EngineSession for FakeSession {
        fn inspect(&mut self) -> PortFuture<'_, Result<EngineInspection, ApplicationError>> {
            self.call("inspect");
            let inspection = self.inspection.clone();
            Box::pin(async move { Ok(inspection) })
        }

        fn is_ready(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
            self.call("ready");
            Box::pin(async { Ok(()) })
        }

        fn set_option(
            &mut self,
            name: &str,
            value: Option<&str>,
        ) -> PortFuture<'_, Result<(), ApplicationError>> {
            self.call(format!("option:{name}={}", value.unwrap_or("<button>")));
            Box::pin(async { Ok(()) })
        }

        fn new_game(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
            self.call("new-game");
            Box::pin(async { Ok(()) })
        }

        fn search(
            &mut self,
            _request: SearchRequest,
        ) -> PortFuture<'_, Result<SearchObservation, ApplicationError>> {
            self.call("search");
            Box::pin(async {
                Ok(SearchObservation {
                    best_move: "e2e4".into(),
                    ponder: None,
                    diagnostics: Vec::new(),
                })
            })
        }

        fn start_search(
            &mut self,
            _request: SearchRequest,
        ) -> PortFuture<'_, Result<(), ApplicationError>> {
            self.call("start-search");
            Box::pin(async { Ok(()) })
        }

        fn stop(
            &mut self,
            _deadline_ms: u64,
        ) -> PortFuture<'_, Result<SearchObservation, ApplicationError>> {
            self.call("stop");
            Box::pin(async {
                Ok(SearchObservation {
                    best_move: "g1f3".into(),
                    ponder: None,
                    diagnostics: Vec::new(),
                })
            })
        }

        fn shutdown(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
            self.call("shutdown");
            Box::pin(async { Ok(()) })
        }
    }

    fn participant(hash: &str) -> RuntimeParticipant {
        let mut launch = EngineLaunchSpec::path_only("stub".into());
        launch
            .options
            .insert("Hash".into(), UciOptionValue::String(hash.into()));
        RuntimeParticipant {
            id: ParticipantId::from_u128(1),
            launch,
        }
    }

    fn factory() -> FakeFactory {
        FakeFactory {
            inspection: EngineInspection {
                name: Some("Stub".into()),
                options: vec![UciOptionSchema::Spin {
                    name: "Hash".into(),
                    default: 16,
                    min: 1,
                    max: 256,
                }],
                ..EngineInspection::default()
            },
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[test]
    fn complete_compliance_sequence_passes_and_never_claims_readback() {
        let factory = factory();
        let report = block_on(CheckEngine::execute(&factory, &participant("128"))).unwrap();
        assert!(report.success);
        assert!(
            report
                .checks
                .iter()
                .all(|check| check.status == ComplianceStatus::Pass)
        );
        let acceptance = report
            .checks
            .iter()
            .find(|check| check.requirement == "option-acceptance")
            .unwrap();
        assert!(acceptance.detail.contains("no read-back"));
        assert_eq!(
            *factory.calls.lock().unwrap(),
            [
                "inspect",
                "ready",
                "option:Hash=128",
                "ready",
                "search",
                "start-search",
                "stop",
                "new-game",
                "ready",
                "shutdown"
            ]
        );
    }

    #[test]
    fn invalid_requested_value_fails_schema_without_sending_it() {
        let factory = factory();
        let report = block_on(CheckEngine::execute(&factory, &participant("999"))).unwrap();
        assert!(!report.success);
        assert_eq!(
            report
                .checks
                .iter()
                .find(|check| check.requirement == "option-schema-validation")
                .unwrap()
                .status,
            ComplianceStatus::Fail
        );
        assert!(
            !factory
                .calls
                .lock()
                .unwrap()
                .iter()
                .any(|call| call.starts_with("option:"))
        );
    }

    #[test]
    fn legal_start_move_accepts_only_the_twenty_initial_moves() {
        for legal in ["a2a3", "h2h4", "b1a3", "b1c3", "g1f3", "g1h3"] {
            assert!(is_legal_start_move(legal));
        }
        for illegal in ["e7e5", "e2e5", "0000", "(none)", "e2e4q"] {
            assert!(!is_legal_start_move(illegal));
        }
    }
}
