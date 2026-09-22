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
    AllowedCpuSet, ClockAccountingReport, CoreClass, CpuCharacteristics, CpuPlacementPolicy,
    CpuTopology, EngineCpuPlacement, EngineFaultKind, EngineGameSpec, GameFault, GamePairIdentity,
    GameSide, GameSlotCpuAllocation, GameSpec, KeptEngine, LiveGameState, OpeningList,
    ResolvedOpening, SlotAllocation, allocate_game_slots, detect_allowed_cpu_set,
    detect_cpu_characteristics, detect_cpu_topology, load_openings_named, plan_cpu_placement,
    run_game, run_game_keeping,
};
use colosseum_uci::SpawnOptions;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::cancellation::Cancellation;
use crate::journal::GameRecord;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

pub const DEFAULT_BASE_MS: u64 = 3_000;
pub const DEFAULT_INCREMENT_MS: u64 = 30;
/// How far past its clock a move may arrive before it forfeits: enough for
/// the harness's own delivery (about a millisecond measured) and the rare
/// scheduling delay, as fastchess and fishtest set it. An engine known to
/// stall for longer is given more with `--margin-ms`.
pub const DEFAULT_MARGIN_MS: u64 = 20;

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
    pub(crate) fn other(self) -> Self {
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

/// Which CPU slot a game ran on, and when it held it.
///
/// The span runs from before the game's first engine was spawned to after both
/// of its engine processes had exited, in microseconds since the Unix epoch.
/// Two games on one slot never overlap; a run's journal is the record of that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotOccupancy {
    /// The slot, counting from zero, as the execution plan numbers it.
    pub index: usize,
    pub started_unix_us: u64,
    pub ended_unix_us: u64,
}

/// The slots of a run's execution plan that no unit holds.
///
/// A unit — a game, or a colour-reversed pair — takes a slot before its first
/// engine is spawned and gives it back only after the engine processes of its
/// last game have exited. Choosing a slot by arithmetic on the game number
/// placed a new game on a slot still playing while a finished one idled: two
/// games on one pinned CPU, whose searches then alternate in scheduler quanta.
/// Taking a free slot cannot do that, and a run keeps exactly as many units
/// live as it has slots.
#[derive(Debug)]
pub struct SlotPool {
    held: Vec<bool>,
}

impl SlotPool {
    fn new(slots: usize) -> Self {
        Self {
            held: vec![false; slots],
        }
    }

    /// Take the lowest-numbered free slot, or `None` when every slot is held.
    pub fn take(&mut self) -> Option<usize> {
        let position = self.held.iter().position(|held| !held)?;
        self.held[position] = true;
        Some(position)
    }

    /// Give a slot back. Its unit's engines have exited.
    pub fn give_back(&mut self, position: usize) {
        debug_assert!(
            self.held.get(position).copied().unwrap_or(false),
            "slot {position} given back while free"
        );
        if let Some(held) = self.held.get_mut(position) {
            *held = false;
        }
    }

    /// Slots currently held: the number of live units.
    #[must_use]
    pub fn held(&self) -> usize {
        self.held.iter().filter(|held| **held).count()
    }
}

/// Microseconds since the Unix epoch, anchored once per process to the wall
/// clock and advanced by the monotonic clock. The journal's slot spans read
/// as a timeline, and a wall-clock step during a run (a time sync) cannot
/// make two spans on one slot appear to overlap.
fn unix_us() -> u64 {
    static ANCHOR: std::sync::OnceLock<(u64, std::time::Instant)> = std::sync::OnceLock::new();
    let (wall_us, started) = ANCHOR.get_or_init(|| {
        let wall = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_micros()).unwrap_or(u64::MAX)
            });
        (wall, std::time::Instant::now())
    });
    let elapsed = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
    wall_us.saturating_add(elapsed)
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
    /// The game's moves, held only from the moment the game ends until it is
    /// committed to `games.pgn`, and never stored, reported or kept by a
    /// driver after that. Thirty thousand games of moves in memory, and again
    /// in every report, was most of what a long run carried.
    #[serde(skip)]
    pub pgn: String,
    /// The CPU slot the game ran on and when it held it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot: Option<SlotOccupancy>,
}

impl MatchGame {
    /// The journal record of this game.
    #[must_use]
    pub fn journal_record(&self, sample: &str, iteration: Option<u32>) -> GameRecord {
        let side = |side: MatchSide| match side {
            MatchSide::A => "a".to_owned(),
            MatchSide::B => "b".to_owned(),
        };
        GameRecord {
            number: self.number,
            pair_number: self.number.div_ceil(2),
            pair_game: if self.number % 2 == 1 { 1 } else { 2 },
            white: side(self.white),
            black: side(self.white.other()),
            result: self.result,
            scorable: self.scorable,
            termination: self.termination,
            opening: self.opening.clone(),
            fault: self.fault.clone(),
            sample: sample.to_owned(),
            clock: self.clock_accounting.clone(),
            iteration,
            round: None,
            error: self.error.clone(),
            slot: self.slot,
        }
    }

