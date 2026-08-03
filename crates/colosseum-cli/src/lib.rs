//! Reusable CLI driving-adapter types.

pub mod config;
pub mod engine_args;
pub mod master_seed;
pub mod run_directory;
pub mod run_record;
pub mod spsa_schedule;

pub use config::{ConfigError, ResolvedConfig, ValueOrigin, built_in_defaults, resolve_config};
pub use engine_args::{EngineArgs, EngineArgsError, parse_cpu_list};
pub use master_seed::{
    MasterSeedEntropy, MasterSeedError, MasterSeedResolution, OsMasterSeedEntropy,
    ResolvedMasterSeedSource, ensure_master_seed,
};
pub use run_directory::{RunDirectory, RunDirectoryError, RunDirectoryOpen, RunDirectoryPaths};
pub use run_record::{
    Anomaly, CapabilityLevel, HostSummary, OfficialSample, RunRecord, RunRecordError, RunRecorder,
    RunStatus,
};
pub use spsa_schedule::{
    SPSA_SCHEDULE_FILE, SpsaScheduleStoreError, persist_and_verify_spsa_schedule,
};
