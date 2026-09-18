use std::collections::BTreeMap;

use serde::Serialize;

pub const SUPPORTED_TELEMETRY_SYNTAXES: [&str; 3] = [
    "PGN tags: [%depth N] [%emt SECONDS] [%nodes N]",
    "key/value comments: depth|d=N time|t=Nms|Ns nodes|n=N",
    "Colosseum move comments: {s=CP|#N d=N t=Nms h=Nms n=N}, {book} and {forfeit [s=CP|#N] [d=N] t=Nms [h=Nms] [n=N]} or {forfeit}; WhiteTimeMarginMs/BlackTimeMarginMs tags",
];

const NODE_SEMANTICS_WARNING: &str = "implied NPS is comparable only when node accounting has compatible semantics, normally within the same engine lineage";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchTelemetryReport {
    pub status: &'static str,
    pub supported_syntaxes: [&'static str; 3],
    pub opening_exclusion: &'static str,
    pub excluded_opening_moves: u32,
    pub node_semantics_warning: &'static str,
    pub engines: Vec<EngineTelemetryReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EngineTelemetryReport {
    pub engine: String,
    pub eligible_moves: u32,
    pub annotated_moves: u32,
    pub annotation_coverage: f64,
    pub depth: TelemetryMetric,
    pub elapsed_seconds: TelemetryMetric,
    pub nodes: TelemetryMetric,
    pub implied_nps: TelemetryMetric,
    /// Reported score in centipawns from the mover's point of view. A mate
    /// score is a claim about distance to mate, not an evaluation, so it is
    /// counted as covered but excluded from the centipawn statistics.
    pub score_cp: TelemetryMetric,
    /// Mean |score|, the usual way to read how decided the games looked.
    pub mean_absolute_score_cp: TelemetryMetric,
    /// Harness overhead per move (`h=`): charged time minus the time the
    /// engine reported, in milliseconds.
    pub harness_overhead_ms: OverheadMetric,
}

/// The distribution of per-move harness overhead, and how often it exceeded
/// the time margin: the moves that would have forfeited had the engine used
/// its whole clock.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OverheadMetric {
    pub status: &'static str,
    pub samples: u32,
    pub eligible_moves: u32,
    pub coverage: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p50: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p99: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p999: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// Moves with an overhead whose game named this side's time margin.
    pub moves_with_margin: u32,
    /// Of those, the moves whose overhead exceeded the margin.
    pub over_margin: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TelemetryMetric {
    pub status: &'static str,
    pub samples: u32,
    pub eligible_moves: u32,
    pub coverage: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median: Option<f64>,
}

#[derive(Debug, Default, Clone, Copy)]
struct MoveTelemetry {
    depth: Option<f64>,
    elapsed_seconds: Option<f64>,
    nodes: Option<f64>,
    score_cp: Option<f64>,
    overhead_ms: Option<f64>,
    mate_score: bool,
    is_book: bool,
}

#[derive(Debug)]
struct ParsedMove {
    white: bool,
    telemetry: MoveTelemetry,
    /// Not a move: the search that lost on time without playing one. It
    /// counts towards the overhead of the side that forfeited and nothing
    /// else.
    forfeit: bool,
}

#[derive(Debug, Default)]
struct EngineSamples {
    eligible: u32,
    annotated: u32,
    depth: Vec<f64>,
    elapsed: Vec<f64>,
    nodes: Vec<f64>,
    implied_nps: Vec<f64>,
    score_cp: Vec<f64>,
    absolute_score_cp: Vec<f64>,
    mate_scores: u32,
    overhead_ms: Vec<f64>,
    moves_with_margin: u32,
    over_margin: u32,
}

pub fn unavailable(reason: impl Into<String>) -> SearchTelemetryReport {
    SearchTelemetryReport {
        status: "unavailable",
        supported_syntaxes: SUPPORTED_TELEMETRY_SYNTAXES,
        opening_exclusion: "OpeningPlyCount tag, then per-move book comments",
        excluded_opening_moves: 0,
        node_semantics_warning: NODE_SEMANTICS_WARNING,
        engines: Vec::new(),
        unavailable_reason: Some(reason.into()),
    }
}

pub fn analyze_pgn(text: &str) -> SearchTelemetryReport {
    let mut by_engine = BTreeMap::<String, EngineSamples>::new();
    let mut excluded_opening_moves = 0_u32;
    for game in split_pgn(text) {
        let white = pgn_tag(game, "White").unwrap_or_else(|| "White".into());
        let black = pgn_tag(game, "Black").unwrap_or_else(|| "Black".into());
        let tagged_opening_plies = pgn_tag(game, "OpeningPlyCount")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let starts_white = pgn_tag(game, "FEN")
            .and_then(|fen| fen.split_whitespace().nth(1).map(|side| side != "b"))
            .unwrap_or(true);
        let margin = |tag| pgn_tag(game, tag).and_then(|value| value.parse::<f64>().ok());
        let white_margin = margin("WhiteTimeMarginMs");
        let black_margin = margin("BlackTimeMarginMs");
        for (index, parsed) in parse_mainline_moves(game, starts_white)
            .into_iter()
            .enumerate()
        {
            let engine = if parsed.white { &white } else { &black };
            if parsed.forfeit {
                let samples = by_engine.entry(engine.clone()).or_default();
                if let Some(overhead) = parsed.telemetry.overhead_ms {
                    samples.overhead_ms.push(overhead);
                    if let Some(margin) = if parsed.white {
                        white_margin
                    } else {
                        black_margin
                    } {
                        samples.moves_with_margin += 1;
                        samples.over_margin += u32::from(overhead > margin);
                    }
                }
                continue;
            }
            if index < tagged_opening_plies || parsed.telemetry.is_book {
                excluded_opening_moves = excluded_opening_moves.saturating_add(1);
                continue;
            }
            let samples = by_engine.entry(engine.clone()).or_default();
            samples.eligible += 1;
            if parsed.telemetry.depth.is_some()
                || parsed.telemetry.elapsed_seconds.is_some()
                || parsed.telemetry.nodes.is_some()
                || parsed.telemetry.score_cp.is_some()
                || parsed.telemetry.overhead_ms.is_some()
                || parsed.telemetry.mate_score
            {
                samples.annotated += 1;
            }
            if let Some(overhead) = parsed.telemetry.overhead_ms {
                samples.overhead_ms.push(overhead);
                if let Some(margin) = if parsed.white {
                    white_margin
                } else {
                    black_margin
                } {
                    samples.moves_with_margin += 1;
                    samples.over_margin += u32::from(overhead > margin);
                }
            }
            if parsed.telemetry.mate_score {
                samples.mate_scores += 1;
            }
            if let Some(value) = parsed.telemetry.score_cp {
                samples.score_cp.push(value);
                samples.absolute_score_cp.push(value.abs());
            }
            if let Some(value) = parsed.telemetry.depth {
                samples.depth.push(value);
            }
            if let Some(value) = parsed.telemetry.elapsed_seconds {
                samples.elapsed.push(value);
            }
            if let Some(value) = parsed.telemetry.nodes {
                samples.nodes.push(value);
            }
            if let (Some(nodes), Some(elapsed)) =
                (parsed.telemetry.nodes, parsed.telemetry.elapsed_seconds)
                && elapsed > 0.0
            {
                samples.implied_nps.push(nodes / elapsed);
            }
        }
    }

    let engines = by_engine
        .into_iter()
        .map(|(engine, samples)| EngineTelemetryReport {
            engine,
            eligible_moves: samples.eligible,
            annotated_moves: samples.annotated,
            annotation_coverage: fraction(samples.annotated, samples.eligible),
            depth: metric(samples.depth, samples.eligible),
            elapsed_seconds: metric(samples.elapsed, samples.eligible),
            nodes: metric(samples.nodes, samples.eligible),
            implied_nps: metric(samples.implied_nps, samples.eligible),
            score_cp: score_metric(samples.score_cp, samples.eligible, samples.mate_scores),
            mean_absolute_score_cp: score_metric(
                samples.absolute_score_cp,
                samples.eligible,
                samples.mate_scores,
            ),
            harness_overhead_ms: overhead_metric(
                samples.overhead_ms,
                samples.eligible,
                samples.moves_with_margin,
                samples.over_margin,
            ),
        })
        .collect::<Vec<_>>();
    let annotated = engines
        .iter()
        .map(|engine| engine.annotated_moves)
        .sum::<u32>();
    SearchTelemetryReport {
        status: if annotated > 0 { "available" } else { "unavailable" },
        supported_syntaxes: SUPPORTED_TELEMETRY_SYNTAXES,
        opening_exclusion: "OpeningPlyCount tag, then per-move book comments",
        excluded_opening_moves,
        node_semantics_warning: NODE_SEMANTICS_WARNING,
        engines,
        unavailable_reason: (annotated == 0).then(|| {
            "no supported post-opening search annotations were found; metrics are unavailable, not zero"
                .into()
        }),
    }
}

fn metric(mut values: Vec<f64>, eligible: u32) -> TelemetryMetric {
    values.sort_by(f64::total_cmp);
    let samples = values.len() as u32;
    let (mean, median) = if values.is_empty() {
        (None, None)
    } else {
        let middle = values.len() / 2;
        let median = if values.len().is_multiple_of(2) {
            (values[middle - 1] + values[middle]) / 2.0
        } else {
            values[middle]
        };
        (
            Some(values.iter().sum::<f64>() / values.len() as f64),
            Some(median),
        )
    };
    TelemetryMetric {
        status: if samples > 0 {
            "available"
        } else {
            "unavailable"
        },
        samples,
        eligible_moves: eligible,
        coverage: fraction(samples, eligible),
        mean,
        median,
    }
}

/// Nearest-rank quantiles of the per-move overhead: the value at or below
/// which that fraction of moves fell. With fewer samples than a quantile can
/// resolve it is the largest value, which is the honest answer.
fn overhead_metric(
    mut values: Vec<f64>,
    eligible: u32,
    moves_with_margin: u32,
    over_margin: u32,
) -> OverheadMetric {
    values.sort_by(f64::total_cmp);
    let quantile = |fraction: f64| {
        let rank = (fraction * values.len() as f64).ceil() as usize;
        values.get(rank.max(1) - 1).copied()
    };
    let samples = values.len() as u32;
    OverheadMetric {
        status: if samples > 0 {
            "available"
        } else {
            "unavailable"
        },
        samples,
        eligible_moves: eligible,
        coverage: fraction(samples, eligible),
        p50: quantile(0.5),
        p99: quantile(0.99),
        p999: quantile(0.999),
        max: values.last().copied(),
        moves_with_margin,
        over_margin,
    }
}

/// A score metric whose coverage counts mate scores, which are reported but
/// deliberately excluded from the centipawn values themselves.
fn score_metric(values: Vec<f64>, eligible: u32, mate_scores: u32) -> TelemetryMetric {
    let mut reported = metric(values, eligible);
    let covered = reported.samples.saturating_add(mate_scores);
    reported.coverage = fraction(covered, eligible);
    if reported.samples == 0 && mate_scores > 0 {
        reported.status = "unavailable";
    }
    reported
}

fn fraction(numerator: u32, denominator: u32) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        f64::from(numerator) / f64::from(denominator)
    }
}

