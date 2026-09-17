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
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u64).range(1..))]
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
    let checkpoint = if run_directory.join("checkpoint.json").exists()
        || run_directory.join("checkpoint.previous.json").exists()
    {
        match RunDirectory::read_checkpoint_snapshot::<spsa_driver::SpsaCheckpoint>(run_directory) {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                eprintln!("SPSA status failed: {error}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        spsa_driver::SpsaCheckpoint::default()
    };
    let verified_schedule = match read_and_verify_spsa_schedule(run_directory, &workflow.schedule) {
        Ok(schedule) => schedule,
        Err(error) => {
            eprintln!("SPSA status failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let durable_updates = checkpoint
        .completed_iterations
        .iter()
        .map(|iteration| SpsaCommittedUpdate {
            iteration: iteration.iteration,
            centers_before: iteration.centers_before.clone(),
            prepared: iteration.prepared.clone(),
            score: iteration.score,
            centers_after: iteration.centers_after.clone(),
        })
        .collect::<Vec<_>>();
    let replayed = match SpsaTuningState::resume(
        verified_schedule,
        workflow.settings,
        workflow.bound_tune.initial_centers(),
        &durable_updates,
    ) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("SPSA status failed: checkpoint does not replay: {error}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(invalid) = &checkpoint.invalid_iteration {
        let prepared = replayed.prepare_next();
        if invalid.iteration != replayed.completed_iterations()
            || invalid.centers_before != replayed.centers()
            || prepared.as_ref().ok().and_then(|value| value.as_ref()) != Some(&invalid.prepared)
        {
            eprintln!(
                "SPSA status failed: terminal invalid iteration does not match the durable prefix"
            );
            return ExitCode::FAILURE;
        }
    }
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
    let invalid = checkpoint.invalid_iteration.is_some() || record.status == RunStatus::Invalid;
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
    }
    ExitCode::SUCCESS
}

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
    let checkpoint = if resumed
        && (directory.paths().checkpoint.exists() || directory.paths().previous_checkpoint.exists())
    {
        match directory.read_checkpoint::<spsa_driver::SpsaCheckpoint>() {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                eprintln!("resume failed: {error}");
                return ExitCode::from(3);
            }
        }
    } else {
        spsa_driver::SpsaCheckpoint::default()
    };
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
        "fault_policy": "any engine fault invalidates the iteration and tune"
    })) {
        eprintln!("run record failed: {error}");
        return ExitCode::from(3);
    }
    let observer = match DurableSpsaOutput::new(
        Arc::clone(&directory),
        checkpoint.clone(),
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
                let block = spsa_progress_block(&observer, &schedule, settings, &centres);
                if schedule.due(block.done) {
                    observer.publish_progress(&block);
                    centres.remember(&observer.current_centers());
                }
            }
        }
    };
    let final_block = spsa_progress_block(&observer, &schedule, settings, &centres);
    if schedule.needs_final(final_block.done) {
        schedule.mark(final_block.done);
        observer.publish_progress(&final_block);
    }
    let driver = match outcome {
        Ok(report) => report,
        Err(error) => {
            eprintln!("SPSA failed: {error}");
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
        driver,
    };
    if let Err(error) = observer.finish(&report) {
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
    /// Knob name and the width of its range, in schedule order.
    knobs: Vec<(String, f64)>,
    /// The centres at the previous published block.
    previous: Vec<f64>,
}

impl SpsaCentreTracker {
    pub(crate) fn new(knobs: &[SpsaDerivedKnob], initial_centers: &[f64]) -> Self {
        Self {
            knobs: knobs
                .iter()
                .map(|knob| (knob.name.clone(), (knob.max - knob.min) as f64))
                .collect(),
            previous: initial_centers.to_vec(),
        }
    }

    /// The three largest moves since the previous block, largest first.
    pub(crate) fn largest_moves(&self, centers: &[f64]) -> Option<String> {
        if centers.len() != self.knobs.len() || self.previous.len() != self.knobs.len() {
            return None;
        }
        let mut moves = self
            .knobs
            .iter()
            .zip(centers)
            .zip(&self.previous)
            .filter(|(((_, range), _), _)| *range > 0.0)
            .map(|(((name, range), center), previous)| (name.clone(), (center - previous) / range))
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

    /// Take the centres a published block reported as the new baseline.
    pub(crate) fn remember(&mut self, centers: &[f64]) {
        if centers.len() == self.knobs.len() {
            self.previous = centers.to_vec();
        }
    }
}

/// What a tune tells the operator: how far through the horizon it is, what the
/// last mini-match said, how hard the schedule is still pushing, and which
/// centres are actually moving.
pub(crate) fn spsa_progress_block(
    observer: &DurableSpsaOutput,
    schedule: &ProgressSchedule,
    settings: SpsaRunSettings,
    centres: &SpsaCentreTracker,
) -> ProgressBlock {
    let last = observer.checkpoint.lock().ok().and_then(|checkpoint| {
        checkpoint
            .completed_iterations
            .last()
            .cloned()
            .map(|iteration| (checkpoint.completed_iterations.len() as u64, iteration))
    });
    let (done, last) = match last {
        Some((done, iteration)) => (done, Some(iteration)),
        None => (0, None),
    };
    let total = u64::from(settings.iterations);
    let mut block = ProgressBlock::new(
        "spsa",
        ProgressUnit::Iterations,
        done,
        Some(total),
        schedule.elapsed(),
    );
    block.field(
        "time remaining",
        match progress::time_for_units(
            schedule.units_since_start(done),
            schedule.elapsed(),
            total.saturating_sub(done),
        ) {
            Some(left) => progress::format_duration(left.as_secs_f64()),
            None => "unknown".to_owned(),
        },
    );
    match &last {
        Some(iteration) => {
            let score = iteration.score;
            block
                .field(
                    "last mini-match",
                    format!(
                        "iteration {}: {:+} (plus {} / draws {} / minus {})",
                        iteration.iteration,
                        score.difference,
                        score.plus_wins,
                        score.draws,
                        score.plus_losses
                    ),
                )
                .field("schedule", coefficient_summary(&iteration.prepared));
            match centres.largest_moves(&iteration.centers_after) {
                Some(moves) => block.field(
                    "largest moves",
                    format!("{moves} (range units since the previous block)"),
                ),
                None => block.field("largest moves", "none recorded yet"),
            };
        }
        None => {
            block.field("last mini-match", "no iteration has committed yet");
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

pub(crate) struct DurableSpsaOutput {
    pub(crate) directory: Arc<RunDirectory>,
    pub(crate) checkpoint: Mutex<spsa_driver::SpsaCheckpoint>,
    pub(crate) recorder: Mutex<Option<RunRecorder>>,
    pub(crate) settings: SpsaRunSettings,
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
    pub(crate) fn new(
        directory: Arc<RunDirectory>,
        checkpoint: spsa_driver::SpsaCheckpoint,
        mut recorder: RunRecorder,
        settings: SpsaRunSettings,
    ) -> Result<Self, String> {
        recorder
            .update_sample(spsa_official_sample(&checkpoint, settings))
            .map_err(|error| error.to_string())?;
        let output = Self {
            directory,
            checkpoint: Mutex::new(checkpoint),
            recorder: Mutex::new(Some(recorder)),
            settings,
        };
        output.rewrite_pgn()?;
        Ok(output)
    }

    /// Committed iterations this run already holds.
    pub(crate) fn committed_iterations(&self) -> u64 {
        self.checkpoint
            .lock()
            .map_or(0, |checkpoint| checkpoint.completed_iterations.len() as u64)
    }

    /// The centre vector the tune currently stands on.
    pub(crate) fn current_centers(&self) -> Vec<f64> {
        self.checkpoint.lock().map_or_else(
            |_| Vec::new(),
            |checkpoint| {
                checkpoint
                    .completed_iterations
                    .last()
                    .map(|iteration| iteration.centers_after.clone())
                    .unwrap_or_default()
            },
        )
    }

    /// Publish a block through the run recorder this observer owns.
    pub(crate) fn publish_progress(&self, block: &ProgressBlock) {
        show_progress(block, &self.directory);
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

    pub(crate) fn persist_checkpoint(&self) -> Result<(), String> {
        let checkpoint = self
            .checkpoint
            .lock()
            .map_err(|_| "SPSA checkpoint lock poisoned")?;
        self.directory
            .write_checkpoint(&*checkpoint)
            .map_err(|error| error.to_string())?;
        let sample = spsa_official_sample(&checkpoint, self.settings);
        drop(checkpoint);
        let mut recorder = self
            .recorder
            .lock()
            .map_err(|_| "SPSA run-record lock poisoned")?;
        recorder
            .as_mut()
            .ok_or("SPSA run recorder is already finished")?
            .update_sample(sample)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn rewrite_pgn(&self) -> Result<(), String> {
        let checkpoint = self
            .checkpoint
            .lock()
            .map_err(|_| "SPSA PGN lock poisoned")?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.directory.paths().root.join("games.pgn"))
            .map_err(|error| error.to_string())?;
        for iteration in &checkpoint.completed_iterations {
            for pair in &iteration.pairs {
                for game in [&pair.first, &pair.second] {
                    writeln!(
                        file,
                        "{tagged}\n",
                        tagged = with_header_tags(
                            game.pgn.trim_end(),
                            &[
                                (
                                    "ColosseumSample",
                                    sample_class(game.scorable, OFFICIAL_SAMPLE),
                                ),
                                ("ColosseumSpsaIteration", &iteration.iteration.to_string()),
                            ],
                        )
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
        }
        if let Some(iteration) = &checkpoint.invalid_iteration {
            for pair in &iteration.pairs {
                for game in [&pair.first, &pair.second] {
                    writeln!(
                        file,
                        "{tagged}\n",
                        tagged = with_header_tags(
                            game.pgn.trim_end(),
                            &[
                                ("ColosseumSample", sample_class(game.scorable, "invalid")),
                                ("ColosseumSpsaIteration", &iteration.iteration.to_string()),
                            ],
                        )
                    )
                    .map_err(|error| error.to_string())?;
                }
            }
        }
        file.sync_all().map_err(|error| error.to_string())
    }

    pub(crate) fn append_event(&self, event: Value) -> Result<(), String> {
        let mut line = serde_json::to_vec(&event).map_err(|error| error.to_string())?;
        line.push(b'\n');
        self.directory
            .append_log(&line)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn finish(&self, report: &SpsaReport) -> Result<(), String> {
        if let Some(result) = &report.tuned_result {
            let json = serde_json::to_vec_pretty(result).map_err(|error| error.to_string())?;
            fs::write(self.directory.paths().root.join("tuned-options.json"), json)
                .map_err(|error| error.to_string())?;
            let mut setoptions = String::new();
            let mut options = BTreeMap::new();
            for parameter in &result.parameters {
                setoptions.push_str(&format!(
                    "setoption name {} value {}\n",
                    parameter.name, parameter.tuned
                ));
                options.insert(parameter.name.clone(), parameter.tuned);
            }
            fs::write(
                self.directory.paths().root.join("tuned-options.txt"),
                setoptions,
            )
            .map_err(|error| error.to_string())?;
            let fragment = toml::to_string_pretty(&SpsaRunFileFragment {
                engine: SpsaRunFileEngine { options },
            })
            .map_err(|error| error.to_string())?;
            fs::write(
                self.directory.paths().root.join("tuned-options.toml"),
                fragment,
            )
            .map_err(|error| error.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
        fs::write(self.directory.paths().root.join("result.json"), bytes)
            .map_err(|error| error.to_string())?;
        self.append_event(json!({
            "event": "spsa-finished",
            "status": report.driver.status,
            "completed_iterations": report.driver.completed_iterations.len(),
            "invalid_iteration": report.driver.invalid_iteration.as_ref().map(|value| value.iteration),
        }))?;
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
        self.checkpoint
            .lock()
            .map_err(|_| "SPSA checkpoint lock poisoned")?
            .completed_iterations
            .push(iteration.clone());
        self.persist_checkpoint()?;
        self.rewrite_pgn()?;
        self.append_event(json!({
            "event": "spsa-iteration-committed",
            "iteration": iteration,
        }))
    }

    fn iteration_invalid(
        &self,
        iteration: &spsa_driver::SpsaInvalidIteration,
    ) -> Result<(), String> {
        self.checkpoint
            .lock()
            .map_err(|_| "SPSA checkpoint lock poisoned")?
            .invalid_iteration = Some(iteration.clone());
        self.persist_checkpoint()?;
        self.rewrite_pgn()?;
        self.append_event(json!({
            "event": "spsa-iteration-invalid",
            "iteration": iteration,
        }))
    }
}

pub(crate) fn spsa_official_sample(
    checkpoint: &spsa_driver::SpsaCheckpoint,
    settings: SpsaRunSettings,
) -> OfficialSample {
    let iterations = checkpoint.completed_iterations.len() as u64;
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

pub(crate) fn print_spsa(report: &SpsaReport, run_directory: &Path) {
    println!(
        "SPSA {:?}: {}/{} complete iterations ({} games each)",
        report.driver.status,
        report.driver.completed_iterations.len(),
        report.driver.settings.iterations,
        report.driver.settings.games_per_iteration
    );
    for (parameter, center) in report
        .bound_tune
        .parameters
        .iter()
        .zip(&report.driver.final_centers)
    {
        println!("{}: {:.6}", parameter.parameter.name, center);
    }
    if let Some(result) = &report.tuned_result {
        match &result.estimator {
            SpsaEstimator::FinalCenter { iteration } => {
                println!("tuned vector: rounded centre vector after iteration {iteration}")
            }
            SpsaEstimator::TailWindowMean(window) => println!(
                "tuned vector: rounded mean of {} sample(s) from final {}% window",
                window.samples_used, window.percent
            ),
        }
        for parameter in &result.parameters {
            println!(
                "setoption name {} value {}  (estimate {:.6})",
                parameter.name, parameter.tuned, parameter.estimate
            );
        }
    }
    if let Some(invalid) = &report.driver.invalid_iteration {
        println!(
            "iteration {} invalid: {} engine faults; no gradient applied",
            invalid.iteration,
            invalid.faults.engine_a + invalid.faults.engine_b
        );
    }
    println!("artifacts: {}", run_directory.display());
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
