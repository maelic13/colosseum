//! The `stats` command: replay, and the offline experiment planners.

use super::*;

#[derive(Debug, Args)]
pub(crate) struct StatsCommand {
    /// Run directory or structured JSON, PGN, log, or console-text file.
    pub(crate) input: Option<PathBuf>,
    /// Engine name used as the perspective for PGN replay.
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
                if let Some(reason) = &report.paired_statistics_unavailable {
                    println!("paired statistics unavailable: {reason}");
                }
                println!("search telemetry: {}", report.telemetry.status);
                if let Some(reason) = &report.telemetry.unavailable_reason {
                    println!("telemetry unavailable: {reason}");
                }
                for engine in &report.telemetry.engines {
                    println!(
                        "{}: {}/{} annotated moves ({:.1}%); depth mean/median {}; elapsed mean/median {}; implied NPS mean/median {}",
                        engine.engine,
                        engine.annotated_moves,
                        engine.eligible_moves,
                        engine.annotation_coverage * 100.0,
                        metric_text(&engine.depth),
                        metric_text(&engine.elapsed_seconds),
                        metric_text(&engine.implied_nps),
                    );
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
