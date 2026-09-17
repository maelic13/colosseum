//! The `calibrate` command: identical-binary symmetry on this machine.

use super::*;

#[derive(Debug, Args)]
pub(crate) struct CalibrationCommand {
    /// Path to the first copy of the representative executable.
    pub(crate) engine_a: PathBuf,
    /// Path to the second copy of the representative executable.
    pub(crate) engine_b: PathBuf,

    /// Complete games to measure; it must be even so every sample is colour-paired.
    #[arg(long, default_value_t = DEFAULT_CALIBRATION_GAMES)]
    pub(crate) games: u32,
    /// Two-sided confidence level for the normalized-Elo interval.
    #[arg(long, default_value_t = DEFAULT_CALIBRATION_CONFIDENCE)]
    pub(crate) confidence: f64,
    /// Inclusive normalized-Elo interval tolerance around zero.
    #[arg(long, default_value_t = DEFAULT_CALIBRATION_TOLERANCE_NELO)]
    pub(crate) tolerance_nelo: f64,

    #[command(flatten)]
    pub(crate) conditions: MatchConditions,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CalibrationReport {
    pub(crate) status: CalibrationStatus,
    pub(crate) design: CalibrationDesign,
    pub(crate) binaries: CalibrationBinaries,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) interval: Option<CalibrationInterval>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) statistics_unavailable: Option<String>,
    pub(crate) fixed_match: match_runner::FixedMatchReport,
}

pub(crate) struct PreparedCalibration {
    pub(crate) design: CalibrationDesign,
    pub(crate) binaries: CalibrationBinaries,
    pub(crate) engine_a: EngineLaunchSpec,
    pub(crate) engine_b: EngineLaunchSpec,
    pub(crate) engine_a_time_control: match_runner::ConfiguredTimeControl,
    pub(crate) engine_b_time_control: match_runner::ConfiguredTimeControl,
    pub(crate) adjudication: AdjudicationConfig,
    pub(crate) ponder: bool,
    pub(crate) fault_policy: match_runner::FaultPolicy,
    pub(crate) execution: match_runner::MatchExecutionPlan,
    pub(crate) master_seed: u64,
    pub(crate) master_seed_generated: bool,
    pub(crate) openings: match_runner::MatchOpenings,
    pub(crate) current_directory: PathBuf,
    pub(crate) resolved: crate::ResolvedConfig,
}

