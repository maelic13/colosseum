//! Engine rating. v1 uses classic incremental Elo behind the [`Rating`] trait so a
//! future regression-based rating (Ordo/Bayeselo, with error bars) can drop in.

use std::collections::HashMap;

use crate::{game::GameResult, ids::EngineId};

/// A rating change for a single engine produced by one game.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RatingDelta {
    pub engine: EngineId,
    pub delta: f64,
}

/// A pluggable rating model.
pub trait Rating {
    /// Apply one game's result and return the per-engine rating deltas.
    fn update(&mut self, white: EngineId, black: EngineId, result: GameResult) -> Vec<RatingDelta>;
    /// Current rating for an engine.
    fn current(&self, engine: EngineId) -> f64;
    /// Change in rating since this engine's baseline (tournament start). Drives the
    /// "Elo Δ" column.
    fn delta_since_start(&self, engine: EngineId) -> f64;
}

/// Classic incremental Elo with a fixed K-factor.
#[derive(Debug, Clone)]
pub struct IncrementalElo {
    k: f64,
    baseline: HashMap<EngineId, f64>,
    current: HashMap<EngineId, f64>,
}

impl IncrementalElo {
    /// Create an Elo model with the given K-factor and no seeded ratings.
    #[must_use]
    pub fn new(k: f64) -> Self {
        Self {
            k,
            baseline: HashMap::new(),
            current: HashMap::new(),
        }
    }

    /// Create an Elo model seeded with starting ratings (used as the Δ baseline).
    pub fn with_seed(k: f64, seeds: impl IntoIterator<Item = (EngineId, f64)>) -> Self {
        let mut model = Self::new(k);
        for (id, elo) in seeds {
            model.baseline.insert(id, elo);
            model.current.insert(id, elo);
        }
        model
    }

    /// Expected score for player A against player B under the logistic Elo model.
    fn expected(ra: f64, rb: f64) -> f64 {
        1.0 / (1.0 + 10f64.powf((rb - ra) / 400.0))
    }
}

impl Rating for IncrementalElo {
    fn update(&mut self, white: EngineId, black: EngineId, result: GameResult) -> Vec<RatingDelta> {
        let ra = *self.current.entry(white).or_insert(0.0);
        let rb = *self.current.entry(black).or_insert(0.0);
        self.baseline.entry(white).or_insert(ra);
        self.baseline.entry(black).or_insert(rb);

        let expected_white = Self::expected(ra, rb);
        let delta = self.k * (result.white_score() - expected_white);

        self.current.insert(white, ra + delta);
        self.current.insert(black, rb - delta);

        vec![
            RatingDelta {
                engine: white,
                delta,
            },
            RatingDelta {
                engine: black,
                delta: -delta,
            },
        ]
    }

    fn current(&self, engine: EngineId) -> f64 {
        self.current.get(&engine).copied().unwrap_or(0.0)
    }

    fn delta_since_start(&self, engine: EngineId) -> f64 {
        self.current(engine) - self.baseline.get(&engine).copied().unwrap_or(0.0)
    }
}
