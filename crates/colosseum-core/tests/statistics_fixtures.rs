//! Phase 1 acceptance tests for the committed statistics fixture corpus.
//!
//! Every comparison is named in `phase-1-acceptance.toml`. External runner
//! artifacts are compared only on oracle-matrix fields that their recorded
//! samples actually support.

use std::collections::{BTreeMap, BTreeSet};

use colosseum_core::{
    EloModel, FixedNTestTails, PairGameResult, PentanomialDistribution, PentanomialVector,
    SprtDecision, StatisticsError, elo_with_error, fixed_n_achieved_resolution, fixed_n_plan,
    pentanomial_sprt, pentanomial_statistics, sprt,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const ANALYTIC_TOML: &str =
    include_str!("../../../tests/fixtures/statistics/analytic-pentanomial.toml");
const EXTERNAL_TOML: &str =
    include_str!("../../../tests/fixtures/statistics/external-observations.toml");
const ACCEPTANCE_TOML: &str =
    include_str!("../../../tests/fixtures/statistics/phase-1-acceptance.toml");
const ORACLE_MATRIX: &str = include_str!("../../../tests/fixtures/statistics/oracle-matrix.md");
const FASTCHESS_CONSOLE: &str =
    include_str!("../../../tests/fixtures/statistics/external/fastchess.console.txt");
const FASTCHESS_PGN: &str =
    include_str!("../../../tests/fixtures/statistics/external/fastchess.pgn");
const CUTECHESS_CONSOLE: &str =
    include_str!("../../../tests/fixtures/statistics/external/cutechess.console.txt");
const CUTECHESS_PGN: &str =
    include_str!("../../../tests/fixtures/statistics/external/cutechess.pgn");
const PHASE_4B_PARITY_TOML: &str =
    include_str!("../../../tests/fixtures/statistics/phase-4b-parity.toml");
const PHASE_4B_FASTCHESS_CONSOLE: &str =
    include_str!("../../../tests/fixtures/statistics/external/phase4b5-fastchess.console.txt");
const PHASE_4B_OPENINGS: &str =
    include_str!("../../../tests/fixtures/statistics/external/phase4b5-openings.epd");
const PHASE_4B_LIVE_FASTCHESS_CONSOLE: &str =
    include_str!("../../../tests/fixtures/statistics/external/phase4b5-live-fastchess.console.txt");
const PHASE_4B_LIVE_CUTECHESS_CONSOLE: &str =
    include_str!("../../../tests/fixtures/statistics/external/phase4b5-live-cutechess.console.txt");
const PHASE_4B_LIVE_COLOSSEUM: &str =
    include_str!("../../../tests/fixtures/statistics/external/phase4b5-live-colosseum.json");
// `normal_quantile` in core inverts the documented A&S CDF approximation.
// Its probability error translates to less than 2e-5 Elo at this fixture.
const NORMAL_QUANTILE_ELO_TOLERANCE: f64 = 2e-5;

#[derive(Debug, Deserialize)]
struct AnalyticFixtures {
    schema_version: u32,
    #[serde(rename = "description")]
    _description: String,
    pair_binning: Vec<PairBinningFixture>,
    statistics: Vec<StatisticsFixture>,
    sprt: Vec<PentanomialSprtFixture>,
    trinomial_sprt: Vec<TrinomialSprtFixture>,
    fixed_n: Vec<FixedNFixture>,
    achieved_resolution: Vec<AchievedResolutionFixture>,
    error: Vec<ErrorFixture>,
}

#[derive(Debug, Deserialize)]
struct PairBinningFixture {
    id: String,
    pairs: Vec<[String; 2]>,
    counts: [u32; 5],
    central_win_loss_pairs: u32,
    central_double_draw_pairs: u32,
    drawn_games: u32,
}

#[derive(Debug, Deserialize)]
struct StatisticsFixture {
    id: String,
    counts: [u32; 5],
    pairs: u32,
    unpaired_games: u32,
    central_win_loss_pairs: u32,
    central_double_draw_pairs: u32,
    z: f64,
    score: f64,
    variance: f64,
    standard_error: f64,
    logistic_elo: f64,
    logistic_margin: f64,
    normalized_elo: f64,
    normalized_margin: f64,
    los: f64,
    draw_ratio: f64,
    pairs_ratio: f64,
    win_loss_to_double_draw_ratio: f64,
}

#[derive(Debug, Deserialize)]
struct PentanomialSprtFixture {
    id: String,
    counts: [u32; 5],
    elo0: f64,
    elo1: f64,
    alpha: f64,
    beta: f64,
    lower: f64,
    upper: f64,
    logistic_llr: f64,
    normalized_llr: f64,
}

#[derive(Debug, Deserialize)]
struct TrinomialSprtFixture {
    id: String,
    wins: u32,
    draws: u32,
    losses: u32,
    elo0: f64,
    elo1: f64,
    alpha: f64,
    beta: f64,
    lower: f64,
    upper: f64,
    llr: f64,
    decision: String,
}

#[derive(Debug, Deserialize)]
struct FixedNFixture {
    id: String,
    probabilities: [f64; 5],
    variance: f64,
    tails: String,
    target_effect: f64,
    significance: f64,
    power: f64,
    normalized_required_pairs: u64,
    logistic_required_pairs: u64,
}

#[derive(Debug, Deserialize)]
struct AchievedResolutionFixture {
    id: String,
    counts: [u32; 5],
    unpaired_games: u32,
    significance: f64,
    confidence: f64,
    normalized_estimate: f64,
    normalized_lower: f64,
    normalized_upper: f64,
    normalized_resolution: f64,
    logistic_estimate: f64,
    logistic_lower: f64,
    logistic_upper: f64,
    logistic_resolution: f64,
}

#[derive(Debug, Deserialize)]
struct ErrorFixture {
    id: String,
    counts: [u32; 5],
    operation: String,
    error: String,
}

#[derive(Debug, Deserialize)]
struct ExternalFixtures {
    schema_version: u32,
    #[serde(rename = "description")]
    _description: String,
    observation: Vec<ExternalObservation>,
}

#[derive(Debug, Deserialize)]
struct ExternalObservation {
    id: String,
    runner: String,
    console: String,
    pgn: String,
    engine_a: String,
    engine_b: String,
    wins: u32,
    draws: u32,
    losses: u32,
    score: f64,
    draw_ratio: f64,
    complete_pairs: u32,
    counts: [u32; 5],
    console_score: String,
}

#[derive(Debug, Deserialize)]
struct AcceptanceManifest {
    schema_version: u32,
    #[serde(rename = "description")]
    _description: String,
    comparison: Vec<Comparison>,
    exclusion: Vec<Exclusion>,
}

#[derive(Debug, Deserialize)]
struct Comparison {
    id: String,
    matrix_row: String,
    source: String,
    fixture: String,
    fields: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Exclusion {
    id: String,
    matrix_rows: Vec<String>,
    sources: Vec<String>,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct Phase4bParity {
    schema_version: u32,
    #[serde(rename = "description")]
    _description: String,
    terminal_replay: TerminalReplay,
    live: LiveParity,
}

#[derive(Debug, Deserialize)]
struct TerminalReplay {
    runner: String,
    runner_version: String,
    runner_sha256: String,
    console: String,
    console_sha256: String,
    openings: String,
    openings_sha256: String,
    engine_a: String,
    engine_a_sha256: String,
    engine_b: String,
    engine_b_sha256: String,
    command: String,
    model: String,
    elo0: f64,
    elo1: f64,
    alpha: f64,
    beta: f64,
    scheduled_pair_cap: u32,
    terminal_pair: u32,
    decision: String,
    counts: [u32; 5],
    wins: u32,
    draws: u32,
    losses: u32,
    reported_llr: f64,
    reported_lower: f64,
    reported_upper: f64,
    display_tolerance: f64,
    elapsed_seconds: u32,
}

#[derive(Debug, Deserialize)]
struct LiveParity {
    host: String,
    date: String,
    engine: String,
    engine_sha256: String,
    conditions: String,
    shared_fields: Vec<String>,
    conditionally_shared_fields: Vec<String>,
    excluded_fields: Vec<String>,
    exclusion_reason: String,
    observation: Vec<LiveObservation>,
}

#[derive(Debug, Deserialize)]
struct LiveObservation {
    runner: String,
    runner_version: String,
    runner_sha256: String,
    artifact: String,
    artifact_sha256: String,
    raw_artifact_sha256: Option<String>,
    command: String,
    exit_code: i32,
    status: String,
    games: u32,
    wins: u32,
    draws: u32,
    losses: u32,
    complete_pairs: u32,
    counts: Option<[u32; 5]>,
    faults: u32,
}

#[derive(Debug, Deserialize)]
struct ColosseumLiveProjection {
    source: String,
    status: String,
    exit_code: i32,
    games: u32,
    wins: u32,
    draws: u32,
    losses: u32,
    complete_pairs: u32,
    pentanomial: [u32; 5],
    colour_split: ColourSplit,
    faults: ProjectedFaults,
}

#[derive(Debug, Deserialize)]
struct ColourSplit {
    a_as_white: [u32; 3],
    a_as_black: [u32; 3],
}

#[derive(Debug, Deserialize)]
struct ProjectedFaults {
    engine_a: u32,
    engine_b: u32,
    time_losses_a: u32,
    time_losses_b: u32,
    infrastructure: u32,
}

#[derive(Debug, Default)]
struct PgnGame {
    round: String,
    white: String,
    black: String,
    result: String,
}

#[derive(Debug)]
struct ConsoleGame<'a> {
    white: &'a str,
    black: &'a str,
    result: &'a str,
}

fn assert_close(case: &str, field: &str, actual: f64, expected: f64, tolerance: f64) {
    assert!(actual.is_finite(), "{case}.{field} is not finite: {actual}");
    assert!(expected.is_finite(), "{case}.{field} fixture is not finite");
    assert!(
        (actual - expected).abs() <= tolerance,
        "{case}.{field}: expected {expected:.15}, got {actual:.15} (tolerance {tolerance})"
    );
}

fn parse_result(value: &str) -> PairGameResult {
    match value {
        "win" => PairGameResult::Win,
        "draw" => PairGameResult::Draw,
        "loss" => PairGameResult::Loss,
        other => panic!("unknown pair result {other:?}"),
    }
}

fn sha256_hex(contents: &str) -> String {
    format!("{:x}", Sha256::digest(contents.as_bytes()))
}

fn parse_finished_games(console: &str) -> Vec<ConsoleGame<'_>> {
    console
        .lines()
        .filter_map(|line| {
            let line = line.strip_prefix("Finished game ")?;
            let (_, rest) = line.split_once(" (")?;
            let (pairing, rest) = rest.split_once("): ")?;
            let (white, black) = pairing.split_once(" vs ")?;
            let result = rest.split_whitespace().next()?;
            Some(ConsoleGame {
                white,
                black,
                result,
            })
        })
        .collect()
}

fn console_result_for(engine: &str, opponent: &str, game: &ConsoleGame<'_>) -> PairGameResult {
    use PairGameResult::{Draw, Loss, Win};

    match (
        game.white == engine && game.black == opponent,
        game.black == engine && game.white == opponent,
        game.result,
    ) {
        (true, false, "1-0") | (false, true, "0-1") => Win,
        (true, false, "0-1") | (false, true, "1-0") => Loss,
        (true, false, "1/2-1/2") | (false, true, "1/2-1/2") => Draw,
        _ => panic!("unexpected console engine names or result in {game:?}"),
    }
}

fn console_sample(console: &str, engine: &str, opponent: &str) -> (PentanomialVector, [u32; 3]) {
    let games = parse_finished_games(console);
    assert_eq!(games.len() % 2, 0, "console has an incomplete pair");
    let mut sample = PentanomialVector::default();
    let mut wdl = [0_u32; 3];
    for pair in games.chunks_exact(2) {
        assert_eq!(pair[0].white, pair[1].black, "pair did not swap White");
        assert_eq!(pair[0].black, pair[1].white, "pair did not swap Black");
        let first = console_result_for(engine, opponent, &pair[0]);
        let second = console_result_for(engine, opponent, &pair[1]);
        for result in [first, second] {
            match result {
                PairGameResult::Win => wdl[0] += 1,
                PairGameResult::Draw => wdl[1] += 1,
                PairGameResult::Loss => wdl[2] += 1,
            }
        }
        sample.record_pair(first, second);
    }
    (sample, wdl)
}

fn sample_from_counts(
    counts: [u32; 5],
    central_win_loss_pairs: u32,
    unpaired_games: u32,
) -> PentanomialVector {
    use PairGameResult::{Draw, Loss, Win};

    assert!(central_win_loss_pairs <= counts[2]);
    let mut sample = PentanomialVector::default();
    for _ in 0..counts[0] {
        sample.record_pair(Loss, Loss);
    }
    for _ in 0..counts[1] {
        sample.record_pair(Loss, Draw);
    }
    for _ in 0..central_win_loss_pairs {
        sample.record_pair(Win, Loss);
    }
    for _ in central_win_loss_pairs..counts[2] {
        sample.record_pair(Draw, Draw);
    }
    for _ in 0..counts[3] {
        sample.record_pair(Draw, Win);
    }
    for _ in 0..counts[4] {
        sample.record_pair(Win, Win);
    }
    for _ in 0..unpaired_games {
        sample.record_unpaired_game();
    }
    assert_eq!(sample.counts(), counts);
    sample
}

fn statistics_error_name(error: StatisticsError) -> &'static str {
    match error {
        StatisticsError::InsufficientPairs { .. } => "InsufficientPairs",
        StatisticsError::InsufficientGames { .. } => "InsufficientGames",
        StatisticsError::ZeroVariance => "ZeroVariance",
        StatisticsError::NonFiniteInput { .. } => "NonFiniteInput",
        StatisticsError::InvalidConfidenceMultiplier { .. } => "InvalidConfidenceMultiplier",
        StatisticsError::InvalidScore { .. } => "InvalidScore",
        StatisticsError::IntervalOutsideLogisticDomain { .. } => "IntervalOutsideLogisticDomain",
        StatisticsError::InvalidProbability { .. } => "InvalidProbability",
        StatisticsError::InvalidSignificance { .. } => "InvalidSignificance",
        StatisticsError::InvalidPower { .. } => "InvalidPower",
        StatisticsError::InvalidTargetEffect { .. } => "InvalidTargetEffect",
        StatisticsError::InvalidHypotheses { .. } => "InvalidHypotheses",
        StatisticsError::InvalidErrorRates { .. } => "InvalidErrorRates",
        StatisticsError::InvalidDistributionProbability { .. } => "InvalidDistributionProbability",
        StatisticsError::DistributionNotNormalized { .. } => "DistributionNotNormalized",
        StatisticsError::DegenerateDistribution => "DegenerateDistribution",
        StatisticsError::LikelihoodSolveFailed => "LikelihoodSolveFailed",
        StatisticsError::RequiredPairsOutOfRange => "RequiredPairsOutOfRange",
    }
}

