// SPDX-License-Identifier: GPL-3.0-or-later
//! Opening-book loading: turn an EPD or PGN file into a list of concrete starting
//! positions ([`ResolvedOpening`]) the scheduler assigns to encounters.
//!
//! - **EPD**: each non-empty, non-comment line is one position. The four required
//!   EPD fields (`board stm castling ep`) are completed into a full FEN; any
//!   trailing opcodes are ignored.
//! - **PGN**: each game's first `plies` half-moves form one opening line, replayed
//!   from the game's start position (the `FEN` tag if present, else the standard
//!   start). The opening is stored as `start_fen` + the UCI moves to pre-play, so
//!   the move history (and the PGN movetext) includes the opening.
//!
//! Ordering is applied here: [`OpeningOrder::Random`] shuffles deterministically
//! from the book's seed (a small self-contained PRNG, so no `rand` dependency and
//! reproducible across resume), then `count` truncates the list.

use colosseum_core::{OpeningBook, OpeningFormat, OpeningOrder};
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, EnPassantMode, Position};

use crate::error::EngineError;

/// A concrete opening: where to start and which moves to pre-play before the
/// engines take over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedOpening {
    /// Starting FEN; `None` means the standard start position.
    pub start_fen: Option<String>,
    /// UCI long-algebraic moves to play out before the engines move.
    pub moves: Vec<String>,
    /// Human-readable label (the FEN, or the opening's SAN line) for previews.
    pub label: String,
}

impl ResolvedOpening {
    /// The standard start position with no pre-played moves.
    #[must_use]
    pub fn startpos() -> Self {
        Self {
            start_fen: None,
            moves: Vec::new(),
            label: "startpos".to_string(),
        }
    }
}

/// Load and order the openings described by `book`.
///
/// Returns an error if the file cannot be read, or if it parses to zero usable
/// openings (so the caller can surface a clear message instead of silently
/// falling back to the start position).
pub fn load_openings(book: &OpeningBook) -> Result<Vec<ResolvedOpening>, EngineError> {
    let text = std::fs::read_to_string(&book.path)?;
    let mut openings = match book.format {
        OpeningFormat::Epd => parse_epd(&text),
        OpeningFormat::Pgn => parse_pgn(&text, book.plies.max(1) as usize),
    };

    if openings.is_empty() {
        return Err(EngineError::Corrupt(format!(
            "no usable openings found in {}",
            book.path.display()
        )));
    }

    if book.order == OpeningOrder::Random {
        shuffle(&mut openings, book.seed);
    }

    if let Some(count) = book.count {
        openings.truncate(count.max(1) as usize);
    }

    Ok(openings)
}

/// Parse EPD lines into resolved openings (each is a bare FEN, no pre-moves).
fn parse_epd(text: &str) -> Vec<ResolvedOpening> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(fen) = epd_to_fen(line) {
            out.push(ResolvedOpening {
                label: fen.clone(),
                start_fen: Some(fen),
                moves: Vec::new(),
            });
        }
    }
    out
}

/// Complete an EPD line into a full, validated FEN string.
///
/// EPD carries `board stm castling ep` plus optional opcodes; FEN additionally
/// needs halfmove and fullmove counters, which we default to `0 1`.
fn epd_to_fen(line: &str) -> Option<String> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 4 {
        return None;
    }
    let fen = format!(
        "{} {} {} {} 0 1",
        fields[0], fields[1], fields[2], fields[3]
    );
    // Validate by round-tripping through shakmaty.
    fen.parse::<Fen>()
        .ok()
        .and_then(|f| f.into_position::<Chess>(CastlingMode::Standard).ok())
        .map(|_| fen)
}

/// Parse PGN games, taking the first `plies` half-moves of each as an opening.
fn parse_pgn(text: &str, plies: usize) -> Vec<ResolvedOpening> {
    let mut out = Vec::new();
    for game in split_pgn_games(text) {
        if let Some(opening) = parse_pgn_game(&game, plies) {
            out.push(opening);
        }
    }
    out
}

