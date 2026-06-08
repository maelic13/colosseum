//! Events emitted by the backend for the GUI to consume. Cheap to clone and send
//! across a channel; the GUI drains these each frame to drive live updates.

use crate::{
    game::{GameResult, GameStats, Termination},
    ids::{EngineId, GameId},
};

/// A single live-update event from the tournament backend.
#[derive(Debug, Clone)]
pub enum TournamentEvent {
    /// A game has started.
    GameStarted {
        game_id: GameId,
        white: EngineId,
        black: EngineId,
        round: u32,
    },
    /// A move was played (carries the mover's latest nps, when available).
    MoveMade {
        game_id: GameId,
        ply: u32,
        nps: Option<u64>,
    },
    /// A game finished with a result.
    GameFinished {
        game_id: GameId,
        result: GameResult,
        termination: Termination,
        stats: GameStats,
    },
    /// Standings/ratings changed; the GUI should refresh the table.
    StandingsUpdated,
    /// An engine produced an error (crash, hang, illegal move, ...).
    EngineError { engine: EngineId, message: String },
    /// The tournament reached its end (all games finished).
    TournamentFinished,
}
