//! Shared live-game state for the GUI's live view.
//!
//! Each running game owns one [`LiveGameState`] behind an `Arc<Mutex<_>>`. The
//! runner writes to it as the game unfolds — every parsed `info` line while an
//! engine thinks, every move played, every clock update — and the GUI reads the
//! selected game's state each frame. Writes are a handful of small field
//! updates, so the per-game mutex is uncontended in practice even with many
//! parallel games.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use colosseum_core::{EngineId, GameId, GameResult, Termination, TimeControl};
use colosseum_uci::Score;

/// A game's live state behind a shared handle.
pub type LiveGameHandle = Arc<Mutex<LiveGameState>>;

/// The latest search information reported by one side. While the side is
/// thinking these update live; once it moves they freeze at the final values
/// of that search ("last search" in the GUI).
#[derive(Debug, Clone, Default)]
pub struct LiveSearch {
    /// Last reported score, normalized to White's point of view.
    pub score: Option<Score>,
    pub depth: Option<u32>,
    pub seldepth: Option<u32>,
    pub nodes: Option<u64>,
    pub nps: Option<u64>,
    /// Principal variation as UCI moves.
    pub pv: Vec<String>,
}

/// One completed depth iteration in an engine's search log (the Fritz-style
/// output pane): pushed when the engine reports a deeper PV, kept across
/// moves as a rolling per-engine log.
#[derive(Debug, Clone)]
pub struct SearchLine {
    /// White-POV score (same convention as [`LiveSearch::score`]).
    pub score: Option<Score>,
    pub depth: u32,
    pub seldepth: Option<u32>,
    pub nodes: Option<u64>,
    /// Time into the search when this line was reported, in ms.
    pub elapsed_ms: u64,
    /// Principal variation as UCI moves (capped — it's a one-line display).
    pub pv: Vec<String>,
}

/// Rolling cap for each side's search log.
pub const SEARCH_LOG_CAP: usize = 80;

/// One eval-history point for the live graph: the final white-POV score of the
/// search that produced a move.
#[derive(Debug, Clone, Copy)]
pub struct EvalPoint {
    /// Number of plies played when the move completed (opening pre-moves included).
    pub ply: u32,
    /// Whether the white engine produced this point.
    pub by_white: bool,
    /// White-POV score of the completed search.
    pub score: Score,
}

/// Everything the live view renders for one game.
#[derive(Debug)]
pub struct LiveGameState {
    pub game_id: GameId,
    pub round: u32,
    pub white: EngineId,
    pub black: EngineId,
    pub white_name: String,
    pub black_name: String,
    /// `None` => standard start position.
    pub start_fen: Option<String>,
    pub time_control: TimeControl,
    pub san_moves: Vec<String>,
    pub uci_moves: Vec<String>,
    /// Plies pre-played from the opening book (no engine eval for these).
    pub opening_plies: u32,
    /// Remaining clock per side (`None` for non-clock time controls).
    pub white_clock_ms: Option<u64>,
    pub black_clock_ms: Option<u64>,
    /// Which side is to move (and thinking, while `finished` is `None`).
    pub white_to_move: bool,
    /// Set while a side has an active `go ponder` search (thinking on the
    /// opponent's time); cleared when the ponder resolves.
    pub white_pondering: bool,
    pub black_pondering: bool,
    /// When the current search started (drives the "time on move" display).
    pub search_started: Option<Instant>,
    pub white_search: LiveSearch,
    pub black_search: LiveSearch,
    /// Per-side search logs (one line per completed depth), newest last,
    /// capped at [`SEARCH_LOG_CAP`].
    pub white_log: Vec<SearchLine>,
    pub black_log: Vec<SearchLine>,
    /// Eval history for the graph, in move order.
    pub evals: Vec<EvalPoint>,
    /// Set once when the game ends; the state then stops changing.
    pub finished: Option<(GameResult, Termination)>,
}

impl LiveGameState {
    /// Fresh pre-game state; the runner fills everything else in as it plays.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        game_id: GameId,
        round: u32,
        white: (EngineId, String),
        black: (EngineId, String),
        start_fen: Option<String>,
        time_control: TimeControl,
    ) -> Self {
        Self {
            game_id,
            round,
            white: white.0,
            black: black.0,
            white_name: white.1,
            black_name: black.1,
            start_fen,
            time_control,
            san_moves: Vec::new(),
            uci_moves: Vec::new(),
            opening_plies: 0,
            white_clock_ms: None,
            black_clock_ms: None,
            white_to_move: true,
            white_pondering: false,
            black_pondering: false,
            search_started: None,
            white_search: LiveSearch::default(),
            black_search: LiveSearch::default(),
            white_log: Vec::new(),
            black_log: Vec::new(),
            evals: Vec::new(),
            finished: None,
        }
    }

    /// A fresh state wrapped in its shared handle.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new_handle(
        game_id: GameId,
        round: u32,
        white: (EngineId, String),
        black: (EngineId, String),
        start_fen: Option<String>,
        time_control: TimeControl,
    ) -> LiveGameHandle {
        Arc::new(Mutex::new(Self::new(
            game_id,
            round,
            white,
            black,
            start_fen,
            time_control,
        )))
    }

    /// The engine name for one side.
    #[must_use]
    pub fn name(&self, white: bool) -> &str {
        if white { &self.white_name } else { &self.black_name }
    }
}

/// Flip a side-to-move score to White's point of view.
#[must_use]
pub fn to_white_pov(score: Score, mover_is_white: bool) -> Score {
    if mover_is_white {
        score
    } else {
        match score {
            Score::Cp(cp) => Score::Cp(-cp),
            Score::Mate(m) => Score::Mate(-m),
        }
    }
}