    /// The game a journal record describes, without its moves. `None` when the
    /// record is not a two-engine game.
    #[must_use]
    pub fn from_journal(record: &GameRecord) -> Option<Self> {
        let white = match record.white.as_str() {
            "a" => MatchSide::A,
            "b" => MatchSide::B,
            _ => return None,
        };
        Some(Self {
            number: record.number,
            white,
            result: record.result,
            scorable: record.scorable,
            termination: record.termination,
            clock_accounting: record.clock.clone(),
            opening: record.opening.clone(),
            fault: record.fault.clone(),
            error: record.error.clone(),
            pgn: String::new(),
            slot: record.slot,
        })
    }

    /// Release the moves once they are committed.
    pub fn strip_moves(&mut self) {
        self.pgn = String::new();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum MatchStatus {
    Completed,
    /// Stopped cleanly on request before every requested game was played. The
    /// games already scored are kept and the run directory resumes.
    Cancelled,
    Invalid,
    InfrastructureError,
}

/// When engine faults make a run invalid.
///
/// A fixed-size run has fixed limits. A sequential run cannot know its length,
/// so a limit it was not given explicitly grows with the games it has played:
/// at every commit it is the larger of the floor and a rate of those games. A
/// rare forfeit — an operating system holding a process for tens of
/// milliseconds — is then scored as the loss it is and the test goes on, while
/// an engine that keeps faulting still voids it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FaultPolicy {
    /// Engine faults tolerated, or the floor of the growing limit.
    pub max_engine_faults: u32,
    /// Time losses tolerated, or the floor of the growing limit. A time loss
    /// is also an engine fault.
    pub max_time_losses: u32,
    /// Which limits grow with the games played, and at what rate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<FaultRate>,
}

/// A fault limit that rises with the games played.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct FaultRate {
    /// Faults tolerated per thousand games played.
    pub per_mille: u32,
    /// The engine-fault limit grows.
    pub engine_faults: bool,
    /// The time-loss limit grows.
    pub time_losses: bool,
}

/// The fewest engine faults a sequential run tolerates by default.
pub const SEQUENTIAL_FAULT_FLOOR: u32 = 3;
/// Engine faults a sequential run tolerates per thousand games played: 0.5%.
pub const SEQUENTIAL_FAULT_PER_MILLE: u32 = 5;

impl FaultPolicy {
    /// A sequential run's policy from its two flags. An omitted engine-fault
    /// limit grows from [`SEQUENTIAL_FAULT_FLOOR`] at
    /// [`SEQUENTIAL_FAULT_PER_MILLE`]; an omitted time-loss limit follows it,
    /// since a time loss is an engine fault. A limit given explicitly stays
    /// fixed, so `0` invalidates on the first fault of its kind.
    #[must_use]
    pub fn sequential(max_engine_faults: Option<u32>, max_time_losses: Option<u32>) -> Self {
        let engine_grows = max_engine_faults.is_none();
        let time_grows = max_time_losses.is_none() && engine_grows;
        let max_engine_faults = max_engine_faults.unwrap_or(SEQUENTIAL_FAULT_FLOOR);
        Self {
            max_engine_faults,
            max_time_losses: max_time_losses.unwrap_or(max_engine_faults),
            rate: (engine_grows || time_grows).then_some(FaultRate {
                per_mille: SEQUENTIAL_FAULT_PER_MILLE,
                engine_faults: engine_grows,
                time_losses: time_grows,
            }),
        }
    }

    /// Engine faults tolerated after `games` games.
    #[must_use]
    pub fn engine_limit(&self, games: u64) -> u64 {
        grown(
            self.max_engine_faults,
            self.rate.filter(|rate| rate.engine_faults),
            games,
        )
    }

    /// Time losses tolerated after `games` games.
    #[must_use]
    pub fn time_limit(&self, games: u64) -> u64 {
        grown(
            self.max_time_losses,
            self.rate.filter(|rate| rate.time_losses),
            games,
        )
    }

    /// Whether `faults` over `games` games make the run invalid.
    #[must_use]
    pub fn exceeded(&self, faults: MatchFaultCounts, games: u64) -> bool {
        u64::from(faults.engine_total()) > self.engine_limit(games)
            || u64::from(faults.time_total()) > self.time_limit(games)
    }
}

fn grown(floor: u32, rate: Option<FaultRate>, games: u64) -> u64 {
    let floor = u64::from(floor);
    rate.map_or(floor, |rate| {
        floor.max(games.saturating_mul(u64::from(rate.per_mille)) / 1000)
    })
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
    /// Add another count to this one.
    pub(crate) fn absorb(&mut self, other: Self) {
        self.engine_a += other.engine_a;
        self.engine_b += other.engine_b;
        self.time_losses_a += other.time_losses_a;
        self.time_losses_b += other.time_losses_b;
        self.infrastructure += other.infrastructure;
    }

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
    pub ponder: bool,
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
        /// Openings this schedule consumes: one per colour-reversed pair, or
        /// one per tournament encounter.
        scheduled_openings: u32,
        /// The exact zero-based index range the run will consume, inclusive.
        first_index: usize,
        last_index: usize,
        /// Modular reuse was explicitly opted into.
        wrap: bool,
        reused_openings: u32,
        reuse_fraction: f64,
    },
}

#[derive(Debug, Clone)]
pub struct MatchOpenings {
    // Shared, never copied: a book holds millions of openings and these
    // settings are cloned once per launched game or pair. A deep copy cost
    // about half a second on the one task that launches games, which
    // serialised every SPSA iteration and every SPRT pair behind it.
    entries: Arc<OpeningList>,
    report: OpeningPolicyReport,
}

