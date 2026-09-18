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

use colosseum_core::rng::stream_names;
use colosseum_core::{NamedRng, OpeningBook, OpeningFormat, OpeningOrder};
use serde::{Deserialize, Serialize};
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess, EnPassantMode, Position};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OpeningError {
    #[error("could not read opening book: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid opening book: {0}")]
    Invalid(String),
}

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

/// A loaded book, held compactly: the text of every opening in one buffer, and
/// per opening only where its parts begin and how long they are.
///
/// A book can hold millions of openings. As one owned FEN, one owned label and
/// one vector per opening, a 2.6-million-line EPD took gigabytes and seconds to
/// build; here it is the text itself plus a fixed-size entry per opening.
/// An opening is materialised as a [`ResolvedOpening`] only when a game is
/// assigned it.
///
/// It is deliberately not `Clone`. Launch settings are cloned once per game,
/// and a book carried in them by value was copied with every launch: half a
/// second each with a 2.6-million-line book, on the one task that starts games.
/// Sharing it behind an `Arc` is the only way to hand it on.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct OpeningList {
    buffer: String,
    entries: Vec<CompactOpening>,
}

/// Where one opening's parts sit in the buffer, laid out as
/// `[start FEN][UCI moves, space-separated][label]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompactOpening {
    start: usize,
    /// Zero for the standard start position.
    fen_len: u32,
    moves_len: u32,
    /// Zero when the label is the start FEN, as it is for every EPD line.
    label_len: u32,
}

impl OpeningList {
    /// A list of the given openings, in order.
    #[must_use]
    pub fn from_resolved(openings: impl IntoIterator<Item = ResolvedOpening>) -> Self {
        let mut list = Self::default();
        for opening in openings {
            let label = (opening.start_fen.as_deref() != Some(opening.label.as_str()))
                .then_some(opening.label.as_str());
            list.push(opening.start_fen.as_deref(), &opening.moves, label);
        }
        list
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn part(&self, start: usize, len: u32) -> &str {
        &self.buffer[start..start + len as usize]
    }

    /// The opening's starting FEN; `None` for the standard start position.
    #[must_use]
    pub fn start_fen(&self, index: usize) -> Option<&str> {
        let entry = self.entries[index];
        (entry.fen_len > 0).then(|| self.part(entry.start, entry.fen_len))
    }

    /// The opening's pre-played UCI moves.
    pub fn moves(&self, index: usize) -> impl Iterator<Item = &str> {
        let entry = self.entries[index];
        self.part(entry.start + entry.fen_len as usize, entry.moves_len)
            .split(' ')
            .filter(|mv| !mv.is_empty())
    }

    /// The opening's label: its FEN, or its SAN line.
    #[must_use]
    pub fn label(&self, index: usize) -> &str {
        let entry = self.entries[index];
        if entry.label_len == 0 {
            self.part(entry.start, entry.fen_len)
        } else {
            self.part(
                entry.start + entry.fen_len as usize + entry.moves_len as usize,
                entry.label_len,
            )
        }
    }

    /// The opening at `index`, materialised.
    #[must_use]
    pub fn get(&self, index: usize) -> ResolvedOpening {
        ResolvedOpening {
            start_fen: self.start_fen(index).map(str::to_owned),
            moves: self.moves(index).map(str::to_owned).collect(),
            label: self.label(index).to_owned(),
        }
    }

    /// Every opening, materialised one at a time.
    pub fn iter(&self) -> impl Iterator<Item = ResolvedOpening> + '_ {
        (0..self.len()).map(|index| self.get(index))
    }

    /// Every opening, materialised.
    #[must_use]
    pub fn into_resolved(self) -> Vec<ResolvedOpening> {
        self.iter().collect()
    }

    fn push(&mut self, fen: Option<&str>, moves: &[String], label: Option<&str>) {
        let start = self.buffer.len();
        let fen = fen.unwrap_or_default();
        self.buffer.push_str(fen);
        let moves_start = self.buffer.len();
        for (index, mv) in moves.iter().enumerate() {
            if index > 0 {
                self.buffer.push(' ');
            }
            self.buffer.push_str(mv);
        }
        let moves_len = self.buffer.len() - moves_start;
        let label = label.unwrap_or_default();
        self.buffer.push_str(label);
        self.entries.push(CompactOpening {
            start,
            fen_len: fen.len() as u32,
            moves_len: moves_len as u32,
            label_len: label.len() as u32,
        });
    }

    /// Append another list after this one, keeping both orders.
    fn extend(&mut self, other: Self) {
        let shift = self.buffer.len();
        self.buffer.push_str(&other.buffer);
        self.entries
            .extend(other.entries.into_iter().map(|entry| CompactOpening {
                start: entry.start + shift,
                ..entry
            }));
    }
}

