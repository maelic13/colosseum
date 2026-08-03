//! Fixed-length direct-engine match orchestration for the CLI.
//!
//! This deliberately runs a finite number of games without sequential
//! statistics or stopping logic. Pair-atomic scheduling, configurable clocks,
//! openings, persistence and fault policy are later phase responsibilities.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use colosseum_application::{CompletePair, CpuAllocation, EngineLaunchSpec};
use colosseum_core::{
    AdjudicationConfig, EngineId, GameId, GameResult, OpeningBook, OpeningFormat, OpeningOrder,
    Termination, TimeControl, is_hash_option,
};
use colosseum_engine::{
    ClockAccountingReport, CoreClass, CpuPlacementPolicy, EngineCpuPlacement, EngineFaultKind,
    EngineGameSpec, GameFault, GameSide, GameSlotCpuAllocation, GameSpec, LiveGameState,
    ResolvedOpening, allocate_game_slots, detect_allowed_cpu_set, detect_cpu_characteristics,
    detect_cpu_topology, load_openings_named, plan_cpu_placement, run_game,
};
use colosseum_uci::SpawnOptions;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

pub const DEFAULT_BASE_MS: u64 = 3_000;
pub const DEFAULT_INCREMENT_MS: u64 = 30;
pub const DEFAULT_MARGIN_MS: u64 = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfiguredTimeControl {
    pub control: TimeControl,
    pub margin_ms: u64,
}

impl Default for ConfiguredTimeControl {
    fn default() -> Self {
        Self {
            control: TimeControl::Increment {
                base_ms: DEFAULT_BASE_MS,
                inc_ms: DEFAULT_INCREMENT_MS,
            },
            margin_ms: DEFAULT_MARGIN_MS,
        }
    }
}