/// Whether a slot's engines live for one game or for the whole run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, clap::ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum EngineProcesses {
    /// Each slot keeps its two engine processes from game to game, restarting
    /// one after a fault or when its launch or option names change, as a
    /// runner that keeps engines for a whole tournament does. An engine's
    /// one-time work is paid once per run, not inside a game's search.
    #[default]
    PerSlot,
    /// Two fresh processes per game, ended with it.
    PerGame,
}

/// The engines each slot keeps between its games, one per side: index 0 is
/// engine A, 1 is engine B, whichever colour they play. A slot has one live
/// game at a time, so a slot's engines are never shared between two games.
pub struct SlotEngines {
    slots: Vec<tokio::sync::Mutex<[Option<KeptEngine>; 2]>>,
}

impl std::fmt::Debug for SlotEngines {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SlotEngines")
            .field("slots", &self.slots.len())
            .finish()
    }
}

impl SlotEngines {
    /// A store for `mode`: `None` when every game starts its own engines.
    #[must_use]
    pub fn for_mode(mode: EngineProcesses, slots: usize) -> Option<Arc<Self>> {
        (mode == EngineProcesses::PerSlot).then(|| {
            Arc::new(Self {
                slots: (0..slots)
                    .map(|_| tokio::sync::Mutex::new([None, None]))
                    .collect(),
            })
        })
    }

    async fn take(&self, slot: usize) -> [Option<KeptEngine>; 2] {
        match self.slots.get(slot) {
            Some(engines) => std::mem::take(&mut *engines.lock().await),
            None => [None, None],
        }
    }

    async fn put(&self, slot: usize, engines: [Option<KeptEngine>; 2]) {
        if let Some(stored) = self.slots.get(slot) {
            *stored.lock().await = engines;
        }
    }

