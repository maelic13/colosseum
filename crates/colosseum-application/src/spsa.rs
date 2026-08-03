//! Runtime-neutral SPSA preflight and live-schema binding policy.

use std::collections::HashSet;

use colosseum_core::{
    SpsaEndSpec, SpsaError, SpsaIteration, SpsaKnob, SpsaScheduleArtifact, prepare_iteration,
    update_centers,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{EngineInspection, UciOptionSchema};

pub const DEFAULT_SPSA_ITERATIONS: u32 = 5_000;
pub const DEFAULT_SPSA_GAMES_PER_ITERATION: u32 = 32;

/// Resolved run-wide SPSA sizing. The defaults are useful production values,
/// not minimums: short development or synthetic runs may use one iteration
/// and one complete colour-reversed pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpsaRunSettings {
    pub iterations: u32,
    pub games_per_iteration: u32,
}

impl Default for SpsaRunSettings {
    fn default() -> Self {
        Self {
            iterations: DEFAULT_SPSA_ITERATIONS,
            games_per_iteration: DEFAULT_SPSA_GAMES_PER_ITERATION,
        }
    }
}

impl SpsaRunSettings {
    pub fn new(iterations: u32, games_per_iteration: u32) -> Result<Self, SpsaRunSettingsError> {
        let settings = Self {
            iterations,
            games_per_iteration,
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn validate(self) -> Result<(), SpsaRunSettingsError> {
        if self.iterations == 0 {
            return Err(SpsaRunSettingsError::ZeroIterations);
        }
        if self.games_per_iteration == 0 {
            return Err(SpsaRunSettingsError::ZeroGamesPerIteration);
        }
        if !self.games_per_iteration.is_multiple_of(2) {
            return Err(SpsaRunSettingsError::IncompleteColourPair {
                games_per_iteration: self.games_per_iteration,
            });
        }
        Ok(())
    }

    #[must_use]
    pub const fn pairs_per_iteration(self) -> u32 {
        self.games_per_iteration / 2
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SpsaRunSettingsError {
    #[error("SPSA iterations must be greater than zero")]
    ZeroIterations,
    #[error("SPSA games per iteration must be greater than zero")]
    ZeroGamesPerIteration,
    #[error(
        "SPSA games per iteration must be even so each mini-match contains complete colour pairs; got {games_per_iteration}"
    )]
    IncompleteColourPair { games_per_iteration: u32 },
}

/// Score of one complete SPSA mini-match from the plus arm's perspective.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpsaMiniMatchScore {
    pub plus_wins: u32,
    pub plus_losses: u32,
    pub draws: u32,
    pub difference: i32,
}

impl SpsaMiniMatchScore {
    fn validate(self, expected_games: u32) -> Result<(), SpsaDriverPolicyError> {
        let games = self
            .plus_wins
            .checked_add(self.plus_losses)
            .and_then(|value| value.checked_add(self.draws))
            .ok_or(SpsaDriverPolicyError::ScoreOverflow)?;
        let difference = i32::try_from(self.plus_wins)
            .and_then(|wins| i32::try_from(self.plus_losses).map(|losses| wins - losses))
            .map_err(|_| SpsaDriverPolicyError::ScoreOverflow)?;
        if games != expected_games || difference != self.difference {
            return Err(SpsaDriverPolicyError::InvalidScore {
                expected_games,
                actual_games: games,
                expected_difference: difference,
                actual_difference: self.difference,
            });
        }
        Ok(())
    }
}

/// Runtime-neutral official update retained in a durable SPSA checkpoint.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaCommittedUpdate {
    pub iteration: u32,
    pub centers_before: Vec<f64>,
    pub prepared: SpsaIteration,
    pub score: SpsaMiniMatchScore,
    pub centers_after: Vec<f64>,
}

/// Terminal policy result for a complete mini-match containing an engine fault.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaInvalidUpdate {
    pub iteration: u32,
    pub centers_before: Vec<f64>,
    pub prepared: SpsaIteration,
    pub engine_faults: u32,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpsaIterationTransition {
    Committed(SpsaCommittedUpdate),
    Invalid(SpsaInvalidUpdate),
}

