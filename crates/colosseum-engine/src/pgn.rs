//! Minimal PGN writer: a seven-tag roster (plus a few useful extras) and SAN
//! movetext wrapped to a sensible width. Movetext numbering honours a FEN start
//! (its fullmove number and side to move), so openings that begin mid-game or with
//! Black to move are numbered correctly.

use colosseum_core::{GameResult, Termination};

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
}

/// Render a complete PGN game (header + movetext + result token).
#[must_use]
pub fn build_pgn(tags: &PgnTags, san_moves: &[String]) -> String {
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

    let (start_move, black_first) = fen_move_context(tags.fen.as_deref());

    out.push('\n');
    out.push_str(&movetext(san_moves, tags.result, start_move, black_first));
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
    result: GameResult,
    start_move: u32,
    black_first: bool,
) -> String {
    const WRAP: usize = 80;
    let mut tokens: Vec<String> = Vec::with_capacity(san_moves.len() + san_moves.len() / 2);
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
        };
        let pgn = build_pgn(&tags, &["e4".into(), "e5".into(), "Qh5".into()]);
        assert!(pgn.contains("[White \"Stockfish\"]"));
        assert!(pgn.contains("[Result \"1-0\"]"));
        assert!(pgn.contains("[Termination \"normal\"]"));
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
        };
        let pgn = build_pgn(&tags, &["Nc6".into(), "Bb5".into(), "a6".into()]);
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
        };
        let pgn = build_pgn(&tags, &[]);
        assert!(pgn.contains("[White \"Engine \\\"X\\\"\"]"));
        assert!(!pgn.contains("[TimeControl"));
    }
}
