//! The game runner: drive two engines through one complete game.
//!
//! Uses `shakmaty` for legality/SAN and natural game-end detection, normalizes engine
//! scores to White's point of view for [`adjudicate`], and produces valid PGN. The
//! engines are spawned, configured, played and shut down within [`run_game`], so a
//! game owns its processes (and `kill_on_drop` cleans them up on cancellation).

use std::collections::HashMap;
use std::time::Duration;

use colosseum_application::CpuAllocation;
use colosseum_core::{
    AdjudicationConfig, EngineId, GameId, GameResult, GameStats, Termination, TimeControl,
    adjudicate,
};
use colosseum_uci::{EngineProcess, GoLimits, SpawnOptions, UciError, UciPosition};
use serde::{Deserialize, Serialize};
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::zobrist::Zobrist64;
use shakmaty::{CastlingMode, Chess, Color, EnPassantMode, Position};

use crate::live::{EvalPoint, LiveGameHandle, SEARCH_LOG_CAP, SearchLine, to_white_pov};
use crate::pgn::{PgnTags, build_pgn};

/// Centipawn magnitude used to represent mate scores for adjudication.
const ADJ_MATE_CP: i32 = 100_000;
/// Hard safety cap on plies (natural draw rules normally end games far sooner).
const MAX_PLIES: usize = 6000;
/// Wall-clock safety cutoff for node-/depth-limited searches, which carry no clock
/// of their own. A search that blows past this is treated as a crash-class hang.
const FIXED_SEARCH_DEADLINE: Duration = Duration::from_secs(600);
/// How long an engine gets to answer `stop` when its ponder prediction missed.
const PONDER_STOP_DEADLINE: Duration = Duration::from_secs(5);

pub const CLOCK_MODEL_ID: &str = "go-write-to-bestmove-read";
pub const CLOCK_MODEL_VERSION: u32 = 1;

fn color_idx(color: Color) -> usize {
    if color == Color::White { 0 } else { 1 }
}

/// Build the live-view `info` sink for one side (used for real searches and
/// ponder searches alike). Fast engines emit hundreds of info lines per
/// second, and taking the mutex + cloning the PV for each one is wasted work
/// whether or not anyone is watching — so a new depth always publishes (it
/// also appends a search-log line) and in between updates are capped at
/// ~10 Hz, which is the GUI's repaint rate anyway. `begin` anchors the log's
/// elapsed column for engines that report no `time`.
fn live_info_sink<'a>(
    live: &'a LiveGameHandle,
    is_white: bool,
    begin: std::time::Instant,
) -> impl FnMut(&colosseum_uci::InfoLine) + 'a {
    let mut last_publish: Option<std::time::Instant> = None;
    // Seed from the existing log so a sink created mid-search (ponderhit
    // continues the ponder's log) doesn't re-log an already-shown depth.
    let mut last_log_depth: u32 = live.lock().map_or(0, |lg| {
        let log = if is_white {
            &lg.white_log
        } else {
            &lg.black_log
        };
        log.last().map_or(0, |l| l.depth)
    });
    move |info| {
        let now = std::time::Instant::now();
        let new_depth_line = !info.pv.is_empty() && info.depth.is_some_and(|d| d > last_log_depth);
        let throttled =
            last_publish.is_some_and(|t| now.duration_since(t) < Duration::from_millis(100));
        if throttled && !new_depth_line {
            return;
        }
        last_publish = Some(now);
        let Ok(mut lg) = live.lock() else { return };
        // Reborrow through the guard so the two fields can be split.
        let lg = &mut *lg;
        let white_pov_score = info.score.map(|s| to_white_pov(s, is_white));
        let (side, log) = if is_white {
            (&mut lg.white_search, &mut lg.white_log)
        } else {
            (&mut lg.black_search, &mut lg.black_log)
        };
        if white_pov_score.is_some() {
            side.score = white_pov_score;
        }
        if info.depth.is_some() {
            side.depth = info.depth;
        }
        if info.seldepth.is_some() {
            side.seldepth = info.seldepth;
        }
        if info.nodes.is_some() {
            side.nodes = info.nodes;
        }
        if let Some(n) = info.nps
            && n > 0
        {
            side.nps = Some(n);
        }
        if !info.pv.is_empty() {
            side.pv = info.pv.clone();
        }
        if new_depth_line {
            last_log_depth = info.depth.unwrap_or(0);
            log.push(SearchLine {
                score: white_pov_score.or(side.score),
                depth: last_log_depth,
                seldepth: info.seldepth,
                nodes: info.nodes,
                elapsed_ms: info
                    .time_ms
                    .unwrap_or_else(|| begin.elapsed().as_millis() as u64),
                pv: info.pv.iter().take(16).cloned().collect(),
            });
            if log.len() > SEARCH_LOG_CAP {
                let overflow = log.len() - SEARCH_LOG_CAP;
                log.drain(0..overflow);
            }
        }
    }
}

/// How to launch and configure one side.
#[derive(Debug, Clone)]
pub struct EngineGameSpec {
    pub id: EngineId,
    pub name: String,
    pub spawn: SpawnOptions,
    /// Resolved `setoption` commands (`value` is `None` for buttons).
    pub options: Vec<(String, Option<String>)>,
    pub allocated_cpus: CpuAllocation,
}

/// Everything needed to play one game.
#[derive(Debug, Clone)]
pub struct GameSpec {
    pub game_id: GameId,
    pub event: String,
    pub site: String,
    pub date: String,
    pub round: u32,
    pub white: EngineGameSpec,
    pub black: EngineGameSpec,
    /// `None` => standard start position.
    pub start_fen: Option<String>,
    /// Opening moves (UCI) to pre-play from `start_fen` before the engines move.
    pub opening_moves: Vec<String>,
    pub white_time_control: TimeControl,
    pub black_time_control: TimeControl,
    pub time_control_label: String,
    pub adjudication: AdjudicationConfig,
    /// Drive the UCI ponder protocol: engines think on the opponent's time
    /// (`go ponder` / `ponderhit` / `stop`).
    pub ponder: bool,
    pub white_time_margin: Duration,
    pub black_time_margin: Duration,
    pub handshake_timeout: Duration,
}

