//! Pairing generation. Step 2 provides a minimal all-pairs generator to exercise the
//! types; Step 3 replaces it with the circle method (balanced colors, proper rounds,
//! `games_per_pair`) plus exhaustive tests.

use crate::{game::Pairing, ids::EngineId};

/// Generate round-robin pairings for the given engines.
///
/// Placeholder implementation: emits each unordered pair once per cycle with the
/// lower-indexed engine as White. Color balancing and `games_per_pair` handling
/// arrive in Step 3.
#[must_use]
pub fn round_robin(engines: &[EngineId], cycles: u32) -> Vec<Pairing> {
    let mut pairings = Vec::new();
    let mut round = 0u32;
    for _cycle in 0..cycles.max(1) {
        for i in 0..engines.len() {
            for j in (i + 1)..engines.len() {
                pairings.push(Pairing {
                    white: engines[i],
                    black: engines[j],
                    round,
                });
                round += 1;
            }
        }
    }
    pairings
}
