//! UCI protocol and engine process management.
//!
//! - [`parse`]: pure parsers for `option`/`info`/`bestmove` lines.
//! - [`EngineProcess`]: an async handle that spawns an engine, performs the
//!   `uci`/`isready` handshake, sets options, runs searches to `bestmove` under a
//!   deadline, and shuts the engine down cleanly (`kill_on_drop` guards against leaks).
//! - [`UciPosition`] / [`GoLimits`]: the `position` and `go` command builders.
//! - [`Score`]: engine score reporting.

pub mod error;
pub mod parse;
pub mod position;
pub mod process;
pub mod score;
pub mod session;

pub use error::UciError;
pub use parse::{InfoLine, parse_bestmove, parse_info_line, parse_option_line};
pub use position::{GoLimits, UciPosition};
pub use process::{
    EngineProcess, HandshakeInfo, MAX_PROTOCOL_LINE_BYTES, MAX_STDERR_LINE_BYTES, SearchOutput,
    SpawnOptions, process_is_alive,
};
pub use score::Score;
pub use session::{AffinityUciSessionFactory, ProcessAffinityFn, UciSessionFactory};
