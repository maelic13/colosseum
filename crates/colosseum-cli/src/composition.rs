//! Independent headless composition root for Colosseum CLI.
//!
//! The parser, the dispatch and the resolvers more than one command needs
//! live here; every command owns a module beside them. The split is a
//! boundary, not a rewrite: the generated command reference is
//! byte-identical to the one the single file produced.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::{
    EngineArgs, OfficialSample, RunDirectory, RunRecord, RunRecorder, RunStatus, built_in_defaults,
    load_spsa_tune, parse_cpu_list, persist_and_verify_spsa_schedule,
    read_and_verify_spsa_schedule, resolve_config,
};
use crate::{
    capabilities, match_runner, self_test, sprt_runner, spsa_driver, suite_driver,
    tournament_driver, uci_stub,
};
use clap::{Args, CommandFactory, Parser, Subcommand, ValueEnum};
use colosseum_application::{
    CalibrationBinaries, CalibrationDesign, CalibrationInterval, CalibrationStatus, CheckEngine,
    CompareNps, CompletePair, ComplianceReport, ComplianceStatus, DEFAULT_CALIBRATION_CONFIDENCE,
    DEFAULT_CALIBRATION_GAMES, DEFAULT_CALIBRATION_TOLERANCE_NELO,
    DEFAULT_SPSA_GAMES_PER_ITERATION, DEFAULT_SPSA_ITERATIONS, EngineInspection, EngineLaunchSpec,
    FixedPlanObjective, FixedPlanReport, FixedPlanRequest, InspectEngine, MeasureNps,
    NpsExperimentDesign, NpsExperimentParticipant, NpsExperimentReport, NpsHashPolicy, NpsReport,
    NpsRequest, NpsScalingInput, NpsScalingReport, NpsStatePolicy, PlanTournament, RateTournament,
    RuntimeParticipant, SPSA_TUNE_RESULT_SCHEMA_VERSION, SprtBundle, SprtDesign,
    SprtLengthPlanReport, SprtLengthPlanRequest, SprtParameters, SpsaBoundTune, SpsaCenterSample,
    SpsaEstimator, SpsaEstimatorPolicy, SpsaGateHashStatus, SpsaPlanReport, SpsaRunSettings,
    SpsaStatusReport, SpsaTimingInput, SpsaTuneAudit, SpsaTuneResult, SpsaTuneResultError,
    SpsaTuneWarning, SpsaWaveShape, TournamentCompletedGame, TournamentDesign,
    TournamentFixedRating, TournamentParticipant, TournamentPlan, UciOptionSchema, UciOptionValue,
    classify_calibration, diagnose_spsa, plan_fixed, plan_sprt_length, plan_spsa, scaling_hash_mb,
    spsa_wave_shape, summarize_nps_scaling,
};
use colosseum_core::{
    AdjudicationConfig, DrawAdjudication, EloModel, GameResult, OpeningBook, OpeningFormat,
    OpeningOrder, PairGameResult, ParticipantId, PentanomialSprtResult, PentanomialVector,
    ResignAdjudication, SprtDecision, SpsaDerivedKnob, SpsaEndSpec, SpsaIteration,
    SpsaScheduleArtifact, Termination, TimeControl, fixed_n_achieved_resolution, pentanomial_sprt,
    pentanomial_statistics, round_half_away_from_zero,
};
use colosseum_engine::{
    CpuPlacementPlan, CpuPlacementPolicy, SlotAllocation, audit_opening_book,
    detect_allowed_cpu_set, detect_cpu_characteristics, detect_cpu_topology, fen_after,
    load_openings_named, plan_cpu_placement,
};
use colosseum_uci::{AffinityUciSessionFactory, UciSessionFactory};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::cancellation::{Cancellation, DEFAULT_STOP_GRACE_SECONDS};
use crate::match_runner::{FaultPolicy, MatchFaultCounts, record_fault};
use crate::progress::{self, ProgressBlock, ProgressSchedule, ProgressUnit};
use crate::run_writer::RunWriter;

// The sample classes a written game may carry are the reader's vocabulary:
// naming them once is what keeps a writer from drifting from the replay that
// honours it.
use crate::stats_replay::{
    INVALID_SAMPLE, OFFICIAL_SAMPLE, POST_TERMINAL_SAMPLE, UNSCORABLE_SAMPLE,
};
use crate::versioned_artifact::require_schema_version;

mod book;
mod calibrate;
mod capabilities_command;
mod durable;
mod engine;
mod match_command;
mod nps;
mod self_test_command;
mod shared;
mod sprt;
mod spsa;
mod stats;
mod status_command;
mod tournament;

