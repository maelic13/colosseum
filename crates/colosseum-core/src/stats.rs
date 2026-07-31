//! Match statistics for two-engine comparisons: Elo with confidence interval,
//! likelihood-of-superiority (LOS), and a sequential probability ratio test
//! (SPRT). The calculations are pure functions over game or paired-outcome
//! counts so they are easy to unit-test and reuse across presentation layers.

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
    win_loss_pairs: u32,
    double_draw_pairs: u32,
    drawn_games: u32,
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

        self.drawn_games += match first {
            PairGameResult::Draw => 1,
            PairGameResult::Win | PairGameResult::Loss => 0,
        };
        self.drawn_games += match second {
            PairGameResult::Draw => 1,
            PairGameResult::Win | PairGameResult::Loss => 0,
        };

        match (first, second) {
            (PairGameResult::Win, PairGameResult::Loss)
            | (PairGameResult::Loss, PairGameResult::Win) => self.win_loss_pairs += 1,
            (PairGameResult::Draw, PairGameResult::Draw) => self.double_draw_pairs += 1,
            _ => {}
        }
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

    /// Fraction of individual games drawn within complete pairs.
    #[must_use]
    pub fn draw_ratio(&self) -> Option<f64> {
        let pairs = self.pairs();
        if pairs == 0 {
            None
        } else {
            Some(f64::from(self.drawn_games) / (2.0 * f64::from(pairs)))
        }
    }

    /// Ratio of pairs scoring above one point to pairs scoring below one point.
    ///
    /// Returns `None` when there are no losing pairs, rather than producing an
    /// infinite or undefined value.
    #[must_use]
    pub fn pairs_ratio(&self) -> Option<f64> {
        let losing_pairs = self.counts[0] + self.counts[1];
        if losing_pairs == 0 {
            None
        } else {
            Some(f64::from(self.counts[3] + self.counts[4]) / f64::from(losing_pairs))
        }
    }

    /// Ratio of one-win/one-loss pairs to double-draw pairs.
    ///
    /// Both outcomes occupy the central one-point pentanomial bin, so their
    /// split is retained separately. Returns `None` when there are no
    /// double-draw pairs.
    #[must_use]
    pub fn win_loss_to_double_draw_ratio(&self) -> Option<f64> {
        if self.double_draw_pairs == 0 {
            None
        } else {
            Some(f64::from(self.win_loss_pairs) / f64::from(self.double_draw_pairs))
        }
    }

    /// `(win/loss, draw/draw)` counts inside the central one-point bin.
    #[must_use]
    pub const fn central_pair_breakdown(&self) -> (u32, u32) {
        (self.win_loss_pairs, self.double_draw_pairs)
    }
}

/// Normalized-Elo estimate and confidence interval.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalizedEloEstimate {
    pub elo: f64,
    pub lower: f64,
    pub upper: f64,
}

impl NormalizedEloEstimate {
    /// Half-width of the interval, i.e. the "± margin".
    #[must_use]
    pub fn margin(&self) -> f64 {
        (self.upper - self.lower) / 2.0
    }
}

/// Estimates derived from complete colour-reversed pairs.
///
/// `variance` is the empirical population variance of the pair-average score
/// values `[0, 0.25, 0.5, 0.75, 1]`. `standard_error` is therefore
/// `sqrt(variance / pairs)`. Unpaired games are reported but excluded from
/// every field in this structure.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PentanomialStatistics {
    pub pairs: u32,
    pub unpaired_games: u32,
    pub score: f64,
    pub variance: f64,
    pub standard_error: f64,
    pub logistic_elo: EloEstimate,
    pub normalized_elo: NormalizedEloEstimate,
    pub los: f64,
    pub draw_ratio: f64,
    pub pairs_ratio: Option<f64>,
    pub win_loss_to_double_draw_ratio: Option<f64>,
}

/// Calculate paired statistics with a two-sided confidence interval at `z`
/// standard deviations (for example, `1.959963984540054` for 95%).
///
/// Returns `None` for fewer than two pairs, zero variance, non-finite input or
/// an interval that leaves the finite logistic-score domain. Phase 1.5 replaces
/// this temporary absence signal with the plan's typed statistical errors.
#[must_use]
pub fn pentanomial_statistics(sample: &PentanomialVector, z: f64) -> Option<PentanomialStatistics> {
    const NORMALIZED_ELO_SCALE: f64 = 800.0 / std::f64::consts::LN_10;

    let pairs = sample.pairs();
    if pairs < 2 || !z.is_finite() || z < 0.0 {
        return None;
    }

    let pairs_f = f64::from(pairs);
    let counts = sample.counts();
    let score = counts
        .iter()
        .zip(PENTANOMIAL_SCORES)
        .map(|(&count, value)| f64::from(count) * value)
        .sum::<f64>()
        / pairs_f;
    let variance = counts
        .iter()
        .zip(PENTANOMIAL_SCORES)
        .map(|(&count, value)| f64::from(count) * (value - score).powi(2))
        .sum::<f64>()
        / pairs_f;

    if !(0.0..1.0).contains(&score) || !variance.is_finite() || variance <= 0.0 {
        return None;
    }

    let standard_error = (variance / pairs_f).sqrt();
    let lower_score = score - z * standard_error;
    let upper_score = score + z * standard_error;
    if !(0.0..1.0).contains(&lower_score) || !(0.0..1.0).contains(&upper_score) {
        return None;
    }

    let normalized_scale = NORMALIZED_ELO_SCALE / (2.0 * variance).sqrt();
    let normalized = |value: f64| (value - 0.5) * normalized_scale;

    Some(PentanomialStatistics {
        pairs,
        unpaired_games: sample.unpaired_games(),
        score,
        variance,
        standard_error,
        logistic_elo: EloEstimate {
            elo: score_to_elo(score),
            score,
            lower: score_to_elo(lower_score),
            upper: score_to_elo(upper_score),
        },
        normalized_elo: NormalizedEloEstimate {
            elo: normalized(score),
            lower: normalized(lower_score),
            upper: normalized(upper_score),
        },
        los: normal_cdf((score - 0.5) / standard_error),
        draw_ratio: sample.draw_ratio()?,
        pairs_ratio: sample.pairs_ratio(),
        win_loss_to_double_draw_ratio: sample.win_loss_to_double_draw_ratio(),
    })
}

