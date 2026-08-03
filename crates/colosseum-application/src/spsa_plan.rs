//! Runtime-neutral SPSA schedule and workload planning.

use std::collections::BTreeSet;

use colosseum_core::{SPSA_SCHEDULE_SCHEMA_VERSION, SpsaEndSpec, SpsaScheduleArtifact};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{SpsaRunSettings, SpsaRunSettingsError, SpsaTune, SpsaTuneAuditError};

pub const SPSA_PLAN_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub enum SpsaTimingInput {
    Range {
        lower_seconds_per_game: f64,
        upper_seconds_per_game: f64,
    },
    PilotGames(Vec<f64>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "kebab-case")]
pub enum SpsaTimingBasis {
    UserRange {
        lower_seconds_per_game: f64,
        upper_seconds_per_game: f64,
    },
    PilotGames {
        samples: u32,
        lower_seconds_per_game: f64,
        median_seconds_per_game: f64,
        upper_seconds_per_game: f64,
    },
}

impl SpsaTimingBasis {
    fn from_input(input: SpsaTimingInput) -> Result<Self, SpsaPlanError> {
        match input {
            SpsaTimingInput::Range {
                lower_seconds_per_game,
                upper_seconds_per_game,
            } => {
                validate_seconds(lower_seconds_per_game)?;
                validate_seconds(upper_seconds_per_game)?;
                if lower_seconds_per_game > upper_seconds_per_game {
                    return Err(SpsaPlanError::DescendingTimingRange {
                        lower: lower_seconds_per_game,
                        upper: upper_seconds_per_game,
                    });
                }
                Ok(Self::UserRange {
                    lower_seconds_per_game,
                    upper_seconds_per_game,
                })
            }
            SpsaTimingInput::PilotGames(mut samples) => {
                if samples.is_empty() {
                    return Err(SpsaPlanError::EmptyPilot);
                }
                for value in &samples {
                    validate_seconds(*value)?;
                }
                samples.sort_by(f64::total_cmp);
                let lower_seconds_per_game = samples[0];
                let upper_seconds_per_game = samples[samples.len() - 1];
                let median_seconds_per_game = if samples.len().is_multiple_of(2) {
                    let high = samples.len() / 2;
                    (samples[high - 1] + samples[high]) / 2.0
                } else {
                    samples[samples.len() / 2]
                };
                Ok(Self::PilotGames {
                    samples: u32::try_from(samples.len())
                        .map_err(|_| SpsaPlanError::CountOverflow)?,
                    lower_seconds_per_game,
                    median_seconds_per_game,
                    upper_seconds_per_game,
                })
            }
        }
    }

