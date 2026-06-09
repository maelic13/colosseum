//! Tournament orchestration: the backend.
//!
//! Step 2 establishes the crate, its error seam, and storage-path helpers. Later
//! steps add: the per-game runner (UCI + shakmaty + adjudication + PGN), the
//! concurrency scheduler with Go/Stop/Force-Stop/resume, the event stream, the Elo
//! updater, SQLite persistence with tournament history + resume (Steps 5–6), and
//! config/engine-library file I/O with `--portable` mode (Step 6).

pub mod config;
pub mod detect;
pub mod error;
pub mod openings;
pub mod paths;
pub mod pgn;
pub mod runner;
pub mod scheduler;
pub mod store;

pub use config::{AppConfig, AppDirs, EngineLibrary};
pub use detect::{DetectResult, detect_engine};
pub use error::EngineError;
pub use openings::{ResolvedOpening, load_openings, summarize};
pub use runner::{EngineGameSpec, GameReport, GameSpec, run_game};
pub use scheduler::{
    Command, EloEntry, Tournament, TournamentSnapshot, TournamentStatus, create_tournament,
    resume_tournament,
};
pub use store::{GameRow, Store, TournamentEngineRow, TournamentRow};
