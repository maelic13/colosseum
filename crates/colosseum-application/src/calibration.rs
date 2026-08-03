//! Runtime-neutral contract for an optional identical-binary calibration.

use colosseum_core::FixedNAchievedResolution;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_CALIBRATION_GAMES: u32 = 30_000;
pub const DEFAULT_CALIBRATION_CONFIDENCE: f64 = 0.95;
pub const DEFAULT_CALIBRATION_TOLERANCE_NELO: f64 = 5.0;

/// Fixed-sample conditions for a machine-specific symmetry observation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CalibrationDesign {
    /// Complete games, always even so the achieved interval has only full
    /// colour-reversed pairs.
    pub games: u32,
    /// Two-sided interval confidence, expressed as a probability.
    pub confidence: f64,
    /// Symmetric normalized-Elo containment tolerance.
    pub tolerance_nelo: f64,
}

impl Default for CalibrationDesign {
    fn default() -> Self {
        Self {
            games: DEFAULT_CALIBRATION_GAMES,
            confidence: DEFAULT_CALIBRATION_CONFIDENCE,
            tolerance_nelo: DEFAULT_CALIBRATION_TOLERANCE_NELO,
        }
    }
}

impl CalibrationDesign {
    pub fn new(games: u32, confidence: f64, tolerance_nelo: f64) -> Result<Self, CalibrationError> {
        if games < 2 || !games.is_multiple_of(2) {
            return Err(CalibrationError::GamesMustBePositiveAndEven { games });
        }
        if !confidence.is_finite() || !(0.0 < confidence && confidence < 1.0) {
            return Err(CalibrationError::InvalidConfidence { confidence });
        }
        if !tolerance_nelo.is_finite() || tolerance_nelo <= 0.0 {
            return Err(CalibrationError::InvalidTolerance { tolerance_nelo });
        }
        Ok(Self {
            games,
            confidence,
            tolerance_nelo,
        })
    }

    #[must_use]
    pub const fn pairs(self) -> u32 {
        self.games / 2
    }

    #[must_use]
    pub fn significance(self) -> f64 {
        1.0 - self.confidence
    }
}

/// Content identities collected by the outer filesystem adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalibrationBinaries {
    pub engine_a_sha256: String,
    pub engine_b_sha256: String,
}

impl CalibrationBinaries {
    pub fn new(
        engine_a_sha256: impl Into<String>,
        engine_b_sha256: impl Into<String>,
    ) -> Result<Self, CalibrationError> {
        let binaries = Self {
            engine_a_sha256: engine_a_sha256.into(),
            engine_b_sha256: engine_b_sha256.into(),
        };
        if !is_sha256_hex(&binaries.engine_a_sha256) || !is_sha256_hex(&binaries.engine_b_sha256) {
            return Err(CalibrationError::InvalidSha256);
        }
        if binaries.engine_a_sha256 != binaries.engine_b_sha256 {
            return Err(CalibrationError::BinaryHashMismatch {
                engine_a_sha256: binaries.engine_a_sha256,
                engine_b_sha256: binaries.engine_b_sha256,
            });
        }
        Ok(binaries)
    }
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// The only conclusions an optional symmetry calibration may make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CalibrationStatus {
    Pass,
    Fail,
    Inconclusive,
    Invalid,
}

/// The normalized-Elo interval evaluated by a calibration decision.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CalibrationInterval {
    pub confidence: f64,
    pub estimate_nelo: f64,
    pub lower_nelo: f64,
    pub upper_nelo: f64,
}

impl From<FixedNAchievedResolution> for CalibrationInterval {
    fn from(resolution: FixedNAchievedResolution) -> Self {
        Self {
            confidence: resolution.confidence,
            estimate_nelo: resolution.estimate,
            lower_nelo: resolution.lower,
            upper_nelo: resolution.upper,
        }
    }
}

