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
pub mod export;
pub mod game;
pub mod ids;
pub mod options;
pub mod pairing;
pub mod rating;
pub mod standings;
pub mod stats;
pub mod time;
pub mod tournament;

// Convenience re-exports.
pub use adjudication::{
    Adjudication, AdjudicationConfig, DrawAdjudication, ResignAdjudication, adjudicate,
};
pub use engine::{EngineConfig, EngineMeta};
pub use event::TournamentEvent;
pub use export::{ExportRow, crosstable_csv, standings_csv};
pub use game::{GameResult, GameStats, Pairing, Termination};
pub use ids::{EngineId, GameId, TournamentId};
pub use options::{UciOption, UciOptionValue, is_hash_option, is_thread_option};
pub use pairing::{gauntlet, generate_schedule, round_robin};
pub use rating::{IncrementalElo, Rating, RatingDelta, ml_ratings, performance_rating};
pub use standings::{EngineStanding, GameOutcome, HeadToHead, PairGameResult, Standings};
pub use stats::{EloEstimate, SprtDecision, SprtResult, elo_with_error, los, sprt};
pub use time::{TimeControl, TimeUnit};
pub use tournament::{
    CommonEngineOptions, EloPolicy, Format, OpeningBook, OpeningFormat, OpeningOrder,
    RatingWriteback, StartPosition, TournamentConfig,
};
