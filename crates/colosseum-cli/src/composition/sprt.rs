//! The `sprt` command: a finite pair-atomic sequential test, and `--apply`.

use super::*;

#[derive(Debug, Args)]
pub(crate) struct SprtCommand {
    /// Path to engine A; omit both engine paths when using --apply.
    pub(crate) engine_a: Option<PathBuf>,
    /// Path to engine B; omit both engine paths when using --apply.
    pub(crate) engine_b: Option<PathBuf>,
    /// Completed SPSA result.json whose original/tuned vectors form the gate.
    #[arg(long, value_name = "RESULT_JSON")]
    pub(crate) apply: Option<PathBuf>,
    /// Use this executable instead of the path recorded by the SPSA result.
    #[arg(long, requires = "apply", value_name = "EXECUTABLE")]
    pub(crate) apply_executable: Option<PathBuf>,
    /// Proceed with --apply despite an executable SHA-256 mismatch.
    #[arg(long, requires = "apply")]
    pub(crate) allow_executable_mismatch: bool,

    /// Required finite cap; reaching it without a boundary is inconclusive.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) max_pairs: u32,
    /// Named starting design; every field remains overridable.
    #[arg(long, value_enum)]
    pub(crate) preset: Option<SprtBundleArg>,
    /// Elo parameterization used by both hypotheses and the LLR.
    #[arg(long, value_enum)]
    pub(crate) model: Option<SprtModelArg>,
    /// Null-hypothesis Elo in the selected model.
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) elo0: Option<f64>,
    /// Alternative-hypothesis Elo in the selected model.
    #[arg(long, allow_hyphen_values = true)]
    pub(crate) elo1: Option<f64>,
    /// Type-I error probability.
    #[arg(long)]
    pub(crate) alpha: Option<f64>,
    /// Type-II error probability.
    #[arg(long)]
    pub(crate) beta: Option<f64>,

    /// Official pairs between progress blocks on standard error.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) progress_every: u64,
    /// Shortest time between two progress blocks. A run whose pairs finish
    /// faster than this coalesces them instead of flooding the console.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) progress_min_secs: u64,

    #[command(flatten)]
    pub(crate) conditions: MatchConditions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SprtBundleArg {
    Gainer,
    Simplify,
}

impl From<SprtBundleArg> for SprtBundle {
    fn from(value: SprtBundleArg) -> Self {
        match value {
            SprtBundleArg::Gainer => Self::Gainer,
            SprtBundleArg::Simplify => Self::Simplify,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SprtModelArg {
    Normalized,
    Logistic,
}

impl From<SprtModelArg> for colosseum_core::EloModel {
    fn from(value: SprtModelArg) -> Self {
        match value {
            SprtModelArg::Normalized => Self::Normalized,
            SprtModelArg::Logistic => Self::Logistic,
        }
    }
}

#[derive(Deserialize)]
pub(crate) struct SpsaApplySource {
    pub(crate) engine: EngineLaunchSpec,
    pub(crate) engine_sha256: String,
    pub(crate) tuned_result: SpsaTuneResult,
}

pub(crate) fn load_spsa_apply(
    requested_result: &Path,
    executable_override: Option<&Path>,
    allow_executable_mismatch: bool,
) -> Result<
    (
        EngineLaunchSpec,
        EngineLaunchSpec,
        Option<sprt_runner::SprtApplyRecord>,
    ),
    String,
> {
    let source_result = dunce::canonicalize(requested_result).map_err(|error| {
        format!(
            "cannot resolve SPSA result {}: {error}",
            requested_result.display()
        )
    })?;
    let bytes = fs::read(&source_result).map_err(|error| {
        format!(
            "cannot read SPSA result {}: {error}",
            source_result.display()
        )
    })?;
    let document: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "cannot parse SPSA result {}: {error}",
            source_result.display()
        )
    })?;
    // The result denies unknown fields, so a version whose fields moved would
    // otherwise be reported as a field rather than as the version it is.
    if let Some(tuned) = document.get("tuned_result") {
        require_schema_version(tuned, SPSA_TUNE_RESULT_SCHEMA_VERSION).map_err(|version| {
            format!(
                "cannot read SPSA result {}: {}",
                source_result.display(),
                SpsaTuneResultError::UnsupportedResultSchema { version }
            )
        })?;
    }
    let mut source: SpsaApplySource = serde_json::from_value(document).map_err(|error| {
        format!(
            "cannot parse SPSA result {}: {error}",
            source_result.display()
        )
    })?;
    source
        .tuned_result
        .validate()
        .map_err(|error| format!("invalid SPSA tuned result: {error}"))?;
    if source.engine_sha256 != source.tuned_result.engine_sha256 {
        return Err("SPSA result carries inconsistent executable SHA-256 values".into());
    }
    let executable = executable_override.unwrap_or(&source.engine.executable);
    let executable = dunce::canonicalize(executable).map_err(|error| {
        format!(
            "cannot resolve SPSA gate executable {}: {error}",
            executable.display()
        )
    })?;
    let actual_sha256 = executable_sha256(&executable)?;
    let identity = source
        .tuned_result
        .verify_gate_identity(actual_sha256, allow_executable_mismatch)
        .map_err(|error| error.to_string())?;
    source.engine.executable = executable.clone();
    source.engine.allocated_cpus = colosseum_application::CpuAllocation::Unrestricted;
    let base_label = source.engine.label.clone().unwrap_or_else(|| {
        executable
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("engine")
            .to_owned()
    });
    let mut tuned = source.engine.clone();
    let mut original = source.engine;
    tuned.label = Some(format!("{base_label} [SPSA tuned]"));
    original.label = Some(format!("{base_label} [SPSA original]"));
    for parameter in &source.tuned_result.parameters {
        tuned.options.insert(
            parameter.name.clone(),
            UciOptionValue::Spin(parameter.tuned),
        );
        original.options.insert(
            parameter.name.clone(),
            UciOptionValue::Spin(parameter.original),
        );
    }
    let record = sprt_runner::SprtApplyRecord {
        source_result,
        executable,
        identity,
        parameters: source.tuned_result.parameters,
    };
    Ok((tuned, original, Some(record)))
}

