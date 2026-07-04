//! The game runner: drive two engines through one complete game.
//!
//! Uses `shakmaty` for legality/SAN and natural game-end detection, normalizes engine
//! scores to White's point of view for [`adjudicate`], and produces valid PGN. The
//! engines are spawned, configured, played and shut down within [`run_game`], so a
//! game owns its processes (and `kill_on_drop` cleans them up on cancellation).

use std::collections::HashMap;
use std::time::Duration;

use colosseum_core::{
    AdjudicationConfig, EngineId, GameId, GameResult, GameStats, Termination, TimeControl,
    adjudicate,
};
use colosseum_uci::{EngineProcess, GoLimits, SpawnOptions, UciError, UciPosition};
use shakmaty::san::SanPlus;
use shakmaty::uci::UciMove;
use shakmaty::zobrist::Zobrist64;
use shakmaty::{CastlingMode, Chess, Color, EnPassantMode, Position};

use crate::pgn::{PgnTags, build_pgn};

/// Centipawn magnitude used to represent mate scores for adjudication.
const ADJ_MATE_CP: i32 = 100_000;
/// Hard safety cap on plies (natural draw rules normally end games far sooner).
const MAX_PLIES: usize = 6000;
/// Wall-clock safety cutoff for node-/depth-limited searches, which carry no clock
/// of their own. A search that blows past this is treated as a crash-class hang.
const FIXED_SEARCH_DEADLINE: Duration = Duration::from_secs(600);

/// How to launch and configure one side.
#[derive(Debug, Clone)]
pub struct EngineGameSpec {
    pub id: EngineId,
    pub name: String,
    pub spawn: SpawnOptions,
    /// Resolved `setoption` commands (`value` is `None` for buttons).
    pub options: Vec<(String, Option<String>)>,
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
    pub time_control: TimeControl,
    pub time_control_label: String,
    pub adjudication: AdjudicationConfig,
    pub timeout_tolerance: Duration,
    pub handshake_timeout: Duration,
}

