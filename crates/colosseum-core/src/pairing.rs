//! Pairing / schedule generation.
//!
//! Round-robin uses the classic **circle method**: with `n` engines (a bye is added
//! when `n` is odd), each of the `n-1` (or `n`) rounds is a near-perfect matching in
//! which every engine plays at most once. Each encounter then expands to
//! `games_per_pair` games with alternating colors, and the whole schedule repeats
//! `cycles` times.

use crate::{game::Pairing, ids::EngineId, tournament::Format, tournament::TournamentConfig};

/// Generate the full game schedule for a tournament configuration.
#[must_use]
pub fn generate_schedule(engines: &[EngineId], config: &TournamentConfig) -> Vec<Pairing> {
    match config.format {
        Format::RoundRobin { cycles } => round_robin(engines, cycles, config.games_per_pair),
        Format::Gauntlet { seeds, cycles } => {
            gauntlet(engines, seeds, cycles, config.games_per_pair)
        }
    }
}

/// Generate round-robin pairings: every engine meets every other, `games_per_pair`
/// games per encounter (colors alternating), repeated `cycles` times.
///
/// Returns an empty schedule if there are fewer than two engines, or if `cycles` or
/// `games_per_pair` is zero.
#[must_use]
pub fn round_robin(engines: &[EngineId], cycles: u32, games_per_pair: u32) -> Vec<Pairing> {
    if engines.len() < 2 || cycles == 0 || games_per_pair == 0 {
        return Vec::new();
    }

    let rounds = circle_rounds(engines.len());
    let mut pairings = Vec::new();
    let mut round_no = 0u32;

    for _cycle in 0..cycles {
        for round_pairs in &rounds {
            round_no += 1;
            for &(a, b) in round_pairs {
                for game in 0..games_per_pair {
                    // Alternate colors per game so each pair plays both colors evenly.
                    let (white_idx, black_idx) = if game % 2 == 0 { (a, b) } else { (b, a) };
                    pairings.push(Pairing {
                        white: engines[white_idx],
                        black: engines[black_idx],
                        round: round_no,
                    });
                }
            }
        }
    }

    pairings
}

/// Generate gauntlet pairings: each of the first `seeds` engines plays every engine
/// that is *not* a seed, `games_per_pair` games per encounter (colors alternating),
/// repeated `cycles` times. Each cycle is one round.
///
/// Returns an empty schedule if there are fewer than two engines, if `seeds` is zero
/// or leaves no opponents (`seeds >= n`), or if `cycles`/`games_per_pair` is zero.
#[must_use]
pub fn gauntlet(engines: &[EngineId], seeds: u32, cycles: u32, games_per_pair: u32) -> Vec<Pairing> {
    let n = engines.len();
    let seeds = seeds as usize;
    if n < 2 || cycles == 0 || games_per_pair == 0 || seeds == 0 || seeds >= n {
        return Vec::new();
    }

    let mut pairings = Vec::new();
    for cycle in 0..cycles {
        let round_no = cycle + 1;
        for seed in 0..seeds {
            for opponent in seeds..n {
                for game in 0..games_per_pair {
                    // Alternate colors per game so each pairing is color-balanced.
                    let (white_idx, black_idx) = if game % 2 == 0 {
                        (seed, opponent)
                    } else {
                        (opponent, seed)
                    };
                    pairings.push(Pairing {
                        white: engines[white_idx],
                        black: engines[black_idx],
                        round: round_no,
                    });
                }
            }
        }
    }

    pairings
}

