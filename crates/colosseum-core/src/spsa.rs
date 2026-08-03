//! Exact, runtime-independent SPSA iteration mathematics.
//!
//! This module owns no UCI schema, engine process, game scheduling or
//! persistence concerns. It turns a floating-point centre vector and the
//! versioned named RNG stream into one pair of integer vectors, then applies a
//! complete mini-match score difference to the centre.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::rng::{NamedRng, stream_names};

pub const SPSA_ALPHA: f64 = 0.601;
pub const SPSA_GAMMA: f64 = 0.102;
pub const SPSA_STABILITY_FRACTION: f64 = 0.1;
const MAX_EXACT_F64_INTEGER: i64 = 9_007_199_254_740_991;

/// Fixed run-wide schedule. Gain decays from the completed iteration index,
/// never from a game or pair count.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpsaSchedule {
    iterations: u32,
    alpha: f64,
    gamma: f64,
    stability_constant: f64,
}

impl SpsaSchedule {
    pub fn new(iterations: u32) -> Result<Self, SpsaError> {
        if iterations == 0 {
            return Err(SpsaError::ZeroIterations);
        }
        Ok(Self {
            iterations,
            alpha: SPSA_ALPHA,
            gamma: SPSA_GAMMA,
            stability_constant: SPSA_STABILITY_FRACTION * f64::from(iterations),
        })
    }

    #[must_use]
    pub const fn iterations(self) -> u32 {
        self.iterations
    }

    #[must_use]
    pub const fn alpha(self) -> f64 {
        self.alpha
    }

    #[must_use]
    pub const fn gamma(self) -> f64 {
        self.gamma
    }

    #[must_use]
    pub const fn stability_constant(self) -> f64 {
        self.stability_constant
    }

    pub fn validate(self) -> Result<(), SpsaError> {
        let expected = Self::new(self.iterations)?;
        if self != expected {
            return Err(SpsaError::UnsupportedSchedule {
                alpha: self.alpha,
                gamma: self.gamma,
                stability_constant: self.stability_constant,
            });
        }
        Ok(())
    }

    pub fn coefficients(
        self,
        iteration: u32,
        knob: SpsaKnob,
    ) -> Result<SpsaCoefficients, SpsaError> {
        self.validate()?;
        knob.validate()?;
        if iteration >= self.iterations {
            return Err(SpsaError::IterationOutOfRange {
                iteration,
                iterations: self.iterations,
            });
        }
        let step = f64::from(iteration) + 1.0;
        let c = knob.c0 / step.powf(self.gamma);
        let a = knob.a0 / (self.stability_constant + step).powf(self.alpha);
        let r = a / c.powi(2);
        let coefficients = SpsaCoefficients { c, a, r };
        coefficients.validate()?;
        Ok(coefficients)
    }
}

/// One tune knob's immutable numeric bounds and initial schedule constants.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpsaKnob {
    pub min: i64,
    pub max: i64,
    pub c0: f64,
    pub a0: f64,
}

impl SpsaKnob {
    pub fn new(min: i64, max: i64, c0: f64, a0: f64) -> Result<Self, SpsaError> {
        let knob = Self { min, max, c0, a0 };
        knob.validate()?;
        Ok(knob)
    }

    pub fn validate(self) -> Result<(), SpsaError> {
        if self.min >= self.max {
            return Err(SpsaError::InvalidBounds {
                min: self.min,
                max: self.max,
            });
        }
        if self.min < -MAX_EXACT_F64_INTEGER || self.max > MAX_EXACT_F64_INTEGER {
            return Err(SpsaError::BoundsOutsideExactFloatRange {
                min: self.min,
                max: self.max,
            });
        }
        for (name, value) in [("c0", self.c0), ("a0", self.a0)] {
            if !value.is_finite() || value <= 0.0 {
                return Err(SpsaError::InvalidGain { name, value });
            }
        }
        Ok(())
    }
}

/// Per-knob coefficients at one iteration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpsaCoefficients {
    pub c: f64,
    pub a: f64,
    pub r: f64,
}

impl SpsaCoefficients {
    fn validate(self) -> Result<(), SpsaError> {
        for (name, value) in [("c", self.c), ("a", self.a), ("r", self.r)] {
            if !value.is_finite() || value <= 0.0 {
                return Err(SpsaError::InvalidCoefficient { name, value });
            }
        }
        Ok(())
    }
}

/// One arm value before and after the binding UCI integer rounding rule.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SpsaArmValue {
    pub floating: f64,
    pub sent: i64,
}

/// Complete deterministic inputs for one mini-match.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaIteration {
    pub iteration: u32,
    pub perturbations: Vec<i8>,
    pub coefficients: Vec<SpsaCoefficients>,
    pub plus: Vec<SpsaArmValue>,
    pub minus: Vec<SpsaArmValue>,
}

