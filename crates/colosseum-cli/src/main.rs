//! Independent headless composition root for Colosseum CLI.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use colosseum_application::{
    CalibrationBinaries, CalibrationDesign, CalibrationInterval, CalibrationStatus, CheckEngine,
    CompletePair, ComplianceReport, ComplianceStatus, DEFAULT_CALIBRATION_CONFIDENCE,
    DEFAULT_CALIBRATION_GAMES, DEFAULT_CALIBRATION_TOLERANCE_NELO, EngineInspection,
    EngineLaunchSpec, InspectEngine, RuntimeParticipant, SprtBundle, SprtDesign, SprtParameters,
    UciOptionSchema, classify_calibration,
};
use colosseum_cli::{
    EngineArgs, OfficialSample, RunDirectory, RunRecord, RunRecorder, RunStatus, built_in_defaults,
    parse_cpu_list, resolve_config,
};
use colosseum_core::{
    AdjudicationConfig, DrawAdjudication, EloModel, GameResult, OpeningBook, OpeningOrder,
    PairGameResult, ParticipantId, PentanomialVector, ResignAdjudication, TimeControl,
    fixed_n_achieved_resolution,
};
use colosseum_engine::CpuPlacementPolicy;
use colosseum_uci::UciSessionFactory;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

mod capabilities;
mod match_runner;
mod self_test;
mod sprt_runner;
mod uci_stub;

#[derive(Debug, Parser)]
#[command(
    name = "colosseum-cli",
    version,
    about = "Run reproducible UCI chess-engine tests and experiments",
    long_about = "A headless harness for inspecting, testing and comparing ordinary UCI chess-engine executables."
)]
struct Cli {
    /// Emit exactly one JSON value on stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Resolve and print configuration/invocations without launching an engine.
    #[arg(long, global = true)]
    dry_run: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print detected topology, restrictions and affinity support.
    Capabilities,
    /// Run a fixed number of games between two ordinary UCI engines.
    Match(Box<MatchCommand>),
    /// Run a finite pair-atomic sequential probability ratio test.
    Sprt(Box<SprtCommand>),
    /// Measure identical-binary symmetry under representative match conditions.
    Calibrate(Box<CalibrationCommand>),
    /// Inspect or compliance-check an ordinary UCI executable.
    Engine(EngineCommand),
    /// Verify this exact executable's protocol, process and persistence paths.
    SelfTest,
    /// Read the official state of any CLI run without modifying it.
    Status {
        /// Self-contained run directory to inspect.
        run_directory: std::path::PathBuf,
    },
    /// Internal deterministic UCI fixture. Not a public engine interface.
    #[command(name = "__uci-stub", hide = true)]
    UciStub(uci_stub::StubArgs),
}

#[derive(Debug, Args)]
struct MatchCommand {
    /// Exact number of games to play; this command never stops early.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    games: u32,

    #[command(flatten)]
    conditions: MatchConditions,
}

#[derive(Debug, Args)]
struct MatchConditions {
    /// Path to engine A's UCI executable.
    engine_a: PathBuf,

    /// Path to engine B's UCI executable.
    engine_b: PathBuf,

    #[arg(long = "a-label")]
    a_label: Option<String>,
    #[arg(long = "a-engine-arg", allow_hyphen_values = true)]
    a_arguments: Vec<OsString>,
    #[arg(long = "a-cwd")]
    a_cwd: Option<PathBuf>,
    #[arg(long = "a-env", value_name = "KEY=VALUE")]
    a_environment: Vec<String>,
    #[arg(long = "a-option", value_name = "NAME=VALUE")]
    a_options: Vec<String>,
    #[arg(long = "a-button", value_name = "NAME")]
    a_buttons: Vec<String>,
    #[arg(long = "a-cores", value_name = "LIST")]
    a_cores: Option<String>,
    #[arg(long = "a-movetime-ms", value_parser = clap::value_parser!(u64).range(1..))]
    a_movetime_ms: Option<u64>,
    #[arg(long = "a-base-ms", value_parser = clap::value_parser!(u64).range(1..))]
    a_base_ms: Option<u64>,
    #[arg(long = "a-increment-ms")]
    a_increment_ms: Option<u64>,
    #[arg(long = "a-nodes", value_parser = clap::value_parser!(u64).range(1..))]
    a_nodes: Option<u64>,
    #[arg(long = "a-depth", value_parser = clap::value_parser!(u32).range(1..))]
    a_depth: Option<u32>,
    #[arg(long = "a-margin-ms", default_value_t = match_runner::DEFAULT_MARGIN_MS)]
    a_margin_ms: u64,

