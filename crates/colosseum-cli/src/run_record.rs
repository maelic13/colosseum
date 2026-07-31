//! Versioned official state read by the common `status` command.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::RunDirectory;

pub const RUN_RECORD_SCHEMA_VERSION: u64 = 1;
static RECORD_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Running,
    Completed,
    Cancelled,
    Aborted,
    Invalid,
}

impl RunStatus {
    #[must_use]
    pub fn is_terminal(self) -> bool {
        !matches!(self, Self::Running)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OfficialSample {
    pub committed_units: u64,
    pub scored_games: u64,
    pub completed_pairs: u64,
    pub pentanomial: [u64; 5],
    pub unpaired_games: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CapabilityLevel {
    Enforced,
    Available,
    Unavailable,
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostSummary {
    pub operating_system: String,
    pub architecture: String,
    pub logical_cpus_visible: usize,
    pub capabilities: BTreeMap<String, CapabilityLevel>,
}

impl HostSummary {
    #[must_use]
    pub fn current() -> Self {
        let mut capabilities = BTreeMap::new();
        capabilities.insert("process-tree-containment".into(), CapabilityLevel::Enforced);
        capabilities.insert("bounded-engine-pipes".into(), CapabilityLevel::Enforced);
        capabilities.insert("cpu-affinity".into(), CapabilityLevel::Deferred);
        Self {
            operating_system: std::env::consts::OS.into(),
            architecture: std::env::consts::ARCH.into(),
            logical_cpus_visible: std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(0),
            capabilities,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anomaly {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub schema_version: u64,
    pub stats_version: u32,
    pub product_version: String,
    pub command: String,
    pub config_sha256: String,
    pub status: RunStatus,
    pub started_unix_ms: u64,
    pub updated_unix_ms: u64,
    pub official_sample: OfficialSample,
    pub host: HostSummary,
    pub anomalies: Vec<Anomaly>,
}

impl RunRecord {
    fn new(directory: &RunDirectory, command: &str) -> Self {
        let now = unix_ms();
        Self {
            schema_version: RUN_RECORD_SCHEMA_VERSION,
            stats_version: colosseum_core::rng::RNG_VERSION,
            product_version: env!("CARGO_PKG_VERSION").into(),
            command: command.into(),
            config_sha256: directory.config_sha256().into(),
            status: RunStatus::Running,
            started_unix_ms: now,
            updated_unix_ms: now,
            official_sample: OfficialSample::default(),
            host: HostSummary::current(),
            anomalies: Vec::new(),
        }
    }

    pub fn read(root: &Path) -> Result<Self, RunRecordError> {
        let path = root.join("run-record.json");
        let bytes = fs::read(&path).map_err(|source| RunRecordError::Io {
            operation: "read run record",
            path: path.clone(),
            source,
        })?;
        let record: Self = serde_json::from_slice(&bytes)?;
        if record.schema_version != RUN_RECORD_SCHEMA_VERSION {
            return Err(RunRecordError::UnsupportedSchema {
                path,
                version: record.schema_version,
            });
        }
        Ok(record)
    }
}

/// Lifecycle owner. Dropping a still-running recorder persists an aborted state.
#[derive(Debug)]
pub struct RunRecorder {
    path: PathBuf,
    record: RunRecord,
}

impl RunRecorder {
    pub fn begin(directory: &RunDirectory, command: &str) -> Result<Self, RunRecordError> {
        let mut recorder = Self {
            path: directory.paths().root.join("run-record.json"),
            record: RunRecord::new(directory, command),
        };
        recorder.persist()?;
        Ok(recorder)
    }

    #[must_use]
    pub fn record(&self) -> &RunRecord {
        &self.record
    }

    pub fn update_sample(&mut self, sample: OfficialSample) -> Result<(), RunRecordError> {
        self.require_running()?;
        self.record.official_sample = sample;
        self.touch();
        self.persist()
    }

    pub fn add_anomaly(
        &mut self,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Result<(), RunRecordError> {
        self.record.anomalies.push(Anomaly {
            code: code.into(),
            message: message.into(),
        });
        self.touch();
        self.persist()
    }

    pub fn finish(mut self, status: RunStatus) -> Result<(), RunRecordError> {
        if !status.is_terminal() {
            return Err(RunRecordError::NonTerminalFinish);
        }
        self.require_running()?;
        self.record.status = status;
        self.touch();
        self.persist()
    }

    fn require_running(&self) -> Result<(), RunRecordError> {
        if self.record.status == RunStatus::Running {
            Ok(())
        } else {
            Err(RunRecordError::AlreadyTerminal(self.record.status))
        }
    }

    fn touch(&mut self) {
        self.record.updated_unix_ms = unix_ms();
    }

    fn persist(&mut self) -> Result<(), RunRecordError> {
        write_atomic(&self.path, &self.record)
    }
}

impl Drop for RunRecorder {
    fn drop(&mut self) {
        if self.record.status == RunStatus::Running {
            self.record.status = RunStatus::Aborted;
            self.record.anomalies.push(Anomaly {
                code: "workflow-owner-dropped".into(),
                message: "workflow ended without an explicit terminal transition".into(),
            });
            self.touch();
            let _ = self.persist();
        }
    }
}

#[derive(Debug, Error)]
pub enum RunRecordError {
    #[error("run-record JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported run-record schema {version} at {path}")]
    UnsupportedSchema { path: PathBuf, version: u64 },
    #[error("cannot finish a run with running status")]
    NonTerminalFinish,
    #[error("run is already terminal: {0:?}")]
    AlreadyTerminal(RunStatus),
    #[error("could not {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
}

fn write_atomic(path: &Path, value: &RunRecord) -> Result<(), RunRecordError> {
    let bytes = serde_json::to_vec(value)?;
    let parent = path.parent().expect("run-record path has a parent");
    let temporary = parent.join(format!(
        ".run-record.{}.{}.tmp",
        std::process::id(),
        RECORD_SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|source| RunRecordError::Io {
                operation: "create run-record temporary",
                path: temporary.clone(),
                source,
            })?;
        file.write_all(&bytes)
            .and_then(|()| file.sync_all())
            .map_err(|source| RunRecordError::Io {
                operation: "write run-record temporary",
                path: temporary.clone(),
                source,
            })?;
        replace_atomic(&temporary, path).map_err(|source| RunRecordError::Io {
            operation: "publish run record",
            path: path.to_path_buf(),
            source,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(not(windows))]
fn replace_atomic(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_atomic(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    // SAFETY: both paths are valid, nul-terminated UTF-16 buffers for this call.
    if unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
