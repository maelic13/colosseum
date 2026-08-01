//! Errors produced by the orchestration layer.

use thiserror::Error;

/// Things that can go wrong while running tournaments and persisting their data.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error(transparent)]
    Opening(#[from] crate::openings::OpeningError),

    #[error(transparent)]
    Application(#[from] colosseum_application::ApplicationError),

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

    #[error("could not determine application directories")]
    NoProjectDirs,
}
