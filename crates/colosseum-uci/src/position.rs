//! UCI position and search-limit commands. v1 supports `startpos`/`fen` positions
//! and `go movetime`; the enums are the seam for adding clock/nodes/depth later.

use std::time::Duration;

/// A position to set with the UCI `position` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UciPosition {
    /// Standard start position, optionally followed by moves (UCI long algebraic).
    StartPos { moves: Vec<String> },
    /// An arbitrary FEN, optionally followed by moves.
    Fen { fen: String, moves: Vec<String> },
}

impl UciPosition {
    /// Render the `position ...` command line.
    #[must_use]
    pub fn to_command(&self) -> String {
        match self {
            Self::StartPos { moves } if moves.is_empty() => "position startpos".to_string(),
            Self::StartPos { moves } => format!("position startpos moves {}", moves.join(" ")),
            Self::Fen { fen, moves } if moves.is_empty() => format!("position fen {fen}"),
            Self::Fen { fen, moves } => format!("position fen {fen} moves {}", moves.join(" ")),
        }
    }
}

/// Search limits for the UCI `go` command. v1 = fixed time per move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoLimits {
    /// `go movetime <ms>`.
    MoveTime(Duration),
}

impl GoLimits {
    /// Render the `go ...` command line.
    #[must_use]
    pub fn to_command(&self) -> String {
        match self {
            Self::MoveTime(d) => format!("go movetime {}", d.as_millis()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startpos_commands() {
        assert_eq!(
            UciPosition::StartPos { moves: vec![] }.to_command(),
            "position startpos"
        );
        assert_eq!(
            UciPosition::StartPos {
                moves: vec!["e2e4".into(), "e7e5".into()]
            }
            .to_command(),
            "position startpos moves e2e4 e7e5"
        );
    }

    #[test]
    fn fen_command() {
        assert_eq!(
            UciPosition::Fen {
                fen: "8/8/8/8/8/8/8/8 w - - 0 1".into(),
                moves: vec![],
            }
            .to_command(),
            "position fen 8/8/8/8/8/8/8/8 w - - 0 1"
        );
    }

    #[test]
    fn go_movetime_command() {
        assert_eq!(
            GoLimits::MoveTime(Duration::from_millis(10)).to_command(),
            "go movetime 10"
        );
    }
}
