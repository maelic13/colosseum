use crate::{
    ApplicationError, EngineInspection, EngineSessionFactory, PortFuture, RuntimeParticipant,
};

/// Framework-independent UCI inspection orchestration.
pub struct InspectEngine;

impl InspectEngine {
    pub fn execute<'a>(
        sessions: &'a dyn EngineSessionFactory,
        participant: &'a RuntimeParticipant,
    ) -> PortFuture<'a, Result<EngineInspection, ApplicationError>> {
        Box::pin(async move {
            let mut session = sessions.open(participant).await?;
            let inspection = session.inspect().await?;
            session.is_ready().await?;
            session.shutdown().await?;
            Ok(inspection)
        })
    }
}
