//! Engine rating. v1 uses classic incremental Elo behind the [`Rating`] trait so a
//! future regression-based rating (Ordo/Bayeselo, with error bars) can drop in.

use std::collections::HashMap;

use crate::{game::GameResult, ids::EngineId, standings::Standings};

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
    pub(crate) fn expected(ra: f64, rb: f64) -> f64 {
        1.0 / (1.0 + 10f64.powf((rb - ra) / 400.0))
    }
}

/// Performance rating: the rating at which the expected score against the
/// given (fixed) opponent ratings equals the achieved score. Used to estimate
/// a single engine's strength without touching its opponents' ratings.
///
/// `results` holds one `(opponent_rating, points_scored, games)` entry per
/// opponent. Returns `None` when no games were played. A 0% or 100% score has
/// no finite solution and is capped 800 points below the weakest / above the
/// strongest opponent faced, matching common practice.
#[must_use]
pub fn performance_rating(results: &[(f64, f64, u32)]) -> Option<f64> {
    let total_games: u32 = results.iter().map(|r| r.2).sum();
    if total_games == 0 {
        return None;
    }
    let total_points: f64 = results.iter().map(|r| r.1).sum();
    let played = || results.iter().filter(|r| r.2 > 0).map(|r| r.0);
    let min_opp = played().fold(f64::INFINITY, f64::min);
    let max_opp = played().fold(f64::NEG_INFINITY, f64::max);
    if total_points <= 0.0 {
        return Some(min_opp - 800.0);
    }
    if total_points >= f64::from(total_games) {
        return Some(max_opp + 800.0);
    }

    // Expected total score is strictly increasing in the candidate rating, so
    // the unique solution can be bisected.
    let expected_total = |r: f64| -> f64 {
        results
            .iter()
            .map(|&(opp, _, games)| f64::from(games) * IncrementalElo::expected(r, opp))
            .sum()
    };
    let mut lo = min_opp - 1000.0;
    let mut hi = max_opp + 1000.0;
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if expected_total(mid) < total_points {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Some(0.5 * (lo + hi))
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

/// Joint maximum-likelihood ratings for a whole tournament (Ordo-style).
///
/// Iteratively sets every engine's rating to its [`performance_rating`]
/// against the others' current ratings (synchronous update with damping),
/// then re-centres so the mean rating of engines *that played* equals the
/// mean of their priors — ML determines only rating differences, so the
/// anchor keeps the numbers on the library's scale. Engines without games
/// keep their prior untouched. Order-independent and K-free, unlike
/// [`IncrementalElo`].
#[must_use]
pub fn ml_ratings(
    standings: &Standings,
    priors: &[(EngineId, f64)],
) -> HashMap<EngineId, f64> {
    let mut ratings: HashMap<EngineId, f64> = priors.iter().copied().collect();
    let played: Vec<EngineId> = priors
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| standings.standing(*id).games() > 0)
        .collect();
    if played.len() < 2 {
        return ratings;
    }
    let prior_mean: f64 = priors
        .iter()
        .filter(|(id, _)| played.contains(id))
        .map(|(_, r)| r)
        .sum::<f64>()
        / played.len() as f64;

    for _ in 0..200 {
        // Synchronous step: every performance rating is computed against the
        // previous iteration's ratings, damped 50/50 for stable convergence.
        let mut next = ratings.clone();
        let mut max_change: f64 = 0.0;
        for &id in &played {
            let results: Vec<(f64, f64, u32)> = played
                .iter()
                .filter(|&&opp| opp != id)
                .map(|&opp| {
                    let h2h = standings.head_to_head(id, opp);
                    (ratings[&opp], h2h.points(), h2h.games())
                })
                .collect();
            if let Some(perf) = performance_rating(&results) {
                let old = ratings[&id];
                let new = 0.5 * old + 0.5 * perf;
                max_change = max_change.max((new - old).abs());
                next.insert(id, new);
            }
        }
        // Re-centre onto the prior mean.
        let mean: f64 = played.iter().map(|id| next[id]).sum::<f64>() / played.len() as f64;
        for id in &played {
            *next.get_mut(id).unwrap() += prior_mean - mean;
        }
        ratings = next;
        if max_change < 0.01 {
            break;
        }
    }
    ratings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Termination;
    use crate::standings::GameOutcome;

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
    fn performance_rating_matches_even_score() {
        // 50% against a single 1500-rated opponent -> exactly 1500.
        let r = performance_rating(&[(1500.0, 2.0, 4)]).unwrap();
        assert!((r - 1500.0).abs() < 0.01, "got {r}");
    }

    #[test]
    fn performance_rating_beats_stronger_field() {
        // 75% against 1500s is ~+191 Elo (logistic model).
        let r = performance_rating(&[(1500.0, 7.5, 10)]).unwrap();
        assert!((r - 1690.8).abs() < 1.0, "got {r}");
        // Symmetric: 25% is ~-191.
        let r = performance_rating(&[(1500.0, 2.5, 10)]).unwrap();
        assert!((r - 1309.2).abs() < 1.0, "got {r}");
    }

    #[test]
    fn performance_rating_caps_perfect_and_zero_scores() {
        assert_eq!(
            performance_rating(&[(1500.0, 4.0, 4), (1700.0, 2.0, 2)]),
            Some(2500.0)
        );
        assert_eq!(
            performance_rating(&[(1500.0, 0.0, 4), (1300.0, 0.0, 2)]),
            Some(500.0)
        );
        assert_eq!(performance_rating(&[]), None);
        assert_eq!(performance_rating(&[(1500.0, 0.0, 0)]), None);
    }

    #[test]
    fn performance_rating_weights_mixed_opposition() {
        // Even score against a mixed field lands between the opponents.
        let r = performance_rating(&[(1400.0, 1.0, 2), (1600.0, 1.0, 2)]).unwrap();
        assert!((r - 1500.0).abs() < 2.0, "got {r}");
    }

    fn game(w: EngineId, b: EngineId, result: GameResult) -> GameOutcome {
        GameOutcome {
            white: w,
            black: b,
            result,
            termination: Termination::Checkmate,
            white_nps: None,
            black_nps: None,
        }
    }

    #[test]
    fn ml_even_score_equalizes_ratings_and_keeps_anchor() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut s = Standings::with_engines(&[a, b]);
        s.record(game(a, b, GameResult::WhiteWin));
        s.record(game(b, a, GameResult::WhiteWin));
        // 50% each: equal ratings, mean preserved at (1500+1700)/2 = 1600.
        let r = ml_ratings(&s, &[(a, 1500.0), (b, 1700.0)]);
        assert!((r[&a] - 1600.0).abs() < 1.0, "a={}", r[&a]);
        assert!((r[&b] - 1600.0).abs() < 1.0, "b={}", r[&b]);
    }

    #[test]
    fn ml_orders_by_strength_and_preserves_mean() {
        let a = EngineId::new();
        let b = EngineId::new();
        let c = EngineId::new();
        let mut s = Standings::with_engines(&[a, b, c]);
        // a beats b and c twice; b beats c twice.
        for _ in 0..2 {
            s.record(game(a, b, GameResult::WhiteWin));
            s.record(game(a, c, GameResult::WhiteWin));
            s.record(game(b, c, GameResult::WhiteWin));
        }
        let priors = [(a, 1500.0), (b, 1500.0), (c, 1500.0)];
        let r = ml_ratings(&s, &priors);
        assert!(r[&a] > r[&b] && r[&b] > r[&c]);
        let mean = (r[&a] + r[&b] + r[&c]) / 3.0;
        assert!((mean - 1500.0).abs() < 1.0, "mean={mean}");
    }

    #[test]
    fn ml_75_percent_is_about_191_apart() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut s = Standings::with_engines(&[a, b]);
        // a scores 7.5/10 against b: 7 wins, 1 draw, 2 losses.
        for _ in 0..7 {
            s.record(game(a, b, GameResult::WhiteWin));
        }
        s.record(game(a, b, GameResult::Draw));
        s.record(game(b, a, GameResult::WhiteWin));
        s.record(game(b, a, GameResult::WhiteWin));
        let r = ml_ratings(&s, &[(a, 1500.0), (b, 1500.0)]);
        let diff = r[&a] - r[&b];
        assert!((diff - 190.8).abs() < 3.0, "diff={diff}");
        assert!(((r[&a] + r[&b]) / 2.0 - 1500.0).abs() < 0.5);
    }

    #[test]
    fn ml_engine_without_games_keeps_prior() {
        let a = EngineId::new();
        let b = EngineId::new();
        let c = EngineId::new();
        let mut s = Standings::with_engines(&[a, b, c]);
        s.record(game(a, b, GameResult::Draw));
        let r = ml_ratings(&s, &[(a, 1500.0), (b, 1600.0), (c, 2400.0)]);
        assert!((r[&c] - 2400.0).abs() < EPS);
        // a and b drew: equal ratings at their prior mean 1550.
        assert!((r[&a] - r[&b]).abs() < 1.0);
        assert!(((r[&a] + r[&b]) / 2.0 - 1550.0).abs() < 0.5);
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
