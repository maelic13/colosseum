//! Game results, how a game ended, per-game stats, and a single pairing.

use serde::{Deserialize, Serialize};

use crate::ids::EngineId;

/// The outcome of a finished game, from White's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GameResult {
    WhiteWin,
    BlackWin,
    Draw,
}

impl GameResult {
    /// PGN result tag string.
    #[must_use]
    pub fn pgn(self) -> &'static str {
        match self {
            Self::WhiteWin => "1-0",
            Self::BlackWin => "0-1",
            Self::Draw => "1/2-1/2",
        }
    }
}

/// Why a game ended. Natural endings plus adjudication and error terminations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Termination {
    Checkmate,
    Stalemate,
    FiftyMove,
    Threefold,
    InsufficientMaterial,
    AdjudicatedDraw,
    AdjudicatedResign,
    MaxMoves,
    TimeForfeit,
    EngineCrash,
    IllegalMove,
    /// Game was force-stopped/aborted; not counted as a result.
    Aborted,
}

/// Per-game statistics surfaced in the results table (extensible).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct GameStats {
    pub plies: u32,
    pub white_nps: Option<u64>,
    pub black_nps: Option<u64>,
    /// Mean search depth across this engine's moves in the game.
    pub white_depth: Option<f64>,
    pub black_depth: Option<f64>,
    /// Mean wall-clock milliseconds this engine spent per move in the game.
    pub white_move_ms: Option<f64>,
    pub black_move_ms: Option<f64>,
    /// Wall-clock duration of the game in milliseconds (None for aborted/setup-failure games).
    pub duration_ms: Option<u64>,
}

/// A single scheduled game: who plays which color, in which round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pairing {
    pub white: EngineId,
    pub black: EngineId,
    pub round: u32,
}