fn parse_pgn_games(pgn: &str) -> Vec<PgnGame> {
    let mut games = Vec::new();
    let mut current: Option<PgnGame> = None;

    for line in pgn.lines().map(str::trim) {
        let Some((tag, value)) = parse_pgn_tag(line) else {
            continue;
        };
        if tag == "Event" {
            if let Some(game) = current.take() {
                games.push(game);
            }
            current = Some(PgnGame::default());
        }
        let Some(game) = current.as_mut() else {
            continue;
        };
        match tag {
            "Round" => game.round = value.to_string(),
            "White" => game.white = value.to_string(),
            "Black" => game.black = value.to_string(),
            "Result" => game.result = value.to_string(),
            _ => {}
        }
    }
    if let Some(game) = current {
        games.push(game);
    }

    for game in &games {
        assert!(!game.round.is_empty(), "PGN game is missing Round");
        assert!(!game.white.is_empty(), "PGN game is missing White");
        assert!(!game.black.is_empty(), "PGN game is missing Black");
        assert!(!game.result.is_empty(), "PGN game is missing Result");
    }
    games
}

fn parse_pgn_tag(line: &str) -> Option<(&str, &str)> {
    let body = line.strip_prefix('[')?.strip_suffix(']')?;
    let (tag, quoted) = body.split_once(" \"")?;
    Some((tag, quoted.strip_suffix('"')?))
}