/// Split a PGN file into individual games. A new game begins at a tag section
/// (`[` line) that follows previous movetext.
fn split_pgn_games(text: &str) -> Vec<String> {
    let mut games: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_moves = false;

    for line in text.lines() {
        let trimmed = line.trim_start();
        let is_tag = trimmed.starts_with('[');
        if is_tag && in_moves {
            // A tag after movetext starts a new game.
            if !current.trim().is_empty() {
                games.push(std::mem::take(&mut current));
            }
            in_moves = false;
        }
        if !is_tag && !trimmed.is_empty() {
            in_moves = true;
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        games.push(current);
    }
    games
}

/// Parse one PGN game's tags + movetext into an opening of up to `plies` moves.
fn parse_pgn_game(game: &str, plies: usize) -> Option<ResolvedOpening> {
    // A `[FEN "..."]` tag sets a non-standard start position.
    let start_fen = extract_fen_tag(game);
    let mut pos: Chess = match &start_fen {
        Some(fen) => fen
            .parse::<Fen>()
            .ok()?
            .into_position(CastlingMode::Standard)
            .ok()?,
        None => Chess::default(),
    };

    let movetext = strip_tags(game);
    let mut moves: Vec<String> = Vec::new();
    let mut sans: Vec<String> = Vec::new();

    for token in tokenize_movetext(&movetext) {
        if moves.len() >= plies {
            break;
        }
        let Ok(san) = token.parse::<San>() else {
            continue; // skip move numbers, results, NAGs, etc.
        };
        let Ok(mv) = san.to_move(&pos) else {
            break; // illegal in this line; stop here
        };
        let uci = mv.to_uci(CastlingMode::Standard).to_string();
        pos.play_unchecked(&mv);
        sans.push(token.to_string());
        moves.push(uci);
    }

    if moves.is_empty() {
        return None;
    }
    Some(ResolvedOpening {
        start_fen,
        label: sans.join(" "),
        moves,
    })
}

/// Extract the value of a `[FEN "..."]` tag, if present.
fn extract_fen_tag(game: &str) -> Option<String> {
    for line in game.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("[FEN ") {
            let inner = rest.trim_end_matches(']').trim().trim_matches('"');
            if !inner.is_empty() {
                return Some(inner.to_string());
            }
        }
    }
    None
}

/// Remove tag lines, leaving only movetext.
fn strip_tags(game: &str) -> String {
    game.lines()
        .filter(|l| !l.trim_start().starts_with('['))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Split movetext into SAN-ish tokens, dropping move numbers, comments,
/// variations, NAGs and result markers.
fn tokenize_movetext(movetext: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = movetext.chars().peekable();
    let mut depth_brace = 0u32; // { comment }
    let mut depth_paren = 0u32; // ( variation )
    let mut current = String::new();

    let flush = |current: &mut String, tokens: &mut Vec<String>| {
        if !current.is_empty() {
            tokens.push(std::mem::take(current));
        }
    };

    while let Some(c) = chars.next() {
        match c {
            '{' => depth_brace += 1,
            '}' => depth_brace = depth_brace.saturating_sub(1),
            '(' => depth_paren += 1,
            ')' => depth_paren = depth_paren.saturating_sub(1),
            _ if depth_brace > 0 || depth_paren > 0 => {}
            c if c.is_whitespace() => flush(&mut current, &mut tokens),
            '$' => {
                // NAG: skip the following digits.
                while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                    chars.next();
                }
            }
            _ => current.push(c),
        }
    }
    flush(&mut current, &mut tokens);

    tokens.into_iter().filter(|t| is_san_candidate(t)).collect()
}

/// Whether a token might be a SAN move (filters move numbers and results).
fn is_san_candidate(token: &str) -> bool {
    if token.is_empty() {
        return false;
    }
    if matches!(token, "1-0" | "0-1" | "1/2-1/2" | "*") {
        return false;
    }
    // Move-number tokens like "1." or "12..." — start with a digit and contain
    // only digits and dots.
    if token.chars().next().is_some_and(|c| c.is_ascii_digit())
        && token.chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        return false;
    }
    true
}

