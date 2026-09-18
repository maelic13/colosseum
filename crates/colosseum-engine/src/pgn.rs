//! Minimal PGN writer: a seven-tag roster (plus a few useful extras) and SAN
//! movetext wrapped to a sensible width. Movetext numbering honours a FEN start
//! (its fullmove number and side to move), so openings that begin mid-game or with
//! Black to move are numbered correctly.

use colosseum_core::{GameResult, Termination};

/// The version of the per-move comment form written by [`build_pgn`].
///
/// A reader that knows this identifier knows exactly which fields a comment
/// can contain and how they are spelled, so a PGN taken months apart stays
/// interpretable. It is recorded in every run record.
pub const PGN_ANNOTATION_WRITER: &str = "colosseum-move-comment/2";

/// A score as the mover reported it, from the mover's own point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationScore {
    /// Signed centipawns.
    Centipawns(i32),
    /// Mate in `n` moves; negative when the mover is the one being mated.
    MateIn(i32),
}

impl AnnotationScore {
    fn render(self) -> String {
        match self {
            Self::Centipawns(cp) => cp.to_string(),
            Self::MateIn(moves) => format!("#{moves}"),
        }
    }
}

/// The search evidence behind one engine move.
///
/// Every field the engine did not report is `None` and is omitted from the
/// comment. It is never written as zero, because a reported zero and an
/// unreported value are different facts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchAnnotation {
    pub score: Option<AnnotationScore>,
    pub depth: Option<u32>,
    /// Harness-charged elapsed milliseconds, per the recorded clock model.
    pub time_ms: Option<u64>,
    /// Harness overhead: the charged milliseconds minus the time the engine
    /// reported. Absent when the engine reported no time.
    pub overhead_ms: Option<i64>,
    pub nodes: Option<u64>,
}

/// What a single half-move carries as its PGN comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveAnnotation {
    /// Pre-played from the opening book; no engine searched it.
    Book,
    /// Played by an engine, with whatever it reported.
    Search(SearchAnnotation),
}

impl MoveAnnotation {
    /// Render `{book}` or `{s=… d=… t=…ms h=…ms n=…}`, or nothing when an
    /// engine move carries no reported field at all.
    fn render(self) -> Option<String> {
        match self {
            Self::Book => Some("{book}".into()),
            Self::Search(search) => {
                let mut fields = Vec::new();
                if let Some(score) = search.score {
                    fields.push(format!("s={}", score.render()));
                }
                if let Some(depth) = search.depth {
                    fields.push(format!("d={depth}"));
                }
                if let Some(time_ms) = search.time_ms {
                    fields.push(format!("t={time_ms}ms"));
                }
                if let Some(overhead_ms) = search.overhead_ms {
                    fields.push(format!("h={overhead_ms}ms"));
                }
                if let Some(nodes) = search.nodes {
                    fields.push(format!("n={nodes}"));
                }
                (!fields.is_empty()).then(|| format!("{{{}}}", fields.join(" ")))
            }
        }
    }
}

/// What a written game needs to be placed back into its schedule without the
/// run directory it came from.
///
/// The seven-tag roster says who played and how it ended; it cannot say which
/// colour-reversed pair a game belongs to, and that is the unit every paired
/// statistic is computed over. Carrying it in the export is what lets a PGN
/// alone reproduce the pentanomial vector the checkpoint holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GamePairIdentity {
    /// Harness game number in schedule order, counting from one.
    pub game_number: u32,
    /// The colour-reversed pair, or the tournament encounter, counting from one.
    pub pair_number: u32,
    /// Which colour assignment of that pair this game is: 1 or 2.
    pub pair_game: u32,
    /// Zero-based index into the resolved opening order; absent without a book.
    pub opening_index: Option<usize>,
    /// The opening's label, `startpos` when no book supplied one.
    pub opening_label: String,
}

/// The tag data needed to render a game's PGN header.
#[derive(Debug, Clone)]
pub struct PgnTags {
    pub event: String,
    pub site: String,
    pub date: String, // "YYYY.MM.DD"
    pub round: u32,
    pub white: String,
    pub black: String,
    pub result: GameResult,
    pub time_control: String,
    pub termination: Option<Termination>,
    /// Set for non-standard start positions (adds `SetUp`/`FEN` tags).
    pub fen: Option<String>,
    /// Pre-played opening half-moves, excluded from search telemetry.
    pub opening_plies: u32,
    /// Schedule identity, for exports that can supply it.
    pub identity: Option<GamePairIdentity>,
    /// The time margins, White's then Black's, in milliseconds: how far a
    /// move's charged time may exceed the clock before it forfeits. With the
    /// per-move `h=`, they say how close each move came.
    pub time_margins_ms: Option<[u64; 2]>,
}

