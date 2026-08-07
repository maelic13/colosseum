//! Adjudication configuration and decision logic. Every rule is optional so any can
//! be disabled. Decisions operate on per-ply evaluations expressed from **White's
//! point of view** in centipawns (mate encoded as a large magnitude). `move_count`
//! fields count *full moves*; the window examined is therefore `2 * move_count` plies.

use serde::{Deserialize, Serialize};

use crate::game::{GameResult, Termination};

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
    /// Require both engines to report the decisive score, rather than only the
    /// engine that would lose the game.
    #[serde(default = "default_resign_two_sided")]
    pub two_sided: bool,
}

const fn default_resign_two_sided() -> bool {
    true
}

/// Full adjudication configuration for a tournament. `None` fields are disabled.
/// Natural game endings (mate/stalemate/50-move/threefold/insufficient material)
/// are always detected regardless of this config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AdjudicationConfig {
    /// Hard cap on the number of full moves before the game is declared a draw.
    pub max_moves: Option<u32>,
    /// Optional draw adjudication.
    pub draw: Option<DrawAdjudication>,
    /// Optional win/loss (resign) adjudication.
    pub resign: Option<ResignAdjudication>,
}

/// The result of adjudicating a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Adjudication {
    pub result: GameResult,
    pub termination: Termination,
}

/// Decide whether a game should be adjudicated given the per-ply evaluations from
/// **White's point of view** (centipawns; mate as a large magnitude). `white_pov_cp`
/// has one entry per ply played so far. Returns `None` if no rule fires.
///
/// Precedence: a decisive resign verdict first, then draw, then the max-move cap.
#[must_use]
pub fn adjudicate(white_pov_cp: &[i32], config: &AdjudicationConfig) -> Option<Adjudication> {
    let plies = white_pov_cp.len();

    // Resign / win adjudication (decisive) takes precedence.
    if let Some(resign) = config.resign
        && resign.move_count > 0
    {
        let window = (resign.move_count as usize) * 2;
        if resign.two_sided && plies >= window {
            let tail = &white_pov_cp[plies - window..];
            let threshold = resign.score_cp.abs();
            if tail.iter().all(|&v| v <= -threshold) {
                // White is hopeless for the whole window.
                return Some(Adjudication {
                    result: GameResult::BlackWin,
                    termination: Termination::AdjudicatedResign,
                });
            }
            if tail.iter().all(|&v| v >= threshold) {
                // Black is hopeless for the whole window.
                return Some(Adjudication {
                    result: GameResult::WhiteWin,
                    termination: Termination::AdjudicatedResign,
                });
            }
        } else if !resign.two_sided {
            let threshold = resign.score_cp.abs();
            if engine_reports_past_threshold(white_pov_cp, 0, resign.move_count as usize, |score| {
                score <= -threshold
            }) {
                return Some(Adjudication {
                    result: GameResult::BlackWin,
                    termination: Termination::AdjudicatedResign,
                });
            }
            if engine_reports_past_threshold(white_pov_cp, 1, resign.move_count as usize, |score| {
                score >= threshold
            }) {
                return Some(Adjudication {
                    result: GameResult::WhiteWin,
                    termination: Termination::AdjudicatedResign,
                });
            }
        }
    }

    // Draw adjudication.
    if let Some(draw) = config.draw
        && draw.move_count > 0
    {
        let window = (draw.move_count as usize) * 2;
        if plies as u32 >= draw.min_ply && plies >= window {
            let tail = &white_pov_cp[plies - window..];
            let threshold = draw.score_cp.abs();
            if tail.iter().all(|&v| v.abs() <= threshold) {
                return Some(Adjudication {
                    result: GameResult::Draw,
                    termination: Termination::AdjudicatedDraw,
                });
            }
        }
    }

    // Maximum move count -> draw.
    if let Some(max_moves) = config.max_moves
        && max_moves > 0
        && plies >= (max_moves as usize) * 2
    {
        return Some(Adjudication {
            result: GameResult::Draw,
            termination: Termination::MaxMoves,
        });
    }

    None
}

