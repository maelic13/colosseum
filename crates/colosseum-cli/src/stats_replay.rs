use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use colosseum_core::{PairGameResult, PentanomialVector, pentanomial_statistics};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::pgn_telemetry::{SearchTelemetryReport, analyze_pgn, unavailable};

const Z95: f64 = 1.959_963_984_540_054;

#[derive(Debug, Clone, Serialize)]
pub struct ReplayAttempt {
    pub authority: &'static str,
    pub path: PathBuf,
    pub accepted: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsReplayReport {
    pub authority: &'static str,
    pub source: PathBuf,
    pub perspective: String,
    pub pairing: &'static str,
    pub games: u32,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    pub score: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pentanomial: Option<[u32; 5]>,
    pub complete_pairs: u32,
    pub unpaired_games: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired_statistics: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paired_statistics_unavailable: Option<String>,
    pub attempts: Vec<ReplayAttempt>,
    pub warnings: Vec<String>,
    pub telemetry: SearchTelemetryReport,
}

#[derive(Debug, Clone)]
struct RawGame {
    number: Option<u32>,
    opening: Option<String>,
    outcome: PairGameResult,
}

pub fn replay(path: &Path, subject: Option<&str>) -> Result<StatsReplayReport, String> {
    if path.is_dir() {
        replay_directory(path, subject)
    } else {
        replay_file(path, subject)
    }
}

fn replay_directory(path: &Path, subject: Option<&str>) -> Result<StatsReplayReport, String> {
    let candidates = [
        ("structured-run-store", path.join("result.json")),
        ("structured-run-store", path.join("checkpoint.json")),
        (
            "structured-run-store",
            path.join("checkpoint.previous.json"),
        ),
        ("pgn-export", path.join("games.pgn")),
        ("forensic-log", path.join("run.log")),
        ("console", path.join("console.txt")),
    ];
    let mut attempts = Vec::new();
    for (authority, candidate) in candidates {
        if !candidate.is_file() {
            attempts.push(ReplayAttempt {
                authority,
                path: candidate,
                accepted: false,
                detail: "not present".into(),
            });
            continue;
        }
        match read_source(authority, &candidate, subject) {
            Ok((games, perspective, paired_capable, telemetry)) if !games.is_empty() => {
                attempts.push(ReplayAttempt {
                    authority,
                    path: candidate.clone(),
                    accepted: true,
                    detail: format!("replayed {} scored games", games.len()),
                });
                return Ok(build_report(
                    authority,
                    candidate,
                    perspective,
                    games,
                    paired_capable,
                    attempts,
                    telemetry,
                ));
            }
            Ok(_) => attempts.push(ReplayAttempt {
                authority,
                path: candidate,
                accepted: false,
                detail: "contains no scored games".into(),
            }),
            Err(error) => attempts.push(ReplayAttempt {
                authority,
                path: candidate,
                accepted: false,
                detail: error,
            }),
        }
    }
    Err(format!(
        "no replayable source found in {}; attempted structured store, PGN, forensic log and console",
        path.display()
    ))
}

fn replay_file(path: &Path, subject: Option<&str>) -> Result<StatsReplayReport, String> {
    let authority = match path.extension().and_then(|value| value.to_str()) {
        Some(value) if value.eq_ignore_ascii_case("json") => "structured-run-store",
        Some(value) if value.eq_ignore_ascii_case("pgn") => "pgn-export",
        Some(value) if value.eq_ignore_ascii_case("log") => "forensic-log",
        _ => "console",
    };
    let (games, perspective, paired_capable, telemetry) = read_source(authority, path, subject)?;
    if games.is_empty() {
        return Err(format!("{} contains no scored games", path.display()));
    }
    let attempts = vec![ReplayAttempt {
        authority,
        path: path.to_owned(),
        accepted: true,
        detail: format!("replayed {} scored games", games.len()),
    }];
    Ok(build_report(
        authority,
        path.to_owned(),
        perspective,
        games,
        paired_capable,
        attempts,
        telemetry,
    ))
}

fn read_source(
    authority: &'static str,
    path: &Path,
    subject: Option<&str>,
) -> Result<(Vec<RawGame>, String, bool, SearchTelemetryReport), String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    match authority {
        "structured-run-store" => structured_games(&text).map(|games| {
            (
                games,
                "engine A".into(),
                true,
                unavailable("structured source has no PGN move annotations"),
            )
        }),
        "pgn-export" => Ok((
            pgn_games(&text, subject),
            subject.map_or_else(|| "White side".into(), |value| value.to_owned()),
            false,
            analyze_pgn(&text),
        )),
        "forensic-log" => Ok((
            log_games(&text),
            "engine A".into(),
            true,
            unavailable("forensic log has no PGN move annotations"),
        )),
        _ => Ok((
            result_tokens(&text),
            "White side (console tokens)".into(),
            false,
            unavailable("console source has no PGN move annotations"),
        )),
    }
}