pub(crate) async fn run_calibration(
    command: CalibrationCommand,
    machine: bool,
    dry_run: bool,
    cancellation: Cancellation,
) -> ExitCode {
    let prepared = match prepare_calibration(&command) {
        Ok(prepared) => prepared,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if dry_run {
        print_output(
            &MachineOutput::DryRun {
                command: "calibrate",
                config_sha256: prepared.resolved.sha256(),
                resolved_configuration: prepared.resolved.value(),
                invocations: vec![&prepared.engine_a, &prepared.engine_b],
            },
            machine,
        );
        return ExitCode::SUCCESS;
    }

    let conditions = &command.conditions;
    let opened = match &conditions.run_directory {
        Some(path) => RunDirectory::open_explicit(path, &prepared.resolved, conditions.restart),
        None => RunDirectory::create_unique(
            &prepared.current_directory,
            "calibrate",
            &prepared.resolved,
        ),
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
    let directory = Arc::new(opened.directory);
    let completed_games = if opened.resumed
        && (directory.paths().checkpoint.exists() || directory.paths().previous_checkpoint.exists())
    {
        match directory.read_checkpoint::<match_runner::MatchCheckpoint>() {
            Ok(checkpoint) => checkpoint.games,
            Err(error) => {
                eprintln!("resume failed: {error}");
                return ExitCode::from(3);
            }
        }
    } else {
        Vec::new()
    };
    let observer = match DurableMatchOutput::new(Arc::clone(&directory), completed_games.clone()) {
        Ok(observer) => Arc::new(observer),
        Err(error) => {
            eprintln!("calibration output failed: {error}");
            return ExitCode::from(3);
        }
    };
    colosseum_engine::incidents::set_dir(directory.paths().root.join("failed-games"));
    let mut recorder = match if opened.resumed {
        RunRecorder::resume(&directory)
    } else {
        RunRecorder::begin(&directory, "calibrate")
    } {
        Ok(recorder) => recorder,
        Err(error) => {
            eprintln!("run record failed: {error}");
            return ExitCode::from(3);
        }
    };
    if let Err(error) = recorder.set_workflow(json!({
        "kind": "calibration",
        "pgn_annotation_writer": colosseum_engine::pgn::PGN_ANNOTATION_WRITER,
        "optional": true,
        "design": prepared.design,
        "binaries": prepared.binaries,
        "engine_a_time_control": prepared.engine_a_time_control,
        "engine_b_time_control": prepared.engine_b_time_control,
        "adjudication": prepared.adjudication,
        "ponder": prepared.ponder,
        "fault_policy": prepared.fault_policy,
        "execution": prepared.execution,
        "master_seed": prepared.master_seed,
        "master_seed_generated": prepared.master_seed_generated,
        "openings": prepared.openings.report(),
    })) {
        eprintln!("run record failed: {error}");
        return ExitCode::from(3);
    }
    let progress = match_runner::MatchProgress::default();
    let request = match_runner::FixedMatchRequest {
        engine_a: prepared.engine_a,
        engine_b: prepared.engine_b,
        games: prepared.design.games,
        engine_a_time_control: prepared.engine_a_time_control,
        engine_b_time_control: prepared.engine_b_time_control,
        adjudication: prepared.adjudication,
        ponder: prepared.ponder,
        fault_policy: prepared.fault_policy,
        execution: prepared.execution,
        master_seed: prepared.master_seed,
        master_seed_generated: prepared.master_seed_generated,
        openings: prepared.openings,
        completed_games,
        progress: progress.clone(),
        cancellation: cancellation.clone(),
        observer: Some(observer.clone()),
    };
    if !machine {
        eprintln!(
            "calibration run directory: {}",
            directory.paths().root.display()
        );
    }
    let calibration_future = match_runner::run_fixed_match(request);
    tokio::pin!(calibration_future);
    let period = Duration::from_secs(conditions.progress_interval_secs);
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    let outcome = loop {
        tokio::select! {
            result = &mut calibration_future => break result,
            _ = interval.tick() => {
                let snapshot = progress.snapshot();
                eprintln!("calibration progress: {}/{} attempted, {} scored, {} faults", snapshot.attempted, prepared.design.games, snapshot.scored, snapshot.faults);
            }
        }
    };
    let fixed_match = match outcome {
        Ok(report) => report,
        Err(error) => {
            eprintln!("calibration failed: {error}");
            return ExitCode::from(3);
        }
    };
    let (status, interval, statistics_unavailable, exit_code) = match fixed_match.status {
        match_runner::MatchStatus::InfrastructureError => {
            eprintln!("calibration failed: a non-scorable infrastructure fault occurred");
            return ExitCode::from(3);
        }
        match_runner::MatchStatus::Invalid => (CalibrationStatus::Invalid, None, None, 5),
        // A calibration measures a fixed sample; stopping short of it has no
        // interval to classify, so it reports the stop rather than a verdict.
        match_runner::MatchStatus::Cancelled => (
            CalibrationStatus::Inconclusive,
            None,
            Some("the calibration stopped cleanly before its fixed sample was complete".to_owned()),
            CANCELLED_EXIT_CODE,
        ),
        match_runner::MatchStatus::Completed => {
            let (interval, unavailable) = match calibration_interval(&fixed_match, prepared.design)
            {
                Ok((interval, unavailable)) => (interval, unavailable),
                Err(error) => {
                    eprintln!("calibration failed: {error}");
                    return ExitCode::from(3);
                }
            };
            let status =
                classify_calibration(prepared.design, interval, fixed_match.faults.engine_total());
            let exit_code = calibration_exit_code(status);
            (status, interval, unavailable, exit_code)
        }
    };
    let report = CalibrationReport {
        status,
        design: prepared.design,
        binaries: prepared.binaries,
        interval,
        statistics_unavailable,
        fixed_match,
    };
    if let Err(error) = observer.finish_calibration(&report) {
        eprintln!("calibration output failed: {error}");
        return ExitCode::from(3);
    }
    let pentanomial = calibration_sample(&report.fixed_match)
        .map(|sample| sample.counts().map(u64::from))
        .unwrap_or([0; 5]);
    let sample = OfficialSample {
        committed_units: u64::from(report.fixed_match.games_attempted),
        scored_games: u64::from(report.fixed_match.games_completed),
        completed_pairs: u64::from(report.fixed_match.games_completed / 2),
        pentanomial,
        unpaired_games: 0,
    };
    if let Err(error) = recorder.update_sample(sample) {
        eprintln!("run record failed: {error}");
        return ExitCode::from(3);
    }
    let run_status = if status == CalibrationStatus::Invalid {
        RunStatus::Invalid
    } else if report.fixed_match.status == match_runner::MatchStatus::Cancelled {
        RunStatus::Cancelled
    } else {
        RunStatus::Completed
    };
    if let Err(error) = recorder.finish(run_status) {
        eprintln!("run record failed: {error}");
        return ExitCode::from(3);
    }
    if machine {
        print_json(&MachineOutput::Calibration {
            run_directory: directory.paths().root.clone(),
            report,
        });
    } else {
        print_calibration(&report, &directory.paths().root);
    }
    ExitCode::from(exit_code)
}

pub(crate) fn prepare_calibration(
    command: &CalibrationCommand,
) -> Result<PreparedCalibration, String> {
    let design = CalibrationDesign::new(command.games, command.confidence, command.tolerance_nelo)
        .map_err(|error| error.to_string())?;
    let conditions = &command.conditions;
    if conditions.book.is_none()
        && (conditions.book_start != 0
            || conditions.book_plies.is_some()
            || conditions.book_order != BookOrderArg::Sequential)
    {
        return Err("book order/start/plies require --book".into());
    }
    let resumed_seed = conditions
        .run_directory
        .as_deref()
        .filter(|path| path.exists() && !conditions.restart)
        .and_then(read_stored_seed);
    let (master_seed, master_seed_generated) = match resumed_seed {
        Some(seed) if conditions.seed.is_none() => seed,
        _ => resolve_master_seed(conditions.seed)?,
    };
    let adjudication = resolve_adjudication(conditions);
    let fault_policy = match_runner::FaultPolicy {
        max_engine_faults: conditions.max_engine_faults,
        max_time_losses: conditions.max_time_losses,
    };
    let engine_a_time_control = resolve_time_control(
        "engine A",
        conditions.a_movetime_ms,
        conditions.a_base_ms,
        conditions.a_increment_ms,
        conditions.a_nodes,
        conditions.a_depth,
        conditions.a_margin_ms,
    )?;
    let engine_b_time_control = resolve_time_control(
        "engine B",
        conditions.b_movetime_ms,
        conditions.b_base_ms,
        conditions.b_increment_ms,
        conditions.b_nodes,
        conditions.b_depth,
        conditions.b_margin_ms,
    )?;
    validate_ponder(
        conditions.ponder,
        &[engine_a_time_control, engine_b_time_control],
    )?;
    let mut engine_a = resolve_match_engine(
        command.engine_a.clone(),
        conditions.a_label.clone(),
        conditions.a_arguments.clone(),
        conditions.a_cwd.clone(),
        conditions.a_environment.clone(),
        conditions.a_options.clone(),
        conditions.a_buttons.clone(),
        conditions.a_cores.clone(),
    )
    .map_err(|error| error.to_string())?;
    let mut engine_b = resolve_match_engine(
        command.engine_b.clone(),
        conditions.b_label.clone(),
        conditions.b_arguments.clone(),
        conditions.b_cwd.clone(),
        conditions.b_environment.clone(),
        conditions.b_options.clone(),
        conditions.b_buttons.clone(),
        conditions.b_cores.clone(),
    )
    .map_err(|error| error.to_string())?;
    configure_ponder(&mut engine_a, conditions.ponder)?;
    configure_ponder(&mut engine_b, conditions.ponder)?;
    let binaries = CalibrationBinaries::new(
        executable_sha256(&engine_a.executable)?,
        executable_sha256(&engine_b.executable)?,
    )
    .map_err(|error| error.to_string())?;
    let placement = resolve_placement(&conditions.placement, conditions.headroom_cores)?;
    let execution = match_runner::plan_execution(
        &engine_a,
        &engine_b,
        conditions.concurrency as usize,
        conditions.cores_per_engine as usize,
        placement,
        conditions.memory_budget_mb,
    )
    .map_err(|error| error.to_string())?;
    let book = conditions.book.clone().map(|path| {
        let mut book = OpeningBook::new(path);
        book.order = match conditions.book_order {
            BookOrderArg::Sequential => OpeningOrder::Sequential,
            BookOrderArg::Random => OpeningOrder::Random,
        };
        book.plies = conditions.book_plies.unwrap_or(8);
        book
    });
    let openings = match_runner::resolve_openings(
        book,
        conditions.book_start,
        design.games.div_ceil(2),
        conditions.book_wrap,
        master_seed,
    )
    .map_err(|error| error.to_string())?;
    let current_directory = std::env::current_dir().map_err(|error| error.to_string())?;
    let resolved = resolve_config(
        built_in_defaults(),
        None,
        json!({
            "command": "calibrate",
            "design": design,
            "binaries": &binaries,
            "engine_a": &engine_a,
            "engine_b": &engine_b,
            "engine_a_time_control": engine_a_time_control,
            "engine_b_time_control": engine_b_time_control,
            "adjudication": adjudication,
            "ponder": conditions.ponder,
            "fault_policy": fault_policy,
            "execution": execution,
            "master_seed": master_seed,
            "master_seed_generated": master_seed_generated,
            "openings": openings.report(),
        }),
        &[],
        &current_directory,
        &match conditions.book {
            Some(_) => vec![
                "/engine_a/executable".into(),
                "/engine_b/executable".into(),
                "/openings/path".into(),
            ],
            None => vec!["/engine_a/executable".into(), "/engine_b/executable".into()],
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(PreparedCalibration {
        design,
        binaries,
        engine_a,
        engine_b,
        engine_a_time_control,
        engine_b_time_control,
        adjudication,
        ponder: conditions.ponder,
        fault_policy,
        execution,
        master_seed,
        master_seed_generated,
        openings,
        current_directory,
        resolved,
    })
}

pub(crate) fn calibration_interval(
    report: &match_runner::FixedMatchReport,
    design: CalibrationDesign,
) -> Result<(Option<CalibrationInterval>, Option<String>), String> {
    let sample = calibration_sample(report)?;
    match fixed_n_achieved_resolution(&sample, EloModel::Normalized, design.significance()) {
        Ok(resolution) => Ok((Some(resolution.into()), None)),
        Err(error) => Ok((None, Some(error.to_string()))),
    }
}

pub(crate) fn calibration_sample(
    report: &match_runner::FixedMatchReport,
) -> Result<PentanomialVector, String> {
    if report.games.len() != report.games_requested as usize {
        return Err("calibration did not retain every requested game".into());
    }
    let mut sample = PentanomialVector::default();
    for [first, second] in report.games.as_chunks::<2>().0 {
        if !first.scorable || !second.scorable {
            return Err("calibration cannot compute an interval from a non-scorable game".into());
        }
        if first.number + 1 != second.number
            || first.white != match_runner::MatchSide::A
            || second.white != match_runner::MatchSide::B
        {
            return Err(
                "calibration game schedule is not a complete colour-reversed prefix".into(),
            );
        }
        sample.record_pair(
            result_for_engine_a(first.white, first.result),
            result_for_engine_a(second.white, second.result),
        );
    }
    Ok(sample)
}

pub(crate) fn result_for_engine_a(
    white: match_runner::MatchSide,
    result: GameResult,
) -> PairGameResult {
    match (white, result) {
        (_, GameResult::Draw) => PairGameResult::Draw,
        (match_runner::MatchSide::A, GameResult::WhiteWin)
        | (match_runner::MatchSide::B, GameResult::BlackWin) => PairGameResult::Win,
        (match_runner::MatchSide::A, GameResult::BlackWin)
        | (match_runner::MatchSide::B, GameResult::WhiteWin) => PairGameResult::Loss,
    }
}

pub(crate) fn calibration_exit_code(status: CalibrationStatus) -> u8 {
    match status {
        CalibrationStatus::Pass => 0,
        CalibrationStatus::Fail => 1,
        CalibrationStatus::Inconclusive => 4,
        CalibrationStatus::Invalid => 5,
    }
}

pub(crate) fn print_calibration(report: &CalibrationReport, run_directory: &Path) {
    println!(
        "calibration {:?}: {} games, {:.0}% confidence, ±{} nElo tolerance",
        report.status,
        report.design.games,
        report.design.confidence * 100.0,
        report.design.tolerance_nelo
    );
    if let Some(interval) = report.interval {
        println!(
            "normalized Elo: {:.3} [{:.3}, {:.3}]",
            interval.estimate_nelo, interval.lower_nelo, interval.upper_nelo
        );
    }
    if let Some(reason) = &report.statistics_unavailable {
        println!("interval unavailable: {reason}");
    }
    println!("artifacts: {}", run_directory.display());
}
