use crate::{
    ApplicationError, CompletedUnit, PortFuture, ProgressEvent, ProgressSink, RunRepository,
};

pub struct CommitUnitDependencies<'a> {
    pub repository: &'a dyn RunRepository,
    pub progress: &'a dyn ProgressSink,
}

pub struct CommitUnit;

impl CommitUnit {
    pub fn execute<'a>(
        dependencies: CommitUnitDependencies<'a>,
        completed: &'a CompletedUnit,
    ) -> PortFuture<'a, Result<crate::CommittedRunSnapshot, ApplicationError>> {
        Box::pin(async move {
            let snapshot = dependencies.repository.commit_unit(completed).await?;
            dependencies.progress.publish(ProgressEvent::UnitCommitted {
                unit_id: completed.unit.id,
                snapshot: snapshot.clone(),
            });
            Ok(snapshot)
        })
    }
}
