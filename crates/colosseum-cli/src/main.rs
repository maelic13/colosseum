//! Independent headless composition root for Colosseum CLI.

use std::collections::BTreeMap;
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
    CompareNps, CompletePair, ComplianceReport, ComplianceStatus, DEFAULT_CALIBRATION_CONFIDENCE,
    DEFAULT_CALIBRATION_GAMES, DEFAULT_CALIBRATION_TOLERANCE_NELO,
    DEFAULT_SPSA_FINAL_WINDOW_PERCENT, DEFAULT_SPSA_GAMES_PER_ITERATION, DEFAULT_SPSA_ITERATIONS,
    EngineInspection, EngineLaunchSpec, InspectEngine, MeasureNps, NpsExperimentDesign,
    NpsExperimentParticipant, NpsExperimentReport, NpsHashPolicy, NpsReport, NpsRequest,
    NpsScalingInput, NpsScalingReport, NpsStatePolicy, RuntimeParticipant, SprtBundle, SprtDesign,
    SprtParameters, SpsaBoundTune, SpsaCenterSample, SpsaCommittedUpdate, SpsaGateHashStatus,
    SpsaPlanReport, SpsaRunSettings, SpsaStatusReport, SpsaTimingInput, SpsaTuneAudit,
    SpsaTuneResult, SpsaTuneWarning, SpsaTuningState, UciOptionSchema, UciOptionValue,
    classify_calibration, diagnose_spsa, plan_spsa, summarize_nps_scaling,
};
use colosseum_cli::{
    EngineArgs, OfficialSample, RunDirectory, RunRecord, RunRecorder, RunStatus, built_in_defaults,
    load_spsa_tune, parse_cpu_list, persist_and_verify_spsa_schedule,
    read_and_verify_spsa_schedule, resolve_config,
};
use colosseum_core::{
    AdjudicationConfig, DrawAdjudication, EloModel, GameResult, OpeningBook, OpeningFormat,
    OpeningOrder, PairGameResult, ParticipantId, PentanomialVector, ResignAdjudication,
    SpsaEndSpec, SpsaScheduleArtifact, TimeControl, fixed_n_achieved_resolution,
};
use colosseum_engine::{
    CpuPlacementPlan, CpuPlacementPolicy, audit_opening_book, detect_allowed_cpu_set,
    detect_cpu_characteristics, detect_cpu_topology, fen_after, load_openings_named,
    plan_cpu_placement,
};
use colosseum_uci::{AffinityUciSessionFactory, UciSessionFactory};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

mod capabilities;
mod match_runner;
mod self_test;
mod sprt_runner;
mod spsa_driver;
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
    /// Tune numeric UCI options with durable pair-atomic SPSA.
    Spsa(Box<SpsaCommand>),
    /// Measure fixed-node search speed using harness monotonic wall time.
    Nps(Box<NpsCommand>),
    /// Measure identical-binary symmetry under representative match conditions.
    Calibrate(Box<CalibrationCommand>),
    /// Inspect or compliance-check an ordinary UCI executable.
    Engine(EngineCommand),
    /// Inspect, verify, hash or deterministically slice an opening book.
    Book(BookCommand),
    /// Replay match statistics from the strongest available evidence source.
    Stats(StatsCommand),
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
    /// Path to engine A's UCI executable.
    engine_a: PathBuf,
    /// Path to engine B's UCI executable.
    engine_b: PathBuf,

    /// Exact number of games to play; this command never stops early.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    games: u32,

    #[command(flatten)]
    conditions: MatchConditions,
}

#[derive(Debug, Args)]
struct MatchConditions {
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
    /// Path to engine A; omit both engine paths when using --apply.
    engine_a: Option<PathBuf>,
    /// Path to engine B; omit both engine paths when using --apply.
    engine_b: Option<PathBuf>,
    /// Completed SPSA result.json whose original/tuned vectors form the gate.
    #[arg(long, value_name = "RESULT_JSON")]
    apply: Option<PathBuf>,
    /// Use this executable instead of the path recorded by the SPSA result.
    #[arg(long, requires = "apply", value_name = "EXECUTABLE")]
    apply_executable: Option<PathBuf>,
    /// Proceed with --apply despite an executable SHA-256 mismatch.
    #[arg(long, requires = "apply")]
    allow_executable_mismatch: bool,

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
    /// Path to the first copy of the representative executable.
    engine_a: PathBuf,
    /// Path to the second copy of the representative executable.
    engine_b: PathBuf,

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

#[derive(Debug, Args)]
#[command(subcommand_precedence_over_arg = true, subcommand_negates_reqs = true)]
struct SpsaCommand {
    #[command(subcommand)]
    action: Option<SpsaAction>,

    #[command(flatten)]
    engine: SpsaEngineArgs,

    /// Ordered TOML parameter vector to tune against the live UCI schema.
    #[arg(long)]
    tune: Option<PathBuf>,
    /// Terminal SPSA gain ratio shared by every tuned parameter.
    #[arg(long)]
    r_end: Option<f64>,
    /// Number of SPSA centre updates in the tune horizon.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    iterations: Option<u32>,
    /// Complete games in each pair-atomic mini-match.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    games_per_iteration: Option<u32>,
    /// Percent of the fixed horizon averaged into the tuned result.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..=100))]
    final_window_percent: Option<u32>,

    #[command(flatten)]
    conditions: SpsaConditions,
}

/// SPSA supports read-only nested commands that need no executable. The live
/// run path validates the executable before resolving the shared engine args.
#[derive(Debug, Args)]
struct SpsaEngineArgs {
    /// Path to the UCI engine executable; omitted only for a nested read-only command.
    executable: Option<PathBuf>,
    #[arg(long)]
    label: Option<String>,
    #[arg(long = "engine-arg", allow_hyphen_values = true)]
    arguments: Vec<OsString>,
    #[arg(long)]
    cwd: Option<PathBuf>,
    #[arg(long = "env", value_name = "KEY=VALUE")]
    environment: Vec<String>,
    #[arg(long = "option", value_name = "NAME=VALUE")]
    options: Vec<String>,
    #[arg(long = "button", value_name = "NAME")]
    buttons: Vec<String>,
    #[arg(long, value_name = "LIST")]
    cores: Option<String>,
}

impl SpsaEngineArgs {
    fn resolve(&self) -> Result<EngineLaunchSpec, String> {
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
enum SpsaAction {
    /// Report the exact gain schedule and factual workload without launching an engine.
    Plan(SpsaPlanCommand),
    /// Read the last durable tune snapshot and report labelled trajectory heuristics.
    Status {
        /// Self-contained SPSA run directory to inspect without mutation.
        run_directory: PathBuf,
    },
}

#[derive(Debug, Args)]
struct SpsaPlanCommand {
    /// Ordered TOML parameter vector whose schedule should be planned.
    #[arg(long)]
    tune: PathBuf,
    /// Terminal SPSA gain ratio shared by every tuned parameter.
    #[arg(long)]
    r_end: f64,
    /// Number of SPSA centre updates in the primary horizon.
    #[arg(long, default_value_t = DEFAULT_SPSA_ITERATIONS, value_parser = clap::value_parser!(u32).range(1..))]
    iterations: u32,
    /// Complete games in each pair-atomic mini-match.
    #[arg(long, default_value_t = DEFAULT_SPSA_GAMES_PER_ITERATION, value_parser = clap::value_parser!(u32).range(1..))]
    games_per_iteration: u32,
    /// Number of games expected to run concurrently within each iteration.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    concurrency: u32,
    /// Compare cost and first/final gain values at another horizon; repeatable.
    #[arg(long = "compare-iterations", value_parser = clap::value_parser!(u32).range(1..))]
    comparison_horizons: Vec<u32>,
    /// Lower end-to-end seconds-per-game assumption for a wall-time range.
    #[arg(
        long,
        requires = "seconds_per_game_high",
        conflicts_with = "pilot_game_seconds",
        value_parser = parse_positive_seconds
    )]
    seconds_per_game_low: Option<f64>,
    /// Upper end-to-end seconds-per-game assumption for a wall-time range.
    #[arg(
        long,
        requires = "seconds_per_game_low",
        conflicts_with = "pilot_game_seconds",
        value_parser = parse_positive_seconds
    )]
    seconds_per_game_high: Option<f64>,
    /// Observed end-to-end duration of one pilot game; repeat for a sample range.
    #[arg(long, value_parser = parse_positive_seconds)]
    pilot_game_seconds: Vec<f64>,
}