fn game_result_for(engine: &str, opponent: &str, game: &PgnGame) -> PairGameResult {
    use PairGameResult::{Draw, Loss, Win};

    match (
        game.white == engine && game.black == opponent,
        game.black == engine && game.white == opponent,
        game.result.as_str(),
    ) {
        (true, false, "1-0") | (false, true, "0-1") => Win,
        (true, false, "0-1") | (false, true, "1-0") => Loss,
        (true, false, "1/2-1/2") | (false, true, "1/2-1/2") => Draw,
        _ => panic!("unexpected engine names or result in {game:?}"),
    }
}

fn external_artifacts(observation: &ExternalObservation) -> (&'static str, &'static str) {
    match observation.id.as_str() {
        "fastchess-clean-sweep" => {
            assert_eq!(observation.runner, "fastchess");
            assert_eq!(observation.console, "external/fastchess.console.txt");
            assert_eq!(observation.pgn, "external/fastchess.pgn");
            (FASTCHESS_CONSOLE, FASTCHESS_PGN)
        }
        "cutechess-clean-sweep" => {
            assert_eq!(observation.runner, "cutechess-cli");
            assert_eq!(observation.console, "external/cutechess.console.txt");
            assert_eq!(observation.pgn, "external/cutechess.pgn");
            (CUTECHESS_CONSOLE, CUTECHESS_PGN)
        }
        other => panic!("unreviewed external observation {other:?}"),
    }
}