    fn bounds(&self) -> (f64, f64) {
        match self {
            Self::UserRange {
                lower_seconds_per_game,
                upper_seconds_per_game,
            }
            | Self::PilotGames {
                lower_seconds_per_game,
                upper_seconds_per_game,
                ..
            } => (*lower_seconds_per_game, *upper_seconds_per_game),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaWallTimeEstimate {
    pub concurrency: u32,
    pub waves_per_iteration: u32,
    pub total_game_waves: u64,
    pub lower_seconds: f64,
    pub upper_seconds: f64,
    pub basis: SpsaTimingBasis,
    pub assumption: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpsaPlanPoint {
    /// Zero-based schedule iteration.
    pub iteration: u32,
    pub c: f64,
    pub a: f64,
    pub r: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaKnobPlan {
    pub name: String,
    pub initial: i64,
    pub min: i64,
    pub max: i64,
    pub c_end: f64,
    pub c0: f64,
    pub a0: f64,
    /// First zero-based iteration whose perturbation is below half a UCI unit.
    pub first_rounding_resolution_hazard: Option<u32>,
    pub trajectory: Vec<SpsaPlanPoint>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaHorizonComparison {
    pub iterations: u32,
    pub games: u64,
    pub pairs: u64,
    pub checkpoint_publications: u32,
    pub wall_time: Option<SpsaWallTimeEstimate>,
    pub knobs: Vec<SpsaHorizonKnob>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaHorizonKnob {
    pub name: String,
    pub first: SpsaPlanPoint,
    pub final_point: SpsaPlanPoint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaPlanReport {
    pub schema_version: u32,
    pub schedule_schema_version: u32,
    pub stats_version: u32,
    pub settings: SpsaRunSettings,
    pub r_end: f64,
    pub total_games: u64,
    pub total_pairs: u64,
    /// One publication follows each complete mini-match.
    pub checkpoint_publications: u32,
    /// The durable store retains current and previous checkpoint generations.
    pub checkpoint_generations_retained: u32,
    pub schedule_artifacts: u32,
    pub wall_time: Option<SpsaWallTimeEstimate>,
    pub knobs: Vec<SpsaKnobPlan>,
    pub horizon_comparisons: Vec<SpsaHorizonComparison>,
    pub interpretation: String,
}

pub fn plan_spsa(
    tune: &SpsaTune,
    settings: SpsaRunSettings,
    r_end: f64,
    concurrency: u32,
    timing: Option<SpsaTimingInput>,
    comparison_horizons: &[u32],
) -> Result<SpsaPlanReport, SpsaPlanError> {
    settings.validate()?;
    tune.audit_configuration()?;
    if concurrency == 0 {
        return Err(SpsaPlanError::ZeroConcurrency);
    }
    let timing = timing.map(SpsaTimingBasis::from_input).transpose()?;
    let primary = build_schedule(tune, settings, r_end, concurrency, timing.clone())?;
    let mut seen = BTreeSet::new();
    let mut horizon_comparisons = Vec::new();
    for &iterations in comparison_horizons {
        if iterations == 0 {
            return Err(SpsaPlanError::ZeroComparisonHorizon);
        }
        if iterations == settings.iterations || !seen.insert(iterations) {
            continue;
        }
        let comparison_settings = SpsaRunSettings::new(iterations, settings.games_per_iteration)?;
        let comparison = build_schedule(
            tune,
            comparison_settings,
            r_end,
            concurrency,
            timing.clone(),
        )?;
        horizon_comparisons.push(SpsaHorizonComparison {
            iterations,
            games: comparison.total_games,
            pairs: comparison.total_pairs,
            checkpoint_publications: iterations,
            wall_time: comparison.wall_time,
            knobs: comparison
                .knobs
                .into_iter()
                .map(|knob| SpsaHorizonKnob {
                    name: knob.name,
                    first: knob.trajectory[0],
                    final_point: knob.trajectory[knob.trajectory.len() - 1],
                })
                .collect(),
        });
    }
    horizon_comparisons.sort_by_key(|comparison| comparison.iterations);

    Ok(SpsaPlanReport {
        schema_version: SPSA_PLAN_SCHEMA_VERSION,
        schedule_schema_version: SPSA_SCHEDULE_SCHEMA_VERSION,
        stats_version: primary.stats_version,
        settings,
        r_end,
        total_games: primary.total_games,
        total_pairs: primary.total_pairs,
        checkpoint_publications: settings.iterations,
        checkpoint_generations_retained: 2,
        schedule_artifacts: 1,
        wall_time: primary.wall_time,
        knobs: primary.knobs,
        horizon_comparisons,
        interpretation: "factual schedule and workload arithmetic; timing assumes supplied end-to-end game durations and is not a chess-convergence forecast".into(),
    })
}

struct BuiltSchedule {
    stats_version: u32,
    total_games: u64,
    total_pairs: u64,
    wall_time: Option<SpsaWallTimeEstimate>,
    knobs: Vec<SpsaKnobPlan>,
}

fn build_schedule(
    tune: &SpsaTune,
    settings: SpsaRunSettings,
    r_end: f64,
    concurrency: u32,
    timing: Option<SpsaTimingBasis>,
) -> Result<BuiltSchedule, SpsaPlanError> {
    let end_specs = tune
        .parameters
        .iter()
        .map(|parameter| SpsaEndSpec {
            name: parameter.name.clone(),
            min: parameter.min,
            max: parameter.max,
            c_end: parameter.c_end,
        })
        .collect::<Vec<_>>();
    // Gain trajectories do not depend on perturbation draws. A fixed planning
    // seed keeps the derived artifact deterministic without pretending to pick
    // the execution seed.
    let artifact = SpsaScheduleArtifact::derive(settings.iterations, r_end, 0, &end_specs)?;
    artifact.validate()?;
    let total_games = u64::from(settings.iterations)
        .checked_mul(u64::from(settings.games_per_iteration))
        .ok_or(SpsaPlanError::CountOverflow)?;
    let total_pairs = u64::from(settings.iterations)
        .checked_mul(u64::from(settings.pairs_per_iteration()))
        .ok_or(SpsaPlanError::CountOverflow)?;
    let wall_time = timing
        .map(|basis| estimate_wall_time(settings, concurrency, basis))
        .transpose()?;
    let knobs = artifact
        .knobs
        .iter()
        .zip(&tune.parameters)
        .map(|(derived, parameter)| {
            let knob = derived.knob()?;
            let trajectory = (0..settings.iterations)
                .map(|iteration| {
                    let coefficients = artifact.schedule.coefficients(iteration, knob)?;
                    Ok(SpsaPlanPoint {
                        iteration,
                        c: coefficients.c,
                        a: coefficients.a,
                        r: coefficients.r,
                    })
                })
                .collect::<Result<Vec<_>, colosseum_core::SpsaError>>()?;
            let first_rounding_resolution_hazard = trajectory
                .iter()
                .find(|point| point.c < 0.5)
                .map(|point| point.iteration);
            Ok(SpsaKnobPlan {
                name: parameter.name.clone(),
                initial: parameter.initial,
                min: parameter.min,
                max: parameter.max,
                c_end: parameter.c_end,
                c0: derived.c0,
                a0: derived.a0,
                first_rounding_resolution_hazard,
                trajectory,
            })
        })
        .collect::<Result<Vec<_>, SpsaPlanError>>()?;
    Ok(BuiltSchedule {
        stats_version: artifact.stats_version,
        total_games,
        total_pairs,
        wall_time,
        knobs,
    })
}

fn estimate_wall_time(
    settings: SpsaRunSettings,
    concurrency: u32,
    basis: SpsaTimingBasis,
) -> Result<SpsaWallTimeEstimate, SpsaPlanError> {
    let waves_per_iteration = settings.games_per_iteration.div_ceil(concurrency);
    let total_game_waves = u64::from(settings.iterations)
        .checked_mul(u64::from(waves_per_iteration))
        .ok_or(SpsaPlanError::CountOverflow)?;
    let (lower, upper) = basis.bounds();
    let lower_seconds = total_game_waves as f64 * lower;
    let upper_seconds = total_game_waves as f64 * upper;
    if !lower_seconds.is_finite() || !upper_seconds.is_finite() {
        return Err(SpsaPlanError::TimingOverflow);
    }
    Ok(SpsaWallTimeEstimate {
        concurrency,
        waves_per_iteration,
        total_game_waves,
        lower_seconds,
        upper_seconds,
        basis,
        assumption: "iterations are sequential; games within one mini-match occupy concurrent waves; supplied durations include the complete game workload".into(),
    })
}

fn validate_seconds(value: f64) -> Result<(), SpsaPlanError> {
    if value.is_finite() && value > 0.0 {
        Ok(())
    } else {
        Err(SpsaPlanError::InvalidGameSeconds { value })
    }
}

#[derive(Debug, Error)]
pub enum SpsaPlanError {
    #[error(transparent)]
    Settings(#[from] SpsaRunSettingsError),
    #[error(transparent)]
    Tune(#[from] SpsaTuneAuditError),
    #[error(transparent)]
    Schedule(#[from] colosseum_core::SpsaError),
    #[error("SPSA planning concurrency must be greater than zero")]
    ZeroConcurrency,
    #[error("SPSA comparison horizon must be greater than zero")]
    ZeroComparisonHorizon,
    #[error("game duration must be finite and positive; got {value}")]
    InvalidGameSeconds { value: f64 },
    #[error("game-duration range is descending: {lower}..{upper}")]
    DescendingTimingRange { lower: f64, upper: f64 },
    #[error("pilot timing requires at least one complete game duration")]
    EmptyPilot,
    #[error("SPSA workload count overflow")]
    CountOverflow,
    #[error("SPSA wall-time estimate overflow")]
    TimingOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SpsaTuneParameter;

    fn tune() -> SpsaTune {
        SpsaTune {
            parameters: vec![SpsaTuneParameter {
                name: "Hash".into(),
                initial: 16,
                min: 1,
                max: 1024,
                c_end: 1.0,
            }],
        }
    }

    #[test]
    fn schedule_and_cost_match_hand_computed_workload() {
        let report = plan_spsa(
            &tune(),
            SpsaRunSettings::new(4, 6).unwrap(),
            0.002,
            4,
            Some(SpsaTimingInput::Range {
                lower_seconds_per_game: 2.0,
                upper_seconds_per_game: 3.0,
            }),
            &[2, 8, 4, 2],
        )
        .unwrap();

        assert_eq!(report.total_games, 24);
        assert_eq!(report.total_pairs, 12);
        assert_eq!(report.checkpoint_publications, 4);
        assert_eq!(report.checkpoint_generations_retained, 2);
        let timing = report.wall_time.unwrap();
        assert_eq!(timing.waves_per_iteration, 2);
        assert_eq!(timing.total_game_waves, 8);
        assert_eq!((timing.lower_seconds, timing.upper_seconds), (16.0, 24.0));
        assert_eq!(report.horizon_comparisons.len(), 2);
        assert_eq!(report.horizon_comparisons[0].iterations, 2);
        assert_eq!(report.horizon_comparisons[1].iterations, 8);
        let knob = &report.knobs[0];
        assert_eq!(knob.trajectory.len(), 4);
        assert_eq!(knob.trajectory[3].c, 1.0);
        assert!((knob.trajectory[3].r - 0.002).abs() < 1e-15);
        assert_eq!(knob.first_rounding_resolution_hazard, None);
    }

    #[test]
    fn pilot_range_covers_every_controlled_game_sample() {
        let samples = vec![1.25, 2.0, 1.5, 1.75];
        let report = plan_spsa(
            &tune(),
            SpsaRunSettings::new(3, 2).unwrap(),
            0.002,
            2,
            Some(SpsaTimingInput::PilotGames(samples.clone())),
            &[],
        )
        .unwrap();
        let timing = report.wall_time.unwrap();
        let SpsaTimingBasis::PilotGames {
            samples: count,
            lower_seconds_per_game,
            median_seconds_per_game,
            upper_seconds_per_game,
        } = timing.basis
        else {
            panic!("expected pilot timing")
        };
        assert_eq!(count, 4);
        assert_eq!(lower_seconds_per_game, 1.25);
        assert_eq!(median_seconds_per_game, 1.625);
        assert_eq!(upper_seconds_per_game, 2.0);
        assert!(
            samples.iter().all(
                |sample| *sample >= lower_seconds_per_game && *sample <= upper_seconds_per_game
            )
        );
        assert_eq!((timing.lower_seconds, timing.upper_seconds), (3.75, 6.0));
    }

    #[test]
    fn invalid_timing_and_tune_are_refused() {
        assert!(matches!(
            plan_spsa(
                &tune(),
                SpsaRunSettings::new(1, 2).unwrap(),
                0.002,
                1,
                Some(SpsaTimingInput::Range {
                    lower_seconds_per_game: 2.0,
                    upper_seconds_per_game: 1.0,
                }),
                &[],
            ),
            Err(SpsaPlanError::DescendingTimingRange { .. })
        ));
        let mut invalid = tune();
        invalid.parameters[0].c_end = 0.49;
        assert!(matches!(
            plan_spsa(
                &invalid,
                SpsaRunSettings::new(1, 2).unwrap(),
                0.002,
                1,
                None,
                &[],
            ),
            Err(SpsaPlanError::Tune(_))
        ));
    }
}