/// Classify a completed calibration without applying a post-hoc statistical
/// threshold. Engine faults always invalidate the result; a degenerate sample
/// has no interval and remains inconclusive.
#[must_use]
pub fn classify_calibration(
    design: CalibrationDesign,
    interval: Option<CalibrationInterval>,
    engine_faults: u32,
) -> CalibrationStatus {
    if engine_faults > 0 {
        return CalibrationStatus::Invalid;
    }
    let Some(interval) = interval else {
        return CalibrationStatus::Inconclusive;
    };
    if interval.lower_nelo >= -design.tolerance_nelo && interval.upper_nelo <= design.tolerance_nelo
    {
        CalibrationStatus::Pass
    } else if interval.lower_nelo > design.tolerance_nelo
        || interval.upper_nelo < -design.tolerance_nelo
    {
        CalibrationStatus::Fail
    } else {
        CalibrationStatus::Inconclusive
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum CalibrationError {
    #[error("calibration --games must be positive and even; got {games}")]
    GamesMustBePositiveAndEven { games: u32 },
    #[error(
        "calibration confidence must be finite and strictly between zero and one; got {confidence}"
    )]
    InvalidConfidence { confidence: f64 },
    #[error("calibration tolerance must be finite and greater than zero; got {tolerance_nelo}")]
    InvalidTolerance { tolerance_nelo: f64 },
    #[error("calibration executable digests must be 64-character SHA-256 hex strings")]
    InvalidSha256,
    #[error(
        "calibration requires byte-identical executables: A={engine_a_sha256}, B={engine_b_sha256}"
    )]
    BinaryHashMismatch {
        engine_a_sha256: String,
        engine_b_sha256: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn defaults_are_the_optional_30k_game_95_percent_plus_minus_5_design() {
        let design = CalibrationDesign::default();
        assert_eq!(design.games, 30_000);
        assert_eq!(design.pairs(), 15_000);
        assert_eq!(design.confidence, 0.95);
        assert_eq!(design.tolerance_nelo, 5.0);
        assert!((design.significance() - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn calibration_design_requires_full_pairs_and_finite_probabilities() {
        assert!(matches!(
            CalibrationDesign::new(3, 0.95, 5.0),
            Err(CalibrationError::GamesMustBePositiveAndEven { games: 3 })
        ));
        assert!(matches!(
            CalibrationDesign::new(4, 1.0, 5.0),
            Err(CalibrationError::InvalidConfidence { .. })
        ));
        assert!(matches!(
            CalibrationDesign::new(4, 0.95, 0.0),
            Err(CalibrationError::InvalidTolerance { .. })
        ));
    }

    #[test]
    fn calibration_requires_equal_full_sha256_identities() {
        assert!(CalibrationBinaries::new(HASH_A, HASH_A).is_ok());
        assert!(matches!(
            CalibrationBinaries::new(HASH_A, HASH_B),
            Err(CalibrationError::BinaryHashMismatch { .. })
        ));
        assert!(matches!(
            CalibrationBinaries::new("short", "short"),
            Err(CalibrationError::InvalidSha256)
        ));
    }

    #[test]
    fn calibration_classification_requires_whole_interval_containment() {
        let design = CalibrationDesign::new(4, 0.95, 5.0).unwrap();
        let interval = |lower, upper| CalibrationInterval {
            confidence: 0.95,
            estimate_nelo: (lower + upper) / 2.0,
            lower_nelo: lower,
            upper_nelo: upper,
        };
        assert_eq!(
            classify_calibration(design, Some(interval(-4.0, 4.0)), 0),
            CalibrationStatus::Pass
        );
        assert_eq!(
            classify_calibration(design, Some(interval(5.1, 8.0)), 0),
            CalibrationStatus::Fail
        );
        assert_eq!(
            classify_calibration(design, Some(interval(-8.0, -5.1)), 0),
            CalibrationStatus::Fail
        );
        assert_eq!(
            classify_calibration(design, Some(interval(-2.0, 6.0)), 0),
            CalibrationStatus::Inconclusive
        );
        assert_eq!(
            classify_calibration(design, None, 1),
            CalibrationStatus::Invalid
        );
    }
}