fn collapsed(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn oracle_matrix_rows() -> BTreeSet<&'static str> {
    ORACLE_MATRIX
        .lines()
        .filter_map(|line| {
            let row = line.strip_prefix('|')?.split('|').next()?.trim();
            (!row.is_empty()
                && row != "Field / behaviour"
                && !row
                    .chars()
                    .all(|character| character == '-' || character == ':'))
            .then_some(row)
        })
        .collect()
}

#[test]
fn acceptance_manifest_names_every_executed_oracle_cell() {
    let manifest: AcceptanceManifest = toml::from_str(ACCEPTANCE_TOML).unwrap();
    assert_eq!(manifest.schema_version, 1);

    let expected = BTreeSet::from([
        "analytic.pair-binning",
        "analytic.pentanomial-moments",
        "analytic.normalized-sprt",
        "analytic.logistic-sprt",
        "analytic.trinomial-sprt",
        "analytic.score-and-draw-ratio",
        "analytic.logistic-interval-los",
        "analytic.normalized-resolution",
        "analytic.fixed-n-plan",
        "analytic.unpaired-exclusion",
        "analytic.typed-errors",
        "fastchess.pair-binning",
        "cutechess.pair-binning",
        "fastchess.score-and-draw-ratio",
        "cutechess.score-and-draw-ratio",
    ]);
    let actual = manifest
        .comparison
        .iter()
        .map(|comparison| comparison.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected, "acceptance comparison list changed");
    assert_eq!(
        actual.len(),
        manifest.comparison.len(),
        "duplicate comparison ID"
    );

    for comparison in &manifest.comparison {
        assert!(
            ORACLE_MATRIX.contains(&format!("| {} |", comparison.matrix_row)),
            "{} names no oracle-matrix row",
            comparison.id
        );
        assert!(!comparison.source.is_empty());
        assert!(!comparison.fixture.is_empty());
        assert!(!comparison.fields.is_empty());
    }
    let compared_rows = manifest
        .comparison
        .iter()
        .map(|comparison| comparison.matrix_row.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        compared_rows,
        oracle_matrix_rows(),
        "every oracle-matrix row must have an executed analytic or compatible external comparison"
    );

    let expected_exclusions = BTreeSet::from([
        "external.clean-sweep-estimates",
        "external.unmatched-sequential-models",
        "external.analytic-only-contracts",
    ]);
    let actual_exclusions = manifest
        .exclusion
        .iter()
        .map(|exclusion| exclusion.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual_exclusions, expected_exclusions);
    assert_eq!(
        actual_exclusions.len(),
        manifest.exclusion.len(),
        "duplicate exclusion ID"
    );
    for exclusion in &manifest.exclusion {
        assert!(!exclusion.reason.trim().is_empty());
        assert!(!exclusion.sources.is_empty());
        for row in &exclusion.matrix_rows {
            assert!(
                ORACLE_MATRIX.contains(&format!("| {row} |")),
                "{} excludes no oracle-matrix row",
                exclusion.id
            );
        }
    }
}

