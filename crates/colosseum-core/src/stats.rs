//! Match statistics for two-engine comparisons: Elo with confidence interval,
//! likelihood-of-superiority (LOS), and a sequential probability ratio test
//! (SPRT). All pure functions over win/draw/loss counts so they are easy to
//! unit-test and reuse from both the live view and the History tab.

use crate::standings::PairGameResult;

/// One of the five possible scores for a two-game, colour-reversed opening
/// pair, from the tested engine's perspective.
///
/// The discriminant is the index in a pentanomial vector ordered as
/// `[0, 0.5, 1, 1.5, 2]` points. The central bin combines draw/draw and a
/// win/loss split because both score one point from the pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum PentanomialBin {
    Zero = 0,
    Half = 1,
    One = 2,
    OneAndHalf = 3,
    Two = 4,
}

impl PentanomialBin {
    /// Map the two game results from one opening pair to its pentanomial bin.
    #[must_use]
    pub fn from_pair(first: PairGameResult, second: PairGameResult) -> Self {
        let score = first.points_twice() + second.points_twice();
        match score {
            0 => Self::Zero,
            1 => Self::Half,
            2 => Self::One,
            3 => Self::OneAndHalf,
            4 => Self::Two,
            _ => unreachable!("two chess game results score from 0 to 4 half-points"),
        }
    }

    /// Index in the `[0, 0.5, 1, 1.5, 2]` pentanomial vector.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }
}

/// Counts of complete colour-reversed pairs, ordered by pair score
/// `[0, 0.5, 1, 1.5, 2]`.
///
/// An incomplete colour pair is deliberately kept out of `counts`: it is
/// recorded in `unpaired_games` for an explicitly labelled non-pentanomial
/// fallback, and must never be supplied to pentanomial SPRT calculations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PentanomialVector {
    counts: [u32; 5],
    unpaired_games: u32,
}

impl PentanomialVector {
    /// Record one complete opening pair.
    ///
    /// The caller establishes that both results use the same opening with
    /// colours reversed; this value intentionally carries no scheduler or
    /// persistence identity.
    pub fn record_pair(&mut self, first: PairGameResult, second: PairGameResult) {
        let bin = PentanomialBin::from_pair(first, second);
        self.counts[bin.index()] += 1;
    }

    /// Record one completed game whose colour-reversed companion is absent.
    ///
    /// This does not change the pentanomial vector or its pair count.
    pub fn record_unpaired_game(&mut self) {
        self.unpaired_games += 1;
    }

    /// The five bin counts in score order `[0, 0.5, 1, 1.5, 2]`.
    #[must_use]
    pub const fn counts(&self) -> [u32; 5] {
        self.counts
    }

    /// Number of complete pairs represented by this vector.
    #[must_use]
    pub const fn pairs(&self) -> u32 {
        self.counts[0] + self.counts[1] + self.counts[2] + self.counts[3] + self.counts[4]
    }

    /// Completed games excluded because their colour-reversed companion is absent.
    #[must_use]
    pub const fn unpaired_games(&self) -> u32 {
        self.unpaired_games
    }
}

/// Standard normal cumulative distribution function Φ(x), via an `erf`
/// approximation (Abramowitz & Stegun 7.1.26, ~1e-7 accuracy).
#[must_use]
pub fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

/// Error function approximation (Abramowitz & Stegun 7.1.26).
fn erf(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-x * x).exp();
    sign * y
}

/// Convert an expected score in (0, 1) to an Elo difference.
#[must_use]
pub fn score_to_elo(score: f64) -> f64 {
    -400.0 * (1.0 / score - 1.0).log10()
}

/// Convert an Elo difference to an expected score in (0, 1).
#[must_use]
pub fn elo_to_score(elo: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf(-elo / 400.0))
}

/// Elo estimate with a symmetric-ish confidence interval (in Elo points).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EloEstimate {
    /// Point estimate of the Elo difference (engine A minus engine B).
    pub elo: f64,
    /// Expected score (points / games) for engine A.
    pub score: f64,
    /// Lower bound of the Elo confidence interval.
    pub lower: f64,
    /// Upper bound of the Elo confidence interval.
    pub upper: f64,
}

impl EloEstimate {
    /// Half-width of the interval, i.e. the "± margin".
    #[must_use]
    pub fn margin(&self) -> f64 {
        (self.upper - self.lower) / 2.0
    }
}

