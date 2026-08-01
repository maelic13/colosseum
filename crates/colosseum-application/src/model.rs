use std::collections::BTreeMap;
use std::path::PathBuf;

use colosseum_core::{GameId, ParticipantId, RunId, UnitId};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Portable identity of one operating-system logical processor.
///
/// Windows processor numbers are only unique within a processor group. Linux
/// and macOS use group zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct LogicalCpuId {
    pub group: u16,
    pub number: u32,
}

impl From<u32> for LogicalCpuId {
    fn from(number: u32) -> Self {
        Self { group: 0, number }
    }
}

/// A configured UCI option value at the application boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "kebab-case")]
pub enum UciOptionValue {
    Check(bool),
    Spin(i64),
    Combo(String),
    String(String),
    Button,
}

impl UciOptionValue {
    #[must_use]
    pub fn command_value(&self) -> Option<String> {
        match self {
            Self::Check(value) => Some(value.to_string()),
            Self::Spin(value) => Some(value.to_string()),
            Self::Combo(value) | Self::String(value) => Some(value.clone()),
            Self::Button => None,
        }
    }
}

/// One UCI option advertised during handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum UciOptionSchema {
    Check {
        name: String,
        default: bool,
    },
    Spin {
        name: String,
        default: i64,
        min: i64,
        max: i64,
    },
    Combo {
        name: String,
        default: String,
        values: Vec<String>,
    },
    Button {
        name: String,
    },
    String {
        name: String,
        default: String,
    },
}

impl UciOptionSchema {
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Check { name, .. }
            | Self::Spin { name, .. }
            | Self::Combo { name, .. }
            | Self::Button { name }
            | Self::String { name, .. } => name,
        }
    }
}

/// Resolved logical CPU allocation for one engine process.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "cpus", rename_all = "kebab-case")]
pub enum CpuAllocation {
    #[default]
    Unrestricted,
    Advisory(Vec<LogicalCpuId>),
    Enforced(Vec<LogicalCpuId>),
}

/// Minimal, resolved instructions for launching an ordinary UCI executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineLaunchSpec {
    pub executable: PathBuf,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default)]
    pub working_directory: Option<PathBuf>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub options: BTreeMap<String, UciOptionValue>,
    #[serde(default)]
    pub allocated_cpus: CpuAllocation,
}

impl EngineLaunchSpec {
    #[must_use]
    pub fn path_only(executable: PathBuf) -> Self {
        Self {
            executable,
            arguments: Vec::new(),
            working_directory: None,
            environment: BTreeMap::new(),
            label: None,
            options: BTreeMap::new(),
            allocated_cpus: CpuAllocation::Unrestricted,
        }
    }
}

/// A run-local participant; GUI library identity and UCI identity are separate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeParticipant {
    pub id: ParticipantId,
    pub launch: EngineLaunchSpec,
}

/// Identity and schema observed through a normal UCI handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EngineInspection {
    pub name: Option<String>,
    pub author: Option<String>,
    pub options: Vec<UciOptionSchema>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchRequest {
    pub position: String,
    pub moves: Vec<String>,
    pub move_time_ms: u64,
    pub deadline_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchObservation {
    pub best_move: String,
    pub ponder: Option<String>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionUnit {
    pub id: UnitId,
    pub game_id: Option<GameId>,
    pub sequence: u64,
    pub payload: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitOutcome {
    Completed {
        result: String,
    },
    EngineFault {
        participant: ParticipantId,
        kind: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedUnit {
    pub run_id: RunId,
    pub unit: ExecutionUnit,
    pub outcome: UnitOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunState {
    Created,
    Running,
    Stopped,
    Finished,
    Cancelled,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommittedRunSnapshot {
    pub run_id: RunId,
    pub durable_sequence: u64,
    pub completed_units: u64,
    pub failed_units: u64,
    pub state: RunState,
    pub anomalies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgressEvent {
    UnitCommitted {
        unit_id: UnitId,
        snapshot: CommittedRunSnapshot,
    },
    StateChanged(CommittedRunSnapshot),
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ApplicationError {
    #[error("configuration fault: {0}")]
    ConfigurationFault(String),
    #[error("engine fault for {participant}: {kind}")]
    EngineFault {
        participant: ParticipantId,
        kind: String,
    },
    #[error("infrastructure fault during {operation}: {message}")]
    InfrastructureFault { operation: String, message: String },
    #[error("operation cancelled")]
    Cancelled,
    #[error("domain error: {0}")]
    DomainError(String),
}
