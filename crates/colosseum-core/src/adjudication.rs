//! Adjudication configuration. Every rule is optional so any can be disabled.
//! The decision logic (operating on per-move score history) lands in Step 3.

use serde::{Deserialize, Serialize};

/// Adjudicate a draw once both engines report a near-zero score for long enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrawAdjudication {
    /// Minimum ply count reached before draw adjudication may trigger.
    pub min_ply: u32,
    /// Number of consecutive moves both scores must stay within the threshold.
    pub move_count: u32,
    /// Absolute score threshold in centipawns (e.g. 8 => |score| <= 8cp).
    pub score_cp: i32,
}

/// Adjudicate a win/loss once the losing side's score is hopeless for long enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResignAdjudication {
    /// Number of consecutive moves the score must stay beyond the threshold.
    pub move_count: u32,
    /// Score magnitude in centipawns past which a side is considered lost.
    pub score_cp: i32,
}

/// Full adjudication configuration for a tournament. `None` fields are disabled.
/// Natural game endings (mate/stalemate/50-move/threefold/insufficient material)
/// are always detected regardless of this config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AdjudicationConfig {
    /// Hard cap on the number of moves before the game is declared a draw.
    pub max_moves: Option<u32>,
    /// Optional draw adjudication.
    pub draw: Option<DrawAdjudication>,
    /// Optional win/loss (resign) adjudication.
    pub resign: Option<ResignAdjudication>,
}