/// Reconstruct the exact perturbation draw range for one iteration. Draw order
/// is iteration-major, then tune-file knob order.
pub fn perturbations_for_iteration(
    master_seed: u64,
    iteration: u32,
    knob_count: usize,
) -> Result<Vec<i8>, SpsaError> {
    if knob_count == 0 {
        return Err(SpsaError::EmptyVector);
    }
    let prior_draws = (iteration as u64)
        .checked_mul(knob_count as u64)
        .ok_or(SpsaError::DrawIndexOverflow)?;
    let byte_offset = prior_draws
        .checked_mul(std::mem::size_of::<u64>() as u64)
        .ok_or(SpsaError::DrawIndexOverflow)?;
    let mut rng = NamedRng::new(master_seed, stream_names::SPSA_PERTURBATIONS)
        .expect("the built-in SPSA stream name is valid");
    // A bound of two has a zero rejection threshold, so every Rademacher draw
    // consumes exactly one little-endian u64 from the named stream.
    rng.seek_bytes(byte_offset);
    Ok((0..knob_count).map(|_| rng.rademacher()).collect())
}

/// Construct the clamped floating arms and the exact integer values sent to
/// UCI. The stored centre is not rounded.
pub fn prepare_iteration(
    schedule: SpsaSchedule,
    master_seed: u64,
    iteration: u32,
    centers: &[f64],
    knobs: &[SpsaKnob],
) -> Result<SpsaIteration, SpsaError> {
    validate_vectors(centers, knobs)?;
    let perturbations = perturbations_for_iteration(master_seed, iteration, knobs.len())?;
    let coefficients = knobs
        .iter()
        .map(|knob| schedule.coefficients(iteration, *knob))
        .collect::<Result<Vec<_>, _>>()?;
    let mut plus = Vec::with_capacity(knobs.len());
    let mut minus = Vec::with_capacity(knobs.len());
    for (((center, knob), coefficients), perturbation) in centers
        .iter()
        .zip(knobs)
        .zip(&coefficients)
        .zip(&perturbations)
    {
        let offset = coefficients.c * f64::from(*perturbation);
        plus.push(arm_value(*center + offset, *knob));
        minus.push(arm_value(*center - offset, *knob));
    }
    Ok(SpsaIteration {
        iteration,
        perturbations,
        coefficients,
        plus,
        minus,
    })
}

/// Apply one complete mini-match result, where `score_difference` is plus-arm
/// wins minus plus-arm losses. Draws contribute zero before this function.
pub fn update_centers(
    centers: &[f64],
    knobs: &[SpsaKnob],
    prepared: &SpsaIteration,
    score_difference: i32,
) -> Result<Vec<f64>, SpsaError> {
    validate_vectors(centers, knobs)?;
    let expected = centers.len();
    if prepared.perturbations.len() != expected
        || prepared.coefficients.len() != expected
        || prepared.plus.len() != expected
        || prepared.minus.len() != expected
    {
        return Err(SpsaError::PreparedDimensionMismatch { expected });
    }
    centers
        .iter()
        .zip(knobs)
        .zip(&prepared.coefficients)
        .zip(&prepared.perturbations)
        .map(|(((center, knob), coefficients), perturbation)| {
            coefficients.validate()?;
            if !matches!(perturbation, -1 | 1) {
                return Err(SpsaError::InvalidPerturbation {
                    value: *perturbation,
                });
            }
            let adjustment = coefficients.c
                * coefficients.r
                * f64::from(score_difference)
                * f64::from(*perturbation);
            let updated = *center + adjustment;
            if !updated.is_finite() {
                return Err(SpsaError::NonFiniteUpdate);
            }
            Ok(updated.clamp(knob.min as f64, knob.max as f64))
        })
        .collect()
}

/// Binding, cross-platform tie rule for UCI values.
pub fn round_half_away_from_zero(value: f64) -> Result<i64, SpsaError> {
    if !value.is_finite() {
        return Err(SpsaError::NonFiniteArm { value });
    }
    let rounded = if value.is_sign_negative() {
        (value - 0.5).ceil()
    } else {
        (value + 0.5).floor()
    };
    const I64_UPPER_EXCLUSIVE: f64 = 9_223_372_036_854_775_808.0;
    if !(-I64_UPPER_EXCLUSIVE..I64_UPPER_EXCLUSIVE).contains(&rounded) {
        return Err(SpsaError::RoundedValueOutOfRange { value });
    }
    Ok(rounded as i64)
}

