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

#[derive(Debug, Default)]
struct PgnGame {
    round: String,
    white: String,
    black: String,
    result: String,
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