fn structured_games(text: &str) -> Result<Vec<RawGame>, String> {
    let mut value: Value = serde_json::from_str(text).map_err(|error| error.to_string())?;
    if value.get("payload").is_some() {
        let payload = value
            .get("payload")
            .cloned()
            .ok_or_else(|| "checkpoint envelope has no payload".to_owned())?;
        let expected = value
            .get("payload_sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| "checkpoint envelope has no payload checksum".to_owned())?;
        let actual = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&payload).expect("JSON checkpoint payload is serializable")
            )
        );
        if actual != expected {
            return Err("checkpoint payload checksum mismatch".into());
        }
        value = payload;
    }
    let mut candidates = Vec::<Vec<RawGame>>::new();
    collect_structured_candidates(&value, &mut candidates);
    candidates
        .into_iter()
        .max_by_key(Vec::len)
        .ok_or_else(|| "structured document contains no game array".into())
}

fn collect_structured_candidates(value: &Value, output: &mut Vec<Vec<RawGame>>) {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "official_pairs"
                    && let Some(pairs) = child.as_array()
                {
                    let games = pairs
                        .iter()
                        .flat_map(|pair| [pair.get("first"), pair.get("second")])
                        .flatten()
                        .filter_map(parse_structured_game)
                        .collect::<Vec<_>>();
                    if !games.is_empty() {
                        output.push(games);
                    }
                }
                if key == "games"
                    && let Some(games) = child.as_array()
                {
                    let games = games
                        .iter()
                        .filter_map(parse_structured_game)
                        .collect::<Vec<_>>();
                    if !games.is_empty() {
                        output.push(games);
                    }
                }
                collect_structured_candidates(child, output);
            }
        }
        Value::Array(values) => {
            for child in values {
                collect_structured_candidates(child, output);
            }
        }
        _ => {}
    }
}

fn parse_structured_game(value: &Value) -> Option<RawGame> {
    if value.get("scorable").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let result = value.get("result")?.as_str()?;
    let white = value.get("white").and_then(Value::as_str).unwrap_or("a");
    let white_score = match result {
        "WhiteWin" | "white-win" | "1-0" => PairGameResult::Win,
        "BlackWin" | "black-win" | "0-1" => PairGameResult::Loss,
        "Draw" | "draw" | "1/2-1/2" => PairGameResult::Draw,
        _ => return None,
    };
    let outcome = if white.eq_ignore_ascii_case("a") {
        white_score
    } else {
        invert(white_score)
    };
    Some(RawGame {
        number: value
            .get("number")
            .and_then(Value::as_u64)
            .and_then(|value| value.try_into().ok()),
        opening: value
            .get("opening")
            .map(|opening| serde_json::to_string(opening).unwrap_or_default()),
        outcome,
    })
}

fn log_games(text: &str) -> Vec<RawGame> {
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value.get("event").and_then(Value::as_str) == Some("game-completed"))
        .filter_map(|value| parse_structured_game(&value["game"]))
        .collect()
}

fn pgn_games(text: &str, subject: Option<&str>) -> Vec<RawGame> {
    split_pgn(text)
        .into_iter()
        .filter_map(|game| {
            let result = pgn_tag(game, "Result")?;
            let white = pgn_tag(game, "White").unwrap_or_default();
            let black = pgn_tag(game, "Black").unwrap_or_default();
            let white_score = token_result(&result)?;
            let outcome = match subject {
                Some(name) if white == name => white_score,
                Some(name) if black == name => invert(white_score),
                Some(_) => return None,
                None => white_score,
            };
            Some(RawGame {
                number: None,
                opening: None,
                outcome,
            })
        })
        .collect()
}

