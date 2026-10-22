//! The Elo of one pair's W/D/L by the trinomial normal approximation, the
//! usual hand calculation from a crosstable cell, reproduced exactly,
//! including the fixed 1.96 quantile.

use serde::{Deserialize, Serialize};

/// The two-sided 95% quantile the trinomial interval is defined with.
pub const TRINOMIAL_Z: f64 = 1.96;

/// One engine's games against one opponent, and what they are worth in Elo.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScoreElo {
    pub games: u32,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    /// Points per game, absent without games.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// `-400 log10(1/score - 1)`; absent at a score of 0 or 1, where the
    /// logistic Elo is unbounded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elo: Option<f64>,
    /// The Elo at `score - 1.96 se`; absent when that score is not above 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elo_lower: Option<f64>,
    /// The Elo at `score + 1.96 se`; absent when that score is not below 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elo_upper: Option<f64>,
}

/// The logistic Elo difference a score per game stands for, when it is
/// finite.
#[must_use]
pub fn logistic_elo(score: f64) -> Option<f64> {
    (score > 0.0 && score < 1.0).then(|| -400.0 * (1.0 / score - 1.0).log10())
}

/// W/D/L and its Elo with a 95% interval by the trinomial normal
/// approximation:
///
/// ```text
/// p   = (W + D/2) / n
/// var = (W + D/4) / n - p^2
/// se  = sqrt(var / n)
/// Elo = -400 log10(1/p - 1), interval at p -/+ 1.96 se
/// ```
#[must_use]
pub fn trinomial_elo(wins: u32, draws: u32, losses: u32) -> ScoreElo {
    let games = wins + draws + losses;
    let mut result = ScoreElo {
        games,
        wins,
        draws,
        losses,
        score: None,
        elo: None,
        elo_lower: None,
        elo_upper: None,
    };
    if games == 0 {
        return result;
    }
    let n = f64::from(games);
    let p = (f64::from(wins) + f64::from(draws) / 2.0) / n;
    let variance = ((f64::from(wins) + f64::from(draws) / 4.0) / n - p * p).max(0.0);
    let margin = TRINOMIAL_Z * (variance / n).sqrt();
    result.score = Some(p);
    result.elo = logistic_elo(p);
    result.elo_lower = logistic_elo(p - margin);
    result.elo_upper = logistic_elo(p + margin);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: Option<f64>, expected: f64, tolerance: f64) {
        let actual = actual.expect("a finite value");
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} != {expected}"
        );
    }

    /// Two crosstable cells, 248-88-64 and 347-44-9, worked by hand to one
    /// decimal.
    #[test]
    fn the_trinomial_interval_reproduces_the_hand_calculation() {
        let houdini = trinomial_elo(248, 88, 64);
        assert_eq!(houdini.games, 400);
        close(houdini.score, 0.73, 1e-12);
        close(houdini.elo, 172.8, 0.05);
        close(houdini.elo_lower, 141.5, 0.05);
        close(houdini.elo_upper, 206.9, 0.05);
        let basilisk = trinomial_elo(347, 44, 9);
        close(basilisk.elo, 430.3, 0.05);
        close(basilisk.elo_lower, 385.5, 0.05);
        close(basilisk.elo_upper, 487.6, 0.05);
    }

    #[test]
    fn a_perfect_or_empty_score_has_no_finite_elo() {
        let empty = trinomial_elo(0, 0, 0);
        assert_eq!((empty.score, empty.elo), (None, None));
        let perfect = trinomial_elo(10, 0, 0);
        assert_eq!(perfect.score, Some(1.0));
        assert_eq!((perfect.elo, perfect.elo_upper), (None, None));
    }
}
