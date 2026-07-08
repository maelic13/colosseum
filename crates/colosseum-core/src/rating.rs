//! Engine rating: Ordo-style joint maximum-likelihood ratings recomputed from
//! the full set of game results ([`ml_ratings`]), single-engine performance
//! ratings against fixed opponents ([`performance_rating`]), and asymptotic
//! error bars for the ML estimates ([`rating_error`]). Recomputing from
//! standings is order-independent and has no tuning knob, unlike incremental
//! K-factor Elo — the same games always produce the same ratings.

use std::collections::HashMap;

use crate::{ids::EngineId, standings::Standings};

/// Expected score for player A against player B under the logistic Elo model.
fn expected(ra: f64, rb: f64) -> f64 {
    1.0 / (1.0 + 10f64.powf((rb - ra) / 400.0))
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
            .map(|&(opp, _, games)| f64::from(games) * expected(r, opp))
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

/// Weight of the Bayesian prior in [`ml_ratings`]: every engine carries this
/// many *virtual draws against its own prior rating*. A tiny sample can no
/// longer swing wildly (one real win against 3 virtual draws is a modest
/// bump, not a capped +800 performance), while after a few dozen real games
/// the prior's influence fades below the rating error. The same idea as
/// Ordo's prior weight.
const PRIOR_WEIGHT: u32 = 6;

/// Joint maximum-likelihood ratings for a whole tournament (Ordo-style).
///
/// Iteratively sets every engine's rating to its [`performance_rating`]
/// against the others' current ratings (synchronous update with damping),
/// then re-centres so the mean rating of engines *that played* equals the
/// mean of their priors — ML determines only rating differences, so the
/// anchor keeps the numbers on the library's scale. Each engine also plays
/// [`PRIOR_WEIGHT`] virtual draws against its own prior, so early estimates
/// are damped toward the prior instead of jumping to capped extremes.
/// Engines without games keep their prior untouched. Order-independent and
/// K-free: the same set of games always yields the same ratings, regardless
/// of play order.
#[must_use]
pub fn ml_ratings(standings: &Standings, priors: &[(EngineId, f64)]) -> HashMap<EngineId, f64> {
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
            let mut results: Vec<(f64, f64, u32)> = played
                .iter()
                .filter(|&&opp| opp != id)
                .map(|&opp| {
                    let h2h = standings.head_to_head(id, opp);
                    (ratings[&opp], h2h.points(), h2h.games())
                })
                .collect();
            // The Bayesian prior: virtual draws against the engine's own
            // prior rating (not its current estimate — the prior is the
            // anchor, otherwise it would drift with the iteration).
            let prior = priors
                .iter()
                .find(|(pid, _)| *pid == id)
                .map_or(1500.0, |(_, p)| *p);
            results.push((prior, f64::from(PRIOR_WEIGHT) * 0.5, PRIOR_WEIGHT));
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

/// [`ml_ratings`] with an *anchor set*: only `updatable` engines move; every
/// other participant stays pinned at its prior — visually and inside the
/// computation, exactly like rating a newcomer against an established, fixed
/// field. Updatable engines are estimated jointly against the mixture of
/// anchored opponents and each other (with the same prior damping); no
/// re-centring is applied — the anchors pin the scale.
#[must_use]
pub fn ml_ratings_anchored(
    standings: &Standings,
    priors: &[(EngineId, f64)],
    updatable: &[EngineId],
) -> HashMap<EngineId, f64> {
    let mut ratings: HashMap<EngineId, f64> = priors.iter().copied().collect();
    let moving: Vec<EngineId> = priors
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| updatable.contains(id) && standings.standing(*id).games() > 0)
        .collect();
    if moving.is_empty() {
        return ratings;
    }
    let all: Vec<EngineId> = priors.iter().map(|(id, _)| *id).collect();

    for _ in 0..200 {
        let mut next = ratings.clone();
        let mut max_change: f64 = 0.0;
        for &id in &moving {
            let mut results: Vec<(f64, f64, u32)> = all
                .iter()
                .filter(|&&opp| opp != id)
                .map(|&opp| {
                    let h2h = standings.head_to_head(id, opp);
                    (ratings[&opp], h2h.points(), h2h.games())
                })
                .collect();
            let prior = priors
                .iter()
                .find(|(pid, _)| *pid == id)
                .map_or(1500.0, |(_, p)| *p);
            results.push((prior, f64::from(PRIOR_WEIGHT) * 0.5, PRIOR_WEIGHT));
            if let Some(perf) = performance_rating(&results) {
                let old = ratings[&id];
                let new = 0.5 * old + 0.5 * perf;
                max_change = max_change.max((new - old).abs());
                next.insert(id, new);
            }
        }
        ratings = next;
        if max_change < 0.01 {
            break;
        }
    }
    ratings
}

/// Asymptotic 95%-confidence half-width (± Elo) of an engine's ML rating.
///
/// The observed Fisher information of a rating under the logistic Elo model is
/// `I = Σ_games e·(1−e)·(ln10/400)²`, where `e` is the expected score against
/// that game's opponent at the given ratings; the standard error is `1/√I` and
/// the 95% interval is `±1.96·SE`. Draws carry the same information as decisive
/// games here (a slight underestimate of certainty in drawish play — acceptable
/// for a live read; use the two-engine [`crate::elo_with_error`] card for
/// match-precision intervals).
///
/// Returns `None` when the engine has no games, or when every game carries no
/// information (expected score pinned at 0/1 — e.g. an 800-point cap).
#[must_use]
pub fn rating_error(
    standings: &Standings,
    ratings: &HashMap<EngineId, f64>,
    engine: EngineId,
) -> Option<f64> {
    const SCALE: f64 = std::f64::consts::LN_10 / 400.0;
    let r = *ratings.get(&engine)?;
    let mut information = 0.0f64;
    for (&opp, &opp_r) in ratings {
        if opp == engine {
            continue;
        }
        let games = standings.head_to_head(engine, opp).games();
        if games == 0 {
            continue;
        }
        let e = expected(r, opp_r);
        information += f64::from(games) * e * (1.0 - e) * SCALE * SCALE;
    }
    if information <= 0.0 {
        return None;
    }
    Some(1.96 / information.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{GameResult, Termination};
    use crate::standings::GameOutcome;

    const EPS: f64 = 1e-9;

    #[test]
    fn expected_is_symmetric_and_centered() {
        assert!((expected(1500.0, 1500.0) - 0.5).abs() < EPS);
        let ea = expected(1600.0, 1400.0);
        let eb = expected(1400.0, 1600.0);
        assert!((ea + eb - 1.0).abs() < EPS);
        assert!(ea > 0.5 && eb < 0.5);
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
            white_depth: None,
            black_depth: None,
            white_move_ms: None,
            black_move_ms: None,
        }
    }

    #[test]
    fn ml_even_score_converges_ratings_and_keeps_anchor() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut s = Standings::with_engines(&[a, b]);
        s.record(game(a, b, GameResult::WhiteWin));
        s.record(game(b, a, GameResult::WhiteWin));
        // 50% each pulls the ratings together; the prior keeps them from
        // collapsing entirely on two games. Mean preserved at 1600.
        let r = ml_ratings(&s, &[(a, 1500.0), (b, 1700.0)]);
        assert!(r[&a] > 1500.0 && r[&a] < 1600.0, "a={}", r[&a]);
        assert!(r[&b] < 1700.0 && r[&b] > 1600.0, "b={}", r[&b]);
        assert!(((r[&a] + r[&b]) / 2.0 - 1600.0).abs() < 0.5);
    }

    #[test]
    fn ml_single_game_is_damped() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut s = Standings::with_engines(&[a, b]);
        s.record(game(a, b, GameResult::WhiteWin));
        // One win must NOT produce a capped ±400 split — the prior draws
        // keep it a modest bump (~±45).
        let r = ml_ratings(&s, &[(a, 1500.0), (b, 1500.0)]);
        let diff = r[&a] - r[&b];
        assert!(diff > 10.0 && diff < 120.0, "diff={diff}");
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
    fn ml_75_percent_approaches_logistic_gap_with_volume() {
        let a = EngineId::new();
        let b = EngineId::new();
        // 75% over 10 games: noticeably damped by the prior.
        let mut small = Standings::with_engines(&[a, b]);
        for _ in 0..7 {
            small.record(game(a, b, GameResult::WhiteWin));
        }
        small.record(game(a, b, GameResult::Draw));
        small.record(game(b, a, GameResult::WhiteWin));
        small.record(game(b, a, GameResult::WhiteWin));
        let r = ml_ratings(&small, &[(a, 1500.0), (b, 1500.0)]);
        let small_diff = r[&a] - r[&b];
        assert!(
            small_diff > 90.0 && small_diff < 190.0,
            "small={small_diff}"
        );
        assert!(((r[&a] + r[&b]) / 2.0 - 1500.0).abs() < 0.5);

        // 75% over 400 games: the prior washes out; the pure logistic gap
        // for a 75% score is 190.8.
        let mut large = Standings::with_engines(&[a, b]);
        for _ in 0..280 {
            large.record(game(a, b, GameResult::WhiteWin));
        }
        for _ in 0..40 {
            large.record(game(a, b, GameResult::Draw));
        }
        for _ in 0..80 {
            large.record(game(b, a, GameResult::WhiteWin));
        }
        let r = ml_ratings(&large, &[(a, 1500.0), (b, 1500.0)]);
        let large_diff = r[&a] - r[&b];
        assert!((large_diff - 190.8).abs() < 8.0, "large={large_diff}");
        assert!(large_diff > small_diff);
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
        // a and b drew: pulled toward each other (one draw against six
        // prior draws only nudges them), mean preserved at 1550.
        assert!(r[&a] > 1500.0 && r[&b] < 1600.0 && r[&a] < r[&b]);
        assert!(((r[&a] + r[&b]) / 2.0 - 1550.0).abs() < 0.5);
    }

    #[test]
    fn anchored_ml_moves_only_the_updatable_engine() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut s = Standings::with_engines(&[a, b]);
        // a crushes b 8-0-2 (80%).
        for _ in 0..8 {
            s.record(game(a, b, GameResult::WhiteWin));
        }
        s.record(game(b, a, GameResult::WhiteWin));
        s.record(game(b, a, GameResult::WhiteWin));
        let priors = [(a, 1500.0), (b, 1500.0)];
        let r = ml_ratings_anchored(&s, &priors, &[a]);
        // b is anchored, exactly at its prior.
        assert!((r[&b] - 1500.0).abs() < EPS, "b={}", r[&b]);
        // a moved up against the fixed opponent (damped by the prior draws).
        assert!(r[&a] > 1550.0 && r[&a] < 1500.0 + 260.0, "a={}", r[&a]);
    }

    #[test]
    fn rating_error_shrinks_with_more_games() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut few = Standings::with_engines(&[a, b]);
        let mut many = Standings::with_engines(&[a, b]);
        for i in 0..40 {
            let g = game(a, b, GameResult::Draw);
            if i < 10 {
                few.record(g);
            }
            many.record(g);
        }
        let ratings: HashMap<EngineId, f64> = [(a, 1500.0), (b, 1500.0)].into();
        let e_few = rating_error(&few, &ratings, a).unwrap();
        let e_many = rating_error(&many, &ratings, a).unwrap();
        // 4× the games → half the standard error.
        assert!((e_few / e_many - 2.0).abs() < 0.01, "{e_few} vs {e_many}");
        // Sanity scale: 10 games between equals → SE ≈ 110 Elo, ±215 at 95%.
        assert!((e_few - 215.3).abs() < 1.0, "got {e_few}");
    }

    #[test]
    fn rating_error_none_without_games() {
        let a = EngineId::new();
        let b = EngineId::new();
        let s = Standings::with_engines(&[a, b]);
        let ratings: HashMap<EngineId, f64> = [(a, 1500.0), (b, 1500.0)].into();
        assert_eq!(rating_error(&s, &ratings, a), None);
        assert_eq!(rating_error(&s, &ratings, EngineId::new()), None);
    }
}
