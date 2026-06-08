//! ECO opening classification from the embedded lichess openings database
//! (CC0, see `assets/openings/LICENSE.md`).
//!
//! The TSVs are replayed once (lazily) with `shakmaty` into a map from
//! position hash → opening, so classification is transposition-aware: a game
//! is labeled by the deepest book position it reaches, no matter the move
//! order. Lookup is a hash-map probe per ply.

use std::collections::HashMap;
use std::sync::LazyLock;

use shakmaty::zobrist::Zobrist64;
use shakmaty::{Chess, EnPassantMode, Position, san::SanPlus};

/// One named opening line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opening {
    pub eco: &'static str,
    pub name: &'static str,
    /// Book-line depth in plies (deeper = more specific).
    pub plies: u32,
}

const TSV: [&str; 5] = [
    include_str!("../assets/openings/a.tsv"),
    include_str!("../assets/openings/b.tsv"),
    include_str!("../assets/openings/c.tsv"),
    include_str!("../assets/openings/d.tsv"),
    include_str!("../assets/openings/e.tsv"),
];

/// Position-hash → opening, keeping the most specific (deepest) line per
/// position. ~3,700 lines replay in a few milliseconds on first use.
static BOOK: LazyLock<HashMap<u64, Opening>> = LazyLock::new(|| {
    let mut map: HashMap<u64, Opening> = HashMap::new();
    for tsv in TSV {
        for line in tsv.lines().skip(1) {
            let mut cols = line.split('\t');
            let (Some(eco), Some(name), Some(pgn)) = (cols.next(), cols.next(), cols.next()) else {
                continue;
            };
            let mut pos = Chess::default();
            let mut plies = 0u32;
            for token in pgn.split_whitespace() {
                if token.ends_with('.') {
                    continue; // move number
                }
                let Ok(san) = token.parse::<SanPlus>() else {
                    break;
                };
                let Ok(m) = san.san.to_move(&pos) else {
                    break;
                };
                pos.play_unchecked(m);
                plies += 1;
            }
            if plies == 0 {
                continue;
            }
            let key = pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal);
            let entry = Opening { eco, name, plies };
            map.entry(key.0)
                .and_modify(|existing| {
                    if plies > existing.plies {
                        *existing = entry;
                    }
                })
                .or_insert(entry);
        }
    }
    map
});

/// Look up the opening for a single position hash.
#[must_use]
pub fn lookup(position_hash: u64) -> Option<Opening> {
    BOOK.get(&position_hash).copied()
}

/// Classify a game played from the standard start position: replay `uci_moves`
/// and return the deepest book position reached. Returns `None` for games from
/// a non-standard FEN (book classification would be meaningless).
///
/// The live view classifies incrementally via [`lookup`]; this whole-game form
/// exists for one-shot callers (and exercises the same book in tests).
#[must_use]
#[allow(dead_code)]
pub fn classify(start_fen: Option<&str>, uci_moves: &[String]) -> Option<Opening> {
    if start_fen.is_some_and(|f| !is_standard_start(f)) {
        return None;
    }
    let mut pos = Chess::default();
    let mut best: Option<Opening> = None;
    for uci in uci_moves {
        let m = uci
            .parse::<shakmaty::uci::UciMove>()
            .ok()?
            .to_move(&pos)
            .ok()?;
        pos.play_unchecked(m);
        let key = pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal);
        if let Some(op) = lookup(key.0) {
            best = Some(op);
        }
    }
    best
}

fn is_standard_start(fen: &str) -> bool {
    fen.trim()
        .starts_with("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_najdorf() {
        let moves: Vec<String> = [
            "e2e4", "c7c5", "g1f3", "d7d6", "d2d4", "c5d4", "f3d4", "g8f6", "b1c3", "a7a6",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
        let op = classify(None, &moves).expect("known opening");
        assert_eq!(op.eco, "B90");
        assert!(op.name.contains("Najdorf"), "got {}", op.name);
    }

    #[test]
    fn transposition_still_classifies() {
        // 1. Nf3 d5 2. d4 reaches a Queen's-Pawn-ish book position by another order.
        let moves: Vec<String> = ["g1f3", "d7d5", "d2d4"]
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert!(classify(None, &moves).is_some());
    }

    #[test]
    fn custom_fen_is_not_classified() {
        let moves = vec!["e2e4".to_string()];
        assert_eq!(
            classify(Some("4k3/8/8/8/8/8/8/4K3 w - - 0 1"), &moves),
            None
        );
    }
}