/// The outcome of a finished game.
#[derive(Debug, Clone)]
pub struct GameReport {
    pub game_id: GameId,
    pub white: EngineId,
    pub black: EngineId,
    pub result: GameResult,
    pub termination: Termination,
    pub stats: GameStats,
    pub san_moves: Vec<String>,
    pub uci_moves: Vec<String>,
    pub pgn: String,
    /// Set when the game ended due to an engine problem (crash/illegal/timeout).
    pub error: Option<String>,
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

/// Spawn, configure, and ready one engine for a game.
async fn prepare(
    spec: &EngineGameSpec,
    handshake_timeout: Duration,
) -> Result<EngineProcess, UciError> {
    // Wine-launched engines: make sure the per-engine prefix exists (normally
    // created at add time; this covers clones and configs moved between
    // machines). A cheap directory check once initialised.
    crate::runtime::ensure_prefix_for(&spec.spawn)
        .await
        .map_err(|e| UciError::Io(std::io::Error::other(e.to_string())))?;
    let mut engine = EngineProcess::spawn(spec.spawn.clone()).await?;
    engine.handshake(handshake_timeout).await?;
    for (name, value) in &spec.options {
        engine.set_option(name, value.as_deref()).await?;
    }
    engine.is_ready(handshake_timeout).await?;
    engine.new_game().await?;
    engine.is_ready(handshake_timeout).await?;
    Ok(engine)
}

/// Play one complete game and return its report. Never panics on engine misbehavior.
pub async fn run_game(spec: GameSpec) -> GameReport {
    let game_start = std::time::Instant::now();

    let white = prepare(&spec.white, spec.handshake_timeout).await;
    let black = prepare(&spec.black, spec.handshake_timeout).await;

    let (mut white, mut black) = match (white, black) {
        (Ok(w), Ok(b)) => (w, b),
        (Err(err), Ok(b)) => {
            let _ = b.quit(Duration::from_millis(500)).await;
            let mut report = setup_failure(&spec, Color::White, &err);
            report.stats.duration_ms = Some(game_start.elapsed().as_millis() as u64);
            return report;
        }
        (Ok(w), Err(err)) => {
            let _ = w.quit(Duration::from_millis(500)).await;
            let mut report = setup_failure(&spec, Color::Black, &err);
            report.stats.duration_ms = Some(game_start.elapsed().as_millis() as u64);
            return report;
        }
        (Err(err), Err(_)) => {
            let mut report = setup_failure(&spec, Color::White, &err);
            report.stats.duration_ms = Some(game_start.elapsed().as_millis() as u64);
            return report;
        }
    };

    let mut pos = initial_position(spec.start_fen.as_deref());
    let mut clocks = Clocks::new(&spec.time_control);

    let mut san_moves: Vec<String> = Vec::new();
    let mut uci_moves: Vec<String> = Vec::new();
    let mut white_pov: Vec<i32> = Vec::new();
    let mut last_white_pov = 0i32;
    let mut repetitions: HashMap<Zobrist64, u8> = HashMap::new();
    let mut white_nps = NpsAccumulator::default();
    let mut black_nps = NpsAccumulator::default();

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

    let outcome = loop {
        if san_moves.len() >= MAX_PLIES {
            break Outcome::natural(GameResult::Draw, Termination::MaxMoves);
        }

        let mover = pos.turn();
        let engine = if mover == Color::White {
            &mut white
        } else {
            &mut black
        };
        let position = build_uci_position(spec.start_fen.as_deref(), &uci_moves);

        let (limits, deadline) =
            move_limits(&spec.time_control, &clocks, mover, spec.timeout_tolerance);
        let search = engine.search(&position, &limits, deadline).await;

        let output = match search {
            Ok(output) => output,
            Err(UciError::MoveTimeout) => {
                break Outcome::loss(mover, Termination::TimeForfeit, None);
            }
            Err(err) => {
                break Outcome::loss(mover, Termination::EngineCrash, Some(err.to_string()));
            }
        };

        if mover == Color::White {
            white_nps.add(output.nps);
        } else {
            black_nps.add(output.nps);
        }

        // Deduct the time used from the mover's clock (clock-based controls only)
        // and credit the increment. Flagging is enforced by the search deadline
        // above: an engine that runs out is cut off and loses on TimeForfeit.
        clocks.consume(&spec.time_control, mover, output.elapsed);

        // Parse and validate the move.
        let parsed = output
            .best_move
            .parse::<UciMove>()
            .ok()
            .and_then(|uci| uci.to_move(&pos).ok());
        let Some(legal_move) = parsed else {
            break Outcome::loss(
                mover,
                Termination::IllegalMove,
                Some(format!("illegal move: {}", output.best_move)),
            );
        };

        san_moves.push(SanPlus::from_move(pos.clone(), legal_move).to_string());
        uci_moves.push(output.best_move.clone());

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
    };

    // Shut engines down gracefully (kill_on_drop covers anything left).
    let _ = white.quit(Duration::from_millis(500)).await;
    let _ = black.quit(Duration::from_millis(500)).await;

    let stats = GameStats {
        plies: san_moves.len() as u32,
        white_nps: white_nps.average(),
        black_nps: black_nps.average(),
        duration_ms: Some(game_start.elapsed().as_millis() as u64),
    };
    let pgn = render_pgn(&spec, &san_moves, outcome.result, outcome.termination);

    GameReport {
        game_id: spec.game_id,
        white: spec.white.id,
        black: spec.black.id,
        result: outcome.result,
        termination: outcome.termination,
        stats,
        san_moves,
        uci_moves,
        pgn,
        error: outcome.error,
    }
}

/// Per-side game clocks for clock-based time controls. Both sides start at the
/// configured base; for fixed-time / nodes / depth controls the clocks are unused.
struct Clocks {
    white: Duration,
    black: Duration,
}

impl Clocks {
    fn new(tc: &TimeControl) -> Self {
        let base = tc.initial_clock().unwrap_or(Duration::ZERO);
        Self {
            white: base,
            black: base,
        }
    }

    fn remaining(&self, side: Color) -> Duration {
        match side {
            Color::White => self.white,
            Color::Black => self.black,
        }
    }