use book::*;
use calibrate::*;
use capabilities_command::*;
use durable::*;
use engine::*;
use match_command::*;
use nps::*;
use self_test_command::*;
use shared::*;
use sprt::*;
use spsa::*;
use stats::*;
use status_command::*;
use tournament::*;

/// A durable run that stopped cleanly on request rather than reaching its
/// terminal state. Its run directory resumes; nothing about it is a failure.
pub const CANCELLED_EXIT_CODE: u8 = 6;

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

    /// Load reusable command options from an inheritable TOML run file.
    #[arg(long, global = true, value_name = "PATH")]
    run_file: Option<PathBuf>,

    /// Remove one inherited run-file option before applying CLI arguments.
    #[arg(long, global = true, value_name = "LONG_NAME", requires = "run_file")]
    unset_run_option: Vec<String>,

    /// Seconds a game in flight may take to finish after an interrupt asks a
    /// durable run to stop; a second interrupt abandons it at once.
    #[arg(long, global = true, default_value_t = DEFAULT_STOP_GRACE_SECONDS, value_name = "SECONDS")]
    stop_grace_secs: u64,

    /// Internal: request the clean stop after this many committed units, so
    /// the interrupt path can be exercised deterministically. Not a public
    /// interface; a console interrupt is how a user stops a run.
    #[arg(
        long = "__stop-after-units",
        global = true,
        hide = true,
        value_name = "UNITS"
    )]
    stop_after_units: Option<u64>,

    #[command(subcommand)]
    command: Command,
}

