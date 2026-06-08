//! Colosseum core: pure domain types and logic. No I/O, no UI, no async.
//!
//! This crate defines the stable "seams" the rest of the app builds on:
//! engine/option/time/tournament/adjudication config types, the [`rating::Rating`]
//! trait, [`event::TournamentEvent`], and the game/result types. Full algorithms
//! (Elo edge cases, round-robin color balancing, adjudication decisions) and their
//! exhaustive tests are completed in Step 3.

pub mod adjudication;
pub mod branding;
pub mod engine;
pub mod event;
pub mod game;
pub mod ids;
pub mod options;
pub mod pairing;
pub mod rating;
pub mod time;
pub mod tournament;

// Convenience re-exports.
pub use adjudication::{AdjudicationConfig, DrawAdjudication, ResignAdjudication};
pub use engine::{EngineConfig, EngineMeta};
pub use event::TournamentEvent;
pub use game::{GameResult, GameStats, Pairing, Termination};
pub use ids::{EngineId, GameId, TournamentId};
pub use options::{UciOption, UciOptionValue};
pub use rating::{IncrementalElo, Rating, RatingDelta};
pub use time::TimeControl;
pub use tournament::{CommonEngineOptions, EloPolicy, Format, StartPosition, TournamentConfig};