/// Standard normal cumulative distribution function Φ(x), via an `erf`
/// approximation (Abramowitz & Stegun 7.1.26, ~1e-7 accuracy).
#[must_use]
pub fn normal_cdf(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / std::f64::consts::SQRT_2))
}

/// Invert [`normal_cdf`] for an interior probability.
fn normal_quantile(probability: f64) -> Option<f64> {
    if !probability.is_finite() || !(0.0..1.0).contains(&probability) {
        return None;
    }

    let mut lower = -8.0;
    let mut upper = 8.0;
    if probability <= normal_cdf(lower) || probability >= normal_cdf(upper) {
        return None;
    }
    for _ in 0..80 {
        let midpoint = (lower + upper) / 2.0;
        if normal_cdf(midpoint) < probability {
            lower = midpoint;
        } else {
            upper = midpoint;
        }
    }
    Some((lower + upper) / 2.0)
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

/// Elo parameterization used by paired statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EloModel {
    /// Constrain the expected pair-average score after applying the logistic
    /// Elo-to-score transform.
    Logistic,
    /// Constrain the standardized pair-average score represented by
    /// normalized Elo.
    Normalized,
}

impl EloModel {
    /// Stable user-facing name for reports and persisted run records.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Logistic => "logistic",
            Self::Normalized => "normalized",
        }
    }
}

/// Assumed distribution of pair-average scores `[0, 0.25, 0.5, 0.75, 1]`.
///
/// Fixed-N planning requires this explicit input because game count alone does
/// not determine the variance, and therefore cannot determine detectable Elo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PentanomialDistribution {
    probabilities: [f64; 5],
}

impl PentanomialDistribution {
    /// Validate and normalize five finite, non-negative probabilities.
    ///
    /// Returns `None` unless they sum to one (within floating-point input
    /// tolerance) and describe a non-degenerate distribution.
    #[must_use]
    pub fn new(probabilities: [f64; 5]) -> Option<Self> {
        if probabilities
            .iter()
            .any(|probability| !probability.is_finite() || *probability < 0.0)
        {
            return None;
        }
        let total = probabilities.iter().sum::<f64>();
        if !total.is_finite() || (total - 1.0).abs() > 1e-9 {
            return None;
        }

        let probabilities = probabilities.map(|probability| probability / total);
        let distribution = Self { probabilities };
        (distribution.variance() > 0.0).then_some(distribution)
    }

    #[must_use]
    pub const fn probabilities(self) -> [f64; 5] {
        self.probabilities
    }

    #[must_use]
    pub fn mean(self) -> f64 {
        distribution_stats(&PENTANOMIAL_SCORES, &self.probabilities).0
    }

    #[must_use]
    pub fn variance(self) -> f64 {
        distribution_stats(&PENTANOMIAL_SCORES, &self.probabilities).1
    }
}

/// Rejection region used by a fixed-sample difference test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedNTestTails {
    /// Detect an effect in the pre-declared direction.
    OneSided,
    /// Detect an effect in either direction.
    TwoSided,
}

impl FixedNTestTails {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OneSided => "one-sided",
            Self::TwoSided => "two-sided",
        }
    }
}

/// Prospective normal-approximation design for a fixed-sample difference test.
///
/// Every planning assumption is retained in the result. This is a design
/// estimate, not a stopping rule and not an SPRT conclusion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixedNPlan {
    pub model: EloModel,
    pub tails: FixedNTestTails,
    /// Positive Elo distance from the null to the alternative, in `model`.
    pub target_effect: f64,
    pub significance: f64,
    pub power: f64,
    pub assumed_distribution: PentanomialDistribution,
    /// Target effect after conversion to pair-average score units.
    pub score_effect: f64,
    pub critical_value: f64,
    pub power_quantile: f64,
    pub required_pairs: u64,
}

impl FixedNPlan {
    /// Games implied by two colour-reversed games per complete pair.
    #[must_use]
    pub const fn required_games(&self) -> u64 {
        self.required_pairs * 2
    }
}

