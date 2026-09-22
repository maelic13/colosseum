//! Engine library entries: metadata plus everything needed to launch and configure
//! an engine process.

use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

use crate::{
    ids::EngineId,
    options::{UciOption, UciOptionValue},
};

/// User-facing, extensible engine metadata.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct EngineMeta {
    /// Display name (defaults to the engine's UCI `id name`).
    pub name: String,
    /// Free-form version string (user-editable).
    pub version: String,
    /// Configured baseline Elo, if known.
    pub elo: Option<i32>,
    /// Open-ended extra fields for future metadata (e.g. `logo`, `author`).
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}

/// A complete engine configuration: how to launch it and how it is tuned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineConfig {
    pub id: EngineId,
    pub meta: EngineMeta,
    /// Path to the engine executable.
    pub path: PathBuf,
    /// Extra command-line arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Working directory to launch the engine in (e.g. for relative NNUE paths).
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    /// Additional environment variables.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// User-chosen option values, keyed by option name.
    #[serde(default)]
    pub options: BTreeMap<String, UciOptionValue>,
    /// Option schema detected at the last UCI handshake (detect-on-add, re-detect on demand).
    #[serde(default)]
    pub detected_options: Vec<UciOption>,
}

impl EngineConfig {
    /// Create a new, minimally-populated engine config for the given executable.
    #[must_use]
    pub fn new(id: EngineId, path: PathBuf) -> Self {
        Self {
            id,
            meta: EngineMeta::default(),
            path,
            args: Vec::new(),
            working_dir: None,
            env: BTreeMap::new(),
            options: BTreeMap::new(),
            detected_options: Vec::new(),
        }
    }
}
