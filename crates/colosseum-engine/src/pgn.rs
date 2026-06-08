//! Minimal PGN writer: a seven-tag roster (plus a few useful extras) and SAN
//! movetext wrapped to a sensible width. v1 numbers from the standard start position;
//! FEN-start numbering is refined when opening books arrive (Step 10).

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

    out.push('\n');
    out.push_str(&movetext(san_moves, tags.result));
    out.push('\n');
    out
}

/// Build wrapped movetext ending with the result token.
fn movetext(san_moves: &[String], result: GameResult) -> String {
    const WRAP: usize = 80;
    let mut tokens: Vec<String> = Vec::with_capacity(san_moves.len() + san_moves.len() / 2);
    for (ply, san) in san_moves.iter().enumerate() {
        if ply % 2 == 0 {
            tokens.push(format!("{}.", ply / 2 + 1));
        }
        tokens.push(san.clone());
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