/// Return the exact public command model used by the shipped executable.
#[must_use]
pub fn command_spec() -> clap::Command {
    Cli::command()
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
    /// Run fixed-work searches over an EPD or FEN position set.
    Suite(Box<suite_driver::SuiteCommand>),
    /// Plan or run a multi-engine round-robin or gauntlet tournament.
    Tournament(TournamentCommand),
    /// Convenience alias for the same tournament gauntlet planner.
    Gauntlet(TournamentPlanCommand),
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

#[tokio::main]
pub async fn run() -> ExitCode {
    if let Err(error) = colosseum_uci::install_process_tree_guard() {
        eprintln!("infrastructure error: cannot install process-tree guard: {error}");
        return ExitCode::from(3);
    }
    let arguments = match crate::run_file::expand_arguments(std::env::args_os()) {
        Ok(arguments) => arguments,
        Err(error) => {
            eprintln!("configuration error: {error}");
            return ExitCode::from(2);
        }
    };
    let cli = Cli::parse_from(arguments);
    let _run_file_controls = (&cli.run_file, &cli.unset_run_option);
    // One handle, one listener, one cancellation path for whichever durable
    // command this invocation turns out to be.
    let mut cancellation = Cancellation::new(Duration::from_secs(cli.stop_grace_secs));
    if let Some(units) = cli.stop_after_units {
        cancellation = cancellation.with_unit_budget(units);
    }
    let _interrupts = cancellation.listen_for_interrupts();
    match cli.command {
        Command::Capabilities if cli.dry_run => unsupported_dry_run("capabilities"),
        Command::Capabilities => run_capabilities(cli.json),
        Command::Match(command) => run_match(*command, cli.json, cli.dry_run, cancellation).await,
        Command::Sprt(command) => run_sprt(*command, cli.json, cli.dry_run, cancellation).await,
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
                Some(SpsaAction::History(_)) if cli.dry_run => unsupported_dry_run("spsa history"),
                Some(SpsaAction::History(history)) => run_spsa_history(&history, cli.json),
                None => run_spsa_command(command, cli.json, cli.dry_run, cancellation).await,
            }
        }
        Command::Nps(command) => run_nps(*command, cli.json, cli.dry_run).await,
        Command::Calibrate(command) => {
            run_calibration(*command, cli.json, cli.dry_run, cancellation).await
        }
        Command::Engine(command) => run_engine(command.command, cli.json, cli.dry_run).await,
        Command::Book(_) if cli.dry_run => unsupported_dry_run("book"),
        Command::Book(command) => run_book(command.action, cli.json),
        Command::Stats(_) if cli.dry_run => unsupported_dry_run("stats"),
        Command::Stats(command) => run_stats(command, cli.json),
        Command::Suite(command) => {
            suite_driver::run(*command, cli.json, cli.dry_run, cancellation).await
        }
        Command::Tournament(command) => match command.action {
            TournamentAction::Plan(_) if cli.dry_run => unsupported_dry_run("tournament plan"),
            TournamentAction::Plan(command) => run_tournament_plan(command, None, cli.json),
            TournamentAction::Run(command) => {
                run_tournament_command(*command, cli.json, cli.dry_run, cancellation).await
            }
        },
        Command::Gauntlet(_) if cli.dry_run => unsupported_dry_run("gauntlet"),
        Command::Gauntlet(command) => {
            run_tournament_plan(command, Some(TournamentFormatArg::Gauntlet), cli.json)
        }
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

#[cfg(test)]
mod commit_path_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unscorable_game_keeps_its_iteration_so_replay_keeps_the_whole_iteration() {
        use colosseum_engine::ClockAccountingReport;
        let game = |number: u32, scorable: bool| match_runner::MatchGame {
            number,
            white: if number % 2 == 1 {
                match_runner::MatchSide::A
            } else {
                match_runner::MatchSide::B
            },
            result: GameResult::Draw,
            scorable,
            termination: Termination::FiftyMove,
            clock_accounting: ClockAccountingReport {
                model: "test".into(),
                version: 1,
                white_margin_ms: 0,
                black_margin_ms: 0,
                monotonic_resolution_ns: 1,
                white_charged_elapsed: None,
                black_charged_elapsed: None,
                white_round_trip: None,
                black_round_trip: None,
                phases: None,
            },
            opening: match_runner::OpeningAssignment {
                book_index: Some(number.div_ceil(2) as usize - 1),
                label: "book".into(),
            },
            fault: None,
            error: None,
            pgn: String::new(),
            slot: None,
        };
        // One iteration of two pairs; the third game could not be scored.
        let entries = [game(1, true), game(2, true), game(3, false), game(4, true)]
            .iter()
            .map(|game| paired_game_entry(game, OFFICIAL_SAMPLE, Some(0)))
            .collect::<Vec<_>>();
        // The export tags the game for what a reader must do: leave it out.
        assert_eq!(entries[2].1, UNSCORABLE_SAMPLE);
        // The journal keeps its class, with the mark beside it.
        assert_eq!(entries[2].0.sample, OFFICIAL_SAMPLE);
        assert!(!entries[2].0.scorable);
        let records = entries
            .into_iter()
            .map(|(record, _)| record)
            .collect::<Vec<_>>();
        let iterations = iterations_from_journal(&records);
        let pairs = &iterations[&0];
        assert_eq!(
            pairs.iter().map(|pair| pair.pair_id).collect::<Vec<_>>(),
            [1, 2],
            "a resume dropped part of the iteration"
        );
        assert!(!pairs[1].first.scorable);
        let official = pairs_from_journal(&records, OFFICIAL_SAMPLE);
        assert_eq!(official.len(), 2);
    }

    #[test]
    fn a_game_budget_gives_the_iterations_it_divides_into_and_names_the_nearest_otherwise() {
        assert_eq!(spsa_iterations(None, None, 32), Ok(DEFAULT_SPSA_ITERATIONS));
        assert_eq!(spsa_iterations(Some(60), None, 32), Ok(60));
        assert_eq!(spsa_iterations(None, Some(160_000), 32), Ok(5_000));
        assert_eq!(spsa_iterations(None, Some(168_000), 42), Ok(4_000));
        let refused = spsa_iterations(None, Some(160_000), 42).unwrap_err();
        assert!(
            refused.contains("159978 (3809 iterations) or 160020 (3810 iterations)"),
            "{refused}"
        );
        let small = spsa_iterations(None, Some(10), 32).unwrap_err();
        assert!(small.contains("32 (1 iteration)"), "{small}");
    }

    #[test]
    fn the_default_forfeit_allowance_is_one_percent_and_at_least_five() {
        assert_eq!(forfeit_allowance(2), 5);
        assert_eq!(forfeit_allowance(599), 5);
        assert_eq!(forfeit_allowance(600), 6);
        assert_eq!(forfeit_allowance(30_000), 300);
    }

    #[test]
    fn an_omitted_time_loss_limit_is_the_engine_fault_allowance() {
        let conditions = |engine: Option<u32>, time: Option<u32>| {
            let mut command = Cli::try_parse_from(["colosseum", "match", "--games", "2", "a", "b"])
                .map(|cli| match cli.command {
                    Command::Match(command) => command.conditions,
                    _ => unreachable!("parsed a match"),
                })
                .unwrap();
            command.max_engine_faults = engine;
            command.max_time_losses = time;
            command
        };
        assert_eq!(
            conditions(None, None).fault_policy(7),
            FaultPolicy {
                max_engine_faults: 7,
                max_time_losses: 7,
                rate: None
            }
        );
        assert_eq!(
            conditions(Some(3), None).fault_policy(7),
            FaultPolicy {
                max_engine_faults: 3,
                max_time_losses: 3,
                rate: None
            }
        );
        assert_eq!(
            conditions(None, Some(1)).fault_policy(0),
            FaultPolicy {
                max_engine_faults: 0,
                max_time_losses: 1,
                rate: None
            }
        );
    }

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