/// Application-owned SPSA state machine. Runtime adapters execute the prepared
/// games and durably persist returned transitions; they cannot advance the gain
/// schedule or apply a forfeit as a gradient themselves.
#[derive(Debug, Clone)]
pub struct SpsaTuningState {
    schedule: VerifiedSpsaSchedule,
    settings: SpsaRunSettings,
    knobs: Vec<SpsaKnob>,
    centers: Vec<f64>,
    completed_iterations: u32,
    invalid: bool,
}

impl SpsaTuningState {
    pub fn resume(
        schedule: VerifiedSpsaSchedule,
        settings: SpsaRunSettings,
        initial_centers: Vec<f64>,
        history: &[SpsaCommittedUpdate],
    ) -> Result<Self, SpsaDriverPolicyError> {
        settings.validate()?;
        let artifact = schedule.artifact();
        if artifact.schedule.iterations() != settings.iterations {
            return Err(SpsaDriverPolicyError::ScheduleHorizonMismatch {
                schedule: artifact.schedule.iterations(),
                settings: settings.iterations,
            });
        }
        if history.len() > settings.iterations as usize {
            return Err(SpsaDriverPolicyError::HistoryBeyondHorizon);
        }
        let knobs = artifact
            .knobs
            .iter()
            .map(|knob| knob.knob())
            .collect::<Result<Vec<_>, _>>()?;
        let mut state = Self {
            schedule,
            settings,
            knobs,
            centers: initial_centers,
            completed_iterations: 0,
            invalid: false,
        };
        for stored in history {
            let iteration = state.completed_iterations;
            let prepared = state
                .prepare_next()?
                .ok_or(SpsaDriverPolicyError::HistoryBeyondHorizon)?;
            if stored.iteration != iteration
                || stored.centers_before != state.centers
                || stored.prepared != prepared
            {
                return Err(SpsaDriverPolicyError::HistoryMismatch { iteration });
            }
            let transition = state.commit_iteration(
                prepared,
                settings.pairs_per_iteration(),
                Some(stored.score),
                0,
            )?;
            let SpsaIterationTransition::Committed(replayed) = transition else {
                unreachable!("fault-free history can only produce a committed update")
            };
            if &replayed != stored {
                return Err(SpsaDriverPolicyError::HistoryMismatch { iteration });
            }
        }
        // Validate even a fresh initial vector before an adapter may launch.
        if state.completed_iterations < state.settings.iterations {
            state.prepare_next()?;
        }
        Ok(state)
    }

    pub fn prepare_next(&self) -> Result<Option<SpsaIteration>, SpsaDriverPolicyError> {
        if self.invalid {
            return Err(SpsaDriverPolicyError::TuneAlreadyInvalid);
        }
        if self.completed_iterations == self.settings.iterations {
            return Ok(None);
        }
        let artifact = self.schedule.artifact();
        prepare_iteration(
            artifact.schedule,
            artifact.perturbations.master_seed,
            self.completed_iterations,
            &self.centers,
            &self.knobs,
        )
        .map(Some)
        .map_err(Into::into)
    }

    pub fn commit_iteration(
        &mut self,
        prepared: SpsaIteration,
        completed_pairs: u32,
        score: Option<SpsaMiniMatchScore>,
        engine_faults: u32,
    ) -> Result<SpsaIterationTransition, SpsaDriverPolicyError> {
        if self.invalid {
            return Err(SpsaDriverPolicyError::TuneAlreadyInvalid);
        }
        let iteration = self.completed_iterations;
        let expected = self
            .prepare_next()?
            .ok_or(SpsaDriverPolicyError::HorizonComplete)?;
        if prepared != expected {
            return Err(SpsaDriverPolicyError::PreparedIterationMismatch { iteration });
        }
        if completed_pairs != self.settings.pairs_per_iteration() {
            return Err(SpsaDriverPolicyError::IncompleteMiniMatch {
                iteration,
                expected_pairs: self.settings.pairs_per_iteration(),
                completed_pairs,
            });
        }
        if engine_faults > 0 {
            if score.is_some() {
                return Err(SpsaDriverPolicyError::FaultedGradientSupplied { iteration });
            }
            self.invalid = true;
            return Ok(SpsaIterationTransition::Invalid(SpsaInvalidUpdate {
                iteration,
                centers_before: self.centers.clone(),
                prepared,
                engine_faults,
                reason: "an engine-attributable fault invalidates the complete mini-match; no gradient was applied".into(),
            }));
        }
        let score = score.ok_or(SpsaDriverPolicyError::MissingScore { iteration })?;
        score.validate(self.settings.games_per_iteration)?;
        let centers_before = self.centers.clone();
        let centers_after =
            update_centers(&centers_before, &self.knobs, &prepared, score.difference)?;
        self.centers.clone_from(&centers_after);
        self.completed_iterations += 1;
        Ok(SpsaIterationTransition::Committed(SpsaCommittedUpdate {
            iteration,
            centers_before,
            prepared,
            score,
            centers_after,
        }))
    }