/// Deterministic in-place Fisher–Yates shuffle seeded by `seed` (SplitMix64).
fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let n = items.len();
    for i in (1..n).rev() {
        let j = (next() % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

/// Quick metadata for the GUI preview without retaining every opening.
pub struct OpeningSummary {
    pub count: usize,
    pub first_label: Option<String>,
}

/// Load a book only to report how many openings it yields and a sample label.
pub fn summarize(book: &OpeningBook) -> Result<OpeningSummary, EngineError> {
    let openings = load_openings(book)?;
    Ok(OpeningSummary {
        count: openings.len(),
        first_label: openings.first().map(|o| o.label.clone()),
    })
}

/// Validate a starting FEN, returning a usable [`Chess`] position.
#[must_use]
pub fn position_from_fen(fen: &str) -> Option<Chess> {
    fen.parse::<Fen>()
        .ok()
        .and_then(|f| f.into_position(CastlingMode::Standard).ok())
}

/// Re-derive the FEN of a position after pre-playing `moves` from `start_fen`.
/// Used in tests and for PGN tags.
#[must_use]
pub fn fen_after(start_fen: Option<&str>, moves: &[String]) -> Option<String> {
    let mut pos: Chess = match start_fen {
        Some(fen) => position_from_fen(fen)?,
        None => Chess::default(),
    };
    for m in moves {
        let uci = m.parse::<UciMove>().ok()?;
        let mv = uci.to_move(&pos).ok()?;
        pos.play_unchecked(&mv);
    }
    Some(Fen::from_position(pos, EnPassantMode::Legal).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("colosseum-openings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        path
    }

    #[test]
    fn epd_lines_become_fens() {
        let epd = "\
# a comment
rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1
r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - bm Nf6;
";
        let path = write_temp("test.epd", epd);
        let book = OpeningBook::new(path);
        let openings = load_openings(&book).unwrap();
        assert_eq!(openings.len(), 2);
        assert!(
            openings[0]
                .start_fen
                .as_deref()
                .unwrap()
                .starts_with("rnbqkbnr")
        );
        assert!(openings[0].moves.is_empty());
        // The second line's trailing opcode is ignored; FEN is still valid.
        assert!(position_from_fen(openings[1].start_fen.as_deref().unwrap()).is_some());
    }

    #[test]
    fn pgn_first_plies_become_moves() {
        let pgn = "\
[Event \"Test\"]
[White \"A\"]
[Black \"B\"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 1-0

[Event \"Test2\"]

1. d4 d5 2. c4 *
";
        let path = write_temp("test.pgn", pgn);
        let mut book = OpeningBook::new(path);
        book.plies = 4;
        let openings = load_openings(&book).unwrap();
        assert_eq!(openings.len(), 2);
        // First game, first 4 plies.
        assert_eq!(openings[0].moves, vec!["e2e4", "e7e5", "g1f3", "b8c6"]);
        assert!(openings[0].start_fen.is_none());
        // Second game has only 3 plies available -> truncated to what's there.
        assert_eq!(openings[1].moves, vec!["d2d4", "d7d5", "c2c4"]);
    }

    #[test]
    fn fen_after_moves_matches_known_position() {
        // 1. e4 from the start position.
        let fen = fen_after(None, &["e2e4".to_string()]).unwrap();
        assert!(fen.starts_with("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b"));
    }

    #[test]
    fn count_and_random_order_are_deterministic() {
        let epd = "\
8/8/8/8/8/8/8/4K2k w - -
rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -
r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq -
rnbqkb1r/pppppppp/5n2/8/8/5N2/PPPPPPPP/RNBQKB1R w KQkq -
";
        let path = write_temp("order.epd", epd);
        let mut book = OpeningBook::new(path);
        book.order = OpeningOrder::Random;
        book.seed = 42;
        book.count = Some(2);
        let a = load_openings(&book).unwrap();
        let b = load_openings(&book).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(a, b, "same seed yields the same order");
    }

    #[test]
    fn missing_or_empty_book_errors() {
        let path = write_temp("empty.epd", "\n# only a comment\n");
        let book = OpeningBook::new(path);
        assert!(load_openings(&book).is_err());
    }
}
