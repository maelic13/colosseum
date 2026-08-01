//! Independent headless composition root for Colosseum CLI.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};
use colosseum_application::{
    CheckEngine, ComplianceReport, ComplianceStatus, EngineInspection, EngineLaunchSpec,
    InspectEngine, RuntimeParticipant, UciOptionSchema,
};
use colosseum_cli::{EngineArgs, RunRecord, built_in_defaults, parse_cpu_list, resolve_config};
use colosseum_core::{
    AdjudicationConfig, DrawAdjudication, OpeningBook, OpeningOrder, ParticipantId,
    ResignAdjudication, TimeControl,
};
use colosseum_engine::CpuPlacementPolicy;
use colosseum_uci::UciSessionFactory;
use serde::Serialize;
use serde_json::{Value, json};

mod capabilities;
mod match_runner;
mod self_test;
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
        report: match_runner::FixedMatchReport,
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

async fn run_match(command: MatchCommand, machine: bool, dry_run: bool) -> ExitCode {
    if command.book.is_none()
        && (command.book_start != 0
            || command.book_plies.is_some()
            || command.book_order != BookOrderArg::Sequential)
    {
        eprintln!("configuration error: book order/start/plies require --book");
        return ExitCode::from(2);
    }
    let (master_seed, master_seed_generated) = match resolve_master_seed(command.seed) {
        Ok(seed) => seed,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
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
    let openings = match match_runner::resolve_openings(
        book,
        command.book_start,
        command.games,
        master_seed,
    ) {
        Ok(openings) => openings,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if dry_run {
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
                "games": command.games,
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
        let output = MachineOutput::DryRun {
            command: "match",
            config_sha256: resolved.sha256(),
            resolved_configuration: resolved.value(),
            invocations: vec![&engine_a, &engine_b],
        };
        print_output(&output, machine);
        return ExitCode::SUCCESS;
    }
    match match_runner::run_fixed_match(match_runner::FixedMatchRequest {
        engine_a,
        engine_b,
        games: command.games,
        engine_a_time_control,
        engine_b_time_control,
        adjudication,
        fault_policy,
        execution,
        master_seed,
        master_seed_generated,
        openings,
    })
    .await
    {
        Ok(report) => {
            let success = report.status == match_runner::MatchStatus::Completed;
            if machine {
                print_json(&MachineOutput::FixedMatch { report });
            } else {
                print_fixed_match(&report);
            }
            if success {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            eprintln!("match failed: {error}");
            ExitCode::FAILURE
        }
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

fn resolve_adjudication(command: &MatchCommand) -> AdjudicationConfig {
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
}