    #[must_use]
    pub fn centers(&self) -> &[f64] {
        &self.centers
    }

    #[must_use]
    pub const fn completed_iterations(&self) -> u32 {
        self.completed_iterations
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SpsaDriverPolicyError {
    #[error(transparent)]
    Settings(#[from] SpsaRunSettingsError),
    #[error(transparent)]
    Schedule(#[from] SpsaError),
    #[error("SPSA schedule horizon {schedule} does not match run setting {settings}")]
    ScheduleHorizonMismatch { schedule: u32, settings: u32 },
    #[error("SPSA durable history extends beyond the configured horizon")]
    HistoryBeyondHorizon,
    #[error("SPSA durable history does not reproduce iteration {iteration}")]
    HistoryMismatch { iteration: u32 },
    #[error("SPSA prepared inputs do not reproduce iteration {iteration}")]
    PreparedIterationMismatch { iteration: u32 },
    #[error("SPSA mini-match {iteration} completed {completed_pairs}/{expected_pairs} pairs")]
    IncompleteMiniMatch {
        iteration: u32,
        expected_pairs: u32,
        completed_pairs: u32,
    },
    #[error(
        "SPSA score count/difference is inconsistent: expected {expected_games} games and difference {expected_difference}, got {actual_games} games and {actual_difference}"
    )]
    InvalidScore {
        expected_games: u32,
        actual_games: u32,
        expected_difference: i32,
        actual_difference: i32,
    },
    #[error("SPSA score count overflow")]
    ScoreOverflow,
    #[error("SPSA iteration {iteration} supplied a gradient despite an engine fault")]
    FaultedGradientSupplied { iteration: u32 },
    #[error("SPSA iteration {iteration} has no complete mini-match score")]
    MissingScore { iteration: u32 },
    #[error("SPSA tune is already invalid")]
    TuneAlreadyInvalid,
    #[error("SPSA horizon is already complete")]
    HorizonComplete,
}

/// Required contents of one ordered SPSA tune-file entry. This is deliberately
/// a numeric UCI-spin vector; source, compiler and engine-specific metadata do
/// not belong in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpsaTuneParameter {
    pub name: String,
    pub initial: i64,
    pub min: i64,
    pub max: i64,
    pub c_end: f64,
}

/// Parsed tune-file content. Parameter order is semantically binding: it is
/// the order consumed by the versioned SPSA perturbation stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpsaTune {
    pub parameters: Vec<SpsaTuneParameter>,
}

/// One tune parameter paired with the exact spin schema advertised by the
/// engine in the live UCI handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpsaLiveSpin {
    pub name: String,
    pub default: i64,
    pub min: i64,
    pub max: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaBoundParameter {
    pub parameter: SpsaTuneParameter,
    pub advertised: SpsaLiveSpin,
}

/// Schema-bound tune vector ready for audit, schedule derivation and game-driver
/// use cases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaBoundTune {
    pub parameters: Vec<SpsaBoundParameter>,
}

