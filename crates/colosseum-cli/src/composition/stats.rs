//! The `stats` command: replay, and the offline experiment planners.

use super::*;

#[derive(Debug, Args)]
pub(crate) struct StatsCommand {
    /// Run directory or structured JSON, PGN, log, or console-text file.
    pub(crate) input: Option<PathBuf>,
    /// The engine whose side the replay reports from, by name. A run
    /// directory refuses a name that is not one of its engines.
    #[arg(long)]
    pub(crate) subject: Option<String>,
    #[command(subcommand)]
    pub(crate) action: Option<StatsAction>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum StatsAction {
    /// Plan prospective fixed-N or expected SPRT length.
    Plan(StatsPlanCommand),
}

#[derive(Debug, Args)]
pub(crate) struct StatsPlanCommand {
    #[command(subcommand)]
    pub(crate) action: StatsPlanAction,
}

#[derive(Debug, Subcommand)]
pub(crate) enum StatsPlanAction {
    /// Estimate fixed-N pairs under explicit distribution assumptions.
    Fixed(StatsFixedPlanCommand),
    /// Simulate a capped SPRT length distribution under explicit assumptions.
    Sprt(StatsSprtPlanCommand),
}

#[derive(Debug, Args)]
pub(crate) struct StatsFixedPlanCommand {
    #[arg(long, value_enum)]
    pub(crate) objective: FixedPlanObjectiveArg,
    #[arg(long, value_enum)]
    pub(crate) model: SprtModelArg,
    /// Positive effect (difference) or symmetric margin (equivalence), in selected Elo model.
    #[arg(long)]
    pub(crate) effect_or_margin: f64,
    #[arg(long, default_value_t = 0.05)]
    pub(crate) significance: f64,
    #[arg(long, default_value_t = 0.8)]
    pub(crate) power: f64,
    /// Five comma-separated pentanomial probabilities summing to one.
    #[arg(long)]
    pub(crate) distribution: String,
    /// Optional five comma-separated observed bin counts for achieved resolution.
    #[arg(long)]
    pub(crate) observed_pentanomial: Option<String>,
}

#[derive(Debug, Args)]
pub(crate) struct StatsSprtPlanCommand {
    #[arg(long, value_enum)]
    pub(crate) model: SprtModelArg,
    #[arg(long)]
    pub(crate) elo0: f64,
    #[arg(long)]
    pub(crate) elo1: f64,
    #[arg(long, default_value_t = 0.05)]
    pub(crate) alpha: f64,
    #[arg(long, default_value_t = 0.05)]
    pub(crate) beta: f64,
    /// Five comma-separated assumed true pentanomial probabilities.
    #[arg(long)]
    pub(crate) distribution: String,
    #[arg(long, default_value_t = 1_000, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) simulations: u32,
    #[arg(long, value_parser = clap::value_parser!(u32).range(2..))]
    pub(crate) max_pairs: u32,
    #[arg(long, default_value_t = 0)]
    pub(crate) seed: u64,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum FixedPlanObjectiveArg {
    Difference,
    Equivalence,
}

impl From<FixedPlanObjectiveArg> for FixedPlanObjective {
    fn from(value: FixedPlanObjectiveArg) -> Self {
        match value {
            FixedPlanObjectiveArg::Difference => Self::Difference,
            FixedPlanObjectiveArg::Equivalence => Self::Equivalence,
        }
    }
}

pub(crate) fn run_stats(command: StatsCommand, machine: bool) -> ExitCode {
    if let Some(StatsAction::Plan(plan)) = command.action {
        if command.input.is_some() || command.subject.is_some() {
            eprintln!("configuration error: stats plan does not accept replay input or --subject");
            return ExitCode::from(2);
        }
        return run_stats_plan(plan.action, machine);
    }
    let Some(input) = command.input else {
        eprintln!("configuration error: stats requires an input path or the plan subcommand");
        return ExitCode::from(2);
    };
    match crate::stats_replay::replay(&input, command.subject.as_deref()) {
        Ok(report) => {
            for warning in &report.warnings {
                eprintln!("stats warning: {warning}");
            }
            if machine {
                print_json(&MachineOutput::StatsReplay { report });
            } else {
                println!(
                    "authority: {} ({})",
                    report.authority,
                    report.source.display()
                );
                println!("perspective: {}", report.perspective);
                println!(
                    "{} games: {} wins, {} draws, {} losses; score {:.6}",
                    report.games, report.wins, report.draws, report.losses, report.score
                );
                println!(
                    "pairing: {}; {} complete pairs, {} unpaired games",
                    report.pairing, report.complete_pairs, report.unpaired_games
                );
                if let Some(vector) = report.pentanomial {
                    println!("pentanomial: {vector:?}");
                }
                if let Some(by_colour) = &report.by_colour {
                    for line in colour_lines(by_colour) {
                        println!("{line}");
                    }
                }
                if let Some(reason) = &report.paired_statistics_unavailable {
                    println!("paired statistics unavailable: {reason}");
                }
                if let Some(tournament) = &report.tournament {
                    for line in tournament_lines(tournament) {
                        println!("{line}");
                    }
                }
                if let Some(terminations) = &report.terminations {
                    for line in termination_lines(terminations) {
                        println!("{line}");
                    }
                }
                if let Some(faults) = &report.faults {
                    for line in fault_lines(faults) {
                        println!("{line}");
                    }
                }
                println!("search telemetry: {}", report.telemetry.status);
                if let Some(reason) = &report.telemetry.unavailable_reason {
                    println!("telemetry unavailable: {reason}");
                }
                for engine in &report.telemetry.engines {
                    println!(
                        "{}: {}/{} annotated moves ({:.1}%); depth mean/median {}; non-mate depth mean/median {} over {} moves; nodes per move mean/median {}; elapsed mean/median {}; implied NPS mean/median {}; total nodes over total time {}",
                        engine.engine,
                        engine.annotated_moves,
                        engine.eligible_moves,
                        engine.annotation_coverage * 100.0,
                        metric_text(&engine.depth),
                        metric_text(&engine.non_mate_depth),
                        engine.non_mate_depth.samples,
                        metric_text(&engine.nodes),
                        metric_text(&engine.elapsed_seconds),
                        metric_text(&engine.implied_nps),
                        engine
                            .nodes_per_second
                            .map_or_else(|| "unavailable".to_owned(), |nps| format!("{nps:.3}")),
                    );
                    if let Some(line) = overhead_text(&engine.harness_overhead_ms) {
                        println!("{}: {line}", engine.engine);
                    }
                }
                if let Some(phases) = &report.game_phases {
                    println!(
                        "game phases over {} games, ms mean / p50 / p90 / p99 / max:",
                        phases.games
                    );
                    for phase in &phases.phases {
                        println!(
                            "  {:<17} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1}",
                            phase.phase,
                            phase.mean_ms,
                            phase.p50_ms,
                            phase.p90_ms,
                            phase.p99_ms,
                            phase.max_ms
                        );
                    }
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("statistics replay failed: {error}");
            ExitCode::FAILURE
        }
    }
}

/// One line per engine: its games as White, then as Black, each from its own
/// side of the board.
pub(crate) fn colour_lines(by_colour: &[crate::stats_replay::EngineColourResults]) -> Vec<String> {
    let record = |record: &crate::stats_replay::ColourRecord| match record.score {
        Some(score) => format!(
            "{} W / {} D / {} L of {}, score {:.1}%",
            record.wins,
            record.draws,
            record.losses,
            record.games,
            score * 100.0
        ),
        None => "no games".to_owned(),
    };
    by_colour
        .iter()
        .map(|engine| {
            format!(
                "{}: as White {}; as Black {}",
                engine.engine,
                record(&engine.as_white),
                record(&engine.as_black)
            )
        })
        .collect()
}

/// What a run prints at its end after its own statistics, in the figures
/// `stats` replays from the same run directory: a two-engine run's colours or
/// a tournament's standings and pairs, how the games ended, the faults and
/// each engine's search.
pub(crate) fn run_results_lines(results: &crate::stats_replay::RunResults) -> Vec<String> {
    let mut lines = colour_lines(&results.by_colour);
    if let Some(tournament) = &results.tournament {
        lines.extend(tournament_lines(tournament));
    }
    if let Some(terminations) = &results.terminations {
        lines.extend(termination_lines(terminations));
    }
    if let Some(faults) = &results.faults {
        lines.extend(fault_lines(faults));
    }
    if let Some(reason) = &results.analysis_unavailable {
        lines.push(format!("game analysis unavailable: {reason}"));
    }
    let telemetry = &results.telemetry;
    if let Some(reason) = &telemetry.unavailable_reason {
        lines.push(format!("search telemetry unavailable: {reason}"));
    }
    for engine in &telemetry.engines {
        let mut parts = vec![format!(
            "{} of {} moves annotated ({:.1}%)",
            grouped(engine.annotated_moves.into()),
            grouped(engine.eligible_moves.into()),
            engine.annotation_coverage * 100.0
        )];
        if let (Some(mean), Some(median)) = (engine.depth.mean, engine.depth.median) {
            parts.push(format!("depth {mean:.2} mean, {median} median"));
        }
        if let (Some(mean), Some(median)) =
            (engine.non_mate_depth.mean, engine.non_mate_depth.median)
        {
            parts.push(format!(
                "without mate scores {mean:.2} mean, {median} median over {} moves",
                grouped(engine.non_mate_depth.samples.into())
            ));
        }
        if let Some(mean) = engine.nodes.mean {
            parts.push(format!("{} nodes per move", grouped(mean.round() as u64)));
        }
        if let Some(mean) = engine.elapsed_seconds.mean {
            parts.push(format!("{:.1} ms per move", mean * 1_000.0));
        }
        if let Some(nps) = engine.nodes_per_second {
            parts.push(format!("{:.2}M nodes per second", nps / 1e6));
        }
        lines.push(format!("{}: {}", engine.engine, parts.join("; ")));
    }
    lines
}

/// `+172.8`, or the bound that does not exist.
fn elo_text(elo: Option<f64>) -> String {
    elo.map_or_else(|| "unbounded".to_owned(), |elo| format!("{elo:+.1}"))
}

/// `248-88-64 of 400, 73.0%, Elo +172.8 [+141.5, +206.9]`.
fn score_elo_text(result: &colosseum_core::ScoreElo) -> String {
    let score = result.score.map_or_else(
        || "no games".to_owned(),
        |score| format!("{:.1}%", score * 100.0),
    );
    format!(
        "{}-{}-{} of {}, {score}, Elo {} [{}, {}]",
        result.wins,
        result.draws,
        result.losses,
        result.games,
        elo_text(result.elo),
        elo_text(result.elo_lower),
        elo_text(result.elo_upper)
    )
}

fn pair_line(pair: &crate::run_analysis::PairReport) -> Vec<String> {
    let mut line = format!("vs {}: {}", pair.opponent, score_elo_text(&pair.result));
    if let (Some(vector), Some(elo)) = (pair.pentanomial, pair.pentanomial_elo) {
        line.push_str(&format!(
            "; Ptnml {vector:?}, Elo {:+.1} [{:+.1}, {:+.1}]",
            elo.elo, elo.lower, elo.upper
        ));
    }
    let mut lines = vec![line];
    if let Some(excluding) = &pair.excluding_faults {
        lines.push(format!(
            "  excluding {} fault-scored {}: {}",
            pair.fault_scored_games,
            if pair.fault_scored_games == 1 {
                "game"
            } else {
                "games"
            },
            score_elo_text(excluding)
        ));
    }
    lines
}

/// Every participant's rating, then each pair from its engine's side: one
/// block for a gauntlet's seed, one per engine for a round robin.
pub(crate) fn tournament_lines(
    tournament: &crate::run_analysis::TournamentAnalysis,
) -> Vec<String> {
    let mut lines = Vec::new();
    match &tournament.standings_unavailable {
        Some(reason) => lines.push(format!("standings unavailable: {reason}")),
        None => {
            lines.push("standings, ratings with 95% intervals:".to_owned());
            lines.extend(standing_lines(&tournament.standings));
        }
    }
    lines.push(
        "pairs, each from its engine's side; Elo by the trinomial formula with a 95% interval:"
            .to_owned(),
    );
    let mut engine = None;
    for pair in &tournament.pairs {
        if engine != Some(&pair.engine) {
            lines.push(format!("  {}:", pair.engine));
            engine = Some(&pair.engine);
        }
        for line in pair_line(pair) {
            lines.push(format!("    {line}"));
        }
    }
    lines
}

/// `1. Name 3428.8 +/- 19.2 [3409.6, 3448.0] - 2007/2400 points (1787-440-173)`.
pub(crate) fn standing_lines(
    standings: &[colosseum_application::TournamentStanding],
) -> Vec<String> {
    standings
        .iter()
        .map(|standing| {
            let rating = if standing.fixed {
                format!("{:.1} (fixed)", standing.rating)
            } else {
                match standing.error_95 {
                    Some(error) => format!(
                        "{:.1} +/- {error:.1} [{:.1}, {:.1}]",
                        standing.rating,
                        standing.rating - error,
                        standing.rating + error
                    ),
                    None => format!("{:.1} (interval unavailable)", standing.rating),
                }
            };
            format!(
                "  {}. {} {rating} - {}/{} points ({}-{}-{})",
                standing.rank,
                standing.name,
                standing.points,
                standing.games,
                standing.wins,
                standing.draws,
                standing.losses
            )
        })
        .collect()
}

/// `Checkmate 1960, Threefold 291`, most frequent first.
fn counts_text(counts: &BTreeMap<String, u32>) -> String {
    if counts.is_empty() {
        return "none".to_owned();
    }
    let mut sorted = counts.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    sorted
        .into_iter()
        .map(|(kind, count)| format!("{kind} {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(crate) fn termination_lines(report: &crate::run_analysis::TerminationReport) -> Vec<String> {
    let mut lines = vec![format!("terminations: {}", counts_text(&report.overall))];
    for engine in &report.by_engine {
        lines.push(format!(
            "  {}: won {}; drew {}; lost {}",
            engine.engine,
            counts_text(&engine.wins),
            counts_text(&engine.draws),
            counts_text(&engine.losses)
        ));
    }
    lines
}

pub(crate) fn fault_lines(report: &crate::run_analysis::FaultReport) -> Vec<String> {
    let faults = if report.faults.is_empty() {
        "none".to_owned()
    } else {
        report
            .faults
            .iter()
            .map(|fault| {
                format!(
                    "{} {} ({}) during {} x{}, scored as {}",
                    fault.engine,
                    fault.termination,
                    fault.kind,
                    fault.phase,
                    fault.games,
                    if fault.scored_as == "not scored" {
                        "nothing: not scored".to_owned()
                    } else {
                        format!("a {}", fault.scored_as)
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    let time_losses = report
        .time_losses
        .iter()
        .filter(|engine| engine.games > 0)
        .map(|engine| format!("{} {}", engine.engine, engine.games))
        .collect::<Vec<_>>();
    vec![
        format!("faults: {faults}"),
        format!(
            "time losses: {}",
            if time_losses.is_empty() {
                "none".to_owned()
            } else {
                time_losses.join(", ")
            }
        ),
    ]
}

pub(crate) fn print_run_results(results: Option<&crate::stats_replay::RunResults>) {
    for line in results.map(run_results_lines).unwrap_or_default() {
        println!("{line}");
    }
}

/// `1234567` as `1,234,567`.
pub(crate) fn grouped(value: u64) -> String {
    let digits = value.to_string();
    let mut text = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

/// The harness overhead of an engine's moves in one line, or nothing when the
/// source carried none.
pub(crate) fn overhead_text(metric: &crate::pgn_telemetry::OverheadMetric) -> Option<String> {
    let (Some(p50), Some(p99), Some(p999), Some(max)) =
        (metric.p50, metric.p99, metric.p999, metric.max)
    else {
        return None;
    };
    let margin = if metric.moves_with_margin > 0 {
        format!(
            "{} of {} moves over the time margin",
            metric.over_margin, metric.moves_with_margin
        )
    } else {
        "time margin not recorded".to_owned()
    };
    Some(format!(
        "harness overhead p50 {p50:.0} ms, p99 {p99:.0} ms, p999 {p999:.0} ms, max {max:.0} ms over {} moves; {margin}",
        metric.samples
    ))
}

pub(crate) fn metric_text(metric: &crate::pgn_telemetry::TelemetryMetric) -> String {
    match (metric.mean, metric.median) {
        (Some(mean), Some(median)) => format!("{mean:.3}/{median:.3}"),
        _ => "unavailable".into(),
    }
}

pub(crate) fn run_stats_plan(action: StatsPlanAction, machine: bool) -> ExitCode {
    match action {
        StatsPlanAction::Fixed(command) => {
            let request = match parse_fixed_plan_request(command) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("configuration error: {error}");
                    return ExitCode::from(2);
                }
            };
            match plan_fixed(request) {
                Ok(report) => {
                    if machine {
                        print_json(&MachineOutput::StatsFixedPlan { report });
                    } else {
                        println!(
                            "required: {} complete pairs ({} games)",
                            report.required_pairs, report.required_games
                        );
                        println!(
                            "planned score half-width: {:.8}",
                            report.planned_score_half_width
                        );
                        if let Some(achieved) = &report.achieved_resolution {
                            println!(
                                "observed {} pairs: estimate {:.4}, interval {:.4}..{:.4}, conservative resolution {:.4}",
                                achieved.pairs,
                                achieved.estimate,
                                achieved.lower,
                                achieved.upper,
                                achieved.conservative_resolution
                            );
                        }
                        println!("interpretation: {}", report.interpretation);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("statistics plan failed: {error}");
                    ExitCode::from(2)
                }
            }
        }
        StatsPlanAction::Sprt(command) => {
            let distribution = match parse_f64_five(&command.distribution) {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("configuration error: {error}");
                    return ExitCode::from(2);
                }
            };
            let request = SprtLengthPlanRequest {
                model: command.model.into(),
                elo0: command.elo0,
                elo1: command.elo1,
                alpha: command.alpha,
                beta: command.beta,
                assumed_true_distribution: distribution,
                simulations: command.simulations,
                max_pairs: command.max_pairs,
                seed: command.seed,
            };
            match plan_sprt_length(request) {
                Ok(report) => {
                    if machine {
                        print_json(&MachineOutput::StatsSprtPlan { report });
                    } else {
                        println!(
                            "expected pairs: mean {:.2}, median {}, 5%..95% {}..{}; capped {} of {}",
                            report.mean_pairs,
                            report.median_pairs,
                            report.p05_pairs,
                            report.p95_pairs,
                            report.capped,
                            report.request.simulations
                        );
                        println!("interpretation: {}", report.interpretation);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("statistics plan failed: {error}");
                    ExitCode::from(2)
                }
            }
        }
    }
}

pub(crate) fn parse_fixed_plan_request(
    command: StatsFixedPlanCommand,
) -> Result<FixedPlanRequest, String> {
    Ok(FixedPlanRequest {
        objective: command.objective.into(),
        model: command.model.into(),
        effect_or_margin: command.effect_or_margin,
        significance: command.significance,
        power: command.power,
        assumed_distribution: parse_f64_five(&command.distribution)?,
        observed_pentanomial: command
            .observed_pentanomial
            .as_deref()
            .map(parse_u32_five)
            .transpose()?,
    })
}

pub(crate) fn parse_f64_five(value: &str) -> Result<[f64; 5], String> {
    let parsed = value
        .split(',')
        .map(|item| {
            item.parse::<f64>()
                .map_err(|_| format!("invalid number {item:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    parsed
        .try_into()
        .map_err(|values: Vec<f64>| format!("expected five values, got {}", values.len()))
}

pub(crate) fn parse_u32_five(value: &str) -> Result<[u32; 5], String> {
    let parsed = value
        .split(',')
        .map(|item| {
            item.parse::<u32>()
                .map_err(|_| format!("invalid count {item:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    parsed
        .try_into()
        .map_err(|values: Vec<u32>| format!("expected five counts, got {}", values.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_grouped_in_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(181_306), "181,306");
        assert_eq!(grouped(1_190_627), "1,190,627");
    }

    /// Not part of the suite: what a two-engine run would print and keep at
    /// its end, computed by the end-of-run code from a recorded run directory
    /// named by `COLOSSEUM_RECORDED_RUN`, without replaying its games.
    #[test]
    #[ignore = "reads a recorded run directory named by COLOSSEUM_RECORDED_RUN"]
    fn the_end_of_run_results_of_a_recorded_run() {
        let root = PathBuf::from(
            std::env::var_os("COLOSSEUM_RECORDED_RUN")
                .expect("COLOSSEUM_RECORDED_RUN names a run directory"),
        );
        let [a, b] = crate::stats_replay::run_engine_names(&root.join("result.json"))
            .expect("the run directory names its engines");
        let results = crate::stats_replay::run_results(&root, Some([a.as_str(), b.as_str()]));
        for line in run_results_lines(&results) {
            println!("{line}");
        }
        println!("{}", serde_json::to_string_pretty(&results).unwrap());
    }
}