/// Estimate the required complete pairs for a fixed-sample difference test.
///
/// The null is zero Elo and `target_effect` is a positive alternative in the
/// selected model. `significance` is alpha; `power` is `1-beta`. The assumed
/// pentanomial variance is treated as known and the pair mean as normally
/// distributed. The returned count is rounded up and never below two pairs.
///
/// This primitive does not implement equivalence/TOST planning; that objective
/// requires an equivalence margin and an assumed true effect, and is composed
/// separately by the experiment-planning workflow.
#[must_use]
pub fn fixed_n_plan(
    assumed_distribution: PentanomialDistribution,
    model: EloModel,
    tails: FixedNTestTails,
    target_effect: f64,
    significance: f64,
    power: f64,
) -> Option<FixedNPlan> {
    if !target_effect.is_finite()
        || target_effect <= 0.0
        || !valid_significance(significance)
        || !power.is_finite()
        || power <= 0.5
        || power >= 1.0
    {
        return None;
    }

    let variance = assumed_distribution.variance();
    let score_effect = match model {
        EloModel::Logistic => elo_to_score(target_effect) - 0.5,
        EloModel::Normalized => target_effect * (2.0 * variance).sqrt() / NELO_PER_T_VALUE,
    };
    if !score_effect.is_finite() || score_effect <= 0.0 || score_effect >= 0.5 {
        return None;
    }

    let critical_probability = match tails {
        FixedNTestTails::OneSided => 1.0 - significance,
        FixedNTestTails::TwoSided => 1.0 - significance / 2.0,
    };
    let critical_value = normal_quantile(critical_probability)?;
    let power_quantile = normal_quantile(power)?;
    let required = variance * ((critical_value + power_quantile) / score_effect).powi(2);
    if !required.is_finite() || required <= 0.0 || required > (u64::MAX / 2) as f64 {
        return None;
    }

    Some(FixedNPlan {
        model,
        tails,
        target_effect,
        significance,
        power,
        assumed_distribution,
        score_effect,
        critical_value,
        power_quantile,
        required_pairs: (required.ceil() as u64).max(2),
    })
}

/// Retrospective confidence interval and conservative achieved resolution.
///
/// This is descriptive fixed-sample output. It deliberately has no hypothesis
/// decision field and must not be presented as post-hoc power, a back-fitted
/// MDE, or an SPRT verdict.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixedNAchievedResolution {
    pub model: EloModel,
    pub pairs: u32,
    pub unpaired_games: u32,
    pub significance: f64,
    pub confidence: f64,
    pub estimate: f64,
    pub lower: f64,
    pub upper: f64,
}

impl FixedNAchievedResolution {
    #[must_use]
    pub fn lower_error(&self) -> f64 {
        self.estimate - self.lower
    }

    #[must_use]
    pub fn upper_error(&self) -> f64 {
        self.upper - self.estimate
    }

    /// Larger distance from the estimate to either interval endpoint.
    ///
    /// Normalized-Elo intervals are symmetric; logistic-Elo intervals need not
    /// be, so using the larger side avoids overstating achieved precision.
    #[must_use]
    pub fn resolution(&self) -> f64 {
        self.lower_error().max(self.upper_error())
    }
}

/// Calculate the achieved two-sided `(1-significance)` interval of a completed
/// fixed sample in the explicitly selected model.
#[must_use]
pub fn fixed_n_achieved_resolution(
    sample: &PentanomialVector,
    model: EloModel,
    significance: f64,
) -> Option<FixedNAchievedResolution> {
    if !valid_significance(significance) {
        return None;
    }
    let critical_value = normal_quantile(1.0 - significance / 2.0)?;
    let statistics = pentanomial_statistics(sample, critical_value)?;
    let (estimate, lower, upper) = match model {
        EloModel::Logistic => (
            statistics.logistic_elo.elo,
            statistics.logistic_elo.lower,
            statistics.logistic_elo.upper,
        ),
        EloModel::Normalized => (
            statistics.normalized_elo.elo,
            statistics.normalized_elo.lower,
            statistics.normalized_elo.upper,
        ),
    };

    Some(FixedNAchievedResolution {
        model,
        pairs: statistics.pairs,
        unpaired_games: statistics.unpaired_games,
        significance,
        confidence: 1.0 - significance,
        estimate,
        lower,
        upper,
    })
}

/// Self-contained result of a paired pentanomial SPRT.
///
/// The model and hypotheses are retained with the LLR so callers cannot report
/// a result whose Elo parameterization is ambiguous.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PentanomialSprtResult {
    pub model: EloModel,
    /// Complete pairs in the official sequential sample.
    pub pairs: u32,
    /// Completed games reported for diagnostics but excluded from the LLR.
    pub unpaired_games: u32,
    pub elo0: f64,
    pub elo1: f64,
    pub alpha: f64,
    pub beta: f64,
    /// Generalized log-likelihood ratio of H1 over H0.
    pub llr: f64,
    /// Wald lower decision bound (accept H0 at or below).
    pub lower: f64,
    /// Wald upper decision bound (accept H1 at or above).
    pub upper: f64,
    pub decision: SprtDecision,
}

