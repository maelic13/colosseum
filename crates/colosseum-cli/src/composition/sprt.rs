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
    let mut source: SpsaApplySource = serde_json::from_slice(&bytes).map_err(|error| {
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
    let fault_policy = match_runner::FaultPolicy {
        max_engine_faults: command.max_engine_faults,
        max_time_losses: command.max_time_losses,
    };
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
    let checkpoint = if opened.resumed
        && (directory.paths().checkpoint.exists() || directory.paths().previous_checkpoint.exists())
    {
        match directory.read_checkpoint::<SprtCheckpoint>() {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                eprintln!("resume failed: {error}");
                return ExitCode::from(3);
            }
        }
    } else {
        SprtCheckpoint::default()
    };
    if !checkpoint.post_terminal_pairs.is_empty() {
        eprintln!("resume failed: a terminal SPRT run cannot be extended");
        return ExitCode::from(2);
    }
    let observer = match DurableSprtOutput::new(Arc::clone(&directory), checkpoint.clone()) {
        Ok(observer) => Arc::new(observer),
        Err(error) => {
            eprintln!("SPRT output failed: {error}");
            return ExitCode::from(3);
        }
    };
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
    if let Err(error) = recorder.set_workflow(json!({
        "kind": "sprt",
        "pgn_annotation_writer": colosseum_engine::pgn::PGN_ANNOTATION_WRITER,
        "design": design,
        "apply": &apply_record,
        "engine_a_time_control": engine_a_time_control,
        "engine_b_time_control": engine_b_time_control,
        "adjudication": adjudication,
        "ponder": command.ponder,
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
    }
    let openings_report = openings.report().clone();
    let request = sprt_runner::PairScheduleRequest {
        settings: match_runner::PairGameSettings {
            engine_a,
            engine_b,
            engine_a_time_control,
            engine_b_time_control,
            adjudication,
            ponder: command.ponder,
            openings,
        },
        execution: execution.clone(),
        design,
        fault_policy,
        completed_pairs: checkpoint.official_pairs,
        cancellation: cancellation.clone(),
        observer: Some(observer.clone()),
    };
    let schedule_future = sprt_runner::run_pair_schedule(request);
    tokio::pin!(schedule_future);
    let period = Duration::from_secs(command.progress_interval_secs);
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    let outcome = loop {
        tokio::select! {
            result = &mut schedule_future => break result,
            _ = interval.tick() => {
                let (official, post_terminal) = observer.progress();
                eprintln!(
                    "SPRT progress: {official}/{} official pairs, {post_terminal} post-terminal",
                    design.max_pairs
                );
            }
        }
    };
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
            if machine {
                print_json(&MachineOutput::Sprt {
                    run_directory: directory.paths().root.clone(),
                    report,
                });
            } else {
                print_sprt(&report, &directory.paths().root);
            }
            ExitCode::from(sprt_exit_code(status))
        }
        Err(error) => {
            eprintln!("SPRT failed: {error}");
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct SprtCheckpoint {
    pub(crate) official_pairs: Vec<CompletePair<match_runner::MatchGame>>,
    pub(crate) post_terminal_pairs: Vec<CompletePair<match_runner::MatchGame>>,
}

pub(crate) struct DurableSprtOutput {
    pub(crate) directory: Arc<RunDirectory>,
    pub(crate) checkpoint: Mutex<SprtCheckpoint>,
}

impl DurableSprtOutput {
    pub(crate) fn new(
        directory: Arc<RunDirectory>,
        checkpoint: SprtCheckpoint,
    ) -> Result<Self, String> {
        let output = Self {
            directory,
            checkpoint: Mutex::new(checkpoint),
        };
        output.rewrite_pgn()?;
        Ok(output)
    }

    pub(crate) fn progress(&self) -> (usize, usize) {
        self.checkpoint.lock().map_or((0, 0), |checkpoint| {
            (
                checkpoint.official_pairs.len(),
                checkpoint.post_terminal_pairs.len(),
            )
        })
    }

    pub(crate) fn persist_pair(
        &self,
        pair: &CompletePair<match_runner::MatchGame>,
        official: bool,
    ) -> Result<(), String> {
        {
            let mut checkpoint = self
                .checkpoint
                .lock()
                .map_err(|_| "SPRT checkpoint lock poisoned")?;
            if official {
                checkpoint.official_pairs.push(pair.clone());
            } else {
                checkpoint.post_terminal_pairs.push(pair.clone());
            }
            self.directory
                .write_checkpoint(&*checkpoint)
                .map_err(|error| error.to_string())?;
        }
        self.rewrite_pgn()?;
        let mut line = serde_json::to_vec(&json!({
            "event": if official { "official-pair" } else { "post-terminal-pair" },
            "pair": pair,
        }))
        .map_err(|error| error.to_string())?;
        line.push(b'\n');
        self.directory
            .append_log(&line)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn rewrite_pgn(&self) -> Result<(), String> {
        let checkpoint = self
            .checkpoint
            .lock()
            .map_err(|_| "SPRT PGN lock poisoned")?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.directory.paths().root.join("games.pgn"))
            .map_err(|error| error.to_string())?;
        for (class, pairs) in [
            ("official", &checkpoint.official_pairs),
            ("post-terminal", &checkpoint.post_terminal_pairs),
        ] {
            for pair in pairs {
                for game in [&pair.first, &pair.second] {
                    let tagged =
                        with_header_tags(game.pgn.trim_end(), &[("ColosseumSample", class)]);
                    writeln!(file, "{tagged}\n").map_err(|error| error.to_string())?;
                }
            }
        }
        file.sync_all().map_err(|error| error.to_string())
    }

    pub(crate) fn finish(&self, report: &sprt_runner::SprtReport) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
        fs::write(self.directory.paths().root.join("result.json"), bytes)
            .map_err(|error| error.to_string())?;
        let mut line = serde_json::to_vec(&json!({
            "event": "sprt-finished",
            "status": report.status,
            "official_pairs": report.schedule.official_pairs.len(),
            "post_terminal_pairs": report.schedule.post_terminal_pairs.len(),
        }))
        .map_err(|error| error.to_string())?;
        line.push(b'\n');
        self.directory
            .append_log(&line)
            .map_err(|error| error.to_string())
    }
}

impl sprt_runner::PairObserver for DurableSprtOutput {
    fn official_pair(&self, pair: &CompletePair<match_runner::MatchGame>) -> Result<(), String> {
        self.persist_pair(pair, true)
    }

    fn post_terminal_pair(
        &self,
        pair: &CompletePair<match_runner::MatchGame>,
    ) -> Result<(), String> {
        self.persist_pair(pair, false)
    }
}

pub(crate) fn print_sprt(report: &sprt_runner::SprtReport, run_directory: &Path) {
    if let Some(apply) = &report.apply {
        println!(
            "SPSA apply: {} ({:?})",
            apply.source_result.display(),
            apply.identity.status
        );
    }
    println!(
        "SPRT {:?}: {} official pairs, {} post-terminal pairs",
        report.status,
        report.schedule.official_pairs.len(),
        report.schedule.post_terminal_pairs.len()
    );
    println!(
        "model {:?}: H0 {} / H1 {}, alpha {}, beta {}, cap {} pairs",
        report.design.parameters.model,
        report.design.parameters.elo0,
        report.design.parameters.elo1,
        report.design.parameters.alpha,
        report.design.parameters.beta,
        report.design.max_pairs
    );
    if let Some(statistics) = report.schedule.statistics {
        println!(
            "LLR {:.6}; bounds [{:.6}, {:.6}]; decision {:?}",
            statistics.llr, statistics.lower, statistics.upper, statistics.decision
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
}