impl ConfiguredTimeControl {
    fn label(self) -> String {
        match self.control {
            TimeControl::PerMove { ms } => format!("movetime/{ms}ms"),
            TimeControl::SuddenDeath { base_ms } => format!("{base_ms}ms"),
            TimeControl::Increment { base_ms, inc_ms } => {
                format!("{base_ms}ms+{inc_ms}ms")
            }
            TimeControl::Nodes { nodes } => format!("nodes/{nodes}"),
            TimeControl::Depth { depth } => format!("depth/{depth}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchSide {
    A,
    B,
}

impl MatchSide {
    fn other(self) -> Self {
        match self {
            Self::A => Self::B,
            Self::B => Self::A,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatchScore {
    pub name: String,
    pub wins: u32,
    pub losses: u32,
    pub draws: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchGame {
    pub number: u32,
    pub white: MatchSide,
    pub result: GameResult,
    pub scorable: bool,
    pub termination: Termination,
    pub clock_accounting: ClockAccountingReport,
    pub opening: OpeningAssignment,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fault: Option<GameFault>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub pgn: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchStatus {
    Completed,
    Invalid,
    InfrastructureError,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FaultPolicy {
    pub max_engine_faults: u32,
    pub max_time_losses: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchFaultCounts {
    pub engine_a: u32,
    pub engine_b: u32,
    pub time_losses_a: u32,
    pub time_losses_b: u32,
    pub infrastructure: u32,
}

impl MatchFaultCounts {
    pub(crate) fn engine_total(self) -> u32 {
        self.engine_a + self.engine_b
    }

    pub(crate) fn time_total(self) -> u32 {
        self.time_losses_a + self.time_losses_b
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FixedMatchReport {
    pub status: MatchStatus,
    pub games_requested: u32,
    pub games_attempted: u32,
    pub games_completed: u32,
    pub engine_a: MatchScore,
    pub engine_b: MatchScore,
    pub engine_a_time_control: ConfiguredTimeControl,
    pub engine_b_time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub fault_policy: FaultPolicy,
    pub faults: MatchFaultCounts,
    pub execution: MatchExecutionPlan,
    pub master_seed: u64,
    pub master_seed_generated: bool,
    pub openings: OpeningPolicyReport,
    pub games: Vec<MatchGame>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpeningAssignment {
    pub book_index: Option<usize>,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum OpeningPolicyReport {
    Startpos {
        warning: String,
    },
    Book {
        path: PathBuf,
        format: OpeningFormat,
        order: OpeningOrder,
        start_index: usize,
        plies: u32,
        available_openings: usize,
        scheduled_pairs: u32,
        reused_pair_assignments: u32,
        reuse_fraction: f64,
    },
}

#[derive(Debug, Clone)]
pub struct MatchOpenings {
    entries: Vec<ResolvedOpening>,
    report: OpeningPolicyReport,
}

#[derive(Debug, Clone)]
pub struct PairGameSettings {
    pub engine_a: EngineLaunchSpec,
    pub engine_b: EngineLaunchSpec,
    pub engine_a_time_control: ConfiguredTimeControl,
    pub engine_b_time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub openings: MatchOpenings,
}

impl MatchOpenings {
    #[must_use]
    pub fn report(&self) -> &OpeningPolicyReport {
        &self.report
    }

    fn assignment(&self, number: u32) -> (ResolvedOpening, OpeningAssignment) {
        if self.entries.is_empty() {
            return (
                ResolvedOpening::startpos(),
                OpeningAssignment {
                    book_index: None,
                    label: "startpos".into(),
                },
            );
        }
        let start_index = match self.report {
            OpeningPolicyReport::Book { start_index, .. } => start_index,
            OpeningPolicyReport::Startpos { .. } => 0,
        };
        let pair_index = (number.saturating_sub(1) / 2) as usize;
        let book_index = (start_index + pair_index) % self.entries.len();
        let opening = self.entries[book_index].clone();
        let assignment = OpeningAssignment {
            book_index: Some(book_index),
            label: opening.label.clone(),
        };
        (opening, assignment)
    }
}

pub fn resolve_openings(
    book: Option<OpeningBook>,
    start_index: usize,
    games: u32,
    master_seed: u64,
) -> Result<MatchOpenings, MatchError> {
    let Some(mut book) = book else {
        return Ok(MatchOpenings {
            entries: Vec::new(),
            report: OpeningPolicyReport::Startpos {
                warning:
                    "no opening book: every game starts from startpos; opening diversity is absent"
                        .into(),
            },
        });
    };
    book.seed = master_seed;
    let entries = load_openings_named(&book, master_seed)
        .map_err(|error| MatchError::Opening(error.to_string()))?;
    if start_index >= entries.len() {
        return Err(MatchError::BookStartOutOfRange {
            start_index,
            openings: entries.len(),
        });
    }
    let scheduled_pairs = games.div_ceil(2);
    let assigned = (0..scheduled_pairs)
        .map(|pair| (start_index + pair as usize) % entries.len())
        .collect::<Vec<_>>();
    let unique = assigned.iter().copied().collect::<BTreeSet<_>>().len() as u32;
    let reused_pair_assignments = scheduled_pairs.saturating_sub(unique);
    let reuse_fraction = if scheduled_pairs == 0 {
        0.0
    } else {
        f64::from(reused_pair_assignments) / f64::from(scheduled_pairs)
    };
    let report = OpeningPolicyReport::Book {
        path: book.path.clone(),
        format: book.format,
        order: book.order,
        start_index,
        plies: book.plies,
        available_openings: entries.len(),
        scheduled_pairs,
        reused_pair_assignments,
        reuse_fraction,
    };
    Ok(MatchOpenings { entries, report })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HashMemoryReport {
    pub engine_a_hash_mb: Option<u64>,
    pub engine_b_hash_mb: Option<u64>,
    pub lower_bound_mb: Option<u64>,
    pub formula: String,
    pub trusted_budget_mb: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MatchExecutionPlan {
    pub concurrency: usize,
    pub cores_per_engine: usize,
    pub placement_policy: CpuPlacementPolicy,
    pub slots: Vec<GameSlotCpuAllocation>,
    pub hash_memory: HashMemoryReport,
}

#[derive(Clone)]
pub struct FixedMatchRequest {
    pub engine_a: EngineLaunchSpec,
    pub engine_b: EngineLaunchSpec,
    pub games: u32,
    pub engine_a_time_control: ConfiguredTimeControl,
    pub engine_b_time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub fault_policy: FaultPolicy,
    pub execution: MatchExecutionPlan,
    pub master_seed: u64,
    pub master_seed_generated: bool,
    pub openings: MatchOpenings,
    pub completed_games: Vec<MatchGame>,
    pub progress: MatchProgress,
    pub observer: Option<Arc<dyn MatchObserver>>,
}

impl std::fmt::Debug for FixedMatchRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FixedMatchRequest")
            .field("games", &self.games)
            .field("completed_games", &self.completed_games.len())
            .field("progress", &self.progress.snapshot())
            .field("observer", &self.observer.as_ref().map(|_| "configured"))
            .finish_non_exhaustive()
    }
}

pub trait MatchObserver: Send + Sync {
    fn game_completed(&self, game: &MatchGame) -> Result<(), String>;
}

#[derive(Debug, Clone, Default)]
pub struct MatchProgress {
    attempted: Arc<AtomicU32>,
    scored: Arc<AtomicU32>,
    faults: Arc<AtomicU32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MatchProgressSnapshot {
    pub attempted: u32,
    pub scored: u32,
    pub faults: u32,
}

impl MatchProgress {
    #[must_use]
    pub fn snapshot(&self) -> MatchProgressSnapshot {
        MatchProgressSnapshot {
            attempted: self.attempted.load(Ordering::Relaxed),
            scored: self.scored.load(Ordering::Relaxed),
            faults: self.faults.load(Ordering::Relaxed),
        }
    }

    fn record(&self, game: &MatchGame) {
        self.attempted.fetch_add(1, Ordering::Relaxed);
        if game.scorable {
            self.scored.fetch_add(1, Ordering::Relaxed);
        }
        if game.fault.is_some() {
            self.faults.fetch_add(1, Ordering::Relaxed);
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MatchCheckpoint {
    pub games: Vec<MatchGame>,
}

#[derive(Debug, Error)]
pub enum MatchError {
    #[error("a fixed match needs at least one game")]
    ZeroGames,
    #[error("match concurrency must be at least one")]
    ZeroConcurrency,
    #[error("cores per engine must be at least one")]
    ZeroCoresPerEngine,
    #[error("CPU placement could not be resolved: {0}")]
    Placement(String),
    #[error("per-side --a-cores/--b-cores allocations require concurrency 1")]
    DirectCpuConcurrency,
    #[error("per-side core lists cannot be combined with a global placement policy")]
    DirectAndGlobalCpuPlacement,
    #[error("a trusted memory budget requires explicit numeric Hash options on both engines")]
    UnknownHashForBudget,
    #[error("Hash memory lower bound {required_mb} MB exceeds trusted budget {budget_mb} MB")]
    MemoryBudgetExceeded { required_mb: u64, budget_mb: u64 },
    #[error("a match worker failed: {0}")]
    Worker(String),
    #[error("opening book could not be loaded: {0}")]
    Opening(String),
    #[error("opening start index {start_index} is outside a book with {openings} openings")]
    BookStartOutOfRange { start_index: usize, openings: usize },
    #[error("durable match output failed: {0}")]
    Output(String),
    #[error("pair identity {0} cannot be represented as two game numbers")]
    PairIdentityOutOfRange(u32),
}

/// Execute both colours of one opening as a single scheduler value. The second
/// game is always attempted after the first returns; only the complete value
/// can enter the pair commit queue.
pub async fn play_pair(
    pair_id: u32,
    slot: &GameSlotCpuAllocation,
    settings: PairGameSettings,
) -> Result<CompletePair<MatchGame>, MatchError> {
    let first_number = pair_id
        .checked_mul(2)
        .and_then(|value| value.checked_sub(1))
        .ok_or(MatchError::PairIdentityOutOfRange(pair_id))?;
    if pair_id == 0 {
        return Err(MatchError::PairIdentityOutOfRange(pair_id));
    }
    let second_number = first_number
        .checked_add(1)
        .ok_or(MatchError::PairIdentityOutOfRange(pair_id))?;
    let mut engine_a = settings.engine_a;
    let mut engine_b = settings.engine_b;
    engine_a.allocated_cpus = slot.engine_a.allocation.clone();
    engine_b.allocated_cpus = slot.engine_b.allocation.clone();
    let engine_a = engine_spec(engine_a, EngineId::from_u128(1));
    let engine_b = engine_spec(engine_b, EngineId::from_u128(2));
    let (first_opening, first_assignment) = settings.openings.assignment(first_number);
    let first = play_game(GameRequest {
        number: first_number,
        engine_a: engine_a.clone(),
        engine_b: engine_b.clone(),
        time_control_a: settings.engine_a_time_control,
        time_control_b: settings.engine_b_time_control,
        adjudication: settings.adjudication,
        opening: first_opening,
        opening_assignment: first_assignment,
    })
    .await;
    let (second_opening, second_assignment) = settings.openings.assignment(second_number);
    let second = play_game(GameRequest {
        number: second_number,
        engine_a,
        engine_b,
        time_control_a: settings.engine_a_time_control,
        time_control_b: settings.engine_b_time_control,
        adjudication: settings.adjudication,
        opening: second_opening,
        opening_assignment: second_assignment,
    })
    .await;
    Ok(CompletePair {
        pair_id,
        first,
        second,
    })
}

pub fn plan_execution(
    engine_a: &EngineLaunchSpec,
    engine_b: &EngineLaunchSpec,
    concurrency: usize,
    cores_per_engine: usize,
    placement_policy: CpuPlacementPolicy,
    trusted_memory_budget_mb: Option<u64>,
) -> Result<MatchExecutionPlan, MatchError> {
    if concurrency == 0 {
        return Err(MatchError::ZeroConcurrency);
    }
    if cores_per_engine == 0 {
        return Err(MatchError::ZeroCoresPerEngine);
    }
    let direct = !matches!(engine_a.allocated_cpus, CpuAllocation::Unrestricted)
        || !matches!(engine_b.allocated_cpus, CpuAllocation::Unrestricted);
    let slots = if direct {
        if !matches!(placement_policy, CpuPlacementPolicy::Off) {
            return Err(MatchError::DirectAndGlobalCpuPlacement);
        }
        if concurrency != 1 {
            return Err(MatchError::DirectCpuConcurrency);
        }
        vec![GameSlotCpuAllocation {
            slot_index: 0,
            engine_a: direct_engine_placement(engine_a.allocated_cpus.clone()),
            engine_b: direct_engine_placement(engine_b.allocated_cpus.clone()),
            asymmetries: Vec::new(),
        }]
    } else if matches!(placement_policy, CpuPlacementPolicy::Off) {
        (0..concurrency)
            .map(|slot_index| GameSlotCpuAllocation {
                slot_index,
                engine_a: direct_engine_placement(CpuAllocation::Unrestricted),
                engine_b: direct_engine_placement(CpuAllocation::Unrestricted),
                asymmetries: Vec::new(),
            })
            .collect()
    } else {
        let topology =
            detect_cpu_topology().map_err(|error| MatchError::Placement(error.to_string()))?;
        let allowed = detect_allowed_cpu_set(&topology)
            .map_err(|error| MatchError::Placement(error.to_string()))?;
        let characteristics = detect_cpu_characteristics(&topology)
            .map_err(|error| MatchError::Placement(error.to_string()))?;
        let plan = plan_cpu_placement(&topology, &allowed, &placement_policy)
            .map_err(|error| MatchError::Placement(error.to_string()))?;
        allocate_game_slots(&plan, &characteristics, concurrency, cores_per_engine)
            .map_err(|error| MatchError::Placement(error.to_string()))?
    };
    let engine_a_hash_mb = configured_hash_mb(engine_a);
    let engine_b_hash_mb = configured_hash_mb(engine_b);
    let lower_bound_mb = engine_a_hash_mb
        .zip(engine_b_hash_mb)
        .and_then(|(a, b)| a.checked_add(b))
        .and_then(|per_slot| per_slot.checked_mul(concurrency as u64));
    if let Some(budget_mb) = trusted_memory_budget_mb {
        let required_mb = lower_bound_mb.ok_or(MatchError::UnknownHashForBudget)?;
        if required_mb > budget_mb {
            return Err(MatchError::MemoryBudgetExceeded {
                required_mb,
                budget_mb,
            });
        }
    }
    Ok(MatchExecutionPlan {
        concurrency,
        cores_per_engine,
        placement_policy,
        slots,
        hash_memory: HashMemoryReport {
            engine_a_hash_mb,
            engine_b_hash_mb,
            lower_bound_mb,
            formula: "concurrency × (engine-a Hash + engine-b Hash)".into(),
            trusted_budget_mb: trusted_memory_budget_mb,
        },
    })
}

fn direct_engine_placement(allocation: CpuAllocation) -> EngineCpuPlacement {
    let unrestricted = matches!(allocation, CpuAllocation::Unrestricted);
    EngineCpuPlacement {
        allocation,
        physical_core_count: 0,
        core_classes: if unrestricted {
            Vec::new()
        } else {
            vec![CoreClass::Unknown]
        },
        numa_nodes: Vec::new(),
    }
}

fn configured_hash_mb(engine: &EngineLaunchSpec) -> Option<u64> {
    engine
        .options
        .iter()
        .find(|(name, _)| is_hash_option(name))
        .and_then(|(_, value)| value.command_value())
        .and_then(|value| value.parse().ok())
}

/// Run exactly `games`, alternating colours by game number and retaining
/// deterministic report order independently of completion order.
/// The same executable path is valid for both sides because side identity is
/// the resolved launch specification, not the path.
pub async fn run_fixed_match(request: FixedMatchRequest) -> Result<FixedMatchReport, MatchError> {
    let FixedMatchRequest {
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
        progress,
        observer,
    } = request;
    if games == 0 {
        return Err(MatchError::ZeroGames);
    }
    let engine_a = engine_spec(engine_a, EngineId::from_u128(1));
    let engine_b = engine_spec(engine_b, EngineId::from_u128(2));
    let mut report = FixedMatchReport {
        status: MatchStatus::Completed,
        games_requested: games,
        games_attempted: 0,
        games_completed: 0,
        engine_a: MatchScore {
            name: engine_a.name.clone(),
            wins: 0,
            losses: 0,
            draws: 0,
        },
        engine_b: MatchScore {
            name: engine_b.name.clone(),
            wins: 0,
            losses: 0,
            draws: 0,
        },
        engine_a_time_control,
        engine_b_time_control,
        adjudication,
        fault_policy,
        faults: MatchFaultCounts::default(),
        execution: execution.clone(),
        master_seed,
        master_seed_generated,
        openings: openings.report().clone(),
        games: completed_games,
    };

    let mut workers = tokio::task::JoinSet::new();
    let completed_numbers = report
        .games
        .iter()
        .map(|game| game.number)
        .collect::<BTreeSet<_>>();
    let mut pending = (1..=games)
        .filter(|number| !completed_numbers.contains(number))
        .collect::<VecDeque<_>>();
    for game in &report.games {
        progress.record(game);
    }
    while !pending.is_empty() || !workers.is_empty() {
        while !pending.is_empty() && workers.len() < execution.concurrency {
            let number = pending.pop_front().expect("pending is not empty");
            let slot = &execution.slots[(number as usize - 1) % execution.slots.len()];
            let mut engine_a = engine_a.clone();
            let mut engine_b = engine_b.clone();
            engine_a.allocated_cpus = slot.engine_a.allocation.clone();
            engine_b.allocated_cpus = slot.engine_b.allocation.clone();
            let (opening, opening_assignment) = openings.assignment(number);
            workers.spawn(play_game(GameRequest {
                number,
                engine_a,
                engine_b,
                time_control_a: engine_a_time_control,
                time_control_b: engine_b_time_control,
                adjudication,
                opening,
                opening_assignment,
            }));
        }
        let Some(joined) = workers.join_next().await else {
            break;
        };
        let game = joined.map_err(|error| MatchError::Worker(error.to_string()))?;
        if let Some(observer) = &observer {
            observer.game_completed(&game).map_err(MatchError::Output)?;
        }
        progress.record(&game);
        report.games.push(game);
    }
    report.games.sort_by_key(|game| game.number);
    for game in report.games.clone() {
        let white_side = game.white;
        report.games_attempted += 1;
        if game.scorable {
            record_score(&mut report, white_side, game.result);
            report.games_completed += 1;
        }
        record_fault(&mut report.faults, white_side, game.fault.as_ref());
    }
    if report
        .games
        .iter()
        .any(|game| matches!(game.fault, Some(GameFault::Infrastructure { .. })))
    {
        report.status = MatchStatus::InfrastructureError;
    } else if report.faults.engine_total() > fault_policy.max_engine_faults
        || report.faults.time_total() > fault_policy.max_time_losses
    {
        report.status = MatchStatus::Invalid;
    }
    Ok(report)
}

struct GameRequest {
    number: u32,
    engine_a: EngineGameSpec,
    engine_b: EngineGameSpec,
    time_control_a: ConfiguredTimeControl,
    time_control_b: ConfiguredTimeControl,
    adjudication: AdjudicationConfig,
    opening: ResolvedOpening,
    opening_assignment: OpeningAssignment,
}

async fn play_game(request: GameRequest) -> MatchGame {
    let GameRequest {
        number,
        engine_a,
        engine_b,
        time_control_a: engine_a_time_control,
        time_control_b: engine_b_time_control,
        adjudication,
        opening,
        opening_assignment,
    } = request;
    let a_is_white = number % 2 == 1;
    let white_side = if a_is_white {
        MatchSide::A
    } else {
        MatchSide::B
    };
    let (white, black, white_time_control, black_time_control) = if a_is_white {
        (
            engine_a,
            engine_b,
            engine_a_time_control,
            engine_b_time_control,
        )
    } else {
        (
            engine_b,
            engine_a,
            engine_b_time_control,
            engine_a_time_control,
        )
    };
    let game_id = GameId::from_u128(u128::from(number) + 100);
    let spec = GameSpec {
        game_id,
        event: "Colosseum CLI fixed match".into(),
        site: "?".into(),
        date: "????.??.??".into(),
        round: number,
        white: white.clone(),
        black: black.clone(),
        start_fen: opening.start_fen,
        opening_moves: opening.moves,
        white_time_control: white_time_control.control,
        black_time_control: black_time_control.control,
        time_control_label: format!(
            "white {}; black {}",
            white_time_control.label(),
            black_time_control.label()
        ),
        adjudication,
        ponder: false,
        white_time_margin: Duration::from_millis(white_time_control.margin_ms),
        black_time_margin: Duration::from_millis(black_time_control.margin_ms),
        handshake_timeout: HANDSHAKE_TIMEOUT,
    };
    let live = LiveGameState::new_handle(
        game_id,
        number,
        (white.id, white.name.clone()),
        (black.id, black.name.clone()),
        spec.start_fen.clone(),
        white_time_control.control,
    );
    let game = run_game(spec, live).await;
    MatchGame {
        number,
        white: white_side,
        result: game.result,
        scorable: game.scorable,
        termination: game.termination,
        clock_accounting: game.clock_accounting,
        opening: opening_assignment,
        fault: game.fault,
        error: game.error,
        pgn: game.pgn,
    }
}

pub(crate) fn record_fault(
    counts: &mut MatchFaultCounts,
    white: MatchSide,
    fault: Option<&GameFault>,
) {
    match fault {
        Some(GameFault::Engine { side, kind, .. }) => {
            let named_side = match side {
                GameSide::White => white,
                GameSide::Black => white.other(),
            };
            let (engine, time) = match named_side {
                MatchSide::A => (&mut counts.engine_a, &mut counts.time_losses_a),
                MatchSide::B => (&mut counts.engine_b, &mut counts.time_losses_b),
            };
            *engine += 1;
            if matches!(kind, EngineFaultKind::Timeout) {
                *time += 1;
            }
        }
        Some(GameFault::Infrastructure { .. }) => counts.infrastructure += 1,
        None => {}
    }
}

fn engine_spec(launch: EngineLaunchSpec, id: EngineId) -> EngineGameSpec {
    let name = launch
        .label
        .clone()
        .unwrap_or_else(|| display_name(&launch.executable));
    EngineGameSpec {
        id,
        name,
        spawn: SpawnOptions {
            path: launch.executable,
            args: launch.arguments,
            working_dir: launch.working_directory,
            env: launch.environment,
        },
        options: launch
            .options
            .into_iter()
            .map(|(name, value)| (name, value.command_value()))
            .collect(),
        allocated_cpus: launch.allocated_cpus,
    }
}

fn display_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("engine")
        .to_owned()
}

fn record_score(report: &mut FixedMatchReport, white: MatchSide, result: GameResult) {
    let winner = match result {
        GameResult::WhiteWin => Some(white),
        GameResult::BlackWin => Some(white.other()),
        GameResult::Draw => None,
    };
    match winner {
        Some(MatchSide::A) => {
            report.engine_a.wins += 1;
            report.engine_b.losses += 1;
        }
        Some(MatchSide::B) => {
            report.engine_b.wins += 1;
            report.engine_a.losses += 1;
        }
        None => {
            report.engine_a.draws += 1;
            report.engine_b.draws += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_are_from_the_named_side_not_the_current_colour() {
        let mut report = FixedMatchReport {
            status: MatchStatus::Completed,
            games_requested: 2,
            games_attempted: 0,
            games_completed: 0,
            engine_a: MatchScore {
                name: "A".into(),
                wins: 0,
                losses: 0,
                draws: 0,
            },
            engine_b: MatchScore {
                name: "B".into(),
                wins: 0,
                losses: 0,
                draws: 0,
            },
            engine_a_time_control: ConfiguredTimeControl::default(),
            engine_b_time_control: ConfiguredTimeControl::default(),
            adjudication: AdjudicationConfig::default(),
            fault_policy: FaultPolicy::default(),
            faults: MatchFaultCounts::default(),
            execution: off_execution_plan(),
            master_seed: 1,
            master_seed_generated: false,
            openings: OpeningPolicyReport::Startpos {
                warning: "test".into(),
            },
            games: Vec::new(),
        };
        record_score(&mut report, MatchSide::A, GameResult::WhiteWin);
        record_score(&mut report, MatchSide::B, GameResult::WhiteWin);
        record_score(&mut report, MatchSide::A, GameResult::Draw);
        assert_eq!((report.engine_a.wins, report.engine_a.losses), (1, 1));
        assert_eq!((report.engine_b.wins, report.engine_b.losses), (1, 1));
        assert_eq!((report.engine_a.draws, report.engine_b.draws), (1, 1));
    }

    #[test]
    fn direct_cpu_requests_are_retained_for_one_game_slot() {
        let engine_a = EngineLaunchSpec {
            allocated_cpus: CpuAllocation::Enforced(vec![0.into()]),
            ..EngineLaunchSpec::path_only("engine".into())
        };
        let engine_b = EngineLaunchSpec {
            allocated_cpus: CpuAllocation::Enforced(vec![1.into()]),
            ..EngineLaunchSpec::path_only("engine".into())
        };
        let plan =
            plan_execution(&engine_a, &engine_b, 1, 1, CpuPlacementPolicy::Off, None).unwrap();
        assert_eq!(plan.slots[0].engine_a.allocation, engine_a.allocated_cpus);
        assert_eq!(plan.slots[0].engine_b.allocation, engine_b.allocated_cpus);
    }

    #[test]
    fn fault_counts_follow_named_engines_across_colour_reversal() {
        let fault = GameFault::Engine {
            side: GameSide::White,
            kind: EngineFaultKind::Timeout,
            message: "late".into(),
        };
        let mut counts = MatchFaultCounts::default();
        record_fault(&mut counts, MatchSide::B, Some(&fault));
        assert_eq!(counts.engine_b, 1);
        assert_eq!(counts.time_losses_b, 1);
        record_fault(
            &mut counts,
            MatchSide::A,
            Some(&GameFault::Infrastructure {
                operation: "artifact".into(),
                message: "disk full".into(),
            }),
        );
        assert_eq!(counts.infrastructure, 1);
        assert_eq!(counts.engine_a, 0);
    }

    fn off_execution_plan() -> MatchExecutionPlan {
        plan_execution(
            &EngineLaunchSpec::path_only("a".into()),
            &EngineLaunchSpec::path_only("b".into()),
            1,
            1,
            CpuPlacementPolicy::Off,
            None,
        )
        .unwrap()
    }
}
