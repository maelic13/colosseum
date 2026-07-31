//! Self-contained, recoverable CLI run-directory storage.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::ResolvedConfig;

static UNIQUE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const CHECKPOINT_SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunDirectoryPaths {
    pub root: PathBuf,
    pub log: PathBuf,
    pub checkpoint: PathBuf,
    pub previous_checkpoint: PathBuf,
}

#[derive(Debug)]
pub struct RunDirectoryOpen {
    pub directory: RunDirectory,
    pub resumed: bool,
    pub archived: Option<PathBuf>,
}

#[derive(Debug)]
pub struct RunDirectory {
    paths: RunDirectoryPaths,
    config_sha256: String,
}

impl RunDirectory {
    /// Create a collision-safe directory beneath `./colosseum-runs`.
    pub fn create_unique(
        invocation_directory: &Path,
        command: &str,
        config: &ResolvedConfig,
    ) -> Result<RunDirectoryOpen, RunDirectoryError> {
        let runs = invocation_directory.join("colosseum-runs");
        fs::create_dir_all(&runs).map_err(|source| RunDirectoryError::Io {
            operation: "create default run root",
            path: runs.clone(),
            source,
        })?;
        for _ in 0..1_000 {
            let root = runs.join(unique_name(command));
            match fs::create_dir(&root) {
                Ok(()) => {
                    let directory = Self::initialize(root, config)?;
                    return Ok(RunDirectoryOpen {
                        directory,
                        resumed: false,
                        archived: None,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(source) => {
                    return Err(RunDirectoryError::Io {
                        operation: "create unique run directory",
                        path: root,
                        source,
                    });
                }
            }
        }
        Err(RunDirectoryError::UniqueNameExhausted(runs))
    }

    /// Open an explicit directory. Existing state resumes unless `restart` is
    /// selected, in which case the complete old directory is archived first.
    pub fn open_explicit(
        root: &Path,
        config: &ResolvedConfig,
        restart: bool,
    ) -> Result<RunDirectoryOpen, RunDirectoryError> {
        if !root.exists() {
            fs::create_dir_all(root).map_err(|source| RunDirectoryError::Io {
                operation: "create explicit run directory",
                path: root.to_path_buf(),
                source,
            })?;
            return Ok(RunDirectoryOpen {
                directory: Self::initialize(root.to_path_buf(), config)?,
                resumed: false,
                archived: None,
            });
        }
        if restart {
            let archived = archive_path(root)?;
            fs::rename(root, &archived).map_err(|source| RunDirectoryError::Io {
                operation: "archive previous run",
                path: root.to_path_buf(),
                source,
            })?;
            fs::create_dir_all(root).map_err(|source| RunDirectoryError::Io {
                operation: "create restarted run directory",
                path: root.to_path_buf(),
                source,
            })?;
            return Ok(RunDirectoryOpen {
                directory: Self::initialize(root.to_path_buf(), config)?,
                resumed: false,
                archived: Some(archived),
            });
        }

        let stored = read_config_hash(root)?;
        if stored != config.sha256() {
            return Err(RunDirectoryError::ConfigMismatch {
                path: root.to_path_buf(),
                stored,
                requested: config.sha256().to_owned(),
            });
        }
        Ok(RunDirectoryOpen {
            directory: Self::from_existing(root.to_path_buf(), stored),
            resumed: true,
            archived: None,
        })
    }

    fn initialize(root: PathBuf, config: &ResolvedConfig) -> Result<Self, RunDirectoryError> {
        config.write_to(&root).map_err(RunDirectoryError::Config)?;
        sync_directory(&root)?;
        Ok(Self::from_existing(root, config.sha256().to_owned()))
    }

    fn from_existing(root: PathBuf, config_sha256: String) -> Self {
        Self {
            paths: RunDirectoryPaths {
                log: root.join("run.log"),
                checkpoint: root.join("checkpoint.json"),
                previous_checkpoint: root.join("checkpoint.previous.json"),
                root,
            },
            config_sha256,
        }
    }

    #[must_use]
    pub fn paths(&self) -> &RunDirectoryPaths {
        &self.paths
    }

    #[must_use]
    pub fn config_sha256(&self) -> &str {
        &self.config_sha256
    }

    /// Append bytes durably. Existing logs are never truncated on resume.
    pub fn append_log(&self, bytes: &[u8]) -> Result<(), RunDirectoryError> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.paths.log)
            .map_err(|source| RunDirectoryError::Io {
                operation: "open append-only run log",
                path: self.paths.log.clone(),
                source,
            })?;
        file.write_all(bytes)
            .map_err(|source| RunDirectoryError::Io {
                operation: "append run log",
                path: self.paths.log.clone(),
                source,
            })?;
        file.sync_data().map_err(|source| RunDirectoryError::Io {
            operation: "sync run log",
            path: self.paths.log.clone(),
            source,
        })
    }

    /// Atomically publish a checksummed checkpoint and retain one previous
    /// valid generation for fallback recovery.
    pub fn write_checkpoint<T: Serialize>(&self, value: &T) -> Result<(), RunDirectoryError> {
        let payload = serde_json::to_value(value).map_err(RunDirectoryError::Json)?;
        let payload_bytes = serde_json::to_vec(&payload).expect("JSON value is serializable");
        let checksum = hex(&Sha256::digest(&payload_bytes));
        let envelope = serde_json::to_vec(&json!({
            "schema_version": CHECKPOINT_SCHEMA_VERSION,
            "payload_sha256": checksum,
            "payload": payload,
        }))
        .expect("checkpoint envelope is serializable");
        let temporary = self.paths.root.join(format!(
            ".checkpoint.{}.{}.tmp",
            std::process::id(),
            UNIQUE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .map_err(|source| RunDirectoryError::Io {
                    operation: "create checkpoint temporary",
                    path: temporary.clone(),
                    source,
                })?;
            file.write_all(&envelope)
                .and_then(|()| file.sync_all())
                .map_err(|source| RunDirectoryError::Io {
                    operation: "write checkpoint temporary",
                    path: temporary.clone(),
                    source,
                })?;
            if self.paths.checkpoint.exists() {
                remove_if_exists(&self.paths.previous_checkpoint)?;
                fs::rename(&self.paths.checkpoint, &self.paths.previous_checkpoint).map_err(
                    |source| RunDirectoryError::Io {
                        operation: "rotate checkpoint generation",
                        path: self.paths.checkpoint.clone(),
                        source,
                    },
                )?;
            }
            fs::rename(&temporary, &self.paths.checkpoint).map_err(|source| {
                RunDirectoryError::Io {
                    operation: "publish checkpoint",
                    path: self.paths.checkpoint.clone(),
                    source,
                }
            })?;
            sync_directory(&self.paths.root)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// Read the current generation, falling back only when it is absent or
    /// fails schema/checksum/JSON validation.
    pub fn read_checkpoint<T: DeserializeOwned>(&self) -> Result<T, RunDirectoryError> {
        match read_generation(&self.paths.checkpoint) {
            Ok(value) => serde_json::from_value(value).map_err(RunDirectoryError::Json),
            Err(current) => match read_generation(&self.paths.previous_checkpoint) {
                Ok(value) => serde_json::from_value(value).map_err(RunDirectoryError::Json),
                Err(previous) => Err(RunDirectoryError::NoValidCheckpoint {
                    current: current.to_string(),
                    previous: previous.to_string(),
                }),
            },
        }
    }
}

#[derive(Debug, Error)]
pub enum RunDirectoryError {
    #[error("could not allocate a unique run name beneath {0}")]
    UniqueNameExhausted(PathBuf),
    #[error("explicit run directory {0} has no stored configuration identity")]
    MissingConfig(PathBuf),
    #[error("run configuration mismatch in {path}: stored {stored}, requested {requested}")]
    ConfigMismatch {
        path: PathBuf,
        stored: String,
        requested: String,
    },
    #[error("no valid checkpoint: current: {current}; previous: {previous}")]
    NoValidCheckpoint { current: String, previous: String },
    #[error("configuration persistence failed: {0}")]
    Config(#[source] crate::ConfigError),
    #[error("checkpoint JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("could not {operation} at {path}: {source}")]
    Io {
        operation: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid checkpoint {path}: {reason}")]
    InvalidCheckpoint { path: PathBuf, reason: String },
    #[error("cannot archive run path without a final name: {0}")]
    UnnameableArchive(PathBuf),
}

fn unique_name(command: &str) -> String {
    let command = command
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = UNIQUE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("{command}-{millis}-{}-{sequence}", std::process::id())
}

fn archive_path(root: &Path) -> Result<PathBuf, RunDirectoryError> {
    let parent = root.parent().unwrap_or_else(|| Path::new("."));
    let name = root
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| RunDirectoryError::UnnameableArchive(root.to_path_buf()))?;
    for _ in 0..1_000 {
        let candidate = parent.join(format!("{name}.archive-{}", unique_name("restart")));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(RunDirectoryError::UniqueNameExhausted(parent.to_path_buf()))
}

fn read_config_hash(root: &Path) -> Result<String, RunDirectoryError> {
    let path = root.join("config.sha256");
    let text = fs::read_to_string(&path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            RunDirectoryError::MissingConfig(root.to_path_buf())
        } else {
            RunDirectoryError::Io {
                operation: "read stored configuration identity",
                path: path.clone(),
                source,
            }
        }
    })?;
    text.split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or(RunDirectoryError::MissingConfig(root.to_path_buf()))
}

fn read_generation(path: &Path) -> Result<Value, RunDirectoryError> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|source| RunDirectoryError::Io {
            operation: "read checkpoint generation",
            path: path.to_path_buf(),
            source,
        })?;
    let envelope: Value =
        serde_json::from_slice(&bytes).map_err(|error| RunDirectoryError::InvalidCheckpoint {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    if envelope["schema_version"] != CHECKPOINT_SCHEMA_VERSION {
        return Err(RunDirectoryError::InvalidCheckpoint {
            path: path.to_path_buf(),
            reason: "unsupported schema version".into(),
        });
    }
    let payload =
        envelope
            .get("payload")
            .cloned()
            .ok_or_else(|| RunDirectoryError::InvalidCheckpoint {
                path: path.to_path_buf(),
                reason: "missing payload".into(),
            })?;
    let expected = envelope["payload_sha256"].as_str().ok_or_else(|| {
        RunDirectoryError::InvalidCheckpoint {
            path: path.to_path_buf(),
            reason: "missing payload checksum".into(),
        }
    })?;
    let actual = hex(&Sha256::digest(
        serde_json::to_vec(&payload).expect("JSON value is serializable"),
    ));
    if actual != expected {
        return Err(RunDirectoryError::InvalidCheckpoint {
            path: path.to_path_buf(),
            reason: "payload checksum mismatch".into(),
        });
    }
    Ok(payload)
}

fn remove_if_exists(path: &Path) -> Result<(), RunDirectoryError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(RunDirectoryError::Io {
            operation: "remove old checkpoint generation",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn sync_directory(path: &Path) -> Result<(), RunDirectoryError> {
    #[cfg(unix)]
    {
        File::open(path)
            .and_then(|file| file.sync_all())
            .map_err(|source| RunDirectoryError::Io {
                operation: "sync run directory",
                path: path.to_path_buf(),
                source,
            })?;
    }
    #[cfg(windows)]
    let _ = path;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to String cannot fail");
    }
    output
}