fn parse_mainline_moves(game: &str, starts_white: bool) -> Vec<ParsedMove> {
    let movetext = game
        .lines()
        .filter(|line| !line.trim_start().starts_with('['))
        .collect::<Vec<_>>()
        .join("\n");
    let mut moves = Vec::new();
    let mut token = String::new();
    let mut chars = movetext.chars().peekable();
    let mut variation_depth = 0_u32;
    let mut white = starts_white;
    while let Some(character) = chars.next() {
        match character {
            '(' => {
                flush_move(&mut token, &mut moves, &mut white, variation_depth);
                variation_depth += 1;
            }
            ')' => {
                flush_move(&mut token, &mut moves, &mut white, variation_depth);
                variation_depth = variation_depth.saturating_sub(1);
            }
            '{' => {
                flush_move(&mut token, &mut moves, &mut white, variation_depth);
                let mut comment = String::new();
                for value in chars.by_ref() {
                    if value == '}' {
                        break;
                    }
                    comment.push(value);
                }
                if variation_depth == 0 && is_forfeit_comment(&comment) {
                    // The side to move searched and lost on time.
                    let mut telemetry = MoveTelemetry::default();
                    merge_comment(&mut telemetry, &comment);
                    moves.push(ParsedMove {
                        white,
                        telemetry,
                        forfeit: true,
                    });
                } else if variation_depth == 0
                    && let Some(last) = moves.last_mut()
                {
                    merge_comment(&mut last.telemetry, &comment);
                }
            }
            ';' => {
                flush_move(&mut token, &mut moves, &mut white, variation_depth);
                let mut comment = String::new();
                for value in chars.by_ref() {
                    if value == '\n' {
                        break;
                    }
                    comment.push(value);
                }
                if variation_depth == 0
                    && let Some(last) = moves.last_mut()
                {
                    merge_comment(&mut last.telemetry, &comment);
                }
            }
            value if value.is_whitespace() => {
                flush_move(&mut token, &mut moves, &mut white, variation_depth);
            }
            value if variation_depth == 0 => token.push(value),
            _ => {}
        }
    }
    flush_move(&mut token, &mut moves, &mut white, variation_depth);
    moves
}