/// Estimate the Elo difference for engine A (wins/draws/losses from A's view)
/// with a confidence interval at the given `z` (e.g. 1.96 for 95%).
///
/// Returns `None` when there are no games, or the score is a clean 0 or 1
/// (Elo is undefined / infinite there).
#[must_use]
pub fn elo_with_error(wins: u32, draws: u32, losses: u32, z: f64) -> Option<EloEstimate> {
    let n = wins + draws + losses;
    if n == 0 {
        return None;
    }
    let n_f = f64::from(n);
    let w = f64::from(wins);
    let d = f64::from(draws);
    let l = f64::from(losses);

    let mu = (w + 0.5 * d) / n_f;
    if mu <= 0.0 || mu >= 1.0 {
        return None;
    }

    // Per-game score variance, then standard error of the mean.
    let var = (w * (1.0 - mu).powi(2) + d * (0.5 - mu).powi(2) + l * (0.0 - mu).powi(2)) / n_f;
    let stderr = (var / n_f).sqrt();

    let lo_score = (mu - z * stderr).clamp(1e-9, 1.0 - 1e-9);
    let hi_score = (mu + z * stderr).clamp(1e-9, 1.0 - 1e-9);

    Some(EloEstimate {
        elo: score_to_elo(mu),
        score: mu,
        lower: score_to_elo(lo_score),
        upper: score_to_elo(hi_score),
    })
}

/// Likelihood that engine A is stronger than engine B, from decisive games only.
/// Returns 0.5 when there are no decisive games.
#[must_use]
pub fn los(wins: u32, losses: u32) -> f64 {
    let decisive = wins + losses;
    if decisive == 0 {
        return 0.5;
    }
    let diff = f64::from(wins) - f64::from(losses);
    normal_cdf(diff / f64::from(decisive).sqrt())
}

/// SPRT verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SprtDecision {
    /// LLR crossed the lower bound: accept H0 (no improvement).
    AcceptH0,
    /// LLR crossed the upper bound: accept H1 (improvement).
    AcceptH1,
    /// Inconclusive so far; keep playing.
    Continue,
}

/// Result of a sequential probability ratio test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SprtResult {
    /// Log-likelihood ratio of H1 over H0.
    pub llr: f64,
    /// Lower decision bound (accept H0 at or below).
    pub lower: f64,
    /// Upper decision bound (accept H1 at or above).
    pub upper: f64,
    pub decision: SprtDecision,
}

