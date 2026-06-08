//! Tournament configuration: format, time control, concurrency, common forwarded
//! engine options, adjudication, Elo policy, start position, and PGN output.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{adjudication::AdjudicationConfig, time::TimeControl};

/// Tournament format. v1 = Round Robin; more formats slot in later.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    /// Every engine plays every other; `cycles` repeats the whole schedule.
    RoundRobin { cycles: u32 },
}

impl Default for Format {
    fn default() -> Self {
        Self::RoundRobin { cycles: 1 }
    }
}

/// Options applied uniformly to every engine in the tournament (forwarded as UCI
/// options). `None`/false means "leave the engine's own default".
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CommonEngineOptions {
    pub threads: Option<u32>,
    pub hash_mb: Option<u32>,
    pub syzygy_path: Option<String>,
    pub syzygy_50_move_rule: Option<bool>,
    /// Forwarded as the UCI `Ponder` option. Default off for fair fast games.
    pub ponder: bool,
}

/// When Elo ratings are recomputed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EloPolicy {
    /// Update incrementally after every game.
    #[default]
    PerGame,
    /// Recompute once when the tournament finishes.
    EndOfTournament,
    /// Never modify ratings.
    Never,
}

/// Where games start from. v1 = standard start position; Step 10 adds opening books.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StartPosition {
    #[default]
    Startpos,
    // OpeningBook(BookRef) added in Step 10.
}

/// Full, serializable tournament configuration. Persisted with each tournament for
/// reproducibility and resume.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TournamentConfig {
    pub format: Format,
    /// Games played per unordered engine pair, per cycle (typically 2: both colors).
    pub games_per_pair: u32,
    pub time_control: TimeControl,
    /// How many games to run concurrently.
    pub concurrency: usize,
    pub common: CommonEngineOptions,
    pub adjudication: AdjudicationConfig,
    pub elo_policy: EloPolicy,
    /// K-factor for incremental Elo updates.
    pub k_factor: f64,
    pub start_position: StartPosition,
    /// Optional path to append finished games as PGN.
    pub pgn_output: Option<PathBuf>,
}

impl Default for TournamentConfig {
    fn default() -> Self {
        Self {
            format: Format::default(),
            games_per_pair: 2,
            time_control: TimeControl::default(),
            concurrency: 1,
            common: CommonEngineOptions {
                threads: Some(1),
                ..CommonEngineOptions::default()
            },
            adjudication: AdjudicationConfig::default(),
            elo_policy: EloPolicy::default(),
            k_factor: 32.0,
            start_position: StartPosition::default(),
            pgn_output: None,
        }
    }
}