/// Circle-method rounds for `n` players, as index pairs. A bye is inserted for odd
/// `n` and any pairing involving it is dropped. Each returned inner `Vec` is one
/// round in which every (real) player appears at most once.
fn circle_rounds(n: usize) -> Vec<Vec<(usize, usize)>> {
    if n < 2 {
        return Vec::new();
    }

    const BYE: usize = usize::MAX;
    let mut arrangement: Vec<usize> = (0..n).collect();
    if n % 2 == 1 {
        arrangement.push(BYE);
    }
    let m = arrangement.len();
    let num_rounds = m - 1;

    let mut rounds = Vec::with_capacity(num_rounds);
    for _ in 0..num_rounds {
        let mut pairs = Vec::with_capacity(m / 2);
        for i in 0..m / 2 {
            let a = arrangement[i];
            let b = arrangement[m - 1 - i];
            if a != BYE && b != BYE {
                pairs.push((a, b));
            }
        }
        rounds.push(pairs);

        // Rotate everything except the fixed first element one step clockwise.
        let last = arrangement[m - 1];
        for i in (2..m).rev() {
            arrangement[i] = arrangement[i - 1];
        }
        arrangement[1] = last;
    }

    rounds
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn ids(n: usize) -> Vec<EngineId> {
        (0..n).map(|_| EngineId::new()).collect()
    }

    /// Map each engine id back to its index for assertions.
    fn index_of(engines: &[EngineId]) -> HashMap<EngineId, usize> {
        engines.iter().enumerate().map(|(i, e)| (*e, i)).collect()
    }

    #[test]
    fn empty_and_trivial_cases() {
        assert!(round_robin(&[], 1, 2).is_empty());
        assert!(round_robin(&ids(1), 1, 2).is_empty());
        assert!(round_robin(&ids(4), 0, 2).is_empty());
        assert!(round_robin(&ids(4), 1, 0).is_empty());
    }

    #[test]
    fn each_pair_meets_expected_number_of_times() {
        for n in 2..=6 {
            let engines = ids(n);
            let idx = index_of(&engines);
            let games_per_pair = 2;
            let cycles = 2;
            let schedule = round_robin(&engines, cycles, games_per_pair);

            // C(n,2) encounters * games_per_pair * cycles.
            let expected_total = (n * (n - 1) / 2) * games_per_pair as usize * cycles as usize;
            assert_eq!(schedule.len(), expected_total, "n={n}");

            // Every unordered pair appears exactly games_per_pair*cycles times.
            let mut pair_counts: HashMap<(usize, usize), u32> = HashMap::new();
            for p in &schedule {
                let (w, b) = (idx[&p.white], idx[&p.black]);
                let key = (w.min(b), w.max(b));
                *pair_counts.entry(key).or_default() += 1;
            }
            assert_eq!(pair_counts.len(), n * (n - 1) / 2, "n={n}");
            for (_, count) in pair_counts {
                assert_eq!(count, games_per_pair * cycles, "n={n}");
            }
        }
    }

    #[test]
    fn colors_are_balanced_per_pair() {
        let engines = ids(4);
        let idx = index_of(&engines);
        let schedule = round_robin(&engines, 1, 2);

        // For each unordered pair, each engine should be White exactly once.
        let mut white_counts: HashMap<(usize, usize), [u32; 2]> = HashMap::new();
        for p in &schedule {
            let (w, b) = (idx[&p.white], idx[&p.black]);
            let key = (w.min(b), w.max(b));
            let slot = white_counts.entry(key).or_default();
            if w == key.0 {
                slot[0] += 1
            } else {
                slot[1] += 1
            }
        }
        for (_, counts) in white_counts {
            assert_eq!(counts, [1, 1]);
        }
    }

    #[test]
    fn gauntlet_empty_and_trivial_cases() {
        assert!(gauntlet(&[], 1, 1, 2).is_empty());
        assert!(gauntlet(&ids(1), 1, 1, 2).is_empty());
        assert!(gauntlet(&ids(4), 0, 1, 2).is_empty()); // no seeds
        assert!(gauntlet(&ids(4), 4, 1, 2).is_empty()); // no opponents left
        assert!(gauntlet(&ids(4), 1, 0, 2).is_empty()); // no cycles
        assert!(gauntlet(&ids(4), 1, 1, 0).is_empty()); // no games
    }

    #[test]
    fn gauntlet_seed_plays_every_opponent_only() {
        // 1 seed vs 3 opponents, 2 games each, 1 cycle => 3 * 2 = 6 games.
        let engines = ids(4);
        let idx = index_of(&engines);
        let schedule = gauntlet(&engines, 1, 1, 2);
        assert_eq!(schedule.len(), 6);

        // The seed (index 0) is in every game; opponents (1..4) never face each other.
        for p in &schedule {
            let (w, b) = (idx[&p.white], idx[&p.black]);
            assert!(w == 0 || b == 0, "every gauntlet game involves the seed");
            assert!(w != b);
        }

        // Each opponent meets the seed exactly twice (once each color).
        let mut counts: HashMap<usize, [u32; 2]> = HashMap::new();
        for p in &schedule {
            let (w, b) = (idx[&p.white], idx[&p.black]);
            let opp = if w == 0 { b } else { w };
            let slot = counts.entry(opp).or_default();
            if w == 0 {
                slot[0] += 1; // seed white
            } else {
                slot[1] += 1; // seed black
            }
        }
        assert_eq!(counts.len(), 3);
        for (_opp, c) in counts {
            assert_eq!(c, [1, 1]);
        }
    }

    #[test]
    fn gauntlet_multiple_seeds_and_cycles() {
        // 2 seeds vs 2 opponents, 1 game each, 2 cycles.
        // Per cycle: 2 seeds * 2 opponents * 1 game = 4. Two cycles => 8.
        let engines = ids(4);
        let idx = index_of(&engines);
        let schedule = gauntlet(&engines, 2, 2, 1);
        assert_eq!(schedule.len(), 8);

        // Seeds (0,1) never play each other; opponents (2,3) never play each other.
        for p in &schedule {
            let (w, b) = (idx[&p.white], idx[&p.black]);
            let both_seeds = w < 2 && b < 2;
            let both_opponents = w >= 2 && b >= 2;
            assert!(!both_seeds && !both_opponents);
        }

        // Each cycle is its own round.
        let rounds: HashSet<u32> = schedule.iter().map(|p| p.round).collect();
        assert_eq!(rounds, HashSet::from([1, 2]));
    }

    #[test]
    fn each_engine_plays_once_per_round_even_n() {
        let engines = ids(4);
        let idx = index_of(&engines);
        let schedule = round_robin(&engines, 1, 1); // 1 game per encounter, 1 cycle

        let mut per_round: HashMap<u32, Vec<usize>> = HashMap::new();
        for p in &schedule {
            let entry = per_round.entry(p.round).or_default();
            entry.push(idx[&p.white]);
            entry.push(idx[&p.black]);
        }
        // 4 engines -> 3 rounds, each round 2 games, each engine appears once.
        assert_eq!(per_round.len(), 3);
        for (_round, mut players) in per_round {
            players.sort_unstable();
            let unique: HashSet<_> = players.iter().copied().collect();
            assert_eq!(
                unique.len(),
                players.len(),
                "an engine played twice in a round"
            );
            assert_eq!(unique.len(), 4);
        }
    }
}
