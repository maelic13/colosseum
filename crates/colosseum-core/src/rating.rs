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

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    #[test]
    fn expected_is_symmetric_and_centered() {
        assert!((IncrementalElo::expected(1500.0, 1500.0) - 0.5).abs() < EPS);
        let ea = IncrementalElo::expected(1600.0, 1400.0);
        let eb = IncrementalElo::expected(1400.0, 1600.0);
        assert!((ea + eb - 1.0).abs() < EPS);
        assert!(ea > 0.5 && eb < 0.5);
    }

    #[test]
    fn draw_between_equals_is_no_change() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut elo = IncrementalElo::with_seed(32.0, [(a, 1500.0), (b, 1500.0)]);
        let deltas = elo.update(a, b, GameResult::Draw);
        for d in deltas {
            assert!(d.delta.abs() < EPS);
        }
        assert!((elo.current(a) - 1500.0).abs() < EPS);
        assert!((elo.current(b) - 1500.0).abs() < EPS);
    }

    #[test]
    fn win_is_zero_sum_and_signed_correctly() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut elo = IncrementalElo::with_seed(32.0, [(a, 1500.0), (b, 1500.0)]);
        let deltas = elo.update(a, b, GameResult::WhiteWin);
        // Equal ratings, expected 0.5, K=32 -> winner +16, loser -16.
        let da = deltas.iter().find(|d| d.engine == a).unwrap().delta;
        let db = deltas.iter().find(|d| d.engine == b).unwrap().delta;
        assert!((da - 16.0).abs() < EPS);
        assert!((db + 16.0).abs() < EPS);
        assert!((da + db).abs() < EPS); // zero-sum
        assert!((elo.delta_since_start(a) - 16.0).abs() < EPS);
        assert!((elo.delta_since_start(b) + 16.0).abs() < EPS);
    }

    #[test]
    fn unseeded_engines_default_to_zero_baseline() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut elo = IncrementalElo::new(32.0);
        elo.update(a, b, GameResult::WhiteWin);
        // Baseline captured at first sighting (0.0), so delta reflects the full change.
        assert!((elo.delta_since_start(a) - elo.current(a)).abs() < EPS);
    }
}