    /// Quit every kept engine. A run calls this when it ends; engines it
    /// never reaches are killed with their handles.
    pub async fn shutdown(&self) {
        for slot in &self.slots {
            let engines = std::mem::take(&mut *slot.lock().await);
            for engine in engines.into_iter().flatten() {
                engine.quit().await;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct PairGameSettings {
    pub engine_a: EngineLaunchSpec,
    pub engine_b: EngineLaunchSpec,
    pub engine_a_time_control: ConfiguredTimeControl,
    pub engine_b_time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub ponder: bool,
    pub openings: MatchOpenings,
    /// Internal, for scale tests: play each game in-process as an instant
    /// result instead of launching engines, so a tune of thousands of
    /// iterations can exercise everything around the games.
    pub synthetic_games: bool,
    /// The engines kept per slot, when a slot keeps its engines between
    /// games.
    pub engines: Option<Arc<SlotEngines>>,
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
        let opening = self.entries.get(book_index);
        let assignment = OpeningAssignment {
            book_index: Some(book_index),
            label: opening.label.clone(),
        };
        (opening, assignment)
    }

    pub(crate) fn select_encounter(&self, encounter: u32) -> (Self, OpeningAssignment) {
        if self.entries.is_empty() {
            return (
                self.clone(),
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
        let book_index = (start_index + encounter.saturating_sub(1) as usize) % self.entries.len();
        let opening = self.entries.get(book_index);
        let assignment = OpeningAssignment {
            book_index: Some(book_index),
            label: opening.label.clone(),
        };
        (
            Self {
                entries: Arc::new(OpeningList::from_resolved([opening])),
                report: OpeningPolicyReport::Startpos {
                    warning: "opening preselected by the tournament encounter".into(),
                },
            },
            assignment,
        )
    }
}

/// Resolve the openings a schedule will consume.
///
/// `required` is how many book entries the schedule needs: one per
/// colour-reversed pair for a match, SPRT, calibration or tune, and one per
/// encounter for a tournament. Entries are consumed sequentially from
/// `start_index` in the resolved order. A run that needs more than remain is
/// refused rather than wrapping silently, because two segments of one book
/// that quietly replay the same openings narrow the error bars of both.
pub fn resolve_openings(
    book: Option<OpeningBook>,
    start_index: usize,
    required: u32,
    wrap: bool,
    master_seed: u64,
) -> Result<MatchOpenings, MatchError> {
    let Some(mut book) = book else {
        return Ok(MatchOpenings {
            entries: Arc::new(OpeningList::default()),
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
    let remaining = entries.len() - start_index;
    if !wrap && required as usize > remaining {
        return Err(MatchError::BookExhausted {
            start_index,
            openings: entries.len(),
            remaining,
            required: required as usize,
            shortfall: required as usize - remaining,
        });
    }
    let assigned = (0..required)
        .map(|index| (start_index + index as usize) % entries.len())
        .collect::<Vec<_>>();
    let unique = assigned.iter().copied().collect::<BTreeSet<_>>().len() as u32;
    let reused_openings = required.saturating_sub(unique);
    let reuse_fraction = if required == 0 {
        0.0
    } else {
        f64::from(reused_openings) / f64::from(required)
    };
    let report = OpeningPolicyReport::Book {
        path: book.path.clone(),
        format: book.format,
        order: book.order,
        start_index,
        plies: book.plies,
        available_openings: entries.len(),
        scheduled_openings: required,
        first_index: assigned.first().copied().unwrap_or(start_index),
        last_index: assigned.last().copied().unwrap_or(start_index),
        wrap,
        reused_openings,
        reuse_fraction,
    };
    Ok(MatchOpenings {
        entries: Arc::new(entries),
        report,
    })
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
    /// How each slot's cores are divided between its two engine processes.
    pub allocation: SlotAllocation,
    pub placement_policy: CpuPlacementPolicy,
    pub slots: Vec<GameSlotCpuAllocation>,
    pub hash_memory: HashMemoryReport,
}

impl MatchExecutionPlan {
    /// The free-slot pool of one run of this plan. A plan whose slots do not
    /// match its concurrency is refused: sharing a slot is the defect the
    /// pool exists to prevent.
    pub fn slot_pool(&self) -> Result<SlotPool, MatchError> {
        if self.slots.len() != self.concurrency {
            return Err(MatchError::SlotCountMismatch {
                slots: self.slots.len(),
                concurrency: self.concurrency,
            });
        }
        Ok(SlotPool::new(self.slots.len()))
    }
}

#[derive(Clone)]
pub struct FixedMatchRequest {
    pub engine_a: EngineLaunchSpec,
    pub engine_b: EngineLaunchSpec,
    pub games: u32,
    pub engine_a_time_control: ConfiguredTimeControl,
    pub engine_b_time_control: ConfiguredTimeControl,
    pub adjudication: AdjudicationConfig,
    pub ponder: bool,
    pub engine_processes: EngineProcesses,
    pub fault_policy: FaultPolicy,
    pub execution: MatchExecutionPlan,
    pub master_seed: u64,
    pub master_seed_generated: bool,
    pub openings: MatchOpenings,
    pub completed_games: Vec<MatchGame>,
    pub progress: MatchProgress,
    pub cancellation: Cancellation,
    /// Schedule identity to write instead of the one this match would derive.
    ///
    /// A tournament plays each game through a one-game match, which would
    /// otherwise call every game the first assignment of its own first pair.
    /// The tournament knows the encounter and the opening it selected, so it
    /// supplies them rather than having them patched into the written text.
    pub identity_override: Option<GamePairIdentity>,
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
    #[error(
        "this schedule needs {required} openings but only {remaining} remain from --book-start {start_index} in a book of {openings}; it is short by {shortfall}. Supply a larger book, lower --book-start, shorten the schedule, or pass --book-wrap to reuse openings deliberately"
    )]
    BookExhausted {
        start_index: usize,
        openings: usize,
        remaining: usize,
        required: usize,
        shortfall: usize,
    },
    #[error("durable match output failed: {0}")]
    Output(String),
    #[error("pair identity {0} cannot be represented as two game numbers")]
    PairIdentityOutOfRange(u32),
    #[error(
        "the execution plan has {slots} CPU slots for {concurrency} concurrent games; every live game needs a slot of its own"
    )]
    SlotCountMismatch { slots: usize, concurrency: usize },
}

/// Execute both colours of one opening as a single scheduler value. The second
/// game is always attempted after the first returns; only the complete value
/// can enter the pair commit queue. `sprt` plays its pairs this way: the pair is
/// its slot-holding unit, and both games run on the one slot it holds.
pub async fn play_pair(
    pair_id: u32,
    slot: &GameSlotCpuAllocation,
    settings: PairGameSettings,
) -> Result<CompletePair<MatchGame>, MatchError> {
    let (first_number, second_number) = pair_game_numbers(pair_id)?;
    let first = play_pair_game(first_number, slot, &settings).await;
    let second = play_pair_game(second_number, slot, &settings).await;
    Ok(CompletePair {
        pair_id,
        first,
        second,
    })
}

/// The two game numbers of a pair: `2p − 1` with engine A as White, then `2p`
/// with the colours reversed on the same opening.
pub fn pair_game_numbers(pair_id: u32) -> Result<(u32, u32), MatchError> {
    if pair_id == 0 {
        return Err(MatchError::PairIdentityOutOfRange(pair_id));
    }
    let first = pair_id
        .checked_mul(2)
        .and_then(|value| value.checked_sub(1))
        .ok_or(MatchError::PairIdentityOutOfRange(pair_id))?;
    let second = first
        .checked_add(1)
        .ok_or(MatchError::PairIdentityOutOfRange(pair_id))?;
    Ok((first, second))
}

/// Play one game of a colour-reversed pair schedule on `slot`: its colours,
/// opening and identity come from its number exactly as when its pair is
/// played as a unit. A caller that places games individually — an SPSA
/// mini-match — reassembles the pairs by their identity afterwards.
pub async fn play_pair_game(
    number: u32,
    slot: &GameSlotCpuAllocation,
    settings: &PairGameSettings,
) -> MatchGame {
    if settings.synthetic_games {
        return synthetic_game(number, slot, settings);
    }
    let mut engine_a = settings.engine_a.clone();
    let mut engine_b = settings.engine_b.clone();
    engine_a.allocated_cpus = slot.engine_a.allocation.clone();
    engine_b.allocated_cpus = slot.engine_b.allocation.clone();
    let (opening, opening_assignment) = settings.openings.assignment(number);
    play_game(GameRequest {
        number,
        slot: slot.slot_index,
        engines: settings.engines.clone(),
        identity_override: None,
        engine_a: engine_spec(engine_a, EngineId::from_u128(1)),
        engine_b: engine_spec(engine_b, EngineId::from_u128(2)),
        time_control_a: settings.engine_a_time_control,
        time_control_b: settings.engine_b_time_control,
        adjudication: settings.adjudication,
        ponder: settings.ponder,
        opening,
        opening_assignment,
    })
    .await
}

/// An instant game with a result drawn from its number, for scale tests. It
/// carries everything a real game's record does — identity, opening, slot and
/// a PGN with the schedule tags — and no moves.
fn synthetic_game(
    number: u32,
    slot: &GameSlotCpuAllocation,
    settings: &PairGameSettings,
) -> MatchGame {
    let started_unix_us = unix_us();
    let (_, opening) = settings.openings.assignment(number);
    let a_is_white = number % 2 == 1;
    let result = match number % 3 {
        0 => GameResult::Draw,
        1 => GameResult::WhiteWin,
        _ => GameResult::BlackWin,
    };
    let pgn = colosseum_engine::pgn::build_pgn(
        &colosseum_engine::pgn::PgnTags {
            event: "Colosseum CLI synthetic game".into(),
            site: "?".into(),
            date: "????.??.??".into(),
            round: number,
            white: if a_is_white { "A" } else { "B" }.into(),
            black: if a_is_white { "B" } else { "A" }.into(),
            result,
            time_control: String::new(),
            termination: Some(Termination::MaxMoves),
            fen: None,
            opening_plies: 0,
            identity: Some(GamePairIdentity {
                game_number: number,
                pair_number: number.div_ceil(2),
                pair_game: if a_is_white { 1 } else { 2 },
                opening_index: opening.book_index,
                opening_label: opening.label.clone(),
            }),
            time_margins_ms: None,
            slot: Some(slot.slot_index),
            forfeited_search: None,
        },
        &[],
        &[],
    );
    MatchGame {
        number,
        white: if a_is_white {
            MatchSide::A
        } else {
            MatchSide::B
        },
        result,
        scorable: true,
        termination: Termination::MaxMoves,
        clock_accounting: ClockAccountingReport {
            model: "synthetic".into(),
            version: 0,
            white_margin_ms: 0,
            black_margin_ms: 0,
            monotonic_resolution_ns: 1,
            white_charged_elapsed: None,
            black_charged_elapsed: None,
            white_round_trip: None,
            black_round_trip: None,
            phases: None,
        },
        opening,
        fault: None,
        error: None,
        pgn,
        slot: Some(SlotOccupancy {
            index: slot.slot_index,
            started_unix_us,
            ended_unix_us: unix_us(),
        }),
    }
}

/// The host's CPUs as the operating system reports them: what a placement
/// policy other than `off` divides between game slots.
#[derive(Debug, Clone)]
pub struct HostCpus {
    pub topology: CpuTopology,
    pub allowed: AllowedCpuSet,
    pub characteristics: CpuCharacteristics,
}

impl HostCpus {
    /// Read the host's topology, the CPUs this process may use and what the
    /// operating system says about each core.
    pub fn detect() -> Result<Self, MatchError> {
        let topology =
            detect_cpu_topology().map_err(|error| MatchError::Placement(error.to_string()))?;
        let allowed = detect_allowed_cpu_set(&topology)
            .map_err(|error| MatchError::Placement(error.to_string()))?;
        let characteristics = detect_cpu_characteristics(&topology)
            .map_err(|error| MatchError::Placement(error.to_string()))?;
        Ok(Self {
            topology,
            allowed,
            characteristics,
        })
    }
}

pub fn plan_execution(
    engine_a: &EngineLaunchSpec,
    engine_b: &EngineLaunchSpec,
    concurrency: usize,
    allocation: SlotAllocation,
    placement_policy: CpuPlacementPolicy,
    trusted_memory_budget_mb: Option<u64>,
) -> Result<MatchExecutionPlan, MatchError> {
    plan_execution_on(
        HostCpus::detect,
        engine_a,
        engine_b,
        concurrency,
        allocation,
        placement_policy,
        trusted_memory_budget_mb,
    )
}

/// [`plan_execution`] against the CPUs `host` reports. The host is read only
/// when the placement policy needs it, so `off` and direct CPU requests plan
/// the same on every machine.
pub fn plan_execution_on(
    host: impl FnOnce() -> Result<HostCpus, MatchError>,
    engine_a: &EngineLaunchSpec,
    engine_b: &EngineLaunchSpec,
    concurrency: usize,
    allocation: SlotAllocation,
    placement_policy: CpuPlacementPolicy,
    trusted_memory_budget_mb: Option<u64>,
) -> Result<MatchExecutionPlan, MatchError> {
    if concurrency == 0 {
        return Err(MatchError::ZeroConcurrency);
    }
    if allocation.cores_per_engine() == 0 {
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
        let host = host()?;
        let plan = plan_cpu_placement(
            &host.topology,
            &host.allowed,
            &host.characteristics,
            &placement_policy,
        )
        .map_err(|error| MatchError::Placement(error.to_string()))?;
        allocate_game_slots(&plan, &host.characteristics, concurrency, allocation)
            .map_err(|error| MatchError::Placement(error.to_string()))?
    };
    if slots.len() != concurrency {
        return Err(MatchError::SlotCountMismatch {
            slots: slots.len(),
            concurrency,
        });
    }
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
        allocation,
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
        cache_domains: Vec::new(),
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
        ponder,
        engine_processes,
        fault_policy,
        execution,
        master_seed,
        master_seed_generated,
        openings,
        completed_games,
        progress,
        cancellation,
        identity_override,
        observer,
    } = request;
    if games == 0 {
        return Err(MatchError::ZeroGames);
    }
    let kept_engines = SlotEngines::for_mode(engine_processes, execution.slots.len());
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
        ponder,
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
    let mut pool = execution.slot_pool()?;
    while (!pending.is_empty() && !cancellation.stopping()) || !workers.is_empty() {
        while !pending.is_empty()
            && !cancellation.stopping()
            && workers.len() < execution.concurrency
        {
            let number = pending.pop_front().expect("pending is not empty");
            let position = pool
                .take()
                .expect("a run keeps no more units live than it has slots");
            debug_assert_eq!(pool.held(), workers.len() + 1);
            let slot = &execution.slots[position];
            let mut engine_a = engine_a.clone();
            let mut engine_b = engine_b.clone();
            engine_a.allocated_cpus = slot.engine_a.allocation.clone();
            engine_b.allocated_cpus = slot.engine_b.allocation.clone();
            let (opening, opening_assignment) = openings.assignment(number);
            let game = play_game(GameRequest {
                number,
                slot: slot.slot_index,
                engines: kept_engines.clone(),
                identity_override: identity_override.clone(),
                engine_a,
                engine_b,
                time_control_a: engine_a_time_control,
                time_control_b: engine_b_time_control,
                adjudication,
                ponder,
                opening,
                opening_assignment,
            });
            workers.spawn(async move { (position, game.await) });
        }
        // One cancellation path: stop launching, then give the games in flight
        // their bounded grace before abandoning them.
        let joined = tokio::select! {
            joined = workers.join_next() => joined,
            () = cancellation.abandon() => {
                workers.abort_all();
                break;
            }
        };
        let Some(joined) = joined else {
            break;
        };
        let (position, mut game) = joined.map_err(|error| MatchError::Worker(error.to_string()))?;
        // The game's engines have exited: `play_game` returns only then.
        pool.give_back(position);
        if let Some(observer) = &observer {
            observer.game_completed(&game).map_err(MatchError::Output)?;
            // The observer committed the moves; the report keeps the summary.
            // A match with no observer is a tournament's one-game match, whose
            // caller commits the moves itself.
            game.strip_moves();
        }
        progress.record(&game);
        report.games.push(game);
        cancellation.record_committed_unit();
    }
    if let Some(engines) = &kept_engines {
        engines.shutdown().await;
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
    } else if fault_policy.exceeded(report.faults, report.games.len() as u64) {
        report.status = MatchStatus::Invalid;
    } else if cancellation.stopping() && report.games.len() < games as usize {
        // A stop is only a stop when work was actually left undone. An
        // interrupt that arrives while the last game is being joined still
        // produced every requested game, and that is a completed match.
        report.status = MatchStatus::Cancelled;
    }
    Ok(report)
}

struct GameRequest {
    number: u32,
    /// The slot index recorded with the game.
    slot: usize,
    /// The slot's kept engines, when it keeps them between games.
    engines: Option<Arc<SlotEngines>>,
    identity_override: Option<GamePairIdentity>,
    engine_a: EngineGameSpec,
    engine_b: EngineGameSpec,
    time_control_a: ConfiguredTimeControl,
    time_control_b: ConfiguredTimeControl,
    adjudication: AdjudicationConfig,
    ponder: bool,
    opening: ResolvedOpening,
    opening_assignment: OpeningAssignment,
}

async fn play_game(request: GameRequest) -> MatchGame {
    let GameRequest {
        number,
        slot,
        engines,
        identity_override,
        engine_a,
        engine_b,
        time_control_a: engine_a_time_control,
        time_control_b: engine_b_time_control,
        adjudication,
        ponder,
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
        ponder,
        white_time_margin: Duration::from_millis(white_time_control.margin_ms),
        black_time_margin: Duration::from_millis(black_time_control.margin_ms),
        handshake_timeout: HANDSHAKE_TIMEOUT,
        // Games are scheduled as colour-reversed pairs: the odd game of a pair
        // is engine A as White, the even one the same opening reversed.
        identity: Some(identity_override.unwrap_or_else(|| GamePairIdentity {
            game_number: number,
            pair_number: number.div_ceil(2),
            pair_game: if a_is_white { 1 } else { 2 },
            opening_index: opening_assignment.book_index,
            opening_label: opening_assignment.label.clone(),
        })),
        slot: Some(slot),
    };
    let live = LiveGameState::new_handle(
        game_id,
        number,
        (white.id, white.name.clone()),
        (black.id, black.name.clone()),
        spec.start_fen.clone(),
        white_time_control.control,
    );
    let started_unix_us = unix_us();
    // `run_game` returns once both engine processes have exited; with kept
    // engines, once both are ready for the slot's next game or have exited.
    let game = match &engines {
        Some(engines) => {
            let [a, b] = engines.take(slot).await;
            let kept = if a_is_white { [a, b] } else { [b, a] };
            let (game, [white, black]) = run_game_keeping(spec, live, kept).await;
            let sides = if a_is_white {
                [white, black]
            } else {
                [black, white]
            };
            engines.put(slot, sides).await;
            game
        }
        None => run_game(spec, live).await,
    };
    let ended_unix_us = unix_us();
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
        slot: Some(SlotOccupancy {
            index: slot,
            started_unix_us,
            ended_unix_us,
        }),
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

    /// The invariant: no two live units hold the same slot, and a unit waits
    /// only when every slot is held. Units here finish in an arbitrary order,
    /// as games of different lengths do.
    #[test]
    fn a_slot_is_held_by_one_unit_at_a_time_and_none_waits_while_one_is_free() {
        let mut pool = SlotPool::new(4);
        let mut live: Vec<(u32, usize)> = Vec::new();
        let finishing_order = [2, 0, 3, 1, 1, 0, 2, 0, 1, 0];
        let mut next_unit = 1;
        for finishing in finishing_order {
            while let Some(slot) = pool.take() {
                assert!(
                    live.iter().all(|(_, held)| *held != slot),
                    "slot {slot} handed to unit {next_unit} while held: {live:?}"
                );
                live.push((next_unit, slot));
                next_unit += 1;
            }
            // Nothing waits while a slot is free: the pool is exhausted
            // exactly when every slot is held.
            assert_eq!(live.len(), 4);
            assert_eq!(pool.held(), 4);
            let (_, slot) = live.remove(finishing % live.len());
            pool.give_back(slot);
            assert_eq!(pool.held(), 3);
        }
        // The freed slot is the one handed out next, whatever the unit's
        // number: arithmetic on the number would pick another.
        let (_, slot) = live.remove(0);
        pool.give_back(slot);
        assert_eq!(pool.take(), Some(slot));
    }

    #[test]
    fn a_sequential_allowance_is_the_larger_of_three_and_half_a_percent_of_games_played() {
        let faults = |engine: u32, time: u32| MatchFaultCounts {
            engine_a: engine,
            time_losses_a: time,
            ..MatchFaultCounts::default()
        };
        let policy = FaultPolicy::sequential(None, None);
        assert_eq!(policy.engine_limit(0), 3);
        assert_eq!(policy.engine_limit(799), 3);
        assert_eq!(policy.engine_limit(1_000), 5);
        assert_eq!(policy.engine_limit(20_000), 100);
        // Time losses are engine faults and follow the same limit.
        assert_eq!(policy.time_limit(20_000), 100);
        assert!(!policy.exceeded(faults(3, 3), 4));
        assert!(policy.exceeded(faults(4, 4), 4));
        assert!(!policy.exceeded(faults(5, 5), 1_000));
        assert!(policy.exceeded(faults(6, 0), 1_100));

        // An explicit zero is strict, whatever the games played.
        let strict = FaultPolicy::sequential(Some(0), None);
        assert!(strict.rate.is_none());
        assert!(strict.exceeded(faults(1, 1), 20_000));
        // A strict time-loss limit leaves other engine faults to the rate.
        let time_strict = FaultPolicy::sequential(None, Some(0));
        assert_eq!(time_strict.engine_limit(20_000), 100);
        assert_eq!(time_strict.time_limit(20_000), 0);
        assert!(time_strict.exceeded(faults(1, 1), 20_000));
        assert!(!time_strict.exceeded(faults(1, 0), 20_000));
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "given back while free")]
    fn giving_back_a_free_slot_is_a_bug_caught_in_debug_builds() {
        let mut pool = SlotPool::new(2);
        pool.give_back(1);
    }

    #[test]
    fn a_plan_whose_slots_differ_from_its_concurrency_is_refused() {
        let mut plan = plan_execution(
            &EngineLaunchSpec::path_only("a".into()),
            &EngineLaunchSpec::path_only("b".into()),
            3,
            SlotAllocation::Shared { cores_per_game: 1 },
            CpuPlacementPolicy::Off,
            None,
        )
        .unwrap();
        assert_eq!(plan.slots.len(), 3);
        assert!(plan.slot_pool().is_ok());
        plan.slots.pop();
        let error = plan.slot_pool().unwrap_err();
        assert!(
            matches!(
                error,
                MatchError::SlotCountMismatch {
                    slots: 2,
                    concurrency: 3
                }
            ),
            "{error}"
        );
    }

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
            ponder: false,
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
        let plan = plan_execution(
            &engine_a,
            &engine_b,
            1,
            SlotAllocation::Shared { cores_per_game: 1 },
            CpuPlacementPolicy::Off,
            None,
        )
        .unwrap();
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
            SlotAllocation::Shared { cores_per_game: 1 },
            CpuPlacementPolicy::Off,
            None,
        )
        .unwrap()
    }

