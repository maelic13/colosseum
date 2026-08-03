//! Runtime-neutral SPSA preflight and live-schema binding policy.

use colosseum_core::{SpsaEndSpec, SpsaError, SpsaScheduleArtifact};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{EngineInspection, UciOptionSchema};

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

/// Schema-bound tune vector ready for later audit, schedule derivation and
/// game-driver use cases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpsaBoundTune {
    pub parameters: Vec<SpsaBoundParameter>,
}

impl SpsaTune {
    /// Bind every ordered entry to the schema observed from this ordinary UCI
    /// executable. Range, duplicate and rounding-resolution policy is
    /// intentionally added by the dedicated Phase-5.6 audit.
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
