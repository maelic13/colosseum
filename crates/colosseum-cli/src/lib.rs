//! Reusable CLI driving-adapter types.

mod capabilities;
mod composition;
mod match_runner;
mod run_file;
mod self_test;
mod sprt_runner;
mod spsa_driver;
mod suite_driver;
mod tournament_driver;
mod uci_stub;

pub mod cancellation;
pub mod config;
pub mod engine_args;
pub mod master_seed;
pub mod pgn_telemetry;
pub mod progress;
pub mod run_directory;
pub mod run_record;
pub mod spsa_schedule;
pub mod spsa_tune;
pub mod stats_replay;
pub mod versioned_artifact;

pub use composition::{command_spec, run};

pub use cancellation::{CancelStage, Cancellation, DEFAULT_STOP_GRACE_SECONDS, InterruptListener};
pub use config::{ConfigError, ResolvedConfig, ValueOrigin, built_in_defaults, resolve_config};
pub use engine_args::{EngineArgs, EngineArgsError, parse_cpu_list};
pub use master_seed::{
    MasterSeedEntropy, MasterSeedError, MasterSeedResolution, OsMasterSeedEntropy,
    ResolvedMasterSeedSource, ensure_master_seed,
};
pub use progress::{ProgressBlock, ProgressField, ProgressSchedule, ProgressUnit};
pub use run_directory::{RunDirectory, RunDirectoryError, RunDirectoryOpen, RunDirectoryPaths};
pub use run_record::{
    Anomaly, CapabilityLevel, HostSummary, OfficialSample, RunRecord, RunRecordError, RunRecorder,
    RunStatus,
};
pub use spsa_schedule::{
    SPSA_SCHEDULE_FILE, SpsaScheduleStoreError, persist_and_verify_spsa_schedule,
    read_and_verify_spsa_schedule,
};
pub use spsa_tune::{SpsaTuneFileError, load_spsa_tune};
pub use versioned_artifact::{declared_schema_version, require_schema_version};
