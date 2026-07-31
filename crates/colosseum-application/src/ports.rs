use std::future::Future;
use std::pin::Pin;

use colosseum_core::{GameId, ParticipantId, RunId, UnitId};

use crate::model::{
    ApplicationError, CommittedRunSnapshot, CompletedUnit, EngineInspection, EngineLaunchSpec,
    ExecutionUnit, ProgressEvent, RuntimeParticipant, SearchObservation, SearchRequest,
};

pub type PortFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait EngineSession: Send {
    fn inspect(&mut self) -> PortFuture<'_, Result<EngineInspection, ApplicationError>>;
    fn is_ready(&mut self) -> PortFuture<'_, Result<(), ApplicationError>>;
    fn set_option(
        &mut self,
        name: &str,
        value: Option<&str>,
    ) -> PortFuture<'_, Result<(), ApplicationError>>;
    fn new_game(&mut self) -> PortFuture<'_, Result<(), ApplicationError>>;
    fn search(
        &mut self,
        request: SearchRequest,
    ) -> PortFuture<'_, Result<SearchObservation, ApplicationError>>;
    fn stop(&mut self) -> PortFuture<'_, Result<(), ApplicationError>>;
    fn shutdown(&mut self) -> PortFuture<'_, Result<(), ApplicationError>>;
}

pub trait EngineSessionFactory: Send + Sync {
    fn open(
        &self,
        participant: &RuntimeParticipant,
    ) -> PortFuture<'_, Result<Box<dyn EngineSession>, ApplicationError>>;
}

pub trait GameExecutor: Send + Sync {
    fn execute(
        &self,
        unit: ExecutionUnit,
    ) -> PortFuture<'_, Result<CompletedUnit, ApplicationError>>;
}

pub trait ExecutionPool: Send + Sync {
    fn execute_all(
        &self,
        units: Vec<ExecutionUnit>,
    ) -> PortFuture<'_, Result<Vec<CompletedUnit>, ApplicationError>>;
}

pub trait RunRepository: Send + Sync {
    fn commit_unit(
        &self,
        completed: &CompletedUnit,
    ) -> PortFuture<'_, Result<CommittedRunSnapshot, ApplicationError>>;
    fn snapshot(
        &self,
        run_id: RunId,
    ) -> PortFuture<'_, Result<CommittedRunSnapshot, ApplicationError>>;
}

pub trait ArtifactSink: Send + Sync {
    fn append(
        &self,
        logical_name: &str,
        bytes: &[u8],
        required: bool,
    ) -> PortFuture<'_, Result<(), ApplicationError>>;
    fn write_atomic(
        &self,
        logical_name: &str,
        bytes: &[u8],
        required: bool,
    ) -> PortFuture<'_, Result<(), ApplicationError>>;
}

pub trait OpeningSource: Send + Sync {
    fn resolve(&self, identity: &str) -> Result<Vec<String>, ApplicationError>;
}

pub trait CpuPlacement: Send + Sync {
    fn apply(&self, participant: ParticipantId, cpus: &[u32]) -> Result<(), ApplicationError>;
}

pub trait Clock: Send + Sync {
    fn monotonic_ticks(&self) -> u64;
    fn monotonic_resolution_ns(&self) -> u64;
    fn utc_timestamp(&self) -> String;
}

pub trait IdGenerator: Send + Sync {
    fn run_id(&self) -> RunId;
    fn participant_id(&self) -> ParticipantId;
    fn game_id(&self) -> GameId;
    fn unit_id(&self) -> UnitId;
}

pub trait MasterSeedSource: Send + Sync {
    fn master_seed(&self) -> Result<u64, ApplicationError>;
}

pub trait ProgressSink: Send + Sync {
    fn publish(&self, event: ProgressEvent);
}

pub trait Cancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

pub trait EngineExecutableResolver: Send + Sync {
    fn resolve(&self, input: &EngineLaunchSpec) -> Result<EngineLaunchSpec, ApplicationError>;
}
