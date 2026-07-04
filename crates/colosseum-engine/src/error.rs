//! Errors produced by the orchestration layer.

use thiserror::Error;

/// Things that can go wrong while running tournaments and persisting their data.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Uci(#[from] colosseum_uci::UciError),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("corrupt stored data: {0}")]
    Corrupt(String),

    #[error("config file parse error: {0}")]
    TomlDe(#[from] toml::de::Error),

    #[error("config file write error: {0}")]
    TomlSer(#[from] toml::ser::Error),

    #[error("could not determine application directories")]
    NoProjectDirs,

    #[error("{0}")]
    Runtime(String),
}
