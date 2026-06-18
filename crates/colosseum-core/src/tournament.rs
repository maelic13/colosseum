//! Tournament configuration: format, time control, concurrency, common forwarded
//! engine options, adjudication, Elo policy, start position, and PGN output.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::{adjudication::AdjudicationConfig, time::TimeControl};

/// Tournament format: how the schedule of encounters is generated. Both variants
/// produce a *static* schedule known upfront (result-independent pairing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    /// Every engine plays every other; `cycles` repeats the whole schedule.
    RoundRobin { cycles: u32 },
    /// The first `seeds` engines (in selection order) each play every *other*
    /// engine; seeds do not play each other and non-seeds do not play each other.
    /// `cycles` repeats the whole gauntlet.
    Gauntlet { seeds: u32, cycles: u32 },
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

/// File format of an opening book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpeningFormat {
    /// One position per line in EPD (`<board> <stm> <castling> <ep> [opcodes]`).
    Epd,
    /// One or more games in PGN; the first `plies` half-moves form the opening.
    Pgn,
}

impl OpeningFormat {
    /// Guess a format from a file extension (defaults to EPD).
    #[must_use]
    pub fn from_extension(ext: &str) -> Self {
        if ext.eq_ignore_ascii_case("pgn") {
            Self::Pgn
        } else {
            Self::Epd
        }
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Epd => "EPD",
            Self::Pgn => "PGN",
        }
    }
}

/// The order in which openings are drawn from the book.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OpeningOrder {
    /// Use openings in file order.
    #[default]
    Sequential,
    /// Shuffle deterministically using `OpeningBook::seed`.
    Random,
}

/// An opening book: a file of starting positions plus how to consume it. Each
/// *encounter* (an engine pair) draws one opening, so both colours are played
/// from the same position; the book cycles if there are more encounters than
/// openings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningBook {
    pub path: PathBuf,
    pub format: OpeningFormat,
    pub order: OpeningOrder,
    /// Half-moves to play out from each PGN game (ignored for EPD).
    pub plies: u32,
    /// Cap on how many openings to use (`None` = all in the file).
    pub count: Option<u32>,
    /// Seed for `OpeningOrder::Random` (kept for reproducible resume).
    pub seed: u64,
}

impl OpeningBook {
    /// A book over `path`, format guessed from its extension, with defaults.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        let format = path
            .extension()
            .and_then(|e| e.to_str())
            .map_or(OpeningFormat::Epd, OpeningFormat::from_extension);
        Self {
            path,
            format,
            order: OpeningOrder::Sequential,
            plies: 8,
            count: None,
            seed: 0,
        }
    }
}

/// Where games start from. v1 default = standard start position; an opening book
/// draws one position per encounter.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StartPosition {
    #[default]
    Startpos,
    Book(OpeningBook),
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