/// Sequential probability ratio test over complete colour-reversed pairs.
///
/// H0 is `elo0` and H1 is `elo1` in the explicitly selected model. Logistic
/// Elo constrains the expected pair-average score; normalized Elo constrains
/// its standardized mean. In both cases the LLR compares the two constrained
/// maximum-likelihood pentanomial distributions.
///
/// Unpaired games are excluded. Until Phase 1.5 introduces typed statistical
/// errors, `None` reports invalid hypotheses/error rates, fewer than two pairs,
/// a degenerate observed distribution, or a failed likelihood solve.
#[must_use]
pub fn pentanomial_sprt(
    sample: &PentanomialVector,
    model: EloModel,
    elo0: f64,
    elo1: f64,
    alpha: f64,
    beta: f64,
) -> Option<PentanomialSprtResult> {
    let pairs = sample.pairs();
    if pairs < 2
        || !elo0.is_finite()
        || !elo1.is_finite()
        || elo0 >= elo1
        || !valid_error_rate(alpha)
        || !valid_error_rate(beta)
        || alpha + beta >= 1.0
        || observed_variance(sample.counts()) <= 0.0
    {
        return None;
    }

    let llr = pentanomial_llr(sample.counts(), model, elo0, elo1)?;
    let lower = (beta / (1.0 - alpha)).ln();
    let upper = ((1.0 - beta) / alpha).ln();
    if !llr.is_finite() || !lower.is_finite() || !upper.is_finite() {
        return None;
    }

    Some(PentanomialSprtResult {
        model,
        pairs,
        unpaired_games: sample.unpaired_games(),
        elo0,
        elo1,
        alpha,
        beta,
        llr,
        lower,
        upper,
        decision: sprt_decision(llr, lower, upper),
    })
}

const PENTANOMIAL_SCORES: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
const EMPTY_BIN_PRIOR: f64 = 1e-3;
const NELO_PER_T_VALUE: f64 = 800.0 / std::f64::consts::LN_10;

fn valid_error_rate(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value < 1.0
}

fn valid_significance(value: f64) -> bool {
    value.is_finite() && value > 0.0 && value < 0.5
}

fn sprt_decision(llr: f64, lower: f64, upper: f64) -> SprtDecision {
    if llr >= upper {
        SprtDecision::AcceptH1
    } else if llr <= lower {
        SprtDecision::AcceptH0
    } else {
        SprtDecision::Continue
    }
}

fn observed_variance(counts: [u32; 5]) -> f64 {
    let pairs = counts.iter().copied().sum::<u32>();
    if pairs == 0 {
        return 0.0;
    }

    let pairs_f = f64::from(pairs);
    let mean = counts
        .iter()
        .zip(PENTANOMIAL_SCORES)
        .map(|(&count, value)| f64::from(count) * value)
        .sum::<f64>()
        / pairs_f;
    counts
        .iter()
        .zip(PENTANOMIAL_SCORES)
        .map(|(&count, value)| f64::from(count) * (value - mean).powi(2))
        .sum::<f64>()
        / pairs_f
}

fn pentanomial_llr(counts: [u32; 5], model: EloModel, elo0: f64, elo1: f64) -> Option<f64> {
    let actual_pairs = counts.iter().copied().sum::<u32>();
    if actual_pairs == 0 {
        return Some(0.0);
    }

    // Empty observed bins cannot support the constrained multinomial MLE.
    // The small prior matches the maintained Fishtest definition and affects
    // the likelihood solve only; public pair counts remain the real sample.
    let regularized = counts.map(|count| {
        if count == 0 {
            EMPTY_BIN_PRIOR
        } else {
            f64::from(count)
        }
    });
    let effective_pairs = regularized.iter().sum::<f64>();
    let empirical = regularized.map(|count| count / effective_pairs);

    let (hypothesis0, hypothesis1, statistic) = match model {
        EloModel::Logistic => (
            elo_to_score(elo0),
            elo_to_score(elo1),
            LikelihoodStatistic::Mean,
        ),
        EloModel::Normalized => (
            elo0 * std::f64::consts::SQRT_2 / NELO_PER_T_VALUE,
            elo1 * std::f64::consts::SQRT_2 / NELO_PER_T_VALUE,
            LikelihoodStatistic::TValue,
        ),
    };

    let mle0 = constrained_mle(&PENTANOMIAL_SCORES, &empirical, hypothesis0, statistic)?;
    let mle1 = constrained_mle(&PENTANOMIAL_SCORES, &empirical, hypothesis1, statistic)?;
    let llr = empirical
        .iter()
        .enumerate()
        .map(|(index, &probability)| probability * (mle1[index] / mle0[index]).ln())
        .sum::<f64>()
        * effective_pairs;
    llr.is_finite().then_some(llr)
}

#[derive(Debug, Clone, Copy)]
enum LikelihoodStatistic {
    Mean,
    TValue,
}

fn constrained_mle<const N: usize>(
    values: &[f64; N],
    empirical: &[f64; N],
    hypothesis: f64,
    statistic: LikelihoodStatistic,
) -> Option<[f64; N]> {
    if !hypothesis.is_finite() {
        return None;
    }
    match statistic {
        LikelihoodStatistic::Mean => constrained_mean_mle(values, empirical, hypothesis),
        LikelihoodStatistic::TValue => constrained_t_value_mle(values, empirical, 0.5, hypothesis),
    }
}

fn constrained_mean_mle<const N: usize>(
    values: &[f64; N],
    empirical: &[f64; N],
    target_mean: f64,
) -> Option<[f64; N]> {
    let shifted = std::array::from_fn(|index| values[index] - target_mean);
    let root = secular_root(&shifted, empirical)?;
    let distribution =
        std::array::from_fn(|index| empirical[index] / (1.0 + root * shifted[index]));
    valid_distribution(&distribution).then_some(distribution)
}

