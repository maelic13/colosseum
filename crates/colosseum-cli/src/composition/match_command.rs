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

    /// Games between progress blocks on standard error.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) progress_every: u64,
    /// Shortest time between two progress blocks. A run whose games finish
    /// faster than this coalesces them instead of flooding the console.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) progress_min_secs: u64,

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
        progress_every,
        progress_min_secs,
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
    let fault_policy = command.fault_policy(forfeit_allowance(u64::from(games)));
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
        let output = MachineOutput::DryRun {
            command: "match",
            config_sha256: resolved.sha256(),
            resolved_configuration: resolved.value(),
            invocations: vec![&engine_a, &engine_b],
            wave_shape: None,
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
    let journal = match open_journal(&directory, opened.resumed).await {
        Ok(journal) => journal,
        Err(error) => {
            eprintln!("resume failed: {error}");
            return ExitCode::from(3);
        }
    };
    let completed_games = journal
        .records
        .iter()
        .filter_map(match_runner::MatchGame::from_journal)
        .collect::<Vec<_>>();
    let writer = match RunWriter::start(Arc::clone(&directory), journal.resume).await {
        Ok(writer) => writer,
        Err(error) => {
            eprintln!("match output failed: {error}");
            return ExitCode::from(3);
        }
    };
    let observer = Arc::new(DurableMatchOutput::new(
        writer.clone(),
        ProgressUnit::Games,
        &completed_games,
    ));
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
    recorder.write_through(writer.clone());
    if let Err(error) = recorder.set_workflow(json!({
        "kind": "match",
        "progress": {"every": progress_every, "unit": "games", "min_secs": progress_min_secs},
        "pgn_annotation_writer": colosseum_engine::pgn::PGN_ANNOTATION_WRITER,
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
    let progress = match_runner::MatchProgress::default();
    let resumed_games = completed_games.len();
    let players = Players::new(&engine_a, &engine_b);
    let request = match_runner::FixedMatchRequest {
        engine_a,
        engine_b,
        games,
        engine_a_time_control,
        engine_b_time_control,
        adjudication,
        ponder: command.ponder,
        engine_processes: command.engine_processes,
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
    let mut schedule =
        ProgressSchedule::new(progress_every, progress_min_secs, resumed_games as u64);
    let mut poll =
        tokio::time::interval_at(tokio::time::Instant::now() + PROGRESS_POLL, PROGRESS_POLL);
    let outcome = loop {
        tokio::select! {
            result = &mut match_future => break result,
            _ = poll.tick() => {
                if schedule.due(observer.units()) {
                    let block = match_progress_block(&observer, &schedule, &players, games, fault_policy);
                    publish_progress(&block, &writer, &mut recorder);
                }
            }
        }
    };
    let final_block = match_progress_block(&observer, &schedule, &players, games, fault_policy);
    if schedule.needs_final(final_block.done) {
        schedule.mark(final_block.done);
        publish_progress(&final_block, &writer, &mut recorder);
    }
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
            if let Err(error) = settle(&writer).await {
                eprintln!("match output failed: {error}");
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
            drop(recorder);
            let _ = settle(&writer).await;
            ExitCode::from(3)
        }
    }
}

/// What a fixed match tells the operator: how the score stands, what it is
/// worth in Elo, and how fast the games are arriving.
///
/// Unpaired games are the unit here, so the estimate is the ordinary W/D/L one
/// rather than the paired estimator an SPRT reports.
pub(crate) fn match_progress_block(
    observer: &DurableMatchOutput,
    schedule: &ProgressSchedule,
    players: &Players,
    total: u32,
    policy: FaultPolicy,
) -> ProgressBlock {
    let (sample, done) = observer.sample();
    let mut block = ProgressBlock::new(
        "match",
        ProgressUnit::Games,
        done,
        Some(u64::from(total)),
        schedule.elapsed(),
    );
    block.field("players", players.to_string());
    let points = f64::from(sample.wins) + 0.5 * f64::from(sample.draws);
    if sample.scored_games > 0 {
        block.field(
            "score",
            format!(
                "{points}/{} ({:.1}%)",
                sample.scored_games,
                100.0 * points / f64::from(sample.scored_games)
            ),
        );
    } else {
        block.field("score", "no scored games yet");
    }
    // A fixed match plays colour-reversed pairs on one opening, exactly as a
    // sequential test does, so it has the same paired estimates and reports
    // them the same way.
    sample.add_fields(&mut block, policy, players);
    if let Some(rate) =
        progress::rate_per_hour(schedule.units_since_start(done), schedule.elapsed_hours())
    {
        block.field("rate", format!("{rate:.0} games/hour"));
    }
    block.field(
        "time remaining",
        match progress::time_for_units(
            schedule.units_since_start(done),
            schedule.elapsed(),
            u64::from(total).saturating_sub(done),
        ) {
            Some(left) => progress::format_duration(left.as_secs_f64()),
            None => "unknown".to_owned(),
        },
    );
    block
}

/// What a fixed match or a calibration writes while it runs.
///
/// Each game is one journal line and one appended PGN game, handed to the
/// writer and forgotten; the output keeps only the running sample. A
/// checkpoint holds that sample and the journal position it covers, so its
/// size is the same at the first game and the thirty-thousandth.
pub(crate) struct DurableMatchOutput {
    pub(crate) writer: RunWriter,
    /// What a checkpoint counts: games for a match, complete pairs for a
    /// calibration.
    unit: ProgressUnit,
    state: Mutex<MatchAggregate>,
}

struct MatchAggregate {
    sample: PairedProgress,
    attempted: u64,
    cadence: CheckpointCadence,
}

impl MatchAggregate {
    fn checkpoint(&self, command: &str) -> Value {
        json!({
            "command": command,
            "games_attempted": self.attempted,
            "games_scored": self.sample.scored_games,
            "wins": self.sample.wins,
            "draws": self.sample.draws,
            "losses": self.sample.losses,
            "pairs": self.sample.pairs,
            "pentanomial": self.sample.vector.counts(),
            "faults": self.sample.faults,
        })
    }
}

impl DurableMatchOutput {
    /// Start from the games a resume found in the journal.
    pub(crate) fn new(
        writer: RunWriter,
        unit: ProgressUnit,
        games: &[match_runner::MatchGame],
    ) -> Self {
        Self {
            writer,
            unit,
            state: Mutex::new(MatchAggregate {
                sample: PairedProgress::from_games(games),
                attempted: games.len() as u64,
                cadence: CheckpointCadence::new(),
            }),
        }
    }

    fn command(&self) -> &'static str {
        match self.unit {
            ProgressUnit::Pairs => "calibrate",
            _ => "match",
        }
    }

    /// Units committed, cheaply, for deciding whether a block is due.
    pub(crate) fn units(&self) -> u64 {
        self.state.lock().map_or(0, |state| match self.unit {
            ProgressUnit::Pairs => u64::from(state.sample.pairs),
            _ => state.attempted,
        })
    }

    /// The running sample and the games attempted, for a progress block.
    pub(crate) fn sample(&self) -> (PairedProgress, u64) {
        self.state.lock().map_or_else(
            |_| (PairedProgress::default(), 0),
            |state| (state.sample.clone(), state.attempted),
        )
    }

    fn final_writes(&self, result: Vec<u8>, event: &Value) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "match state lock poisoned")?;
        self.writer.checkpoint(state.checkpoint(self.command()))?;
        drop(state);
        self.writer
            .replace(self.writer_root().join("result.json"), result)?;
        log_event(&self.writer, event);
        Ok(())
    }

    fn writer_root(&self) -> PathBuf {
        self.writer.root().to_path_buf()
    }

    pub(crate) fn finish(&self, report: &match_runner::FixedMatchReport) -> Result<(), String> {
        self.final_writes(
            serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?,
            &json!({
                "event": "match-finished",
                "status": report.status,
                "attempted": report.games_attempted,
                "scored": report.games_completed,
            }),
        )
    }

    pub(crate) fn finish_calibration(&self, report: &CalibrationReport) -> Result<(), String> {
        self.final_writes(
            serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?,
            &json!({
                "event": "calibration-finished",
                "status": report.status,
                "attempted": report.fixed_match.games_attempted,
                "scored": report.fixed_match.games_completed,
            }),
        )
    }
}

