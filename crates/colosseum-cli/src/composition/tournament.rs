//! The `tournament` command and its `gauntlet` alias.

use super::*;

#[derive(Debug, Args)]
pub(crate) struct TournamentCommand {
    #[command(subcommand)]
    pub(crate) action: TournamentAction,
}

#[derive(Debug, Subcommand)]
pub(crate) enum TournamentAction {
    /// Print the exact static schedule without launching engines.
    Plan(TournamentPlanCommand),
    /// Play the schedule, persist every game, and write ratings and CSV files.
    Run(Box<TournamentRunCommand>),
}

#[derive(Debug, Args)]
pub(crate) struct TournamentPlanCommand {
    /// UCI executable in selection/seed order; repeat at least twice.
    #[arg(long = "engine", required = true)]
    pub(crate) engines: Vec<PathBuf>,
    /// Round-robin or gauntlet schedule.
    #[arg(long, value_enum)]
    pub(crate) format: Option<TournamentFormatArg>,
    /// Leading engines treated as gauntlet seeds.
    #[arg(long, default_value_t = 1)]
    pub(crate) seeds: u32,
    /// Repetitions of the complete format schedule.
    #[arg(long, default_value_t = 1)]
    pub(crate) cycles: u32,
    /// Games per encounter, alternating colours.
    #[arg(long, default_value_t = 2)]
    pub(crate) games_per_pair: u32,
    /// Initial rating aligned with each --engine; defaults to 1500.
    #[arg(long = "rating", allow_hyphen_values = true)]
    pub(crate) ratings: Vec<f64>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum TournamentFormatArg {
    RoundRobin,
    Gauntlet,
}

#[derive(Debug, Args)]
pub(crate) struct TournamentRunCommand {
    #[command(flatten)]
    pub(crate) plan: TournamentPlanCommand,

    /// Display labels aligned with --engine; omit all to use file stems.
    #[arg(long = "label")]
    pub(crate) labels: Vec<String>,
    /// Argument passed to every engine process; repeat as needed.
    #[arg(long = "engine-arg", allow_hyphen_values = true)]
    pub(crate) arguments: Vec<OsString>,
    /// Argument for one engine as one-based INDEX:ARG.
    #[arg(
        long = "engine-arg-at",
        allow_hyphen_values = true,
        value_name = "INDEX:ARG"
    )]
    pub(crate) indexed_arguments: Vec<String>,
    /// Working directory used for every engine process.
    #[arg(long)]
    pub(crate) cwd: Option<PathBuf>,
    /// Working directory override for one engine as one-based INDEX:PATH.
    #[arg(long = "engine-cwd", value_name = "INDEX:PATH")]
    pub(crate) indexed_cwds: Vec<String>,
    /// Environment override applied to every engine as KEY=VALUE.
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub(crate) environment: Vec<String>,
    /// Environment override for one engine as INDEX:KEY=VALUE.
    #[arg(long = "engine-env", value_name = "INDEX:KEY=VALUE")]
    pub(crate) indexed_environment: Vec<String>,
    /// UCI option applied to every engine as NAME=VALUE.
    #[arg(long = "option", value_name = "NAME=VALUE")]
    pub(crate) options: Vec<String>,
    /// UCI option for one engine as INDEX:NAME=VALUE.
    #[arg(long = "engine-option", value_name = "INDEX:NAME=VALUE")]
    pub(crate) indexed_options: Vec<String>,
    /// Trigger this UCI button on every engine.
    #[arg(long = "button", value_name = "NAME")]
    pub(crate) buttons: Vec<String>,
    /// Trigger a UCI button on one engine as INDEX:NAME.
    #[arg(long = "engine-button", value_name = "INDEX:NAME")]
    pub(crate) indexed_buttons: Vec<String>,

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
    /// Let engines think on the opponent's clock through the UCI ponder protocol.
    /// Both engines then search at once, so each needs its own cores.
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
    /// One-based --engine index whose prior rating fixes the Elo scale.
    #[arg(long)]
    pub(crate) anchor: Option<usize>,
    /// Pin a participant at a supplied rating as one-based INDEX:RATING;
    /// repeat for every member of an established field.
    #[arg(long = "fixed", value_name = "INDEX:RATING")]
    pub(crate) fixed_ratings: Vec<String>,
    /// Invalidate only after more engine faults than this; omitted is non-strict.
    #[arg(long)]
    pub(crate) max_engine_faults: Option<u32>,
    #[arg(long = "dir")]
    pub(crate) run_directory: Option<PathBuf>,
    #[arg(long, requires = "run_directory")]
    pub(crate) restart: bool,
}