fn flush_move(
    token: &mut String,
    moves: &mut Vec<ParsedMove>,
    white: &mut bool,
    variation_depth: u32,
) {
    if variation_depth == 0 && is_move_token(token) {
        moves.push(ParsedMove {
            white: *white,
            telemetry: MoveTelemetry::default(),
            forfeit: false,
        });
        *white = !*white;
    }
    token.clear();
}

fn is_move_token(token: &str) -> bool {
    if token.is_empty()
        || matches!(token, "1-0" | "0-1" | "1/2-1/2" | "*")
        || token.starts_with('$')
        || matches!(token, "!" | "?" | "!!" | "??" | "!?" | "?!")
    {
        return false;
    }
    !(token
        .chars()
        .next()
        .is_some_and(|value| value.is_ascii_digit())
        && token
            .chars()
            .all(|value| value.is_ascii_digit() || value == '.'))
}

/// `{forfeit …}`: Colosseum's comment for a search that lost on time.
fn is_forfeit_comment(comment: &str) -> bool {
    comment.split_whitespace().next() == Some("forfeit")
}

fn merge_comment(telemetry: &mut MoveTelemetry, comment: &str) {
    let lower = comment.to_ascii_lowercase();
    telemetry.is_book |= lower
        .split(|value: char| !value.is_ascii_alphanumeric())
        .any(|word| word == "book");

    let mut rest = comment;
    while let Some(start) = rest.find("[%") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(']') else { break };
        let body = rest[..end].trim();
        let mut fields = body.split_whitespace();
        if let (Some(key), Some(value)) = (fields.next(), fields.next()) {
            set_field(telemetry, key, value, true);
        }
        rest = &rest[end + 1..];
    }
    for field in comment.split_whitespace() {
        let field = field.trim_matches(|value: char| matches!(value, ',' | ';' | '(' | ')'));
        if let Some((key, value)) = field.split_once('=') {
            set_field(telemetry, key, value, false);
        }
    }
}

