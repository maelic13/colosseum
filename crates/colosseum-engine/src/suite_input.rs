use colosseum_application::{MalformedSuitePosition, SuiteEntry, SuiteExpectation, SuitePosition};
use shakmaty::fen::Fen;
use shakmaty::san::San;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuiteInputFormat {
    Epd,
    Fen,
}

#[must_use]
pub fn parse_suite_input(text: &str, format: SuiteInputFormat) -> Vec<SuiteEntry> {
    text.lines()
        .enumerate()
        .filter_map(|(offset, source)| {
            let line = source.trim();
            if line.is_empty() || line.starts_with('#') {
                None
            } else {
                let index = u32::try_from(offset + 1).unwrap_or(u32::MAX);
                Some(match format {
                    SuiteInputFormat::Epd => parse_epd_line(index, line),
                    SuiteInputFormat::Fen => parse_fen_line(index, line),
                })
            }
        })
        .collect()
}

fn parse_epd_line(index: u32, source: &str) -> SuiteEntry {
    match parse_epd(index, source) {
        Ok(position) => SuiteEntry::Position(position),
        Err(reason) => malformed(index, source, reason),
    }
}

fn parse_epd(index: u32, source: &str) -> Result<SuitePosition, String> {
    let mut segments = source.split(';');
    let first = segments.next().unwrap_or_default().trim();
    let fields = first.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 4 {
        return Err("EPD requires board, side, castling and en-passant fields".into());
    }
    let fen = format!(
        "{} {} {} {} 0 1",
        fields[0], fields[1], fields[2], fields[3]
    );
    let position = parse_position(&fen)?;
    let mut operations = Vec::new();
    if fields.len() > 4 {
        operations.push(fields[4..].join(" "));
    }
    operations.extend(
        segments
            .map(str::trim)
            .filter(|operation| !operation.is_empty())
            .map(str::to_owned),
    );

    let mut best = None;
    let mut avoid = None;
    let mut id = None;
    let mut unknown = Vec::new();
    for operation in operations {
        let (opcode, operands) = operation
            .split_once(char::is_whitespace)
            .map_or((operation.as_str(), ""), |(opcode, operands)| {
                (opcode, operands.trim())
            });
        match opcode {
            "bm" => {
                if best.is_some() {
                    return Err("duplicate bm operation".into());
                }
                best = Some(parse_moves(&position, operands, "bm")?);
            }
            "am" => {
                if avoid.is_some() {
                    return Err("duplicate am operation".into());
                }
                avoid = Some(parse_moves(&position, operands, "am")?);
            }
            "id" => {
                id = Some(operands.trim_matches('"').to_owned());
            }
            _ => unknown.push(operation),
        }
    }
    if best.is_some() && avoid.is_some() {
        return Err("an EPD entry cannot combine bm and am expectations".into());
    }
    Ok(SuitePosition {
        index,
        id: id
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| format!("line {index}")),
        fen,
        expectation: if let Some(moves) = best {
            SuiteExpectation::Best(moves)
        } else if let Some(moves) = avoid {
            SuiteExpectation::Avoid(moves)
        } else {
            SuiteExpectation::None
        },
        unknown_operations: unknown,
    })
}

fn parse_fen_line(index: u32, source: &str) -> SuiteEntry {
    match parse_position(source) {
        Ok(_) => SuiteEntry::Position(SuitePosition {
            index,
            id: format!("line {index}"),
            fen: source.into(),
            expectation: SuiteExpectation::None,
            unknown_operations: Vec::new(),
        }),
        Err(reason) => malformed(index, source, reason),
    }
}

fn parse_position(fen: &str) -> Result<Chess, String> {
    fen.parse::<Fen>()
        .map_err(|error| format!("invalid position: {error}"))?
        .into_position(CastlingMode::Standard)
        .map_err(|error| format!("illegal position: {error}"))
}

fn parse_moves(position: &Chess, operands: &str, opcode: &str) -> Result<Vec<String>, String> {
    let tokens = operands.split_whitespace().collect::<Vec<_>>();
    if tokens.is_empty() {
        return Err(format!("{opcode} requires at least one legal move"));
    }
    tokens
        .into_iter()
        .map(|token| {
            if let Ok(san) = token.parse::<San>() {
                let chess_move = san
                    .to_move(position)
                    .map_err(|error| format!("illegal {opcode} move {token:?}: {error}"))?;
                return Ok(chess_move.to_uci(CastlingMode::Standard).to_string());
            }
            let uci = token
                .parse::<UciMove>()
                .map_err(|error| format!("invalid {opcode} move {token:?}: {error}"))?;
            let chess_move = uci
                .to_move(position)
                .map_err(|error| format!("illegal {opcode} move {token:?}: {error}"))?;
            Ok(chess_move.to_uci(CastlingMode::Standard).to_string())
        })
        .collect()
}

fn malformed(index: u32, source: &str, reason: String) -> SuiteEntry {
    SuiteEntry::Malformed(MalformedSuitePosition {
        index,
        source: source.into(),
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -";

    #[test]
    fn epd_converts_multiple_san_expectations_and_preserves_unknown_operations() {
        let entries = parse_suite_input(
            &format!("{START} bm e4 d4; id \"multi\"; ce 10;\n{START} am e4 d4;\n{START};\n"),
            SuiteInputFormat::Epd,
        );
        let SuiteEntry::Position(first) = &entries[0] else {
            panic!()
        };
        assert_eq!(first.id, "multi");
        assert_eq!(
            first.expectation,
            SuiteExpectation::Best(vec!["e2e4".into(), "d2d4".into()])
        );
        assert_eq!(first.unknown_operations, vec!["ce 10"]);
        let SuiteEntry::Position(second) = &entries[1] else {
            panic!()
        };
        assert!(matches!(second.expectation, SuiteExpectation::Avoid(_)));
        let SuiteEntry::Position(third) = &entries[2] else {
            panic!()
        };
        assert_eq!(third.expectation, SuiteExpectation::None);
    }

    #[test]
    fn malformed_and_fen_inputs_have_deterministic_entries() {
        let malformed = parse_suite_input(
            &format!("{START} bm Ke9;\nnot a position\n"),
            SuiteInputFormat::Epd,
        );
        assert!(
            malformed
                .iter()
                .all(|entry| matches!(entry, SuiteEntry::Malformed(_)))
        );
        let fen = parse_suite_input("8/8/8/8/8/8/K7/7k w - - 0 1\n", SuiteInputFormat::Fen);
        assert!(matches!(fen[0], SuiteEntry::Position(_)));
    }
}