/// Render a complete PGN game (header + movetext + result token).
///
/// `annotations` is parallel to `san_moves`; a shorter list simply leaves the
/// remaining moves uncommented, so a caller with no evidence passes `&[]`.
#[must_use]
pub fn build_pgn(tags: &PgnTags, san_moves: &[String], annotations: &[MoveAnnotation]) -> String {
    let mut out = String::new();
    let mut tag = |key: &str, value: &str| {
        // Escape backslashes and quotes per the PGN spec.
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        out.push_str(&format!("[{key} \"{escaped}\"]\n"));
    };

    tag("Event", &tags.event);
    tag("Site", &tags.site);
    tag("Date", &tags.date);
    tag("Round", &tags.round.to_string());
    tag("White", &tags.white);
    tag("Black", &tags.black);
    tag("Result", tags.result.pgn());
    if !tags.time_control.is_empty() {
        tag("TimeControl", &tags.time_control);
    }
    if let Some(term) = tags.termination {
        tag("Termination", termination_tag(term));
    }
    if let Some(fen) = &tags.fen {
        tag("SetUp", "1");
        tag("FEN", fen);
    }
    if tags.opening_plies > 0 {
        tag("OpeningPlyCount", &tags.opening_plies.to_string());
    }
    if let Some(identity) = &tags.identity {
        tag("GameNumber", &identity.game_number.to_string());
        tag("PairNumber", &identity.pair_number.to_string());
        tag("PairGame", &identity.pair_game.to_string());
        if let Some(index) = identity.opening_index {
            tag("OpeningIndex", &index.to_string());
        }
        tag("OpeningLabel", &identity.opening_label);
    }
    if let Some([white, black]) = tags.time_margins_ms {
        tag("WhiteTimeMarginMs", &white.to_string());
        tag("BlackTimeMarginMs", &black.to_string());
    }

    let (start_move, black_first) = fen_move_context(tags.fen.as_deref());

    out.push('\n');
    out.push_str(&movetext(
        san_moves,
        annotations,
        tags.result,
        start_move,
        black_first,
    ));
    out.push('\n');
    out
}

/// Derive `(starting fullmove number, black-to-move-first)` from a start FEN.
/// Defaults to `(1, false)` for the standard start position or an unparsable FEN.
fn fen_move_context(fen: Option<&str>) -> (u32, bool) {
    let Some(fen) = fen else {
        return (1, false);
    };
    let fields: Vec<&str> = fen.split_whitespace().collect();
    let black_first = fields.get(1).is_some_and(|s| *s == "b");
    let start_move = fields
        .get(5)
        .and_then(|s| s.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1);
    (start_move, black_first)
}

/// Build wrapped movetext ending with the result token, numbering from
/// `start_move` and accounting for whether Black moves first.
fn movetext(
    san_moves: &[String],
    annotations: &[MoveAnnotation],
    result: GameResult,
    start_move: u32,
    black_first: bool,
) -> String {
    const WRAP: usize = 80;
    let mut tokens: Vec<String> = Vec::with_capacity(san_moves.len() * 3);
    let mut move_no = start_move;
    let mut white_to_move = !black_first;
    for (ply, san) in san_moves.iter().enumerate() {
        if white_to_move {
            tokens.push(format!("{move_no}."));
        } else if ply == 0 {
            // Black moves first from this start position: "N..." prefix.
            tokens.push(format!("{move_no}..."));
        }
        tokens.push(san.clone());
        if let Some(comment) = annotations
            .get(ply)
            .copied()
            .and_then(MoveAnnotation::render)
        {
            tokens.push(comment);
        }
        if !white_to_move {
            move_no += 1;
        }
        white_to_move = !white_to_move;
    }
    tokens.push(result.pgn().to_string());

    let mut lines = String::new();
    let mut line = String::new();
    for token in tokens {
        if !line.is_empty() && line.len() + 1 + token.len() > WRAP {
            lines.push_str(&line);
            lines.push('\n');
            line.clear();
        }
        if line.is_empty() {
            line.push_str(&token);
        } else {
            line.push(' ');
            line.push_str(&token);
        }
    }
    if !line.is_empty() {
        lines.push_str(&line);
        lines.push('\n');
    }
    lines
}