    #[arg(long = "b-label")]
    b_label: Option<String>,
    #[arg(long = "b-engine-arg", allow_hyphen_values = true)]
    b_arguments: Vec<OsString>,
    #[arg(long = "b-cwd")]
    b_cwd: Option<PathBuf>,
    #[arg(long = "b-env", value_name = "KEY=VALUE")]
    b_environment: Vec<String>,
    #[arg(long = "b-option", value_name = "NAME=VALUE")]
    b_options: Vec<String>,
    #[arg(long = "b-button", value_name = "NAME")]
    b_buttons: Vec<String>,
    #[arg(long = "b-cores", value_name = "LIST")]
    b_cores: Option<String>,
    #[arg(long = "b-movetime-ms", value_parser = clap::value_parser!(u64).range(1..))]
    b_movetime_ms: Option<u64>,
    #[arg(long = "b-base-ms", value_parser = clap::value_parser!(u64).range(1..))]
    b_base_ms: Option<u64>,
    #[arg(long = "b-increment-ms")]
    b_increment_ms: Option<u64>,
    #[arg(long = "b-nodes", value_parser = clap::value_parser!(u64).range(1..))]
    b_nodes: Option<u64>,
    #[arg(long = "b-depth", value_parser = clap::value_parser!(u32).range(1..))]
    b_depth: Option<u32>,
    #[arg(long = "b-margin-ms", default_value_t = match_runner::DEFAULT_MARGIN_MS)]
    b_margin_ms: u64,

    /// Disable the default conservative draw adjudication.
    #[arg(long)]
    no_draw_adjudication: bool,
    #[arg(long, default_value_t = 40, value_parser = clap::value_parser!(u32).range(1..))]
    draw_move: u32,
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..))]
    draw_moves: u32,
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(i32).range(0..))]
    draw_score_cp: i32,

    /// Disable the default two-sided resignation adjudication.
    #[arg(long)]
    no_resign_adjudication: bool,
    #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
    resign_moves: u32,
    #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(i32).range(1..))]
    resign_score_cp: i32,

    /// Draw after this many full moves; omitted means no maximum-move cap.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    max_moves: Option<u32>,

    /// Invalidate after more engine-attributable faults than this value.
    #[arg(long, default_value_t = 0)]
    max_engine_faults: u32,
    /// Invalidate after more time losses than this value.
    #[arg(long, default_value_t = 0)]
    max_time_losses: u32,

    /// Number of games allowed to run at once.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    concurrency: u32,
    /// Physical cores allocated separately to each engine in each game slot.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    cores_per_engine: u32,
    /// CPU placement: off, auto, or an explicit logical CPU list.
    #[arg(long, default_value = "off")]
    placement: String,
    /// Whole physical cores left free when placement is auto.
    #[arg(long, default_value_t = 2)]
    headroom_cores: usize,
    /// Trusted hard budget for the two engines' configured Hash memory.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    memory_budget_mb: Option<u64>,

    /// Optional EPD or PGN opening book; format is inferred from the extension.
    #[arg(long)]
    book: Option<PathBuf>,
    /// Opening order within the book.
    #[arg(long, value_enum, default_value_t = BookOrderArg::Sequential)]
    book_order: BookOrderArg,
    /// Zero-based first opening after ordering.
    #[arg(long, default_value_t = 0)]
    book_start: usize,
    /// PGN half-moves to pre-play; EPD positions ignore this value.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    book_plies: Option<u32>,
    /// Master seed for every random choice; generated and reported when omitted.
    #[arg(long)]
    seed: Option<u64>,

    /// Self-contained run directory; an existing matching directory resumes.
    #[arg(long = "dir")]
    run_directory: Option<PathBuf>,
    /// Archive an existing --dir and start a fresh run there.
    #[arg(long, requires = "run_directory")]
    restart: bool,
    /// Seconds between live progress reports on standard error.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    progress_interval_secs: u64,
}

