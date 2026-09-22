//! Durable SPSA schedule adapter.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use colosseum_application::{SpsaPreflightError, VerifiedSpsaSchedule};
use colosseum_core::{SPSA_SCHEDULE_SCHEMA_VERSION, SpsaError, SpsaScheduleArtifact};
use serde_json::Value;
use thiserror::Error;

use crate::RunDirectory;
use crate::versioned_artifact::require_schema_version;

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
    let written = parse_schedule(&bytes, &path)?;
    VerifiedSpsaSchedule::verify_written(expected, written).map_err(Into::into)
}

/// Read a stored schedule, naming an unreadable schema version before any
/// field of that version can be reported instead.
fn parse_schedule(
    bytes: &[u8],
    path: &Path,
) -> Result<SpsaScheduleArtifact, SpsaScheduleStoreError> {
    let document: Value =
        serde_json::from_slice(bytes).map_err(|source| SpsaScheduleStoreError::Json {
            operation: "parse written schedule",
            path: path.to_owned(),
            source,
        })?;
    require_schema_version(&document, SPSA_SCHEDULE_SCHEMA_VERSION).map_err(|version| {
        SpsaScheduleStoreError::Preflight(SpsaPreflightError::from(
            SpsaError::UnsupportedArtifactSchema { version },
        ))
    })?;
    serde_json::from_value(document).map_err(|source| SpsaScheduleStoreError::Json {
        operation: "parse written schedule",
        path: path.to_owned(),
        source,
    })
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
    let written = parse_schedule(&bytes, &path)?;
    VerifiedSpsaSchedule::verify_written(expected, written).map_err(Into::into)
}

#[derive(Debug, Error)]
pub enum SpsaScheduleStoreError {
    #[error("{0}")]
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

#[cfg(test)]
mod tests {
    use colosseum_core::SpsaEndSpec;
    use serde_json::json;

    use super::*;
    use crate::{built_in_defaults, resolve_config};

    fn schedule(seed: u64) -> SpsaScheduleArtifact {
        SpsaScheduleArtifact::derive(
            5_000,
            0.002,
            seed,
            &[
                SpsaEndSpec {
                    name: "Aspiration".into(),
                    min: 1,
                    max: 500,
                    c_end: 0.75,
                },
                SpsaEndSpec {
                    name: "Reduction".into(),
                    min: -100,
                    max: 100,
                    c_end: 2.5,
                },
            ],
        )
        .unwrap()
    }

    fn run(root: &std::path::Path) -> RunDirectory {
        let config = resolve_config(
            built_in_defaults(),
            None,
            json!({"command": "spsa", "seed": 7}),
            &[],
            root,
            &[],
        )
        .unwrap();
        RunDirectory::open_explicit(&root.join("run"), &config, false)
            .unwrap()
            .directory
    }

    #[test]
    fn schedule_is_persisted_with_the_complete_reproducibility_contract() {
        let root = tempfile::tempdir().unwrap();
        let run = run(root.path());
        let expected = schedule(7);
        let verified = persist_and_verify_spsa_schedule(&run, &expected).unwrap();
        assert_eq!(verified.artifact(), &expected);

        let value: Value = serde_json::from_slice(
            &std::fs::read(run.paths().root.join(SPSA_SCHEDULE_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(value["schema_version"], 2);
        // The artifact records how the draws were made, not how a statistic is
        // defined, so the field names the random-stream version it holds.
        assert_eq!(value["rng_version"], 1);
        assert!(value.get("stats_version").is_none());
        assert_eq!(value["schedule"]["iterations"], 5_000);
        assert_eq!(value["r_end"], 0.002);
        assert_eq!(
            value["perturbations"]["algorithm"],
            "chacha12-64-bit-counter-zero-stream-v1"
        );
        assert_eq!(value["perturbations"]["stream_name"], "spsa-perturbations");
        assert_eq!(
            value["perturbations"]["draw_order"],
            "iteration-major-knob-order"
        );
        assert_eq!(value["perturbations"]["master_seed"], 7);
        assert_eq!(value["knobs"][0]["name"], "Aspiration");
    }

    #[test]
    fn a_mutated_written_schedule_cannot_produce_the_preflight_launch_token() {
        let root = tempfile::tempdir().unwrap();
        let run = run(root.path());
        let expected = schedule(7);
        persist_and_verify_spsa_schedule(&run, &expected).unwrap();
        let path = run.paths().root.join(SPSA_SCHEDULE_FILE);
        let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        value["knobs"][0]["c0"] = json!(123.0);
        std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

        assert!(matches!(
            persist_and_verify_spsa_schedule(&run, &expected),
            Err(SpsaScheduleStoreError::Preflight(
                SpsaPreflightError::InvalidSchedule(_)
            ))
        ));
        let still_mutated: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(still_mutated["knobs"][0]["c0"], 123.0);
    }

    #[test]
    fn resume_refuses_a_valid_schedule_derived_from_different_inputs() {
        let root = tempfile::tempdir().unwrap();
        let run = run(root.path());
        persist_and_verify_spsa_schedule(&run, &schedule(7)).unwrap();
        assert!(matches!(
            persist_and_verify_spsa_schedule(&run, &schedule(8)),
            Err(SpsaScheduleStoreError::Preflight(
                SpsaPreflightError::WrittenScheduleMismatch
            ))
        ));
    }

    #[test]
    fn short_horizon_schedule_survives_exact_json_round_trip() {
        let expected = SpsaScheduleArtifact::derive(
            3,
            0.002,
            7,
            &[SpsaEndSpec {
                name: "Hash".into(),
                min: 1,
                max: 1024,
                c_end: 1.0,
            }],
        )
        .unwrap();
        let bytes = serde_json::to_vec_pretty(&expected).unwrap();
        let written: SpsaScheduleArtifact = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(written, expected);
    }
}
