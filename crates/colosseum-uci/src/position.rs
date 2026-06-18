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

/// Search limits for the UCI `go` command: fixed time per move, a game clock
/// (with optional increment), fixed nodes, or fixed depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoLimits {
    /// `go movetime <ms>`.
    MoveTime(Duration),
    /// `go wtime <ms> btime <ms> winc <ms> binc <ms>` — remaining clocks plus
    /// per-move increments for both sides.
    Clock {
        wtime: Duration,
        btime: Duration,
        winc: Duration,
        binc: Duration,
    },
    /// `go nodes <n>`.
    Nodes(u64),
    /// `go depth <d>`.
    Depth(u32),
}

impl GoLimits {
    /// Render the `go ...` command line.
    #[must_use]
    pub fn to_command(&self) -> String {
        match self {
            Self::MoveTime(d) => format!("go movetime {}", d.as_millis()),
            Self::Clock {
                wtime,
                btime,
                winc,
                binc,
            } => {
                // Always include winc/binc (0 for sudden death) — engines tolerate it
                // and it keeps the command unambiguous.
                format!(
                    "go wtime {} btime {} winc {} binc {}",
                    wtime.as_millis(),
                    btime.as_millis(),
                    winc.as_millis(),
                    binc.as_millis(),
                )
            }
            Self::Nodes(n) => format!("go nodes {n}"),
            Self::Depth(d) => format!("go depth {d}"),
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

    #[test]
    fn go_clock_command() {
        assert_eq!(
            GoLimits::Clock {
                wtime: Duration::from_millis(60_000),
                btime: Duration::from_millis(59_400),
                winc: Duration::from_millis(600),
                binc: Duration::from_millis(600),
            }
            .to_command(),
            "go wtime 60000 btime 59400 winc 600 binc 600"
        );
    }

    #[test]
    fn go_sudden_death_has_zero_increment() {
        assert_eq!(
            GoLimits::Clock {
                wtime: Duration::from_millis(1000),
                btime: Duration::from_millis(1000),
                winc: Duration::ZERO,
                binc: Duration::ZERO,
            }
            .to_command(),
            "go wtime 1000 btime 1000 winc 0 binc 0"
        );
    }

    #[test]
    fn go_nodes_and_depth_commands() {
        assert_eq!(GoLimits::Nodes(50_000).to_command(), "go nodes 50000");
        assert_eq!(GoLimits::Depth(12).to_command(), "go depth 12");
    }
}