#[derive(Debug, Args)]
struct SpsaConditions {
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    movetime_ms: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    base_ms: Option<u64>,
    #[arg(long)]
    increment_ms: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    nodes: Option<u64>,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    depth: Option<u32>,
    #[arg(long, default_value_t = match_runner::DEFAULT_MARGIN_MS)]
    margin_ms: u64,

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
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    max_moves: Option<u32>,

    /// Number of games allowed to run at once.
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    concurrency: u32,
    #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u32).range(1..))]
    cores_per_engine: u32,
    #[arg(long, default_value = "off")]
    placement: String,
    #[arg(long, default_value_t = 2)]
    headroom_cores: usize,
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    memory_budget_mb: Option<u64>,

    /// Optional EPD or PGN opening book; it is parsed once per process session.
    #[arg(long)]
    book: Option<PathBuf>,
    #[arg(long, value_enum, default_value_t = BookOrderArg::Sequential)]
    book_order: BookOrderArg,
    #[arg(long, default_value_t = 0)]
    book_start: usize,
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    book_plies: Option<u32>,
    #[arg(long)]
    seed: Option<u64>,

    #[arg(long = "dir")]
    run_directory: Option<PathBuf>,
    #[arg(long, requires = "run_directory")]
    restart: bool,
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..))]
    progress_interval_secs: u64,
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

#[derive(Debug, Args)]
struct BookCommand {
    #[command(subcommand)]
    action: BookAction,
}

#[derive(Debug, Args)]
struct StatsCommand {
    /// Run directory or structured JSON, PGN, log, or console-text file.
    input: PathBuf,
    /// Engine name used as the perspective for PGN replay.
    #[arg(long)]
    subject: Option<String>,
}

#[derive(Debug, Subcommand)]
enum BookAction {
    /// Write a deterministic canonical EPD subset.
    Slice(BookSliceCommand),
    /// Compute the SHA-256 of the exact input bytes.
    Hash(BookInput),
    /// Report parsed entry, uniqueness and ply statistics.
    Stats(BookInput),
    /// Strictly account for every candidate and reject malformed entries.
    Verify(BookInput),
}

#[derive(Debug, Clone, Args)]
struct BookInput {
    /// EPD or PGN input path.
    input: PathBuf,
    /// Override format detection from the file extension.
    #[arg(long, value_enum)]
    format: Option<BookFormatArg>,
    /// PGN half-moves retained per game.
    #[arg(long, default_value_t = 8, value_parser = clap::value_parser!(u32).range(1..))]
    plies: u32,
}