impl match_runner::MatchObserver for DurableMatchOutput {
    fn game_completed(&self, game: &match_runner::MatchGame) -> Result<(), String> {
        // The journal files an unscorable game under the class it was played
        // in, marked `scorable: false`; the PGN tags it `unscorable`.
        let record = game.journal_record(OFFICIAL_SAMPLE, None);
        // A scorable game needs no class: an untagged game is part of the
        // sample, so a match export stays exactly what it was.
        let moves = if game.scorable {
            game.pgn.clone()
        } else {
            with_header_tags(
                game.pgn.trim_end(),
                &[("ColosseumSample", UNSCORABLE_SAMPLE)],
            )
        };
        log_fault(&self.writer, &record);
        self.writer.game(record, moves)?;
        let mut state = self.state.lock().map_err(|_| "match state lock poisoned")?;
        let pairs_before = state.sample.pairs;
        state.sample.add_game(game);
        state.attempted += 1;
        let units = match self.unit {
            ProgressUnit::Pairs => u64::from(state.sample.pairs - pairs_before),
            _ => 1,
        };
        if state.cadence.record(units) {
            self.writer.checkpoint(state.checkpoint(self.command()))?;
        }
        Ok(())
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
    let sample = PairedProgress::from_games(&report.games);
    match pentanomial_statistics(&sample.vector, Z95) {
        Ok(statistics) => println!(
            "Elo {}; nElo {} over {}",
            progress::estimate(
                statistics.logistic_elo.elo,
                statistics.logistic_elo.lower,
                statistics.logistic_elo.upper
            ),
            progress::estimate(
                statistics.normalized_elo.elo,
                statistics.normalized_elo.lower,
                statistics.normalized_elo.upper
            ),
            progress::plural(u64::from(sample.pairs), "complete pair")
        ),
        Err(error) => println!("Elo and nElo unavailable: {error}"),
    }
    println!("Ptnml: {:?}", sample.vector.counts());
    let players = Players {
        a: report.engine_a.name.clone(),
        b: report.engine_b.name.clone(),
    };
    println!(
        "faults: {}; infrastructure {}; {}",
        fault_counts_text(report.faults, &players),
        report.faults.infrastructure,
        fault_allowance_text(
            report.fault_policy,
            report.faults,
            u64::from(report.games_attempted)
        )
    );
    println!("abnormal games: {}", abnormal_games(&report.games));
}

/// How many games ended badly and in which way.
///
/// The per-game list this replaces printed one line for every game a run
/// played, which buries the report it belongs to and says nothing a reader
/// cannot get better elsewhere: `games.pgn` has every result, `run.log` has
/// every event, and `failed-games/` has the UCI traffic of each abnormal one.
pub(crate) fn abnormal_games(games: &[match_runner::MatchGame]) -> String {
    let mut counts = BTreeMap::<&'static str, u32>::new();
    for game in games.iter().filter(|game| game.fault.is_some()) {
        let kind = match game.termination {
            Termination::TimeForfeit => "time forfeit",
            Termination::EngineCrash => "engine crash",
            Termination::IllegalMove => "illegal move",
            Termination::Aborted => "aborted",
            _ => "other fault",
        };
        *counts.entry(kind).or_default() += 1;
    }
    if counts.is_empty() {
        return "none".to_owned();
    }
    counts
        .into_iter()
        .map(|(kind, count)| format!("{kind} {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}
