//! Runtime-neutral, explicitly heuristic SPSA trajectory diagnostics.

use colosseum_core::SpsaScheduleArtifact;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{SpsaBoundTune, SpsaCenterSample, SpsaRunSettings, SpsaRunSettingsError};

pub const SPSA_STATUS_SCHEMA_VERSION: u32 = 1;
pub const SPSA_DIAGNOSTIC_MIN_HISTORY: usize = 6;
pub const SPSA_FREQUENT_BOUND_CONTACT_FRACTION: f64 = 0.20;
pub const SPSA_LITTLE_MOVEMENT_RANGE_FRACTION: f64 = 0.01;
pub const SPSA_RECENT_STABILITY_RANGE_FRACTION: f64 = 0.01;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpsaHeuristicState {
    Observed,
    NotObserved,
    InsufficientHistory,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaHeuristic {
    pub state: SpsaHeuristicState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metric: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<f64>,
    pub observation: String,
    pub caveat: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpsaTrajectoryPoint {
    /// Zero-based iteration whose completed update produced this centre.
    pub iteration: u32,
    pub value: f64,
    pub normalized_to_range: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpsaThirdSummary {
    pub first_iteration: u32,
    pub last_iteration: u32,
    pub samples: u32,
    pub mean: f64,
    pub mean_normalized_to_range: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaThirdsComparison {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thirds: Option<[SpsaThirdSummary; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaKnobDiagnostics {
    pub name: String,
    pub initial: i64,
    pub min: i64,
    pub max: i64,
    pub current: f64,
    pub current_normalized_to_range: f64,
    pub current_perturbation: f64,
    pub trajectory: Vec<SpsaTrajectoryPoint>,
    pub thirds: SpsaThirdsComparison,
    pub frequent_bound_contact: SpsaHeuristic,
    pub little_net_movement: SpsaHeuristic,
    pub recent_stability: SpsaHeuristic,
    pub dead_perturbation: SpsaHeuristic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaEta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_seconds: Option<f64>,
    pub completed_iterations_basis: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
    pub interpretation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaStatusReport {
    pub schema_version: u32,
    pub settings: SpsaRunSettings,
    pub completed_iterations: u32,
    pub percent_complete: f64,
    pub invalid: bool,
    pub eta: SpsaEta,
    pub knobs: Vec<SpsaKnobDiagnostics>,
    pub interpretation: String,
}

pub fn diagnose_spsa(
    tune: &SpsaBoundTune,
    schedule: &SpsaScheduleArtifact,
    settings: SpsaRunSettings,
    history: &[SpsaCenterSample],
    invalid: bool,
    elapsed_to_last_checkpoint_seconds: Option<f64>,
) -> Result<SpsaStatusReport, SpsaStatusError> {
    settings.validate()?;
    schedule.validate()?;
    if schedule.schedule.iterations() != settings.iterations {
        return Err(SpsaStatusError::ScheduleHorizonMismatch {
            schedule: schedule.schedule.iterations(),
            settings: settings.iterations,
        });
    }
    if tune.parameters.is_empty() || tune.parameters.len() != schedule.knobs.len() {
        return Err(SpsaStatusError::DimensionMismatch);
    }
    if history.len() > settings.iterations as usize {
        return Err(SpsaStatusError::HistoryBeyondHorizon);
    }
    for (index, sample) in history.iter().enumerate() {
        if sample.iteration != index as u32 || sample.centers.len() != tune.parameters.len() {
            return Err(SpsaStatusError::HistoryMismatch { index });
        }
    }
    let completed_iterations = u32::try_from(history.len()).expect("history is bounded by u32");
    let percent_complete = f64::from(completed_iterations) * 100.0 / f64::from(settings.iterations);
    let eta = eta(
        settings,
        completed_iterations,
        invalid,
        elapsed_to_last_checkpoint_seconds,
    )?;
    let coefficient_iteration = completed_iterations.min(settings.iterations - 1);
    let knobs = tune
        .parameters
        .iter()
        .enumerate()
        .map(|(knob_index, bound)| {
            let parameter = &bound.parameter;
            let range = (parameter.max - parameter.min) as f64;
            let values = history
                .iter()
                .map(|sample| sample.centers[knob_index])
                .collect::<Vec<_>>();
            for value in &values {
                if !value.is_finite()
                    || *value < parameter.min as f64
                    || *value > parameter.max as f64
                {
                    return Err(SpsaStatusError::CenterOutsideRange {
                        name: parameter.name.clone(),
                        value: *value,
                    });
                }
            }
            let current = values
                .last()
                .copied()
                .unwrap_or(parameter.initial as f64);
            let trajectory = history
                .iter()
                .zip(&values)
                .map(|(sample, value)| SpsaTrajectoryPoint {
                    iteration: sample.iteration,
                    value: *value,
                    normalized_to_range: normalize(*value, parameter.min, range),
                })
                .collect::<Vec<_>>();
            let current_perturbation = schedule.schedule.coefficients(
                coefficient_iteration,
                schedule.knobs[knob_index].knob()?,
            )?.c;
            let enough = values.len() >= SPSA_DIAGNOSTIC_MIN_HISTORY;
            let contacts = values
                .iter()
                .filter(|value| {
                    **value == parameter.min as f64 || **value == parameter.max as f64
                })
                .count();
            let contact_fraction = if values.is_empty() {
                0.0
            } else {
                contacts as f64 / values.len() as f64
            };
            let movement = (current - parameter.initial as f64).abs() / range;
            let recent_start = values.len() * 2 / 3;
            let recent = &values[recent_start..];
            let recent_span = if enough {
                let low = recent.iter().copied().fold(f64::INFINITY, f64::min);
                let high = recent
                    .iter()
                    .copied()
                    .fold(f64::NEG_INFINITY, f64::max);
                (high - low) / range
            } else {
                0.0
            };
            Ok(SpsaKnobDiagnostics {
                name: parameter.name.clone(),
                initial: parameter.initial,
                min: parameter.min,
                max: parameter.max,
                current,
                current_normalized_to_range: normalize(current, parameter.min, range),
                current_perturbation,
                trajectory,
                thirds: thirds(history, &values, parameter.min, range),
                frequent_bound_contact: history_heuristic(
                    enough,
                    contact_fraction >= SPSA_FREQUENT_BOUND_CONTACT_FRACTION,
                    contact_fraction,
                    SPSA_FREQUENT_BOUND_CONTACT_FRACTION,
                    "fraction of completed centres exactly on either requested bound",
                ),
                little_net_movement: history_heuristic(
                    enough,
                    movement <= SPSA_LITTLE_MOVEMENT_RANGE_FRACTION,
                    movement,
                    SPSA_LITTLE_MOVEMENT_RANGE_FRACTION,
                    "absolute movement from the seed, normalized to the requested range",
                ),
                recent_stability: history_heuristic(
                    enough,
                    recent_span <= SPSA_RECENT_STABILITY_RANGE_FRACTION,
                    recent_span,
                    SPSA_RECENT_STABILITY_RANGE_FRACTION,
                    "span of the most recent third, normalized to the requested range",
                ),
                dead_perturbation: SpsaHeuristic {
                    state: if current_perturbation < 0.5 {
                        SpsaHeuristicState::Observed
                    } else {
                        SpsaHeuristicState::NotObserved
                    },
                    metric: Some(current_perturbation),
                    threshold: Some(0.5),
                    observation: "current scheduled perturbation compared with the half-unit UCI integer-resolution boundary".into(),
                    caveat: heuristic_caveat(),
                },
            })
        })
        .collect::<Result<Vec<_>, SpsaStatusError>>()?;
    Ok(SpsaStatusReport {
        schema_version: SPSA_STATUS_SCHEMA_VERSION,
        settings,
        completed_iterations,
        percent_complete,
        invalid,
        eta,
        knobs,
        interpretation: "trajectory signals are descriptive heuristics, not evidence of causation or convergence and never advice to continue or abandon a tune".into(),
    })
}

fn eta(
    settings: SpsaRunSettings,
    completed: u32,
    invalid: bool,
    elapsed: Option<f64>,
) -> Result<SpsaEta, SpsaStatusError> {
    if invalid {
        return Ok(SpsaEta {
            remaining_seconds: None,
            completed_iterations_basis: completed,
            unavailable_reason: Some("the tune is invalid and will not advance".into()),
            interpretation: "unavailable".into(),
        });
    }
    if completed == settings.iterations {
        return Ok(SpsaEta {
            remaining_seconds: Some(0.0),
            completed_iterations_basis: completed,
            unavailable_reason: None,
            interpretation: "horizon complete".into(),
        });
    }
    let Some(elapsed) = elapsed else {
        return Ok(SpsaEta {
            remaining_seconds: None,
            completed_iterations_basis: completed,
            unavailable_reason: Some(
                "no uncontaminated elapsed-to-checkpoint timing is available".into(),
            ),
            interpretation: "unavailable".into(),
        });
    };
    if !elapsed.is_finite() || elapsed < 0.0 {
        return Err(SpsaStatusError::InvalidElapsed { value: elapsed });
    }
    if completed == 0 || elapsed == 0.0 {
        return Ok(SpsaEta {
            remaining_seconds: None,
            completed_iterations_basis: completed,
            unavailable_reason: Some("no completed timed iteration is available".into()),
            interpretation: "unavailable".into(),
        });
    }
    Ok(SpsaEta {
        remaining_seconds: Some(
            elapsed / f64::from(completed) * f64::from(settings.iterations - completed),
        ),
        completed_iterations_basis: completed,
        unavailable_reason: None,
        interpretation: "linear projection from durable elapsed time through the last committed checkpoint; setup and host-load changes may make it inaccurate".into(),
    })
}

fn thirds(
    history: &[SpsaCenterSample],
    values: &[f64],
    min: i64,
    range: f64,
) -> SpsaThirdsComparison {
    if values.len() < SPSA_DIAGNOSTIC_MIN_HISTORY {
        return SpsaThirdsComparison {
            thirds: None,
            unavailable_reason: Some(format!(
                "need at least {SPSA_DIAGNOSTIC_MIN_HISTORY} completed iterations; have {}",
                values.len()
            )),
        };
    }
    let summaries = std::array::from_fn(|third| {
        let start = third * values.len() / 3;
        let end = (third + 1) * values.len() / 3;
        let slice = &values[start..end];
        let mean = slice.iter().sum::<f64>() / slice.len() as f64;
        SpsaThirdSummary {
            first_iteration: history[start].iteration,
            last_iteration: history[end - 1].iteration,
            samples: slice.len() as u32,
            mean,
            mean_normalized_to_range: normalize(mean, min, range),
        }
    });
    SpsaThirdsComparison {
        thirds: Some(summaries),
        unavailable_reason: None,
    }
}

fn history_heuristic(
    enough: bool,
    observed: bool,
    metric: f64,
    threshold: f64,
    observation: &str,
) -> SpsaHeuristic {
    SpsaHeuristic {
        state: if !enough {
            SpsaHeuristicState::InsufficientHistory
        } else if observed {
            SpsaHeuristicState::Observed
        } else {
            SpsaHeuristicState::NotObserved
        },
        metric: enough.then_some(metric),
        threshold: Some(threshold),
        observation: observation.into(),
        caveat: heuristic_caveat(),
    }
}

fn heuristic_caveat() -> String {
    "may reflect the objective, noise, gain schedule, clipping, host variation or an unsuitable range; it is not a causal or convergence claim".into()
}

fn normalize(value: f64, min: i64, range: f64) -> f64 {
    (value - min as f64) / range
}

#[derive(Debug, Error)]
pub enum SpsaStatusError {
    #[error(transparent)]
    Settings(#[from] SpsaRunSettingsError),
    #[error(transparent)]
    Schedule(#[from] colosseum_core::SpsaError),
    #[error("SPSA schedule horizon {schedule} does not match run setting {settings}")]
    ScheduleHorizonMismatch { schedule: u32, settings: u32 },
    #[error("SPSA tune/schedule dimensions do not match")]
    DimensionMismatch,
    #[error("SPSA history extends beyond the configured horizon")]
    HistoryBeyondHorizon,
    #[error("SPSA history is non-contiguous or has the wrong dimension at index {index}")]
    HistoryMismatch { index: usize },
    #[error("SPSA centre {name}={value} is outside its requested range")]
    CenterOutsideRange { name: String, value: f64 },
    #[error("elapsed-to-checkpoint timing must be finite and nonnegative; got {value}")]
    InvalidElapsed { value: f64 },
}

#[cfg(test)]
mod tests {
    use colosseum_core::{SpsaEndSpec, SpsaScheduleArtifact};

    use super::*;
    use crate::{SpsaBoundParameter, SpsaLiveSpin, SpsaTuneParameter};

    fn bound(c_end: f64) -> SpsaBoundTune {
        SpsaBoundTune {
            parameters: vec![SpsaBoundParameter {
                parameter: SpsaTuneParameter {
                    name: "Reduction".into(),
                    initial: 50,
                    min: 0,
                    max: 100,
                    c_end,
                },
                advertised: SpsaLiveSpin {
                    name: "Reduction".into(),
                    default: 50,
                    min: 0,
                    max: 100,
                },
            }],
        }
    }

    fn schedule(iterations: u32, c_end: f64) -> SpsaScheduleArtifact {
        SpsaScheduleArtifact::derive(
            iterations,
            0.002,
            7,
            &[SpsaEndSpec {
                name: "Reduction".into(),
                min: 0,
                max: 100,
                c_end,
            }],
        )
        .unwrap()
    }

    fn history(values: &[f64]) -> Vec<SpsaCenterSample> {
        values
            .iter()
            .enumerate()
            .map(|(iteration, value)| SpsaCenterSample {
                iteration: iteration as u32,
                centers: vec![*value],
            })
            .collect()
    }

    #[test]
    fn diagnostics_match_hand_computed_thirds_and_eta() {
        let values = [0.0, 50.0, 50.0, 51.0, 51.1, 51.05];
        let report = diagnose_spsa(
            &bound(1.0),
            &schedule(9, 1.0),
            SpsaRunSettings::new(9, 2).unwrap(),
            &history(&values),
            false,
            Some(60.0),
        )
        .unwrap();
        assert_eq!(report.completed_iterations, 6);
        assert!((report.percent_complete - 66.666_666_666_666_67).abs() < 1e-12);
        assert_eq!(report.eta.remaining_seconds, Some(30.0));
        let knob = &report.knobs[0];
        let thirds = knob.thirds.thirds.unwrap();
        assert_eq!(thirds[0].mean, 25.0);
        assert_eq!(thirds[1].mean, 50.5);
        assert_eq!(thirds[2].mean, 51.075);
        assert_eq!(
            knob.frequent_bound_contact.state,
            SpsaHeuristicState::NotObserved
        );
        assert_eq!(
            knob.little_net_movement.state,
            SpsaHeuristicState::NotObserved
        );
        assert_eq!(knob.recent_stability.state, SpsaHeuristicState::Observed);
        assert_eq!(
            knob.dead_perturbation.state,
            SpsaHeuristicState::NotObserved
        );
    }

    #[test]
    fn short_history_is_explicitly_insufficient() {
        let report = diagnose_spsa(
            &bound(1.0),
            &schedule(10, 1.0),
            SpsaRunSettings::new(10, 2).unwrap(),
            &history(&[50.0, 50.1]),
            false,
            None,
        )
        .unwrap();
        let knob = &report.knobs[0];
        assert!(knob.thirds.thirds.is_none());
        assert_eq!(
            knob.frequent_bound_contact.state,
            SpsaHeuristicState::InsufficientHistory
        );
        assert!(report.eta.remaining_seconds.is_none());
    }

    #[test]
    fn dead_perturbation_is_observed_without_inventing_advice() {
        let report = diagnose_spsa(
            &bound(0.4),
            &schedule(6, 0.4),
            SpsaRunSettings::new(6, 2).unwrap(),
            &history(&[50.0; 6]),
            false,
            Some(6.0),
        )
        .unwrap();
        assert_eq!(
            report.knobs[0].dead_perturbation.state,
            SpsaHeuristicState::Observed
        );
        assert!(report.interpretation.contains("never advice"));
    }
}