#[derive(Debug, Args)]
struct SprtCommand {
    /// Required finite cap; reaching it without a boundary is inconclusive.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    max_pairs: u32,
    /// Named starting design; every field remains overridable.
    #[arg(long, value_enum)]
    preset: Option<SprtBundleArg>,
    /// Elo parameterization used by both hypotheses and the LLR.
    #[arg(long, value_enum)]
    model: Option<SprtModelArg>,
    /// Null-hypothesis Elo in the selected model.
    #[arg(long, allow_hyphen_values = true)]
    elo0: Option<f64>,
    /// Alternative-hypothesis Elo in the selected model.
    #[arg(long, allow_hyphen_values = true)]
    elo1: Option<f64>,
    /// Type-I error probability.
    #[arg(long)]
    alpha: Option<f64>,
    /// Type-II error probability.
    #[arg(long)]
    beta: Option<f64>,

    #[command(flatten)]
    conditions: MatchConditions,
}

#[derive(Debug, Args)]
struct CalibrationCommand {
    /// Complete games to measure; it must be even so every sample is colour-paired.
    #[arg(long, default_value_t = DEFAULT_CALIBRATION_GAMES)]
    games: u32,
    /// Two-sided confidence level for the normalized-Elo interval.
    #[arg(long, default_value_t = DEFAULT_CALIBRATION_CONFIDENCE)]
    confidence: f64,
    /// Inclusive normalized-Elo interval tolerance around zero.
    #[arg(long, default_value_t = DEFAULT_CALIBRATION_TOLERANCE_NELO)]
    tolerance_nelo: f64,

    #[command(flatten)]
    conditions: MatchConditions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum SprtBundleArg {
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
enum SprtModelArg {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum BookOrderArg {
    Sequential,
    Random,
}

#[derive(Debug, Args)]
struct EngineCommand {
    #[command(subcommand)]
    command: EngineAction,
}

#[derive(Debug, Subcommand)]
enum EngineAction {
    /// Print UCI identity and the option schema advertised during handshake.
    Inspect(EngineInvocation),
    /// Run the bounded UCI compliance report.
    Check(EngineInvocation),
}

#[derive(Debug, Args)]
struct EngineInvocation {
    #[command(flatten)]
    engine: EngineArgs,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Capabilities if cli.dry_run => unsupported_dry_run("capabilities"),
        Command::Capabilities => run_capabilities(cli.json),
        Command::Match(command) => run_match(*command, cli.json, cli.dry_run).await,
        Command::Sprt(command) => run_sprt(*command, cli.json, cli.dry_run).await,
        Command::Calibrate(command) => run_calibration(*command, cli.json, cli.dry_run).await,
        Command::Engine(command) => run_engine(command.command, cli.json, cli.dry_run).await,
        Command::SelfTest if cli.dry_run => unsupported_dry_run("self-test"),
        Command::SelfTest => run_self_test(cli.json).await,
        Command::Status { .. } if cli.dry_run => unsupported_dry_run("status"),
        Command::Status { run_directory } => run_status(&run_directory, cli.json),
        Command::UciStub(args) => match uci_stub::run(args).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("internal UCI stub failed: {error}");
                ExitCode::FAILURE
            }
        },
    }
}

