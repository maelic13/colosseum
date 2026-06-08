//! UCI protocol and engine process management.
//!
//! Step 2 establishes the crate and its error seam. Step 4 implements: async process
//! spawn (args/workdir/env), the `uci`/`isready` handshake, `option` line parsing,
//! `setoption`, `position`/`go movetime`, `info`/`bestmove` parsing, per-move
//! timeouts, crash/hang detection, guaranteed child cleanup, and optional I/O logging.

pub mod error;

pub use error::UciError;