pub(crate) fn run_tournament_plan(
    command: TournamentPlanCommand,
    forced_format: Option<TournamentFormatArg>,
    machine: bool,
) -> ExitCode {
    let launches = command
        .engines
        .iter()
        .cloned()
        .map(EngineLaunchSpec::path_only)
        .collect();
    let report = match build_tournament_plan(command, forced_format, launches) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if machine {
        print_json(&MachineOutput::TournamentPlan { report });
    } else {
        println!(
            "Tournament plan: {:?}; {} participants, {} games",
            report.design.format,
            report.participants.len(),
            report.schedule.len()
        );
        for game in &report.schedule {
            println!(
                "game {:>4}: round {:>3}, encounter {:>3}.{} — {} vs {}",
                game.number,
                game.round,
                game.encounter,
                game.game_in_encounter,
                game.white,
                game.black
            );
        }
    }
    ExitCode::SUCCESS
}

pub(crate) fn build_tournament_plan(
    command: TournamentPlanCommand,
    forced_format: Option<TournamentFormatArg>,
    launches: Vec<EngineLaunchSpec>,
) -> Result<TournamentPlan, String> {
    let format = match (forced_format, command.format) {
        (Some(forced), Some(requested))
            if std::mem::discriminant(&forced) != std::mem::discriminant(&requested) =>
        {
            return Err("the gauntlet alias cannot select round-robin".into());
        }
        (Some(forced), _) => forced,
        (None, Some(requested)) => requested,
        (None, None) => TournamentFormatArg::RoundRobin,
    };
    if launches.len() != command.engines.len() {
        return Err("resolved engine controls do not match --engine count".into());
    }
    if !command.ratings.is_empty() && command.ratings.len() != command.engines.len() {
        return Err("--rating must be omitted or repeated once for every --engine".into());
    }
    let ratings = if command.ratings.is_empty() {
        vec![1_500.0; command.engines.len()]
    } else {
        command.ratings
    };
    let participants = command
        .engines
        .into_iter()
        .zip(launches)
        .zip(ratings)
        .enumerate()
        .map(
            |(index, ((_executable, launch), initial_rating))| TournamentParticipant {
                participant: RuntimeParticipant {
                    id: ParticipantId::from_u128(index as u128 + 1),
                    launch,
                },
                initial_rating,
            },
        )
        .collect();
    let format = match format {
        TournamentFormatArg::RoundRobin => colosseum_core::Format::RoundRobin {
            cycles: command.cycles,
        },
        TournamentFormatArg::Gauntlet => colosseum_core::Format::Gauntlet {
            seeds: command.seeds,
            cycles: command.cycles,
        },
    };
    PlanTournament::execute(
        participants,
        TournamentDesign {
            format,
            games_per_pair: command.games_per_pair,
        },
    )
    .map_err(|error| error.to_string())
}