    /// Subtract the time spent and credit the increment for the side that moved.
    /// A no-op for non-clock controls.
    fn consume(&mut self, tc: &TimeControl, side: Color, used: Duration) {
        if !tc.is_clock() {
            return;
        }
        let inc = tc.increment();
        let clock = match side {
            Color::White => &mut self.white,
            Color::Black => &mut self.black,
        };
        *clock = clock.saturating_sub(used) + inc;
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
    termination: Termination,
    error: Option<String>,
}

impl Outcome {
    fn natural(result: GameResult, termination: Termination) -> Self {
        Self {
            result,
            termination,
            error: None,
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
            termination,
            error: None,
        }
    }

    /// The side to move loses (timeout/crash/illegal).
    fn loss(mover: Color, termination: Termination, error: Option<String>) -> Self {
        Self {
            result: if mover == Color::White {
                GameResult::BlackWin
            } else {
                GameResult::WhiteWin
            },
            termination,
            error,
        }
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

/// Build a report for an engine that failed during setup; the other side wins.
fn setup_failure(spec: &GameSpec, failed: Color, err: &UciError) -> GameReport {
    let outcome = Outcome::loss(failed, Termination::EngineCrash, Some(err.to_string()));
    let pgn = render_pgn(spec, &[], outcome.result, outcome.termination);
    GameReport {
        game_id: spec.game_id,
        white: spec.white.id,
        black: spec.black.id,
        result: outcome.result,
        termination: outcome.termination,
        stats: GameStats::default(),
        san_moves: Vec::new(),
        uci_moves: Vec::new(),
        pgn,
        error: outcome.error,
    }
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

    #[test]
    fn per_move_limits_are_constant_per_move() {
        let tc = TimeControl::PerMove { ms: 200 };
        let clocks = Clocks::new(&tc);
        let (limits, deadline) = move_limits(&tc, &clocks, Color::White, TOL);
        assert_eq!(limits, GoLimits::MoveTime(Duration::from_millis(200)));
        assert_eq!(deadline, Duration::from_millis(250));
    }

    #[test]
    fn sudden_death_clock_decrements_without_increment() {
        let tc = TimeControl::SuddenDeath { base_ms: 1000 };
        let mut clocks = Clocks::new(&tc);
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

        clocks.consume(&tc, Color::White, Duration::from_millis(300));
        assert_eq!(clocks.white, Duration::from_millis(700));
        assert_eq!(clocks.black, Duration::from_millis(1000));
    }

    #[test]
    fn increment_is_credited_after_each_move() {
        let tc = TimeControl::Increment {
            base_ms: 1000,
            inc_ms: 100,
        };
        let mut clocks = Clocks::new(&tc);
        clocks.consume(&tc, Color::Black, Duration::from_millis(400));
        // 1000 - 400 + 100 = 700
        assert_eq!(clocks.black, Duration::from_millis(700));

        // Overrunning the remaining time floors at zero before the increment.
        clocks.consume(&tc, Color::Black, Duration::from_millis(5000));
        assert_eq!(clocks.black, Duration::from_millis(100));
    }

    #[test]
    fn nodes_and_depth_use_fixed_limits_and_safety_deadline() {
        let nodes = TimeControl::Nodes { nodes: 50_000 };
        let clocks = Clocks::new(&nodes);
        let (limits, deadline) = move_limits(&nodes, &clocks, Color::White, TOL);
        assert_eq!(limits, GoLimits::Nodes(50_000));
        assert_eq!(deadline, FIXED_SEARCH_DEADLINE);

        let depth = TimeControl::Depth { depth: 12 };
        let (limits, deadline) = move_limits(&depth, &clocks, Color::Black, TOL);
        assert_eq!(limits, GoLimits::Depth(12));
        assert_eq!(deadline, FIXED_SEARCH_DEADLINE);
    }

    #[test]
    fn consume_is_a_noop_for_non_clock_controls() {
        let tc = TimeControl::PerMove { ms: 100 };
        let mut clocks = Clocks::new(&tc);
        clocks.consume(&tc, Color::White, Duration::from_millis(50));
        assert_eq!(clocks.white, Duration::ZERO);
        assert_eq!(clocks.black, Duration::ZERO);
    }
}