fn arm_value(value: f64, knob: SpsaKnob) -> SpsaArmValue {
    let floating = value.clamp(knob.min as f64, knob.max as f64);
    let sent = round_half_away_from_zero(floating)
        .expect("validated finite exact-float bounds make the arm representable");
    SpsaArmValue { floating, sent }
}

fn validate_vectors(centers: &[f64], knobs: &[SpsaKnob]) -> Result<(), SpsaError> {
    if centers.is_empty() {
        return Err(SpsaError::EmptyVector);
    }
    if centers.len() != knobs.len() {
        return Err(SpsaError::DimensionMismatch {
            centers: centers.len(),
            knobs: knobs.len(),
        });
    }
    for (index, (center, knob)) in centers.iter().zip(knobs).enumerate() {
        knob.validate()?;
        if !center.is_finite() || *center < knob.min as f64 || *center > knob.max as f64 {
            return Err(SpsaError::InvalidCenter {
                index,
                value: *center,
                min: knob.min,
                max: knob.max,
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SpsaError {
    #[error("SPSA iterations must be greater than zero")]
    ZeroIterations,
    #[error(
        "unsupported SPSA schedule constants: alpha={alpha}, gamma={gamma}, A={stability_constant}"
    )]
    UnsupportedSchedule {
        alpha: f64,
        gamma: f64,
        stability_constant: f64,
    },
    #[error("SPSA iteration {iteration} is outside the 0..{iterations} horizon")]
    IterationOutOfRange { iteration: u32, iterations: u32 },
    #[error("SPSA bounds must satisfy min < max; got {min}..{max}")]
    InvalidBounds { min: i64, max: i64 },
    #[error("SPSA bounds {min}..{max} cannot be represented exactly by the floating centre")]
    BoundsOutsideExactFloatRange { min: i64, max: i64 },
    #[error("SPSA {name} must be finite and positive; got {value}")]
    InvalidGain { name: &'static str, value: f64 },
    #[error("SPSA coefficient {name} must be finite and positive; got {value}")]
    InvalidCoefficient { name: &'static str, value: f64 },
    #[error("SPSA vector must contain at least one knob")]
    EmptyVector,
    #[error("SPSA vector dimension mismatch: {centers} centers for {knobs} knobs")]
    DimensionMismatch { centers: usize, knobs: usize },
    #[error("SPSA prepared iteration does not contain exactly {expected} knob values")]
    PreparedDimensionMismatch { expected: usize },
    #[error("SPSA centre {index}={value} is non-finite or outside {min}..{max}")]
    InvalidCenter {
        index: usize,
        value: f64,
        min: i64,
        max: i64,
    },
    #[error("SPSA perturbation must be -1 or +1; got {value}")]
    InvalidPerturbation { value: i8 },
    #[error("SPSA perturbation draw index is not representable")]
    DrawIndexOverflow,
    #[error("SPSA arm value must be finite; got {value}")]
    NonFiniteArm { value: f64 },
    #[error("rounded SPSA arm value is outside the UCI integer range: {value}")]
    RoundedValueOutOfRange { value: f64 },
    #[error("SPSA update produced a non-finite centre")]
    NonFiniteUpdate,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: u64 = 0x0123_4567_89ab_cdef;

    fn close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() <= 1e-14, "{actual} != {expected}");
    }

    #[test]
    fn schedule_matches_hand_computed_fishtest_coefficients_per_iteration() {
        let schedule = SpsaSchedule::new(100).unwrap();
        let knob = SpsaKnob::new(-100, 100, 2.5, 0.75).unwrap();
        let first = schedule.coefficients(0, knob).unwrap();
        close(first.c, 2.5);
        close(first.a, 0.177_494_238_206_212_32);
        close(first.r, 0.028_399_078_112_993_973);
        let second = schedule.coefficients(1, knob).unwrap();
        close(second.c, 2.329_351_073_261_932_5);
        close(second.a, 0.168_450_899_551_546_32);
        close(second.r, 0.031_045_841_525_769_474);
        let last = schedule.coefficients(99, knob).unwrap();
        close(last.c, 1.562_931_731_939_214_4);
        close(last.a, 0.044_481_995_272_847_48);
        close(last.r, 0.018_209_760_852_241_793);
        assert!(last.c < first.c && last.a < first.a);
        assert_eq!(schedule.stability_constant(), 10.0);
    }

    #[test]
    fn named_seed_and_knob_order_reproduce_every_perturbation_and_arm() {
        let schedule = SpsaSchedule::new(10).unwrap();
        let knobs = [
            SpsaKnob::new(-10, 10, 0.5, 0.2).unwrap(),
            SpsaKnob::new(-10, 10, 1.5, 0.2).unwrap(),
            SpsaKnob::new(-10, 10, 1.5, 0.2).unwrap(),
        ];
        let centers = [0.0, 9.0, -9.0];
        let first = prepare_iteration(schedule, MASTER, 0, &centers, &knobs).unwrap();
        let replay = prepare_iteration(schedule, MASTER, 0, &centers, &knobs).unwrap();
        assert_eq!(first, replay);
        assert_eq!(first.perturbations, [-1, 1, 1]);
        assert_eq!(
            first.plus.iter().map(|arm| arm.sent).collect::<Vec<_>>(),
            [-1, 10, -8]
        );
        assert_eq!(
            first.minus.iter().map(|arm| arm.sent).collect::<Vec<_>>(),
            [1, 8, -10]
        );
        assert_eq!(
            perturbations_for_iteration(MASTER, 1, 3).unwrap(),
            [-1, 1, -1]
        );
    }

    #[test]
    fn send_time_rounding_is_half_away_from_zero_and_centers_stay_floating() {
        for (value, expected) in [
            (-2.5, -3),
            (-1.5, -2),
            (-0.5, -1),
            (0.0, 0),
            (0.5, 1),
            (1.5, 2),
            (2.5, 3),
        ] {
            assert_eq!(round_half_away_from_zero(value).unwrap(), expected);
        }
        let knobs = [SpsaKnob::new(-10, 10, 1.0, 1.0).unwrap()];
        let prepared = SpsaIteration {
            iteration: 0,
            perturbations: vec![1],
            coefficients: vec![SpsaCoefficients {
                c: 2.0,
                a: 0.4,
                r: 0.1,
            }],
            plus: vec![SpsaArmValue {
                floating: 2.25,
                sent: 2,
            }],
            minus: vec![SpsaArmValue {
                floating: -1.75,
                sent: -2,
            }],
        };
        let updated = update_centers(&[0.25], &knobs, &prepared, 3).unwrap();
        close(updated[0], 0.85);
    }

    #[test]
    fn score_sign_swap_negates_the_update_and_bounds_clip() {
        let knobs = [
            SpsaKnob::new(-1, 1, 1.0, 1.0).unwrap(),
            SpsaKnob::new(-10, 10, 1.0, 1.0).unwrap(),
        ];
        let prepared = SpsaIteration {
            iteration: 0,
            perturbations: vec![-1, 1],
            coefficients: vec![
                SpsaCoefficients {
                    c: 2.0,
                    a: 0.4,
                    r: 0.1,
                },
                SpsaCoefficients {
                    c: 2.0,
                    a: 0.4,
                    r: 0.1,
                },
            ],
            plus: vec![
                SpsaArmValue {
                    floating: 0.0,
                    sent: 0
                };
                2
            ],
            minus: vec![
                SpsaArmValue {
                    floating: 0.0,
                    sent: 0
                };
                2
            ],
        };
        let positive = update_centers(&[0.9, 0.0], &knobs, &prepared, 3).unwrap();
        let negative = update_centers(&[0.9, 0.0], &knobs, &prepared, -3).unwrap();
        close(positive[0], 0.3);
        close(negative[0], 1.0);
        close(positive[1], 0.6);
        close(negative[1], -0.6);
    }

    #[test]
    fn invalid_schedule_vectors_and_numeric_inputs_are_typed_errors() {
        assert_eq!(SpsaSchedule::new(0), Err(SpsaError::ZeroIterations));
        let schedule = SpsaSchedule::new(2).unwrap();
        let knob = SpsaKnob::new(-10, 10, 1.0, 1.0).unwrap();
        assert!(matches!(
            schedule.coefficients(2, knob),
            Err(SpsaError::IterationOutOfRange { .. })
        ));
        assert!(matches!(
            SpsaKnob::new(1, 1, 1.0, 1.0),
            Err(SpsaError::InvalidBounds { .. })
        ));
        assert!(matches!(
            SpsaKnob::new(-1, 1, f64::NAN, 1.0),
            Err(SpsaError::InvalidGain { .. })
        ));
        assert!(matches!(
            prepare_iteration(schedule, MASTER, 0, &[0.0, 1.0], &[knob]),
            Err(SpsaError::DimensionMismatch { .. })
        ));
        assert!(matches!(
            prepare_iteration(schedule, MASTER, 0, &[11.0], &[knob]),
            Err(SpsaError::InvalidCenter { .. })
        ));
        assert!(matches!(
            round_half_away_from_zero(f64::INFINITY),
            Err(SpsaError::NonFiniteArm { .. })
        ));
        assert!(matches!(
            round_half_away_from_zero(9_223_372_036_854_775_808.0),
            Err(SpsaError::RoundedValueOutOfRange { .. })
        ));
    }
}