/// Non-fatal observations from an SPSA configuration audit. They remain in the
/// run record because a deliberate non-default starting point can materially
/// affect how a tune should be interpreted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SpsaTuneWarning {
    InitialDiffersFromEngineDefault {
        name: String,
        initial: i64,
        advertised_default: i64,
    },
    InitialOnLowerRail {
        name: String,
        initial: i64,
        rail: i64,
    },
    InitialOnUpperRail {
        name: String,
        initial: i64,
        rail: i64,
    },
}

/// Completed application-level SPSA configuration audit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpsaTuneAudit {
    pub warnings: Vec<SpsaTuneWarning>,
}

impl SpsaTune {
    /// Validate tune-file invariants that do not require launching the engine.
    /// This runs before schedule derivation so a dry run has the same hard
    /// duplicate, range-shape and UCI-integer-resolution protections as a live
    /// tune. The live-schema portion is completed by [`SpsaBoundTune::audit`].
    pub fn audit_configuration(&self) -> Result<(), SpsaTuneAuditError> {
        let mut names = HashSet::with_capacity(self.parameters.len());
        for parameter in &self.parameters {
            if !names.insert(&parameter.name) {
                return Err(SpsaTuneAuditError::DuplicateParameter {
                    name: parameter.name.clone(),
                });
            }
            validate_tune_parameter_shape(parameter)?;
        }
        Ok(())
    }

    /// Bind every ordered entry to the schema observed from this ordinary UCI
    /// executable. The resulting vector is then checked by
    /// [`SpsaBoundTune::audit`] for range, duplicate and rounding-resolution
    /// policy.
    pub fn bind_live_schema(
        &self,
        inspection: &EngineInspection,
    ) -> Result<SpsaBoundTune, SpsaTuneError> {
        if self.parameters.is_empty() {
            return Err(SpsaTuneError::EmptyParameters);
        }
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| {
                let option = inspection
                    .options
                    .iter()
                    .find(|option| option.name() == parameter.name)
                    .ok_or_else(|| SpsaTuneError::OptionNotAdvertised {
                        name: parameter.name.clone(),
                    })?;
                let UciOptionSchema::Spin {
                    name,
                    default,
                    min,
                    max,
                } = option
                else {
                    return Err(SpsaTuneError::OptionIsNotSpin {
                        name: parameter.name.clone(),
                        advertised_kind: option_kind(option),
                    });
                };
                Ok(SpsaBoundParameter {
                    parameter: parameter.clone(),
                    advertised: SpsaLiveSpin {
                        name: name.clone(),
                        default: *default,
                        min: *min,
                        max: *max,
                    },
                })
            })
            .collect::<Result<Vec<_>, SpsaTuneError>>()?;
        Ok(SpsaBoundTune { parameters })
    }
}

impl SpsaBoundTune {
    /// Validate the complete tune vector against the observed UCI spin schema.
    /// The audit deliberately separates hard refusals from warnings: all hard
    /// failures would make one or both arms unmeasurable, while defaults and
    /// rail seeds can be intentional experimental choices.
    pub fn audit(&self) -> Result<SpsaTuneAudit, SpsaTuneAuditError> {
        let mut warnings = Vec::new();
        let mut names = HashSet::with_capacity(self.parameters.len());
        for bound in &self.parameters {
            let parameter = &bound.parameter;
            if !names.insert(&parameter.name) {
                return Err(SpsaTuneAuditError::DuplicateParameter {
                    name: parameter.name.clone(),
                });
            }
            validate_tune_parameter_shape(parameter)?;
            for (field, value) in [
                ("initial", parameter.initial),
                ("min", parameter.min),
                ("max", parameter.max),
            ] {
                if value < bound.advertised.min || value > bound.advertised.max {
                    return Err(SpsaTuneAuditError::ValueOutsideAdvertisedRange {
                        name: parameter.name.clone(),
                        field,
                        value,
                        advertised_min: bound.advertised.min,
                        advertised_max: bound.advertised.max,
                    });
                }
            }
            if parameter.initial != bound.advertised.default {
                warnings.push(SpsaTuneWarning::InitialDiffersFromEngineDefault {
                    name: parameter.name.clone(),
                    initial: parameter.initial,
                    advertised_default: bound.advertised.default,
                });
            }
            if parameter.initial == parameter.min {
                warnings.push(SpsaTuneWarning::InitialOnLowerRail {
                    name: parameter.name.clone(),
                    initial: parameter.initial,
                    rail: parameter.min,
                });
            }
            if parameter.initial == parameter.max {
                warnings.push(SpsaTuneWarning::InitialOnUpperRail {
                    name: parameter.name.clone(),
                    initial: parameter.initial,
                    rail: parameter.max,
                });
            }
        }
        Ok(SpsaTuneAudit { warnings })
    }