/// Load and order the openings described by `book`.
///
/// Returns an error if the file cannot be read, or if it parses to zero usable
/// openings (so the caller can surface a clear message instead of silently
/// falling back to the start position).
pub fn load_openings(book: &OpeningBook) -> Result<Vec<ResolvedOpening>, OpeningError> {
    load_openings_with_order(book, |entries| {
        if book.order == OpeningOrder::Random {
            shuffle(entries, book.seed);
        }
        Ok(())
    })
    .map(OpeningList::into_resolved)
}

/// Load openings using the versioned named RNG contract used by CLI runs.
pub fn load_openings_named(
    book: &OpeningBook,
    master_seed: u64,
) -> Result<OpeningList, OpeningError> {
    load_openings_with_order(book, |entries| {
        if book.order == OpeningOrder::Random {
            NamedRng::new(master_seed, stream_names::OPENING_ORDER)
                .map_err(|error| OpeningError::Invalid(error.to_string()))?
                .shuffle(entries);
        }
        Ok(())
    })
}

/// Load a book and order it. Ordering permutes the fixed-size entries, with
/// the same swaps an owned list of openings received, so a seed selects the
/// same sequence it always did.
fn load_openings_with_order(
    book: &OpeningBook,
    order: impl FnOnce(&mut [CompactOpening]) -> Result<(), OpeningError>,
) -> Result<OpeningList, OpeningError> {
    let text = std::fs::read_to_string(&book.path)?;
    let mut openings = match book.format {
        OpeningFormat::Epd => parse_epd(&text),
        OpeningFormat::Pgn => parse_pgn(&text, book.plies.max(1) as usize),
    };

    if openings.is_empty() {
        return Err(OpeningError::Invalid(format!(
            "no usable openings found in {}",
            book.path.display()
        )));
    }

    order(&mut openings.entries)?;

    if let Some(count) = book.count {
        openings.entries.truncate(count.max(1) as usize);
    }

    Ok(openings)
}

/// Lines per parsing chunk below which a book is parsed on one thread.
const EPD_PARALLEL_LINES: usize = 20_000;

/// Parse EPD lines into openings (each is a bare FEN, no pre-moves).
///
/// Every line is validated as a position, which is most of the cost of loading
/// a large book; the text is cut at line boundaries into one chunk per
/// available core and the chunks are joined in file order, so the result is
/// the one a single pass produces.
fn parse_epd(text: &str) -> OpeningList {
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(text.len() / (EPD_PARALLEL_LINES * 40) + 1);
    if threads <= 1 {
        return parse_epd_lines(text);
    }
    let mut bounds = vec![0];
    for chunk in 1..threads {
        let target = text.len() * chunk / threads;
        // A byte search, so a cut never lands inside a multi-byte character:
        // the byte after a newline always starts one.
        let cut = text.as_bytes()[target..]
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(text.len(), |offset| target + offset + 1);
        if cut > *bounds.last().expect("bounds start at zero") {
            bounds.push(cut);
        }
    }
    bounds.push(text.len());
    bounds.dedup();
    let parts = std::thread::scope(|scope| {
        bounds
            .windows(2)
            .map(|window| {
                let part = &text[window[0]..window[1]];
                scope.spawn(move || parse_epd_lines(part))
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|handle| handle.join().expect("an EPD parsing thread panicked"))
            .collect::<Vec<_>>()
    });
    let mut list = OpeningList::default();
    for part in parts {
        list.extend(part);
    }
    list
}

fn parse_epd_lines(text: &str) -> OpeningList {
    let mut list = OpeningList {
        buffer: String::with_capacity(text.len()),
        entries: Vec::new(),
    };
    let mut fen = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if epd_fen_into(line, &mut fen) {
            list.push(Some(&fen), &[], None);
        }
    }
    list
}

/// Complete an EPD line into a full, validated FEN string.
///
/// EPD carries `board stm castling ep` plus optional opcodes; FEN additionally
/// needs halfmove and fullmove counters, which we default to `0 1`.
fn epd_to_fen(line: &str) -> Option<String> {
    let mut fen = String::new();
    epd_fen_into(line, &mut fen).then_some(fen)
}

/// Write the FEN of an EPD line into `fen` and say whether it is a valid
/// standard-chess position.
fn epd_fen_into(line: &str, fen: &mut String) -> bool {
    fen.clear();
    let mut fields = line.split_whitespace();
    for index in 0..4 {
        let Some(field) = fields.next() else {
            return false;
        };
        if index > 0 {
            fen.push(' ');
        }
        fen.push_str(field);
    }
    fen.push_str(" 0 1");
    // Validate by round-tripping through shakmaty.
    fen.parse::<Fen>()
        .ok()
        .and_then(|f| f.into_position::<Chess>(CastlingMode::Standard).ok())
        .is_some()
}