#[test]
fn analytic_fixture_covers_every_phase_one_statistics_contract() {
    let fixtures: AnalyticFixtures = toml::from_str(ANALYTIC_TOML).unwrap();
    assert_eq!(fixtures.schema_version, 1);
    assert_eq!(fixtures.pair_binning.len(), 1);
    assert_eq!(fixtures.statistics.len(), 1);
    assert_eq!(fixtures.sprt.len(), 1);
    assert_eq!(fixtures.trinomial_sprt.len(), 1);
    assert_eq!(fixtures.fixed_n.len(), 1);
    assert_eq!(fixtures.achieved_resolution.len(), 1);
    assert_eq!(fixtures.error.len(), 4);

    let binning = &fixtures.pair_binning[0];
    assert_eq!(binning.id, "all-pair-outcomes");
    let mut binned = PentanomialVector::default();
    for pair in &binning.pairs {
        binned.record_pair(parse_result(&pair[0]), parse_result(&pair[1]));
    }
    assert_eq!(binned.counts(), binning.counts);
    assert_eq!(
        binned.central_pair_breakdown(),
        (
            binning.central_win_loss_pairs,
            binning.central_double_draw_pairs
        )
    );
    assert_close(
        &binning.id,
        "drawn_games",
        binned.draw_ratio().unwrap() * f64::from(2 * binned.pairs()),
        f64::from(binning.drawn_games),
        1e-12,
    );

    let fixture = &fixtures.statistics[0];
    assert_eq!(fixture.id, "symmetric-nine-pair");
    let sample = sample_from_counts(
        fixture.counts,
        fixture.central_win_loss_pairs,
        fixture.unpaired_games,
    );
    assert_eq!(
        sample.central_pair_breakdown(),
        (
            fixture.central_win_loss_pairs,
            fixture.central_double_draw_pairs
        )
    );
    let statistics = pentanomial_statistics(&sample, fixture.z).unwrap();
    assert_eq!(statistics.pairs, fixture.pairs);
    assert_eq!(statistics.unpaired_games, fixture.unpaired_games);
    assert_close(&fixture.id, "score", statistics.score, fixture.score, 1e-12);
    assert_close(
        &fixture.id,
        "variance",
        statistics.variance,
        fixture.variance,
        1e-12,
    );
    assert_close(
        &fixture.id,
        "standard_error",
        statistics.standard_error,
        fixture.standard_error,
        1e-12,
    );
    assert_close(
        &fixture.id,
        "logistic_elo",
        statistics.logistic_elo.elo,
        fixture.logistic_elo,
        1e-12,
    );
    assert_close(
        &fixture.id,
        "logistic_margin",
        statistics.logistic_elo.margin(),
        fixture.logistic_margin,
        1e-9,
    );
    assert_close(
        &fixture.id,
        "normalized_elo",
        statistics.normalized_elo.elo,
        fixture.normalized_elo,
        1e-12,
    );
    assert_close(
        &fixture.id,
        "normalized_margin",
        statistics.normalized_elo.margin(),
        fixture.normalized_margin,
        1e-9,
    );
    assert_close(&fixture.id, "los", statistics.los, fixture.los, 1e-7);
    assert_close(
        &fixture.id,
        "draw_ratio",
        statistics.draw_ratio,
        fixture.draw_ratio,
        1e-12,
    );
    assert_close(
        &fixture.id,
        "pairs_ratio",
        statistics.pairs_ratio.unwrap(),
        fixture.pairs_ratio,
        1e-12,
    );
    assert_close(
        &fixture.id,
        "win_loss_to_double_draw_ratio",
        statistics.win_loss_to_double_draw_ratio.unwrap(),
        fixture.win_loss_to_double_draw_ratio,
        1e-12,
    );

    let fixture = &fixtures.sprt[0];
    assert_eq!(fixture.id, "both-models-1150-pairs");
    let sample = sample_from_counts(fixture.counts, 0, 0);
    for (model, expected_llr) in [
        (EloModel::Logistic, fixture.logistic_llr),
        (EloModel::Normalized, fixture.normalized_llr),
    ] {
        let result = pentanomial_sprt(
            &sample,
            model,
            fixture.elo0,
            fixture.elo1,
            fixture.alpha,
            fixture.beta,
        )
        .unwrap();
        assert_eq!(result.model, model);
        assert_eq!(result.pairs, sample.pairs());
        assert_close(&fixture.id, "LLR", result.llr, expected_llr, 1e-9);
        assert_close(&fixture.id, "lower", result.lower, fixture.lower, 1e-12);
        assert_close(&fixture.id, "upper", result.upper, fixture.upper, 1e-12);
    }

    let fixture = &fixtures.trinomial_sprt[0];
    assert_eq!(fixture.id, "logistic-1000-games");
    let result = sprt(
        fixture.wins,
        fixture.draws,
        fixture.losses,
        fixture.elo0,
        fixture.elo1,
        fixture.alpha,
        fixture.beta,
    )
    .unwrap();
    assert_close(&fixture.id, "LLR", result.llr, fixture.llr, 1e-12);
    assert_close(&fixture.id, "lower", result.lower, fixture.lower, 1e-12);
    assert_close(&fixture.id, "upper", result.upper, fixture.upper, 1e-12);
    let expected_decision = match fixture.decision.as_str() {
        "accept-h0" => SprtDecision::AcceptH0,
        "accept-h1" => SprtDecision::AcceptH1,
        "continue" => SprtDecision::Continue,
        other => panic!("unknown SPRT decision {other:?}"),
    };
    assert_eq!(result.decision, expected_decision);

    let fixture = &fixtures.fixed_n[0];
    assert_eq!(fixture.id, "symmetric-assumed-distribution");
    let distribution = PentanomialDistribution::new(fixture.probabilities).unwrap();
    assert_close(
        &fixture.id,
        "variance",
        distribution.variance(),
        fixture.variance,
        1e-12,
    );
    let tails = match fixture.tails.as_str() {
        "one-sided" => FixedNTestTails::OneSided,
        "two-sided" => FixedNTestTails::TwoSided,
        other => panic!("unknown fixed-N tails {other:?}"),
    };
    for (model, expected_pairs) in [
        (EloModel::Normalized, fixture.normalized_required_pairs),
        (EloModel::Logistic, fixture.logistic_required_pairs),
    ] {
        let plan = fixed_n_plan(
            distribution,
            model,
            tails,
            fixture.target_effect,
            fixture.significance,
            fixture.power,
        )
        .unwrap();
        assert_eq!(plan.model, model);
        assert_eq!(plan.required_pairs, expected_pairs);
    }

    let fixture = &fixtures.achieved_resolution[0];
    assert_eq!(fixture.id, "both-models-1150-pairs");
    let sample = sample_from_counts(fixture.counts, 0, fixture.unpaired_games);
    for (model, estimate, lower, upper, resolution) in [
        (
            EloModel::Normalized,
            fixture.normalized_estimate,
            fixture.normalized_lower,
            fixture.normalized_upper,
            fixture.normalized_resolution,
        ),
        (
            EloModel::Logistic,
            fixture.logistic_estimate,
            fixture.logistic_lower,
            fixture.logistic_upper,
            fixture.logistic_resolution,
        ),
    ] {
        let achieved = fixed_n_achieved_resolution(&sample, model, fixture.significance).unwrap();
        assert_eq!(achieved.model, model);
        assert_eq!(achieved.unpaired_games, fixture.unpaired_games);
        assert_close(
            &fixture.id,
            "confidence",
            achieved.confidence,
            fixture.confidence,
            1e-12,
        );
        assert_close(&fixture.id, "estimate", achieved.estimate, estimate, 1e-9);
        assert_close(
            &fixture.id,
            "lower",
            achieved.lower,
            lower,
            NORMAL_QUANTILE_ELO_TOLERANCE,
        );
        assert_close(
            &fixture.id,
            "upper",
            achieved.upper,
            upper,
            NORMAL_QUANTILE_ELO_TOLERANCE,
        );
        assert_close(
            &fixture.id,
            "resolution",
            achieved.resolution(),
            resolution,
            NORMAL_QUANTILE_ELO_TOLERANCE,
        );
    }

    for fixture in &fixtures.error {
        assert_eq!(fixture.operation, "pentanomial_statistics");
        let sample = sample_from_counts(fixture.counts, 0, 0);
        let error = pentanomial_statistics(&sample, 1.959_963_984_540_054).unwrap_err();
        assert_eq!(
            statistics_error_name(error),
            fixture.error,
            "{}",
            fixture.id
        );
    }
}

