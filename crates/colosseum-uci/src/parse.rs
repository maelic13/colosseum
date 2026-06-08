//! Pure parsers for the UCI lines we care about: `option`, `info`, and `bestmove`.
//! Kept free of I/O so they can be unit-tested exhaustively against canned strings.

use colosseum_core::UciOption;

use crate::score::Score;

const VALUE_KEYWORDS: [&str; 4] = ["default", "min", "max", "var"];

/// Parse an `option name ... type ...` line into a [`UciOption`]. Returns `None` if
/// the line is not a well-formed option declaration.
#[must_use]
pub fn parse_option_line(line: &str) -> Option<UciOption> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let mut i = 0;
    if tokens.first() != Some(&"option") {
        return None;
    }
    i += 1;
    if tokens.get(i) != Some(&"name") {
        return None;
    }
    i += 1;

    // Name runs until the `type` keyword (names may contain spaces).
    let name_start = i;
    while i < tokens.len() && tokens[i] != "type" {
        i += 1;
    }
    if i >= tokens.len() {
        return None; // no `type`
    }
    let name = tokens[name_start..i].join(" ");
    i += 1; // skip `type`
    let option_type = *tokens.get(i)?;
    i += 1;

    let mut default: Option<String> = None;
    let mut min: Option<i64> = None;
    let mut max: Option<i64> = None;
    let mut vars: Vec<String> = Vec::new();

    while i < tokens.len() {
        match tokens[i] {
            "default" => {
                let (value, next) = read_value(&tokens, i + 1);
                default = Some(value);
                i = next;
            }
            "var" => {
                let (value, next) = read_value(&tokens, i + 1);
                vars.push(value);
                i = next;
            }
            "min" => {
                min = tokens.get(i + 1).and_then(|t| t.parse().ok());
                i += 2;
            }
            "max" => {
                max = tokens.get(i + 1).and_then(|t| t.parse().ok());
                i += 2;
            }
            _ => i += 1, // ignore anything unexpected
        }
    }

    match option_type {
        "check" => Some(UciOption::Check {
            name,
            default: default
                .as_deref()
                .is_some_and(|d| d.eq_ignore_ascii_case("true")),
        }),
        "spin" => Some(UciOption::Spin {
            name,
            default: default.and_then(|d| d.parse().ok()).unwrap_or(0),
            min: min.unwrap_or(i64::MIN),
            max: max.unwrap_or(i64::MAX),
        }),
        "combo" => Some(UciOption::Combo {
            name,
            default: default.unwrap_or_default(),
            vars,
        }),
        "button" => Some(UciOption::Button { name }),
        "string" => Some(UciOption::Str {
            name,
            // Engines use the literal `<empty>` to mean an empty string.
            default: default.filter(|d| d != "<empty>").unwrap_or_default(),
        }),
        _ => None,
    }
}

/// Collect a (possibly multi-word) value starting at `start`, stopping at the next
/// value keyword or end of line. Returns the joined value and the next index.
fn read_value(tokens: &[&str], start: usize) -> (String, usize) {
    let mut i = start;
    while i < tokens.len() && !VALUE_KEYWORDS.contains(&tokens[i]) {
        i += 1;
    }
    (tokens[start..i].join(" "), i)
}

/// The fields of an `info` line we track.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InfoLine {
    pub depth: Option<u32>,
    pub score: Option<Score>,
    pub nps: Option<u64>,
}

/// Parse an `info ...` line, extracting depth/score/nps. Returns `None` for non-info
/// lines; an `info string ...` engine message yields an all-`None` [`InfoLine`].
#[must_use]
pub fn parse_info_line(line: &str) -> Option<InfoLine> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.first() != Some(&"info") {
        return None;
    }
    let mut info = InfoLine::default();
    if tokens.get(1) == Some(&"string") {
        return Some(info); // human-readable message, nothing to extract
    }

    let mut i = 1;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => {
                info.depth = tokens.get(i + 1).and_then(|t| t.parse().ok());
                i += 2;
            }
            "nps" => {
                info.nps = tokens.get(i + 1).and_then(|t| t.parse().ok());
                i += 2;
            }
            "score" => match tokens.get(i + 1).copied() {
                Some("cp") => {
                    info.score = tokens
                        .get(i + 2)
                        .and_then(|t| t.parse().ok())
                        .map(Score::Cp);
                    i += 3;
                }
                Some("mate") => {
                    info.score = tokens
                        .get(i + 2)
                        .and_then(|t| t.parse().ok())
                        .map(Score::Mate);
                    i += 3;
                }
                _ => i += 1,
            },
            // The principal variation is the rest of the line; stop scanning.
            "pv" => break,
            _ => i += 1,
        }
    }
    Some(info)
}

