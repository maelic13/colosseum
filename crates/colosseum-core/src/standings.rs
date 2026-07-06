//! Live standings aggregation: per-engine W-D-L + points, a head-to-head matrix, and
//! average nps. Ratings (Elo / Elo Δ) live in [`crate::rating`] and are joined in the
//! GUI; engine *names* are joined from the engine library. This type only aggregates
//! game outcomes, so it can be updated incrementally after every finished game.

use std::cmp::Ordering;
use std::collections::HashMap;

use crate::{
    game::{GameResult, Termination},
    ids::EngineId,
};

/// One engine's record against one specific opponent (from this engine's view).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HeadToHead {
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
}

impl HeadToHead {
    #[must_use]
    pub fn games(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    #[must_use]
    pub fn points(&self) -> f64 {
        f64::from(self.wins) + 0.5 * f64::from(self.draws)
    }
}

/// One engine's overall record plus nps and search-depth accumulation.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EngineStanding {
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    /// Losses by running out of time.
    pub time_losses: u32,
    /// Losses by crashing or playing an illegal move.
    pub crash_losses: u32,
    nps_total: u128,
    nps_samples: u64,
    depth_total: f64,
    depth_samples: u64,
    move_ms_total: f64,
    move_ms_samples: u64,
}

impl EngineStanding {
    #[must_use]
    pub fn games(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    /// Tournament points (1 per win, 0.5 per draw).
    #[must_use]
    pub fn points(&self) -> f64 {
        f64::from(self.wins) + 0.5 * f64::from(self.draws)
    }

    /// Average nps across sampled games, if any were sampled.
    #[must_use]
    pub fn avg_nps(&self) -> Option<u64> {
        if self.nps_samples == 0 {
            None
        } else {
            Some((self.nps_total / u128::from(self.nps_samples)) as u64)
        }
    }

    /// Average search depth across sampled games, if any were sampled.
    #[must_use]
    pub fn avg_depth(&self) -> Option<f64> {
        if self.depth_samples == 0 {
            None
        } else {
            Some(self.depth_total / self.depth_samples as f64)
        }
    }

    /// Average wall-clock milliseconds per move across sampled games.
    #[must_use]
    pub fn avg_move_ms(&self) -> Option<f64> {
        if self.move_ms_samples == 0 {
            None
        } else {
            Some(self.move_ms_total / self.move_ms_samples as f64)
        }
    }
}

/// A finished game's outcome, fed into [`Standings::record`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GameOutcome {
    pub white: EngineId,
    pub black: EngineId,
    pub result: GameResult,
    pub termination: Termination,
    pub white_nps: Option<u64>,
    pub black_nps: Option<u64>,
    /// Mean search depth over each engine's moves in this game.
    pub white_depth: Option<f64>,
    pub black_depth: Option<f64>,
    /// Mean wall-clock milliseconds per move for each engine in this game.
    pub white_move_ms: Option<f64>,
    pub black_move_ms: Option<f64>,
}

/// One game's result from a specific engine's perspective, in played order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairGameResult {
    Win,
    Draw,
    Loss,
}

/// Aggregated standings for a tournament.
#[derive(Debug, Clone, Default)]
pub struct Standings {
    per_engine: HashMap<EngineId, EngineStanding>,
    /// Keyed by `(engine, opponent)`; record is from `engine`'s perspective.
    head_to_head: HashMap<(EngineId, EngineId), HeadToHead>,
    /// Per-pair individual results in played order, from the first id's
    /// perspective (drives the per-game crosstable display).
    pair_results: HashMap<(EngineId, EngineId), Vec<PairGameResult>>,
}

impl Standings {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Pre-register engines so they appear (with a zero record) before playing.
    #[must_use]
    pub fn with_engines(engines: &[EngineId]) -> Self {
        let mut standings = Self::default();
        for &engine in engines {
            standings.per_engine.entry(engine).or_default();
        }
        standings
    }