#[test]
fn compatible_external_cells_match_reviewed_console_and_pgn_artifacts() {
    let fixtures: ExternalFixtures = toml::from_str(EXTERNAL_TOML).unwrap();
    assert_eq!(fixtures.schema_version, 1);
    assert_eq!(fixtures.observation.len(), 2);

    for observation in &fixtures.observation {
        let (console, pgn) = external_artifacts(observation);
        let expected_console_score = collapsed(&observation.console_score);
        assert!(
            console
                .lines()
                .map(collapsed)
                .any(|line| line == expected_console_score),
            "{} console has no matching final W/D/L score",
            observation.id
        );

        let games = parse_pgn_games(pgn);
        assert_eq!(
            games.len() as u32,
            observation.wins + observation.draws + observation.losses,
            "{} PGN game count",
            observation.id
        );

        let mut rounds: BTreeMap<&str, Vec<&PgnGame>> = BTreeMap::new();
        let mut wins = 0;
        let mut draws = 0;
        let mut losses = 0;
        for game in &games {
            rounds.entry(&game.round).or_default().push(game);
            match game_result_for(&observation.engine_a, &observation.engine_b, game) {
                PairGameResult::Win => wins += 1,
                PairGameResult::Draw => draws += 1,
                PairGameResult::Loss => losses += 1,
            }
        }
        assert_eq!(
            (wins, draws, losses),
            (observation.wins, observation.draws, observation.losses)
        );

        let mut sample = PentanomialVector::default();
        for (round, pair) in rounds {
            assert_eq!(
                pair.len(),
                2,
                "{} round {round} is not a complete pair",
                observation.id
            );
            assert_eq!(
                pair[0].white, pair[1].black,
                "{} round {round} did not swap White",
                observation.id
            );
            assert_eq!(
                pair[0].black, pair[1].white,
                "{} round {round} did not swap Black",
                observation.id
            );
            sample.record_pair(
                game_result_for(&observation.engine_a, &observation.engine_b, pair[0]),
                game_result_for(&observation.engine_a, &observation.engine_b, pair[1]),
            );
        }
        assert_eq!(
            sample.pairs(),
            observation.complete_pairs,
            "{} pair count",
            observation.id
        );
        assert_eq!(
            sample.counts(),
            observation.counts,
            "{} pentanomial counts",
            observation.id
        );

        let games = f64::from(wins + draws + losses);
        let score = (f64::from(wins) + f64::from(draws) / 2.0) / games;
        let draw_ratio = f64::from(draws) / games;
        assert_close(&observation.id, "score", score, observation.score, 1e-12);
        assert_close(
            &observation.id,
            "draw_ratio",
            draw_ratio,
            observation.draw_ratio,
            1e-12,
        );

        // The oracle matrix explicitly excludes Elo/interval parity for these
        // clean sweeps. Colosseum must retain its typed-error contract instead
        // of accepting the runners' infinity/NaN presentation.
        let error = elo_with_error(wins, draws, losses, 1.959_963_984_540_054).unwrap_err();
        assert_eq!(statistics_error_name(error), "InvalidScore");
    }
}

