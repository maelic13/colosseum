//! The `spsa` command, its `plan` and `status` actions and its artifacts.

use super::*;

#[derive(Debug, Args)]
#[command(subcommand_precedence_over_arg = true, subcommand_negates_reqs = true)]
pub(crate) struct SpsaCommand {
    #[command(subcommand)]
    pub(crate) action: Option<SpsaAction>,

    #[command(flatten)]
    pub(crate) engine: SpsaEngineArgs,

    /// Ordered TOML parameter vector to tune against the live UCI schema.
    #[arg(long)]
    pub(crate) tune: Option<PathBuf>,
    /// Terminal SPSA gain ratio shared by every tuned parameter.
    #[arg(long)]
    pub(crate) r_end: Option<f64>,
    /// Number of SPSA centre updates in the tune horizon.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) iterations: Option<u32>,
    /// Complete games in each pair-atomic mini-match.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) games_per_iteration: Option<u32>,
    /// Average this percent of the fixed horizon instead of taking the final
    /// centre vector, which is the default estimator.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=100))]
    pub(crate) final_window_percent: Option<u32>,
    /// Stop cleanly after this many completed iterations without changing the
    /// stored horizon; the run can be resumed from its own directory.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) stop_after_iteration: Option<u32>,

    #[command(flatten)]
    pub(crate) conditions: SpsaConditions,
}

/// SPSA supports read-only nested commands that need no executable. The live
/// run path validates the executable before resolving the shared engine args.
#[derive(Debug, Args)]
pub(crate) struct SpsaEngineArgs {
    /// Path to the UCI engine executable; omitted only for a nested read-only command.
    pub(crate) executable: Option<PathBuf>,
    #[arg(long)]
    pub(crate) label: Option<String>,
    #[arg(long = "engine-arg", allow_hyphen_values = true)]
    pub(crate) arguments: Vec<OsString>,
    #[arg(long)]
    pub(crate) cwd: Option<PathBuf>,
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub(crate) environment: Vec<String>,
    #[arg(long = "option", value_name = "NAME=VALUE")]
    pub(crate) options: Vec<String>,
    #[arg(long = "button", value_name = "NAME")]
    pub(crate) buttons: Vec<String>,
    #[arg(long, value_name = "LIST")]
    pub(crate) cores: Option<String>,
}

impl SpsaEngineArgs {
    pub(crate) fn resolve(&self) -> Result<EngineLaunchSpec, String> {
        let executable = self
            .executable
            .clone()
            .ok_or_else(|| "an engine executable is required for a live SPSA run".to_owned())?;
        EngineArgs {
            executable,
            label: self.label.clone(),
            arguments: self.arguments.clone(),
            cwd: self.cwd.clone(),
            environment: self.environment.clone(),
            options: self.options.clone(),
            buttons: self.buttons.clone(),
            cores: self.cores.clone(),
        }
        .resolve()
        .map_err(|error| error.to_string())
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum SpsaAction {
    /// Report the exact gain schedule and factual workload without launching an engine.
    Plan(SpsaPlanCommand),
    /// Read the last durable tune snapshot and report labelled trajectory heuristics.
    Status {
        /// Self-contained SPSA run directory to inspect without mutation.
        run_directory: PathBuf,
    },
}

#[derive(Debug, Args)]
pub(crate) struct SpsaPlanCommand {
    /// Ordered TOML parameter vector whose schedule should be planned.
    #[arg(long)]
    pub(crate) tune: PathBuf,
    /// Terminal SPSA gain ratio shared by every tuned parameter.
    #[arg(long)]
    pub(crate) r_end: f64,
    /// Number of SPSA centre updates in the primary horizon.
    #[arg(long, default_value_t = DEFAULT_SPSA_ITERATIONS, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) iterations: u32,
    /// Complete games in each pair-atomic mini-match.
    #[arg(long, default_value_t = DEFAULT_SPSA_GAMES_PER_ITERATION, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) games_per_iteration: u32,
    /// Number of games expected to run concurrently within each iteration.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) concurrency: u32,
    /// Compare cost and first/final gain values at another horizon; repeatable.
    #[arg(long = "compare-iterations", value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) comparison_horizons: Vec<u32>,
    /// Lower end-to-end seconds-per-game assumption for a wall-time range.
    #[arg(
        long,
        requires = "seconds_per_game_high",
        conflicts_with = "pilot_game_seconds",
        value_parser = parse_positive_seconds
    )]
    pub(crate) seconds_per_game_low: Option<f64>,
    /// Upper end-to-end seconds-per-game assumption for a wall-time range.
    #[arg(
        long,
        requires = "seconds_per_game_low",
        conflicts_with = "pilot_game_seconds",
        value_parser = parse_positive_seconds
    )]
    pub(crate) seconds_per_game_high: Option<f64>,
    /// Observed end-to-end duration of one pilot game; repeat for a sample range.
    #[arg(long, value_parser = parse_positive_seconds)]
    pub(crate) pilot_game_seconds: Vec<f64>,
}

#[derive(Debug, Args)]
pub(crate) struct SpsaConditions {
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) movetime_ms: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) base_ms: Option<u64>,
    #[arg(long)]
    pub(crate) increment_ms: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) nodes: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) depth: Option<u32>,
    #[arg(long, default_value_t = match_runner::DEFAULT_MARGIN_MS)]
    pub(crate) margin_ms: u64,
    /// Let both perturbation arms think on the opponent's clock. Both then
    /// search at once, so each needs its own cores.
    #[arg(long, requires = "cores_per_engine")]
    pub(crate) ponder: bool,

    /// Adjudicate a draw once both engines agree; off unless requested.
    #[arg(long)]
    pub(crate) draw_adjudication: bool,
    #[arg(long, default_value_t = 40, value_parser = clap::value_parser!(u32).range(1..), requires = "draw_adjudication")]
    pub(crate) draw_move: u32,
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..), requires = "draw_adjudication")]
    pub(crate) draw_moves: u32,
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(i32).range(0..), requires = "draw_adjudication")]
    pub(crate) draw_score_cp: i32,
    /// Adjudicate a resignation once both engines agree; off unless requested.
    #[arg(long)]
    pub(crate) resign_adjudication: bool,
    /// Use only the losing engine's evaluations for resignation adjudication.
    #[arg(long, requires = "resign_adjudication")]
    pub(crate) one_sided_resign_adjudication: bool,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..), requires = "resign_adjudication")]
    pub(crate) resign_moves: u32,
    #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(i32).range(1..), requires = "resign_adjudication")]
    pub(crate) resign_score_cp: i32,
    /// Draw after this many full moves; omitted means no maximum-move cap.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) max_moves: Option<u32>,

    /// Invalidate the tune after more engine faults than this; a time loss is
    /// one. Omitted: 0.5% of the games played so far, at least 3.
    #[arg(long)]
    pub(crate) max_engine_faults: Option<u32>,
    /// Invalidate the tune after more time losses than this. Omitted: the
    /// engine-fault allowance, which already counts them.
    #[arg(long)]
    pub(crate) max_time_losses: Option<u32>,

    /// Number of games allowed to run at once.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) concurrency: u32,
    /// Physical cores per game slot, shared by both of its engines. This is
    /// the default allocation, because without pondering only one engine of a
    /// game searches at a time.
    #[arg(
        long,
        value_parser = clap::value_parser!(u32).range(1..),
        conflicts_with = "cores_per_engine"
    )]
    pub(crate) cores_per_game: Option<u32>,
    /// Physical cores allocated separately to each engine in each game slot.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) cores_per_engine: Option<u32>,
    #[arg(long, default_value = "off")]
    pub(crate) placement: String,
    /// Whole physical cores left free for the harness and the operating system.
    #[arg(long, default_value_t = colosseum_engine::DEFAULT_AUTO_HEADROOM_PHYSICAL_CORES)]
    pub(crate) headroom_cores: usize,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) memory_budget_mb: Option<u64>,

    /// Optional EPD or PGN opening book; it is parsed once per process session.
    #[arg(long)]
    pub(crate) book: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = BookOrderArg::Sequential)]
    pub(crate) book_order: BookOrderArg,
    #[arg(long, default_value_t = 0)]
    pub(crate) book_start: usize,
    /// Reuse openings from the start of the book once they run out, instead of
    /// refusing a schedule the book cannot cover.
    #[arg(long, requires = "book")]
    pub(crate) book_wrap: bool,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) book_plies: Option<u32>,
    #[arg(long)]
    pub(crate) seed: Option<u64>,

    #[arg(long = "dir")]
    pub(crate) run_directory: Option<PathBuf>,
    #[arg(long, requires = "run_directory")]
    pub(crate) restart: bool,
    /// Committed iterations between progress blocks on standard error.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) progress_every: u64,
    /// Shortest time between two progress blocks. A run whose iterations
    /// finish faster than this coalesces them instead of flooding the console.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) progress_min_secs: u64,
}