    /// A host of `cores` physical cores without SMT, every CPU allowed and
    /// nothing known about any core's class, node or cache.
    fn host(cores: u32) -> HostCpus {
        let topology = CpuTopology {
            source: colosseum_engine::TopologySource::LinuxThreadSiblingsList,
            physical_core_count: cores as usize,
            logical_cpu_count: cores as usize,
            sibling_mapping: colosseum_engine::SiblingMapping::Known {
                cores: (0..cores)
                    .map(|cpu| colosseum_engine::PhysicalCore {
                        logical_cpus: vec![cpu.into()],
                    })
                    .collect(),
            },
        };
        HostCpus {
            allowed: AllowedCpuSet::Known {
                source: colosseum_engine::AllowedCpuSource::LinuxSchedulerAffinity,
                cpus: (0..cores).map(Into::into).collect(),
            },
            characteristics: CpuCharacteristics::unknown(&topology).unwrap(),
            topology,
        }
    }

    fn plan_on(
        cores: u32,
        concurrency: usize,
        allocation: SlotAllocation,
    ) -> Result<MatchExecutionPlan, MatchError> {
        plan_execution_on(
            || Ok(host(cores)),
            &EngineLaunchSpec::path_only("a".into()),
            &EngineLaunchSpec::path_only("b".into()),
            concurrency,
            allocation,
            CpuPlacementPolicy::Explicit {
                cpus: (0..cores).map(Into::into).collect(),
            },
            None,
        )
    }