#[test]
fn phase_4b_ordered_fastchess_stream_has_the_same_terminal_pair() {
    let fixtures: Phase4bParity = toml::from_str(PHASE_4B_PARITY_TOML).unwrap();
    assert_eq!(fixtures.schema_version, 1);
    let fixture = &fixtures.terminal_replay;
    assert_eq!(fixture.runner, "fastchess");
    assert!(fixture.runner_version.starts_with("fastchess alpha 1.8.0"));
    assert_eq!(fixture.runner_sha256.len(), 64);
    assert_eq!(fixture.console, "external/phase4b5-fastchess.console.txt");
    assert_eq!(
        sha256_hex(PHASE_4B_FASTCHESS_CONSOLE),
        fixture.console_sha256
    );
    assert_eq!(fixture.openings, "external/phase4b5-openings.epd");
    assert_eq!(sha256_hex(PHASE_4B_OPENINGS), fixture.openings_sha256);
    assert_eq!(fixture.engine_a, "Rarog 2.3.1");
    assert_eq!(fixture.engine_b, "Stockfish 18");
    assert_eq!(fixture.engine_a_sha256.len(), 64);
    assert_eq!(fixture.engine_b_sha256.len(), 64);
    assert!(fixture.command.contains("model=normalized"));
    assert!(fixture.command.contains("-maxmoves 50"));
    assert!(fixture.scheduled_pair_cap > fixture.terminal_pair);
    assert!(fixture.elapsed_seconds < 30 * 60);
    assert_eq!(fixture.model, "normalized");

    let games = parse_finished_games(PHASE_4B_FASTCHESS_CONSOLE);
    assert_eq!(games.len() as u32, fixture.terminal_pair * 2);
    let mut sample = PentanomialVector::default();
    let mut wdl = [0_u32; 3];
    let mut observed_terminal = None;
    let mut terminal_result = None;
    for (index, pair) in games.chunks_exact(2).enumerate() {
        assert_eq!(pair[0].white, pair[1].black, "pair did not swap White");
        assert_eq!(pair[0].black, pair[1].white, "pair did not swap Black");
        let first = console_result_for("Rarog", "Stockfish", &pair[0]);
        let second = console_result_for("Rarog", "Stockfish", &pair[1]);
        for result in [first, second] {
            match result {
                PairGameResult::Win => wdl[0] += 1,
                PairGameResult::Draw => wdl[1] += 1,
                PairGameResult::Loss => wdl[2] += 1,
            }
        }
        sample.record_pair(first, second);
        let result = pentanomial_sprt(
            &sample,
            EloModel::Normalized,
            fixture.elo0,
            fixture.elo1,
            fixture.alpha,
            fixture.beta,
        );
        let result = match result {
            Ok(result) => result,
            Err(StatisticsError::InsufficientPairs { .. } | StatisticsError::ZeroVariance) => {
                assert!(observed_terminal.is_none());
                continue;
            }
            Err(error) => panic!("unexpected ordered-prefix error: {error}"),
        };
        if result.decision == SprtDecision::Continue {
            assert!(observed_terminal.is_none());
        } else {
            assert!(observed_terminal.is_none(), "more than one terminal prefix");
            observed_terminal = Some(index as u32 + 1);
            terminal_result = Some(result);
        }
    }

    assert_eq!(observed_terminal, Some(fixture.terminal_pair));
    assert_eq!(sample.counts(), fixture.counts);
    assert_eq!(wdl, [fixture.wins, fixture.draws, fixture.losses]);
    let result = terminal_result.unwrap();
    assert_eq!(fixture.decision, "accept-h0");
    assert_eq!(result.decision, SprtDecision::AcceptH0);
    assert_close(
        "phase4b-fastchess",
        "LLR",
        result.llr,
        fixture.reported_llr,
        fixture.display_tolerance,
    );
    assert_close(
        "phase4b-fastchess",
        "lower",
        result.lower,
        fixture.reported_lower,
        fixture.display_tolerance,
    );
    assert_close(
        "phase4b-fastchess",
        "upper",
        result.upper,
        fixture.reported_upper,
        fixture.display_tolerance,
    );
    assert!(PHASE_4B_FASTCHESS_CONSOLE.contains("completed - H0 was accepted"));
}