pub(crate) async fn run_sprt(
    command: SprtCommand,
    machine: bool,
    dry_run: bool,
    cancellation: Cancellation,
) -> ExitCode {
    let design = match resolve_sprt_design(&command) {
        Ok(design) => design,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let engine_a_path = command.engine_a.clone();
    let engine_b_path = command.engine_b.clone();
    let apply_path = command.apply.clone();
    let apply_executable = command.apply_executable.clone();
    let allow_executable_mismatch = command.allow_executable_mismatch;
    let progress_every = command.progress_every;
    let progress_min_secs = command.progress_min_secs;
    let command = command.conditions;
    if command.book.is_none()
        && (command.book_start != 0
            || command.book_plies.is_some()
            || command.book_order != BookOrderArg::Sequential)
    {
        eprintln!("configuration error: book order/start/plies require --book");
        return ExitCode::from(2);
    }
    let resumed_seed = command
        .run_directory
        .as_deref()
        .filter(|path| path.exists() && !command.restart)
        .and_then(read_stored_seed);
    let (master_seed, master_seed_generated) = match resumed_seed {
        Some(seed) if command.seed.is_none() => seed,
        _ => match resolve_master_seed(command.seed) {
            Ok(seed) => seed,
            Err(error) => {
                eprintln!("configuration error: {error}");
                return ExitCode::from(2);
            }
        },
    };
    let adjudication = resolve_adjudication(&command);
    // A forfeit is scored as the loss it is; the test is void only when the
    // faults outrun 0.5% of the games played, and never fewer than three.
    let fault_policy = command.sequential_fault_policy();
    let engine_a_time_control = match resolve_time_control(
        "engine A",
        command.a_movetime_ms,
        command.a_base_ms,
        command.a_increment_ms,
        command.a_nodes,
        command.a_depth,
        command.a_margin_ms,
    ) {
        Ok(control) => control,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let engine_b_time_control = match resolve_time_control(
        "engine B",
        command.b_movetime_ms,
        command.b_base_ms,
        command.b_increment_ms,
        command.b_nodes,
        command.b_depth,
        command.b_margin_ms,
    ) {
        Ok(control) => control,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = validate_ponder(
        command.ponder,
        &[engine_a_time_control, engine_b_time_control],
    ) {
        eprintln!("configuration error: {error}");
        return ExitCode::from(2);
    }
    let (mut engine_a, mut engine_b, apply_record) = if let Some(apply_path) = apply_path {
        if engine_a_path.is_some() || engine_b_path.is_some() {
            eprintln!(
                "configuration error: positional engine paths must be omitted with --apply; use --apply-executable to relocate the recorded executable"
            );
            return ExitCode::from(2);
        }
        if match_engine_overrides_requested(&command) {
            eprintln!(
                "configuration error: per-side labels, process controls, UCI options and cores are not allowed with --apply; the gate arms come from the SPSA artifact"
            );
            return ExitCode::from(2);
        }
        match load_spsa_apply(
            &apply_path,
            apply_executable.as_deref(),
            allow_executable_mismatch,
        ) {
            Ok(resolved) => resolved,
            Err(error) => {
                eprintln!("configuration error: {error}");
                return ExitCode::from(2);
            }
        }
    } else {
        let (Some(engine_a_path), Some(engine_b_path)) = (engine_a_path, engine_b_path) else {
            eprintln!(
                "configuration error: SPRT requires two engine paths or --apply <SPSA result.json>"
            );
            return ExitCode::from(2);
        };
        let engine_a = match resolve_match_engine(
            engine_a_path,
            command.a_label.clone(),
            command.a_arguments.clone(),
            command.a_cwd.clone(),
            command.a_environment.clone(),
            command.a_options.clone(),
            command.a_buttons.clone(),
            command.a_cores.clone(),
        ) {
            Ok(engine) => engine,
            Err(error) => {
                eprintln!("configuration error: {error}");
                return ExitCode::from(2);
            }
        };
        let engine_b = match resolve_match_engine(
            engine_b_path,
            command.b_label.clone(),
            command.b_arguments.clone(),
            command.b_cwd.clone(),
            command.b_environment.clone(),
            command.b_options.clone(),
            command.b_buttons.clone(),
            command.b_cores.clone(),
        ) {
            Ok(engine) => engine,
            Err(error) => {
                eprintln!("configuration error: {error}");
                return ExitCode::from(2);
            }
        };
        (engine_a, engine_b, None)
    };
    if let Err(error) = configure_ponder(&mut engine_a, command.ponder)
        .and_then(|()| configure_ponder(&mut engine_b, command.ponder))
    {
        eprintln!("configuration error: {error}");
        return ExitCode::from(2);
    }
    if apply_record
        .as_ref()
        .is_some_and(|record| record.identity.status == SpsaGateHashStatus::MismatchOverridden)
    {
        let record = apply_record.as_ref().expect("record was just matched");
        eprintln!(
            "WARNING: SPSA apply executable SHA-256 mismatch explicitly overridden (expected {}, actual {})",
            record.identity.expected_sha256, record.identity.actual_sha256
        );
    }
    let placement_policy = match resolve_placement(&command.placement, command.headroom_cores) {
        Ok(policy) => policy,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let execution = match match_runner::plan_execution(
        &engine_a,
        &engine_b,
        command.concurrency as usize,
        resolve_slot_allocation(command.cores_per_game, command.cores_per_engine),
        placement_policy,
        command.memory_budget_mb,
    ) {
        Ok(execution) => execution,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let book = command.book.clone().map(|path| {
        let mut book = OpeningBook::new(path);
        book.order = match command.book_order {
            BookOrderArg::Sequential => OpeningOrder::Sequential,
            BookOrderArg::Random => OpeningOrder::Random,
        };
        book.plies = command.book_plies.unwrap_or(8);
        book
    });
    let games = match design.max_pairs.checked_mul(2) {
        Some(games) => games,
        None => {
            eprintln!("configuration error: max-pairs is too large to schedule");
            return ExitCode::from(2);
        }
    };
    let openings = match match_runner::resolve_openings(
        book,
        command.book_start,
        games.div_ceil(2),
        command.book_wrap,
        master_seed,
    ) {
        Ok(openings) => openings,
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
    let resolved = match resolve_config(
        built_in_defaults(),
        None,
        json!({
            "command": "sprt",
            "design": design,
            "engine_a": &engine_a,
            "engine_b": &engine_b,
            "apply": &apply_record,
            "engine_a_time_control": engine_a_time_control,
            "engine_b_time_control": engine_b_time_control,
            "adjudication": adjudication,
            "ponder": command.ponder,
        "engine_processes": command.engine_processes,
            "fault_policy": fault_policy,
            "execution": execution,
            "master_seed": master_seed,
            "master_seed_generated": master_seed_generated,
            "openings": openings.report(),
        }),
        &[],
        &current_directory,
        &match command.book {
            Some(_) => vec![
                "/engine_a/executable".into(),
                "/engine_b/executable".into(),
                "/openings/path".into(),
            ],
            None => vec!["/engine_a/executable".into(), "/engine_b/executable".into()],
        },
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
                command: "sprt",
                config_sha256: resolved.sha256(),
                resolved_configuration: resolved.value(),
                invocations: vec![&engine_a, &engine_b],
                wave_shape: None,
            },
            machine,
        );
        return ExitCode::SUCCESS;
    }

    let opened = match &command.run_directory {
        Some(path) => RunDirectory::open_explicit(path, &resolved, command.restart),
        None => RunDirectory::create_unique(&current_directory, "sprt", &resolved),
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
    let journal = match open_journal(&directory, opened.resumed).await {
        Ok(journal) => journal,
        Err(error) => {
            eprintln!("resume failed: {error}");
            return ExitCode::from(3);
        }
    };
    if journal
        .records
        .iter()
        .any(|record| record.sample == POST_TERMINAL_SAMPLE)
    {
        eprintln!("resume failed: a terminal SPRT run cannot be extended");
        return ExitCode::from(2);
    }
    // The official prefix is whatever the journal's pairs replay to. The
    // schedule admits them in pair order and refuses a prefix that crossed a
    // boundary it did not record, so nothing about it is taken on trust.
    let official_pairs = pairs_from_journal(&journal.records, OFFICIAL_SAMPLE);
    let clock = journal.clock();
    let writer = match RunWriter::start(Arc::clone(&directory), journal.resume).await {
        Ok(writer) => writer.on_clock(clock),
        Err(error) => {
            eprintln!("SPRT output failed: {error}");
            return ExitCode::from(3);
        }
    };
    let observer = Arc::new(DurableSprtOutput::new(writer.clone(), &official_pairs));
    colosseum_engine::incidents::set_dir(directory.paths().root.join("failed-games"));
    let mut recorder = match if opened.resumed {
        RunRecorder::resume(&directory)
    } else {
        RunRecorder::begin(&directory, "sprt")
    } {
        Ok(recorder) => recorder,
        Err(error) => {
            eprintln!("run record failed: {error}");
            return ExitCode::from(3);
        }
    };
    recorder.write_through(writer.clone());
    if let Err(error) = recorder.set_workflow(json!({
        "kind": "sprt",
        "progress": {"every": progress_every, "unit": "pairs", "min_secs": progress_min_secs},
        "pgn_annotation_writer": colosseum_engine::pgn::PGN_ANNOTATION_WRITER,
        "design": design,
        "apply": &apply_record,
        "engine_a_time_control": engine_a_time_control,
        "engine_b_time_control": engine_b_time_control,
        "adjudication": adjudication,
        "ponder": command.ponder,
        "engine_processes": command.engine_processes,
        "fault_policy": fault_policy,
        "execution": execution,
        "master_seed": master_seed,
        "master_seed_generated": master_seed_generated,
        "openings": openings.report(),
    })) {
        eprintln!("run record failed: {error}");
        return ExitCode::from(3);
    }
    if !machine {
        eprintln!("SPRT run directory: {}", directory.paths().root.display());
        eprintln!("SPRT test: {}", sprt_test_line(design));
    }
    let openings_report = openings.report().clone();
    let resumed_pairs = official_pairs.len() as u64;
    let players = Players::new(&engine_a, &engine_b);
    let kept_engines =
        match_runner::SlotEngines::for_mode(command.engine_processes, execution.slots.len());
    let request = sprt_runner::PairScheduleRequest {
        settings: match_runner::PairGameSettings {
            engine_a,
            engine_b,
            engine_a_time_control,
            engine_b_time_control,
            adjudication,
            ponder: command.ponder,
            openings,
            synthetic_games: false,
            engines: kept_engines.clone(),
        },
        execution: execution.clone(),
        design,
        fault_policy,
        completed_pairs: official_pairs,
        cancellation: cancellation.clone(),
        observer: Some(observer.clone()),
    };
    let schedule_future = sprt_runner::run_pair_schedule(request);
    tokio::pin!(schedule_future);
    let mut progress =
        ProgressSchedule::new(progress_every, progress_min_secs, resumed_pairs).on_clock(clock);
    let mut poll =
        tokio::time::interval_at(tokio::time::Instant::now() + PROGRESS_POLL, PROGRESS_POLL);
    let outcome = loop {
        tokio::select! {
            result = &mut schedule_future => break result,
            _ = poll.tick() => {
                if progress.due(observer.units()) {
                    let (sample, post_terminal) = observer.sample();
                    let block = sprt_progress_block(
                        &sample,
                        post_terminal,
                        &progress,
                        design,
                        &players,
                        false,
                        fault_policy,
                    );
                    publish_progress(&block, &writer, &mut recorder);
                }
            }
        }
    };
    if let Some(engines) = &kept_engines {
        engines.shutdown().await;
    }
    let (sample, post_terminal) = observer.sample();
    let final_block = sprt_progress_block(
        &sample,
        post_terminal,
        &progress,
        design,
        &players,
        true,
        fault_policy,
    );
    if progress.needs_final(final_block.done) {
        progress.mark(final_block.done);
        publish_progress(&final_block, &writer, &mut recorder);
    }
    match outcome {
        Ok(schedule) => {
            let status = schedule.status();
            let report = sprt_runner::SprtReport {
                status,
                design,
                engine_a_time_control,
                engine_b_time_control,
                adjudication,
                fault_policy,
                execution,
                master_seed,
                master_seed_generated,
                openings: openings_report,
                apply: apply_record,
                schedule,
            };
            if let Err(error) = observer.finish(&report) {
                eprintln!("SPRT output failed: {error}");
                return ExitCode::from(3);
            }
            let sample = OfficialSample {
                committed_units: report.schedule.official_pairs.len() as u64,
                scored_games: (report.schedule.official_pairs.len() * 2) as u64,
                completed_pairs: report.schedule.official_pairs.len() as u64,
                pentanomial: report.schedule.pentanomial.map(u64::from),
                unpaired_games: 0,
            };
            if let Err(error) = recorder.update_sample(sample) {
                eprintln!("run record failed: {error}");
                return ExitCode::from(3);
            }
            let run_status = match status {
                sprt_runner::SprtStatus::H1
                | sprt_runner::SprtStatus::H0
                | sprt_runner::SprtStatus::Inconclusive => RunStatus::Completed,
                sprt_runner::SprtStatus::Cancelled => RunStatus::Cancelled,
                sprt_runner::SprtStatus::Invalid => RunStatus::Invalid,
            };
            if let Err(error) = recorder.finish(run_status) {
                eprintln!("run record failed: {error}");
                return ExitCode::from(3);
            }
            if let Err(error) = settle(&writer).await {
                eprintln!("SPRT output failed: {error}");
                return ExitCode::from(3);
            }
            if machine {
                print_json(&MachineOutput::Sprt {
                    run_directory: directory.paths().root.clone(),
                    report,
                });
            } else {
                print_sprt(&report, &directory.paths().root, progress.run_elapsed());
            }
            ExitCode::from(sprt_exit_code(status))
        }
        Err(error) => {
            eprintln!("SPRT failed: {error}");
            drop(recorder);
            let _ = settle(&writer).await;
            ExitCode::from(3)
        }
    }
}

pub(crate) fn sprt_exit_code(status: sprt_runner::SprtStatus) -> u8 {
    match status {
        sprt_runner::SprtStatus::H1 => 0,
        sprt_runner::SprtStatus::H0 => 1,
        sprt_runner::SprtStatus::Inconclusive => 4,
        sprt_runner::SprtStatus::Invalid => 5,
        sprt_runner::SprtStatus::Cancelled => CANCELLED_EXIT_CODE,
    }
}

pub(crate) fn resolve_sprt_design(command: &SprtCommand) -> Result<SprtDesign, String> {
    let bundle = command.preset.map(Into::into);
    let defaults = bundle.map(SprtBundle::defaults);
    let required = |value: Option<f64>, name: &str| {
        value
            .or_else(|| {
                defaults.map(|parameters| match name {
                    "elo0" => parameters.elo0,
                    "elo1" => parameters.elo1,
                    "alpha" => parameters.alpha,
                    "beta" => parameters.beta,
                    _ => unreachable!("known SPRT scalar"),
                })
            })
            .ok_or_else(|| format!("--{name} is required without --preset"))
    };
    let model = command
        .model
        .map(Into::into)
        .or_else(|| defaults.map(|parameters| parameters.model))
        .ok_or_else(|| "--model is required without --preset".to_owned())?;
    let parameters = SprtParameters {
        model,
        elo0: required(command.elo0, "elo0")?,
        elo1: required(command.elo1, "elo1")?,
        alpha: required(command.alpha, "alpha")?,
        beta: required(command.beta, "beta")?,
    };
    SprtDesign::new(parameters, command.max_pairs, bundle).map_err(|error| error.to_string())
}

/// What a sequential test tells the operator: the sample, what it is worth in
/// both Elo models, how far the LLR is from its bounds, and how much longer it
/// would run if the evidence kept arriving at the rate it has.
///
/// `sample` is the official sample committed so far and `post_terminal` the
/// pairs kept after the boundary, as [`DurableSprtOutput::sample`] reports them.
pub(crate) fn sprt_progress_block(
    sample: &PairedProgress,
    post_terminal: usize,
    progress: &ProgressSchedule,
    design: SprtDesign,
    players: &Players,
    terminal: bool,
    policy: FaultPolicy,
) -> ProgressBlock {
    let done = u64::from(sample.pairs);
    let mut block = ProgressBlock::new(
        "sprt",
        ProgressUnit::Pairs,
        done,
        Some(u64::from(design.max_pairs)),
        progress.run_elapsed(),
    );
    block.field("players", players.to_string());
    block.field("test", sprt_test_line(design));
    sample.add_fields(&mut block, policy);
    if post_terminal > 0 {
        block.field(
            "post-terminal",
            format!(
                "{} kept as evidence the sample excludes",
                progress::plural(post_terminal as u64, "pair")
            ),
        );
    }
    let parameters = design.parameters;
    let statistics = pentanomial_sprt(
        &sample.vector,
        parameters.model,
        parameters.elo0,
        parameters.elo1,
        parameters.alpha,
        parameters.beta,
    );
    match &statistics {
        Ok(result) => {
            block.field(
                "LLR",
                format!(
                    "{:+.2} in [{:.2}, {:.2}] ({})",
                    result.llr,
                    result.lower,
                    result.upper,
                    match result.decision {
                        SprtDecision::Continue => "continue",
                        SprtDecision::AcceptH0 => "accept H0",
                        SprtDecision::AcceptH1 => "accept H1",
                    }
                ),
            );
        }
        Err(error) => {
            block.field("LLR", format!("unavailable: {error}"));
        }
    }
    // Pairs are this command's unit of work, but throughput is reported in
    // games, as every other command reports it, so two runs can be compared
    // directly. A committed pair is exactly two games.
    if let Some(rate) = progress::rate_per_hour(
        progress.units_since_start(done).saturating_mul(2),
        progress.session_hours(),
    ) {
        block.field("rate", format!("{rate:.0} games/hour"));
    }
    // A test that has stopped has nothing left to run, whatever the cap says.
    // Otherwise the estimate is the pairs the LLR would need at its current
    // drift, and never more than the pairs the cap still allows.
    let to_cap = u64::from(design.max_pairs).saturating_sub(done);
    // An LLR past its bound has decided the test even before the driver has
    // stopped it; without this its estimate fell back to the whole cap.
    let decided = terminal
        || statistics
            .as_ref()
            .is_ok_and(|result| result.decision != SprtDecision::Continue);
    let remaining = remaining_pairs(statistics.as_ref().ok(), to_cap, decided);
    block.field(
        "time remaining",
        match progress::time_for_units(
            progress.units_since_start(done),
            progress.session_elapsed(),
            remaining,
        ) {
            // The estimate assumes the LLR keeps moving as it has so far.
            Some(left) if decided => progress::format_duration(left.as_secs_f64()),
            Some(left) => format!(
                "{} if the trend holds",
                progress::format_duration(left.as_secs_f64())
            ),
            None => "unknown".to_owned(),
        },
    );
    block
}

/// Pairs until the LLR would reach the bound it is drifting towards.
///
/// This extrapolates the LLR the test already computed at its average rate per
/// pair. It is arithmetic on the existing statistic and not a second estimator:
/// a real sequential test's path is not a straight line, and the figure is
/// labelled as the current drift for that reason.
/// Pairs an SPRT still has to play: none once decided, otherwise the pairs its
/// LLR needs at its current drift, never more than the cap still allows.
fn remaining_pairs(result: Option<&PentanomialSprtResult>, to_cap: u64, decided: bool) -> u64 {
    if decided {
        return 0;
    }
    result
        .and_then(pairs_to_bound)
        .map_or(to_cap, |pairs| pairs.min(to_cap))
}

fn pairs_to_bound(result: &PentanomialSprtResult) -> Option<u64> {
    if result.pairs == 0 || !result.llr.is_finite() {
        return None;
    }
    let drift = result.llr / f64::from(result.pairs);
    let target = if result.llr > 0.0 {
        result.upper
    } else {
        result.lower
    };
    let remaining = (target - result.llr) / drift;
    (drift != 0.0 && remaining.is_finite() && remaining > 0.0).then(|| remaining.ceil() as u64)
}

/// What a sequential test writes while it runs: each admitted pair as two
/// journal lines and two appended PGN games, and a checkpoint of the official
/// sample it has reached. The pairs themselves are the driver's to keep.
pub(crate) struct DurableSprtOutput {
    pub(crate) writer: RunWriter,
    state: Mutex<SprtAggregate>,
}

struct SprtAggregate {
    sample: PairedProgress,
    post_terminal: usize,
    cadence: CheckpointCadence,
}

impl SprtAggregate {
    fn checkpoint(&self) -> Value {
        json!({
            "command": "sprt",
            "official_pairs": self.sample.pairs,
            "post_terminal_pairs": self.post_terminal,
            "wins": self.sample.wins,
            "draws": self.sample.draws,
            "losses": self.sample.losses,
            "pentanomial": self.sample.vector.counts(),
            "faults": self.sample.faults,
        })
    }
}

impl DurableSprtOutput {
    /// Start from the official pairs a resume found in the journal.
    pub(crate) fn new(
        writer: RunWriter,
        official_pairs: &[CompletePair<match_runner::MatchGame>],
    ) -> Self {
        Self {
            writer,
            state: Mutex::new(SprtAggregate {
                sample: PairedProgress::from_pairs(official_pairs),
                post_terminal: 0,
                cadence: CheckpointCadence::new(),
            }),
        }
    }

    /// Official pairs, cheaply, for deciding whether a block is due.
    pub(crate) fn units(&self) -> u64 {
        self.state
            .lock()
            .map_or(0, |state| u64::from(state.sample.pairs))
    }

    /// The official sample and the post-terminal count, for a progress block.
    pub(crate) fn sample(&self) -> (PairedProgress, usize) {
        self.state.lock().map_or_else(
            |_| (PairedProgress::default(), 0),
            |state| (state.sample.clone(), state.post_terminal),
        )
    }

    fn commit_pair(
        &self,
        pair: &CompletePair<match_runner::MatchGame>,
        class: &'static str,
    ) -> Result<(), String> {
        for game in [&pair.first, &pair.second] {
            let (record, sample) = paired_game_entry(game, class, None);
            log_fault(&self.writer, &record);
            let moves = with_header_tags(game.pgn.trim_end(), &[("ColosseumSample", sample)]);
            self.writer.game(record, moves)?;
        }
        let mut state = self.state.lock().map_err(|_| "SPRT state lock poisoned")?;
        if class == OFFICIAL_SAMPLE {
            state.sample.add_pair(pair);
        } else {
            state.post_terminal += 1;
        }
        if state.cadence.record(1) {
            self.writer.checkpoint(state.checkpoint())?;
        }
        Ok(())
    }

    pub(crate) fn finish(&self, report: &sprt_runner::SprtReport) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "SPRT state lock poisoned")?;
        self.writer.checkpoint(state.checkpoint())?;
        drop(state);
        self.writer.replace(
            self.writer.root().join("result.json"),
            serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?,
        )?;
        log_event(
            &self.writer,
            &json!({
                "event": "sprt-finished",
                "status": report.status,
                "official_pairs": report.schedule.official_pairs.len(),
                "post_terminal_pairs": report.schedule.post_terminal_pairs.len(),
            }),
        );
        Ok(())
    }
}

impl sprt_runner::PairObserver for DurableSprtOutput {
    fn official_pair(&self, pair: &CompletePair<match_runner::MatchGame>) -> Result<(), String> {
        self.commit_pair(pair, OFFICIAL_SAMPLE)
    }

    fn post_terminal_pair(
        &self,
        pair: &CompletePair<match_runner::MatchGame>,
    ) -> Result<(), String> {
        self.commit_pair(pair, POST_TERMINAL_SAMPLE)
    }
}

/// The Elo model as a reader names it rather than as the enum spells it.
/// What an SPRT tests, in one line: the Elo bounds of its two hypotheses in
/// the model they are measured in, and the error rates. A named preset is
/// named, and said to be overridden when any value differs from it, so the
/// line never passes off an edited design as the preset.
pub(crate) fn sprt_test_line(design: SprtDesign) -> String {
    let parameters = design.parameters;
    let scale = match parameters.model {
        EloModel::Normalized => "nElo",
        EloModel::Logistic => "Elo",
    };
    let mut line = format!(
        "{scale} [{:.2}, {:.2}], alpha {}, beta {}",
        parameters.elo0, parameters.elo1, parameters.alpha, parameters.beta
    );
    if let Some(bundle) = design.bundle {
        let name = match bundle {
            SprtBundle::Gainer => "gainer",
            SprtBundle::Simplify => "simplify",
        };
        if parameters == bundle.defaults() {
            line.push_str(&format!(" (preset {name})"));
        } else {
            line.push_str(&format!(" (preset {name}, overridden)"));
        }
    }
    line
}

fn elo_model_name(model: EloModel) -> &'static str {
    match model {
        EloModel::Normalized => "normalized",
        EloModel::Logistic => "logistic",
    }
}

/// The verdict in a sentence, naming what was accepted rather than only which
/// hypothesis it was.
fn sprt_verdict(report: &sprt_runner::SprtReport) -> String {
    let parameters = report.design.parameters;
    let model = format!("{} Elo", elo_model_name(parameters.model));
    match report.status {
        sprt_runner::SprtStatus::H1 => format!(
            "completed - H1 accepted: the gain is at least {:.2} {model}",
            parameters.elo1
        ),
        sprt_runner::SprtStatus::H0 => format!(
            "completed - H0 accepted: the gain is no more than {:.2} {model}",
            parameters.elo0
        ),
        sprt_runner::SprtStatus::Inconclusive => format!(
            "inconclusive - the {} cap was reached without a boundary",
            progress::plural(u64::from(report.design.max_pairs), "pair")
        ),
        sprt_runner::SprtStatus::Cancelled => {
            "cancelled - stopped cleanly before a boundary or the cap".to_owned()
        }
        sprt_runner::SprtStatus::Invalid => {
            "invalid - the engine or time fault policy was exceeded".to_owned()
        }
    }
}

pub(crate) fn print_sprt(
    report: &sprt_runner::SprtReport,
    run_directory: &Path,
    elapsed: Duration,
) {
    if let Some(apply) = &report.apply {
        println!(
            "SPSA apply: {} ({:?})",
            apply.source_result.display(),
            apply.identity.status
        );
    }
    println!(
        "SPRT [{:.2}, {:.2}] {}",
        report.design.parameters.elo0,
        report.design.parameters.elo1,
        sprt_verdict(report)
    );
    println!(
        "official sample: {}; post-terminal: {}",
        progress::plural(report.schedule.official_pairs.len() as u64, "pair"),
        progress::plural(report.schedule.post_terminal_pairs.len() as u64, "pair")
    );
    let faults = report.schedule.faults;
    println!(
        "faults: {}; {}",
        fault_counts_text(faults),
        fault_allowance_text(
            report.fault_policy,
            faults,
            report.schedule.official_pairs.len() as u64 * 2
        )
    );
    println!(
        "model {}: alpha {}, beta {}, cap {}",
        elo_model_name(report.design.parameters.model),
        report.design.parameters.alpha,
        report.design.parameters.beta,
        progress::plural(u64::from(report.design.max_pairs), "pair")
    );
    if let Some(statistics) = report.schedule.statistics {
        println!(
            "LLR {:.6} in [{:.6}, {:.6}]",
            statistics.llr, statistics.lower, statistics.upper
        );
    } else {
        println!("LLR unavailable: official sample is still statistically degenerate");
    }
    println!(
        "terminal pair: {}; invalid pair: {}",
        report
            .schedule
            .terminal_pair
            .map_or_else(|| "none".into(), |value| value.to_string()),
        report
            .schedule
            .invalid_pair
            .map_or_else(|| "none".into(), |value| value.to_string())
    );
    println!("artifacts: {}", run_directory.display());
    println!("Finished match");
    println!(
        "Total Time: {}",
        progress::format_duration(elapsed.as_secs_f64())
    );
}

#[cfg(test)]
mod remaining_tests {
    use super::*;

    #[test]
    fn a_test_whose_llr_has_crossed_its_bound_has_nothing_left_to_run() {
        // 241 pairs of a clear gain, as the qualification verdict had when
        // its last block still estimated the whole cap.
        let mut vector = PentanomialVector::default();
        for _ in 0..120 {
            vector.record_pair(PairGameResult::Win, PairGameResult::Draw);
            vector.record_pair(PairGameResult::Draw, PairGameResult::Draw);
        }
        vector.record_pair(PairGameResult::Win, PairGameResult::Win);
        let result = pentanomial_sprt(&vector, EloModel::Normalized, 0.0, 10.0, 0.05, 0.05)
            .expect("statistics for a non-empty sample");
        assert_eq!(result.decision, SprtDecision::AcceptH1);
        // Past the bound the drift estimate has no pairs left to give, which
        // the estimate used to read as "unknown" and replace with the cap.
        assert_eq!(pairs_to_bound(&result), None);
        assert_eq!(remaining_pairs(Some(&result), 7_759, false), 7_759);
        assert_eq!(remaining_pairs(Some(&result), 7_759, true), 0);
    }
}

#[cfg(test)]
mod progress_block_tests {
    use std::time::Instant;

    use super::*;

    /// Ten drawn pairs and ten won pairs: a sample with variance that stays
    /// inside the Wald bounds of a [0, 10] normalized test.
    fn sample() -> PairedProgress {
        let mut sample = PairedProgress::default();
        for _ in 0..10 {
            sample
                .vector
                .record_pair(PairGameResult::Draw, PairGameResult::Draw);
            sample
                .vector
                .record_pair(PairGameResult::Win, PairGameResult::Win);
        }
        sample.pairs = 20;
        sample.scored_games = 40;
        sample.wins = 20;
        sample.draws = 20;
        sample
    }

    fn design(max_pairs: u32) -> SprtDesign {
        let parameters = SprtParameters {
            model: EloModel::Normalized,
            elo0: 0.0,
            elo1: 10.0,
            alpha: 0.05,
            beta: 0.05,
        };
        SprtDesign::new(parameters, max_pairs, None).unwrap()
    }

    fn players() -> Players {
        Players {
            a: "Challenger 2".into(),
            b: "Baseline 1".into(),
        }
    }

    /// A schedule that started a minute ago, so a rate has time to divide by.
    fn schedule() -> ProgressSchedule {
        let started = Instant::now()
            .checked_sub(Duration::from_secs(60))
            .expect("a host that has been up for a minute");
        ProgressSchedule::started_at(10, 1, 0, started)
    }

    #[test]
    fn the_test_line_names_the_bounds_in_their_model_and_an_edited_preset_as_such() {
        let gainer = SprtBundle::Gainer.defaults();
        let preset = SprtDesign::new(gainer, 100, Some(SprtBundle::Gainer)).unwrap();
        assert_eq!(
            sprt_test_line(preset),
            "nElo [0.00, 5.00], alpha 0.05, beta 0.05 (preset gainer)"
        );

        let narrowed = SprtParameters {
            elo1: 3.0,
            ..gainer
        };
        let edited = SprtDesign::new(narrowed, 100, Some(SprtBundle::Gainer)).unwrap();
        assert_eq!(
            sprt_test_line(edited),
            "nElo [0.00, 3.00], alpha 0.05, beta 0.05 (preset gainer, overridden)"
        );

        let logistic = SprtParameters {
            model: EloModel::Logistic,
            elo0: -3.0,
            elo1: 1.0,
            alpha: 0.1,
            beta: 0.05,
        };
        let custom = SprtDesign::new(logistic, 100, None).unwrap();
        assert_eq!(
            sprt_test_line(custom),
            "Elo [-3.00, 1.00], alpha 0.1, beta 0.05"
        );
    }

    fn field(block: &ProgressBlock, label: &str) -> String {
        block
            .fields
            .iter()
            .find(|field| field.label == label)
            .unwrap_or_else(|| panic!("no {label} in:\n{}", block.render()))
            .value
            .clone()
    }

    #[test]
    fn an_sprt_block_carries_the_sample_both_models_and_the_llr() {
        let block = sprt_progress_block(
            &sample(),
            0,
            &schedule(),
            design(20),
            &players(),
            true,
            FaultPolicy::default(),
        );
        let text = block.render();
        assert!(
            text.starts_with("progress [sprt]: 20/20 pairs (100%), "),
            "{text}"
        );
        let labels = block
            .fields
            .iter()
            .map(|field| field.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                "players",
                "test",
                "games",
                "Elo",
                "nElo",
                "W/D/L",
                "Ptnml",
                "faults",
                "LLR",
                "rate",
                "time remaining"
            ],
            "{text}"
        );
        assert_eq!(field(&block, "players"), "Challenger 2 vs. Baseline 1");
        assert_eq!(
            field(&block, "test"),
            "nElo [0.00, 10.00], alpha 0.05, beta 0.05"
        );
        assert_eq!(field(&block, "games"), "40");
        assert_eq!(field(&block, "W/D/L"), "20/20/0");
        // The pentanomial is the committed vector, and the LLR is stated
        // against its exact Wald bounds.
        assert_eq!(field(&block, "Ptnml"), "[0, 0, 10, 0, 10]");
        assert!(
            field(&block, "LLR").contains("in [-2.94, 2.94] (continue)"),
            "{text}"
        );
        // Both estimates read as a value and a margin.
        for model in ["Elo", "nElo"] {
            assert!(field(&block, model).contains(" +/- "), "{text}");
        }
        // Throughput is in games, as every command reports it, although this
        // command's unit of work is the pair.
        assert!(field(&block, "rate").ends_with(" games/hour"), "{text}");
        // A terminated test has nothing left to run.
        assert_eq!(field(&block, "time remaining"), "0s");
    }

    #[test]
    fn a_running_sprt_names_its_post_terminal_pairs_and_the_trend_it_extrapolates() {
        let block = sprt_progress_block(
            &sample(),
            2,
            &schedule(),
            design(1_000),
            &players(),
            false,
            FaultPolicy::default(),
        );
        assert_eq!(
            field(&block, "post-terminal"),
            "2 pairs kept as evidence the sample excludes"
        );
        let remaining = field(&block, "time remaining");
        assert!(remaining.ends_with(" if the trend holds"), "{remaining}");
    }
}
