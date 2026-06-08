//! Time control model. v1 supports time-per-move; the enum is the seam that grows
//! into base+increment / total-game-time / nodes / depth without touching callers.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long an engine is given to choose each move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeControl {
    /// Fixed wall-clock time per move (`go movetime <ms>`).
    PerMove { ms: u64 },
    // Future variants (do not reorder existing ones):
    // Increment { base_ms: u64, inc_ms: u64 },
    // Nodes(u64),
    // Depth(u32),
}

impl TimeControl {
    /// The per-move duration, when the control expresses one.
    #[must_use]
    pub fn movetime(&self) -> Option<Duration> {
        match self {
            Self::PerMove { ms } => Some(Duration::from_millis(*ms)),
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
