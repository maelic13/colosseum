//! Application-port adapter for the concrete Tokio UCI process driver.

use std::time::Duration;

use colosseum_application::{
    ApplicationError, EngineInspection, EngineSession, EngineSessionFactory, PortFuture,
    RuntimeParticipant, SearchObservation, SearchRequest, UciOptionSchema,
};
use colosseum_core::{ParticipantId, UciOption};

use crate::{EngineProcess, GoLimits, SpawnOptions, UciError, UciPosition};

#[derive(Debug, Clone, Default)]
pub struct UciSessionFactory;

impl EngineSessionFactory for UciSessionFactory {
    fn open(
        &self,
        participant: &RuntimeParticipant,
    ) -> PortFuture<'_, Result<Box<dyn EngineSession>, ApplicationError>> {
        let id = participant.id;
        let launch = participant.launch.clone();
        Box::pin(async move {
            let process = EngineProcess::spawn(SpawnOptions {
                path: launch.executable,
                args: launch.arguments,
                working_dir: launch.working_directory,
                env: launch.environment,
            })
            .await
            .map_err(|error| engine_error(id, error))?;
            Ok(Box::new(UciSession {
                id,
                process: Some(process),
            }) as Box<dyn EngineSession>)
        })
    }
}

struct UciSession {
    id: ParticipantId,
    process: Option<EngineProcess>,
}

impl UciSession {
    fn process(&mut self) -> Result<&mut EngineProcess, ApplicationError> {
        self.process
            .as_mut()
            .ok_or_else(|| ApplicationError::InfrastructureFault {
                operation: "engine-session".into(),
                message: "session is already closed".into(),
            })
    }
}

impl EngineSession for UciSession {
    fn inspect(&mut self) -> PortFuture<'_, Result<EngineInspection, ApplicationError>> {
        let id = self.id;
        let process = self.process();
        Box::pin(async move {
            let process = process?;
            process
                .handshake(Duration::from_secs(10))
                .await
                .map_err(|error| engine_error(id, error))?;
            Ok(EngineInspection {
                name: process.name().map(str::to_owned),
                author: process.author().map(str::to_owned),
                options: process.options().iter().map(option_schema).collect(),
                diagnostics: process.transcript(),
            })
        })
    }

    fn is_ready(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
        let id = self.id;
        let process = self.process();
        Box::pin(async move {
            process?
                .is_ready(Duration::from_secs(10))
                .await
                .map_err(|error| engine_error(id, error))
        })
    }

    fn set_option(
        &mut self,
        name: &str,
        value: Option<&str>,
    ) -> PortFuture<'_, Result<(), ApplicationError>> {
        let id = self.id;
        let process = self.process();
        let name = name.to_owned();
        let value = value.map(str::to_owned);
        Box::pin(async move {
            process?
                .set_option(&name, value.as_deref())
                .await
                .map_err(|error| engine_error(id, error))
        })
    }

    fn new_game(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
        let id = self.id;
        let process = self.process();
        Box::pin(async move {
            process?
                .new_game()
                .await
                .map_err(|error| engine_error(id, error))
        })
    }

    fn search(
        &mut self,
        request: SearchRequest,
    ) -> PortFuture<'_, Result<SearchObservation, ApplicationError>> {
        let id = self.id;
        let process = self.process();
        Box::pin(async move {
            let position = if request.position == "startpos" {
                UciPosition::StartPos {
                    moves: request.moves,
                }
            } else {
                UciPosition::Fen {
                    fen: request.position,
                    moves: request.moves,
                }
            };
            let output = process?
                .search(
                    &position,
                    &GoLimits::MoveTime(Duration::from_millis(request.move_time_ms)),
                    Duration::from_millis(request.deadline_ms),
                    |_| {},
                )
                .await
                .map_err(|error| engine_error(id, error))?;
            Ok(SearchObservation {
                best_move: output.best_move,
                ponder: output.ponder,
                diagnostics: Vec::new(),
            })
        })
    }

    fn stop(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
        let id = self.id;
        let process = self.process();
        Box::pin(async move {
            process?
                .kill()
                .await
                .map_err(|error| engine_error(id, error))
        })
    }

    fn shutdown(&mut self) -> PortFuture<'_, Result<(), ApplicationError>> {
        let id = self.id;
        let process = self.process.take();
        Box::pin(async move {
            let Some(process) = process else {
                return Ok(());
            };
            process
                .quit(Duration::from_secs(2))
                .await
                .map_err(|error| engine_error(id, error))
        })
    }
}

fn option_schema(option: &UciOption) -> UciOptionSchema {
    match option {
        UciOption::Check { name, default } => UciOptionSchema::Check {
            name: name.clone(),
            default: *default,
        },
        UciOption::Spin {
            name,
            default,
            min,
            max,
        } => UciOptionSchema::Spin {
            name: name.clone(),
            default: *default,
            min: *min,
            max: *max,
        },
        UciOption::Combo {
            name,
            default,
            vars,
        } => UciOptionSchema::Combo {
            name: name.clone(),
            default: default.clone(),
            values: vars.clone(),
        },
        UciOption::Button { name } => UciOptionSchema::Button { name: name.clone() },
        UciOption::Str { name, default } => UciOptionSchema::String {
            name: name.clone(),
            default: default.clone(),
        },
    }
}

fn engine_error(participant: ParticipantId, error: UciError) -> ApplicationError {
    ApplicationError::EngineFault {
        participant,
        kind: error.to_string(),
    }
}