fn set_field(telemetry: &mut MoveTelemetry, key: &str, value: &str, bracketed: bool) {
    let key = key.to_ascii_lowercase();
    match key.as_str() {
        // An engine that reports depth 0 reported it; the writer omits a field
        // it has no value for, so a zero here is evidence, not a placeholder.
        "depth" | "d" => {
            if let Ok(value) = value.parse::<u32>() {
                telemetry.depth = Some(f64::from(value));
            }
        }
        "emt" if bracketed => telemetry.elapsed_seconds = parse_seconds(value, true),
        "time" | "t" => telemetry.elapsed_seconds = parse_seconds(value, false),
        // Overhead is a difference, so it may be negative; it is written in
        // milliseconds.
        "h" | "overhead" => {
            let value = value.trim_matches(|character: char| matches!(character, ',' | ';'));
            let value = value.strip_suffix("ms").unwrap_or(value);
            if let Ok(value) = value.parse::<f64>()
                && value.is_finite()
            {
                telemetry.overhead_ms = Some(value);
            }
        }
        "s" | "score" | "eval" | "ev" => {
            let value = value.trim_matches(|character: char| matches!(character, ',' | ';'));
            if let Some(mate) = value.strip_prefix('#') {
                // A mate claim is a distance, not an evaluation.
                telemetry.mate_score |= mate.parse::<i32>().is_ok();
            } else if let Ok(value) = value.parse::<f64>()
                && value.is_finite()
            {
                telemetry.score_cp = Some(value);
            }
        }
        "nodes" | "n" => {
            if let Ok(value) = value.replace('_', "").parse::<u64>() {
                telemetry.nodes = Some(value as f64);
            }
        }
        _ => {}
    }
}