/// Sequential probability ratio test for "is engine A stronger than B?".
///
/// H0: Elo difference = `elo0`; H1: Elo difference = `elo1` (`elo1 > elo0`).
/// Uses the trinomial model with the *observed* draw rate held fixed between
/// the hypotheses (the win/loss split is set to match each hypothesis' expected
/// score) — the standard approach used by cutechess-cli. The draw term then
/// cancels from the LLR. `alpha`/`beta` are the type-I/II error rates.
#[must_use]
pub fn sprt(
    wins: u32,
    draws: u32,
    losses: u32,
    elo0: f64,
    elo1: f64,
    alpha: f64,
    beta: f64,
) -> SprtResult {
    let lower = (beta / (1.0 - alpha)).ln();
    let upper = ((1.0 - beta) / alpha).ln();

    let n = wins + draws + losses;
    let llr = if n == 0 {
        0.0
    } else {
        let n_f = f64::from(n);
        let draw_rate = f64::from(draws) / n_f;
        // Win/loss probabilities under each hypothesis, draw rate fixed.
        let split = |elo: f64| {
            let s = elo_to_score(elo);
            let pw = (s - draw_rate / 2.0).clamp(1e-9, 1.0);
            let pl = (1.0 - s - draw_rate / 2.0).clamp(1e-9, 1.0);
            (pw, pl)
        };
        let (w0, l0) = split(elo0);
        let (w1, l1) = split(elo1);
        f64::from(wins) * (w1 / w0).ln() + f64::from(losses) * (l1 / l0).ln()
    };

    let decision = if llr >= upper {
        SprtDecision::AcceptH1
    } else if llr <= lower {
        SprtDecision::AcceptH0
    } else {
        SprtDecision::Continue
    };

    SprtResult {
        llr,
        lower,
        upper,
        decision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f64, b: f64, eps: f64) -> bool {
        (a - b).abs() < eps
    }

    #[test]
    fn elo_score_roundtrip() {
        for &s in &[0.25, 0.4, 0.5, 0.6, 0.75] {
            assert!(approx(elo_to_score(score_to_elo(s)), s, 1e-9));
        }
        assert!(approx(score_to_elo(0.5), 0.0, 1e-9));
    }

    #[test]
    fn even_match_is_zero_elo_and_half_los() {
        let est = elo_with_error(10, 0, 10, 1.96).unwrap();
        assert!(approx(est.elo, 0.0, 1e-6));
        assert!(est.lower < 0.0 && est.upper > 0.0);
        assert!(approx(los(10, 10), 0.5, 1e-9));
    }

    #[test]
    fn winning_record_is_positive_elo_and_high_los() {
        let est = elo_with_error(70, 20, 10, 1.96).unwrap();
        assert!(est.elo > 0.0);
        assert!(est.lower < est.elo && est.elo < est.upper);
        assert!(los(70, 10) > 0.99);
    }

    #[test]
    fn elo_undefined_at_clean_sweep_or_no_games() {
        assert!(elo_with_error(0, 0, 0, 1.96).is_none());
        assert!(elo_with_error(5, 0, 0, 1.96).is_none()); // score = 1.0
        assert!(elo_with_error(0, 0, 5, 1.96).is_none()); // score = 0.0
    }

    #[test]
    fn sprt_bounds_match_error_rates() {
        let r = sprt(0, 0, 0, 0.0, 5.0, 0.05, 0.05);
        assert!(approx(r.lower, (0.05f64 / 0.95).ln(), 1e-9));
        assert!(approx(r.upper, (0.95f64 / 0.05).ln(), 1e-9));
        assert_eq!(r.decision, SprtDecision::Continue);
        assert!(approx(r.llr, 0.0, 1e-12));
    }

    #[test]
    fn sprt_accepts_h1_for_a_strong_result() {
        // A dominant score should eventually push the LLR over the upper bound.
        let r = sprt(700, 200, 100, 0.0, 5.0, 0.05, 0.05);
        assert_eq!(r.decision, SprtDecision::AcceptH1);
        assert!(r.llr >= r.upper);
    }

    #[test]
    fn sprt_accepts_h0_for_a_losing_result() {
        let r = sprt(100, 200, 700, 0.0, 5.0, 0.05, 0.05);
        assert_eq!(r.decision, SprtDecision::AcceptH0);
        assert!(r.llr <= r.lower);
    }

    #[test]
    fn pentanomial_bins_cover_every_two_game_score() {
        use PairGameResult::{Draw, Loss, Win};

        let cases = [
            ((Loss, Loss), PentanomialBin::Zero),
            ((Loss, Draw), PentanomialBin::Half),
            ((Draw, Loss), PentanomialBin::Half),
            ((Loss, Win), PentanomialBin::One),
            ((Draw, Draw), PentanomialBin::One),
            ((Win, Loss), PentanomialBin::One),
            ((Draw, Win), PentanomialBin::OneAndHalf),
            ((Win, Draw), PentanomialBin::OneAndHalf),
            ((Win, Win), PentanomialBin::Two),
        ];

        for ((first, second), expected) in cases {
            assert_eq!(PentanomialBin::from_pair(first, second), expected);
        }
    }

    #[test]
    fn pentanomial_vector_keeps_incomplete_pairs_out_of_the_bins() {
        use PairGameResult::{Draw, Loss, Win};

        let mut vector = PentanomialVector::default();
        vector.record_pair(Loss, Loss);
        vector.record_pair(Loss, Draw);
        vector.record_pair(Draw, Loss);
        vector.record_pair(Loss, Win);
        vector.record_pair(Draw, Draw);
        vector.record_pair(Win, Loss);
        vector.record_pair(Draw, Win);
        vector.record_pair(Win, Draw);
        vector.record_pair(Win, Win);

        // This game is visibly retained for an unpaired fallback but cannot
        // affect any future pentanomial SPRT input.
        vector.record_unpaired_game();

        assert_eq!(vector.counts(), [1, 2, 3, 2, 1]);
        assert_eq!(vector.pairs(), 9);
        assert_eq!(vector.unpaired_games(), 1);
    }
}