pub(crate) fn run_spsa_plan(command: SpsaPlanCommand, machine: bool) -> ExitCode {
    let tune = match load_spsa_tune(&command.tune) {
        Ok(tune) => tune,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let settings = match SpsaRunSettings::new(command.iterations, command.games_per_iteration) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let timing = match (
        command.seconds_per_game_low,
        command.seconds_per_game_high,
        command.pilot_game_seconds.is_empty(),
    ) {
        (Some(lower), Some(upper), true) => Some(SpsaTimingInput::Range {
            lower_seconds_per_game: lower,
            upper_seconds_per_game: upper,
        }),
        (None, None, false) => Some(SpsaTimingInput::PilotGames(command.pilot_game_seconds)),
        (None, None, true) => None,
        _ => {
            eprintln!(
                "configuration error: supply both --seconds-per-game-low and --seconds-per-game-high, or repeat --pilot-game-seconds"
            );
            return ExitCode::from(2);
        }
    };
    let report = match plan_spsa(
        &tune,
        settings,
        command.r_end,
        command.concurrency,
        timing,
        &command.comparison_horizons,
    ) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if machine {
        print_json(&MachineOutput::SpsaPlan { report });
    } else {
        print_spsa_plan(&report);
    }
    ExitCode::SUCCESS
}

pub(crate) fn print_spsa_plan(report: &SpsaPlanReport) {
    println!("SPSA schedule and workload plan");
    println!("iterations: {}", report.settings.iterations);
    println!(
        "games: {} ({} pairs; {} games/iteration)",
        report.total_games, report.total_pairs, report.settings.games_per_iteration
    );
    println!(
        "durability: {} checkpoint publications; {} retained generations; {} schedule artifact",
        report.checkpoint_publications,
        report.checkpoint_generations_retained,
        report.schedule_artifacts
    );
    if let Some(timing) = &report.wall_time {
        println!(
            "estimated wall time: {:.1}..{:.1} seconds ({} concurrent, {} game waves)",
            timing.lower_seconds, timing.upper_seconds, timing.concurrency, timing.total_game_waves
        );
    } else {
        println!("estimated wall time: unavailable (supply a seconds/game range or pilot samples)");
    }
    for knob in &report.knobs {
        let first = knob
            .trajectory
            .first()
            .expect("a validated schedule has an iteration");
        let final_point = knob
            .trajectory
            .last()
            .expect("a validated schedule has an iteration");
        let hazard = knob
            .first_rounding_resolution_hazard
            .map_or_else(|| "none".into(), |iteration| iteration.to_string());
        println!(
            "{}: c {:.6}->{:.6}, a {:.6}->{:.6}, r {:.6}->{:.6}, first sub-half-unit hazard: {}",
            knob.name,
            first.c,
            final_point.c,
            first.a,
            final_point.a,
            first.r,
            final_point.r,
            hazard
        );
    }
    for comparison in &report.horizon_comparisons {
        println!(
            "comparison horizon {}: {} games, {} pairs, {} checkpoints",
            comparison.iterations,
            comparison.games,
            comparison.pairs,
            comparison.checkpoint_publications
        );
    }
    println!("interpretation: {}", report.interpretation);
}

#[derive(Debug, Deserialize)]
pub(crate) struct StoredSpsaWorkflow {
    pub(crate) settings: SpsaRunSettings,
    /// Absent means the default final-centre estimator.
    #[serde(default)]
    pub(crate) final_window_percent: Option<u32>,
    pub(crate) bound_tune: SpsaBoundTune,
    pub(crate) engine_sha256: String,
    pub(crate) schedule: SpsaScheduleArtifact,
}

#[derive(Debug, Serialize)]
pub(crate) struct SpsaStatusOutput {
    pub(crate) run_status: RunStatus,
    pub(crate) diagnostics: SpsaStatusReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) candidate_result: Option<SpsaTuneResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) candidate_unavailable_reason: Option<String>,
    pub(crate) snapshot_authority: String,
}