fn parse_seconds(value: &str, unitless_seconds: bool) -> Option<f64> {
    let value = value.trim_matches(|character: char| matches!(character, ',' | ';'));
    // An explicit unit makes zero a measurement — a move that took under one
    // millisecond — rather than the placeholder a bare `0` usually is.
    if let Some(value) = value.strip_suffix("ms") {
        let seconds = value.parse::<f64>().ok()? / 1_000.0;
        return (seconds.is_finite() && seconds >= 0.0).then_some(seconds);
    }
    let seconds = if let Some(value) = value.strip_suffix('s') {
        value.parse::<f64>().ok()?
    } else if unitless_seconds {
        if value.contains(':') {
            let fields = value
                .split(':')
                .map(str::parse::<f64>)
                .collect::<Result<Vec<_>, _>>()
                .ok()?;
            fields
                .into_iter()
                .fold(0.0, |total, field| total * 60.0 + field)
        } else {
            value.parse::<f64>().ok()?
        }
    } else {
        return None;
    };
    (seconds.is_finite() && seconds > 0.0).then_some(seconds)
}

fn split_pgn(text: &str) -> Vec<&str> {
    let starts = text
        .match_indices("[Event ")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    starts
        .iter()
        .enumerate()
        .map(|(index, start)| &text[*start..starts.get(index + 1).copied().unwrap_or(text.len())])
        .collect()
}