    #[must_use]
    pub fn end_specs(&self) -> Vec<SpsaEndSpec> {
        self.parameters
            .iter()
            .map(|bound| SpsaEndSpec {
                name: bound.parameter.name.clone(),
                min: bound.parameter.min,
                max: bound.parameter.max,
                c_end: bound.parameter.c_end,
            })
            .collect()
    }

    #[must_use]
    pub fn initial_centers(&self) -> Vec<f64> {
        self.parameters
            .iter()
            .map(|bound| bound.parameter.initial as f64)
            .collect()
    }
}

fn validate_tune_parameter_shape(parameter: &SpsaTuneParameter) -> Result<(), SpsaTuneAuditError> {
    if parameter.min >= parameter.max {
        return Err(SpsaTuneAuditError::InvalidTuningBounds {
            name: parameter.name.clone(),
            min: parameter.min,
            max: parameter.max,
        });
    }
    if parameter.initial < parameter.min || parameter.initial > parameter.max {
        return Err(SpsaTuneAuditError::InitialOutsideTuningBounds {
            name: parameter.name.clone(),
            initial: parameter.initial,
            min: parameter.min,
            max: parameter.max,
        });
    }
    // UCI receives integer values. At the final schedule point `c == c_end`;
    // with the binding half-away-from-zero rule, a magnitude below 0.5 sends
    // zero for both arms around an integral centre, making the knob unmeasured.
    if parameter.c_end.is_finite() && parameter.c_end > 0.0 && parameter.c_end < 0.5 {
        return Err(SpsaTuneAuditError::PerturbationRoundsToZero {
            name: parameter.name.clone(),
            c_end: parameter.c_end,
        });
    }
    Ok(())
}

fn option_kind(option: &UciOptionSchema) -> &'static str {
    match option {
        UciOptionSchema::Check { .. } => "check",
        UciOptionSchema::Spin { .. } => "spin",
        UciOptionSchema::Combo { .. } => "combo",
        UciOptionSchema::Button { .. } => "button",
        UciOptionSchema::String { .. } => "string",
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SpsaTuneError {
    #[error("SPSA tune file must declare at least one parameter")]
    EmptyParameters,
    #[error("SPSA parameter {name:?} was not advertised by the live UCI engine")]
    OptionNotAdvertised { name: String },
    #[error("SPSA parameter {name:?} must map to an advertised spin option, not {advertised_kind}")]
    OptionIsNotSpin {
        name: String,
        advertised_kind: &'static str,
    },
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SpsaTuneAuditError {
    #[error("SPSA tune parameter {name:?} is declared more than once")]
    DuplicateParameter { name: String },
    #[error(
        "SPSA parameter {name:?} has invalid tuning bounds: min {min} must be less than max {max}"
    )]
    InvalidTuningBounds { name: String, min: i64, max: i64 },
    #[error(
        "SPSA parameter {name:?} initial value {initial} must lie within its tuning bounds [{min}, {max}]"
    )]
    InitialOutsideTuningBounds {
        name: String,
        initial: i64,
        min: i64,
        max: i64,
    },
    #[error(
        "SPSA parameter {name:?} {field} value {value} is outside advertised UCI spin range [{advertised_min}, {advertised_max}]"
    )]
    ValueOutsideAdvertisedRange {
        name: String,
        field: &'static str,
        value: i64,
        advertised_min: i64,
        advertised_max: i64,
    },
    #[error(
        "SPSA parameter {name:?} c_end {c_end} rounds to zero at the end of the schedule; use at least 0.5 for an integer UCI option"
    )]
    PerturbationRoundsToZero { name: String, c_end: f64 },
}

