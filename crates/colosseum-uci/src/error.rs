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

    /// The same end, with the exit status the runtime managed to reap. An
    /// incident report can then distinguish an engine that failed from one
    /// that was killed, which a bare EOF cannot.
    #[error("engine terminated unexpectedly ({0})")]
    TerminatedWithStatus(String),

    #[error("engine did not exit after quit before the shutdown deadline")]
    ShutdownTimeout,

    #[error("protocol error: {0}")]
    Protocol(String),
}
