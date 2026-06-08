//! Errors produced by the UCI layer.

use thiserror::Error;

/// Things that can go wrong while talking to a UCI engine.
#[derive(Debug, Error)]
pub enum UciError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("engine handshake timed out")]
    HandshakeTimeout,

    #[error("timed out waiting for a move")]
    MoveTimeout,

    #[error("engine terminated unexpectedly")]
    Terminated,

    #[error("protocol error: {0}")]
    Protocol(String),
}