fn engine_reports_past_threshold(
    scores: &[i32],
    ply_parity: usize,
    required: usize,
    predicate: impl Fn(i32) -> bool,
) -> bool {
    let mut matching = 0;
    for (index, &score) in scores.iter().enumerate().rev() {
        if index % 2 != ply_parity {
            continue;
        }
        if !predicate(score) {
            return false;
        }
        matching += 1;
        if matching == required {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_config_never_adjudicates() {
        let cfg = AdjudicationConfig::default();
        assert_eq!(adjudicate(&[0, 0, 0, 0, 0, 0], &cfg), None);
    }

    #[test]
    fn resign_white_loses() {
        let cfg = AdjudicationConfig {
            resign: Some(ResignAdjudication {
                move_count: 2,
                score_cp: 800,
                two_sided: true,
            }),
            ..Default::default()
        };
        // Last 4 plies all <= -800 -> White hopeless -> Black wins.
        let scores = [10, -50, -900, -1000, -1200, -1500];
        assert_eq!(
            adjudicate(&scores, &cfg),
            Some(Adjudication {
                result: GameResult::BlackWin,
                termination: Termination::AdjudicatedResign,
            })
        );
    }

    #[test]
    fn resign_black_loses() {
        let cfg = AdjudicationConfig {
            resign: Some(ResignAdjudication {
                move_count: 2,
                score_cp: 800,
                two_sided: true,
            }),
            ..Default::default()
        };
        let scores = [900, 1000, 1100, 1200];
        assert_eq!(
            adjudicate(&scores, &cfg),
            Some(Adjudication {
                result: GameResult::WhiteWin,
                termination: Termination::AdjudicatedResign,
            })
        );
    }

    #[test]
    fn two_sided_resign_requires_both_engines_for_full_window() {
        let cfg = AdjudicationConfig {
            resign: Some(ResignAdjudication {
                move_count: 2,
                score_cp: 800,
                two_sided: true,
            }),
            ..Default::default()
        };
        // One value inside the window is not past the threshold -> no resign.
        let scores = [-900, -50, -900, -1000];
        assert_eq!(adjudicate(&scores, &cfg), None);
        // Too few plies for the window.
        let short = [-900, -1000];
        assert_eq!(adjudicate(&short, &cfg), None);
    }

    #[test]
    fn one_sided_resign_uses_only_losing_engines_reports() {
        let cfg = AdjudicationConfig {
            resign: Some(ResignAdjudication {
                move_count: 2,
                score_cp: 800,
                two_sided: false,
            }),
            ..Default::default()
        };

        // White's reports (indices 0 and 2) cross the threshold while Black
        // disagrees. A two-sided rule would keep playing; one-sided resigns.
        let scores = [-900, -50, -1000];
        assert_eq!(
            adjudicate(&scores, &cfg),
            Some(Adjudication {
                result: GameResult::BlackWin,
                termination: Termination::AdjudicatedResign,
            })
        );
    }

    #[test]
    fn legacy_resign_configuration_deserializes_as_two_sided() {
        let resign: ResignAdjudication =
            serde_json::from_str(r#"{"move_count":3,"score_cp":600}"#).unwrap();
        assert!(resign.two_sided);
    }

    #[test]
    fn draw_adjudication_triggers() {
        let cfg = AdjudicationConfig {
            draw: Some(DrawAdjudication {
                min_ply: 4,
                move_count: 2,
                score_cp: 8,
            }),
            ..Default::default()
        };
        let scores = [3, -2, 0, 5, -4, 1];
        assert_eq!(
            adjudicate(&scores, &cfg),
            Some(Adjudication {
                result: GameResult::Draw,
                termination: Termination::AdjudicatedDraw,
            })
        );
    }

    #[test]
    fn draw_respects_min_ply() {
        let cfg = AdjudicationConfig {
            draw: Some(DrawAdjudication {
                min_ply: 10,
                move_count: 2,
                score_cp: 8,
            }),
            ..Default::default()
        };
        // Window is drawish but we have not reached min_ply yet.
        let scores = [0, 0, 0, 0];
        assert_eq!(adjudicate(&scores, &cfg), None);
    }

    #[test]
    fn max_moves_is_a_draw() {
        let cfg = AdjudicationConfig {
            max_moves: Some(3),
            ..Default::default()
        };
        assert_eq!(adjudicate(&[0; 5], &cfg), None);
        assert_eq!(
            adjudicate(&[0; 6], &cfg),
            Some(Adjudication {
                result: GameResult::Draw,
                termination: Termination::MaxMoves,
            })
        );
    }

    #[test]
    fn resign_beats_draw_when_both_could_apply() {
        // Resign window decisive; draw never matches because scores are large.
        let cfg = AdjudicationConfig {
            draw: Some(DrawAdjudication {
                min_ply: 0,
                move_count: 1,
                score_cp: 10,
            }),
            resign: Some(ResignAdjudication {
                move_count: 1,
                score_cp: 800,
                two_sided: true,
            }),
            ..Default::default()
        };
        let scores = [1500, 1600];
        assert_eq!(
            adjudicate(&scores, &cfg).map(|a| a.termination),
            Some(Termination::AdjudicatedResign)
        );
    }
}
