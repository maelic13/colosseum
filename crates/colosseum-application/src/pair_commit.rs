//! Pair-atomic deterministic commit ordering for sequential experiments.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A complete colour-reversed unit. There is intentionally no constructor for
/// a one-game value in the commit queue's public input type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletePair<T> {
    pub pair_id: u32,
    pub first: T,
    pub second: T,
}

#[derive(Debug)]
pub struct PairCommitQueue<T> {
    next_pair_id: u32,
    max_pairs: u32,
    pending: BTreeMap<u32, CompletePair<T>>,
}

impl<T> PairCommitQueue<T> {
    pub fn new(next_pair_id: u32, max_pairs: u32) -> Result<Self, PairCommitError> {
        if next_pair_id == 0 || next_pair_id > max_pairs.saturating_add(1) {
            return Err(PairCommitError::InvalidNextPair {
                next_pair_id,
                max_pairs,
            });
        }
        Ok(Self {
            next_pair_id,
            max_pairs,
            pending: BTreeMap::new(),
        })
    }

    /// Record one worker completion and release every now-contiguous pair in
    /// schedule order. An out-of-order completion remains private until every
    /// earlier pair is available.
    pub fn complete(
        &mut self,
        pair: CompletePair<T>,
    ) -> Result<Vec<CompletePair<T>>, PairCommitError> {
        if pair.pair_id < self.next_pair_id || pair.pair_id > self.max_pairs {
            return Err(PairCommitError::PairOutsidePendingRange {
                pair_id: pair.pair_id,
                next_pair_id: self.next_pair_id,
                max_pairs: self.max_pairs,
            });
        }
        let pair_id = pair.pair_id;
        if self.pending.contains_key(&pair_id) {
            return Err(PairCommitError::DuplicatePair(pair_id));
        }
        self.pending.insert(pair_id, pair);
        let mut released = Vec::new();
        while let Some(pair) = self.pending.remove(&self.next_pair_id) {
            released.push(pair);
            self.next_pair_id += 1;
        }
        Ok(released)
    }

    #[must_use]
    pub const fn next_pair_id(&self) -> u32 {
        self.next_pair_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PairCommitError {
    #[error("next pair {next_pair_id} is invalid for a schedule capped at {max_pairs}")]
    InvalidNextPair { next_pair_id: u32, max_pairs: u32 },
    #[error("pair {pair_id} is outside the pending schedule range {next_pair_id}..={max_pairs}")]
    PairOutsidePendingRange {
        pair_id: u32,
        next_pair_id: u32,
        max_pairs: u32,
    },
    #[error("pair {0} completed more than once")]
    DuplicatePair(u32),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(id: u32) -> CompletePair<&'static str> {
        CompletePair {
            pair_id: id,
            first: "first colour",
            second: "reversed colour",
        }
    }

    #[test]
    fn out_of_order_workers_release_only_contiguous_complete_pairs() {
        let mut queue = PairCommitQueue::new(1, 4).unwrap();
        assert!(queue.complete(pair(3)).unwrap().is_empty());
        assert!(queue.complete(pair(2)).unwrap().is_empty());
        assert_eq!(
            queue
                .complete(pair(1))
                .unwrap()
                .into_iter()
                .map(|pair| pair.pair_id)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(queue.complete(pair(4)).unwrap()[0].pair_id, 4);
        assert_eq!(queue.next_pair_id(), 5);
    }

    #[test]
    fn resume_and_duplicate_ranges_are_fail_closed() {
        let mut queue = PairCommitQueue::new(3, 5).unwrap();
        assert!(matches!(
            queue.complete(pair(2)),
            Err(PairCommitError::PairOutsidePendingRange { .. })
        ));
        assert!(queue.complete(pair(4)).unwrap().is_empty());
        assert!(matches!(
            queue.complete(pair(4)),
            Err(PairCommitError::DuplicatePair(4))
        ));
    }
}
