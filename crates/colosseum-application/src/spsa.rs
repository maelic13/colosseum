//! Runtime-neutral SPSA preflight policy.

use colosseum_core::{SpsaError, SpsaScheduleArtifact};
use thiserror::Error;

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
}