fn split_pgn(text: &str) -> Vec<&str> {
    let starts = text
        .match_indices("[Event ")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if starts.is_empty() {
        return Vec::new();
    }
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

fn result_tokens(text: &str) -> Vec<RawGame> {
    text.split_whitespace()
        .filter_map(token_result)
        .map(|outcome| RawGame {
            number: None,
            opening: None,
            outcome,
        })
        .collect()
}

fn token_result(token: &str) -> Option<PairGameResult> {
    match token.trim_matches(|character: char| character == '"' || character == ',') {
        "1-0" => Some(PairGameResult::Win),
        "0-1" => Some(PairGameResult::Loss),
        "1/2-1/2" => Some(PairGameResult::Draw),
        _ => None,
    }
}

fn invert(result: PairGameResult) -> PairGameResult {
    match result {
        PairGameResult::Win => PairGameResult::Loss,
        PairGameResult::Draw => PairGameResult::Draw,
        PairGameResult::Loss => PairGameResult::Win,
    }
}

fn build_report(
    authority: &'static str,
    source: PathBuf,
    perspective: String,
    games: Vec<RawGame>,
    paired_capable: bool,
    attempts: Vec<ReplayAttempt>,
    telemetry: SearchTelemetryReport,
) -> StatsReplayReport {
    let mut wins = 0;
    let mut draws = 0;
    let mut losses = 0;
    for game in &games {
        match game.outcome {
            PairGameResult::Win => wins += 1,
            PairGameResult::Draw => draws += 1,
            PairGameResult::Loss => losses += 1,
        }
    }
    let mut sample = PentanomialVector::default();
    let mut paired_games = 0;
    if paired_capable {
        let mut by_pair = BTreeMap::<u32, Vec<&RawGame>>::new();
        for game in &games {
            if let Some(number) = game.number.filter(|number| *number > 0) {
                by_pair.entry((number - 1) / 2).or_default().push(game);
            }
        }
        for pair in by_pair.values_mut() {
            pair.sort_by_key(|game| game.number);
            if pair.len() == 2
                && pair[0].number.is_some_and(|number| number % 2 == 1)
                && pair[1].number == pair[0].number.map(|number| number + 1)
                && pair[0].opening == pair[1].opening
            {
                sample.record_pair(pair[0].outcome, pair[1].outcome);
                paired_games += 2;
            }
        }
    }
    for _ in paired_games..games.len() {
        sample.record_unpaired_game();
    }
    let paired_statistics = pentanomial_statistics(&sample, Z95).ok().map(|stats| {
        json!({
            "score": stats.score,
            "variance": stats.variance,
            "standard_error": stats.standard_error,
                "logistic_elo": {
                    "elo": stats.logistic_elo.elo,
                    "score": stats.logistic_elo.score,
                    "lower": stats.logistic_elo.lower,
                    "upper": stats.logistic_elo.upper,
                },
                "normalized_elo": {
                    "elo": stats.normalized_elo.elo,
                    "lower": stats.normalized_elo.lower,
                    "upper": stats.normalized_elo.upper,
                },
            "los": stats.los,
            "draw_ratio": stats.draw_ratio,
            "pairs_ratio": stats.pairs_ratio,
            "win_loss_to_double_draw_ratio": stats.win_loss_to_double_draw_ratio,
        })
    });
    let unavailable = (sample.pairs() > 0 && paired_statistics.is_none()).then(|| {
        pentanomial_statistics(&sample, Z95)
            .expect_err("statistics were unavailable")
            .to_string()
    });
    let games_count = games.len() as u32;
    let mut warnings = Vec::new();
    if sample.unpaired_games() > 0 {
        warnings.push(
            "pair/opening identity is absent or incomplete for some games; unpaired W/D/L is reported without invented pentanomial statistics"
                .into(),
        );
    }
    if telemetry.status == "available" {
        warnings.push(telemetry.node_semantics_warning.into());
    }
    StatsReplayReport {
        authority,
        source,
        perspective,
        pairing: if sample.pairs() > 0 {
            "paired"
        } else {
            "unpaired"
        },
        games: games_count,
        wins,
        draws,
        losses,
        score: (f64::from(wins) + 0.5 * f64::from(draws)) / f64::from(games_count),
        pentanomial: (sample.pairs() > 0).then(|| sample.counts()),
        complete_pairs: sample.pairs(),
        unpaired_games: sample.unpaired_games(),
        paired_statistics,
        paired_statistics_unavailable: unavailable,
        attempts,
        warnings,
        telemetry,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pgn_without_pair_identity_is_never_guessed_into_pairs() {
        let pgn = "[Event \"x\"]\n[White \"A\"]\n[Black \"B\"]\n[Result \"1-0\"]\n\n1-0\n\n[Event \"x\"]\n[White \"B\"]\n[Black \"A\"]\n[Result \"0-1\"]\n\n0-1\n";
        let games = pgn_games(pgn, Some("A"));
        let report = build_report(
            "pgn-export",
            "x.pgn".into(),
            "A".into(),
            games,
            false,
            vec![],
            unavailable("fixture has no annotations"),
        );
        assert_eq!(report.wins, 2);
        assert_eq!(report.complete_pairs, 0);
        assert_eq!(report.unpaired_games, 2);
        assert_eq!(report.pairing, "unpaired");
    }
}