#[derive(Debug, Args)]
struct BookSliceCommand {
    #[command(flatten)]
    book: BookInput,
    /// Canonical EPD output path.
    output: PathBuf,
    /// Number of ordered entries skipped before writing.
    #[arg(long, default_value_t = 0)]
    start: usize,
    /// Maximum entries written.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    count: u64,
    /// Sequential file order or named-stream random order.
    #[arg(long, value_enum, default_value_t = BookOrderArg::Sequential)]
    order: BookOrderArg,
    /// Master seed used by random order.
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// Permit replacing an existing output file.
    #[arg(long)]
    force: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum BookFormatArg {
    Epd,
    Pgn,
}

impl From<BookFormatArg> for OpeningFormat {
    fn from(value: BookFormatArg) -> Self {
        match value {
            BookFormatArg::Epd => Self::Epd,
            BookFormatArg::Pgn => Self::Pgn,
        }
    }
}

#[derive(Debug, Args)]
struct NpsCommand {
    #[command(flatten)]
    engine: EngineArgs,
    /// Fixed number of nodes requested from the engine.
    #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
    nodes: u64,
    /// Primary executable for arm B; enables A/B comparison.
    #[arg(long)]
    against: Option<PathBuf>,
    /// Additional executable in arm A; repeat for pooled builds.
    #[arg(long = "a-build")]
    a_builds: Vec<PathBuf>,
    /// Additional executable in arm B; repeat for pooled builds.
    #[arg(long = "b-build")]
    b_builds: Vec<PathBuf>,
    /// Compare the primary executable with itself instead of using --against.
    #[arg(long, conflicts_with = "against")]
    self_pair: bool,
    /// UCI FEN to search; repeat for a suite. Omit to use startpos.
    #[arg(long)]
    positions: Vec<String>,
    /// Move following the selected position; repeat to provide a move list.
    #[arg(long = "move")]
    moves: Vec<String>,
    /// Measured repetitions of the complete position/build schedule.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u32).range(1..))]
    repetitions: u32,
    /// Unreported repetitions run before measurement.
    #[arg(long, default_value_t = 1)]
    warmup: u32,
    /// Restart per sample (cold) or retain each engine process (warm).
    #[arg(long, value_enum, default_value_t = NpsStateArg::Warm)]
    state: NpsStateArg,
    /// Master seed for position, pair and warm-up scheduling.
    #[arg(long, default_value_t = 0)]
    seed: u64,
    /// Bootstrap resamples used for each arm's median confidence interval.
    #[arg(long, default_value_t = 10_000, value_parser = clap::value_parser!(u32).range(1..))]
    bootstrap_samples: u32,
    /// Maximum absolute self-pair median difference before warning.
    #[arg(long, default_value_t = 0.5)]
    self_tolerance_percent: f64,
    /// Comma-separated search-thread counts; enables a pinned scaling sweep.
    #[arg(long, value_name = "1,2,4,8")]
    scale_threads: Option<String>,
    /// Advertised UCI spin option controlling engine search threads.
    #[arg(long, requires = "scale_threads")]
    threads_option: Option<String>,
    /// Hash allocation rule used by a scaling sweep.
    #[arg(long, value_enum, default_value_t = NpsHashPolicyArg::FixedTotal)]
    hash_policy: NpsHashPolicyArg,
    /// Hash MiB: total under fixed-total, or per thread under per-thread.
    #[arg(long, default_value_t = 16, value_parser = clap::value_parser!(u64).range(1..))]
    hash_mb: u64,
    /// Advertised UCI spin option controlling hash size.
    #[arg(long, default_value = "Hash")]
    hash_option: String,
    /// Maximum wall time allowed for the fixed-node search.
    #[arg(long, default_value_t = 60_000, value_parser = clap::value_parser!(u64).range(1..))]
    deadline_ms: u64,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum NpsStateArg {
    Cold,
    Warm,
}

impl From<NpsStateArg> for NpsStatePolicy {
    fn from(value: NpsStateArg) -> Self {
        match value {
            NpsStateArg::Cold => Self::Cold,
            NpsStateArg::Warm => Self::Warm,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum NpsHashPolicyArg {
    FixedTotal,
    PerThread,
}

impl From<NpsHashPolicyArg> for NpsHashPolicy {
    fn from(value: NpsHashPolicyArg) -> Self {
        match value {
            NpsHashPolicyArg::FixedTotal => Self::FixedTotal,
            NpsHashPolicyArg::PerThread => Self::PerThread,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    if let Err(error) = colosseum_uci::install_process_tree_guard() {
        eprintln!("infrastructure error: cannot install process-tree guard: {error}");
        return ExitCode::from(3);
    }
    let cli = Cli::parse();
    match cli.command {
        Command::Capabilities if cli.dry_run => unsupported_dry_run("capabilities"),
        Command::Capabilities => run_capabilities(cli.json),
        Command::Match(command) => run_match(*command, cli.json, cli.dry_run).await,
        Command::Sprt(command) => run_sprt(*command, cli.json, cli.dry_run).await,
        Command::Spsa(command) => {
            let mut command = *command;
            match command.action.take() {
                Some(SpsaAction::Plan(_)) if cli.dry_run => unsupported_dry_run("spsa plan"),
                Some(SpsaAction::Plan(plan)) => run_spsa_plan(plan, cli.json),
                Some(SpsaAction::Status { .. }) if cli.dry_run => {
                    unsupported_dry_run("spsa status")
                }
                Some(SpsaAction::Status { run_directory }) => {
                    run_spsa_status(&run_directory, cli.json)
                }
                None => run_spsa_command(command, cli.json, cli.dry_run).await,
            }
        }
        Command::Nps(command) => run_nps(*command, cli.json, cli.dry_run).await,
        Command::Calibrate(command) => run_calibration(*command, cli.json, cli.dry_run).await,
        Command::Engine(command) => run_engine(command.command, cli.json, cli.dry_run).await,
        Command::Book(_) if cli.dry_run => unsupported_dry_run("book"),
        Command::Book(command) => run_book(command.action, cli.json),
        Command::Stats(_) if cli.dry_run => unsupported_dry_run("stats"),
        Command::Stats(command) => run_stats(command, cli.json),
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

#[derive(Deserialize)]
struct SpsaApplySource {
    engine: EngineLaunchSpec,
    engine_sha256: String,
    tuned_result: SpsaTuneResult,
}

fn match_engine_overrides_requested(conditions: &MatchConditions) -> bool {
    conditions.a_label.is_some()
        || !conditions.a_arguments.is_empty()
        || conditions.a_cwd.is_some()
        || !conditions.a_environment.is_empty()
        || !conditions.a_options.is_empty()
        || !conditions.a_buttons.is_empty()
        || conditions.a_cores.is_some()
        || conditions.b_label.is_some()
        || !conditions.b_arguments.is_empty()
        || conditions.b_cwd.is_some()
        || !conditions.b_environment.is_empty()
        || !conditions.b_options.is_empty()
        || !conditions.b_buttons.is_empty()
        || conditions.b_cores.is_some()
}

fn load_spsa_apply(
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

async fn run_sprt(command: SprtCommand, machine: bool, dry_run: bool) -> ExitCode {
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
    let (engine_a, engine_b, apply_record) = if let Some(apply_path) = apply_path {
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
            "apply": &apply_record,
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
        "apply": &apply_record,
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

fn parse_positive_seconds(value: &str) -> Result<f64, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("expected a finite positive number of seconds, got {value}"))?;
    if parsed.is_finite() && parsed > 0.0 {
        Ok(parsed)
    } else {
        Err(format!(
            "expected a finite positive number of seconds, got {value}"
        ))
    }
}

fn run_spsa_plan(command: SpsaPlanCommand, machine: bool) -> ExitCode {
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

fn print_spsa_plan(report: &SpsaPlanReport) {
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
struct StoredSpsaWorkflow {
    settings: SpsaRunSettings,
    final_window_percent: u32,
    bound_tune: SpsaBoundTune,
    engine_sha256: String,
    schedule: SpsaScheduleArtifact,
}

#[derive(Debug, Serialize)]
struct SpsaStatusOutput {
    run_status: RunStatus,
    diagnostics: SpsaStatusReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_result: Option<SpsaTuneResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    candidate_unavailable_reason: Option<String>,
    snapshot_authority: String,
}

fn run_spsa_status(run_directory: &Path, machine: bool) -> ExitCode {
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
            workflow.final_window_percent,
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

fn print_spsa_status(report: &SpsaStatusOutput, run_directory: &Path) {
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

async fn run_spsa_command(command: SpsaCommand, machine: bool, dry_run: bool) -> ExitCode {
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
            "resuming the stored SPSA horizon: {} iterations, {} games per iteration, r_end {}, final window {}%",
            stored.iterations, stored.games_per_iteration, stored_r_end, stored_window
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
            command
                .final_window_percent
                .unwrap_or(DEFAULT_SPSA_FINAL_WINDOW_PERCENT),
        )
    };
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
    let adjudication = AdjudicationConfig {
        max_moves: conditions.max_moves,
        draw: (!conditions.no_draw_adjudication).then_some(DrawAdjudication {
            min_ply: conditions.draw_move.saturating_mul(2),
            move_count: conditions.draw_moves,
            score_cp: conditions.draw_score_cp,
        }),
        resign: (!conditions.no_resign_adjudication).then_some(ResignAdjudication {
            move_count: conditions.resign_moves,
            score_cp: conditions.resign_score_cp,
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
        conditions.cores_per_engine as usize,
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
    let openings =
        match match_runner::resolve_openings(book, conditions.book_start, total_games, master_seed)
        {
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
        "settings": settings,
        "r_end": r_end,
        "final_window_percent": final_window_percent,
        "bound_tune": &bound_tune,
        "tune_audit": &tune_audit,
        "engine_sha256": &engine_sha256,
        "schedule": verified_schedule.artifact(),
        "engine_time_control": engine_time_control,
        "adjudication": adjudication,
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
            openings: openings.clone(),
        },
        execution: execution.clone(),
        checkpoint,
        progress: progress.clone(),
        observer: Some(observer.clone()),
    };
    let driver_future = spsa_driver::run_spsa(driver_request);
    tokio::pin!(driver_future);
    let period = Duration::from_secs(conditions.progress_interval_secs);
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    let outcome = loop {
        tokio::select! {
            result = &mut driver_future => break result,
            _ = interval.tick() => {
                let snapshot = progress.snapshot();
                eprintln!(
                    "SPSA progress: {}/{} committed iterations, {} complete pairs played",
                    snapshot.completed_iterations,
                    settings.iterations,
                    snapshot.completed_pairs
                );
            }
        }
    };
    let driver = match outcome {
        Ok(report) => report,
        Err(error) => {
            eprintln!("SPSA failed: {error}");
            return ExitCode::from(3);
        }
    };
    let status = driver.status;
    let tuned_result = if status == spsa_driver::SpsaStatus::Completed {
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
            final_window_percent,
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
        spsa_driver::SpsaStatus::Invalid => ExitCode::from(5),
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

#[derive(Debug, Clone, Serialize)]
struct BookHashReport {
    path: PathBuf,
    bytes: u64,
    sha256: String,
}

#[derive(Debug, Clone, Serialize)]
struct BookStatsReport {
    path: PathBuf,
    format: String,
    sha256: String,
    bytes: u64,
    candidates: usize,
    usable: usize,
    rejected: usize,
    unique: usize,
    duplicates: usize,
    min_plies: usize,
    max_plies: usize,
    mean_plies: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    eval_band: Option<BookEvalBand>,
}

#[derive(Debug, Clone, Serialize)]
struct BookEvalBand {
    samples: usize,
    unit: String,
    minimum: f64,
    mean: f64,
    maximum: f64,
}

#[derive(Debug, Clone, Serialize)]
struct BookSliceReport {
    input: PathBuf,
    output: PathBuf,
    input_sha256: String,
    output_sha256: String,
    start: usize,
    requested_count: usize,
    written: usize,
    order: String,
    seed: u64,
}

fn run_book(action: BookAction, machine: bool) -> ExitCode {
    match action {
        BookAction::Hash(input) => match hash_file_report(&input.input) {
            Ok(report) => {
                if machine {
                    print_json(&MachineOutput::BookHash { report });
                } else {
                    println!("{}  {}", report.sha256, report.path.display());
                    println!("bytes: {}", report.bytes);
                }
                ExitCode::SUCCESS
            }
            Err(error) => book_error(error),
        },
        BookAction::Verify(input) => match opening_book(&input)
            .and_then(|book| audit_opening_book(&book).map_err(|error| error.to_string()))
        {
            Ok(audit) => {
                let valid = audit.valid();
                if machine {
                    print_json(&MachineOutput::BookVerify { audit });
                } else {
                    println!(
                        "{}: {} usable of {} candidates",
                        if valid { "valid" } else { "invalid" },
                        audit.usable,
                        audit.candidates
                    );
                    if !audit.rejected_indices.is_empty() {
                        println!(
                            "rejected candidate indices: {}",
                            audit
                                .rejected_indices
                                .iter()
                                .map(usize::to_string)
                                .collect::<Vec<_>>()
                                .join(",")
                        );
                    }
                }
                if valid {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
            Err(error) => book_error(error),
        },
        BookAction::Stats(input) => match book_stats(&input) {
            Ok(report) => {
                if machine {
                    print_json(&MachineOutput::BookStats { report });
                } else {
                    println!(
                        "{}: {} usable / {} candidates; {} unique, {} duplicates",
                        report.format,
                        report.usable,
                        report.candidates,
                        report.unique,
                        report.duplicates
                    );
                    println!(
                        "plies: min {}, mean {:.3}, max {}",
                        report.min_plies, report.mean_plies, report.max_plies
                    );
                    if let Some(eval) = &report.eval_band {
                        println!(
                            "eval band ({}; {} samples): {:.3}..{:.3}, mean {:.3}",
                            eval.unit, eval.samples, eval.minimum, eval.maximum, eval.mean
                        );
                    }
                    println!("SHA-256: {}", report.sha256);
                }
                ExitCode::SUCCESS
            }
            Err(error) => book_error(error),
        },
        BookAction::Slice(command) => match slice_book(&command) {
            Ok(report) => {
                if machine {
                    print_json(&MachineOutput::BookSlice { report });
                } else {
                    println!(
                        "wrote {} canonical EPD entries to {}",
                        report.written,
                        report.output.display()
                    );
                    println!("output SHA-256: {}", report.output_sha256);
                }
                ExitCode::SUCCESS
            }
            Err(error) => book_error(error),
        },
    }
}

fn run_stats(command: StatsCommand, machine: bool) -> ExitCode {
    match colosseum_cli::stats_replay::replay(&command.input, command.subject.as_deref()) {
        Ok(report) => {
            for warning in &report.warnings {
                eprintln!("stats warning: {warning}");
            }
            if machine {
                print_json(&MachineOutput::StatsReplay { report });
            } else {
                println!(
                    "authority: {} ({})",
                    report.authority,
                    report.source.display()
                );
                println!("perspective: {}", report.perspective);
                println!(
                    "{} games: {} wins, {} draws, {} losses; score {:.6}",
                    report.games, report.wins, report.draws, report.losses, report.score
                );
                println!(
                    "pairing: {}; {} complete pairs, {} unpaired games",
                    report.pairing, report.complete_pairs, report.unpaired_games
                );
                if let Some(vector) = report.pentanomial {
                    println!("pentanomial: {vector:?}");
                }
                if let Some(reason) = &report.paired_statistics_unavailable {
                    println!("paired statistics unavailable: {reason}");
                }
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("statistics replay failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn opening_book(input: &BookInput) -> Result<OpeningBook, String> {
    let format = input.format.map(Into::into).unwrap_or_else(|| {
        OpeningFormat::from_extension(
            input
                .input
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default(),
        )
    });
    Ok(OpeningBook {
        path: input.input.clone(),
        format,
        order: OpeningOrder::Sequential,
        count: None,
        plies: input.plies,
        seed: 0,
    })
}

fn hash_file_report(path: &Path) -> Result<BookHashReport, String> {
    let bytes =
        fs::read(path).map_err(|error| format!("cannot read book {}: {error}", path.display()))?;
    Ok(BookHashReport {
        path: path.to_owned(),
        bytes: bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    })
}

fn book_stats(input: &BookInput) -> Result<BookStatsReport, String> {
    let book = opening_book(input)?;
    let audit = audit_opening_book(&book).map_err(|error| error.to_string())?;
    let openings = load_openings_named(&book, 0).map_err(|error| error.to_string())?;
    let hash = hash_file_report(&input.input)?;
    let unique = openings
        .iter()
        .map(|opening| format!("{:?}\0{}", opening.start_fen, opening.moves.join(" ")))
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let min_plies = openings
        .iter()
        .map(|opening| opening.moves.len())
        .min()
        .unwrap_or(0);
    let max_plies = openings
        .iter()
        .map(|opening| opening.moves.len())
        .max()
        .unwrap_or(0);
    let mean_plies = openings
        .iter()
        .map(|opening| opening.moves.len() as f64)
        .sum::<f64>()
        / openings.len() as f64;
    let text = fs::read_to_string(&input.input)
        .map_err(|error| format!("cannot read book {}: {error}", input.input.display()))?;
    let (eval_values, eval_unit) = book_eval_values(&text, book.format);
    let eval_band = (!eval_values.is_empty()).then(|| BookEvalBand {
        samples: eval_values.len(),
        unit: eval_unit.into(),
        minimum: eval_values.iter().copied().fold(f64::INFINITY, f64::min),
        mean: eval_values.iter().sum::<f64>() / eval_values.len() as f64,
        maximum: eval_values
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max),
    });
    Ok(BookStatsReport {
        path: input.input.clone(),
        format: book.format.label().into(),
        sha256: hash.sha256,
        bytes: hash.bytes,
        candidates: audit.candidates,
        usable: audit.usable,
        rejected: audit.rejected_indices.len(),
        unique,
        duplicates: openings.len() - unique,
        min_plies,
        max_plies,
        mean_plies,
        eval_band,
    })
}

fn book_eval_values(text: &str, format: OpeningFormat) -> (Vec<f64>, &'static str) {
    match format {
        OpeningFormat::Epd => {
            let values = text
                .lines()
                .filter_map(|line| {
                    let tokens = line
                        .split(|character: char| character.is_whitespace() || character == ';')
                        .filter(|token| !token.is_empty())
                        .collect::<Vec<_>>();
                    tokens
                        .windows(2)
                        .find_map(|pair| (pair[0] == "ce").then(|| pair[1].parse().ok()).flatten())
                })
                .collect();
            (values, "centipawns (EPD ce)")
        }
        OpeningFormat::Pgn => {
            let mut rest = text;
            let mut values = Vec::new();
            while let Some((_, after)) = rest.split_once("[%eval ") {
                if let Some(value) = after
                    .split(|character: char| character == ']' || character.is_whitespace())
                    .next()
                    .and_then(|value| value.parse().ok())
                {
                    values.push(value);
                }
                rest = after;
            }
            (values, "pawns (PGN %eval)")
        }
    }
}

fn slice_book(command: &BookSliceCommand) -> Result<BookSliceReport, String> {
    if command.output == command.book.input {
        return Err("slice output must differ from the input path".into());
    }
    let mut book = opening_book(&command.book)?;
    let audit = audit_opening_book(&book).map_err(|error| error.to_string())?;
    if !audit.valid() {
        return Err(format!(
            "book contains rejected candidates at indices {}",
            audit
                .rejected_indices
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    book.order = match command.order {
        BookOrderArg::Sequential => OpeningOrder::Sequential,
        BookOrderArg::Random => OpeningOrder::Random,
    };
    let openings = load_openings_named(&book, command.seed).map_err(|error| error.to_string())?;
    if command.start >= openings.len() {
        return Err(format!(
            "slice start {} is outside {} usable entries",
            command.start,
            openings.len()
        ));
    }
    let selected = openings
        .iter()
        .skip(command.start)
        .take(usize::try_from(command.count).unwrap_or(usize::MAX))
        .collect::<Vec<_>>();
    let mut output = String::new();
    for opening in &selected {
        let fen = fen_after(opening.start_fen.as_deref(), &opening.moves)
            .ok_or_else(|| format!("cannot materialize opening {:?}", opening.label))?;
        output.push_str(&fen.split_whitespace().take(4).collect::<Vec<_>>().join(" "));
        output.push('\n');
    }
    let mut options = OpenOptions::new();
    options.write(true);
    if command.force {
        options.create(true).truncate(true);
    } else {
        options.create_new(true);
    }
    let mut file = options.open(&command.output).map_err(|error| {
        format!(
            "cannot create slice output {}: {error}",
            command.output.display()
        )
    })?;
    file.write_all(output.as_bytes()).map_err(|error| {
        format!(
            "cannot write slice output {}: {error}",
            command.output.display()
        )
    })?;
    file.sync_all().map_err(|error| {
        format!(
            "cannot flush slice output {}: {error}",
            command.output.display()
        )
    })?;
    let input_hash = hash_file_report(&command.book.input)?;
    let output_hash = hash_file_report(&command.output)?;
    Ok(BookSliceReport {
        input: command.book.input.clone(),
        output: command.output.clone(),
        input_sha256: input_hash.sha256,
        output_sha256: output_hash.sha256,
        start: command.start,
        requested_count: usize::try_from(command.count).unwrap_or(usize::MAX),
        written: selected.len(),
        order: match command.order {
            BookOrderArg::Sequential => "sequential",
            BookOrderArg::Random => "random",
        }
        .into(),
        seed: command.seed,
    })
}

fn book_error(error: String) -> ExitCode {
    eprintln!("book command failed: {error}");
    ExitCode::FAILURE
}

async fn run_nps(command: NpsCommand, machine: bool, dry_run: bool) -> ExitCode {
    let base_launch = match command.engine.resolve() {
        Ok(launch) => launch,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    if !command.self_tolerance_percent.is_finite() || command.self_tolerance_percent < 0.0 {
        eprintln!("configuration error: --self-tolerance-percent must be finite and non-negative");
        return ExitCode::from(2);
    }
    if command.scale_threads.is_some() {
        return run_nps_scaling(command, base_launch, machine, dry_run).await;
    }
    let comparison = command.against.is_some()
        || command.self_pair
        || !command.a_builds.is_empty()
        || !command.b_builds.is_empty();
    if comparison && command.against.is_none() && !command.self_pair {
        eprintln!("configuration error: NPS comparison requires --against or --self-pair");
        return ExitCode::from(2);
    }
    if comparison && !command.moves.is_empty() {
        eprintln!("configuration error: --move is currently limited to single-sample nps");
        return ExitCode::from(2);
    }
    let positions = if command.positions.is_empty() {
        vec!["startpos".to_owned()]
    } else {
        command.positions.clone()
    };
    let design = NpsExperimentDesign {
        nodes: command.nodes,
        positions: positions.clone(),
        repetitions: command.repetitions,
        warmup_repetitions: command.warmup,
        deadline_ms: command.deadline_ms,
        state_policy: command.state.into(),
        seed: command.seed,
        bootstrap_samples: command.bootstrap_samples,
    };
    let participants = if comparison {
        nps_participants(&base_launch, &command)
    } else {
        vec![NpsExperimentParticipant {
            arm: "A".into(),
            build: nps_build_label(&base_launch.executable, "A", 1),
            participant: RuntimeParticipant {
                id: ParticipantId::from_u128(1),
                launch: base_launch,
            },
        }]
    };
    let mut path_pointers = Vec::new();
    for (index, item) in participants.iter().enumerate() {
        path_pointers.push(format!(
            "/participants/{index}/participant/launch/executable"
        ));
        if item.participant.launch.working_directory.is_some() {
            path_pointers.push(format!(
                "/participants/{index}/participant/launch/working_directory"
            ));
        }
    }
    let current_directory = match std::env::current_dir() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("configuration error: cannot read current directory: {error}");
            return ExitCode::from(2);
        }
    };
    let resolved = match resolve_config(
        built_in_defaults(),
        None,
        json!({
            "command": "nps",
            "participants": participants,
            "design": design,
            "self_tolerance_percent": command.self_tolerance_percent
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
    let participants: Vec<NpsExperimentParticipant> =
        serde_json::from_value(resolved.value()["participants"].clone())
            .expect("resolved NPS participants retain their schema");
    let design: NpsExperimentDesign = serde_json::from_value(resolved.value()["design"].clone())
        .expect("resolved NPS design retains its schema");
    if dry_run {
        let invocations = participants
            .iter()
            .map(|item| &item.participant.launch)
            .collect();
        print_output(
            &MachineOutput::DryRun {
                command: "nps",
                config_sha256: resolved.sha256(),
                resolved_configuration: resolved.value(),
                invocations,
            },
            machine,
        );
        return ExitCode::SUCCESS;
    }
    if design.positions == ["startpos"] {
        eprintln!(
            "nps warning: startpos alone is a weak workload; use representative positions for decisions"
        );
    }
    if !comparison {
        let request = NpsRequest {
            nodes: design.nodes,
            position: design.positions[0].clone(),
            moves: command.moves,
            deadline_ms: design.deadline_ms,
        };
        return print_nps_result(
            MeasureNps::execute(
                &AffinityUciSessionFactory::new(apply_nps_affinity),
                &participants[0].participant,
                request,
            )
            .await,
            machine,
        );
    }
    match CompareNps::execute(
        &AffinityUciSessionFactory::new(apply_nps_affinity),
        &participants,
        design,
    )
    .await
    {
        Ok(report) => {
            let self_pair = participants[0].participant.launch.executable
                == participants
                    .iter()
                    .find(|item| item.arm == "B")
                    .expect("comparison has B")
                    .participant
                    .launch
                    .executable;
            if !self_pair {
                eprintln!(
                    "nps warning: no self pair was recorded; run --self-pair under matching conditions to measure harness noise"
                );
            } else if let [left, right] = report.arms.as_slice() {
                let difference = ((right.median_nps / left.median_nps) - 1.0).abs() * 100.0;
                if difference > command.self_tolerance_percent {
                    eprintln!(
                        "nps warning: self-pair median difference {difference:.3}% exceeds tolerance ±{:.3}%",
                        command.self_tolerance_percent
                    );
                }
            }
            if machine {
                print_json(&MachineOutput::NpsComparison { report });
            } else {
                for arm in &report.arms {
                    println!(
                        "arm {}: median {:.3} NPS (95% bootstrap CI {:.3}..{:.3}); best build {:.3}",
                        arm.arm,
                        arm.median_nps,
                        arm.median_ci95[0],
                        arm.median_ci95[1],
                        arm.best_of_nps
                    );
                    for build in &arm.builds {
                        println!(
                            "  {}: median {:.3} over {} samples",
                            build.build, build.median_nps, build.samples
                        );
                    }
                }
                println!(
                    "per-round ratio SD: {}",
                    report.per_round_ratio_sd.map_or_else(
                        || "unavailable (need at least two rounds)".into(),
                        |value| format!("{value:.6}")
                    )
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("nps measurement failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn apply_nps_affinity(
    process_id: u32,
    allocation: &colosseum_application::CpuAllocation,
) -> Result<(), String> {
    colosseum_engine::apply_process_affinity(process_id, allocation)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

async fn run_nps_scaling(
    command: NpsCommand,
    base_launch: EngineLaunchSpec,
    machine: bool,
    dry_run: bool,
) -> ExitCode {
    if command.against.is_some()
        || command.self_pair
        || !command.a_builds.is_empty()
        || !command.b_builds.is_empty()
        || !command.moves.is_empty()
    {
        eprintln!(
            "configuration error: scaling sweep cannot be combined with A/B builds or --move"
        );
        return ExitCode::from(2);
    }
    let threads = match parse_scaling_threads(command.scale_threads.as_deref().unwrap_or_default())
    {
        Ok(threads) => threads,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let Some(threads_option) = command.threads_option.as_deref() else {
        eprintln!("configuration error: --threads-option is required for a scaling sweep");
        return ExitCode::from(2);
    };
    let topology = match detect_cpu_topology() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: scaling requires exact CPU topology: {error}");
            return ExitCode::from(2);
        }
    };
    let allowed = match detect_allowed_cpu_set(&topology) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: scaling requires the allowed CPU set: {error}");
            return ExitCode::from(2);
        }
    };
    let characteristics = match detect_cpu_characteristics(&topology) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: scaling requires CPU class/NUMA evidence: {error}");
            return ExitCode::from(2);
        }
    };
    let placement = match plan_cpu_placement(
        &topology,
        &allowed,
        &CpuPlacementPolicy::Auto {
            headroom_physical_cores: 0,
        },
    ) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: cannot allocate scaling cores: {error}");
            return ExitCode::from(2);
        }
    };
    let CpuPlacementPlan::WholePhysicalCores { cores, .. } = placement else {
        eprintln!("configuration error: scaling requires whole physical-core placement");
        return ExitCode::from(2);
    };
    if threads.last().copied().unwrap_or(0) as usize > cores.len() {
        eprintln!(
            "configuration error: requested {} threads but only {} allowed physical cores are available",
            threads.last().copied().unwrap_or(0),
            cores.len()
        );
        return ExitCode::from(2);
    }
    let positions = if command.positions.is_empty() {
        vec!["startpos".into()]
    } else {
        command.positions.clone()
    };
    if positions == ["startpos"] {
        eprintln!(
            "nps warning: startpos alone is a weak workload; use representative positions for decisions"
        );
    }
    let hash_policy: NpsHashPolicy = command.hash_policy.into();
    let mut jobs = Vec::new();
    for (job_index, thread_count) in threads.iter().copied().enumerate() {
        let selected = &cores[..thread_count as usize];
        let cpus = selected
            .iter()
            .flat_map(|core| core.logical_cpus.iter().copied())
            .collect::<Vec<_>>();
        let point_hash = match hash_policy {
            NpsHashPolicy::FixedTotal => command.hash_mb,
            NpsHashPolicy::PerThread => {
                match command.hash_mb.checked_mul(u64::from(thread_count)) {
                    Some(value) => value,
                    None => {
                        eprintln!("configuration error: per-thread Hash size overflows u64");
                        return ExitCode::from(2);
                    }
                }
            }
        };
        let mut launch = base_launch.clone();
        launch.options.insert(
            threads_option.to_owned(),
            UciOptionValue::String(thread_count.to_string()),
        );
        launch.options.insert(
            command.hash_option.clone(),
            UciOptionValue::String(point_hash.to_string()),
        );
        launch.allocated_cpus = colosseum_application::CpuAllocation::Enforced(cpus.clone());
        let participants = ["A", "B"]
            .into_iter()
            .enumerate()
            .map(|(side, arm)| NpsExperimentParticipant {
                arm: arm.into(),
                build: format!("{}t", thread_count),
                participant: RuntimeParticipant {
                    id: ParticipantId::from_u128((job_index * 2 + side + 1) as u128),
                    launch: launch.clone(),
                },
            })
            .collect();
        let selected_characteristics = characteristics
            .cores
            .iter()
            .filter(|item| item.logical_cpus.iter().any(|cpu| cpus.contains(cpu)))
            .collect::<Vec<_>>();
        let mut core_classes = selected_characteristics
            .iter()
            .map(|item| format!("{:?}", item.core_class))
            .collect::<Vec<_>>();
        core_classes.sort();
        core_classes.dedup();
        let mut numa_nodes = selected_characteristics
            .iter()
            .filter_map(|item| item.numa_node)
            .map(|node| format!("{}:{}", node.group, node.number))
            .collect::<Vec<_>>();
        numa_nodes.sort();
        numa_nodes.dedup();
        jobs.push(NpsScalingJob {
            threads: thread_count,
            hash_mb: point_hash,
            cpus,
            core_classes,
            numa_nodes,
            participants,
            design: NpsExperimentDesign {
                nodes: command.nodes,
                positions: positions.clone(),
                repetitions: command.repetitions,
                warmup_repetitions: command.warmup,
                deadline_ms: command.deadline_ms,
                state_policy: command.state.into(),
                seed: command.seed,
                bootstrap_samples: command.bootstrap_samples,
            },
        });
    }
    let current_directory = match std::env::current_dir() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: cannot read current directory: {error}");
            return ExitCode::from(2);
        }
    };
    let mut path_pointers = Vec::new();
    for (job, scaling_job) in jobs.iter().enumerate() {
        for participant in 0..2 {
            path_pointers.push(format!(
                "/jobs/{job}/participants/{participant}/participant/launch/executable"
            ));
            if scaling_job.participants[participant]
                .participant
                .launch
                .working_directory
                .is_some()
            {
                path_pointers.push(format!(
                    "/jobs/{job}/participants/{participant}/participant/launch/working_directory"
                ));
            }
        }
    }
    let resolved = match resolve_config(
        built_in_defaults(),
        None,
        json!({
            "command": "nps-scaling",
            "hash_policy": hash_policy,
            "threads_option": threads_option,
            "hash_option": command.hash_option,
            "jobs": jobs,
            "topology": topology,
            "characteristics": characteristics,
        }),
        &[],
        &current_directory,
        &path_pointers,
    ) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let jobs: Vec<NpsScalingJob> = serde_json::from_value(resolved.value()["jobs"].clone())
        .expect("resolved scaling jobs retain their schema");
    if dry_run {
        let invocations = jobs
            .iter()
            .flat_map(|job| job.participants.iter().map(|item| &item.participant.launch))
            .collect();
        print_output(
            &MachineOutput::DryRun {
                command: "nps-scaling",
                config_sha256: resolved.sha256(),
                resolved_configuration: resolved.value(),
                invocations,
            },
            machine,
        );
        return ExitCode::SUCCESS;
    }
    let factory = AffinityUciSessionFactory::new(apply_nps_affinity);
    let mut inputs = Vec::new();
    for job in jobs {
        let comparison = match CompareNps::execute(&factory, &job.participants, job.design).await {
            Ok(value) => value,
            Err(error) => {
                eprintln!("nps scaling failed at {} threads: {error}", job.threads);
                return ExitCode::FAILURE;
            }
        };
        let mut values = comparison
            .samples
            .iter()
            .map(|sample| sample.measurement.authoritative_nps)
            .collect::<Vec<_>>();
        values.sort_by(f64::total_cmp);
        let median_nps = if values.len().is_multiple_of(2) {
            (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
        } else {
            values[values.len() / 2]
        };
        inputs.push(NpsScalingInput {
            threads: job.threads,
            pinned_physical_cores: job.threads,
            hash_mb: job.hash_mb,
            median_nps,
            core_classes: job.core_classes,
            numa_nodes: job.numa_nodes,
        });
    }
    let report = match summarize_nps_scaling(hash_policy, inputs) {
        Ok(value) => value,
        Err(error) => {
            eprintln!("nps scaling failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    if machine {
        print_json(&MachineOutput::NpsScaling { report });
    } else {
        for point in &report.points {
            println!(
                "{} threads / {} cores / {} MiB Hash: {:.3} NPS, {:.3}x speedup, {:.2}% efficiency",
                point.input.threads,
                point.input.pinned_physical_cores,
                point.input.hash_mb,
                point.input.median_nps,
                point.speedup,
                point.parallel_efficiency * 100.0
            );
        }
    }
    ExitCode::SUCCESS
}

fn parse_scaling_threads(value: &str) -> Result<Vec<u32>, String> {
    let mut threads = value
        .split(',')
        .map(|item| {
            item.parse::<u32>()
                .map_err(|_| format!("invalid scaling thread count {item:?}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    threads.sort_unstable();
    if threads.first().copied() != Some(1)
        || threads.contains(&0)
        || threads.windows(2).any(|pair| pair[0] == pair[1])
    {
        return Err("scaling thread counts must be unique, positive, and include 1".into());
    }
    Ok(threads)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NpsScalingJob {
    threads: u32,
    hash_mb: u64,
    cpus: Vec<colosseum_application::LogicalCpuId>,
    core_classes: Vec<String>,
    numa_nodes: Vec<String>,
    participants: Vec<NpsExperimentParticipant>,
    design: NpsExperimentDesign,
}

fn print_nps_result(
    result: Result<NpsReport, colosseum_application::ApplicationError>,
    machine: bool,
) -> ExitCode {
    match result {
        Ok(report) => {
            if machine {
                print_json(&MachineOutput::Nps { report });
            } else {
                println!("authoritative NPS: {:.3}", report.authoritative_nps);
                println!(
                    "fixed work: {} requested, {} reported; harness wall time: {:.3} ms",
                    report.requested_nodes,
                    report.reported_nodes,
                    report.harness_elapsed_ns as f64 / 1_000_000.0
                );
                println!(
                    "engine diagnostics: time={} ms, nps={}",
                    report
                        .engine_reported_time_ms
                        .map_or_else(|| "unavailable".into(), |value| value.to_string()),
                    report
                        .engine_reported_nps
                        .map_or_else(|| "unavailable".into(), |value| value.to_string())
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("nps measurement failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn nps_participants(
    base: &EngineLaunchSpec,
    command: &NpsCommand,
) -> Vec<NpsExperimentParticipant> {
    let mut paths_a = vec![base.executable.clone()];
    paths_a.extend(command.a_builds.clone());
    let mut paths_b = vec![
        command
            .against
            .clone()
            .unwrap_or_else(|| base.executable.clone()),
    ];
    paths_b.extend(command.b_builds.clone());
    paths_a
        .into_iter()
        .enumerate()
        .map(|(index, path)| ("A", index, path))
        .chain(
            paths_b
                .into_iter()
                .enumerate()
                .map(|(index, path)| ("B", index, path)),
        )
        .enumerate()
        .map(|(identity, (arm, index, executable))| {
            let mut launch = base.clone();
            launch.executable = executable.clone();
            launch.label = Some(nps_build_label(&executable, arm, index + 1));
            NpsExperimentParticipant {
                arm: arm.into(),
                build: launch.label.clone().expect("label assigned"),
                participant: RuntimeParticipant {
                    id: ParticipantId::from_u128(identity as u128 + 1),
                    launch,
                },
            }
        })
        .collect()
}

fn nps_build_label(path: &Path, arm: &str, ordinal: usize) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| format!("{arm}-{ordinal}"), str::to_owned)
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
    Spsa {
        run_directory: PathBuf,
        report: Box<SpsaReport>,
    },
    SpsaPlan {
        report: SpsaPlanReport,
    },
    SpsaStatus {
        run_directory: &'a Path,
        report: SpsaStatusOutput,
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
    BookHash {
        report: BookHashReport,
    },
    BookStats {
        report: BookStatsReport,
    },
    BookVerify {
        audit: colosseum_engine::OpeningAudit,
    },
    BookSlice {
        report: BookSliceReport,
    },
    StatsReplay {
        report: colosseum_cli::stats_replay::StatsReplayReport,
    },
    Nps {
        report: NpsReport,
    },
    NpsComparison {
        report: NpsExperimentReport,
    },
    NpsScaling {
        report: NpsScalingReport,
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

#[derive(Debug, Clone, Serialize)]
struct SpsaReport {
    engine: EngineLaunchSpec,
    schedule: SpsaScheduleArtifact,
    bound_tune: SpsaBoundTune,
    tune_audit: SpsaTuneAudit,
    #[serde(skip_serializing_if = "Option::is_none")]
    tuned_result: Option<SpsaTuneResult>,
    engine_sha256: String,
    engine_time_control: match_runner::ConfiguredTimeControl,
    adjudication: AdjudicationConfig,
    execution: match_runner::MatchExecutionPlan,
    master_seed: u64,
    master_seed_generated: bool,
    openings: match_runner::OpeningPolicyReport,
    driver: spsa_driver::SpsaDriverReport,
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
    let engine_b = resolve_match_engine(
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
    let bytes = fs::read(path)
        .map_err(|error| format!("cannot hash executable {}: {error}", path.display()))?;
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
    let engine_a = match resolve_match_engine(
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
    let engine_b = match resolve_match_engine(
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

fn read_stored_spsa_inputs(root: &Path) -> Result<(SpsaRunSettings, f64, u32), String> {
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
    let final_window_percent = value
        .get("final_window_percent")
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| (1..=100).contains(value))
        .ok_or_else(|| "stored SPSA final-window percent is missing or invalid".to_owned())?;
    let settings =
        SpsaRunSettings::new(iterations, games_per_iteration).map_err(|error| error.to_string())?;
    Ok((settings, r_end, final_window_percent))
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

struct DurableSpsaOutput {
    directory: Arc<RunDirectory>,
    checkpoint: Mutex<spsa_driver::SpsaCheckpoint>,
    recorder: Mutex<Option<RunRecorder>>,
    settings: SpsaRunSettings,
}

#[derive(Serialize)]
struct SpsaRunFileFragment {
    engine: SpsaRunFileEngine,
}

#[derive(Serialize)]
struct SpsaRunFileEngine {
    options: BTreeMap<String, i64>,
}

impl DurableSpsaOutput {
    fn new(
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

    fn persist_checkpoint(&self) -> Result<(), String> {
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

    fn rewrite_pgn(&self) -> Result<(), String> {
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
                        "{{Colosseum SPSA iteration: {}; sample: committed}}",
                        iteration.iteration
                    )
                    .map_err(|error| error.to_string())?;
                    writeln!(file, "{}\n", game.pgn.trim_end())
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        if let Some(iteration) = &checkpoint.invalid_iteration {
            for pair in &iteration.pairs {
                for game in [&pair.first, &pair.second] {
                    writeln!(
                        file,
                        "{{Colosseum SPSA iteration: {}; sample: invalid}}",
                        iteration.iteration
                    )
                    .map_err(|error| error.to_string())?;
                    writeln!(file, "{}\n", game.pgn.trim_end())
                        .map_err(|error| error.to_string())?;
                }
            }
        }
        file.sync_all().map_err(|error| error.to_string())
    }

    fn append_event(&self, event: Value) -> Result<(), String> {
        let mut line = serde_json::to_vec(&event).map_err(|error| error.to_string())?;
        line.push(b'\n');
        self.directory
            .append_log(&line)
            .map_err(|error| error.to_string())
    }

    fn finish(&self, report: &SpsaReport) -> Result<(), String> {
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

fn spsa_official_sample(
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

fn print_spsa(report: &SpsaReport, run_directory: &Path) {
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
        println!(
            "tuned vector: rounded mean of {} sample(s) from final {}% window",
            result.window.samples_used, result.window.percent
        );
        for parameter in &result.parameters {
            println!(
                "setoption name {} value {}  (mean {:.6})",
                parameter.name, parameter.tuned, parameter.mean
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

fn print_spsa_tune_warning(warning: &SpsaTuneWarning) {
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

    #[test]
    fn calibration_terminal_classes_have_distinct_automation_exit_codes() {
        assert_eq!(calibration_exit_code(CalibrationStatus::Pass), 0);
        assert_eq!(calibration_exit_code(CalibrationStatus::Fail), 1);
        assert_eq!(calibration_exit_code(CalibrationStatus::Inconclusive), 4);
        assert_eq!(calibration_exit_code(CalibrationStatus::Invalid), 5);
    }

    #[test]
    fn scaling_thread_list_requires_unique_positive_one_thread_baseline() {
        assert_eq!(parse_scaling_threads("4,1,2").unwrap(), [1, 2, 4]);
        assert!(parse_scaling_threads("2,4").is_err());
        assert!(parse_scaling_threads("1,2,2").is_err());
        assert!(parse_scaling_threads("1,0").is_err());
    }
}