fn constrained_t_value_mle<const N: usize>(
    values: &[f64; N],
    empirical: &[f64; N],
    reference: f64,
    target_t: f64,
) -> Option<[f64; N]> {
    if N < 2 {
        return None;
    }

    let mut distribution = [1.0 / N as f64; N];
    for _ in 0..10 {
        let previous = distribution;
        let (mean, variance) = distribution_stats(values, &previous);
        if !variance.is_finite() || variance <= 0.0 {
            return None;
        }
        let sigma = variance.sqrt();
        let shifted = std::array::from_fn(|index| {
            values[index]
                - reference
                - target_t * sigma * (1.0 + ((mean - values[index]) / sigma).powi(2)) / 2.0
        });
        let root = secular_root(&shifted, empirical)?;
        distribution =
            std::array::from_fn(|index| empirical[index] / (1.0 + root * shifted[index]));
        if !valid_distribution(&distribution) {
            return None;
        }
        let converged = previous
            .iter()
            .zip(distribution)
            .all(|(&before, after)| (before - after).abs() < 1e-9);
        if converged {
            break;
        }
    }

    let (mean, variance) = distribution_stats(values, &distribution);
    let achieved_t = (mean - reference) / variance.sqrt();
    (achieved_t.is_finite() && (achieved_t - target_t).abs() < 1e-5).then_some(distribution)
}

fn secular_root<const N: usize>(values: &[f64; N], probabilities: &[f64; N]) -> Option<f64> {
    const ENDPOINT_EPSILON: f64 = 1e-9;

    let minimum = values.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    if !minimum.is_finite() || !maximum.is_finite() || minimum >= 0.0 || maximum <= 0.0 {
        return None;
    }

    let evaluate = |root: f64| {
        values
            .iter()
            .zip(probabilities)
            .map(|(&value, &probability)| probability * value / (1.0 + root * value))
            .sum::<f64>()
    };
    let mut lower = -1.0 / maximum + ENDPOINT_EPSILON;
    let mut upper = -1.0 / minimum - ENDPOINT_EPSILON;
    let lower_value = evaluate(lower);
    let upper_value = evaluate(upper);
    if !lower_value.is_finite()
        || !upper_value.is_finite()
        || lower_value < 0.0
        || upper_value > 0.0
    {
        return None;
    }

    // The secular function is strictly decreasing inside this bracket.
    // Bisection avoids an optimization dependency and 128 iterations exceed
    // f64 precision for every practical hypothesis range.
    for _ in 0..128 {
        let midpoint = (lower + upper) / 2.0;
        let value = evaluate(midpoint);
        if !value.is_finite() {
            return None;
        }
        if value > 0.0 {
            lower = midpoint;
        } else {
            upper = midpoint;
        }
    }
    Some((lower + upper) / 2.0)
}

fn distribution_stats<const N: usize>(values: &[f64; N], probabilities: &[f64; N]) -> (f64, f64) {
    let mean = values
        .iter()
        .zip(probabilities)
        .map(|(&value, &probability)| value * probability)
        .sum::<f64>();
    let variance = values
        .iter()
        .zip(probabilities)
        .map(|(&value, &probability)| probability * (value - mean).powi(2))
        .sum::<f64>();
    (mean, variance)
}

