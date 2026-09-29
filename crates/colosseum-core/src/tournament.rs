//! Tournament building blocks: the schedule format and the opening book.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
