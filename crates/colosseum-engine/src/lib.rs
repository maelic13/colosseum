//! Tournament orchestration: the backend.
//!
//! Step 2 establishes the crate, its error seam, and storage-path helpers. Later
//! steps add: the per-game runner (UCI + shakmaty + adjudication + PGN), the
//! concurrency scheduler with Go/Stop/Force-Stop/resume, the event stream, the Elo
//! updater, SQLite persistence with tournament history + resume (Steps 5–6), and
//! config/engine-library file I/O with `--portable` mode (Step 6).

pub mod detect;
pub mod error;
pub mod incidents;
pub mod live;
pub mod openings;
pub mod pgn;
pub mod runner;
pub mod scheduler;
pub mod store;

pub use colosseum_uci::Score;
pub use detect::{DetectResult, detect_engine, split_name_version};
pub use error::EngineError;
pub use live::{EvalPoint, LiveGameHandle, LiveGameState, LiveSearch};
pub use openings::{ResolvedOpening, load_openings, summarize};
pub use runner::{EngineGameSpec, GameReport, GameSpec, run_game};
pub use scheduler::{
    Command, EloEntry, InFlightGame, ResultParticipant, Tournament, TournamentResults,
    TournamentSnapshot, TournamentStatus, create_tournament, load_tournament_results,
    resume_tournament,
};
pub use store::{GameRow, PendingGame, Store, TournamentEngineRow, TournamentRow};