pub(crate) async fn run_tournament_command(
    command: TournamentRunCommand,
    machine: bool,
    dry_run: bool,
    cancellation: Cancellation,
) -> ExitCode {
    if !command.labels.is_empty() && command.labels.len() != command.plan.engines.len() {
        eprintln!(
            "configuration error: --label must be omitted or repeated once for every --engine"
        );
        return ExitCode::from(2);
    }
    if command.book.is_none()
        && (command.book_start != 0
            || command.book_plies.is_some()
            || command.book_order != BookOrderArg::Sequential)
    {
        eprintln!("configuration error: book order/start/plies require --book");
        return ExitCode::from(2);
    }
    let engine_count = command.plan.engines.len();
    let indexed_arguments =
        match parse_indexed_values(&command.indexed_arguments, engine_count, "--engine-arg-at") {
            Ok(values) => values,
            Err(error) => return tournament_configuration_error(&error),
        };
    let indexed_cwds =
        match parse_indexed_values(&command.indexed_cwds, engine_count, "--engine-cwd") {
            Ok(values) => values,
            Err(error) => return tournament_configuration_error(&error),
        };
    if indexed_cwds.iter().any(|values| values.len() > 1) {
        return tournament_configuration_error("--engine-cwd may occur only once per engine");
    }
    let indexed_environment =
        match parse_indexed_values(&command.indexed_environment, engine_count, "--engine-env") {
            Ok(values) => values,
            Err(error) => return tournament_configuration_error(&error),
        };
    let indexed_options =
        match parse_indexed_values(&command.indexed_options, engine_count, "--engine-option") {
            Ok(values) => values,
            Err(error) => return tournament_configuration_error(&error),
        };
    let indexed_buttons =
        match parse_indexed_values(&command.indexed_buttons, engine_count, "--engine-button") {
            Ok(values) => values,
            Err(error) => return tournament_configuration_error(&error),
        };
    let launches = command
        .plan
        .engines
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, executable)| {
            let mut arguments = command.arguments.clone();
            arguments.extend(indexed_arguments[index].iter().map(OsString::from));
            let mut environment = command.environment.clone();
            environment.extend(indexed_environment[index].iter().cloned());
            let mut options = command.options.clone();
            options.extend(indexed_options[index].iter().cloned());
            let mut buttons = command.buttons.clone();
            buttons.extend(indexed_buttons[index].iter().cloned());
            resolve_match_engine(
                executable,
                command.labels.get(index).cloned(),
                arguments,
                indexed_cwds[index]
                    .first()
                    .map(PathBuf::from)
                    .or_else(|| command.cwd.clone()),
                environment,
                options,
                buttons,
                None,
            )
        })
        .collect::<Result<Vec<_>, _>>();
    let mut launches = match launches {
        Ok(launches) => launches,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = launches
        .iter_mut()
        .try_for_each(|launch| configure_ponder(launch, command.ponder))
    {
        eprintln!("configuration error: {error}");
        return ExitCode::from(2);
    }
    let plan = match build_tournament_plan(command.plan, None, launches) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let anchor = match command.anchor {
        Some(index) if (1..=plan.participants.len()).contains(&index) => {
            Some(ParticipantId::from_u128(index as u128))
        }
        Some(_) => {
            eprintln!("configuration error: --anchor is outside the --engine list");
            return ExitCode::from(2);
        }
        None => None,
    };
    let fixed_ratings = match parse_fixed_ratings(&command.fixed_ratings, plan.participants.len()) {
        Ok(fixed) => fixed,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let time_control = match resolve_time_control(
        "tournament",
        command.movetime_ms,
        command.base_ms,
        command.increment_ms,
        command.nodes,
        command.depth,
        command.margin_ms,
    ) {
        Ok(control) => control,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = validate_ponder(command.ponder, &[time_control]) {
        eprintln!("configuration error: {error}");
        return ExitCode::from(2);
    }
    let adjudication = AdjudicationConfig {
        max_moves: command.max_moves,
        draw: command.draw_adjudication.then_some(DrawAdjudication {
            min_ply: command.draw_move.saturating_mul(2),
            move_count: command.draw_moves,
            score_cp: command.draw_score_cp,
        }),
        resign: command.resign_adjudication.then_some(ResignAdjudication {
            move_count: command.resign_moves,
            score_cp: command.resign_score_cp,
            two_sided: !command.one_sided_resign_adjudication,
        }),
    };
    let placement = match resolve_placement(&command.placement, command.headroom_cores) {
        Ok(placement) => placement,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let execution = match match_runner::plan_execution(
        &plan.participants[0].participant.launch,
        &plan.participants[1].participant.launch,
        command.concurrency as usize,
        resolve_slot_allocation(command.cores_per_game, command.cores_per_engine),
        placement,
        command.memory_budget_mb,
    ) {
        Ok(execution) => execution,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let resumed_seed = command
        .run_directory
        .as_deref()
        .filter(|path| path.exists() && !command.restart)
        .and_then(read_stored_seed);
    let (master_seed, master_seed_generated) = match (command.seed, resumed_seed) {
        (None, Some(stored)) => stored,
        (configured, _) => match resolve_master_seed(configured) {
            Ok(seed) => seed,
            Err(error) => {
                eprintln!("configuration error: {error}");
                return ExitCode::from(2);
            }
        },
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
    // A tournament plays every game of one encounter from the same opening, so
    // the schedule consumes one book entry per encounter rather than per pair.
    let encounters = plan
        .schedule
        .iter()
        .map(|game| game.encounter)
        .collect::<BTreeSet<_>>()
        .len();
    let encounters = match u32::try_from(encounters) {
        Ok(encounters) => encounters,
        Err(_) => {
            eprintln!("configuration error: tournament schedule is too large");
            return ExitCode::from(2);
        }
    };
    let openings = match match_runner::resolve_openings(
        book,
        command.book_start,
        encounters,
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
    let mut path_pointers = plan
        .participants
        .iter()
        .enumerate()
        .flat_map(|(index, participant)| {
            let mut paths = vec![format!(
                "/plan/participants/{index}/participant/launch/executable"
            )];
            if participant.participant.launch.working_directory.is_some() {
                paths.push(format!(
                    "/plan/participants/{index}/participant/launch/working_directory"
                ));
            }
            paths
        })
        .collect::<Vec<_>>();
    if command.book.is_some() {
        path_pointers.push("/openings/path".into());
    }
    let resolved = match resolve_config(
        built_in_defaults(),
        None,
        json!({
            "command": "tournament",
            "plan": &plan,
            "anchor": anchor,
            "fixed_ratings": &fixed_ratings,
            "time_control": time_control,
            "adjudication": adjudication,
            "ponder": command.ponder,
            "execution": &execution,
            "max_engine_faults": command.max_engine_faults,
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
        let invocations = plan
            .participants
            .iter()
            .map(|participant| &participant.participant.launch)
            .collect();
        print_output(
            &MachineOutput::DryRun {
                command: "tournament",
                config_sha256: resolved.sha256(),
                resolved_configuration: resolved.value(),
                invocations,
            },
            machine,
        );
        return ExitCode::SUCCESS;
    }
    let opened = match &command.run_directory {
        Some(path) => RunDirectory::open_explicit(path, &resolved, command.restart),
        None => RunDirectory::create_unique(&current_directory, "tournament", &resolved),
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
    let checkpoint = if opened.resumed {
        match directory.read_checkpoint::<tournament_driver::TournamentCheckpoint>() {
            Ok(checkpoint) => checkpoint,
            Err(error) => {
                eprintln!("resume failed: {error}");
                return ExitCode::from(3);
            }
        }
    } else {
        tournament_driver::TournamentCheckpoint::default()
    };
    let observer = match DurableTournamentOutput::new(Arc::clone(&directory), checkpoint.clone()) {
        Ok(observer) => Arc::new(observer),
        Err(error) => {
            eprintln!("tournament output failed: {error}");
            return ExitCode::from(3);
        }
    };
    colosseum_engine::incidents::set_dir(directory.paths().root.join("failed-games"));
    let mut recorder = match if opened.resumed {
        RunRecorder::resume(&directory)
    } else {
        RunRecorder::begin(&directory, "tournament")
    } {
        Ok(recorder) => recorder,
        Err(error) => {
            eprintln!("run record failed: {error}");
            return ExitCode::from(3);
        }
    };
    if let Err(error) = recorder.set_workflow(json!({
        "kind": "tournament",
        "pgn_annotation_writer": colosseum_engine::pgn::PGN_ANNOTATION_WRITER,
        "format": plan.design.format,
        "games_scheduled": plan.schedule.len(),
        "anchor": anchor,
        "fixed_ratings": &fixed_ratings,
        "ponder": command.ponder,
        "fault_policy": {
            "mode": if command.max_engine_faults.is_some() { "strict-limit" } else { "exploratory-non-strict" },
            "max_engine_faults": command.max_engine_faults,
        },
        "artifacts": ["checkpoint.json", "games.pgn", "standings.csv", "crosstable.csv", "result.json"],
    })) {
        eprintln!("run record failed: {error}");
        return ExitCode::from(3);
    }
    let request = tournament_driver::TournamentRunRequest {
        plan,
        anchor,
        fixed_ratings,
        cancellation: cancellation.clone(),
        time_control,
        adjudication,
        ponder: command.ponder,
        execution,
        master_seed,
        master_seed_generated,
        openings,
        max_engine_faults: command.max_engine_faults,
        completed_games: checkpoint.games,
        observer: Some(observer),
    };
    match tournament_driver::run_tournament(request).await {
        Ok(report) => {
            if let Err(error) = write_tournament_artifacts(&directory, &report) {
                eprintln!("tournament output failed: {error}");
                return ExitCode::from(3);
            }
            if let Err(error) = recorder.update_sample(OfficialSample {
                committed_units: report.games.len() as u64,
                scored_games: report.results.games_scored as u64,
                completed_pairs: 0,
                pentanomial: [0; 5],
                unpaired_games: report.results.games_scored as u64,
            }) {
                eprintln!("run record failed: {error}");
                return ExitCode::from(3);
            }
            let (run_status, exit_code) = match report.status {
                tournament_driver::TournamentRunStatus::Completed => (RunStatus::Completed, 0),
                tournament_driver::TournamentRunStatus::Cancelled => {
                    (RunStatus::Cancelled, CANCELLED_EXIT_CODE)
                }
                tournament_driver::TournamentRunStatus::Invalid => (RunStatus::Invalid, 1),
                tournament_driver::TournamentRunStatus::InfrastructureError => {
                    (RunStatus::Aborted, 3)
                }
            };
            if let Err(error) = recorder.finish(run_status) {
                eprintln!("run record failed: {error}");
                return ExitCode::from(3);
            }
            if machine {
                print_json(&MachineOutput::Tournament {
                    run_directory: directory.paths().root.clone(),
                    report,
                });
            } else {
                print_tournament(&report);
                println!("artifacts: {}", directory.paths().root.display());
            }
            ExitCode::from(exit_code)
        }
        Err(error) => {
            eprintln!("tournament failed: {error}");
            ExitCode::from(3)
        }
    }
}

/// Parse repeated `--fixed INDEX:RATING` values against the `--engine` order.
///
/// The index is one-based to match `--engine` and every other indexed option.
/// A malformed, duplicated or out-of-range entry is refused here rather than
/// silently dropped: a pinned rating is part of what the result means.
pub(crate) fn parse_fixed_ratings(
    values: &[String],
    participants: usize,
) -> Result<Vec<TournamentFixedRating>, String> {
    let mut seen = BTreeSet::new();
    let mut fixed = Vec::with_capacity(values.len());
    for value in values {
        let Some((index, rating)) = value.split_once(':') else {
            return Err(format!("--fixed {value:?} must use INDEX:RATING"));
        };
        let index = index
            .trim()
            .parse::<usize>()
            .map_err(|_| format!("--fixed {value:?} has an invalid engine index"))?;
        if !(1..=participants).contains(&index) {
            return Err(format!(
                "--fixed {value:?} is outside the --engine list of {participants}"
            ));
        }
        if !seen.insert(index) {
            return Err(format!("--fixed names engine {index} more than once"));
        }
        let rating = rating
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("--fixed {value:?} has an invalid rating"))?;
        if !rating.is_finite() {
            return Err(format!("--fixed {value:?} has a non-finite rating"));
        }
        fixed.push(TournamentFixedRating {
            participant: ParticipantId::from_u128(index as u128),
            rating,
        });
    }
    if !fixed.is_empty() && fixed.len() >= participants {
        return Err(
            "--fixed pins every participant, so nothing would be estimated; leave at least one free"
                .into(),
        );
    }
    Ok(fixed)
}

pub(crate) fn tournament_configuration_error(error: &str) -> ExitCode {
    eprintln!("configuration error: {error}");
    ExitCode::from(2)
}

pub(crate) fn write_tournament_artifacts(
    directory: &RunDirectory,
    report: &tournament_driver::TournamentReport,
) -> Result<(), String> {
    fs::write(
        directory.paths().root.join("standings.csv"),
        &report.results.standings_csv,
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        directory.paths().root.join("crosstable.csv"),
        &report.results.crosstable_csv,
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        directory.paths().root.join("result.json"),
        serde_json::to_vec_pretty(report).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())
}

pub(crate) fn print_tournament(report: &tournament_driver::TournamentReport) {
    println!(
        "Tournament {:?}: {}/{} games scored ({} attempted)",
        report.status,
        report.results.games_scored,
        report.results.games_scheduled,
        report.results.games_attempted
    );
    for row in &report.results.standings {
        let error = row
            .error_95
            .map_or_else(|| "unavailable".into(), |value| format!("±{value:.1}"));
        let fixed = if row.fixed { " [fixed]" } else { "" };
        println!(
            "{}. {}: {:.1} {error}{fixed}; {:.1}/{} ({}-{}-{})",
            row.rank, row.name, row.rating, row.points, row.games, row.wins, row.draws, row.losses
        );
    }
}

pub(crate) struct DurableTournamentOutput {
    pub(crate) directory: Arc<RunDirectory>,
    pub(crate) checkpoint: Mutex<tournament_driver::TournamentCheckpoint>,
}

impl DurableTournamentOutput {
    pub(crate) fn new(
        directory: Arc<RunDirectory>,
        mut checkpoint: tournament_driver::TournamentCheckpoint,
    ) -> Result<Self, String> {
        checkpoint.games.sort_by_key(|game| game.number);
        let output = Self {
            directory,
            checkpoint: Mutex::new(checkpoint),
        };
        output.persist()?;
        Ok(output)
    }

    pub(crate) fn persist(&self) -> Result<(), String> {
        let checkpoint = self
            .checkpoint
            .lock()
            .map_err(|_| "tournament checkpoint lock poisoned")?;
        self.directory
            .write_checkpoint(&*checkpoint)
            .map_err(|error| error.to_string())?;
        let mut pgn = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.directory.paths().root.join("games.pgn"))
            .map_err(|error| error.to_string())?;
        for game in &checkpoint.games {
            pgn.write_all(game.pgn.as_bytes())
                .and_then(|()| pgn.write_all(b"\n"))
                .map_err(|error| error.to_string())?;
        }
        pgn.sync_all().map_err(|error| error.to_string())
    }
}

impl tournament_driver::TournamentObserver for DurableTournamentOutput {
    fn game_completed(&self, game: &tournament_driver::TournamentGame) -> Result<(), String> {
        {
            let mut checkpoint = self
                .checkpoint
                .lock()
                .map_err(|_| "tournament checkpoint lock poisoned")?;
            if checkpoint
                .games
                .iter()
                .any(|saved| saved.number == game.number)
            {
                return Err(format!("duplicate tournament game {}", game.number));
            }
            checkpoint.games.push(game.clone());
            checkpoint.games.sort_by_key(|saved| saved.number);
        }
        self.persist()?;
        let event = serde_json::to_vec(&json!({
            "event": "tournament-game-completed",
            "game": game,
        }))
        .map_err(|error| error.to_string())?;
        self.directory
            .append_log(&[event, b"\n".to_vec()].concat())
            .map_err(|error| error.to_string())
    }
}