/// The outcome of a finished game.
#[derive(Debug, Clone)]
pub struct GameReport {
    pub game_id: GameId,
    pub white: EngineId,
    pub black: EngineId,
    pub result: GameResult,
    pub scorable: bool,
    pub termination: Termination,
    pub stats: GameStats,
    pub san_moves: Vec<String>,
    pub uci_moves: Vec<String>,
    pub pgn: String,
    pub clock_accounting: ClockAccountingReport,
    pub fault: Option<GameFault>,
    /// Set when the game ended due to an engine problem (crash/illegal/timeout).
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GameSide {
    White,
    Black,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EngineFaultKind {
    Timeout,
    Crash,
    Disconnect,
    Protocol,
    IllegalMove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cause", rename_all = "kebab-case")]
pub enum GameFault {
    Engine {
        side: GameSide,
        kind: EngineFaultKind,
        message: String,
    },
    Infrastructure {
        operation: String,
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChargedElapsedSummary {
    pub samples: u32,
    pub min_ns: u64,
    pub median_ns: u64,
    pub max_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockAccountingReport {
    pub model: String,
    pub version: u32,
    pub white_margin_ms: u64,
    pub black_margin_ms: u64,
    pub monotonic_resolution_ns: u64,
    pub white_charged_elapsed: Option<ChargedElapsedSummary>,
    pub black_charged_elapsed: Option<ChargedElapsedSummary>,
}

/// Per-side running average of reported nps.
#[derive(Default)]
struct NpsAccumulator {
    total: u128,
    samples: u64,
}

impl NpsAccumulator {
    fn add(&mut self, nps: Option<u64>) {
        if let Some(nps) = nps {
            self.total += u128::from(nps);
            self.samples += 1;
        }
    }

    fn average(&self) -> Option<u64> {
        if self.samples == 0 {
            None
        } else {
            Some((self.total / u128::from(self.samples)) as u64)
        }
    }
}

/// Per-side running average of reported search depth.
#[derive(Default)]
struct DepthAccumulator {
    total: u64,
    samples: u64,
}

impl DepthAccumulator {
    fn add(&mut self, depth: Option<u32>) {
        if let Some(depth) = depth {
            self.total += u64::from(depth);
            self.samples += 1;
        }
    }

    fn average(&self) -> Option<f64> {
        if self.samples == 0 {
            None
        } else {
            Some(self.total as f64 / self.samples as f64)
        }
    }
}

/// Per-side running average of wall-clock time spent per move.
#[derive(Default)]
struct MoveTimeAccumulator {
    total_ms: u128,
    samples: u64,
    elapsed_ns: Vec<u64>,
}

impl MoveTimeAccumulator {
    fn add(&mut self, elapsed: std::time::Duration) {
        self.total_ms += elapsed.as_millis();
        self.samples += 1;
        self.elapsed_ns
            .push(elapsed.as_nanos().min(u128::from(u64::MAX)) as u64);
    }

    fn average_ms(&self) -> Option<f64> {
        if self.samples == 0 {
            None
        } else {
            Some(self.total_ms as f64 / self.samples as f64)
        }
    }

    fn summary(&self) -> Option<ChargedElapsedSummary> {
        let mut values = self.elapsed_ns.clone();
        if values.is_empty() {
            return None;
        }
        values.sort_unstable();
        let middle = values.len() / 2;
        let median_ns = if values.len().is_multiple_of(2) {
            ((u128::from(values[middle - 1]) + u128::from(values[middle])) / 2) as u64
        } else {
            values[middle]
        };
        Some(ChargedElapsedSummary {
            samples: values.len().min(u32::MAX as usize) as u32,
            min_ns: values[0],
            median_ns,
            max_ns: *values.last().expect("nonempty elapsed sample"),
        })
    }
}

fn monotonic_resolution_ns() -> u64 {
    let mut previous = std::time::Instant::now();
    let mut minimum = u64::MAX;
    for _ in 0..1_024 {
        let now = std::time::Instant::now();
        let elapsed = now.saturating_duration_since(previous).as_nanos();
        if elapsed > 0 {
            minimum = minimum.min(elapsed.min(u128::from(u64::MAX)) as u64);
        }
        previous = now;
    }
    if minimum == u64::MAX { 1 } else { minimum }
}

fn clock_accounting_report(
    spec: &GameSpec,
    monotonic_resolution_ns: u64,
    white: Option<&MoveTimeAccumulator>,
    black: Option<&MoveTimeAccumulator>,
) -> ClockAccountingReport {
    ClockAccountingReport {
        model: CLOCK_MODEL_ID.to_owned(),
        version: CLOCK_MODEL_VERSION,
        white_margin_ms: spec.white_time_margin.as_millis().min(u128::from(u64::MAX)) as u64,
        black_margin_ms: spec.black_time_margin.as_millis().min(u128::from(u64::MAX)) as u64,
        monotonic_resolution_ns,
        white_charged_elapsed: white.and_then(MoveTimeAccumulator::summary),
        black_charged_elapsed: black.and_then(MoveTimeAccumulator::summary),
    }
}

/// The result of preparing one engine for a game. (The `Ready` variant holds
/// the full process inline; this enum is short-lived and immediately matched,
/// so the size difference between variants is immaterial.)
#[allow(clippy::large_enum_variant)]
enum Prepared {
    /// Handshaken, configured, and ready to play.
    Ready(EngineProcess),
    /// Setup failed *after* the process spawned — the process is retained so
    /// its UCI transcript and stderr can go into an incident report (this is
    /// where finicky engines like Deep Junior die: on handshake / setoption /
    /// `ucinewgame`, not mid-game).
    Failed(UciError, Box<EngineProcess>),
    /// The process never spawned (bad path, missing DLL, etc.); no forensics
    /// beyond the error itself.
    NoSpawn(UciError),
    /// The process spawned, but the harness could not apply/verify the
    /// requested resource boundary. This must never become a forfeit.
    Infrastructure(String, Box<EngineProcess>),
}

/// Run the handshake / option / ready sequence on an already-spawned engine.
async fn init_engine(
    engine: &mut EngineProcess,
    spec: &EngineGameSpec,
    handshake_timeout: Duration,
) -> Result<(), UciError> {
    engine.handshake(handshake_timeout).await?;
    for (name, value) in &spec.options {
        engine.set_option(name, value.as_deref()).await?;
    }
    engine.is_ready(handshake_timeout).await?;
    engine.new_game().await?;
    engine.is_ready(handshake_timeout).await?;
    Ok(())
}

/// Spawn, configure, and ready one engine for a game — retaining the process
/// on setup failure so the caller can capture forensics.
async fn prepare(spec: &EngineGameSpec, handshake_timeout: Duration) -> Prepared {
    let mut engine = match EngineProcess::spawn(spec.spawn.clone()).await {
        Ok(engine) => engine,
        Err(err) => return Prepared::NoSpawn(err),
    };
    if !matches!(spec.allocated_cpus, CpuAllocation::Unrestricted) {
        let Some(process_id) = engine.id() else {
            return Prepared::Infrastructure(
                "spawned engine has no process identifier".into(),
                Box::new(engine),
            );
        };
        if let Err(error) =
            crate::affinity::apply_process_affinity(process_id, &spec.allocated_cpus)
        {
            return Prepared::Infrastructure(error.to_string(), Box::new(engine));
        }
    }
    match init_engine(&mut engine, spec, handshake_timeout).await {
        Ok(()) => Prepared::Ready(engine),
        Err(err) => Prepared::Failed(err, Box::new(engine)),
    }
}

/// Play one complete game and return its report. Never panics on engine misbehavior.
/// `live` is updated throughout for the GUI's live view.
pub async fn run_game(spec: GameSpec, live: LiveGameHandle) -> GameReport {
    let game_start = std::time::Instant::now();
    let monotonic_resolution_ns = monotonic_resolution_ns();

    let white = prepare(&spec.white, spec.handshake_timeout).await;
    let black = prepare(&spec.black, spec.handshake_timeout).await;

    let (mut white, mut black) = match (white, black) {
        (Prepared::Ready(w), Prepared::Ready(b)) => (w, b),
        (white, black) => {
            let mut report =
                handle_setup_failure(&spec, white, black, monotonic_resolution_ns).await;
            report.stats.duration_ms = Some(game_start.elapsed().as_millis() as u64);
            if let Ok(mut lg) = live.lock() {
                lg.finished = Some((report.result, report.termination));
            }
            return report;
        }
    };

    let mut pos = initial_position(spec.start_fen.as_deref());
    let mut clocks = Clocks::new(&spec.white_time_control, &spec.black_time_control);

    let mut san_moves: Vec<String> = Vec::new();
    let mut uci_moves: Vec<String> = Vec::new();
    let mut white_pov: Vec<i32> = Vec::new();
    let mut last_white_pov = 0i32;
    let mut repetitions: HashMap<Zobrist64, u8> = HashMap::new();
    let mut white_nps = NpsAccumulator::default();
    let mut black_nps = NpsAccumulator::default();
    let mut white_depth = DepthAccumulator::default();
    let mut black_depth = DepthAccumulator::default();
    let mut white_move_time = MoveTimeAccumulator::default();
    let mut black_move_time = MoveTimeAccumulator::default();
    // Per color: the predicted reply the engine is currently pondering on
    // (canonical UCI) and when that ponder search started. `Some` means a
    // `go ponder` is outstanding and must be resolved before the engine's
    // next search.
    let mut ponder_pred: [Option<(String, std::time::Instant)>; 2] = [None, None];

    // Pre-play the assigned opening moves before the engines take over. These
    // were validated when the book was loaded, but we re-validate defensively.
    for uci in &spec.opening_moves {
        let Some(legal) = uci
            .parse::<UciMove>()
            .ok()
            .and_then(|m| m.to_move(&pos).ok())
        else {
            break;
        };
        san_moves.push(SanPlus::from_move(pos.clone(), legal).to_string());
        uci_moves.push(uci.clone());
        white_pov.push(last_white_pov); // no engine eval for opening plies
        pos.play_unchecked(legal);
        let key = pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal);
        *repetitions.entry(key).or_insert(0) += 1;
    }

    if let Ok(mut lg) = live.lock() {
        lg.san_moves = san_moves.clone();
        lg.uci_moves = uci_moves.clone();
        lg.opening_plies = san_moves.len() as u32;
        lg.white_clock_ms = clock_ms(&spec, &clocks, Color::White);
        lg.black_clock_ms = clock_ms(&spec, &clocks, Color::Black);
        lg.white_to_move = pos.turn() == Color::White;
    }

    let outcome = loop {
        if san_moves.len() >= MAX_PLIES {
            break Outcome::natural(GameResult::Draw, Termination::MaxMoves);
        }

        let mover = pos.turn();
        let (engine, opponent) = if mover == Color::White {
            (&mut white, &mut black)
        } else {
            (&mut black, &mut white)
        };
        let position = build_uci_position(spec.start_fen.as_deref(), &uci_moves);

        let (limits, deadline) = move_limits(
            time_control_for(&spec, mover),
            &clocks,
            mover,
            time_margin_for(&spec, mover),
        );

        // Resolve the mover's outstanding ponder: a correct prediction turns
        // it into the real search (`ponderhit`); a miss aborts it first.
        let predicted = ponder_pred[color_idx(mover)].take();
        let hit = predicted
            .as_ref()
            .is_some_and(|(m, _)| Some(m.as_str()) == uci_moves.last().map(String::as_str));
        if predicted.is_some()
            && !hit
            && let Err(err) = engine.stop_ponder(PONDER_STOP_DEADLINE).await
        {
            break Outcome::engine_loss(
                mover,
                Termination::EngineCrash,
                fault_kind_for_uci(&err),
                Some(format!("failed to abort ponder search: {err}")),
            );
        }

        if let Ok(mut lg) = live.lock() {
            lg.white_to_move = mover == Color::White;
            lg.search_started = Some(std::time::Instant::now());
            if mover == Color::White {
                lg.white_pondering = false;
            } else {
                lg.black_pondering = false;
            }
            // The log shows the *current* search only — clear at every fresh
            // start. A ponderhit continues the running ponder search, so the
            // log it accumulated while pondering is kept.
            if !hit {
                if mover == Color::White {
                    lg.white_log.clear();
                } else {
                    lg.black_log.clear();
                }
            }
        }
        let search_begin = std::time::Instant::now();
        let mover_search = async {
            let sink = live_info_sink(&live, mover == Color::White, search_begin);
            if hit {
                engine.ponderhit(deadline, sink).await
            } else {
                engine.search(&position, &limits, deadline, sink).await
            }
        };
        // While the mover thinks, keep pumping the pondering opponent's
        // output so its pipe never backs up and the live view streams it.
        // `drain_ponder` never completes; it is dropped (cancel-safe read)
        // when the mover's search resolves. The ponderer's latest score is
        // kept so the eval graph can plot both engines every ply.
        let mut opp_ponder_score: Option<colosseum_uci::Score> = None;
        let opp_is_white = mover != Color::White;
        let search = match &ponder_pred[color_idx(mover.other())] {
            Some((_, ponder_begin)) => {
                let mut opp_sink = live_info_sink(&live, opp_is_white, *ponder_begin);
                tokio::select! {
                    result = mover_search => result,
                    () = opponent.drain_ponder(|info| {
                        if let Some(s) = info.score {
                            opp_ponder_score = Some(to_white_pov(s, opp_is_white));
                        }
                        opp_sink(info);
                    }) => {
                        unreachable!("drain_ponder never completes")
                    }
                }
            }
            None => mover_search.await,
        };

        let output = match search {
            Ok(output) => output,
            Err(UciError::MoveTimeout) => {
                break Outcome::engine_loss(
                    mover,
                    Termination::TimeForfeit,
                    EngineFaultKind::Timeout,
                    Some("move deadline exceeded".into()),
                );
            }
            Err(err) => {
                break Outcome::engine_loss(
                    mover,
                    Termination::EngineCrash,
                    fault_kind_for_uci(&err),
                    Some(err.to_string()),
                );
            }
        };

        if mover == Color::White {
            white_nps.add(output.nps);
            white_depth.add(output.depth);
            white_move_time.add(output.elapsed);
        } else {
            black_nps.add(output.nps);
            black_depth.add(output.depth);
            black_move_time.add(output.elapsed);
        }

        // Apply the binding E > R + M rule. The margin only decides whether
        // the response is accepted; it is never sent in the UCI clock values.
        if !clocks.accept_and_charge(
            time_control_for(&spec, mover),
            mover,
            output.elapsed,
            time_margin_for(&spec, mover),
        ) {
            break Outcome::engine_loss(
                mover,
                Termination::TimeForfeit,
                EngineFaultKind::Timeout,
                Some("charged elapsed time exceeded remaining time plus margin".into()),
            );
        }

        // Parse and validate the move, tolerating two common nonstandard
        // notations from older engines (see `parse_engine_move`).
        let Some((legal_move, leniency)) = parse_engine_move(&output.best_move, &pos) else {
            break Outcome::engine_loss(
                mover,
                Termination::IllegalMove,
                EngineFaultKind::IllegalMove,
                Some(format!("illegal move: {}", output.best_move)),
            );
        };
        if let Some(note) = leniency {
            tracing::warn!(
                target: "runner",
                "{} sent '{}' — accepted leniently ({note})",
                if mover == Color::White { &spec.white.name } else { &spec.black.name },
                output.best_move,
            );
        }

        san_moves.push(SanPlus::from_move(pos.clone(), legal_move).to_string());
        // Store the CANONICAL encoding, not the engine's raw text: the move
        // list is replayed to the opponent every move, so a tolerated
        // nonstandard form must never leak into the shared history.
        uci_moves.push(legal_move.to_uci(CastlingMode::Standard).to_string());

        // Score normalized to White's point of view.
        let white_pov_cp = match output.score {
            Some(score) => {
                let cp = score.to_cp(ADJ_MATE_CP);
                if mover == Color::White { cp } else { -cp }
            }
            None => last_white_pov,
        };
        last_white_pov = white_pov_cp;
        white_pov.push(white_pov_cp);

        pos.play_unchecked(legal_move);

        if let Ok(mut lg) = live.lock() {
            lg.san_moves
                .push(san_moves.last().cloned().unwrap_or_default());
            lg.uci_moves
                .push(uci_moves.last().cloned().unwrap_or_default());
            lg.white_clock_ms = clock_ms(&spec, &clocks, Color::White);
            lg.black_clock_ms = clock_ms(&spec, &clocks, Color::Black);
            lg.white_to_move = pos.turn() == Color::White;
            lg.search_started = None;
            if let Some(score) = output.score {
                lg.evals.push(EvalPoint {
                    ply: san_moves.len() as u32,
                    by_white: mover == Color::White,
                    score: to_white_pov(score, mover == Color::White),
                });
            }
            // The opponent thought on the mover's time (pondering): plot its
            // latest eval at the same ply so both lines advance every move.
            if let Some(score) = opp_ponder_score {
                lg.evals.push(EvalPoint {
                    ply: san_moves.len() as u32,
                    by_white: opp_is_white,
                    score,
                });
            }
        }

        // Natural endings take precedence over adjudication.
        if pos.is_checkmate() {
            break Outcome::win(mover, Termination::Checkmate);
        }
        if pos.is_stalemate() {
            break Outcome::natural(GameResult::Draw, Termination::Stalemate);
        }
        if pos.is_insufficient_material() {
            break Outcome::natural(GameResult::Draw, Termination::InsufficientMaterial);
        }
        if pos.halfmoves() >= 100 {
            break Outcome::natural(GameResult::Draw, Termination::FiftyMove);
        }
        let key = pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal);
        let count = repetitions.entry(key).or_insert(0);
        *count += 1;
        if *count >= 3 {
            break Outcome::natural(GameResult::Draw, Termination::Threefold);
        }

        if let Some(adjudication) = adjudicate(&white_pov, &spec.adjudication) {
            break Outcome::natural(adjudication.result, adjudication.termination);
        }

        // Arm the mover's next ponder: think on the opponent's time about its
        // own predicted continuation. Only when the hint is legal in the new
        // position — engines occasionally send junk hints, and a failed arm
        // simply means no ponder this move.
        if spec.ponder
            && let Some(hint) = output
                .ponder
                .as_deref()
                .and_then(|h| h.parse::<UciMove>().ok())
                .and_then(|m| m.to_move(&pos).ok())
        {
            let hint_uci = hint.to_uci(CastlingMode::Standard).to_string();
            let mut ponder_moves = uci_moves.clone();
            ponder_moves.push(hint_uci.clone());
            let ponder_pos = build_uci_position(spec.start_fen.as_deref(), &ponder_moves);
            let (ponder_limits, _) = move_limits(
                time_control_for(&spec, mover),
                &clocks,
                mover,
                time_margin_for(&spec, mover),
            );
            if engine
                .start_ponder(&ponder_pos, &ponder_limits)
                .await
                .is_ok()
            {
                ponder_pred[color_idx(mover)] = Some((hint_uci, std::time::Instant::now()));
                if let Ok(mut lg) = live.lock() {
                    // The ponder is a fresh search: its log starts clean.
                    if mover == Color::White {
                        lg.white_pondering = true;
                        lg.white_log.clear();
                    } else {
                        lg.black_pondering = true;
                        lg.black_log.clear();
                    }
                }
            }
        }
    };

    // Abnormal end: write a forensic incident report while the engines'
    // transcripts are still available, and point the error text at it.
    let mut outcome = outcome;
    if matches!(
        outcome.termination,
        Termination::TimeForfeit | Termination::EngineCrash | Termination::IllegalMove
    ) {
        let text = incident_report(&spec, &uci_moves, &clocks, &outcome, &white, &black);
        let stub = format!(
            "{:?}-{}-vs-{}-r{}",
            outcome.termination, spec.white.name, spec.black.name, spec.round
        );
        if let Some(file) = crate::incidents::write(&stub, &text) {
            let detail = outcome.error.take().unwrap_or_default();
            outcome.error = Some(if detail.is_empty() {
                format!("see logs/incidents/{file}")
            } else {
                format!("{detail} — see logs/incidents/{file}")
            });
        }
    }

    if let Ok(mut lg) = live.lock() {
        lg.finished = Some((outcome.result, outcome.termination));
        lg.search_started = None;
        lg.white_pondering = false;
        lg.black_pondering = false;
    }

    // Shut engines down gracefully (kill_on_drop covers anything left).
    let _ = white.quit(Duration::from_millis(500)).await;
    let _ = black.quit(Duration::from_millis(500)).await;

    let stats = GameStats {
        plies: san_moves.len() as u32,
        white_nps: white_nps.average(),
        black_nps: black_nps.average(),
        white_depth: white_depth.average(),
        black_depth: black_depth.average(),
        white_move_ms: white_move_time.average_ms(),
        black_move_ms: black_move_time.average_ms(),
        duration_ms: Some(game_start.elapsed().as_millis() as u64),
    };
    let pgn = render_pgn(&spec, &san_moves, outcome.result, outcome.termination);

    GameReport {
        game_id: spec.game_id,
        white: spec.white.id,
        black: spec.black.id,
        result: outcome.result,
        scorable: outcome.scorable,
        termination: outcome.termination,
        stats,
        san_moves,
        uci_moves,
        pgn,
        clock_accounting: clock_accounting_report(
            &spec,
            monotonic_resolution_ns,
            Some(&white_move_time),
            Some(&black_move_time),
        ),
        fault: outcome.fault,
        error: outcome.error,
    }
}

/// Parse an engine's `bestmove` against the current position, tolerating two
/// widespread nonstandard notations (each only when it resolves to a legal
/// move): uppercase text, and bare promotions like `d7d8` (promotion piece
/// omitted → assume queen). King-onto-rook castling (`e1h1`, Chess960 style)
/// is already understood by shakmaty; the caller's canonical re-encoding
/// turns it into `e1g1` before it reaches the opponent's move list.
///
/// Returns the legal move plus a note when leniency was applied.
fn parse_engine_move(text: &str, pos: &Chess) -> Option<(shakmaty::Move, Option<&'static str>)> {
    let try_uci = |s: &str| {
        s.parse::<UciMove>()
            .ok()
            .and_then(|uci| uci.to_move(pos).ok())
    };
    let trimmed = text.trim();
    if let Some(mv) = try_uci(trimmed) {
        return Some((mv, None));
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower != trimmed
        && let Some(mv) = try_uci(&lower)
    {
        return Some((mv, Some("uppercase notation")));
    }
    if lower.len() == 4
        && let Some(mv) = try_uci(&format!("{lower}q"))
    {
        return Some((mv, Some("promotion piece omitted — assumed queen")));
    }
    None
}

/// Build the plain-text forensic report for an abnormal game end.
fn incident_report(
    spec: &GameSpec,
    uci_moves: &[String],
    clocks: &Clocks,
    outcome: &Outcome,
    white: &EngineProcess,
    black: &EngineProcess,
) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(8 * 1024);
    let _ = writeln!(s, "event:       {} (round {})", spec.event, spec.round);
    let _ = writeln!(
        s,
        "white:       {}  [{}]",
        spec.white.name,
        spec.white.spawn.path.display()
    );
    let _ = writeln!(
        s,
        "black:       {}  [{}]",
        spec.black.name,
        spec.black.spawn.path.display()
    );
    let _ = writeln!(s, "time control: {}", spec.time_control_label);
    let _ = writeln!(s, "termination: {:?}", outcome.termination);
    let _ = writeln!(s, "result:      {}", outcome.result.pgn());
    if let Some(err) = &outcome.error {
        let _ = writeln!(s, "detail:      {err}");
    }
    let _ = writeln!(
        s,
        "clocks at end: white {:?}, black {:?}",
        clocks.remaining(Color::White),
        clocks.remaining(Color::Black)
    );
    let _ = writeln!(
        s,
        "start fen:   {}",
        spec.start_fen.as_deref().unwrap_or("startpos")
    );
    let _ = writeln!(s, "opening plies: {}", spec.opening_moves.len());
    let _ = writeln!(s, "moves ({}): {}", uci_moves.len(), uci_moves.join(" "));
    for (label, engine) in [("white", white), ("black", black)] {
        let _ = writeln!(
            s,
            "\n── {label} UCI transcript (last lines; > sent, < received; info collapsed) ──"
        );
        for line in engine.transcript() {
            let _ = writeln!(s, "{line}");
        }
        let stderr = engine.stderr_tail();
        if !stderr.is_empty() {
            let _ = writeln!(s, "── {label} stderr ──");
            for line in stderr {
                let _ = writeln!(s, "{line}");
            }
        }
    }
    s
}

/// Per-side game clocks for clock-based time controls. Both sides start at the
/// configured base; for fixed-time / nodes / depth controls the clocks are unused.
struct Clocks {
    white: Duration,
    black: Duration,
}

/// Remaining clock in ms for the live view; `None` for non-clock controls.
fn time_control_for(spec: &GameSpec, side: Color) -> &TimeControl {
    match side {
        Color::White => &spec.white_time_control,
        Color::Black => &spec.black_time_control,
    }
}

fn time_margin_for(spec: &GameSpec, side: Color) -> Duration {
    match side {
        Color::White => spec.white_time_margin,
        Color::Black => spec.black_time_margin,
    }
}

fn clock_ms(spec: &GameSpec, clocks: &Clocks, side: Color) -> Option<u64> {
    let tc = time_control_for(spec, side);
    tc.is_clock()
        .then(|| clocks.remaining(side).as_millis() as u64)
}

impl Clocks {
    fn new(white_tc: &TimeControl, black_tc: &TimeControl) -> Self {
        Self {
            white: white_tc.initial_clock().unwrap_or(Duration::ZERO),
            black: black_tc.initial_clock().unwrap_or(Duration::ZERO),
        }
    }

    fn remaining(&self, side: Color) -> Duration {
        match side {
            Color::White => self.white,
            Color::Black => self.black,
        }
    }

    /// Accept exactly at the budget-plus-margin boundary. For clock controls,
    /// deduct elapsed before crediting increment: max(0, R-E)+I.
    fn accept_and_charge(
        &mut self,
        tc: &TimeControl,
        side: Color,
        used: Duration,
        margin: Duration,
    ) -> bool {
        let budget = match tc {
            TimeControl::PerMove { ms } => Some(Duration::from_millis(*ms)),
            TimeControl::SuddenDeath { .. } | TimeControl::Increment { .. } => {
                Some(self.remaining(side))
            }
            TimeControl::Nodes { .. } | TimeControl::Depth { .. } => None,
        };
        if budget.is_some_and(|budget| used > budget.saturating_add(margin)) {
            return false;
        }
        if tc.is_clock() {
            let increment = tc.increment();
            let clock = match side {
                Color::White => &mut self.white,
                Color::Black => &mut self.black,
            };
            *clock = clock.saturating_sub(used).saturating_add(increment);
        }
        true
    }
}

/// Build the `go` limits and the hard wall-clock deadline for the side to move.
fn move_limits(
    tc: &TimeControl,
    clocks: &Clocks,
    mover: Color,
    tolerance: Duration,
) -> (GoLimits, Duration) {
    match tc {
        TimeControl::PerMove { ms } => {
            let mt = Duration::from_millis(*ms);
            (GoLimits::MoveTime(mt), mt + tolerance)
        }
        TimeControl::SuddenDeath { .. } | TimeControl::Increment { .. } => {
            let inc = tc.increment();
            let limits = GoLimits::Clock {
                wtime: clocks.white,
                btime: clocks.black,
                winc: inc,
                binc: inc,
            };
            // The mover must answer within their remaining time (plus IO grace).
            (limits, clocks.remaining(mover) + tolerance)
        }
        TimeControl::Nodes { nodes } => (GoLimits::Nodes(*nodes), FIXED_SEARCH_DEADLINE),
        TimeControl::Depth { depth } => (GoLimits::Depth(*depth), FIXED_SEARCH_DEADLINE),
    }
}

/// Small helper bundling a game's verdict.
struct Outcome {
    result: GameResult,
    scorable: bool,
    termination: Termination,
    error: Option<String>,
    fault: Option<GameFault>,
}

impl Outcome {
    fn natural(result: GameResult, termination: Termination) -> Self {
        Self {
            result,
            scorable: true,
            termination,
            error: None,
            fault: None,
        }
    }

    /// The side that just moved wins.
    fn win(mover: Color, termination: Termination) -> Self {
        Self {
            result: if mover == Color::White {
                GameResult::WhiteWin
            } else {
                GameResult::BlackWin
            },
            scorable: true,
            termination,
            error: None,
            fault: None,
        }
    }

    /// The side to move loses due to an attributable engine fault.
    fn engine_loss(
        mover: Color,
        termination: Termination,
        kind: EngineFaultKind,
        error: Option<String>,
    ) -> Self {
        let message = error.clone().unwrap_or_else(|| format!("{kind:?}"));
        Self {
            result: if mover == Color::White {
                GameResult::BlackWin
            } else {
                GameResult::WhiteWin
            },
            scorable: true,
            termination,
            error,
            fault: Some(GameFault::Engine {
                side: if mover == Color::White {
                    GameSide::White
                } else {
                    GameSide::Black
                },
                kind,
                message,
            }),
        }
    }

    fn infrastructure(operation: impl Into<String>, message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            // Required for the legacy report/PGN shape, but explicitly not
            // scorable and therefore never enters match statistics.
            result: GameResult::Draw,
            scorable: false,
            termination: Termination::Aborted,
            error: Some(message.clone()),
            fault: Some(GameFault::Infrastructure {
                operation: operation.into(),
                message,
            }),
        }
    }
}

fn fault_kind_for_uci(error: &UciError) -> EngineFaultKind {
    match error {
        UciError::MoveTimeout | UciError::HandshakeTimeout => EngineFaultKind::Timeout,
        UciError::Terminated | UciError::Io(_) => EngineFaultKind::Disconnect,
        UciError::Protocol(_) => EngineFaultKind::Protocol,
        UciError::ShutdownTimeout => EngineFaultKind::Crash,
    }
}

/// Build the UCI `position` payload: from a FEN when the opening sets one,
/// otherwise from the standard start, in both cases followed by the moves so far.
fn build_uci_position(start_fen: Option<&str>, moves: &[String]) -> UciPosition {
    match start_fen {
        Some(fen) => UciPosition::Fen {
            fen: fen.to_string(),
            moves: moves.to_vec(),
        },
        None => UciPosition::StartPos {
            moves: moves.to_vec(),
        },
    }
}

fn initial_position(start_fen: Option<&str>) -> Chess {
    match start_fen {
        None => Chess::default(),
        Some(fen) => fen
            .parse::<shakmaty::fen::Fen>()
            .ok()
            .and_then(|f| f.into_position(CastlingMode::Standard).ok())
            .unwrap_or_default(),
    }
}

/// Handle a game where one (or both) engines failed to set up. Picks the
/// failing side (white takes precedence when both fail), quits any survivor,
/// writes a forensic incident report from the failed engine's transcript and
/// stderr, and returns the loss report.
async fn handle_setup_failure(
    spec: &GameSpec,
    white: Prepared,
    black: Prepared,
    monotonic_resolution_ns: u64,
) -> GameReport {
    // Consume each side: quit survivors, harvest forensics from the failure.
    async fn consume(
        prepared: Prepared,
    ) -> (Option<(UciError, bool)>, Option<(Vec<String>, Vec<String>)>) {
        match prepared {
            Prepared::Ready(engine) => {
                let _ = engine.quit(Duration::from_millis(500)).await;
                (None, None)
            }
            Prepared::Failed(err, engine) => (
                Some((err, false)),
                Some((engine.transcript(), engine.stderr_tail())),
            ),
            Prepared::NoSpawn(err) => (Some((err, true)), None),
            Prepared::Infrastructure(message, engine) => (
                Some((UciError::Protocol(message), true)),
                Some((engine.transcript(), engine.stderr_tail())),
            ),
        }
    }

    let (white_err, white_forensics) = consume(white).await;
    let (black_err, black_forensics) = consume(black).await;

    // White takes precedence when both failed.
    let (failed, err, spawn_failed, forensics) = if let Some((err, spawn_failed)) = white_err {
        (Color::White, err, spawn_failed, white_forensics)
    } else {
        let (err, spawn_failed) = black_err.expect("a setup failure occurred");
        (Color::Black, err, spawn_failed, black_forensics)
    };

    let outcome = if spawn_failed {
        Outcome::infrastructure("engine-spawn", err.to_string())
    } else {
        Outcome::engine_loss(
            failed,
            Termination::EngineCrash,
            fault_kind_for_uci(&err),
            Some(err.to_string()),
        )
    };
    let mut report = GameReport {
        game_id: spec.game_id,
        white: spec.white.id,
        black: spec.black.id,
        result: outcome.result,
        scorable: outcome.scorable,
        termination: outcome.termination,
        stats: GameStats::default(),
        san_moves: Vec::new(),
        uci_moves: Vec::new(),
        pgn: render_pgn(spec, &[], outcome.result, outcome.termination),
        clock_accounting: clock_accounting_report(spec, monotonic_resolution_ns, None, None),
        fault: outcome.fault,
        error: outcome.error,
    };

    // Forensic incident: setup failures never reach the main game-loop
    // reporter, so write one here — this is what makes an engine that dies on
    // startup (e.g. Junior under heavy load) diagnosable.
    let side_spec = if failed == Color::White {
        &spec.white
    } else {
        &spec.black
    };
    let text = setup_incident_report(spec, failed, side_spec, &err, forensics.as_ref());
    let stub = format!("SetupCrash-{}-r{}", side_spec.name, spec.round);
    if let Some(file) = crate::incidents::write(&stub, &text) {
        let detail = report.error.take().unwrap_or_default();
        report.error = Some(format!("{detail} — see logs/incidents/{file}"));
    }
    report
}

/// Plain-text incident report for an engine that failed during setup.
fn setup_incident_report(
    spec: &GameSpec,
    failed: Color,
    side_spec: &EngineGameSpec,
    err: &UciError,
    forensics: Option<&(Vec<String>, Vec<String>)>,
) -> String {
    use std::fmt::Write as _;
    let mut s = String::with_capacity(4 * 1024);
    let _ = writeln!(s, "event:       {} (round {})", spec.event, spec.round);
    let _ = writeln!(
        s,
        "failed side: {} — {}  [{}]",
        if failed == Color::White {
            "white"
        } else {
            "black"
        },
        side_spec.name,
        side_spec.spawn.path.display()
    );
    let _ = writeln!(
        s,
        "opponent:    {}",
        if failed == Color::White {
            &spec.black.name
        } else {
            &spec.white.name
        }
    );
    let _ = writeln!(s, "termination: EngineCrash (during setup)");
    let _ = writeln!(s, "detail:      {err}");
    let _ = writeln!(s, "handshake timeout: {:?}", spec.handshake_timeout);
    let _ = writeln!(
        s,
        "options sent: {}",
        side_spec
            .options
            .iter()
            .map(|(n, v)| match v {
                Some(v) => format!("{n}={v}"),
                None => n.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ")
    );
    match forensics {
        Some((transcript, stderr)) => {
            let _ = writeln!(s, "\n── UCI transcript (> sent, < received) ──");
            for line in transcript {
                let _ = writeln!(s, "{line}");
            }
            if stderr.is_empty() {
                let _ = writeln!(s, "── stderr: (none captured) ──");
            } else {
                let _ = writeln!(s, "── stderr ──");
                for line in stderr {
                    let _ = writeln!(s, "{line}");
                }
            }
        }
        None => {
            let _ = writeln!(
                s,
                "\n(process never spawned — check the executable path, its DLLs, \
                 and the working directory)"
            );
        }
    }
    s
}

fn render_pgn(
    spec: &GameSpec,
    san_moves: &[String],
    result: GameResult,
    termination: Termination,
) -> String {
    let tags = PgnTags {
        event: spec.event.clone(),
        site: spec.site.clone(),
        date: spec.date.clone(),
        round: spec.round,
        white: spec.white.name.clone(),
        black: spec.black.name.clone(),
        result,
        time_control: spec.time_control_label.clone(),
        termination: Some(termination),
        fen: spec.start_fen.clone(),
    };
    build_pgn(&tags, san_moves)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: Duration = Duration::from_millis(50);

    fn pos_from(fen: &str) -> Chess {
        fen.parse::<shakmaty::fen::Fen>()
            .unwrap()
            .into_position(CastlingMode::Standard)
            .unwrap()
    }

    #[test]
    fn lenient_parsing_defaults_bare_promotion_to_queen() {
        let pos = pos_from("k7/3P4/8/8/8/8/8/K7 w - - 0 1");
        let (mv, note) = parse_engine_move("d7d8", &pos).expect("accepted");
        assert!(note.is_some());
        assert_eq!(mv.to_uci(CastlingMode::Standard).to_string(), "d7d8q");
        // A correctly-specified promotion is passed through without a note.
        let (mv, note) = parse_engine_move("d7d8n", &pos).expect("accepted");
        assert!(note.is_none());
        assert_eq!(mv.to_uci(CastlingMode::Standard).to_string(), "d7d8n");
    }

    #[test]
    fn king_onto_rook_castling_is_canonicalized() {
        // shakmaty accepts Chess960-style castling notation even in standard
        // games; the canonical re-encoding must turn it into e1g1/e1c1 so the
        // OPPONENT's move list never sees the nonstandard form (a real desync
        // source before step 49 — the raw text used to be forwarded).
        let pos = pos_from("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        let (mv, _) = parse_engine_move("e1h1", &pos).expect("accepted");
        assert_eq!(mv.to_uci(CastlingMode::Standard).to_string(), "e1g1");
        let (mv, _) = parse_engine_move("e1a1", &pos).expect("accepted");
        assert_eq!(mv.to_uci(CastlingMode::Standard).to_string(), "e1c1");
    }

    #[test]
    fn lenient_parsing_accepts_uppercase() {
        let pos = Chess::default();
        let (mv, note) = parse_engine_move("E2E4", &pos).expect("accepted");
        assert!(note.is_some());
        assert_eq!(mv.to_uci(CastlingMode::Standard).to_string(), "e2e4");
    }

    #[test]
    fn lenient_parsing_still_rejects_garbage_and_illegal() {
        let pos = Chess::default();
        assert!(parse_engine_move("(none)", &pos).is_none());
        assert!(parse_engine_move("0000", &pos).is_none());
        assert!(parse_engine_move("e2e5", &pos).is_none()); // illegal push
        assert!(parse_engine_move("d3c2", &pos).is_none()); // no piece there
    }

    #[test]
    fn per_move_limits_are_constant_per_move() {
        let tc = TimeControl::PerMove { ms: 200 };
        let clocks = Clocks::new(&tc, &tc);
        let (limits, deadline) = move_limits(&tc, &clocks, Color::White, TOL);
        assert_eq!(limits, GoLimits::MoveTime(Duration::from_millis(200)));
        assert_eq!(deadline, Duration::from_millis(250));
    }

    #[test]
    fn sudden_death_clock_decrements_without_increment() {
        let tc = TimeControl::SuddenDeath { base_ms: 1000 };
        let mut clocks = Clocks::new(&tc, &tc);
        assert_eq!(clocks.white, Duration::from_millis(1000));

        let (limits, deadline) = move_limits(&tc, &clocks, Color::White, TOL);
        assert_eq!(
            limits,
            GoLimits::Clock {
                wtime: Duration::from_millis(1000),
                btime: Duration::from_millis(1000),
                winc: Duration::ZERO,
                binc: Duration::ZERO,
            }
        );
        assert_eq!(deadline, Duration::from_millis(1050));

        assert!(clocks.accept_and_charge(&tc, Color::White, Duration::from_millis(300), TOL));
        assert_eq!(clocks.white, Duration::from_millis(700));
        assert_eq!(clocks.black, Duration::from_millis(1000));
    }

    #[test]
    fn increment_is_credited_after_each_move() {
        let tc = TimeControl::Increment {
            base_ms: 1000,
            inc_ms: 100,
        };
        let mut clocks = Clocks::new(&tc, &tc);
        assert!(clocks.accept_and_charge(&tc, Color::Black, Duration::from_millis(400), TOL));
        // 1000 - 400 + 100 = 700
        assert_eq!(clocks.black, Duration::from_millis(700));

        // An overrun beyond remaining plus margin forfeits before increment.
        assert!(!clocks.accept_and_charge(&tc, Color::Black, Duration::from_millis(751), TOL));
        assert_eq!(clocks.black, Duration::from_millis(700));
    }

    #[test]
    fn increment_margin_boundaries_deduct_before_credit_and_accept_equality() {
        let tc = TimeControl::Increment {
            base_ms: 1000,
            inc_ms: 100,
        };
        for (elapsed_ms, accepted) in [(1049, true), (1050, true), (1051, false)] {
            let mut clocks = Clocks::new(&tc, &tc);
            assert_eq!(
                clocks.accept_and_charge(
                    &tc,
                    Color::White,
                    Duration::from_millis(elapsed_ms),
                    Duration::from_millis(50),
                ),
                accepted
            );
            assert_eq!(
                clocks.white,
                if accepted {
                    Duration::from_millis(100)
                } else {
                    Duration::from_millis(1000)
                }
            );
        }
    }

    #[test]
    fn nodes_and_depth_use_fixed_limits_and_safety_deadline() {
        let nodes = TimeControl::Nodes { nodes: 50_000 };
        let clocks = Clocks::new(&nodes, &nodes);
        let (limits, deadline) = move_limits(&nodes, &clocks, Color::White, TOL);
        assert_eq!(limits, GoLimits::Nodes(50_000));
        assert_eq!(deadline, FIXED_SEARCH_DEADLINE);

        let depth = TimeControl::Depth { depth: 12 };
        let (limits, deadline) = move_limits(&depth, &clocks, Color::Black, TOL);
        assert_eq!(limits, GoLimits::Depth(12));
        assert_eq!(deadline, FIXED_SEARCH_DEADLINE);
    }

    #[test]
    fn charging_is_a_noop_for_non_clock_controls() {
        let tc = TimeControl::PerMove { ms: 100 };
        let mut clocks = Clocks::new(&tc, &tc);
        assert!(clocks.accept_and_charge(
            &tc,
            Color::White,
            Duration::from_millis(50),
            Duration::ZERO
        ));
        assert_eq!(clocks.white, Duration::ZERO);
        assert_eq!(clocks.black, Duration::ZERO);
    }

    #[test]
    fn charged_elapsed_summary_uses_the_ordered_middle() {
        let mut samples = MoveTimeAccumulator::default();
        for ms in [9, 1, 5, 3] {
            samples.add(Duration::from_millis(ms));
        }
        let summary = samples.summary().unwrap();
        assert_eq!(summary.samples, 4);
        assert_eq!(summary.min_ns, 1_000_000);
        assert_eq!(summary.median_ns, 4_000_000);
        assert_eq!(summary.max_ns, 9_000_000);
    }
}