#[test]
fn phase_4b_controlled_live_parity_compares_only_shared_fields() {
    let fixtures: Phase4bParity = toml::from_str(PHASE_4B_PARITY_TOML).unwrap();
    let live = &fixtures.live;
    assert!(live.host.contains("Windows 11"));
    assert_eq!(live.date, "2026-08-01");
    assert_eq!(live.engine, "Rarog 2.3.1");
    assert_eq!(live.engine_sha256.len(), 64);
    assert!(live.conditions.contains("same executable both arms"));
    assert!(live.shared_fields.contains(&"W/D/L".to_string()));
    assert_eq!(
        live.conditionally_shared_fields,
        ["pentanomial vector (Fastchess and Colosseum only)"]
    );
    assert!(live.excluded_fields.contains(&"LLR".to_string()));
    assert!(live.excluded_fields.contains(&"LOS".to_string()));
    assert!(live.exclusion_reason.contains("zero variance"));
    assert_eq!(live.observation.len(), 3);

    let expected_runners = ["fastchess", "cutechess-cli", "colosseum-cli"];
    for (observation, expected_runner) in live.observation.iter().zip(expected_runners) {
        assert_eq!(observation.runner, expected_runner);
        assert!(!observation.runner_version.is_empty());
        assert_eq!(observation.runner_sha256.len(), 64);
        assert!(!observation.command.is_empty());
        assert_eq!(observation.games, 8);
        assert_eq!(
            (observation.wins, observation.draws, observation.losses),
            (0, 8, 0)
        );
        assert_eq!(observation.complete_pairs, 4);
        assert_eq!(observation.faults, 0);
    }

    for (observation, console) in [
        (&live.observation[0], PHASE_4B_LIVE_FASTCHESS_CONSOLE),
        (&live.observation[1], PHASE_4B_LIVE_CUTECHESS_CONSOLE),
    ] {
        assert_eq!(sha256_hex(console), observation.artifact_sha256);
        let (sample, wdl) = console_sample(console, "A", "B");
        assert_eq!(sample.pairs(), observation.complete_pairs);
        assert_eq!(
            wdl,
            [observation.wins, observation.draws, observation.losses]
        );
        assert_eq!(sample.draw_ratio().unwrap(), 1.0);
        assert_eq!(parse_finished_games(console).len(), 8);
        assert!(console.contains("Draw by adjudication"));
    }
    assert_eq!(live.observation[0].counts, Some([0, 0, 4, 0, 0]));
    assert_eq!(live.observation[1].counts, None);

    let observation = &live.observation[2];
    assert_eq!(
        observation.artifact,
        "external/phase4b5-live-colosseum.json"
    );
    assert_eq!(
        sha256_hex(PHASE_4B_LIVE_COLOSSEUM),
        observation.artifact_sha256
    );
    assert_eq!(
        observation.raw_artifact_sha256.as_deref().unwrap().len(),
        64
    );
    let projection: ColosseumLiveProjection =
        serde_json::from_str(PHASE_4B_LIVE_COLOSSEUM).unwrap();
    assert!(projection.source.contains("reviewed projection"));
    assert_eq!(projection.status, observation.status);
    assert_eq!(projection.exit_code, observation.exit_code);
    assert_eq!(projection.games, observation.games);
    assert_eq!(
        (projection.wins, projection.draws, projection.losses),
        (observation.wins, observation.draws, observation.losses)
    );
    assert_eq!(projection.complete_pairs, observation.complete_pairs);
    assert_eq!(projection.pentanomial, observation.counts.unwrap());
    assert_eq!(projection.colour_split.a_as_white, [0, 4, 0]);
    assert_eq!(projection.colour_split.a_as_black, [0, 4, 0]);
    assert_eq!(
        projection.faults.engine_a
            + projection.faults.engine_b
            + projection.faults.time_losses_a
            + projection.faults.time_losses_b
            + projection.faults.infrastructure,
        observation.faults
    );
    assert_eq!(live.observation[0].status, "completed");
    assert_eq!(live.observation[0].exit_code, 0);
    assert_eq!(live.observation[1].status, "completed");
    assert_eq!(live.observation[1].exit_code, 0);
    assert_eq!(observation.status, "inconclusive");
    assert_eq!(observation.exit_code, 4);
}