/// Map a [`Termination`] to a PGN `Termination` tag value.
fn termination_tag(termination: Termination) -> &'static str {
    match termination {
        Termination::TimeForfeit => "time forfeit",
        Termination::EngineCrash | Termination::Aborted => "abandoned",
        Termination::IllegalMove => "rules infraction",
        Termination::AdjudicatedDraw | Termination::AdjudicatedResign | Termination::MaxMoves => {
            "adjudication"
        }
        _ => "normal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_header_and_movetext() {
        let tags = PgnTags {
            event: "Colosseum".into(),
            site: "Local".into(),
            date: "2026.06.08".into(),
            round: 1,
            white: "Stockfish".into(),
            black: "Basilisk".into(),
            result: GameResult::WhiteWin,
            time_control: "movetime/100ms".into(),
            termination: Some(Termination::Checkmate),
            fen: None,
            identity: None,
            opening_plies: 2,
            time_margins_ms: Some([20, 25]),
        };
        let pgn = build_pgn(&tags, &["e4".into(), "e5".into(), "Qh5".into()], &[]);
        assert!(pgn.contains("[White \"Stockfish\"]"));
        assert!(pgn.contains("[Result \"1-0\"]"));
        assert!(pgn.contains("[Termination \"normal\"]"));
        assert!(pgn.contains("[OpeningPlyCount \"2\"]"));
        assert!(pgn.contains("[WhiteTimeMarginMs \"20\"]"));
        assert!(pgn.contains("[BlackTimeMarginMs \"25\"]"));
        assert!(pgn.contains("1. e4 e5 2. Qh5"));
        assert!(pgn.trim_end().ends_with("1-0"));
    }

    #[test]
    fn fen_start_numbers_from_fullmove_and_black_first() {
        // Position after 1.e4 e5 2.Nf3 — Black to move, fullmove 2.
        let tags = PgnTags {
            event: "E".into(),
            site: "S".into(),
            date: "2026.01.01".into(),
            round: 1,
            white: "W".into(),
            black: "B".into(),
            result: GameResult::Draw,
            time_control: String::new(),
            termination: None,
            fen: Some("rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2".into()),
            identity: None,
            opening_plies: 0,
            time_margins_ms: None,
        };
        let pgn = build_pgn(&tags, &["Nc6".into(), "Bb5".into(), "a6".into()], &[]);
        // Black moves first at move 2, then White's move 3, then Black's move 3.
        assert!(pgn.contains("2... Nc6 3. Bb5 a6"));
        assert!(pgn.contains("[FEN \""));
        assert!(pgn.contains("[SetUp \"1\"]"));
    }

    #[test]
    fn escapes_quotes_in_names() {
        let tags = PgnTags {
            event: "E".into(),
            site: "S".into(),
            date: "2026.01.01".into(),
            round: 1,
            white: "Engine \"X\"".into(),
            black: "Y".into(),
            result: GameResult::Draw,
            time_control: String::new(),
            termination: None,
            fen: None,
            identity: None,
            opening_plies: 0,
            time_margins_ms: None,
        };
        let pgn = build_pgn(&tags, &[], &[]);
        assert!(pgn.contains("[White \"Engine \\\"X\\\"\"]"));
        assert!(!pgn.contains("[TimeControl"));
        assert!(!pgn.contains("MarginMs"));
    }

    #[test]
    fn a_search_comment_carries_the_overhead_beside_the_charged_time() {
        let search = |overhead_ms| {
            MoveAnnotation::Search(SearchAnnotation {
                score: Some(AnnotationScore::Centipawns(12)),
                depth: Some(9),
                time_ms: Some(57),
                overhead_ms,
                nodes: Some(1_000),
            })
            .render()
            .unwrap()
        };
        assert_eq!(search(Some(4)), "{s=12 d=9 t=57ms h=4ms n=1000}");
        assert_eq!(search(Some(-1)), "{s=12 d=9 t=57ms h=-1ms n=1000}");
        assert_eq!(search(None), "{s=12 d=9 t=57ms n=1000}");
    }
}