/// Parse PGN games, taking the first `plies` half-moves of each as an opening.
fn parse_pgn(text: &str, plies: usize) -> OpeningList {
    let mut out = OpeningList::default();
    for game in split_pgn_games(text) {
        if let Some(opening) = parse_pgn_game(&game, plies) {
            out.push(
                opening.start_fen.as_deref(),
                &opening.moves,
                Some(&opening.label),
            );
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
        pos.play_unchecked(mv);
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpeningSummary {
    pub count: usize,
    pub first_label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningAudit {
    pub format: String,
    pub candidates: usize,
    pub usable: usize,
    pub rejected_indices: Vec<usize>,
}

impl OpeningAudit {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.usable > 0 && self.rejected_indices.is_empty()
    }
}

/// Strictly account for every non-comment EPD line or PGN game. Normal match
/// loading may skip malformed candidates; this audit makes those skips visible.
pub fn audit_opening_book(book: &OpeningBook) -> Result<OpeningAudit, OpeningError> {
    let text = std::fs::read_to_string(&book.path)?;
    let validity = match book.format {
        OpeningFormat::Epd => text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(|line| epd_to_fen(line).is_some())
            .collect::<Vec<_>>(),
        OpeningFormat::Pgn => split_pgn_games(&text)
            .iter()
            .map(|game| parse_pgn_game(game, book.plies.max(1) as usize).is_some())
            .collect::<Vec<_>>(),
    };
    let rejected_indices = validity
        .iter()
        .enumerate()
        .filter_map(|(index, valid)| (!valid).then_some(index + 1))
        .collect::<Vec<_>>();
    Ok(OpeningAudit {
        format: book.format.label().into(),
        candidates: validity.len(),
        usable: validity.iter().filter(|valid| **valid).count(),
        rejected_indices,
    })
}

/// Load a book only to report how many openings it yields and a sample label.
pub fn summarize(book: &OpeningBook) -> Result<OpeningSummary, OpeningError> {
    let openings = load_openings(book)?;
    Ok(OpeningSummary {
        count: openings.len(),
        first_label: openings.first().map(|o| o.label.clone()),
    })
}

/// True when a FEN's castling field uses the Shredder/X-FEN file letters that
/// encode Chess960 castling rights.
///
/// The harness plays standard chess only, and a shuffled start position is
/// otherwise indistinguishable from an ordinary one. This is the encoding that
/// says so explicitly, so it is the one place a Chess960 position can be
/// refused instead of silently misread.
#[must_use]
pub fn is_chess960_fen(fen: &str) -> bool {
    let Some(castling) = fen.split_whitespace().nth(2) else {
        return false;
    };
    castling != "-"
        && castling
            .chars()
            .any(|right| matches!(right, 'a'..='h' | 'A'..='H') && !matches!(right, 'k' | 'K'))
}

/// Validate a starting FEN, returning a usable [`Chess`] position.
///
/// A Chess960 castling encoding yields `None`: it is refused rather than
/// reinterpreted as standard castling.
#[must_use]
pub fn position_from_fen(fen: &str) -> Option<Chess> {
    if is_chess960_fen(fen) {
        return None;
    }
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
        pos.play_unchecked(mv);
    }
    Some(Fen::from_position(&pos, EnPassantMode::Legal).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// The loader as it was before the book was held compactly: one owned
    /// opening per line, shuffled as a list. The compact loader must give the
    /// same openings in the same order for the same seed.
    fn owned_reference(book: &OpeningBook, master_seed: u64) -> Vec<ResolvedOpening> {
        let text = std::fs::read_to_string(&book.path).unwrap();
        let mut openings = match book.format {
            OpeningFormat::Epd => text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty() && !line.starts_with('#'))
                .filter_map(epd_to_fen)
                .map(|fen| ResolvedOpening {
                    label: fen.clone(),
                    start_fen: Some(fen),
                    moves: Vec::new(),
                })
                .collect::<Vec<_>>(),
            OpeningFormat::Pgn => split_pgn_games(&text)
                .iter()
                .filter_map(|game| parse_pgn_game(game, book.plies.max(1) as usize))
                .collect(),
        };
        if book.order == OpeningOrder::Random {
            NamedRng::new(master_seed, stream_names::OPENING_ORDER)
                .unwrap()
                .shuffle(&mut openings);
        }
        if let Some(count) = book.count {
            openings.truncate(count.max(1) as usize);
        }
        openings
    }

    /// Distinct legal positions: walk a few pseudo-random legal moves from the
    /// start position for each line.
    fn synthetic_epd(lines: usize) -> String {
        let mut text = String::with_capacity(lines * 64);
        let mut state = 0x2545_F491_4F6C_DD1D_u64;
        for index in 0..lines {
            let mut position = Chess::default();
            for _ in 0..(4 + index % 5) {
                let moves = position.legal_moves();
                if moves.is_empty() {
                    break;
                }
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let chosen = moves[(state % moves.len() as u64) as usize];
                position.play_unchecked(chosen);
            }
            let fen = Fen::from_position(&position, EnPassantMode::Legal).to_string();
            let epd = fen.split_whitespace().take(4).collect::<Vec<_>>().join(" ");
            text.push_str(&epd);
            if index % 97 == 0 {
                text.push_str(" bm e4; id \"line\";");
            }
            text.push('\n');
            if index % 1_001 == 0 {
                text.push_str("# a comment, é and all\n\nnot a position\n");
            }
        }
        text
    }

    #[test]
    fn the_compact_book_gives_the_same_openings_in_the_same_order() {
        // Large enough to be parsed in parallel chunks.
        let path = write_temp("identity.epd", &synthetic_epd(60_000));
        for (order, count) in [
            (OpeningOrder::Sequential, None),
            (OpeningOrder::Random, None),
            (OpeningOrder::Random, Some(1_000)),
        ] {
            let mut book = OpeningBook::new(path.clone());
            book.order = order;
            book.count = count;
            let compact = load_openings_named(&book, 42).unwrap();
            let reference = owned_reference(&book, 42);
            assert_eq!(compact.len(), reference.len());
            assert_eq!(compact.into_resolved(), reference, "{order:?} {count:?}");
        }
        let pgn = "[Event \"a\"]\n\n1. e4 e5 2. Nf3 Nc6 1-0\n\n[Event \"b\"]\n[FEN \"8/8/8/8/8/8/K7/7k w - - 0 1\"]\n\n1. Ka3 Kg1 1/2-1/2\n\n[Event \"c\"]\n\n1. d4 d5 0-1\n";
        let mut book = OpeningBook::new(write_temp("identity.pgn", pgn));
        book.format = OpeningFormat::Pgn;
        book.plies = 3;
        book.order = OpeningOrder::Random;
        assert_eq!(
            load_openings_named(&book, 7).unwrap().into_resolved(),
            owned_reference(&book, 7)
        );
    }

    /// A book of 2.6 million lines loads in well under a second in an
    /// optimised build; a debug build checks a tenth of it against a bound
    /// scaled for unoptimised position validation.
    #[test]
    fn a_large_epd_book_loads_quickly() {
        let (lines, limit) = if cfg!(debug_assertions) {
            (260_000, std::time::Duration::from_secs(10))
        } else {
            (2_600_000, std::time::Duration::from_secs(1))
        };
        let path = write_temp("large.epd", &synthetic_epd(lines));
        let book = OpeningBook::new(path.clone());
        let started = std::time::Instant::now();
        let openings = load_openings_named(&book, 0).unwrap();
        let elapsed = started.elapsed();
        eprintln!("{lines} EPD lines loaded in {elapsed:?}");
        assert_eq!(openings.len(), lines);
        assert!(elapsed < limit, "{lines} lines loaded in {elapsed:?}");
        let _ = std::fs::remove_file(path);
    }

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
    fn strict_audit_accounts_for_rejected_candidates() {
        let path = write_temp(
            "audit.epd",
            "8/8/8/8/8/8/K7/7k w - -\nnot a valid epd\n# ignored\n",
        );
        let book = OpeningBook::new(path);
        let audit = audit_opening_book(&book).unwrap();
        assert_eq!(audit.candidates, 2);
        assert_eq!(audit.usable, 1);
        assert_eq!(audit.rejected_indices, [2]);
        assert!(!audit.valid());
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
    fn cli_named_random_order_is_reproducible_from_the_master_seed() {
        let epd = "\
8/8/8/8/8/8/8/4K2k w - -
rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -
r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq -
rnbqkb1r/pppppppp/5n2/8/8/5N2/PPPPPPPP/RNBQKB1R w KQkq -
";
        let path = write_temp("named-order.epd", epd);
        let mut book = OpeningBook::new(path);
        book.order = OpeningOrder::Random;
        let first = load_openings_named(&book, 42).unwrap();
        let repeated = load_openings_named(&book, 42).unwrap();
        assert_eq!(first, repeated);
        assert_ne!(
            first,
            load_openings_named(&book, 43).unwrap(),
            "the reviewed fixture exercises the seed"
        );
    }

    #[test]
    fn missing_or_empty_book_errors() {
        let path = write_temp("empty.epd", "\n# only a comment\n");
        let book = OpeningBook::new(path);
        assert!(load_openings(&book).is_err());
    }
}