async fn run_sprt(command: SprtCommand, machine: bool, dry_run: bool) -> ExitCode {
    let design = match resolve_sprt_design(&command) {
        Ok(design) => design,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
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
    let engine_a = match resolve_match_engine(
        command.engine_a,
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
    let engine_b = match resolve_match_engine(
        command.engine_b,
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
        command.cores_per_engine as usize,
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
    let openings =
        match match_runner::resolve_openings(book, command.book_start, games, master_seed) {
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
            "engine_a_time_control": engine_a_time_control,
            "engine_b_time_control": engine_b_time_control,
            "adjudication": adjudication,
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
        "design": design,
        "engine_a_time_control": engine_a_time_control,
        "engine_b_time_control": engine_b_time_control,
        "adjudication": adjudication,
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
            openings,
        },
        execution: execution.clone(),
        design,
        fault_policy,
        completed_pairs: checkpoint.official_pairs,
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

fn sprt_exit_code(status: sprt_runner::SprtStatus) -> u8 {
    match status {
        sprt_runner::SprtStatus::H1 => 0,
        sprt_runner::SprtStatus::H0 => 1,
        sprt_runner::SprtStatus::Inconclusive => 4,
        sprt_runner::SprtStatus::Invalid => 5,
    }
}

fn resolve_sprt_design(command: &SprtCommand) -> Result<SprtDesign, String> {
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

fn unsupported_dry_run(command: &str) -> ExitCode {
    eprintln!("configuration error: --dry-run is not meaningful for read-only {command}");
    ExitCode::from(2)
}

async fn run_engine(command: EngineAction, machine: bool, dry_run: bool) -> ExitCode {
    let invocation = match &command {
        EngineAction::Inspect(invocation) | EngineAction::Check(invocation) => invocation,
    };
    let action = match command {
        EngineAction::Inspect(_) => "engine-inspect",
        EngineAction::Check(_) => "engine-check",
    };
    let (launch, resolved) = match resolve_invocation(action, &invocation.engine) {
        Ok(resolved) => resolved,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if dry_run {
        let output = MachineOutput::DryRun {
            command: action,
            config_sha256: resolved.sha256(),
            resolved_configuration: resolved.value(),
            invocations: vec![&launch],
        };
        print_output(&output, machine);
        return ExitCode::SUCCESS;
    }
    let participant = RuntimeParticipant {
        id: ParticipantId::from_u128(1),
        launch,
    };
    match command {
        EngineAction::Inspect(_) => {
            match InspectEngine::execute(&UciSessionFactory, &participant).await {
                Ok(inspection) => {
                    if machine {
                        print_json(&MachineOutput::EngineInspection { inspection });
                    } else {
                        print_inspection(inspection);
                    }
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("engine inspect failed: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        EngineAction::Check(_) => {
            match CheckEngine::execute(&UciSessionFactory, &participant).await {
                Ok(report) => {
                    let success = report.success;
                    if machine {
                        print_json(&MachineOutput::EngineCompliance { report });
                    } else {
                        print_compliance(&report);
                    }
                    if success {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(error) => {
                    eprintln!("engine check failed: {error}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn resolve_invocation(
    action: &str,
    engine: &EngineArgs,
) -> Result<(EngineLaunchSpec, colosseum_cli::ResolvedConfig), Box<dyn std::error::Error>> {
    let launch = engine.resolve()?;
    let mut path_pointers = vec!["/engine/executable".to_owned()];
    if launch.working_directory.is_some() {
        path_pointers.push("/engine/working_directory".to_owned());
    }
    let current_directory = std::env::current_dir()?;
    let resolved = resolve_config(
        built_in_defaults(),
        None,
        json!({ "command": action, "engine": launch }),
        &[],
        Path::new(&current_directory),
        &path_pointers,
    )?;
    let launch = serde_json::from_value(
        resolved
            .value()
            .get("engine")
            .cloned()
            .expect("engine is part of the CLI layer"),
    )?;
    Ok((launch, resolved))
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
enum MachineOutput<'a> {
    Capabilities {
        report: capabilities::CapabilitiesReport,
    },
    FixedMatch {
        run_directory: PathBuf,
        report: match_runner::FixedMatchReport,
    },
    Sprt {
        run_directory: PathBuf,
        report: sprt_runner::SprtReport,
    },
    Calibration {
        run_directory: PathBuf,
        report: CalibrationReport,
    },
    DryRun {
        command: &'a str,
        config_sha256: &'a str,
        resolved_configuration: &'a Value,
        invocations: Vec<&'a EngineLaunchSpec>,
    },
    EngineInspection {
        inspection: EngineInspection,
    },
    EngineCompliance {
        report: ComplianceReport,
    },
    SelfTest {
        report: self_test::SelfTestReport,
    },
    RunStatus {
        run_directory: &'a Path,
        record: RunRecord,
    },
}

#[derive(Debug, Clone, Serialize)]
struct CalibrationReport {
    status: CalibrationStatus,
    design: CalibrationDesign,
    binaries: CalibrationBinaries,
    #[serde(skip_serializing_if = "Option::is_none")]
    interval: Option<CalibrationInterval>,
    #[serde(skip_serializing_if = "Option::is_none")]
    statistics_unavailable: Option<String>,
    fixed_match: match_runner::FixedMatchReport,
}

struct PreparedCalibration {
    design: CalibrationDesign,
    binaries: CalibrationBinaries,
    engine_a: EngineLaunchSpec,
    engine_b: EngineLaunchSpec,
    engine_a_time_control: match_runner::ConfiguredTimeControl,
    engine_b_time_control: match_runner::ConfiguredTimeControl,
    adjudication: AdjudicationConfig,
    fault_policy: match_runner::FaultPolicy,
    execution: match_runner::MatchExecutionPlan,
    master_seed: u64,
    master_seed_generated: bool,
    openings: match_runner::MatchOpenings,
    current_directory: PathBuf,
    resolved: colosseum_cli::ResolvedConfig,
}

async fn run_calibration(command: CalibrationCommand, machine: bool, dry_run: bool) -> ExitCode {
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
        "optional": true,
        "design": prepared.design,
        "binaries": prepared.binaries,
        "engine_a_time_control": prepared.engine_a_time_control,
        "engine_b_time_control": prepared.engine_b_time_control,
        "adjudication": prepared.adjudication,
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
        fault_policy: prepared.fault_policy,
        execution: prepared.execution,
        master_seed: prepared.master_seed,
        master_seed_generated: prepared.master_seed_generated,
        openings: prepared.openings,
        completed_games,
        progress: progress.clone(),
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

fn prepare_calibration(command: &CalibrationCommand) -> Result<PreparedCalibration, String> {
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
    let engine_a = resolve_match_engine(
        conditions.engine_a.clone(),
        conditions.a_label.clone(),
        conditions.a_arguments.clone(),
        conditions.a_cwd.clone(),
        conditions.a_environment.clone(),
        conditions.a_options.clone(),
        conditions.a_buttons.clone(),
        conditions.a_cores.clone(),
    )
    .map_err(|error| error.to_string())?;
    let engine_b = resolve_match_engine(
        conditions.engine_b.clone(),
        conditions.b_label.clone(),
        conditions.b_arguments.clone(),
        conditions.b_cwd.clone(),
        conditions.b_environment.clone(),
        conditions.b_options.clone(),
        conditions.b_buttons.clone(),
        conditions.b_cores.clone(),
    )
    .map_err(|error| error.to_string())?;
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
    let openings =
        match_runner::resolve_openings(book, conditions.book_start, design.games, master_seed)
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
        fault_policy,
        execution,
        master_seed,
        master_seed_generated,
        openings,
        current_directory,
        resolved,
    })
}

fn executable_sha256(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|error| {
        format!(
            "cannot hash calibration executable {}: {error}",
            path.display()
        )
    })?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn calibration_interval(
    report: &match_runner::FixedMatchReport,
    design: CalibrationDesign,
) -> Result<(Option<CalibrationInterval>, Option<String>), String> {
    let sample = calibration_sample(report)?;
    match fixed_n_achieved_resolution(&sample, EloModel::Normalized, design.significance()) {
        Ok(resolution) => Ok((Some(resolution.into()), None)),
        Err(error) => Ok((None, Some(error.to_string()))),
    }
}

fn calibration_sample(
    report: &match_runner::FixedMatchReport,
) -> Result<PentanomialVector, String> {
    if report.games.len() != report.games_requested as usize {
        return Err("calibration did not retain every requested game".into());
    }
    let mut sample = PentanomialVector::default();
    for pair in report.games.chunks_exact(2) {
        let [first, second] = pair else {
            unreachable!("chunks_exact(2) always has pairs")
        };
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

fn result_for_engine_a(white: match_runner::MatchSide, result: GameResult) -> PairGameResult {
    match (white, result) {
        (_, GameResult::Draw) => PairGameResult::Draw,
        (match_runner::MatchSide::A, GameResult::WhiteWin)
        | (match_runner::MatchSide::B, GameResult::BlackWin) => PairGameResult::Win,
        (match_runner::MatchSide::A, GameResult::BlackWin)
        | (match_runner::MatchSide::B, GameResult::WhiteWin) => PairGameResult::Loss,
    }
}

fn calibration_exit_code(status: CalibrationStatus) -> u8 {
    match status {
        CalibrationStatus::Pass => 0,
        CalibrationStatus::Fail => 1,
        CalibrationStatus::Inconclusive => 4,
        CalibrationStatus::Invalid => 5,
    }
}

async fn run_match(command: MatchCommand, machine: bool, dry_run: bool) -> ExitCode {
    let MatchCommand {
        games,
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
    let engine_a = match resolve_match_engine(
        command.engine_a,
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
    let engine_b = match resolve_match_engine(
        command.engine_b,
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
        command.cores_per_engine as usize,
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
    let openings =
        match match_runner::resolve_openings(book, command.book_start, games, master_seed) {
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
        "engine_a_time_control": engine_a_time_control,
        "engine_b_time_control": engine_b_time_control,
        "adjudication": adjudication,
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
        fault_policy,
        execution,
        master_seed,
        master_seed_generated,
        openings,
        completed_games,
        progress: progress.clone(),
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

fn read_stored_seed(root: &Path) -> Option<(u64, bool)> {
    let bytes = fs::read(root.join("resolved-config.json")).ok()?;
    let value: Value = serde_json::from_slice(&bytes).ok()?;
    Some((
        value.get("master_seed")?.as_u64()?,
        value
            .get("master_seed_generated")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    ))
}

struct DurableMatchOutput {
    directory: Arc<RunDirectory>,
    games: Mutex<Vec<match_runner::MatchGame>>,
}

impl DurableMatchOutput {
    fn new(
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

    fn rewrite_pgn(&self) -> Result<(), String> {
        let games = self.games.lock().map_err(|_| "PGN lock poisoned")?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(self.directory.paths().root.join("games.pgn"))
            .map_err(|error| error.to_string())?;
        for game in games.iter() {
            writeln!(file, "{}", game.pgn.trim_end()).map_err(|error| error.to_string())?;
            writeln!(file).map_err(|error| error.to_string())?;
        }
        file.sync_all().map_err(|error| error.to_string())
    }

    fn finish(&self, report: &match_runner::FixedMatchReport) -> Result<(), String> {
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

    fn finish_calibration(&self, report: &CalibrationReport) -> Result<(), String> {
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SprtCheckpoint {
    official_pairs: Vec<CompletePair<match_runner::MatchGame>>,
    post_terminal_pairs: Vec<CompletePair<match_runner::MatchGame>>,
}

struct DurableSprtOutput {
    directory: Arc<RunDirectory>,
    checkpoint: Mutex<SprtCheckpoint>,
}

impl DurableSprtOutput {
    fn new(directory: Arc<RunDirectory>, checkpoint: SprtCheckpoint) -> Result<Self, String> {
        let output = Self {
            directory,
            checkpoint: Mutex::new(checkpoint),
        };
        output.rewrite_pgn()?;
        Ok(output)
    }

    fn progress(&self) -> (usize, usize) {
        self.checkpoint.lock().map_or((0, 0), |checkpoint| {
            (
                checkpoint.official_pairs.len(),
                checkpoint.post_terminal_pairs.len(),
            )
        })
    }

    fn persist_pair(
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

    fn rewrite_pgn(&self) -> Result<(), String> {
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
                    writeln!(file, "{{Colosseum sample: {class}}}")
                        .map_err(|error| error.to_string())?;
                    writeln!(file, "{}\n", game.pgn.trim_end())
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        file.sync_all().map_err(|error| error.to_string())
    }

    fn finish(&self, report: &sprt_runner::SprtReport) -> Result<(), String> {
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

fn resolve_master_seed(configured: Option<u64>) -> Result<(u64, bool), String> {
    if let Some(seed) = configured {
        return Ok((seed, false));
    }
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
    Ok((u64::from_le_bytes(bytes), true))
}

fn resolve_placement(value: &str, headroom_cores: usize) -> Result<CpuPlacementPolicy, String> {
    match value {
        "off" => Ok(CpuPlacementPolicy::Off),
        "auto" => Ok(CpuPlacementPolicy::Auto {
            headroom_physical_cores: headroom_cores,
        }),
        explicit => parse_cpu_list(explicit)
            .map(|cpus| CpuPlacementPolicy::Explicit { cpus })
            .map_err(|error| error.to_string()),
    }
}

fn resolve_adjudication(command: &MatchConditions) -> AdjudicationConfig {
    AdjudicationConfig {
        max_moves: command.max_moves,
        draw: (!command.no_draw_adjudication).then_some(DrawAdjudication {
            min_ply: command.draw_move.saturating_mul(2),
            move_count: command.draw_moves,
            score_cp: command.draw_score_cp,
        }),
        resign: (!command.no_resign_adjudication).then_some(ResignAdjudication {
            move_count: command.resign_moves,
            score_cp: command.resign_score_cp,
        }),
    }
}

fn resolve_time_control(
    side: &str,
    movetime_ms: Option<u64>,
    base_ms: Option<u64>,
    increment_ms: Option<u64>,
    nodes: Option<u64>,
    depth: Option<u32>,
    margin_ms: u64,
) -> Result<match_runner::ConfiguredTimeControl, String> {
    let selected = usize::from(movetime_ms.is_some())
        + usize::from(base_ms.is_some())
        + usize::from(nodes.is_some())
        + usize::from(depth.is_some());
    if selected > 1 {
        return Err(format!(
            "{side} must select only one of movetime, base/increment, nodes or depth"
        ));
    }
    if increment_ms.is_some() && base_ms.is_none() {
        return Err(format!("{side} increment requires a base time"));
    }
    let control = if let Some(ms) = movetime_ms {
        TimeControl::PerMove { ms }
    } else if let Some(base_ms) = base_ms {
        match increment_ms {
            Some(inc_ms) => TimeControl::Increment { base_ms, inc_ms },
            None => TimeControl::SuddenDeath { base_ms },
        }
    } else if let Some(nodes) = nodes {
        TimeControl::Nodes { nodes }
    } else if let Some(depth) = depth {
        TimeControl::Depth { depth }
    } else {
        TimeControl::Increment {
            base_ms: match_runner::DEFAULT_BASE_MS,
            inc_ms: match_runner::DEFAULT_INCREMENT_MS,
        }
    };
    Ok(match_runner::ConfiguredTimeControl { control, margin_ms })
}

#[allow(clippy::too_many_arguments)]
fn resolve_match_engine(
    executable: PathBuf,
    label: Option<String>,
    arguments: Vec<OsString>,
    cwd: Option<PathBuf>,
    environment: Vec<String>,
    options: Vec<String>,
    buttons: Vec<String>,
    cores: Option<String>,
) -> Result<EngineLaunchSpec, colosseum_cli::EngineArgsError> {
    EngineArgs {
        executable,
        label,
        arguments,
        cwd,
        environment,
        options,
        buttons,
        cores,
    }
    .resolve()
}

fn print_fixed_match(report: &match_runner::FixedMatchReport) {
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

fn print_sprt(report: &sprt_runner::SprtReport, run_directory: &Path) {
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

fn print_calibration(report: &CalibrationReport, run_directory: &Path) {
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

fn run_capabilities(machine: bool) -> ExitCode {
    let report = capabilities::probe();
    if machine {
        print_json(&MachineOutput::Capabilities { report });
    } else {
        capabilities::print_text(&report);
    }
    ExitCode::SUCCESS
}

fn run_status(run_directory: &Path, machine: bool) -> ExitCode {
    let record = match RunRecord::read(run_directory) {
        Ok(record) => record,
        Err(error) => {
            eprintln!("status failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if machine {
        print_json(&MachineOutput::RunStatus {
            run_directory,
            record,
        });
    } else {
        println!("command: {}", record.command);
        println!("status: {:?}", record.status);
        println!("config: {}", record.config_sha256);
        println!(
            "committed units: {}",
            record.official_sample.committed_units
        );
        println!("scored games: {}", record.official_sample.scored_games);
        println!("anomalies: {}", record.anomalies.len());
    }
    ExitCode::SUCCESS
}

async fn run_self_test(machine: bool) -> ExitCode {
    let report = self_test::execute().await;
    let success = report.success;
    if machine {
        print_json(&MachineOutput::SelfTest { report });
    } else {
        for check in &report.checks {
            let status = if check.success { "PASS" } else { "FAIL" };
            println!("[{status}] {} — {}", check.name, check.detail);
        }
    }
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_output(output: &MachineOutput<'_>, machine: bool) {
    if machine {
        print_json(output);
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(output).expect("machine output is serializable")
        );
    }
}

fn print_json(output: &MachineOutput<'_>) {
    println!(
        "{}",
        serde_json::to_string(output).expect("machine output is serializable")
    );
}

fn print_inspection(inspection: EngineInspection) {
    println!(
        "name: {}",
        inspection.name.as_deref().unwrap_or("<not reported>")
    );
    println!(
        "author: {}",
        inspection.author.as_deref().unwrap_or("<not reported>")
    );
    println!("options: {}", inspection.options.len());
    for option in inspection.options {
        println!("  {}", describe_option(&option));
    }
}

fn print_compliance(report: &ComplianceReport) {
    for check in &report.checks {
        let status = match check.status {
            ComplianceStatus::Pass => "PASS",
            ComplianceStatus::Fail => "FAIL",
            ComplianceStatus::Skipped => "SKIP",
        };
        println!("[{status}] {} — {}", check.requirement, check.detail);
    }
}

fn describe_option(option: &UciOptionSchema) -> String {
    match option {
        UciOptionSchema::Check { name, default } => {
            format!("{name}: check (default {default})")
        }
        UciOptionSchema::Spin {
            name,
            default,
            min,
            max,
        } => format!("{name}: spin (default {default}, range {min}..={max})"),
        UciOptionSchema::Combo {
            name,
            default,
            values,
        } => format!(
            "{name}: combo (default {default}, values {})",
            values.join(", ")
        ),
        UciOptionSchema::Button { name } => format!("{name}: button"),
        UciOptionSchema::String { name, default } => {
            format!("{name}: string (default {default:?})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_control_resolution_covers_every_supported_mode_and_default() {
        let movetime = resolve_time_control("A", Some(25), None, None, None, None, 5).unwrap();
        assert_eq!(movetime.control, TimeControl::PerMove { ms: 25 });
        let sudden = resolve_time_control("A", None, Some(500), None, None, None, 5).unwrap();
        assert_eq!(sudden.control, TimeControl::SuddenDeath { base_ms: 500 });
        let depth = resolve_time_control("A", None, None, None, None, Some(12), 5).unwrap();
        assert_eq!(depth.control, TimeControl::Depth { depth: 12 });
        let default = resolve_time_control("A", None, None, None, None, None, 5).unwrap();
        assert_eq!(
            default.control,
            TimeControl::Increment {
                base_ms: match_runner::DEFAULT_BASE_MS,
                inc_ms: match_runner::DEFAULT_INCREMENT_MS,
            }
        );
    }

    #[test]
    fn sprt_terminal_classes_have_distinct_automation_exit_codes() {
        assert_eq!(sprt_exit_code(sprt_runner::SprtStatus::H1), 0);
        assert_eq!(sprt_exit_code(sprt_runner::SprtStatus::H0), 1);
        assert_eq!(sprt_exit_code(sprt_runner::SprtStatus::Inconclusive), 4);
        assert_eq!(sprt_exit_code(sprt_runner::SprtStatus::Invalid), 5);
    }
}