    /// The pool arithmetic each mode uses, named in the refusal when the
    /// pool does not fit.
    #[test]
    fn a_pool_too_small_refuses_and_names_the_arithmetic_it_applied() {
        let shared = plan_on(2, 3, SlotAllocation::Shared { cores_per_game: 1 })
            .unwrap_err()
            .to_string();
        assert!(
            shared.contains("3 game slots need 3 physical cores (game-slots × cores-per-game)")
                && shared.contains("provides 2"),
            "{shared}"
        );

        let disjoint = plan_on(
            2,
            2,
            SlotAllocation::PerEngine {
                cores_per_engine: 1,
            },
        )
        .unwrap_err()
        .to_string();
        assert!(
            disjoint
                .contains("2 game slots need 4 physical cores (game-slots × 2 × cores-per-engine)")
                && disjoint.contains("provides 2"),
            "{disjoint}"
        );
    }

    #[test]
    fn a_shared_slot_pins_both_engines_to_the_same_core_set() {
        let plan = plan_on(2, 2, SlotAllocation::Shared { cores_per_game: 1 }).unwrap();
        assert_eq!(plan.slots.len(), 2);
        for slot in &plan.slots {
            assert_eq!(slot.engine_a.allocation, slot.engine_b.allocation);
            assert_eq!(slot.engine_a.physical_core_count, 1);
            assert!(matches!(
                slot.engine_a.allocation,
                CpuAllocation::Enforced(_)
            ));
        }
        assert_ne!(
            plan.slots[0].engine_a.allocation,
            plan.slots[1].engine_a.allocation
        );

        // The disjoint mode gives each engine a core of its own.
        let plan = plan_on(
            2,
            1,
            SlotAllocation::PerEngine {
                cores_per_engine: 1,
            },
        )
        .unwrap();
        assert_ne!(
            plan.slots[0].engine_a.allocation,
            plan.slots[0].engine_b.allocation
        );
    }

