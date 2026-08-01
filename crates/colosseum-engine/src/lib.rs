//! Tournament orchestration: the backend.
//!
//! Step 2 establishes the crate, its error seam, and storage-path helpers. Later
//! steps add: the per-game runner (UCI + shakmaty + adjudication + PGN), the
//! concurrency scheduler with Go/Stop/Force-Stop/resume, the event stream, the Elo
//! updater, SQLite persistence with tournament history + resume (Steps 5–6), and
//! config/engine-library file I/O with `--portable` mode (Step 6).

pub mod affinity;
pub mod allowed_cpus;
pub mod characteristics;
#[cfg(feature = "tournament")]
pub mod detect;
#[cfg(feature = "tournament")]
pub mod error;
#[cfg(feature = "runner")]
pub mod incidents;
#[cfg(feature = "runner")]
pub mod live;
#[cfg(feature = "tournament")]
pub mod openings;
#[cfg(feature = "runner")]
pub mod pgn;
pub mod placement;
#[cfg(feature = "runner")]
pub mod runner;
#[cfg(feature = "tournament")]
pub mod scheduler;
#[cfg(feature = "tournament")]
pub mod store;
pub mod topology;

pub use affinity::{
    AffinityCapability, AffinityError, AffinityOutcome, AffinitySupportLevel, AppliedAffinity,
    affinity_capability, apply_process_affinity, process_affinity_groups,
};
pub use allowed_cpus::{AllowedCpuError, AllowedCpuSet, AllowedCpuSource, detect_allowed_cpu_set};
pub use characteristics::{
    CharacteristicsError, CharacteristicsSource, CoreClass, CpuCharacteristics, NumaNodeId,
    PhysicalCoreCharacteristics, detect_cpu_characteristics,
};
#[cfg(feature = "runner")]
pub use colosseum_uci::Score;
#[cfg(feature = "tournament")]
pub use detect::{DetectResult, detect_engine, split_name_version};
#[cfg(feature = "tournament")]
pub use error::EngineError;
#[cfg(feature = "runner")]
pub use live::{EvalPoint, LiveGameHandle, LiveGameState, LiveSearch};
#[cfg(feature = "tournament")]
pub use openings::{ResolvedOpening, load_openings, summarize};
pub use placement::{
    CpuPlacementError, CpuPlacementPlan, CpuPlacementPolicy, DEFAULT_AUTO_HEADROOM_PHYSICAL_CORES,
    EngineCpuPlacement, GameSlotCpuAllocation, PlacementAsymmetry, allocate_game_slots,
    plan_cpu_placement,
};
#[cfg(feature = "runner")]
pub use runner::{
    CLOCK_MODEL_ID, CLOCK_MODEL_VERSION, ChargedElapsedSummary, ClockAccountingReport,
    EngineGameSpec, GameReport, GameSpec, run_game,
};
#[cfg(feature = "tournament")]
pub use scheduler::{
    Command, EloEntry, InFlightGame, ResultParticipant, Tournament, TournamentResults,
    TournamentSnapshot, TournamentStatus, create_tournament, load_tournament_results,
    resume_tournament,
};
#[cfg(feature = "tournament")]
pub use store::{GameRow, PendingGame, Store, TournamentEngineRow, TournamentRow};
pub use topology::{
    CpuTopology, LogicalCpuId, PhysicalCore, SiblingMapping, TopologyError, TopologySource,
    detect_cpu_topology,
};