/// Capability token proving that the schedule read from durable storage is
/// valid and exactly matches the schedule derived from resolved run inputs.
/// The SPSA driver accepts this token rather than an unchecked artifact.
#[derive(Debug, Clone)]
pub struct VerifiedSpsaSchedule {
    artifact: SpsaScheduleArtifact,
}

impl VerifiedSpsaSchedule {
    pub fn verify_written(
        expected: &SpsaScheduleArtifact,
        written: SpsaScheduleArtifact,
    ) -> Result<Self, SpsaPreflightError> {
        expected.validate()?;
        written.validate()?;
        if &written != expected {
            return Err(SpsaPreflightError::WrittenScheduleMismatch);
        }
        Ok(Self { artifact: written })
    }

    #[must_use]
    pub fn artifact(&self) -> &SpsaScheduleArtifact {
        &self.artifact
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum SpsaPreflightError {
    #[error("SPSA schedule is invalid: {0}")]
    InvalidSchedule(#[from] SpsaError),
    #[error("written SPSA schedule does not exactly match the schedule derived from run inputs")]
    WrittenScheduleMismatch,
}

#[cfg(test)]
mod tests {
    use colosseum_core::{SpsaEndSpec, SpsaScheduleArtifact};

    use super::*;

    fn inspection(options: Vec<UciOptionSchema>) -> EngineInspection {
        EngineInspection {
            name: Some("fixture".into()),
            author: None,
            options,
            diagnostics: Vec::new(),
        }
    }

    fn parameter(initial: i64, min: i64, max: i64, c_end: f64) -> SpsaTuneParameter {
        SpsaTuneParameter {
            name: "Hash".into(),
            initial,
            min,
            max,
            c_end,
        }
    }

    fn bind(parameter: SpsaTuneParameter) -> SpsaBoundTune {
        SpsaTune {
            parameters: vec![parameter],
        }
        .bind_live_schema(&inspection(vec![UciOptionSchema::Spin {
            name: "Hash".into(),
            default: 16,
            min: 1,
            max: 1024,
        }]))
        .unwrap()
    }

    #[test]
    fn run_settings_default_to_useful_production_values_without_becoming_minima() {
        let defaults = SpsaRunSettings::default();
        assert_eq!(defaults.iterations, DEFAULT_SPSA_ITERATIONS);
        assert_eq!(
            defaults.games_per_iteration,
            DEFAULT_SPSA_GAMES_PER_ITERATION
        );
        assert_eq!(defaults.pairs_per_iteration(), 16);

        let short = SpsaRunSettings::new(1, 2).unwrap();
        assert_eq!(short.iterations, 1);
        assert_eq!(short.games_per_iteration, 2);
        assert_eq!(short.pairs_per_iteration(), 1);
    }

    #[test]
    fn run_settings_reject_only_non_executable_iteration_or_pair_counts() {
        assert_eq!(
            SpsaRunSettings::new(0, 2),
            Err(SpsaRunSettingsError::ZeroIterations)
        );
        assert_eq!(
            SpsaRunSettings::new(1, 0),
            Err(SpsaRunSettingsError::ZeroGamesPerIteration)
        );
        assert_eq!(
            SpsaRunSettings::new(1, 3),
            Err(SpsaRunSettingsError::IncompleteColourPair {
                games_per_iteration: 3
            })
        );
    }

    fn schedule(seed: u64) -> SpsaScheduleArtifact {
        SpsaScheduleArtifact::derive(
            100,
            0.01,
            seed,
            &[SpsaEndSpec {
                name: "Tempo".into(),
                min: -100,
                max: 100,
                c_end: 1.0,
            }],
        )
        .unwrap()
    }

    #[test]
    fn verified_token_requires_the_written_schedule_to_match_resolved_inputs() {
        let expected = schedule(7);
        let verified = VerifiedSpsaSchedule::verify_written(&expected, expected.clone()).unwrap();
        assert_eq!(verified.artifact(), &expected);
        assert_eq!(
            VerifiedSpsaSchedule::verify_written(&expected, schedule(8)).unwrap_err(),
            SpsaPreflightError::WrittenScheduleMismatch
        );
    }

    #[test]
    fn application_state_owns_complete_iteration_commit_and_exact_resume() {
        let artifact = schedule(7);
        let verified = VerifiedSpsaSchedule::verify_written(&artifact, artifact.clone()).unwrap();
        let settings = SpsaRunSettings::new(100, 2).unwrap();
        let mut state =
            SpsaTuningState::resume(verified.clone(), settings, vec![0.0], &[]).unwrap();
        let prepared = state.prepare_next().unwrap().unwrap();
        assert!(matches!(
            state.commit_iteration(prepared.clone(), 0, None, 0),
            Err(SpsaDriverPolicyError::IncompleteMiniMatch { .. })
        ));
        let score = SpsaMiniMatchScore {
            plus_wins: 1,
            plus_losses: 0,
            draws: 1,
            difference: 1,
        };
        let SpsaIterationTransition::Committed(update) =
            state.commit_iteration(prepared, 1, Some(score), 0).unwrap()
        else {
            panic!("fault-free complete mini-match must commit")
        };
        assert_eq!(state.completed_iterations(), 1);
        assert_eq!(state.centers(), update.centers_after);

        let resumed = SpsaTuningState::resume(verified, settings, vec![0.0], &[update]).unwrap();
        assert_eq!(resumed.completed_iterations(), 1);
        assert_eq!(resumed.centers(), state.centers());
        assert_eq!(
            resumed.prepare_next().unwrap().unwrap().iteration,
            1,
            "resume must continue the exact RNG iteration"
        );
    }

    #[test]
    fn application_state_never_turns_an_engine_fault_into_a_gradient() {
        let artifact = schedule(7);
        let verified = VerifiedSpsaSchedule::verify_written(&artifact, artifact.clone()).unwrap();
        let settings = SpsaRunSettings::new(100, 2).unwrap();
        let mut state = SpsaTuningState::resume(verified, settings, vec![0.0], &[]).unwrap();
        let prepared = state.prepare_next().unwrap().unwrap();
        assert!(matches!(
            state.commit_iteration(
                prepared.clone(),
                1,
                Some(SpsaMiniMatchScore {
                    plus_wins: 1,
                    plus_losses: 0,
                    draws: 1,
                    difference: 1,
                }),
                1,
            ),
            Err(SpsaDriverPolicyError::FaultedGradientSupplied { iteration: 0 })
        ));
        let SpsaIterationTransition::Invalid(invalid) =
            state.commit_iteration(prepared, 1, None, 1).unwrap()
        else {
            panic!("engine fault must invalidate")
        };
        assert_eq!(invalid.centers_before, [0.0]);
        assert_eq!(state.centers(), [0.0]);
        assert_eq!(state.completed_iterations(), 0);
        assert!(matches!(
            state.prepare_next(),
            Err(SpsaDriverPolicyError::TuneAlreadyInvalid)
        ));
    }

    #[test]
    fn tune_binding_preserves_parameter_order_and_the_advertised_spin_schema() {
        let tune = SpsaTune {
            parameters: vec![
                SpsaTuneParameter {
                    name: "Reduction".into(),
                    initial: 12,
                    min: 0,
                    max: 64,
                    c_end: 0.5,
                },
                SpsaTuneParameter {
                    name: "Aspiration".into(),
                    initial: 20,
                    min: 1,
                    max: 128,
                    c_end: 1.25,
                },
            ],
        };
        let bound = tune
            .bind_live_schema(&inspection(vec![
                UciOptionSchema::Spin {
                    name: "Aspiration".into(),
                    default: 16,
                    min: 0,
                    max: 256,
                },
                UciOptionSchema::Spin {
                    name: "Reduction".into(),
                    default: 8,
                    min: 0,
                    max: 128,
                },
            ]))
            .unwrap();
        assert_eq!(
            bound
                .parameters
                .iter()
                .map(|parameter| parameter.parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["Reduction", "Aspiration"]
        );
        assert_eq!(bound.parameters[0].advertised.default, 8);
        assert_eq!(bound.initial_centers(), [12.0, 20.0]);
        assert_eq!(
            bound
                .end_specs()
                .into_iter()
                .map(|spec| spec.name)
                .collect::<Vec<_>>(),
            ["Reduction", "Aspiration"]
        );
    }

    #[test]
    fn configuration_audit_rejects_duplicate_unmeasurable_and_out_of_range_vectors() {
        assert_eq!(
            SpsaTune {
                parameters: vec![parameter(16, 1, 1024, 1.0), parameter(16, 1, 1024, 1.0)]
            }
            .audit_configuration(),
            Err(SpsaTuneAuditError::DuplicateParameter {
                name: "Hash".into()
            })
        );
        assert_eq!(
            SpsaTune {
                parameters: vec![parameter(16, 32, 32, 1.0)]
            }
            .audit_configuration(),
            Err(SpsaTuneAuditError::InvalidTuningBounds {
                name: "Hash".into(),
                min: 32,
                max: 32,
            })
        );
        assert_eq!(
            SpsaTune {
                parameters: vec![parameter(16, 32, 64, 1.0)]
            }
            .audit_configuration(),
            Err(SpsaTuneAuditError::InitialOutsideTuningBounds {
                name: "Hash".into(),
                initial: 16,
                min: 32,
                max: 64,
            })
        );
        assert_eq!(
            SpsaTune {
                parameters: vec![parameter(16, 1, 1024, 0.499_999)]
            }
            .audit_configuration(),
            Err(SpsaTuneAuditError::PerturbationRoundsToZero {
                name: "Hash".into(),
                c_end: 0.499_999,
            })
        );
        assert_eq!(
            bind(parameter(1025, 1, 1025, 1.0)).audit(),
            Err(SpsaTuneAuditError::ValueOutsideAdvertisedRange {
                name: "Hash".into(),
                field: "initial",
                value: 1025,
                advertised_min: 1,
                advertised_max: 1024,
            })
        );
        assert_eq!(
            bind(parameter(16, 0, 1024, 1.0)).audit(),
            Err(SpsaTuneAuditError::ValueOutsideAdvertisedRange {
                name: "Hash".into(),
                field: "min",
                value: 0,
                advertised_min: 1,
                advertised_max: 1024,
            })
        );
        assert!(bind(parameter(16, 1, 1024, 0.5)).audit().is_ok());
    }

    #[test]
    fn configuration_audit_records_default_and_rail_warnings_without_refusal() {
        let audit = bind(parameter(1, 1, 1024, 1.0)).audit().unwrap();
        assert_eq!(
            audit.warnings,
            vec![
                SpsaTuneWarning::InitialDiffersFromEngineDefault {
                    name: "Hash".into(),
                    initial: 1,
                    advertised_default: 16,
                },
                SpsaTuneWarning::InitialOnLowerRail {
                    name: "Hash".into(),
                    initial: 1,
                    rail: 1,
                },
            ]
        );
    }

    #[test]
    fn tune_binding_rejects_empty_missing_and_non_spin_live_options() {
        let inspection = inspection(vec![UciOptionSchema::Check {
            name: "Use NNUE".into(),
            default: true,
        }]);
        assert_eq!(
            SpsaTune {
                parameters: Vec::new()
            }
            .bind_live_schema(&inspection),
            Err(SpsaTuneError::EmptyParameters)
        );
        let missing = SpsaTune {
            parameters: vec![SpsaTuneParameter {
                name: "Missing".into(),
                initial: 1,
                min: 0,
                max: 2,
                c_end: 0.5,
            }],
        };
        assert_eq!(
            missing.bind_live_schema(&inspection),
            Err(SpsaTuneError::OptionNotAdvertised {
                name: "Missing".into()
            })
        );
        let non_spin = SpsaTune {
            parameters: vec![SpsaTuneParameter {
                name: "Use NNUE".into(),
                initial: 1,
                min: 0,
                max: 2,
                c_end: 0.5,
            }],
        };
        assert_eq!(
            non_spin.bind_live_schema(&inspection),
            Err(SpsaTuneError::OptionIsNotSpin {
                name: "Use NNUE".into(),
                advertised_kind: "check"
            })
        );
    }
}