    /// Incorporate one finished game.
    pub fn record(&mut self, outcome: GameOutcome) {
        {
            let white = self.per_engine.entry(outcome.white).or_default();
            match outcome.result {
                GameResult::WhiteWin => white.wins += 1,
                GameResult::BlackWin => white.losses += 1,
                GameResult::Draw => white.draws += 1,
            }
            if let Some(nps) = outcome.white_nps {
                white.nps_total += u128::from(nps);
                white.nps_samples += 1;
            }
            if let Some(depth) = outcome.white_depth {
                white.depth_total += depth;
                white.depth_samples += 1;
            }
            if let Some(ms) = outcome.white_move_ms {
                white.move_ms_total += ms;
                white.move_ms_samples += 1;
            }
        }
        {
            let black = self.per_engine.entry(outcome.black).or_default();
            match outcome.result {
                GameResult::WhiteWin => black.losses += 1,
                GameResult::BlackWin => black.wins += 1,
                GameResult::Draw => black.draws += 1,
            }
            if let Some(nps) = outcome.black_nps {
                black.nps_total += u128::from(nps);
                black.nps_samples += 1;
            }
            if let Some(depth) = outcome.black_depth {
                black.depth_total += depth;
                black.depth_samples += 1;
            }
            if let Some(ms) = outcome.black_move_ms {
                black.move_ms_total += ms;
                black.move_ms_samples += 1;
            }
        }

        // Attribute forfeit-style terminations to the losing engine.
        let loser = match outcome.result {
            GameResult::WhiteWin => Some(outcome.black),
            GameResult::BlackWin => Some(outcome.white),
            GameResult::Draw => None,
        };
        if let Some(loser) = loser {
            let entry = self.per_engine.entry(loser).or_default();
            match outcome.termination {
                Termination::TimeForfeit => entry.time_losses += 1,
                Termination::EngineCrash | Termination::IllegalMove => entry.crash_losses += 1,
                _ => {}
            }
        }

        // Head-to-head, both directions.
        record_h2h(
            self.head_to_head
                .entry((outcome.white, outcome.black))
                .or_default(),
            outcome.result,
            true,
        );
        record_h2h(
            self.head_to_head
                .entry((outcome.black, outcome.white))
                .or_default(),
            outcome.result,
            false,
        );

        // Per-pair result sequences, both perspectives.
        let (white_res, black_res) = match outcome.result {
            GameResult::WhiteWin => (PairGameResult::Win, PairGameResult::Loss),
            GameResult::BlackWin => (PairGameResult::Loss, PairGameResult::Win),
            GameResult::Draw => (PairGameResult::Draw, PairGameResult::Draw),
        };
        self.pair_results
            .entry((outcome.white, outcome.black))
            .or_default()
            .push(white_res);
        self.pair_results
            .entry((outcome.black, outcome.white))
            .or_default()
            .push(black_res);
    }

    /// Overall standing for an engine (zeroed if it has not played).
    #[must_use]
    pub fn standing(&self, engine: EngineId) -> EngineStanding {
        self.per_engine.get(&engine).copied().unwrap_or_default()
    }

    /// `engine`'s record against `opponent`.
    #[must_use]
    pub fn head_to_head(&self, engine: EngineId, opponent: EngineId) -> HeadToHead {
        self.head_to_head
            .get(&(engine, opponent))
            .copied()
            .unwrap_or_default()
    }

    /// `engine`'s individual game results against `opponent`, in played order.
    #[must_use]
    pub fn pair_results(&self, engine: EngineId, opponent: EngineId) -> &[PairGameResult] {
        self.pair_results
            .get(&(engine, opponent))
            .map_or(&[], Vec::as_slice)
    }

    /// All engines currently tracked.
    pub fn engines(&self) -> impl Iterator<Item = EngineId> + '_ {
        self.per_engine.keys().copied()
    }

    /// Engines ordered by points (descending), breaking ties deterministically by id.
    /// Name/Elo sorts are applied in the GUI where those values are available.
    #[must_use]
    pub fn ranked_by_points(&self) -> Vec<EngineId> {
        let mut rows: Vec<(EngineId, f64)> = self
            .per_engine
            .iter()
            .map(|(id, standing)| (*id, standing.points()))
            .collect();
        rows.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(Ordering::Equal)
                .then_with(|| a.0.0.cmp(&b.0.0))
        });
        rows.into_iter().map(|(id, _)| id).collect()
    }
}