/// Parse a `bestmove <move> [ponder <move>]` line, returning the best move token
/// (e.g. `e2e4`, `0000`, or `(none)`).
#[must_use]
pub fn parse_bestmove(line: &str) -> Option<String> {
    let mut tokens = line.split_whitespace();
    if tokens.next() != Some("bestmove") {
        return None;
    }
    tokens.next().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spin_option_with_range() {
        let opt =
            parse_option_line("option name Threads type spin default 1 min 1 max 1024").unwrap();
        assert_eq!(
            opt,
            UciOption::Spin {
                name: "Threads".into(),
                default: 1,
                min: 1,
                max: 1024,
            }
        );
    }

    #[test]
    fn parses_check_option() {
        let opt = parse_option_line("option name Ponder type check default false").unwrap();
        assert_eq!(
            opt,
            UciOption::Check {
                name: "Ponder".into(),
                default: false,
            }
        );
    }

    #[test]
    fn parses_multiword_name_and_combo() {
        let opt =
            parse_option_line("option name Analysis Contempt type combo default Both var Off var White var Black var Both")
                .unwrap();
        assert_eq!(
            opt,
            UciOption::Combo {
                name: "Analysis Contempt".into(),
                default: "Both".into(),
                vars: vec!["Off".into(), "White".into(), "Black".into(), "Both".into()],
            }
        );
    }

    #[test]
    fn parses_string_option_and_empty_sentinel() {
        let opt = parse_option_line("option name SyzygyPath type string default <empty>").unwrap();
        assert_eq!(
            opt,
            UciOption::Str {
                name: "SyzygyPath".into(),
                default: String::new(),
            }
        );
        let opt2 =
            parse_option_line("option name NNUEFile type string default nn-abc.nnue").unwrap();
        assert_eq!(
            opt2,
            UciOption::Str {
                name: "NNUEFile".into(),
                default: "nn-abc.nnue".into(),
            }
        );
    }

    #[test]
    fn parses_button_option() {
        let opt = parse_option_line("option name Clear Hash type button").unwrap();
        assert_eq!(
            opt,
            UciOption::Button {
                name: "Clear Hash".into()
            }
        );
    }

    #[test]
    fn rejects_non_option_lines() {
        assert!(parse_option_line("id name Stockfish").is_none());
        assert!(parse_option_line("option name Foo").is_none()); // no type
    }

    #[test]
    fn parses_info_score_and_nps() {
        let info = parse_info_line(
            "info depth 20 seldepth 28 score cp 34 nodes 1000000 nps 5000000 time 200 pv e2e4 e7e5",
        )
        .unwrap();
        assert_eq!(info.depth, Some(20));
        assert_eq!(info.score, Some(Score::Cp(34)));
        assert_eq!(info.nps, Some(5_000_000));
    }

    #[test]
    fn parses_info_mate_score() {
        let info = parse_info_line("info depth 30 score mate -3 nps 12345 pv a1a2").unwrap();
        assert_eq!(info.score, Some(Score::Mate(-3)));
        assert_eq!(info.nps, Some(12_345));
    }

    #[test]
    fn info_string_message_is_empty() {
        let info = parse_info_line("info string NNUE evaluation using nn-abc.nnue").unwrap();
        assert_eq!(info, InfoLine::default());
        assert!(parse_info_line("bestmove e2e4").is_none());
    }

    #[test]
    fn parses_bestmove_with_and_without_ponder() {
        assert_eq!(
            parse_bestmove("bestmove e2e4 ponder e7e5").as_deref(),
            Some("e2e4")
        );
        assert_eq!(parse_bestmove("bestmove (none)").as_deref(), Some("(none)"));
        assert!(parse_bestmove("info depth 1").is_none());
    }
}
