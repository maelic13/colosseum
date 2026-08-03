//! Durable SPSA schedule adapter.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use colosseum_application::{SpsaPreflightError, VerifiedSpsaSchedule};
use colosseum_core::SpsaScheduleArtifact;
use thiserror::Error;

use crate::RunDirectory;

pub const SPSA_SCHEDULE_FILE: &str = "spsa-schedule.json";
static SCHEDULE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Persist a new schedule without replacing resume authority, read the bytes
/// back, validate every derived/reproducibility field, and return the only
/// token an SPSA game driver may accept.
pub fn persist_and_verify_spsa_schedule(
    directory: &RunDirectory,
    expected: &SpsaScheduleArtifact,
) -> Result<VerifiedSpsaSchedule, SpsaScheduleStoreError> {
    expected.validate().map_err(SpsaPreflightError::from)?;
    let path = directory.paths().root.join(SPSA_SCHEDULE_FILE);
    if !path.exists() {
        let mut bytes =
            serde_json::to_vec_pretty(expected).map_err(|source| SpsaScheduleStoreError::Json {
                operation: "serialize expected schedule",
                path: path.clone(),
                source,
            })?;
        bytes.push(b'\n');
        let temporary = directory.paths().root.join(format!(
            ".spsa-schedule.{}.{}.tmp",
            std::process::id(),
            SCHEDULE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|source| SpsaScheduleStoreError::Io {
                    operation: "create schedule temporary",
                    path: temporary.clone(),
                    source,
                })?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|source| SpsaScheduleStoreError::Io {
                    operation: "write schedule temporary",
                    path: temporary.clone(),
                    source,
                })?;
            fs::rename(&temporary, &path).map_err(|source| SpsaScheduleStoreError::Io {
                operation: "publish schedule",
                path: path.clone(),
                source,
            })
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result?;
    }

    let bytes = fs::read(&path).map_err(|source| SpsaScheduleStoreError::Io {
        operation: "read written schedule",
        path: path.clone(),
        source,
    })?;
    let written =
        serde_json::from_slice(&bytes).map_err(|source| SpsaScheduleStoreError::Json {
            operation: "parse written schedule",
            path: path.clone(),
            source,
        })?;
    VerifiedSpsaSchedule::verify_written(expected, written).map_err(Into::into)
}

/// Read and verify an existing schedule without creating or replacing files.
/// Command-specific status uses this path so observation cannot take ownership
/// of, repair or otherwise mutate a live run.
pub fn read_and_verify_spsa_schedule(
    root: &Path,
    expected: &SpsaScheduleArtifact,
) -> Result<VerifiedSpsaSchedule, SpsaScheduleStoreError> {
    let path = root.join(SPSA_SCHEDULE_FILE);
    let bytes = fs::read(&path).map_err(|source| SpsaScheduleStoreError::Io {
        operation: "read written schedule",
        path: path.clone(),
        source,
    })?;
    let written =
        serde_json::from_slice(&bytes).map_err(|source| SpsaScheduleStoreError::Json {
            operation: "parse written schedule",
            path,
            source,
        })?;
    VerifiedSpsaSchedule::verify_written(expected, written).map_err(Into::into)
}

#[derive(Debug, Error)]
pub enum SpsaScheduleStoreError {
    #[error("SPSA schedule preflight failed: {0}")]
    Preflight(#[from] SpsaPreflightError),
    #[error("could not {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not {operation} at {path}: {source}")]
    Json {
        operation: &'static str,
        path: PathBuf,
        source: serde_json::Error,
    },
}