fn pgn_tag(game: &str, name: &str) -> Option<String> {
    let prefix = format!("[{name} \"");
    game.lines().find_map(|line| {
        line.strip_prefix(&prefix)?
            .strip_suffix("\"]")
            .map(str::to_owned)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_syntaxes_aggregate_by_engine_and_exclude_opening_plies() {
        let pgn = r#"[Event "telemetry"]
[White "A"]
[Black "B"]
[Result "1/2-1/2"]
[OpeningPlyCount "2"]

1. e4 {[%depth 1] [%emt 1] [%nodes 1]} e5 {d=1 t=1s n=1}
2. Nf3 {[%depth 10] [%emt 0.5] [%nodes 500]} Nc6 {d=20 t=250ms n=1000}
3. Bb5 {[%depth 30] [%emt 1.5] [%nodes 3000]} a6 {d=40 t=0.75s n=1500} 1/2-1/2
"#;
        let report = analyze_pgn(pgn);
        assert_eq!(report.status, "available");
        assert_eq!(report.excluded_opening_moves, 2);
        let a = report
            .engines
            .iter()
            .find(|engine| engine.engine == "A")
            .unwrap();
        assert_eq!(a.eligible_moves, 2);
        assert_eq!(a.annotation_coverage, 1.0);
        assert_eq!(a.depth.mean, Some(20.0));
        assert_eq!(a.elapsed_seconds.median, Some(1.0));
        assert_eq!(a.implied_nps.mean, Some(1_500.0));
        let b = report
            .engines
            .iter()
            .find(|engine| engine.engine == "B")
            .unwrap();
        assert_eq!(b.depth.median, Some(30.0));
        assert_eq!(b.elapsed_seconds.mean, Some(0.5));
        assert_eq!(b.implied_nps.mean, Some(3_000.0));
    }

    #[test]
    fn overhead_round_trips_through_the_writer_into_quantiles_and_over_margin_counts() {
        use colosseum_core::GameResult;
        use colosseum_engine::pgn::{
            AnnotationScore, MoveAnnotation, PgnTags, SearchAnnotation, build_pgn,
        };
        let search = |time_ms, overhead_ms| {
            MoveAnnotation::Search(SearchAnnotation {
                score: Some(AnnotationScore::Centipawns(0)),
                depth: Some(10),
                time_ms: Some(time_ms),
                overhead_ms: Some(overhead_ms),
                nodes: Some(1),
            })
        };
        let tags = PgnTags {
            event: "e".into(),
            site: "s".into(),
            date: "2026.09.18".into(),
            round: 1,
            white: "A".into(),
            black: "B".into(),
            result: GameResult::Draw,
            time_control: String::new(),
            termination: None,
            fen: None,
            opening_plies: 0,
            identity: None,
            time_margins_ms: Some([20, 5]),
            slot: None,
            forfeited_search: None,
        };
        let moves = ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"].map(String::from);
        let annotations = [
            search(40, 2),
            search(40, -1),
            search(40, 25),
            search(40, 6),
            search(40, 3),
            search(40, 5),
        ];
        let pgn = build_pgn(&tags, &moves, &annotations);
        assert!(pgn.contains("h=25ms") && pgn.contains("h=-1ms"), "{pgn}");
        let report = analyze_pgn(&pgn);
        let engine = |name: &str| {
            report
                .engines
                .iter()
                .find(|engine| engine.engine == name)
                .unwrap()
                .harness_overhead_ms
                .clone()
        };
        let a = engine("A");
        assert_eq!(a.samples, 3);
        assert_eq!(
            (a.p50, a.p99, a.p999, a.max),
            (Some(3.0), Some(25.0), Some(25.0), Some(25.0))
        );
        // Only the 25 ms overhead exceeds White's 20 ms margin.
        assert_eq!((a.moves_with_margin, a.over_margin), (3, 1));
        let b = engine("B");
        assert_eq!((b.p50, b.max), (Some(5.0), Some(6.0)));
        assert_eq!(b.p50, Some(5.0));
        // Black's margin is 5 ms: 6 exceeds it, 5 does not, -1 does not.
        assert_eq!((b.moves_with_margin, b.over_margin), (3, 1));
    }

    #[test]
    fn a_forfeited_search_counts_for_the_side_that_forfeited_and_only_in_overhead() {
        let pgn = "[Event \"x\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n[WhiteTimeMarginMs \"20\"]\n[BlackTimeMarginMs \"20\"]\n\n1. e4 {t=30ms h=2ms} e5 {t=30ms h=1ms} 2. Nf3 {t=30ms h=3ms} {forfeit t=164ms h=150ms} 1-0\n";
        let report = analyze_pgn(pgn);
        let engine = |name: &str| {
            report
                .engines
                .iter()
                .find(|engine| engine.engine == name)
                .unwrap()
                .clone()
        };
        let black = engine("B");
        // Black's one move and its forfeited search.
        assert_eq!(black.eligible_moves, 1);
        assert_eq!(black.harness_overhead_ms.samples, 2);
        assert_eq!(black.harness_overhead_ms.max, Some(150.0));
        assert_eq!(
            (
                black.harness_overhead_ms.moves_with_margin,
                black.harness_overhead_ms.over_margin
            ),
            (2, 1)
        );
        // White's last move keeps its own comment, not the forfeit's.
        let white = engine("A");
        assert_eq!(white.harness_overhead_ms.max, Some(3.0));
        assert_eq!(white.harness_overhead_ms.over_margin, 0);
    }

    #[test]
    fn overhead_without_a_margin_tag_is_measured_but_not_compared() {
        let pgn = "[Event \"x\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n\n1. e4 {t=30ms h=12ms} e5 {t=30ms h=1ms} *\n";
        let report = analyze_pgn(pgn);
        let a = &report.engines[0].harness_overhead_ms;
        assert_eq!(a.max, Some(12.0));
        assert_eq!((a.moves_with_margin, a.over_margin), (0, 0));
    }

    #[test]
    fn missing_annotations_are_unavailable_and_book_comments_are_excluded() {
        let pgn = "[Event \"x\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"*\"]\n\n1. e4 {book} e5 {book} 2. Nf3 Nc6 *\n";
        let report = analyze_pgn(pgn);
        assert_eq!(report.status, "unavailable");
        assert_eq!(report.excluded_opening_moves, 2);
        assert!(
            report
                .engines
                .iter()
                .all(|engine| engine.depth.mean.is_none())
        );
    }
}
