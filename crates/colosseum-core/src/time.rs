//! Time control model. Supports time-per-move, whole-game clocks (sudden death and
//! Fischer increment), and fixed nodes/depth per move. The enum is the seam these
//! grew from; new variants are appended (never reordered) to keep persisted
//! tournaments deserializable.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How an engine's thinking time is bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeControl {
    /// Fixed wall-clock time per move (`go movetime <ms>`).
    PerMove { ms: u64 },
    /// A whole-game time budget per side with no increment; flagging loses
    /// (`go wtime <ms> btime <ms>`).
    SuddenDeath { base_ms: u64 },
    /// A base time budget per side plus a per-move Fischer increment
    /// (`go wtime btime winc binc`).
    Increment { base_ms: u64, inc_ms: u64 },
    /// Fixed node count per move (`go nodes <n>`).
    Nodes { nodes: u64 },
    /// Fixed search depth per move (`go depth <d>`).
    Depth { depth: u32 },
}

impl TimeControl {
    /// The per-move duration, when the control expresses one (`PerMove` only).
    #[must_use]
    pub fn movetime(&self) -> Option<Duration> {
        match self {
            Self::PerMove { ms } => Some(Duration::from_millis(*ms)),
            _ => None,
        }
    }

    /// Whether this control runs a per-side game clock (sudden death / increment).
    #[must_use]
    pub fn is_clock(&self) -> bool {
        matches!(self, Self::SuddenDeath { .. } | Self::Increment { .. })
    }

    /// The starting per-side clock for clock-based controls.
    #[must_use]
    pub fn initial_clock(&self) -> Option<Duration> {
        match self {
            Self::SuddenDeath { base_ms } | Self::Increment { base_ms, .. } => {
                Some(Duration::from_millis(*base_ms))
            }
            _ => None,
        }
    }

    /// The Fischer increment added after each move (zero unless `Increment`).
    #[must_use]
    pub fn increment(&self) -> Duration {
        match self {
            Self::Increment { inc_ms, .. } => Duration::from_millis(*inc_ms),
            _ => Duration::ZERO,
        }
    }
}

impl Default for TimeControl {
    fn default() -> Self {
        Self::PerMove { ms: 100 }
    }
}

/// Units offered by the time-control widget (value + unit dropdown).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeUnit {
    Milliseconds,
    Seconds,
    Minutes,
}

impl TimeUnit {
    /// Convert a value in this unit to milliseconds.
    #[must_use]
    pub fn to_millis(self, value: f64) -> u64 {
        let factor = match self {
            Self::Milliseconds => 1.0,
            Self::Seconds => 1_000.0,
            Self::Minutes => 60_000.0,
        };
        (value * factor).round().max(0.0) as u64
    }

    /// Short label for the dropdown.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Milliseconds => "ms",
            Self::Seconds => "s",
            Self::Minutes => "min",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn movetime_only_for_per_move() {
        assert_eq!(
            TimeControl::PerMove { ms: 250 }.movetime(),
            Some(Duration::from_millis(250))
        );
        assert_eq!(TimeControl::SuddenDeath { base_ms: 1000 }.movetime(), None);
        assert_eq!(
            TimeControl::Increment {
                base_ms: 1000,
                inc_ms: 100
            }
            .movetime(),
            None
        );
        assert_eq!(TimeControl::Nodes { nodes: 1000 }.movetime(), None);
        assert_eq!(TimeControl::Depth { depth: 12 }.movetime(), None);
    }

    #[test]
    fn clock_controls_report_initial_clock_and_increment() {
        let sd = TimeControl::SuddenDeath { base_ms: 60_000 };
        assert!(sd.is_clock());
        assert_eq!(sd.initial_clock(), Some(Duration::from_millis(60_000)));
        assert_eq!(sd.increment(), Duration::ZERO);

        let inc = TimeControl::Increment {
            base_ms: 60_000,
            inc_ms: 600,
        };
        assert!(inc.is_clock());
        assert_eq!(inc.initial_clock(), Some(Duration::from_millis(60_000)));
        assert_eq!(inc.increment(), Duration::from_millis(600));
    }

    #[test]
    fn non_clock_controls_have_no_clock() {
        for tc in [
            TimeControl::PerMove { ms: 100 },
            TimeControl::Nodes { nodes: 50_000 },
            TimeControl::Depth { depth: 8 },
        ] {
            assert!(!tc.is_clock());
            assert_eq!(tc.initial_clock(), None);
            assert_eq!(tc.increment(), Duration::ZERO);
        }
    }
}
