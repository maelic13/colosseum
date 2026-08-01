//! Runtime-neutral design contract for capped sequential experiments.

use colosseum_core::{EloModel, StatisticsError, sprt_wald_bounds};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SprtBundle {
    Gainer,
    Simplify,
}

impl SprtBundle {
    #[must_use]
    pub const fn defaults(self) -> SprtParameters {
        match self {
            Self::Gainer => SprtParameters {
                model: EloModel::Normalized,
                elo0: 0.0,
                elo1: 5.0,
                alpha: 0.05,
                beta: 0.05,
            },
            Self::Simplify => SprtParameters {
                model: EloModel::Normalized,
                elo0: -5.0,
                elo1: 0.0,
                alpha: 0.05,
                beta: 0.05,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SprtParameters {
    pub model: EloModel,
    pub elo0: f64,
    pub elo1: f64,
    pub alpha: f64,
    pub beta: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SprtDesign {
    pub parameters: SprtParameters,
    pub max_pairs: u32,
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub bundle: Option<SprtBundle>,
}

impl SprtDesign {
    pub fn new(
        parameters: SprtParameters,
        max_pairs: u32,
        bundle: Option<SprtBundle>,
    ) -> Result<Self, SprtDesignError> {
        if max_pairs == 0 {
            return Err(SprtDesignError::ZeroPairCap);
        }
        let (lower_bound, upper_bound) = sprt_wald_bounds(
            parameters.elo0,
            parameters.elo1,
            parameters.alpha,
            parameters.beta,
        )?;
        Ok(Self {
            parameters,
            max_pairs,
            lower_bound,
            upper_bound,
            bundle,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum SprtDesignError {
    #[error("SPRT max-pairs must be at least one")]
    ZeroPairCap,
    #[error(transparent)]
    Statistics(#[from] StatisticsError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_bundles_expand_to_documented_normalized_designs() {
        assert_eq!(
            SprtBundle::Gainer.defaults(),
            SprtParameters {
                model: EloModel::Normalized,
                elo0: 0.0,
                elo1: 5.0,
                alpha: 0.05,
                beta: 0.05,
            }
        );
        assert_eq!(SprtBundle::Simplify.defaults().elo0, -5.0);
        assert_eq!(SprtBundle::Simplify.defaults().elo1, 0.0);
    }

    #[test]
    fn design_requires_a_finite_cap_and_valid_explicit_statistics() {
        assert!(matches!(
            SprtDesign::new(SprtBundle::Gainer.defaults(), 0, None),
            Err(SprtDesignError::ZeroPairCap)
        ));
        let mut invalid = SprtBundle::Gainer.defaults();
        invalid.elo1 = invalid.elo0;
        assert!(matches!(
            SprtDesign::new(invalid, 1, None),
            Err(SprtDesignError::Statistics(
                StatisticsError::InvalidHypotheses { .. }
            ))
        ));
    }
}