    #[test]
    fn placement_off_never_reads_the_host() {
        let plan = plan_execution_on(
            || panic!("placement off read the host's CPUs"),
            &EngineLaunchSpec::path_only("a".into()),
            &EngineLaunchSpec::path_only("b".into()),
            2,
            SlotAllocation::Shared { cores_per_game: 1 },
            CpuPlacementPolicy::Off,
            None,
        )
        .unwrap();
        assert!(plan.slots.iter().all(|slot| {
            slot.engine_a.allocation == CpuAllocation::Unrestricted
                && slot.engine_b.allocation == CpuAllocation::Unrestricted
        }));
    }

    #[test]
    fn a_host_that_cannot_be_read_refuses_placement() {
        let error = plan_execution_on(
            || {
                Err(MatchError::Placement(
                    "logical CPU identities are unavailable for this topology".into(),
                ))
            },
            &EngineLaunchSpec::path_only("a".into()),
            &EngineLaunchSpec::path_only("b".into()),
            1,
            SlotAllocation::Shared { cores_per_game: 1 },
            CpuPlacementPolicy::default(),
            None,
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("CPU placement could not be resolved"),
            "{error}"
        );
    }

    /// Launch settings are cloned once per launched game or pair. A book
    /// carried in them by value was copied with every launch, which spread
    /// one wave's launches over seconds with a large book; the clone must
    /// share the one parsed book.
    #[test]
    fn cloned_launch_settings_share_the_opening_book() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("book.epd");
        std::fs::write(
            &path,
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq -\n".repeat(4),
        )
        .unwrap();
        let openings = resolve_openings(Some(OpeningBook::new(path)), 0, 2, false, 7).unwrap();
        assert_eq!(openings.entries.len(), 4);
        let settings = PairGameSettings {
            engine_a: EngineLaunchSpec::path_only("a".into()),
            engine_b: EngineLaunchSpec::path_only("b".into()),
            engine_a_time_control: ConfiguredTimeControl::default(),
            engine_b_time_control: ConfiguredTimeControl::default(),
            adjudication: AdjudicationConfig::default(),
            ponder: false,
            openings,
            synthetic_games: false,
            engines: None,
        };
        let launched = settings.clone();
        assert!(Arc::ptr_eq(
            &settings.openings.entries,
            &launched.openings.entries
        ));
    }
}
