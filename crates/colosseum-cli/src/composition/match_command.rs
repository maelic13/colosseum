//! The `match` command: a fixed number of games, no sequential stopping.

use super::*;

#[derive(Debug, Args)]
pub(crate) struct MatchCommand {
    /// Path to engine A's UCI executable.
    pub(crate) engine_a: PathBuf,
    /// Path to engine B's UCI executable.
    pub(crate) engine_b: PathBuf,

    /// Exact number of games to play; this command never stops early.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    pub(crate) games: u32,

    #[command(flatten)]
    pub(crate) conditions: MatchConditions,
}

pub(crate) async fn run_match(
    command: MatchCommand,
    machine: bool,
    dry_run: bool,
    cancellation: Cancellation,
) -> ExitCode {
    let MatchCommand {
        games,
        engine_a: engine_a_path,
        engine_b: engine_b_path,
        conditions: command,
    } = command;
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
    let mut engine_a = match resolve_match_engine(
        engine_a_path,
        command.a_label,
        command.a_arguments,
        command.a_cwd,
        command.a_environment,
        command.a_options,
        command.a_buttons,
        command.a_cores,
    ) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let mut engine_b = match resolve_match_engine(
        engine_b_path,
        command.b_label,
        command.b_arguments,
        command.b_cwd,
        command.b_environment,
        command.b_options,
        command.b_buttons,
        command.b_cores,
    ) {
        Ok(engine) => engine,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = configure_ponder(&mut engine_a, command.ponder)
        .and_then(|()| configure_ponder(&mut engine_b, command.ponder))
    {
        eprintln!("configuration error: {error}");
        return ExitCode::from(2);
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
            "command": "match",
            "games": games,
            "engine_a": &engine_a,
            "engine_b": &engine_b,
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
        let output = MachineOutput::DryRun {
            command: "match",
            config_sha256: resolved.sha256(),
            resolved_configuration: resolved.value(),
            invocations: vec![&engine_a, &engine_b],
        };
        print_output(&output, machine);
        return ExitCode::SUCCESS;
    }

    let opened = match &command.run_directory {
        Some(path) => RunDirectory::open_explicit(path, &resolved, command.restart),
        None => RunDirectory::create_unique(&current_directory, "match", &resolved),
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
            eprintln!("match output failed: {error}");
            return ExitCode::from(3);
        }
    };
    colosseum_engine::incidents::set_dir(directory.paths().root.join("failed-games"));
    let mut recorder = match if opened.resumed {
        RunRecorder::resume(&directory)
    } else {
        RunRecorder::begin(&directory, "match")
    } {
        Ok(recorder) => recorder,
        Err(error) => {
            eprintln!("run record failed: {error}");
            return ExitCode::from(3);
        }
    };
    if let Err(error) = recorder.set_workflow(json!({
        "kind": "match",
        "pgn_annotation_writer": colosseum_engine::pgn::PGN_ANNOTATION_WRITER,
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
    let progress = match_runner::MatchProgress::default();
    let request = match_runner::FixedMatchRequest {
        engine_a,
        engine_b,
        games,
        engine_a_time_control,
        engine_b_time_control,
        adjudication,
        ponder: command.ponder,
        fault_policy,
        execution,
        master_seed,
        master_seed_generated,
        openings,
        completed_games,
        progress: progress.clone(),
        cancellation: cancellation.clone(),
        identity_override: None,
        observer: Some(observer.clone()),
    };
    if !machine {
        eprintln!("match run directory: {}", directory.paths().root.display());
    }
    if opened.resumed && !machine {
        eprintln!(
            "resuming {} durable game(s) from the stored schedule",
            progress.snapshot().attempted
        );
    }
    let match_future = match_runner::run_fixed_match(request);
    tokio::pin!(match_future);
    let period = Duration::from_secs(command.progress_interval_secs);
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    let outcome = loop {
        tokio::select! {
            result = &mut match_future => break result,
            _ = interval.tick() => {
                let snapshot = progress.snapshot();
                eprintln!(
                    "progress: {}/{} attempted, {} scored, {} faults",
                    snapshot.attempted, games, snapshot.scored, snapshot.faults
                );
            }
        }
    };
    match outcome {
        Ok(report) => {
            if let Err(error) = observer.finish(&report) {
                eprintln!("match output failed: {error}");
                return ExitCode::from(3);
            }
            let sample = OfficialSample {
                committed_units: u64::from(report.games_attempted),
                scored_games: u64::from(report.games_completed),
                completed_pairs: u64::from(report.games_completed / 2),
                pentanomial: [0; 5],
                unpaired_games: u64::from(report.games_completed % 2),
            };
            if let Err(error) = recorder.update_sample(sample) {
                eprintln!("run record failed: {error}");
                return ExitCode::from(3);
            }
            let (run_status, exit_code) = match report.status {
                match_runner::MatchStatus::Completed => (RunStatus::Completed, 0),
                match_runner::MatchStatus::Cancelled => (RunStatus::Cancelled, CANCELLED_EXIT_CODE),
                match_runner::MatchStatus::Invalid => (RunStatus::Invalid, 1),
                match_runner::MatchStatus::InfrastructureError => (RunStatus::Aborted, 3),
            };
            if let Err(error) = recorder.finish(run_status) {
                eprintln!("run record failed: {error}");
                return ExitCode::from(3);
            }
            if machine {
                print_json(&MachineOutput::FixedMatch {
                    run_directory: directory.paths().root.clone(),
                    report,
                });
            } else {
                print_fixed_match(&report);
                println!("artifacts: {}", directory.paths().root.display());
            }
            ExitCode::from(exit_code)
        }
        Err(error) => {
            eprintln!("match failed: {error}");
            ExitCode::from(3)
        }
    }
}

pub(crate) struct DurableMatchOutput {
    pub(crate) directory: Arc<RunDirectory>,
    pub(crate) games: Mutex<Vec<match_runner::MatchGame>>,
}

impl DurableMatchOutput {
    pub(crate) fn new(
        directory: Arc<RunDirectory>,
        mut games: Vec<match_runner::MatchGame>,
    ) -> Result<Self, String> {
        games.sort_by_key(|game| game.number);
        let output = Self {
            directory,
            games: Mutex::new(games),
        };
        output.rewrite_pgn()?;
        Ok(output)
    }

    pub(crate) fn rewrite_pgn(&self) -> Result<(), String> {
        let games = self.games.lock().map_err(|_| "PGN lock poisoned")?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.directory.paths().root.join("games.pgn"))
            .map_err(|error| error.to_string())?;
        for game in games.iter() {
            // A scorable game needs no class: an untagged game is part of the
            // sample, so a match export stays exactly what it was.
            let rendered = if game.scorable {
                game.pgn.trim_end().to_owned()
            } else {
                with_header_tags(
                    game.pgn.trim_end(),
                    &[("ColosseumSample", UNSCORABLE_SAMPLE)],
                )
            };
            writeln!(file, "{rendered}").map_err(|error| error.to_string())?;
            writeln!(file).map_err(|error| error.to_string())?;
        }
        file.sync_all().map_err(|error| error.to_string())
    }

    pub(crate) fn finish(&self, report: &match_runner::FixedMatchReport) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
        fs::write(self.directory.paths().root.join("result.json"), bytes)
            .map_err(|error| error.to_string())?;
        let mut line = serde_json::to_vec(&json!({
            "event": "match-finished",
            "status": report.status,
            "attempted": report.games_attempted,
            "scored": report.games_completed,
        }))
        .map_err(|error| error.to_string())?;
        line.push(b'\n');
        self.directory
            .append_log(&line)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn finish_calibration(&self, report: &CalibrationReport) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?;
        fs::write(self.directory.paths().root.join("result.json"), bytes)
            .map_err(|error| error.to_string())?;
        let mut line = serde_json::to_vec(&json!({
            "event": "calibration-finished",
            "status": report.status,
            "attempted": report.fixed_match.games_attempted,
            "scored": report.fixed_match.games_completed,
        }))
        .map_err(|error| error.to_string())?;
        line.push(b'\n');
        self.directory
            .append_log(&line)
            .map_err(|error| error.to_string())
    }
}

impl match_runner::MatchObserver for DurableMatchOutput {
    fn game_completed(&self, game: &match_runner::MatchGame) -> Result<(), String> {
        {
            let mut games = self.games.lock().map_err(|_| "checkpoint lock poisoned")?;
            games.push(game.clone());
            games.sort_by_key(|game| game.number);
            self.directory
                .write_checkpoint(&match_runner::MatchCheckpoint {
                    games: games.clone(),
                })
                .map_err(|error| error.to_string())?;
        }
        self.rewrite_pgn()?;
        let mut line = serde_json::to_vec(&json!({
            "event": "game-completed",
            "game": game,
        }))
        .map_err(|error| error.to_string())?;
        line.push(b'\n');
        self.directory
            .append_log(&line)
            .map_err(|error| error.to_string())
    }
}

pub(crate) fn print_fixed_match(report: &match_runner::FixedMatchReport) {
    println!(
        "fixed match {:?}: {}/{} games completed ({} attempted)",
        report.status, report.games_completed, report.games_requested, report.games_attempted
    );
    for (side, score) in [("A", &report.engine_a), ("B", &report.engine_b)] {
        println!(
            "engine {side} ({}): {} W / {} L / {} D",
            score.name, score.wins, score.losses, score.draws
        );
    }
    println!(
        "faults: A {} ({} time), B {} ({} time), infrastructure {}",
        report.faults.engine_a,
        report.faults.time_losses_a,
        report.faults.engine_b,
        report.faults.time_losses_b,
        report.faults.infrastructure
    );
    for game in &report.games {
        let error = game
            .error
            .as_deref()
            .map_or_else(String::new, |error| format!(" — {error}"));
        println!(
            "game {}: {:?} white, {} ({:?}){}",
            game.number,
            game.white,
            game.result.pgn(),
            game.termination,
            error
        );
    }
}
