use colosseum_core::{
    EloModel, FixedNTestTails, NamedRng, PairGameResult, PentanomialDistribution,
    PentanomialVector, SprtDecision, fixed_n_achieved_resolution, fixed_n_plan, pentanomial_sprt,
    rng::stream_names, sprt_wald_bounds,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FixedPlanObjective {
    Difference,
    Equivalence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixedPlanRequest {
    pub objective: FixedPlanObjective,
    pub model: EloModel,
    pub effect_or_margin: f64,
    pub significance: f64,
    pub power: f64,
    pub assumed_distribution: [f64; 5],
    pub observed_pentanomial: Option<[u32; 5]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FixedPlanReport {
    pub objective: FixedPlanObjective,
    pub model: EloModel,
    pub effect_or_margin: f64,
    pub significance: f64,
    pub power: f64,
    pub assumed_distribution: [f64; 5],
    pub assumed_mean: f64,
    pub assumed_variance: f64,
    pub tails_per_test: String,
    pub required_pairs: u64,
    pub required_games: u64,
    pub planned_score_standard_error: f64,
    pub planned_score_half_width: f64,
    pub achieved_resolution: Option<AchievedResolutionReport>,
    pub interpretation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AchievedResolutionReport {
    pub pairs: u32,
    pub estimate: f64,
    pub lower: f64,
    pub upper: f64,
    pub conservative_resolution: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SprtLengthPlanRequest {
    pub model: EloModel,
    pub elo0: f64,
    pub elo1: f64,
    pub alpha: f64,
    pub beta: f64,
    pub assumed_true_distribution: [f64; 5],
    pub simulations: u32,
    pub max_pairs: u32,
    pub seed: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SprtLengthPlanReport {
    pub request: SprtLengthPlanRequest,
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub mean_pairs: f64,
    pub median_pairs: u32,
    pub p05_pairs: u32,
    pub p95_pairs: u32,
    pub minimum_pairs: u32,
    pub maximum_pairs: u32,
    pub accepted_h0: u32,
    pub accepted_h1: u32,
    pub capped: u32,
    pub rng_stream: String,
    pub sampling_algorithm: String,
    pub interpretation: String,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum StatsPlanError {
    #[error(transparent)]
    Statistics(#[from] colosseum_core::StatisticsError),
    #[error("observed pentanomial count total is too large")]
    ObservedCountOverflow,
    #[error("SPRT expected-length simulation requires at least one simulation and two max pairs")]
    InvalidSimulationSize,
}

pub fn plan_fixed(request: FixedPlanRequest) -> Result<FixedPlanReport, StatsPlanError> {
    let distribution = PentanomialDistribution::new(request.assumed_distribution)?;
    let tails = match request.objective {
        FixedPlanObjective::Difference => FixedNTestTails::TwoSided,
        FixedPlanObjective::Equivalence => FixedNTestTails::OneSided,
    };
    let plan = fixed_n_plan(
        distribution,
        request.model,
        tails,
        request.effect_or_margin,
        request.significance,
        request.power,
    )?;
    let standard_error = (distribution.variance() / plan.required_pairs as f64).sqrt();
    let achieved_resolution = request
        .observed_pentanomial
        .map(pentanomial_from_counts)
        .transpose()?
        .map(|sample| fixed_n_achieved_resolution(&sample, request.model, request.significance))
        .transpose()?
        .map(|resolution| AchievedResolutionReport {
            pairs: resolution.pairs,
            estimate: resolution.estimate,
            lower: resolution.lower,
            upper: resolution.upper,
            conservative_resolution: resolution.lower_error().max(resolution.upper_error()),
        });
    Ok(FixedPlanReport {
        objective: request.objective,
        model: request.model,
        effect_or_margin: request.effect_or_margin,
        significance: request.significance,
        power: request.power,
        assumed_distribution: request.assumed_distribution,
        assumed_mean: distribution.mean(),
        assumed_variance: distribution.variance(),
        tails_per_test: tails.as_str().into(),
        required_pairs: plan.required_pairs,
        required_games: plan.required_games(),
        planned_score_standard_error: standard_error,
        planned_score_half_width: plan.critical_value * standard_error,
        achieved_resolution,
        interpretation: match request.objective {
            FixedPlanObjective::Difference => "prospective two-sided normal-approximation design; not a stopping rule or post-hoc claim".into(),
            FixedPlanObjective::Equivalence => "prospective symmetric TOST approximation at assumed true effect zero; both one-sided tests must pass in the actual analysis".into(),
        },
    })
}

pub fn plan_sprt_length(
    request: SprtLengthPlanRequest,
) -> Result<SprtLengthPlanReport, StatsPlanError> {
    if request.simulations == 0 || request.max_pairs < 2 {
        return Err(StatsPlanError::InvalidSimulationSize);
    }
    let distribution = PentanomialDistribution::new(request.assumed_true_distribution)?;
    let (lower_bound, upper_bound) =
        sprt_wald_bounds(request.elo0, request.elo1, request.alpha, request.beta)?;
    let mut rng = NamedRng::new(request.seed, stream_names::SPRT_LENGTH_SIMULATION)
        .expect("stable ASCII stream name");
    let mut lengths = Vec::with_capacity(request.simulations as usize);
    let mut accepted_h0 = 0;
    let mut accepted_h1 = 0;
    let mut capped = 0;
    for _ in 0..request.simulations {
        let mut sample = PentanomialVector::default();
        let mut terminal = None;
        for pair in 1..=request.max_pairs {
            record_bin(&mut sample, sample_distribution(&mut rng, distribution));
            if pair < 2 {
                continue;
            }
            let Ok(result) = pentanomial_sprt(
                &sample,
                request.model,
                request.elo0,
                request.elo1,
                request.alpha,
                request.beta,
            ) else {
                continue;
            };
            match result.decision {
                SprtDecision::AcceptH0 => {
                    accepted_h0 += 1;
                    terminal = Some(pair);
                }
                SprtDecision::AcceptH1 => {
                    accepted_h1 += 1;
                    terminal = Some(pair);
                }
                SprtDecision::Continue => {}
            }
            if terminal.is_some() {
                break;
            }
        }
        lengths.push(terminal.unwrap_or_else(|| {
            capped += 1;
            request.max_pairs
        }));
    }
    lengths.sort_unstable();
    let quantile = |numerator: usize| lengths[(lengths.len() - 1) * numerator / 100];
    Ok(SprtLengthPlanReport {
        request,
        lower_bound,
        upper_bound,
        mean_pairs: lengths.iter().map(|value| f64::from(*value)).sum::<f64>()
            / lengths.len() as f64,
        median_pairs: quantile(50),
        p05_pairs: quantile(5),
        p95_pairs: quantile(95),
        minimum_pairs: lengths[0],
        maximum_pairs: *lengths.last().expect("non-empty simulations"),
        accepted_h0,
        accepted_h1,
        capped,
        rng_stream: stream_names::SPRT_LENGTH_SIMULATION.into(),
        sampling_algorithm: "u64-to-[0,1) cumulative pentanomial inverse-CDF v1".into(),
        interpretation: "seeded expected-length planning distribution under explicit assumptions; not a stopping guarantee or forecast for a different engine/workload".into(),
    })
}

fn pentanomial_from_counts(counts: [u32; 5]) -> Result<PentanomialVector, StatsPlanError> {
    counts
        .iter()
        .try_fold(0_u32, |total, count| total.checked_add(*count))
        .ok_or(StatsPlanError::ObservedCountOverflow)?;
    let mut sample = PentanomialVector::default();
    for (index, count) in counts.into_iter().enumerate() {
        for _ in 0..count {
            record_bin(&mut sample, index);
        }
    }
    Ok(sample)
}

fn record_bin(sample: &mut PentanomialVector, index: usize) {
    use PairGameResult::{Draw, Loss, Win};
    let pair = match index {
        0 => (Loss, Loss),
        1 => (Loss, Draw),
        2 => (Draw, Draw),
        3 => (Draw, Win),
        4 => (Win, Win),
        _ => unreachable!("pentanomial bin index"),
    };
    sample.record_pair(pair.0, pair.1);
}

fn sample_distribution(rng: &mut NamedRng, distribution: PentanomialDistribution) -> usize {
    let draw = rng.next_u64() as f64 / 18_446_744_073_709_551_616.0;
    let mut cumulative = 0.0;
    for (index, probability) in distribution.probabilities().into_iter().enumerate() {
        cumulative += probability;
        if draw < cumulative || index == 4 {
            return index;
        }
    }
    unreachable!("last distribution bin catches rounding")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_plan_retains_assumptions_and_optional_achieved_resolution() {
        let report = plan_fixed(FixedPlanRequest {
            objective: FixedPlanObjective::Difference,
            model: EloModel::Normalized,
            effect_or_margin: 5.0,
            significance: 0.05,
            power: 0.8,
            assumed_distribution: [0.05, 0.2, 0.5, 0.2, 0.05],
            observed_pentanomial: Some([5, 20, 50, 20, 5]),
        })
        .unwrap();
        assert!(report.required_pairs >= 2);
        assert_eq!(report.required_games, report.required_pairs * 2);
        assert_eq!(report.achieved_resolution.unwrap().pairs, 100);
    }

    #[test]
    fn sprt_length_simulation_is_seeded_and_capped() {
        let request = SprtLengthPlanRequest {
            model: EloModel::Normalized,
            elo0: 0.0,
            elo1: 5.0,
            alpha: 0.05,
            beta: 0.05,
            assumed_true_distribution: [0.05, 0.2, 0.5, 0.2, 0.05],
            simulations: 20,
            max_pairs: 20,
            seed: 42,
        };
        let first = plan_sprt_length(request.clone()).unwrap();
        let second = plan_sprt_length(request).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.accepted_h0 + first.accepted_h1 + first.capped, 20);
        assert!(first.maximum_pairs <= 20);
    }
}