fn record_h2h(record: &mut HeadToHead, result: GameResult, is_white: bool) {
    let win = (result == GameResult::WhiteWin && is_white)
        || (result == GameResult::BlackWin && !is_white);
    let loss = (result == GameResult::BlackWin && is_white)
        || (result == GameResult::WhiteWin && !is_white);
    if result == GameResult::Draw {
        record.draws += 1;
    } else if win {
        record.wins += 1;
    } else if loss {
        record.losses += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(w: EngineId, b: EngineId, result: GameResult) -> GameOutcome {
        GameOutcome {
            white: w,
            black: b,
            result,
            termination: Termination::Checkmate,
            white_nps: Some(1_000_000),
            black_nps: Some(2_000_000),
            white_depth: Some(20.0),
            black_depth: Some(10.0),
            white_move_ms: Some(100.0),
            black_move_ms: Some(300.0),
        }
    }

    #[test]
    fn points_wdl_and_h2h() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut s = Standings::with_engines(&[a, b]);

        s.record(outcome(a, b, GameResult::WhiteWin)); // a beats b
        s.record(outcome(b, a, GameResult::Draw)); // draw
        s.record(outcome(a, b, GameResult::BlackWin)); // b beats a

        let sa = s.standing(a);
        let sb = s.standing(b);
        assert_eq!((sa.wins, sa.draws, sa.losses), (1, 1, 1));
        assert_eq!((sb.wins, sb.draws, sb.losses), (1, 1, 1));
        assert!((sa.points() - 1.5).abs() < f64::EPSILON);
        assert!((sb.points() - 1.5).abs() < f64::EPSILON);

        let ab = s.head_to_head(a, b);
        let ba = s.head_to_head(b, a);
        assert_eq!((ab.wins, ab.draws, ab.losses), (1, 1, 1));
        assert_eq!((ba.wins, ba.draws, ba.losses), (1, 1, 1));
        assert_eq!(ab.games(), 3);
    }

    #[test]
    fn average_nps() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut s = Standings::new();
        s.record(outcome(a, b, GameResult::Draw)); // a:1.0M, b:2.0M
        s.record(outcome(b, a, GameResult::Draw)); // b:1.0M, a:2.0M
        // a sampled 1.0M then 2.0M -> 1.5M; b sampled 2.0M then 1.0M -> 1.5M.
        assert_eq!(s.standing(a).avg_nps(), Some(1_500_000));
        assert_eq!(s.standing(b).avg_nps(), Some(1_500_000));
        // Depth mirrors the same per-color sampling: both average 15.
        assert_eq!(s.standing(a).avg_depth(), Some(15.0));
        assert_eq!(s.standing(b).avg_depth(), Some(15.0));
        // Time/move likewise: both average 200 ms.
        assert_eq!(s.standing(a).avg_move_ms(), Some(200.0));
        assert_eq!(s.standing(b).avg_move_ms(), Some(200.0));
    }

    #[test]
    fn ranking_orders_by_points() {
        let a = EngineId::new();
        let b = EngineId::new();
        let c = EngineId::new();
        let mut s = Standings::with_engines(&[a, b, c]);
        // a beats b and c; b beats c.
        s.record(outcome(a, b, GameResult::WhiteWin));
        s.record(outcome(a, c, GameResult::WhiteWin));
        s.record(outcome(b, c, GameResult::WhiteWin));
        let ranked = s.ranked_by_points();
        assert_eq!(ranked.first().copied(), Some(a)); // 2 pts
        assert_eq!(ranked.last().copied(), Some(c)); // 0 pts
    }

    #[test]
    fn forfeit_terminations_attributed_to_loser() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut s = Standings::with_engines(&[a, b]);
        // b loses on time as black, then a crashes as white.
        s.record(GameOutcome {
            termination: Termination::TimeForfeit,
            ..outcome(a, b, GameResult::WhiteWin)
        });
        s.record(GameOutcome {
            termination: Termination::EngineCrash,
            ..outcome(a, b, GameResult::BlackWin)
        });
        assert_eq!(s.standing(b).time_losses, 1);
        assert_eq!(s.standing(b).crash_losses, 0);
        assert_eq!(s.standing(a).crash_losses, 1);
        assert_eq!(s.standing(a).time_losses, 0);
    }

    #[test]
    fn pair_results_keep_order_and_perspective() {
        let a = EngineId::new();
        let b = EngineId::new();
        let mut s = Standings::new();
        s.record(outcome(a, b, GameResult::WhiteWin)); // a wins
        s.record(outcome(b, a, GameResult::Draw)); // draw
        s.record(outcome(a, b, GameResult::BlackWin)); // b wins
        assert_eq!(
            s.pair_results(a, b),
            &[
                PairGameResult::Win,
                PairGameResult::Draw,
                PairGameResult::Loss
            ]
        );
        assert_eq!(
            s.pair_results(b, a),
            &[
                PairGameResult::Loss,
                PairGameResult::Draw,
                PairGameResult::Win
            ]
        );
    }

    #[test]
    fn unknown_engine_is_zeroed() {
        let s = Standings::new();
        let x = EngineId::new();
        assert_eq!(s.standing(x), EngineStanding::default());
        assert_eq!(s.standing(x).avg_nps(), None);
    }
}