fn valid_distribution<const N: usize>(probabilities: &[f64; N]) -> bool {
    probabilities
        .iter()
        .all(|probability| probability.is_finite() && *probability > 0.0)
        && (probabilities.iter().sum::<f64>() - 1.0).abs() < 1e-6
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

    let decision = sprt_decision(llr, lower, upper);

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

    fn pentanomial_sample(counts: [u32; 5]) -> PentanomialVector {
        use PairGameResult::{Draw, Loss, Win};

        let representatives = [
            (Loss, Loss),
            (Loss, Draw),
            (Draw, Draw),
            (Draw, Win),
            (Win, Win),
        ];
        let mut sample = PentanomialVector::default();
        for ((first, second), count) in representatives.into_iter().zip(counts) {
            for _ in 0..count {
                sample.record_pair(first, second);
            }
        }
        assert_eq!(sample.counts(), counts);
        sample
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
    fn assumed_pentanomial_distribution_is_explicit_and_non_degenerate() {
        let distribution = PentanomialDistribution::new([0.1, 0.2, 0.4, 0.2, 0.1]).unwrap();
        for (actual, expected) in distribution
            .probabilities()
            .into_iter()
            .zip([0.1, 0.2, 0.4, 0.2, 0.1])
        {
            assert!(approx(actual, expected, 1e-12));
        }
        assert!(approx(distribution.mean(), 0.5, 1e-12));
        assert!(approx(distribution.variance(), 0.075, 1e-12));

        assert!(PentanomialDistribution::new([0.0; 5]).is_none());
        assert!(PentanomialDistribution::new([0.0, 0.0, 1.0, 0.0, 0.0]).is_none());
        assert!(PentanomialDistribution::new([0.1, 0.2, 0.3, 0.2, 0.1]).is_none());
        assert!(PentanomialDistribution::new([f64::NAN, 0.0, 0.0, 0.0, 1.0]).is_none());
    }

    #[test]
    fn normal_quantiles_match_standard_reference_values() {
        assert!(approx(normal_quantile(0.8).unwrap(), 0.841_621_23, 2e-6));
        assert!(approx(normal_quantile(0.95).unwrap(), 1.644_853_63, 2e-6));
        assert!(approx(normal_quantile(0.975).unwrap(), 1.959_963_98, 2e-6));
        assert!(normal_quantile(0.0).is_none());
        assert!(normal_quantile(1.0).is_none());
    }

    #[test]
    fn fixed_n_plans_match_hand_derived_normal_approximation() {
        let distribution = PentanomialDistribution::new([0.1, 0.2, 0.4, 0.2, 0.1]).unwrap();
        let normalized = fixed_n_plan(
            distribution,
            EloModel::Normalized,
            FixedNTestTails::TwoSided,
            5.0,
            0.05,
            0.8,
        )
        .unwrap();
        let logistic = fixed_n_plan(
            distribution,
            EloModel::Logistic,
            FixedNTestTails::TwoSided,
            5.0,
            0.05,
            0.8,
        )
        .unwrap();

        assert_eq!(normalized.model, EloModel::Normalized);
        assert_eq!(normalized.tails, FixedNTestTails::TwoSided);
        assert_eq!(normalized.tails.as_str(), "two-sided");
        assert!(approx(normalized.target_effect, 5.0, f64::EPSILON));
        assert!(approx(normalized.significance, 0.05, f64::EPSILON));
        assert!(approx(normalized.power, 0.8, f64::EPSILON));
        assert_eq!(normalized.assumed_distribution, distribution);
        assert!(approx(normalized.critical_value, 1.959_963_98, 2e-6));
        assert!(approx(normalized.power_quantile, 0.841_621_23, 2e-6));
        assert_eq!(normalized.required_pairs, 18_949);
        assert_eq!(normalized.required_games(), 37_898);

        assert_eq!(logistic.model, EloModel::Logistic);
        assert_eq!(logistic.required_pairs, 11_371);
        assert_ne!(logistic.score_effect, normalized.score_effect);

        // A wider assumed pair distribution increases logistic-Elo sample
        // size. Normalized Elo absorbs that variance into its scale.
        let wider = PentanomialDistribution::new([0.2, 0.1, 0.4, 0.1, 0.2]).unwrap();
        let wider_logistic = fixed_n_plan(
            wider,
            EloModel::Logistic,
            FixedNTestTails::TwoSided,
            5.0,
            0.05,
            0.8,
        )
        .unwrap();
        let wider_normalized = fixed_n_plan(
            wider,
            EloModel::Normalized,
            FixedNTestTails::TwoSided,
            5.0,
            0.05,
            0.8,
        )
        .unwrap();
        assert_eq!(wider_logistic.required_pairs, 17_057);
        assert_eq!(wider_normalized.required_pairs, normalized.required_pairs);
    }

    #[test]
    fn one_sided_fixed_n_plan_matches_one_standard_deviation_fixture() {
        let distribution = PentanomialDistribution::new([0.1, 0.2, 0.4, 0.2, 0.1]).unwrap();
        // A normalized-Elo effect of NELO_PER_T_VALUE/sqrt(2) is exactly one
        // assumed pair standard deviation. N=(z_.95+z_.90)^2=8.564 -> 9.
        let one_sigma_nelo = NELO_PER_T_VALUE / std::f64::consts::SQRT_2;
        let plan = fixed_n_plan(
            distribution,
            EloModel::Normalized,
            FixedNTestTails::OneSided,
            one_sigma_nelo,
            0.05,
            0.9,
        )
        .unwrap();
        assert_eq!(plan.tails.as_str(), "one-sided");
        assert!(approx(
            plan.score_effect,
            distribution.variance().sqrt(),
            1e-12
        ));
        assert_eq!(plan.required_pairs, 9);
    }

    #[test]
    fn fixed_n_plan_rejects_missing_or_impossible_assumptions() {
        let distribution = PentanomialDistribution::new([0.1, 0.2, 0.4, 0.2, 0.1]).unwrap();
        for plan in [
            fixed_n_plan(
                distribution,
                EloModel::Normalized,
                FixedNTestTails::TwoSided,
                0.0,
                0.05,
                0.8,
            ),
            fixed_n_plan(
                distribution,
                EloModel::Normalized,
                FixedNTestTails::TwoSided,
                5.0,
                0.0,
                0.8,
            ),
            fixed_n_plan(
                distribution,
                EloModel::Normalized,
                FixedNTestTails::TwoSided,
                5.0,
                0.05,
                0.5,
            ),
        ] {
            assert!(plan.is_none());
        }
    }

    #[test]
    fn achieved_resolution_reports_interval_without_a_verdict() {
        let mut sample = pentanomial_sample([100, 200, 400, 300, 150]);
        sample.record_unpaired_game();
        let normalized = fixed_n_achieved_resolution(&sample, EloModel::Normalized, 0.05).unwrap();
        let logistic = fixed_n_achieved_resolution(&sample, EloModel::Logistic, 0.05).unwrap();

        assert_eq!(normalized.model, EloModel::Normalized);
        assert_eq!(normalized.pairs, 1_150);
        assert_eq!(normalized.unpaired_games, 1);
        assert!(approx(normalized.significance, 0.05, f64::EPSILON));
        assert!(approx(normalized.confidence, 0.95, f64::EPSILON));
        assert!(approx(normalized.estimate, 37.852_044_633_505_43, 1e-9));
        let expected_normalized_resolution =
            normal_quantile(0.975).unwrap() * NELO_PER_T_VALUE / (2.0 * 1_150.0f64).sqrt();
        assert!(approx(
            normalized.resolution(),
            expected_normalized_resolution,
            1e-9
        ));
        assert!(approx(
            normalized.lower_error(),
            normalized.upper_error(),
            1e-9
        ));

        assert_eq!(logistic.model, EloModel::Logistic);
        assert!(approx(logistic.estimate, 30.288_285_575_247_304, 1e-9));
        assert!(logistic.upper_error() > logistic.lower_error());
        assert!(approx(logistic.resolution(), 11.456_245_846_843_164, 1e-4));

        assert!(fixed_n_achieved_resolution(&sample, EloModel::Normalized, 0.0).is_none());
        assert!(
            fixed_n_achieved_resolution(&PentanomialVector::default(), EloModel::Normalized, 0.05)
                .is_none()
        );
    }

    #[test]
    fn constrained_mles_match_binary_closed_forms() {
        let values = [0.0, 1.0];
        let empirical = [0.7, 0.3];

        // On binary support, fixing the mean to p uniquely fixes [1-p, p].
        let mean_mle = constrained_mean_mle(&values, &empirical, 0.6).unwrap();
        assert!(approx(mean_mle[0], 0.4, 1e-12));
        assert!(approx(mean_mle[1], 0.6, 1e-12));

        // For Bernoulli p, t=(p-1/2)/sqrt(p(1-p)), hence
        // p=1/2+t/(2*sqrt(1+t^2)).
        let target_t: f64 = 0.5;
        let expected_p = 0.5 + target_t / (2.0 * (1.0f64 + target_t.powi(2)).sqrt());
        let t_mle = constrained_t_value_mle(&values, &empirical, 0.5, target_t).unwrap();
        assert!(approx(t_mle[0], 1.0 - expected_p, 1e-9));
        assert!(approx(t_mle[1], expected_p, 1e-9));
    }

    #[test]
    fn pentanomial_sprt_names_and_separates_both_models() {
        // Maintained Fishtest model spot values for this all-positive empirical
        // distribution. Phase 1.7 owns the versioned external oracle corpus.
        let sample = pentanomial_sample([100, 200, 400, 300, 150]);
        let logistic = pentanomial_sprt(&sample, EloModel::Logistic, 0.0, 5.0, 0.05, 0.05).unwrap();
        let normalized =
            pentanomial_sprt(&sample, EloModel::Normalized, 0.0, 5.0, 0.05, 0.05).unwrap();

        assert_eq!(logistic.model, EloModel::Logistic);
        assert_eq!(logistic.model.as_str(), "logistic");
        assert_eq!(normalized.model, EloModel::Normalized);
        assert_eq!(normalized.model.as_str(), "normalized");
        assert_eq!(logistic.pairs, 1_150);
        assert_eq!(normalized.pairs, 1_150);
        assert!(approx(logistic.elo0, 0.0, f64::EPSILON));
        assert!(approx(logistic.elo1, 5.0, f64::EPSILON));
        assert!(approx(logistic.alpha, 0.05, f64::EPSILON));
        assert!(approx(logistic.beta, 0.05, f64::EPSILON));
        assert!(approx(logistic.lower, (0.05f64 / 0.95).ln(), 1e-12));
        assert!(approx(logistic.upper, (0.95f64 / 0.05).ln(), 1e-12));
        assert!(approx(logistic.llr, 4.023_649_372_725_66, 1e-9));
        assert!(approx(normalized.llr, 3.311_547_209_757_04, 1e-9));
        assert_eq!(logistic.decision, SprtDecision::AcceptH1);
        assert_eq!(normalized.decision, SprtDecision::AcceptH1);
    }

    #[test]
    fn symmetric_pentanomial_sample_has_zero_llr_in_both_models() {
        let sample = pentanomial_sample([1, 2, 3, 2, 1]);
        for model in [EloModel::Logistic, EloModel::Normalized] {
            let result = pentanomial_sprt(&sample, model, -5.0, 5.0, 0.05, 0.05).unwrap();
            assert!(approx(result.llr, 0.0, 1e-12));
            assert_eq!(result.decision, SprtDecision::Continue);
        }
    }

    #[test]
    fn pentanomial_sprt_excludes_unpaired_games() {
        let mut sample = pentanomial_sample([10, 20, 30, 40, 50]);
        let before = pentanomial_sprt(&sample, EloModel::Normalized, 0.0, 5.0, 0.05, 0.05);
        sample.record_unpaired_game();
        sample.record_unpaired_game();
        let after = pentanomial_sprt(&sample, EloModel::Normalized, 0.0, 5.0, 0.05, 0.05);
        let before = before.unwrap();
        let after = after.unwrap();
        assert_eq!(after.pairs, before.pairs);
        assert_eq!(after.unpaired_games, 2);
        assert_eq!(after.llr, before.llr);
        assert_eq!(after.decision, before.decision);
    }

    #[test]
    fn pentanomial_sprt_regularizes_empty_bins_without_inflating_pair_count() {
        let sample = pentanomial_sample([0, 2, 5, 3, 0]);
        let logistic = pentanomial_sprt(&sample, EloModel::Logistic, 0.0, 5.0, 0.05, 0.05).unwrap();
        let normalized =
            pentanomial_sprt(&sample, EloModel::Normalized, 0.0, 5.0, 0.05, 0.05).unwrap();

        assert_eq!(logistic.pairs, 10);
        assert_eq!(normalized.pairs, 10);
        assert!(approx(logistic.llr, 0.049_333_177_176_969, 1e-12));
        assert!(approx(normalized.llr, 0.026_697_182_964_011_1, 1e-12));
    }

    #[test]
    fn pentanomial_sprt_rejects_invalid_or_degenerate_inputs() {
        use PairGameResult::Draw;

        let empty = PentanomialVector::default();
        assert_eq!(
            pentanomial_llr([0; 5], EloModel::Logistic, 0.0, 5.0),
            Some(0.0)
        );
        assert!(pentanomial_sprt(&empty, EloModel::Logistic, 0.0, 5.0, 0.05, 0.05).is_none());

        let mut one_pair = PentanomialVector::default();
        one_pair.record_pair(Draw, Draw);
        assert!(pentanomial_sprt(&one_pair, EloModel::Normalized, 0.0, 5.0, 0.05, 0.05).is_none());

        let all_draws = pentanomial_sample([0, 0, 10, 0, 0]);
        assert!(pentanomial_sprt(&all_draws, EloModel::Logistic, 0.0, 5.0, 0.05, 0.05).is_none());

        let valid = pentanomial_sample([1, 2, 3, 4, 5]);
        for invalid in [
            pentanomial_sprt(&valid, EloModel::Logistic, 5.0, 0.0, 0.05, 0.05),
            pentanomial_sprt(&valid, EloModel::Logistic, 0.0, 5.0, 0.0, 0.05),
            pentanomial_sprt(&valid, EloModel::Logistic, 0.0, 5.0, 0.6, 0.4),
        ] {
            assert!(invalid.is_none());
        }
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
        assert_eq!(vector.central_pair_breakdown(), (2, 1));
        assert!(approx(vector.draw_ratio().unwrap(), 1.0 / 3.0, 1e-12));
        assert!(approx(vector.pairs_ratio().unwrap(), 1.0, 1e-12));
        assert!(approx(
            vector.win_loss_to_double_draw_ratio().unwrap(),
            2.0,
            1e-12
        ));
    }

    #[test]
    fn pentanomial_statistics_match_hand_derived_symmetric_fixture() {
        use PairGameResult::{Draw, Loss, Win};

        // The Cartesian product of W/D/L gives bins [1, 2, 3, 2, 1].
        // Pair-average mean = 1/2 and population variance = 1/12.
        let mut sample = PentanomialVector::default();
        for first in [Loss, Draw, Win] {
            for second in [Loss, Draw, Win] {
                sample.record_pair(first, second);
            }
        }
        sample.record_unpaired_game();

        let stats = pentanomial_statistics(&sample, 1.959_963_984_540_054).unwrap();
        assert_eq!(stats.pairs, 9);
        assert_eq!(stats.unpaired_games, 1);
        assert!(approx(stats.score, 0.5, 1e-12));
        assert!(approx(stats.variance, 1.0 / 12.0, 1e-12));
        assert!(approx(stats.standard_error, (1.0f64 / 108.0).sqrt(), 1e-12));
        assert!(approx(stats.logistic_elo.elo, 0.0, 1e-12));
        assert!(approx(
            stats.logistic_elo.margin(),
            137.857_437_832_269,
            1e-9
        ));
        assert!(approx(stats.normalized_elo.elo, 0.0, 1e-12));
        assert!(approx(
            stats.normalized_elo.margin(),
            160.504_102_230_314,
            1e-9
        ));
        assert!(approx(stats.los, 0.5, 1e-7));
        assert!(approx(stats.draw_ratio, 1.0 / 3.0, 1e-12));
        assert!(approx(stats.pairs_ratio.unwrap(), 1.0, 1e-12));
        assert!(approx(
            stats.win_loss_to_double_draw_ratio.unwrap(),
            2.0,
            1e-12
        ));
    }

    #[test]
    fn pentanomial_statistics_match_fastchess_reference_values() {
        use PairGameResult::{Draw, Loss, Win};

        let mut sample = PentanomialVector::default();
        let categories = [
            (Loss, Loss, 34),
            (Loss, Draw, 54),
            (Loss, Win, 31),
            (Draw, Draw, 32),
            (Win, Draw, 64),
            (Win, Win, 75),
        ];
        for (first, second, count) in categories {
            for _ in 0..count {
                sample.record_pair(first, second);
            }
        }

        let stats = pentanomial_statistics(&sample, 1.959_963_984_540_054).unwrap();
        assert!(approx(stats.score, 0.579, 0.001));
        assert!(approx(stats.logistic_elo.elo, 55.58, 0.01));
        assert!(approx(stats.logistic_elo.margin(), 27.65, 0.01));
        assert!(approx(stats.normalized_elo.elo, 57.94, 0.01));
        assert!(approx(stats.normalized_elo.margin(), 28.28, 0.01));
        assert!(stats.los > 0.999_9);
        assert!(approx(stats.draw_ratio, 182.0 / 580.0, 1e-12));
        assert!(approx(stats.pairs_ratio.unwrap(), 139.0 / 88.0, 1e-12));
        assert!(approx(
            stats.win_loss_to_double_draw_ratio.unwrap(),
            31.0 / 32.0,
            1e-12
        ));
    }

    #[test]
    fn ratios_are_absent_instead_of_infinite_when_denominators_are_zero() {
        use PairGameResult::{Draw, Win};

        let empty = PentanomialVector::default();
        assert_eq!(empty.draw_ratio(), None);
        assert_eq!(empty.pairs_ratio(), None);
        assert_eq!(empty.win_loss_to_double_draw_ratio(), None);

        let mut winning = PentanomialVector::default();
        winning.record_pair(Win, Win);
        winning.record_pair(Win, Draw);
        assert_eq!(winning.pairs_ratio(), None);
        assert_eq!(winning.win_loss_to_double_draw_ratio(), None);
        assert_eq!(pentanomial_statistics(&winning, 1.96), None);
    }
}