pub(crate) fn run_spsa_status(run_directory: &Path, machine: bool) -> ExitCode {
    let record = match RunRecord::read(run_directory) {
        Ok(record) => record,
        Err(error) => {
            eprintln!("SPSA status failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if record.command != "spsa"
        || record.workflow.get("kind").and_then(Value::as_str) != Some("spsa")
    {
        eprintln!(
            "SPSA status failed: {} is a {:?} run, not an SPSA tune",
            run_directory.display(),
            record.command
        );
        return ExitCode::from(2);
    }
    let workflow = match serde_json::from_value::<StoredSpsaWorkflow>(record.workflow.clone()) {
        Ok(workflow) => workflow,
        Err(error) => {
            eprintln!("SPSA status failed: invalid stored workflow: {error}");
            return ExitCode::FAILURE;
        }
    };
    let verified_schedule = match read_and_verify_spsa_schedule(run_directory, &workflow.schedule) {
        Ok(schedule) => schedule,
        Err(error) => {
            eprintln!("SPSA status failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    // The iterations are rebuilt from the journal, as a resume would, without
    // changing a byte of a run that may still be going.
    let records = match read_anchor(run_directory).and_then(|anchor| {
        crate::journal::load_journal(
            run_directory,
            anchor.as_ref(),
            crate::journal::LoadMode::ReadOnly,
        )
        .map_err(|error| error.to_string())
    }) {
        Ok(loaded) => loaded.records,
        Err(error) => {
            eprintln!("SPSA status failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let has_invalid = records.iter().any(|record| record.sample == INVALID_SAMPLE);
    let checkpoint = match spsa_driver::replay_iterations(
        verified_schedule,
        workflow.settings,
        workflow.bound_tune.initial_centers(),
        &iterations_from_journal(&records),
    ) {
        Ok(completed_iterations) => spsa_driver::SpsaCheckpoint {
            completed_iterations,
            invalid_iteration: None,
        },
        Err(error) => {
            eprintln!("SPSA status failed: the journal does not replay: {error}");
            return ExitCode::FAILURE;
        }
    };
    let centers = checkpoint
        .completed_iterations
        .iter()
        .map(|iteration| SpsaCenterSample {
            iteration: iteration.iteration,
            centers: iteration.centers_after.clone(),
        })
        .collect::<Vec<_>>();
    let resumed = record
        .anomalies
        .iter()
        .any(|anomaly| anomaly.code == "run-resumed");
    let elapsed = (!resumed).then_some(
        record
            .updated_unix_ms
            .saturating_sub(record.started_unix_ms) as f64
            / 1_000.0,
    );
    let invalid = has_invalid || record.status == RunStatus::Invalid;
    let diagnostics = match diagnose_spsa(
        &workflow.bound_tune,
        &workflow.schedule,
        workflow.settings,
        &centers,
        invalid,
        elapsed,
    ) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("SPSA status failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (candidate_result, candidate_unavailable_reason) = if invalid {
        (
            None,
            Some("the tune is invalid; no gate candidate is emitted".into()),
        )
    } else {
        match workflow.bound_tune.result_from_centers(
            workflow.engine_sha256,
            &workflow.schedule,
            workflow.settings,
            spsa_estimator_policy(workflow.final_window_percent),
            &centers,
        ) {
            Ok(result) => (Some(result), None),
            Err(error) => (None, Some(error.to_string())),
        }
    };
    let output = SpsaStatusOutput {
        run_status: record.status,
        diagnostics,
        candidate_result,
        candidate_unavailable_reason,
        snapshot_authority: "checksum-verified current checkpoint with previous-generation fallback; read-only and never recovered in place".into(),
    };
    if machine {
        print_json(&MachineOutput::SpsaStatus {
            run_directory,
            report: output,
        });
    } else {
        print_spsa_status(&output, run_directory);
        // The trajectory the console deliberately does not print every block.
        if let Some(iteration) = checkpoint.completed_iterations.last() {
            for (label, value) in spsa_iteration_detail(
                iteration,
                SpsaCentreTracker::new(&workflow.schedule.knobs, &iteration.centers_before)
                    .moves_since_start(&iteration.centers_after),
                "in this iteration",
            ) {
                println!("{label}: {value}");
            }
        }
    }
    ExitCode::SUCCESS
}

/// The lifecycle view. The caller adds the last iteration's trajectory after
/// it, from the checkpoint it already verified.
pub(crate) fn print_spsa_status(report: &SpsaStatusOutput, run_directory: &Path) {
    println!(
        "SPSA {:?}: {}/{} iterations ({:.2}%)",
        report.run_status,
        report.diagnostics.completed_iterations,
        report.diagnostics.settings.iterations,
        report.diagnostics.percent_complete
    );
    match report.diagnostics.eta.remaining_seconds {
        Some(seconds) => println!("ETA: {:.1} seconds", seconds),
        None => println!(
            "ETA unavailable: {}",
            report
                .diagnostics
                .eta
                .unavailable_reason
                .as_deref()
                .unwrap_or("no reason recorded")
        ),
    }
    for knob in &report.diagnostics.knobs {
        println!(
            "{}: {:.6} ({:.2}% of range); bound-contact {:?}, seed-movement {:?}, recent-stability {:?}, rounding-resolution {:?}",
            knob.name,
            knob.current,
            knob.current_normalized_to_range * 100.0,
            knob.frequent_bound_contact.state,
            knob.little_net_movement.state,
            knob.recent_stability.state,
            knob.dead_perturbation.state
        );
    }
    if let Some(reason) = &report.candidate_unavailable_reason {
        println!("gate candidate unavailable: {reason}");
    } else {
        println!("gate candidate: available in JSON output");
    }
    println!("interpretation: {}", report.diagnostics.interpretation);
    println!("snapshot: {}", run_directory.display());
}

pub(crate) async fn run_spsa_command(
    command: SpsaCommand,
    machine: bool,
    dry_run: bool,
    cancellation: Cancellation,
) -> ExitCode {
    let conditions = &command.conditions;
    let stored_schedule_inputs = match conditions
        .run_directory
        .as_deref()
        .filter(|path| path.exists() && !conditions.restart)
        .map(read_stored_spsa_inputs)
        .transpose()
    {
        Ok(inputs) => inputs,
        Err(error) => {
            eprintln!("resume failed: {error}");
            return ExitCode::from(2);
        }
    };
    let (iterations, games_per_iteration, r_end, final_window_percent) = if let Some((
        stored,
        stored_r_end,
        stored_window,
    )) =
        stored_schedule_inputs
    {
        eprintln!(
            "resuming the stored SPSA horizon: {} iterations, {} games per iteration, r_end {}, estimator {}",
            stored.iterations,
            stored.games_per_iteration,
            stored_r_end,
            describe_spsa_estimator(stored_window)
        );
        (
            stored.iterations,
            stored.games_per_iteration,
            stored_r_end,
            stored_window,
        )
    } else {
        let Some(r_end) = command.r_end else {
            eprintln!("configuration error: --r-end is required for a new SPSA run");
            return ExitCode::from(2);
        };
        (
            command.iterations.unwrap_or(DEFAULT_SPSA_ITERATIONS),
            command
                .games_per_iteration
                .unwrap_or(DEFAULT_SPSA_GAMES_PER_ITERATION),
            r_end,
            command.final_window_percent,
        )
    };
    let estimator = spsa_estimator_policy(final_window_percent);
    let settings = match SpsaRunSettings::new(iterations, games_per_iteration) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if conditions.book.is_none()
        && (conditions.book_start != 0
            || conditions.book_plies.is_some()
            || conditions.book_order != BookOrderArg::Sequential)
    {
        eprintln!("configuration error: book order/start/plies require --book");
        return ExitCode::from(2);
    }
    let Some(tune_path) = command.tune.as_ref() else {
        eprintln!("configuration error: --tune is required for a live SPSA run");
        return ExitCode::from(2);
    };
    let tune = match load_spsa_tune(tune_path) {
        Ok(tune) => tune,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = tune.audit_configuration() {
        eprintln!("configuration error: {error}");
        return ExitCode::from(2);
    }
    let resumed_seed = conditions
        .run_directory
        .as_deref()
        .filter(|path| path.exists() && !conditions.restart)
        .and_then(read_stored_seed);
    let (master_seed, master_seed_generated) = match resumed_seed {
        Some(seed) if conditions.seed.is_none() => seed,
        _ => match resolve_master_seed(conditions.seed) {
            Ok(seed) => seed,
            Err(error) => {
                eprintln!("configuration error: {error}");
                return ExitCode::from(2);
            }
        },
    };
    let engine = match command.engine.resolve() {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if !matches!(
        engine.allocated_cpus,
        colosseum_application::CpuAllocation::Unrestricted
    ) {
        eprintln!(
            "configuration error: SPSA --cores cannot describe two disjoint arms; use --placement with --cores-per-engine"
        );
        return ExitCode::from(2);
    }
    let engine_time_control = match resolve_time_control(
        "SPSA engine",
        conditions.movetime_ms,
        conditions.base_ms,
        conditions.increment_ms,
        conditions.nodes,
        conditions.depth,
        conditions.margin_ms,
    ) {
        Ok(control) => control,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = validate_ponder(conditions.ponder, &[engine_time_control]) {
        eprintln!("configuration error: {error}");
        return ExitCode::from(2);
    }
    let mut engine = engine;
    if let Err(error) = configure_ponder(&mut engine, conditions.ponder) {
        eprintln!("configuration error: {error}");
        return ExitCode::from(2);
    }
    let adjudication = AdjudicationConfig {
        max_moves: conditions.max_moves,
        draw: conditions.draw_adjudication.then_some(DrawAdjudication {
            min_ply: conditions.draw_move.saturating_mul(2),
            move_count: conditions.draw_moves,
            score_cp: conditions.draw_score_cp,
        }),
        resign: conditions
            .resign_adjudication
            .then_some(ResignAdjudication {
                move_count: conditions.resign_moves,
                score_cp: conditions.resign_score_cp,
                two_sided: !conditions.one_sided_resign_adjudication,
            }),
    };
    let placement = match resolve_placement(&conditions.placement, conditions.headroom_cores) {
        Ok(placement) => placement,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let mut planning_engine = engine.clone();
    for parameter in &tune.parameters {
        if colosseum_core::is_hash_option(&parameter.name) {
            planning_engine.options.insert(
                parameter.name.clone(),
                colosseum_application::UciOptionValue::Spin(parameter.max),
            );
        }
    }
    let execution = match match_runner::plan_execution(
        &planning_engine,
        &planning_engine,
        conditions.concurrency as usize,
        resolve_slot_allocation(conditions.cores_per_game, conditions.cores_per_engine),
        placement,
        conditions.memory_budget_mb,
    ) {
        Ok(execution) => execution,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let total_games = match settings
        .iterations
        .checked_mul(settings.games_per_iteration)
    {
        Some(total) => total,
        None => {
            eprintln!("configuration error: SPSA game horizon is too large");
            return ExitCode::from(2);
        }
    };
    let book = conditions.book.clone().map(|path| {
        let mut book = OpeningBook::new(path);
        book.order = match conditions.book_order {
            BookOrderArg::Sequential => OpeningOrder::Sequential,
            BookOrderArg::Random => OpeningOrder::Random,
        };
        book.plies = conditions.book_plies.unwrap_or(8);
        book
    });
    // This is the only book parse in one SPSA process session. The resolved
    // in-memory entries are reused by every iteration and game worker.
    let openings = match match_runner::resolve_openings(
        book,
        conditions.book_start,
        total_games.div_ceil(2),
        conditions.book_wrap,
        master_seed,
    ) {
        Ok(openings) => openings,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let end_specs = tune
        .parameters
        .iter()
        .map(|parameter| SpsaEndSpec {
            name: parameter.name.clone(),
            min: parameter.min,
            max: parameter.max,
            c_end: parameter.c_end,
        })
        .collect::<Vec<_>>();
    let expected_schedule =
        match SpsaScheduleArtifact::derive(settings.iterations, r_end, master_seed, &end_specs) {
            Ok(schedule) => schedule,
            Err(error) => {
                eprintln!("configuration error: {error}");
                return ExitCode::from(2);
            }
        };
    let current_directory = match std::env::current_dir() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let mut path_pointers = vec!["/engine/executable".into(), "/tune/path".into()];
    if engine.working_directory.is_some() {
        path_pointers.push("/engine/working_directory".into());
    }
    if conditions.book.is_some() {
        path_pointers.push("/openings/path".into());
    }
    // A forfeit is scored as the loss it is and moves the gradient like any
    // other result; the tune is void only when faults outrun 0.5% of the
    // games played, and never fewer than three.
    let fault_policy = match_runner::FaultPolicy::sequential(
        conditions.max_engine_faults,
        conditions.max_time_losses,
    );
    let resolved = match resolve_config(
        built_in_defaults(),
        None,
        json!({
            "command": "spsa",
            "engine": &engine,
            "engine_sha256": "computed-and-checked-before-live-launch",
            "tune": {
                "path": tune_path,
                "parameters": &tune.parameters,
                "live_schema": "verified-before-game-launch"
            },
            "settings": settings,
            "r_end": r_end,
            "final_window_percent": final_window_percent,
            "schedule": &expected_schedule,
            "engine_time_control": engine_time_control,
            "adjudication": adjudication,
            "ponder": conditions.ponder,
            "execution": execution,
            "master_seed": master_seed,
            "master_seed_generated": master_seed_generated,
            "openings": openings.report(),
            "fault_policy": fault_policy,
        }),
        &[],
        &current_directory,
        &path_pointers,
    ) {
        Ok(resolved) => resolved,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if dry_run {
        print_output(
            &MachineOutput::DryRun {
                command: "spsa",
                config_sha256: resolved.sha256(),
                resolved_configuration: resolved.value(),
                invocations: vec![&engine],
            },
            machine,
        );
        return ExitCode::SUCCESS;
    }

    let opened = match &conditions.run_directory {
        Some(path) => RunDirectory::open_explicit(path, &resolved, conditions.restart),
        None => RunDirectory::create_unique(&current_directory, "spsa", &resolved),
    };
    let opened = match opened {
        Ok(opened) => opened,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Some(archived) = &opened.archived {
        eprintln!("archived previous run at {}", archived.display());
    }
    let resumed = opened.resumed;
    let directory = Arc::new(opened.directory);
    let engine_sha256 = match executable_sha256(&engine.executable) {
        Ok(hash) => hash,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let recorded_engine = match serde_json::from_value::<EngineLaunchSpec>(
        resolved
            .value()
            .get("engine")
            .cloned()
            .unwrap_or(Value::Null),
    ) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("configuration error: could not retain resolved SPSA engine: {error}");
            return ExitCode::from(2);
        }
    };
    if resumed {
        let stored_record = match RunRecord::read(&directory.paths().root) {
            Ok(record) => record,
            Err(error) => {
                eprintln!("resume failed: {error}");
                return ExitCode::from(3);
            }
        };
        let stored_hash = stored_record
            .workflow
            .get("engine_sha256")
            .and_then(Value::as_str);
        if stored_hash != Some(engine_sha256.as_str()) {
            eprintln!(
                "resume failed: SPSA engine content changed (stored {}, current {})",
                stored_hash.unwrap_or("missing"),
                engine_sha256
            );
            return ExitCode::from(2);
        }
    }
    let mut recorder = match if resumed {
        RunRecorder::resume(&directory)
    } else {
        RunRecorder::begin(&directory, "spsa")
    } {
        Ok(recorder) => recorder,
        Err(error) => {
            eprintln!("run record failed: {error}");
            return ExitCode::from(3);
        }
    };

    let mut inspection_launch = engine.clone();
    inspection_launch.allocated_cpus = colosseum_application::CpuAllocation::Unrestricted;
    let inspection = match InspectEngine::execute(
        &UciSessionFactory,
        &RuntimeParticipant {
            id: ParticipantId::from_u128(51),
            launch: inspection_launch,
        },
    )
    .await
    {
        Ok(inspection) => inspection,
        Err(error) => {
            eprintln!("SPSA engine inspection failed: {error}");
            return ExitCode::from(3);
        }
    };
    let bound_tune = match tune.bind_live_schema(&inspection) {
        Ok(bound) => bound,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let tune_audit = match bound_tune.audit() {
        Ok(audit) => audit,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    for warning in &tune_audit.warnings {
        print_spsa_tune_warning(warning);
    }
    let verified_schedule = match persist_and_verify_spsa_schedule(&directory, &expected_schedule) {
        Ok(schedule) => schedule,
        Err(error) => {
            eprintln!("SPSA schedule preflight failed: {error}");
            return ExitCode::from(3);
        }
    };
    let journal = match open_journal(&directory, resumed).await {
        Ok(journal) => journal,
        Err(error) => {
            eprintln!("resume failed: {error}");
            return ExitCode::from(3);
        }
    };
    if journal
        .records
        .iter()
        .any(|record| record.sample == INVALID_SAMPLE)
    {
        eprintln!("resume failed: an invalidated SPSA tune cannot be extended");
        return ExitCode::from(2);
    }
    // Iteration boundaries and every centre are recomputed from the games,
    // never read back from a stored copy of the state.
    let checkpoint = match spsa_driver::replay_iterations(
        verified_schedule.clone(),
        settings,
        bound_tune.initial_centers(),
        &iterations_from_journal(&journal.records),
    ) {
        Ok(completed_iterations) => spsa_driver::SpsaCheckpoint {
            completed_iterations,
            invalid_iteration: None,
        },
        Err(error) => {
            eprintln!("resume failed: the journal does not replay: {error}");
            return ExitCode::from(3);
        }
    };
    let writer = match RunWriter::start(Arc::clone(&directory), journal.resume).await {
        Ok(writer) => writer,
        Err(error) => {
            eprintln!("SPSA output failed: {error}");
            return ExitCode::from(3);
        }
    };
    recorder.write_through(writer.clone());
    if let Err(error) = recorder.set_workflow(json!({
        "kind": "spsa",
        "stop_after_iteration": command.stop_after_iteration,
        "pgn_annotation_writer": colosseum_engine::pgn::PGN_ANNOTATION_WRITER,
        "settings": settings,
        "r_end": r_end,
        "final_window_percent": final_window_percent,
        "bound_tune": &bound_tune,
        "tune_audit": &tune_audit,
        "engine_sha256": &engine_sha256,
        "schedule": verified_schedule.artifact(),
        "engine_time_control": engine_time_control,
        "adjudication": adjudication,
        "ponder": conditions.ponder,
        "execution": execution,
        "master_seed": master_seed,
        "master_seed_generated": master_seed_generated,
        "openings": openings.report(),
        "fault_policy": fault_policy
    })) {
        eprintln!("run record failed: {error}");
        return ExitCode::from(3);
    }
    let observer = match DurableSpsaOutput::new(
        writer.clone(),
        &checkpoint.completed_iterations,
        recorder,
        settings,
    ) {
        Ok(output) => Arc::new(output),
        Err(error) => {
            eprintln!("SPSA output failed: {error}");
            return ExitCode::from(3);
        }
    };
    colosseum_engine::incidents::set_dir(directory.paths().root.join("failed-games"));
    if !machine {
        eprintln!("SPSA run directory: {}", directory.paths().root.display());
        if resumed {
            eprintln!(
                "resuming {} complete durable iteration(s); the stored schedule remains authoritative",
                checkpoint.completed_iterations.len()
            );
        }
    }
    let progress = spsa_driver::SpsaProgress::default();
    let knobs = verified_schedule.artifact().knobs.clone();
    let initial_centers = bound_tune.initial_centers();
    let resumed_iterations = observer.committed_iterations();
    let driver_request = spsa_driver::SpsaDriverRequest {
        schedule: verified_schedule,
        settings,
        initial_centers: bound_tune.initial_centers(),
        base_engine: engine.clone(),
        game_settings: match_runner::PairGameSettings {
            engine_a: engine.clone(),
            engine_b: engine,
            engine_a_time_control: engine_time_control,
            engine_b_time_control: engine_time_control,
            adjudication,
            ponder: conditions.ponder,
            openings: openings.clone(),
        },
        execution: execution.clone(),
        fault_policy,
        checkpoint,
        stop_after_iteration: command.stop_after_iteration,
        progress: progress.clone(),
        cancellation: cancellation.clone(),
        observer: Some(observer.clone()),
    };
    let driver_future = spsa_driver::run_spsa(driver_request);
    tokio::pin!(driver_future);
    let mut schedule = ProgressSchedule::new(
        conditions.progress_every,
        conditions.progress_min_secs,
        resumed_iterations,
    );
    let mut centres = SpsaCentreTracker::new(&knobs, &initial_centers);
    let mut poll =
        tokio::time::interval_at(tokio::time::Instant::now() + PROGRESS_POLL, PROGRESS_POLL);
    let outcome = loop {
        tokio::select! {
            result = &mut driver_future => break result,
            _ = poll.tick() => {
                if schedule.due(observer.committed_iterations()) {
                    let block = spsa_progress_block(
                        &observer,
                        &schedule,
                        settings,
                        &centres,
                        fault_policy,
                    );
                    observer.publish_progress(&block);
                    centres.remember(&observer.current_centers());
                }
            }
        }
    };
    let final_block = spsa_progress_block(&observer, &schedule, settings, &centres, fault_policy);
    if schedule.needs_final(final_block.done) {
        schedule.mark(final_block.done);
        observer.publish_progress(&final_block);
    }
    let driver = match outcome {
        Ok(report) => report,
        Err(error) => {
            eprintln!("SPSA failed: {error}");
            drop(observer);
            let _ = settle(&writer).await;
            return ExitCode::from(3);
        }
    };
    let status = driver.status;
    let tuned_result = if matches!(
        status,
        spsa_driver::SpsaStatus::Completed | spsa_driver::SpsaStatus::Cancelled
    ) {
        let history = driver
            .completed_iterations
            .iter()
            .map(|iteration| SpsaCenterSample {
                iteration: iteration.iteration,
                centers: iteration.centers_after.clone(),
            })
            .collect::<Vec<_>>();
        match bound_tune.result_from_centers(
            engine_sha256.clone(),
            &expected_schedule,
            settings,
            estimator,
            &history,
        ) {
            Ok(result) => Some(result),
            Err(error) => {
                eprintln!("SPSA result failed: {error}");
                return ExitCode::from(3);
            }
        }
    } else {
        None
    };
    let report = SpsaReport {
        engine: recorded_engine,
        schedule: expected_schedule,
        bound_tune,
        tune_audit,
        tuned_result,
        engine_sha256,
        engine_time_control,
        adjudication,
        ponder: conditions.ponder,
        execution,
        master_seed,
        master_seed_generated,
        openings: openings.report().clone(),
        fault_policy,
        driver,
    };
    if let Err(error) = observer.finish(&report) {
        eprintln!("SPSA output failed: {error}");
        return ExitCode::from(3);
    }
    if let Err(error) = settle(&writer).await {
        eprintln!("SPSA output failed: {error}");
        return ExitCode::from(3);
    }
    if machine {
        print_json(&MachineOutput::Spsa {
            run_directory: directory.paths().root.clone(),
            report: Box::new(report),
        });
    } else {
        print_spsa(&report, &directory.paths().root);
    }
    match status {
        spsa_driver::SpsaStatus::Completed => ExitCode::SUCCESS,
        spsa_driver::SpsaStatus::Cancelled => ExitCode::from(CANCELLED_EXIT_CODE),
        spsa_driver::SpsaStatus::Invalid => ExitCode::from(5),
    }
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SpsaReport {
    pub(crate) engine: EngineLaunchSpec,
    pub(crate) schedule: SpsaScheduleArtifact,
    pub(crate) bound_tune: SpsaBoundTune,
    pub(crate) tune_audit: SpsaTuneAudit,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tuned_result: Option<SpsaTuneResult>,
    pub(crate) engine_sha256: String,
    pub(crate) engine_time_control: match_runner::ConfiguredTimeControl,
    pub(crate) adjudication: AdjudicationConfig,
    pub(crate) ponder: bool,
    pub(crate) execution: match_runner::MatchExecutionPlan,
    pub(crate) master_seed: u64,
    pub(crate) master_seed_generated: bool,
    pub(crate) openings: match_runner::OpeningPolicyReport,
    pub(crate) fault_policy: match_runner::FaultPolicy,
    pub(crate) driver: spsa_driver::SpsaDriverReport,
}

pub(crate) fn read_stored_spsa_inputs(
    root: &Path,
) -> Result<(SpsaRunSettings, f64, Option<u32>), String> {
    let path = root.join("resolved-config.json");
    let bytes = fs::read(&path).map_err(|error| {
        format!(
            "cannot read stored configuration {}: {error}",
            path.display()
        )
    })?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "cannot parse stored configuration {}: {error}",
            path.display()
        )
    })?;
    if value.get("command").and_then(Value::as_str) != Some("spsa") {
        return Err(format!(
            "stored configuration {} is not an SPSA run",
            path.display()
        ));
    }
    let iterations = value
        .pointer("/settings/iterations")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "stored SPSA iteration horizon is missing or invalid".to_owned())?;
    let games_per_iteration = value
        .pointer("/settings/games_per_iteration")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "stored SPSA mini-match size is missing or invalid".to_owned())?;
    let r_end = value
        .get("r_end")
        .and_then(Value::as_f64)
        .ok_or_else(|| "stored SPSA r_end is missing or invalid".to_owned())?;
    // An absent or null percent is the default final-centre estimator.
    let final_window_percent = match value.get("final_window_percent") {
        None | Some(Value::Null) => None,
        Some(stored) => Some(
            stored
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| (1..=100).contains(value))
                .ok_or_else(|| "stored SPSA final-window percent is invalid".to_owned())?,
        ),
    };
    let settings =
        SpsaRunSettings::new(iterations, games_per_iteration).map_err(|error| error.to_string())?;
    Ok((settings, r_end, final_window_percent))
}

/// Which centres a tune has moved, measured in the fraction of each knob's own
/// range that it travelled.
///
/// A raw centre change is not comparable between knobs: ten units of a
/// thousand-wide knob and ten units of a twenty-wide one are different facts.
/// Range units make the three largest moves the three that matter.
pub(crate) struct SpsaCentreTracker {
    /// Knob name, the width of its range and its rails, in schedule order.
    knobs: Vec<SpsaKnobRange>,
    /// The centres the tune started from.
    initial: Vec<f64>,
    /// The centres at the previous published block.
    previous: Vec<f64>,
}

struct SpsaKnobRange {
    name: String,
    min: i64,
    max: i64,
    width: f64,
}

impl SpsaCentreTracker {
    pub(crate) fn new(knobs: &[SpsaDerivedKnob], initial_centers: &[f64]) -> Self {
        Self {
            knobs: knobs
                .iter()
                .map(|knob| SpsaKnobRange {
                    name: knob.name.clone(),
                    min: knob.min,
                    max: knob.max,
                    width: (knob.max - knob.min) as f64,
                })
                .collect(),
            initial: initial_centers.to_vec(),
            previous: initial_centers.to_vec(),
        }
    }

    /// The three largest moves since the tune began, largest first.
    ///
    /// This is the one trajectory signal a reader can act on while the run is
    /// going: a knob that has travelled a long way is being pushed, and one
    /// that has not is either right or dead.
    pub(crate) fn moves_since_start(&self, centers: &[f64]) -> Option<String> {
        self.largest(&self.initial, centers)
    }

    /// The three largest moves since the previous published block.
    pub(crate) fn moves_since_last_block(&self, centers: &[f64]) -> Option<String> {
        self.largest(&self.previous, centers)
    }

    fn largest(&self, baseline: &[f64], centers: &[f64]) -> Option<String> {
        if centers.len() != self.knobs.len() || baseline.len() != self.knobs.len() {
            return None;
        }
        let mut moves = self
            .knobs
            .iter()
            .zip(centers)
            .zip(baseline)
            .filter(|((knob, _), _)| knob.width > 0.0)
            .map(|((knob, center), baseline)| (knob.name.clone(), (center - baseline) / knob.width))
            .collect::<Vec<_>>();
        moves.sort_by(|left, right| {
            right
                .1
                .abs()
                .partial_cmp(&left.1.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        moves.truncate(3);
        (!moves.is_empty()).then(|| {
            moves
                .iter()
                .map(|(name, moved)| format!("{name} {moved:+.4}"))
                .collect::<Vec<_>>()
                .join(", ")
        })
    }

    /// Knobs whose centre now rounds onto one of its own rails, which is where
    /// the gradient becomes one-sided.
    pub(crate) fn at_a_rail(&self, centers: &[f64]) -> String {
        if centers.len() != self.knobs.len() {
            return "unknown".to_owned();
        }
        let railed = self
            .knobs
            .iter()
            .zip(centers)
            .filter_map(|(knob, center)| {
                let rounded = round_half_away_from_zero(*center).ok()?;
                if rounded <= knob.min {
                    Some(format!("{} at {}", knob.name, knob.min))
                } else if rounded >= knob.max {
                    Some(format!("{} at {}", knob.name, knob.max))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        if railed.is_empty() {
            "none".to_owned()
        } else {
            railed.join(", ")
        }
    }

    /// Take the centres a published block reported as the new baseline.
    pub(crate) fn remember(&mut self, centers: &[f64]) {
        if centers.len() == self.knobs.len() {
            self.previous = centers.to_vec();
        }
    }
}

/// Every engine fault a tune has committed, across every iteration it kept.
pub(crate) fn spsa_faults(checkpoint: &spsa_driver::SpsaCheckpoint) -> MatchFaultCounts {
    let mut faults = MatchFaultCounts::default();
    let pairs = checkpoint
        .completed_iterations
        .iter()
        .flat_map(|iteration| &iteration.pairs)
        .chain(
            checkpoint
                .invalid_iteration
                .iter()
                .flat_map(|iteration| &iteration.pairs),
        );
    for pair in pairs {
        for game in [&pair.first, &pair.second] {
            record_fault(&mut faults, game.white, game.fault.as_ref());
        }
    }
    faults
}

/// What one committed iteration says about the search: the mini-match it
/// played, the gain and perturbation scale it used, and what moved.
///
/// This is the trajectory. It is recorded rather than printed, because a
/// console that shows it every block is a console nobody reads; `run.log` and
/// `spsa status` are where it is wanted.
pub(crate) fn spsa_iteration_detail(
    iteration: &spsa_driver::SpsaCommittedIteration,
    moves: Option<String>,
    moves_span: &str,
) -> Vec<(String, String)> {
    let score = iteration.score;
    vec![
        (
            "last mini-match".to_owned(),
            format!(
                "iteration {}: {:+} (plus {} / draws {} / minus {})",
                iteration.iteration,
                score.difference,
                score.plus_wins,
                score.draws,
                score.plus_losses
            ),
        ),
        (
            "schedule".to_owned(),
            coefficient_summary(&iteration.prepared),
        ),
        (
            "moved most".to_owned(),
            match moves {
                Some(moves) => format!("{moves} (range units {moves_span})"),
                None => "none recorded yet".to_owned(),
            },
        ),
    ]
}

/// What a tune tells the operator while it runs: how far through the horizon
/// it is, how long is left, whether anything is faulting, which centres have
/// hit a rail, and which knobs the search has actually moved.
pub(crate) fn spsa_progress_block(
    observer: &DurableSpsaOutput,
    schedule: &ProgressSchedule,
    settings: SpsaRunSettings,
    centres: &SpsaCentreTracker,
    fault_policy: match_runner::FaultPolicy,
) -> ProgressBlock {
    let (done, last, faults) = observer.snapshot();
    let total = u64::from(settings.iterations);
    let mut block = ProgressBlock::new(
        "spsa",
        ProgressUnit::Iterations,
        done,
        Some(total),
        schedule.elapsed(),
    );
    block
        .field(
            "time remaining",
            match progress::time_for_units(
                schedule.units_since_start(done),
                schedule.elapsed(),
                total.saturating_sub(done),
            ) {
                Some(left) => progress::format_duration(left.as_secs_f64()),
                None => "unknown".to_owned(),
            },
        )
        .field(
            "faults",
            format!(
                "time {}/{}, other {}/{}; {}",
                faults.time_losses_a,
                faults.time_losses_b,
                faults.engine_a.saturating_sub(faults.time_losses_a),
                faults.engine_b.saturating_sub(faults.time_losses_b),
                fault_allowance_text(
                    fault_policy,
                    faults,
                    observer.iterations_played() * u64::from(settings.games_per_iteration)
                )
            ),
        );
    match &last {
        Some(iteration) => {
            block
                .field("at a rail", centres.at_a_rail(&iteration.centers_after))
                .field(
                    "moved most since start",
                    centres
                        .moves_since_start(&iteration.centers_after)
                        .unwrap_or_else(|| "nothing yet".to_owned()),
                );
            for (label, value) in spsa_iteration_detail(
                iteration,
                centres.moves_since_last_block(&iteration.centers_after),
                "since the previous block",
            ) {
                block.detail(label, value);
            }
        }
        None => {
            block.field("at a rail", "no iteration has committed yet");
        }
    }
    block
}

/// The gain and perturbation scale an iteration used, collapsed when every
/// knob agrees and shown as a range when they do not.
fn coefficient_summary(prepared: &SpsaIteration) -> String {
    let summarize = |values: Vec<f64>| -> String {
        let low = values.iter().copied().fold(f64::INFINITY, f64::min);
        let high = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if !low.is_finite() || !high.is_finite() {
            "unavailable".to_owned()
        } else if (high - low).abs() < 1e-12 {
            format!("{low:.4}")
        } else {
            format!("{low:.4}\u{2013}{high:.4}")
        }
    };
    format!(
        "gain a {}, perturbation scale c {}",
        summarize(prepared.coefficients.iter().map(|value| value.a).collect()),
        summarize(prepared.coefficients.iter().map(|value| value.c).collect())
    )
}

/// What a tune writes while it runs: each finished iteration's games as
/// journal lines and appended PGN games, and a checkpoint of the iteration it
/// reached and the centres it stands on. It keeps only the last iteration,
/// for the progress block, and the fault count.
pub(crate) struct DurableSpsaOutput {
    pub(crate) writer: RunWriter,
    pub(crate) recorder: Mutex<Option<RunRecorder>>,
    pub(crate) settings: SpsaRunSettings,
    state: Mutex<SpsaAggregate>,
}

struct SpsaAggregate {
    completed: u64,
    last: Option<spsa_driver::SpsaCommittedIteration>,
    faults: MatchFaultCounts,
    invalid: Option<u32>,
    cadence: CheckpointCadence,
}

impl SpsaAggregate {
    fn checkpoint(&self) -> Value {
        json!({
            "command": "spsa",
            "completed_iterations": self.completed,
            "centers": self.last.as_ref().map(|iteration| iteration.centers_after.clone()),
            "invalid_iteration": self.invalid,
            "faults": self.faults,
        })
    }
}

#[derive(Serialize)]
pub(crate) struct SpsaRunFileFragment {
    pub(crate) engine: SpsaRunFileEngine,
}

#[derive(Serialize)]
pub(crate) struct SpsaRunFileEngine {
    pub(crate) options: BTreeMap<String, i64>,
}

impl DurableSpsaOutput {
    /// Start from the iterations a resume rebuilt from the journal.
    pub(crate) fn new(
        writer: RunWriter,
        completed: &[spsa_driver::SpsaCommittedIteration],
        mut recorder: RunRecorder,
        settings: SpsaRunSettings,
    ) -> Result<Self, String> {
        recorder
            .update_sample(spsa_official_sample(completed.len() as u64, settings))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            writer,
            recorder: Mutex::new(Some(recorder)),
            settings,
            state: Mutex::new(SpsaAggregate {
                completed: completed.len() as u64,
                last: completed.last().cloned(),
                faults: MatchFaultCounts::default(),
                invalid: None,
                cadence: CheckpointCadence::new(),
            }),
        })
    }

    /// Committed iterations this run holds.
    pub(crate) fn committed_iterations(&self) -> u64 {
        self.state.lock().map_or(0, |state| state.completed)
    }

    /// The centre vector the tune currently stands on.
    pub(crate) fn current_centers(&self) -> Vec<f64> {
        self.state
            .lock()
            .ok()
            .and_then(|state| {
                state
                    .last
                    .as_ref()
                    .map(|iteration| iteration.centers_after.clone())
            })
            .unwrap_or_default()
    }

    /// Iterations, the last one, and the faults, for a progress block.
    pub(crate) fn snapshot(
        &self,
    ) -> (
        u64,
        Option<spsa_driver::SpsaCommittedIteration>,
        MatchFaultCounts,
    ) {
        self.state.lock().map_or_else(
            |_| (0, None, MatchFaultCounts::default()),
            |state| (state.completed, state.last.clone(), state.faults),
        )
    }

    /// Iterations whose games are in the fault count: the committed ones and
    /// an invalid one.
    pub(crate) fn iterations_played(&self) -> u64 {
        self.state.lock().map_or(0, |state| {
            state.completed + u64::from(state.invalid.is_some())
        })
    }

    /// Publish a block through the run recorder this observer owns.
    pub(crate) fn publish_progress(&self, block: &ProgressBlock) {
        show_progress(block, &self.writer);
        let Ok(mut recorder) = self.recorder.lock() else {
            eprintln!("run record failed: SPSA run-record lock poisoned");
            return;
        };
        if let Some(recorder) = recorder.as_mut()
            && let Err(error) = recorder.update_progress(block.clone())
        {
            eprintln!("run record failed: {error}");
        }
    }

    fn commit_games(
        &self,
        iteration: u32,
        pairs: &[CompletePair<match_runner::MatchGame>],
        class: &'static str,
    ) -> Result<MatchFaultCounts, String> {
        let mut faults = MatchFaultCounts::default();
        let number = iteration.to_string();
        for pair in pairs {
            for game in [&pair.first, &pair.second] {
                let (record, sample) = paired_game_entry(game, class, Some(iteration));
                record_fault(&mut faults, game.white, game.fault.as_ref());
                log_fault(&self.writer, &record);
                let moves = with_header_tags(
                    game.pgn.trim_end(),
                    &[
                        ("ColosseumSample", sample),
                        ("ColosseumSpsaIteration", &number),
                    ],
                );
                self.writer.game(record, moves)?;
            }
        }
        Ok(faults)
    }

    fn update_sample(&self, completed: u64) -> Result<(), String> {
        let mut recorder = self
            .recorder
            .lock()
            .map_err(|_| "SPSA run-record lock poisoned")?;
        recorder
            .as_mut()
            .ok_or("SPSA run recorder is already finished")?
            .update_sample(spsa_official_sample(completed, self.settings))
            .map_err(|error| error.to_string())
    }

    pub(crate) fn finish(&self, report: &SpsaReport) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "SPSA state lock poisoned")?;
        self.writer.checkpoint(state.checkpoint())?;
        drop(state);
        let root = self.writer.root();
        if let Some(result) = &report.tuned_result {
            let json = serde_json::to_vec_pretty(result).map_err(|error| error.to_string())?;
            self.writer.replace(root.join("tuned-options.json"), json)?;
            let mut setoptions = String::new();
            let mut options = BTreeMap::new();
            for parameter in &result.parameters {
                setoptions.push_str(&format!(
                    "setoption name {} value {}\n",
                    parameter.name, parameter.tuned
                ));
                options.insert(parameter.name.clone(), parameter.tuned);
            }
            self.writer
                .replace(root.join("tuned-options.txt"), setoptions.into_bytes())?;
            let fragment = toml::to_string_pretty(&SpsaRunFileFragment {
                engine: SpsaRunFileEngine { options },
            })
            .map_err(|error| error.to_string())?;
            self.writer
                .replace(root.join("tuned-options.toml"), fragment.into_bytes())?;
        }
        self.writer.replace(
            root.join("result.json"),
            serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?,
        )?;
        log_event(
            &self.writer,
            &json!({
                "event": "spsa-finished",
                "status": report.driver.status,
                "completed_iterations": report.driver.completed_iterations.len(),
                "invalid_iteration": report.driver.invalid_iteration.as_ref().map(|value| value.iteration),
            }),
        );
        let status = match report.driver.status {
            spsa_driver::SpsaStatus::Completed => RunStatus::Completed,
            spsa_driver::SpsaStatus::Cancelled => RunStatus::Cancelled,
            spsa_driver::SpsaStatus::Invalid => RunStatus::Invalid,
        };
        let recorder = self
            .recorder
            .lock()
            .map_err(|_| "SPSA run-record lock poisoned")?
            .take()
            .ok_or("SPSA run recorder is already finished")?;
        recorder.finish(status).map_err(|error| error.to_string())
    }
}

impl spsa_driver::SpsaObserver for DurableSpsaOutput {
    fn iteration_committed(
        &self,
        iteration: &spsa_driver::SpsaCommittedIteration,
    ) -> Result<(), String> {
        let faults = self.commit_games(iteration.iteration, &iteration.pairs, OFFICIAL_SAMPLE)?;
        let mut state = self.state.lock().map_err(|_| "SPSA state lock poisoned")?;
        state.completed += 1;
        state.faults.engine_a += faults.engine_a;
        state.faults.engine_b += faults.engine_b;
        state.faults.time_losses_a += faults.time_losses_a;
        state.faults.time_losses_b += faults.time_losses_b;
        // The last iteration is kept without its moves: the block needs its
        // score and coefficients, not its games.
        let mut last = iteration.clone();
        for pair in &mut last.pairs {
            pair.first.strip_moves();
            pair.second.strip_moves();
        }
        state.last = Some(last);
        let completed = state.completed;
        if state.cadence.record(1) {
            self.writer.checkpoint(state.checkpoint())?;
        }
        drop(state);
        self.update_sample(completed)
    }

    fn iteration_invalid(
        &self,
        iteration: &spsa_driver::SpsaInvalidIteration,
    ) -> Result<(), String> {
        let faults = self.commit_games(iteration.iteration, &iteration.pairs, INVALID_SAMPLE)?;
        let mut state = self.state.lock().map_err(|_| "SPSA state lock poisoned")?;
        state.faults.engine_a += faults.engine_a;
        state.faults.engine_b += faults.engine_b;
        state.faults.time_losses_a += faults.time_losses_a;
        state.faults.time_losses_b += faults.time_losses_b;
        state.invalid = Some(iteration.iteration);
        // An invalidated iteration ends the tune: record it at once.
        self.writer.checkpoint(state.checkpoint())?;
        let completed = state.completed;
        drop(state);
        self.update_sample(completed)
    }
}

pub(crate) fn spsa_official_sample(iterations: u64, settings: SpsaRunSettings) -> OfficialSample {
    let pairs = iterations * u64::from(settings.pairs_per_iteration());
    OfficialSample {
        committed_units: iterations,
        scored_games: pairs * 2,
        completed_pairs: pairs,
        pentanomial: [0; 5],
        unpaired_games: 0,
    }
}

/// An absent percent means the default: the final completed centre vector.
pub(crate) fn spsa_estimator_policy(final_window_percent: Option<u32>) -> SpsaEstimatorPolicy {
    match final_window_percent {
        Some(percent) => SpsaEstimatorPolicy::TailWindowMean { percent },
        None => SpsaEstimatorPolicy::FinalCenter,
    }
}

pub(crate) fn describe_spsa_estimator(final_window_percent: Option<u32>) -> String {
    match final_window_percent {
        Some(percent) => format!("mean of the final {percent}% window"),
        None => "final centre vector".to_owned(),
    }
}

/// One row of the result table, already rendered.
struct SpsaResultRow {
    parameter: String,
    initial: String,
    tuned: String,
    estimate: String,
    delta: String,
    range: String,
}

/// The result as a table in tune-file order.
///
/// The per-parameter prose this replaces said each value twice, once
/// unrounded and once as a `setoption` line, and neither form let a reader
/// compare a knob against where it started or against its own range. The
/// paste-ready forms are the `tuned-options.*` artifacts, which is where a
/// reader who wants to copy values should go.
fn spsa_result_rows(report: &SpsaReport) -> Vec<SpsaResultRow> {
    report
        .bound_tune
        .parameters
        .iter()
        .enumerate()
        .map(|(index, bound)| {
            let parameter = &bound.parameter;
            let tuned = report
                .tuned_result
                .as_ref()
                .and_then(|result| result.parameters.get(index))
                .map(|tuned| tuned.tuned);
            let estimate = report
                .tuned_result
                .as_ref()
                .and_then(|result| result.parameters.get(index))
                .map(|tuned| tuned.estimate)
                .or_else(|| report.driver.final_centers.get(index).copied());
            SpsaResultRow {
                parameter: parameter.name.clone(),
                initial: parameter.initial.to_string(),
                tuned: tuned.map_or_else(|| "-".to_owned(), |value| value.to_string()),
                estimate: estimate.map_or_else(|| "-".to_owned(), |value| format!("{value:.4}")),
                delta: tuned.map_or_else(
                    || "-".to_owned(),
                    |value| format!("{:+}", value - parameter.initial),
                ),
                range: format!("{}..{}", parameter.min, parameter.max),
            }
        })
        .collect()
}

/// Knobs whose tuned value sits on one of its own rails, where the gradient
/// was one-sided and the result is a boundary rather than an optimum.
fn spsa_railed_parameters(report: &SpsaReport) -> String {
    let railed = report
        .bound_tune
        .parameters
        .iter()
        .enumerate()
        .filter_map(|(index, bound)| {
            let parameter = &bound.parameter;
            let value = report
                .tuned_result
                .as_ref()
                .and_then(|result| result.parameters.get(index))
                .map(|tuned| tuned.tuned)
                .or_else(|| {
                    report
                        .driver
                        .final_centers
                        .get(index)
                        .copied()
                        .and_then(|center| round_half_away_from_zero(center).ok())
                })?;
            if value <= parameter.min {
                Some(format!("{} at {}", parameter.name, parameter.min))
            } else if value >= parameter.max {
                Some(format!("{} at {}", parameter.name, parameter.max))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    if railed.is_empty() {
        "none".to_owned()
    } else {
        railed.join(", ")
    }
}

pub(crate) fn print_spsa(report: &SpsaReport, run_directory: &Path) {
    println!("SPSA {}", spsa_verdict(report.driver.status));

    let rows = spsa_result_rows(report);
    let headers = [
        "parameter",
        "initial",
        "tuned",
        "estimate",
        "delta",
        "range",
    ];
    let columns: Vec<Vec<&str>> = vec![
        rows.iter().map(|row| row.parameter.as_str()).collect(),
        rows.iter().map(|row| row.initial.as_str()).collect(),
        rows.iter().map(|row| row.tuned.as_str()).collect(),
        rows.iter().map(|row| row.estimate.as_str()).collect(),
        rows.iter().map(|row| row.delta.as_str()).collect(),
        rows.iter().map(|row| row.range.as_str()).collect(),
    ];
    let widths = headers
        .iter()
        .zip(&columns)
        .map(|(header, column)| {
            column
                .iter()
                .map(|value| value.chars().count())
                .chain(std::iter::once(header.chars().count()))
                .max()
                .unwrap_or(0)
        })
        .collect::<Vec<_>>();
    let render = |cells: &[&str]| -> String {
        cells
            .iter()
            .zip(&widths)
            .enumerate()
            .map(|(index, (cell, width))| {
                // The name reads left to right; every number reads right to
                // left, so a column of them lines up on its digits.
                if index == 0 {
                    format!("{cell:<width$}")
                } else {
                    format!("{cell:>width$}")
                }
            })
            .collect::<Vec<_>>()
            .join("  ")
            .trim_end()
            .to_owned()
    };
    println!("{}", render(&headers));
    for row in &rows {
        println!(
            "{}",
            render(&[
                &row.parameter,
                &row.initial,
                &row.tuned,
                &row.estimate,
                &row.delta,
                &row.range,
            ])
        );
    }

    println!(
        "estimator: {}",
        match report.tuned_result.as_ref().map(|result| &result.estimator) {
            Some(SpsaEstimator::FinalCenter { iteration }) =>
                format!("rounded centre vector after iteration {iteration}"),
            Some(SpsaEstimator::TailWindowMean(window)) => format!(
                "rounded mean of {} from the final {}% window",
                progress::plural(u64::from(window.samples_used), "sample"),
                window.percent
            ),
            None => "none: this tune produced no vector".to_owned(),
        }
    );
    let committed = report.driver.completed_iterations.len();
    println!(
        "iterations: {} of {} committed; games: {}",
        committed,
        report.driver.settings.iterations,
        committed as u64 * u64::from(report.driver.settings.games_per_iteration)
    );
    let faults = spsa_faults(&spsa_driver::SpsaCheckpoint {
        completed_iterations: report.driver.completed_iterations.clone(),
        invalid_iteration: report.driver.invalid_iteration.clone(),
    });
    let games_played = (committed as u64 + u64::from(report.driver.invalid_iteration.is_some()))
        * u64::from(report.driver.settings.games_per_iteration);
    println!(
        "faults: time {}/{}, other {}/{}; {}",
        faults.time_losses_a,
        faults.time_losses_b,
        faults.engine_a.saturating_sub(faults.time_losses_a),
        faults.engine_b.saturating_sub(faults.time_losses_b),
        fault_allowance_text(report.fault_policy, faults, games_played)
    );
    if let Some(invalid) = &report.driver.invalid_iteration {
        println!(
            "iteration {} invalid: {}; no gradient applied",
            invalid.iteration, invalid.reason
        );
    }
    println!("centres at a rail: {}", spsa_railed_parameters(report));
    println!("artifacts: {}", run_directory.display());
}

/// The run's outcome in the words a reader uses for it.
fn spsa_verdict(status: spsa_driver::SpsaStatus) -> &'static str {
    match status {
        spsa_driver::SpsaStatus::Completed => "completed",
        spsa_driver::SpsaStatus::Cancelled => {
            "cancelled - stopped cleanly at an iteration boundary"
        }
        spsa_driver::SpsaStatus::Invalid => "invalid - an engine fault invalidated a mini-match",
    }
}

pub(crate) fn print_spsa_tune_warning(warning: &SpsaTuneWarning) {
    match warning {
        SpsaTuneWarning::InitialDiffersFromEngineDefault {
            name,
            initial,
            advertised_default,
        } => eprintln!(
            "SPSA tune warning: {name:?} starts at {initial}, but the engine advertises default {advertised_default}; this may be deliberate"
        ),
        SpsaTuneWarning::InitialOnLowerRail {
            name,
            initial,
            rail,
        } => eprintln!(
            "SPSA tune warning: {name:?} starts on its lower rail ({initial} = {rail}); its initial gradient is one-sided"
        ),
        SpsaTuneWarning::InitialOnUpperRail {
            name,
            initial,
            rail,
        } => eprintln!(
            "SPSA tune warning: {name:?} starts on its upper rail ({initial} = {rail}); its initial gradient is one-sided"
        ),
    }
}
