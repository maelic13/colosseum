//! Tournament orchestration: the backend.
//!
//! Step 2 establishes the crate, its error seam, and storage-path helpers. Later
//! steps add: the per-game runner (UCI + shakmaty + adjudication + PGN), the
//! concurrency scheduler with Go/Stop/Force-Stop/resume, the event stream, the Elo
//! updater, and SQLite persistence with tournament history + resume (Steps 5–6).

pub mod error;
pub mod paths;
pub mod pgn;
pub mod runner;
pub mod store;

pub use error::EngineError;
pub use runner::{EngineGameSpec, GameReport, GameSpec, run_game};
pub use store::{GameRow, Store, TournamentEngineRow, TournamentRow};
