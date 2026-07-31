//! Colosseum core: pure domain types and logic. No I/O, no UI, no async.
//!
//! This crate defines the stable "seams" the rest of the app builds on:
//! engine/option/time/tournament/adjudication config types, the rating math
//! ([`rating::ml_ratings`] and friends), [`event::TournamentEvent`], and the
//! game/result types.

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
pub use options::{
    UciOption, UciOptionValue, is_hash_option, is_tablebase_option, is_thread_option,
};
pub use pairing::{gauntlet, generate_schedule, round_robin};
pub use rating::{ml_ratings, ml_ratings_anchored, performance_rating, rating_error};
pub use standings::{EngineStanding, GameOutcome, HeadToHead, PairGameResult, Standings};
pub use stats::{
    EloEstimate, PentanomialBin, PentanomialVector, SprtDecision, SprtResult, elo_with_error, los,
    sprt,
};
pub use time::{TimeControl, TimeUnit};
pub use tournament::{
    CommonEngineOptions, Format, OpeningBook, OpeningFormat, OpeningOrder, RatingWriteback,
    StartPosition, TournamentConfig,
};
