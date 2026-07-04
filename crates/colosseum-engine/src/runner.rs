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

        // Parse and validate the move, tolerating two common nonstandard
        // notations from older engines (see `parse_engine_move`).
        let Some((legal_move, leniency)) = parse_engine_move(&output.best_move, &pos) else {
            break Outcome::loss(
                mover,
                Termination::IllegalMove,
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
    let _ = writeln!(s, "white:       {}  [{}]", spec.white.name, spec.white.spawn.path.display());
    let _ = writeln!(s, "black:       {}  [{}]", spec.black.name, spec.black.spawn.path.display());
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
        let _ = writeln!(s, "\n── {label} UCI transcript (last lines; > sent, < received; info collapsed) ──");
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
