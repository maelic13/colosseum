//! Engine search scores, as reported in `info score ...` lines (from the perspective
//! of the side to move).

/// A score reported by an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Score {
    /// Centipawns (positive = good for the side to move).
    Cp(i32),
    /// Mate in `n` moves (positive = side to move delivers mate; negative = is mated).
    Mate(i32),
}

impl Score {
    /// Collapse to a single centipawn value for adjudication, mapping mate scores to a
    /// large magnitude near `mate_value`. Still from the side-to-move perspective; the
    /// game runner normalizes to White's point of view.
    #[must_use]
    pub fn to_cp(self, mate_value: i32) -> i32 {
        match self {
            Self::Cp(cp) => cp,
            // Closer mates map to larger magnitudes.
            Self::Mate(m) if m >= 0 => mate_value - m,
            Self::Mate(m) => -mate_value - m,
        }
    }
}
