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
    let white = prepare(&spec.white, spec.handshake_timeout).await;
    let black = prepare(&spec.black, spec.handshake_timeout).await;

    let (mut white, mut black) = match (white, black) {
        (Ok(w), Ok(b)) => (w, b),
        (Err(err), Ok(b)) => {
            let _ = b.quit(Duration::from_millis(500)).await;
            return setup_failure(&spec, Color::White, &err);
        }
        (Ok(w), Err(err)) => {
            let _ = w.quit(Duration::from_millis(500)).await;
            return setup_failure(&spec, Color::Black, &err);
        }
        (Err(err), Err(_)) => return setup_failure(&spec, Color::White, &err),
    };

    let mut pos = initial_position(spec.start_fen.as_deref());
    let movetime = spec
        .time_control
        .movetime()
        .unwrap_or(Duration::from_millis(100));
    let deadline = movetime + spec.timeout_tolerance;

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
        san_moves.push(SanPlus::from_move(pos.clone(), legal.clone()).to_string());
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

        let search = engine
            .search(&position, &GoLimits::MoveTime(movetime), deadline)
            .await;

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

        san_moves.push(SanPlus::from_move(pos.clone(), legal_move.clone()).to_string());
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
