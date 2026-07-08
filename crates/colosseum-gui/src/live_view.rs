// SPDX-License-Identifier: GPL-3.0-or-later
//! The live game view: watch a running game with a full board, per-engine
//! panels, and an evaluation graph.
//!
//! Layout (left to right): games rail (only with 2+ games in flight) · moves
//! column (matchup, opening, move list, material) · the board, as big as the
//! window allows · engine column (black panel on top beside black's pieces,
//! eval graph in the middle, white panel at the bottom). The two engine panels
//! are equal-height, so the graph's vertical centre — eval 0 — sits level with
//! the board's 4th/5th-rank border. Below a minimum board size the whole body
//! scrolls instead of shrinking further.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align2, Color32, Pos2, Rect, RichText, ScrollArea, Sense, Stroke, Ui, pos2, vec2,
};
use shakmaty::zobrist::Zobrist64;
use shakmaty::{
    CastlingMode, Chess, Color as ChessColor, EnPassantMode, Position, Role, Square, fen::Fen,
    uci::UciMove,
};

use colosseum_core::{EngineId, GameId, GameResult, Termination, TournamentId};
use colosseum_engine::live::{LiveSearch, SearchLine};
use colosseum_engine::{InFlightGame, LiveGameHandle, Score};

use crate::backend::Backend;
use crate::{board, eco, logo, theme, widgets};

const RAIL_W: f32 = 150.0;
const MOVES_W: f32 = 240.0;
/// The engine/graph column is elastic: at least this wide (so the header's
/// four sections — logo, name+eval, stats, clock — never collide), but it
/// soaks up whatever horizontal space the board leaves rather than stranding
/// it as dead margin — the wider it gets, the more room the eval graph has.
const RIGHT_MIN: f32 = 400.0;
const RIGHT_MAX: f32 = 720.0;
const GAP: f32 = 10.0;
/// Safety margin subtracted from the available size before sizing the columns.
/// An *exact* fit trips `ScrollArea` into showing bars (frame strokes overhang
/// half a pixel), and one bar's lane then forces the other bar too.
const FIT_EPS: f32 = 4.0;
/// The eval graph is a wide, shallow strip: cap its painted height and keep
/// it centred so the 0-line stays on the board midline.
const GRAPH_MAX_H: f32 = 160.0;
/// Engine-column card metrics (also drive the layout's minimum height).
const CARD_PAD: f32 = 12.0;
const DIVIDER_H: f32 = 9.0;
const PANEL_MIN: f32 = 150.0;
const PANEL_MAX: f32 = 320.0;
const GRAPH_MIN: f32 = 60.0;
/// Below this board size the body scrolls instead of shrinking further.
const MIN_BOARD: f32 = 320.0;
/// The real lower bound for the board/columns: every column must still render
/// its minimum content (the engine card is the tallest constraint), so the
/// board never shrinks below what the side sections need — and scrollbars only
/// appear once *this* size no longer fits.
const MIN_SIDE: f32 = {
    let column = 2.0 * PANEL_MIN + GRAPH_MIN + 2.0 * DIVIDER_H + 2.0 * CARD_PAD;
    if MIN_BOARD > column {
        MIN_BOARD
    } else {
        column
    }
};
/// Hold the result banner this long before auto-following to the next game.
const FOLLOW_DELAY: Duration = Duration::from_secs(2);
/// Graph range snap steps (pawns).
const RANGE_STEPS: [f32; 5] = [1.0, 2.0, 3.0, 5.0, 10.0];

/// Per-tournament live-view states plus the shared logo cache.
#[derive(Default)]
pub struct LiveViews {
    states: HashMap<TournamentId, ViewState>,
    logos: logo::LogoCache,
    /// Whether the games rail (left) is collapsed to a slim strip.
    rail_collapsed: bool,
}

struct ViewState {
    /// `true` = the Live lens is active (vs. Standings).
    watching: bool,
    auto_follow: bool,
    selected: Option<Selected>,
    /// When the selected game was first seen finished (auto-follow delay).
    finished_since: Option<Instant>,
    /// Highest `launch_seq` in flight when the watched game finished; the
    /// auto-follow target must be launched *after* this (a fresh game).
    finish_watermark: Option<u64>,
    replay: Option<Replay>,
    /// Current eval-graph range ±R (monotonic per game, see `update_range`).
    range: f32,
}

impl ViewState {
    fn new(watching: bool, concurrency: usize) -> Self {
        Self {
            watching,
            // Single-game tournaments follow the action by default; parallel
            // ones leave the choice to the viewer.
            auto_follow: concurrency <= 1,
            selected: None,
            finished_since: None,
            finish_watermark: None,
            replay: None,
            range: RANGE_STEPS[0],
        }
    }
}

/// The watched game. The handle is kept even after the game leaves the
/// in-flight list, so a finished game stays frozen on screen.
struct Selected {
    game_id: GameId,
    launch_seq: u64,
    live: LiveGameHandle,
}

/// Incrementally replayed position for the selected game.
struct Replay {
    game_id: GameId,
    applied: usize,
    pos: Chess,
    last_move: Option<(Option<Square>, Square)>,
    opening: Option<eco::Opening>,
    standard_start: bool,
}

/// An owned per-frame copy of the watched game's state (lock held briefly).
struct Snap {
    round: u32,
    white_name: String,
    black_name: String,
    white_id: EngineId,
    black_id: EngineId,
    start_fen: Option<String>,
    san: Vec<String>,
    uci: Vec<String>,
    white_clock_ms: Option<u64>,
    black_clock_ms: Option<u64>,
    white_to_move: bool,
    white_pondering: bool,
    black_pondering: bool,
    search_elapsed: Option<Duration>,
    white_search: LiveSearch,
    black_search: LiveSearch,
    white_log: Vec<SearchLine>,
    black_log: Vec<SearchLine>,
    evals: Vec<colosseum_engine::EvalPoint>,
    finished: Option<(GameResult, Termination)>,
}

fn snapshot(live: &LiveGameHandle) -> Option<Snap> {
    let lg = live.lock().ok()?;
    Some(Snap {
        round: lg.round,
        white_name: lg.white_name.clone(),
        black_name: lg.black_name.clone(),
        white_id: lg.white,
        black_id: lg.black,
        start_fen: lg.start_fen.clone(),
        san: lg.san_moves.clone(),
        uci: lg.uci_moves.clone(),
        white_clock_ms: lg.white_clock_ms,
        black_clock_ms: lg.black_clock_ms,
        white_to_move: lg.white_to_move,
        white_pondering: lg.white_pondering,
        black_pondering: lg.black_pondering,
        search_elapsed: lg.search_started.map(|t| t.elapsed()),
        white_search: lg.white_search.clone(),
        black_search: lg.black_search.clone(),
        white_log: lg.white_log.clone(),
        black_log: lg.black_log.clone(),
        evals: lg.evals.clone(),
        finished: lg.finished,
    })
}

impl LiveViews {
    /// Whether the Live lens is active for this tournament.
    #[must_use]
    pub fn is_watching(&self, id: TournamentId) -> bool {
        self.states.get(&id).is_some_and(|s| s.watching)
    }

    /// The `Standings | Live` lens switcher plus the auto-follow toggle,
    /// laid out left-to-right in the control bar. Always the same widgets in
    /// the same order, so nothing shifts as games start and finish.
    pub fn header_controls(
        &mut self,
        ui: &mut Ui,
        id: TournamentId,
        in_flight: usize,
        concurrency: usize,
    ) {
        let state = self.state(id, concurrency);
        widgets::choice_chip(ui, &mut state.watching, false, "Standings");
        // The dot glows green while games are in flight — visible from the
        // Standings lens too.
        let (label, dot) = if in_flight > 0 {
            (format!("Live ({in_flight})"), Some(theme::success()))
        } else {
            ("Live".to_string(), None)
        };
        widgets::choice_chip_dot(ui, &mut state.watching, true, &label, dot);
        ui.add_space(8.0);
        auto_follow_chip(ui, &mut state.auto_follow);
    }

    /// Switch to the Live lens with `game` selected (Playing-panel click).
    pub fn watch_game(&mut self, id: TournamentId, game: &InFlightGame, concurrency: usize) {
        let state = self.state(id, concurrency);
        state.watching = true;
        state.select(game);
    }

    fn state(&mut self, id: TournamentId, concurrency: usize) -> &mut ViewState {
        self.states
            .entry(id)
            .or_insert_with(|| ViewState::new(false, concurrency))
    }

    /// Draw the live body. Call only when watching.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        backend: &Backend,
        id: TournamentId,
        in_flight: &[InFlightGame],
        concurrency: usize,
    ) {
        self.logos.begin_frame();
        let logos = &mut self.logos;
        let rail_collapsed = &mut self.rail_collapsed;
        let state = self
            .states
            .entry(id)
            .or_insert_with(|| ViewState::new(true, concurrency));

        let mut games: Vec<&InFlightGame> = in_flight.iter().collect();
        games.sort_by_key(|g| g.launch_seq);
        state.reconcile_selection(&games);

        let Some(selected) = &state.selected else {
            empty_state(ui);
            return;
        };
        let Some(snap) = snapshot(&selected.live) else {
            empty_state(ui);
            return;
        };
        let selected_id = selected.game_id;

        // Keep the view animating (clocks, thinking numbers) while visible.
        ui.ctx().request_repaint_after(Duration::from_millis(100));

        state.update_replay(selected_id, &snap);
        state.update_range(&snap);

        // Games rail (2+ games): a real side panel — resizable, and
        // collapsible to a slim strip for more board space.
        let mut clicked_game: Option<GameId> = None;
        if games.len() >= 2 {
            if *rail_collapsed {
                egui::Panel::left("live_rail_collapsed")
                    .exact_size(24.0)
                    .resizable(false)
                    .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(2, 6)))
                    .show(ui, |ui| {
                        if widgets::expand_strip(ui, "›", &format!("Playing ({})", games.len())) {
                            *rail_collapsed = false;
                        }
                    });
            } else {
                egui::Panel::left("live_rail")
                    .default_size(RAIL_W)
                    .size_range(110.0..=280.0)
                    .resizable(true)
                    .frame(egui::Frame::new().inner_margin(egui::Margin {
                        left: 0,
                        right: 8,
                        top: 0,
                        bottom: 0,
                    }))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("Playing ({})", games.len()))
                                    .color(theme::text())
                                    .font(theme::semibold(12.0)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if widgets::collapse_button(ui, "‹")
                                        .on_hover_text("Hide the games list.")
                                        .clicked()
                                    {
                                        *rail_collapsed = true;
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);
                        clicked_game = games_rail(ui, &games, selected_id);
                    });
            }
        }

        // Fixed columns to the left of the elastic engine column: the moves
        // column and the inter-column gaps (moves↔board↔right). The rail is
        // a panel, already subtracted from the available size.
        let left_cols = MOVES_W + GAP * 2.0;
        let avail = ui.available_size();
        // Usable area (safety epsilon: an exact fit shows spurious scrollbars).
        let usable_w = avail.x - FIT_EPS;
        let usable_h = avail.y - FIT_EPS;
        // The board is square and height-bound; give it what height allows but
        // never so much width that the engine column drops below its minimum.
        // Everything scales down together to `MIN_SIDE` (every column's
        // minimum content still fits there); only below that do scrollbars
        // appear, with the layout frozen at its minimum.
        let board_side = usable_h.min(usable_w - left_cols - RIGHT_MIN).max(MIN_SIDE);
        // The engine column takes the remaining width (clamped), so a wide
        // window widens the graph instead of leaving a blank strip on the right.
        let right_w = (usable_w - left_cols - board_side).clamp(RIGHT_MIN, RIGHT_MAX);

        let body = |ui: &mut Ui, state: &ViewState, logos: &mut logo::LogoCache| {
            ui.horizontal_top(|ui| {
                // The item spacing IS the column gap — explicit spacers on
                // top of it would make the row wider than the computed
                // board size and spill into scrollbars.
                ui.spacing_mut().item_spacing.x = GAP;
                // Columns get explicit top-down layouts: `allocate_ui`
                // would inherit the surrounding left-to-right flow and
                // spill their contents sideways.
                let column = egui::Layout::top_down(egui::Align::Min);
                ui.allocate_ui_with_layout(vec2(MOVES_W, board_side), column, |ui| {
                    ui.set_max_width(MOVES_W);
                    moves_column(ui, &snap, state.replay.as_ref(), board_side);
                });
                let (board_rect, _) =
                    ui.allocate_exact_size(vec2(board_side, board_side), Sense::hover());
                if let Some(replay) = &state.replay {
                    board::draw(ui, board_rect, replay.pos.board(), replay.last_move);
                }
                if let Some((result, termination)) = snap.finished {
                    result_banner(ui, board_rect, result, termination);
                }
                ui.allocate_ui_with_layout(vec2(right_w, board_side), column, |ui| {
                    ui.set_max_width(right_w);
                    engine_column(ui, backend, logos, state, &snap, right_w, board_side);
                });
            });
        };
        // A ScrollArea only when the window is genuinely below the minimum
        // layout. Wrapping unconditionally trapped the view in permanent
        // scrollbars: content was sized from the pre-bar available size, so
        // the moment a transient overflow showed a bar, its lane shrank the
        // viewport below the content and the bars never went away.
        let fits = usable_h >= MIN_SIDE && usable_w >= left_cols + MIN_SIDE + RIGHT_MIN;
        if fits {
            body(ui, state, logos);
        } else {
            ScrollArea::both()
                .id_salt("live_view_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| body(ui, state, logos));
        }

        if let Some(gid) = clicked_game
            && let Some(game) = games.iter().find(|g| g.game_id == gid)
        {
            let game = (*game).clone();
            let state = self.states.get_mut(&id).expect("state just used");
            state.select(&game);
        }
    }
}

impl ViewState {
    fn select(&mut self, game: &InFlightGame) {
        self.selected = Some(Selected {
            game_id: game.game_id,
            launch_seq: game.launch_seq,
            live: game.live.clone(),
        });
        self.finished_since = None;
        self.finish_watermark = None;
        self.replay = None;
        self.range = RANGE_STEPS[0];
    }

    /// Keep the selection valid: pick the first game when nothing is selected,
    /// drop selections whose game was discarded (force-stop), and auto-follow
    /// to the replacement game shortly after the watched one finishes.
    fn reconcile_selection(&mut self, games: &[&InFlightGame]) {
        let now = Instant::now();
        match &self.selected {
            None => {
                if let Some(first) = games.first() {
                    let first = (*first).clone();
                    self.select(&first);
                }
            }
            Some(sel) => {
                let finished = sel.live.lock().is_ok_and(|lg| lg.finished.is_some());
                let still_listed = games.iter().any(|g| g.game_id == sel.game_id);
                if !finished && !still_listed {
                    // Discarded mid-game (force-stop): nothing to freeze on.
                    self.selected = None;
                    if let Some(first) = games.first() {
                        let first = (*first).clone();
                        self.select(&first);
                    }
                    return;
                }
                if finished {
                    if self.finished_since.is_none() {
                        self.finished_since = Some(now);
                        // Watermark: whatever is already in flight at the
                        // moment the watched game ends is mid-game — the
                        // *replacement* is the first game launched after this
                        // point, and that one can be watched from move 1.
                        self.finish_watermark =
                            Some(games.iter().map(|g| g.launch_seq).max().unwrap_or(0));
                    }
                    let since = self.finished_since.unwrap_or(now);
                    if self.auto_follow && now.duration_since(since) >= FOLLOW_DELAY {
                        let watermark = self.finish_watermark.unwrap_or(sel.launch_seq);
                        let next = games
                            .iter()
                            .filter(|g| g.launch_seq > watermark)
                            .min_by_key(|g| g.launch_seq)
                            .map(|g| (*g).clone());
                        if let Some(next) = next {
                            self.select(&next);
                        }
                        // No fresh launch yet (e.g. the scheduler is between
                        // games, or the tournament is winding down): stay on
                        // the finished board rather than jumping into the
                        // middle of an older sibling game.
                    }
                } else {
                    self.finished_since = None;
                    self.finish_watermark = None;
                }
            }
        }
    }

    /// Incrementally replay new plies into the cached position.
    fn update_replay(&mut self, game_id: GameId, snap: &Snap) {
        let rebuild = self
            .replay
            .as_ref()
            .is_none_or(|r| r.game_id != game_id || r.applied > snap.uci.len());
        if rebuild {
            let standard =
                snap.start_fen.is_none() || snap.start_fen.as_deref().is_some_and(is_standard_fen);
            let pos = snap
                .start_fen
                .as_deref()
                .and_then(|f| f.parse::<Fen>().ok())
                .and_then(|f| f.into_position(CastlingMode::Standard).ok())
                .unwrap_or_default();
            self.replay = Some(Replay {
                game_id,
                applied: 0,
                pos,
                last_move: None,
                opening: None,
                standard_start: standard,
            });
        }
        let replay = self.replay.as_mut().expect("just ensured");
        while replay.applied < snap.uci.len() {
            let uci = &snap.uci[replay.applied];
            let Some(m) = uci
                .parse::<UciMove>()
                .ok()
                .and_then(|m| m.to_move(&replay.pos).ok())
            else {
                // Corrupt/desynced move list: freeze the board where it is.
                replay.applied = snap.uci.len();
                break;
            };
            replay.last_move = Some((m.from(), m.to()));
            replay.pos.play_unchecked(m);
            replay.applied += 1;
            if replay.standard_start {
                let key = replay.pos.zobrist_hash::<Zobrist64>(EnPassantMode::Legal);
                if let Some(op) = eco::lookup(key.0) {
                    replay.opening = Some(op);
                }
            }
        }
    }

    /// Grow ±R to the smallest snap step covering every centipawn eval seen.
    /// Monotonic within a game so the graph never squashes back; mate scores
    /// pin to the rail and do not drive the range.
    fn update_range(&mut self, snap: &Snap) {
        let mut max_abs: f32 = 0.0;
        for point in &snap.evals {
            if let Score::Cp(cp) = point.score {
                max_abs = max_abs.max((cp as f32 / 100.0).abs());
            }
        }
        for step in RANGE_STEPS {
            if step >= self.range && step >= max_abs {
                self.range = step;
                return;
            }
        }
        self.range = *RANGE_STEPS.last().expect("non-empty");
    }
}

fn is_standard_fen(fen: &str) -> bool {
    fen.trim()
        .starts_with("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq")
}

fn empty_state(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(ui.available_height() * 0.35);
        ui.label(
            RichText::new("No live games")
                .color(theme::text_weak())
                .font(theme::semibold(15.0)),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Games appear here the moment they start.")
                .color(theme::text_faint())
                .size(12.0),
        );
    });
}

/// The compact auto-follow toggle chip with its explanation on hover.
/// Always framed (never `selectable_label`, which gains frame padding on
/// hover and shifts the row).
fn auto_follow_chip(ui: &mut Ui, auto_follow: &mut bool) {
    let text = RichText::new("Auto-follow")
        .size(12.5)
        .color(if *auto_follow {
            theme::accent_bright()
        } else {
            theme::text_weak()
        });
    let mut button = egui::Button::new(text).corner_radius(egui::CornerRadius::same(4));
    if *auto_follow {
        button = button
            .fill(theme::tint(theme::accent(), 0.15))
            .stroke(Stroke::new(1.0, theme::tint(theme::accent(), 0.4)));
    }
    let resp = ui
        .add(button)
        .on_hover_text("When the watched game ends, switch to the game that replaces it.");
    if resp.clicked() {
        *auto_follow = !*auto_follow;
    }
}

// ── Games rail ──────────────────────────────────────────────────────────────

fn games_rail(ui: &mut Ui, games: &[&InFlightGame], selected: GameId) -> Option<GameId> {
    // The panel header already carries the "Playing (n)" caption.
    let mut clicked = None;
    ScrollArea::vertical()
        .id_salt("live_rail_scroll")
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for game in games {
                let (names, plies, result) = match game.live.lock() {
                    Ok(lg) => (
                        (lg.white_name.clone(), lg.black_name.clone()),
                        lg.san_moves.len(),
                        lg.finished,
                    ),
                    Err(_) => continue,
                };
                let is_sel = game.game_id == selected;
                let fill = if is_sel {
                    theme::tint(theme::accent(), 0.14)
                } else {
                    theme::bg_elevated()
                };
                let stroke = if is_sel {
                    Stroke::new(1.0, theme::accent())
                } else {
                    Stroke::new(1.0, theme::stroke())
                };
                let resp = egui::Frame::new()
                    .fill(fill)
                    .stroke(stroke)
                    .corner_radius(egui::CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(7, 5))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.add(
                            egui::Label::new(
                                RichText::new(&names.0).color(theme::text()).size(11.5),
                            )
                            .truncate()
                            .selectable(false),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(&names.1).color(theme::text_weak()).size(11.5),
                            )
                            .truncate()
                            .selectable(false),
                        );
                        let status = match result {
                            Some((r, _)) => format!("R{} · {}", game.round, result_label(r)),
                            None => format!("R{} · move {} ●", game.round, plies.div_ceil(2)),
                        };
                        ui.label(RichText::new(status).color(theme::text_faint()).size(10.5));
                    })
                    .response;
                let click = ui.interact(
                    resp.rect,
                    egui::Id::new("live_rail_game").with(game.game_id),
                    Sense::click(),
                );
                if click.clicked() {
                    clicked = Some(game.game_id);
                }
                ui.add_space(4.0);
            }
        });
    clicked
}

fn result_label(result: GameResult) -> &'static str {
    match result {
        GameResult::WhiteWin => "1–0",
        GameResult::BlackWin => "0–1",
        GameResult::Draw => "½–½",
    }
}

// ── Moves column ────────────────────────────────────────────────────────────

fn moves_column(ui: &mut Ui, snap: &Snap, replay: Option<&Replay>, height: f32) {
    egui::Frame::new()
        .fill(theme::bg_darkest())
        .stroke(Stroke::new(1.0, theme::stroke()))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.set_min_size(vec2(MOVES_W - 20.0, height - 20.0));
            ui.set_max_height(height - 20.0);

            // Matchup header: the players are the headline.
            ui.vertical_centered(|ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&snap.white_name)
                            .color(theme::text())
                            .font(theme::semibold(15.0)),
                    )
                    .truncate(),
                );
                ui.label(RichText::new("vs").color(theme::text_faint()).size(10.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(&snap.black_name)
                            .color(theme::text())
                            .font(theme::semibold(15.0)),
                    )
                    .truncate(),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("Round {}", snap.round))
                        .color(theme::text_faint())
                        .size(10.5),
                );
            });
            if let Some(op) = replay.and_then(|r| r.opening) {
                ui.add_space(6.0);
                // ECO chip on its own line, name below — both centred, so the
                // block reads as part of the centred matchup header above it.
                ui.vertical_centered(|ui| {
                    egui::Frame::new()
                        .stroke(Stroke::new(1.0, theme::stroke()))
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(5, 1))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(op.eco)
                                    .color(theme::text_weak())
                                    .size(10.5)
                                    .monospace(),
                            );
                        });
                    ui.add_space(2.0);
                    ui.label(RichText::new(op.name).color(theme::text_weak()).size(11.5));
                });
            }

            ui.separator();

            // Move list (auto-follows the tail), leaving room for material.
            let list_height = (ui.available_height() - 52.0).max(40.0);
            ScrollArea::vertical()
                .id_salt("live_moves_scroll")
                .max_height(list_height)
                .min_scrolled_height(list_height)
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    moves_grid(ui, &snap.san);
                });

            ui.separator();
            if let Some(replay) = replay {
                material_row(ui, replay.pos.board());
            }
        });
}

fn moves_grid(ui: &mut Ui, san: &[String]) {
    ui.spacing_mut().item_spacing.y = 2.0;
    let last = san.len().saturating_sub(1);
    for (row, pair) in san.chunks(2).enumerate() {
        ui.horizontal(|ui| {
            ui.add_sized(
                vec2(26.0, 14.0),
                egui::Label::new(
                    RichText::new(format!("{}.", row + 1))
                        .color(theme::text_faint())
                        .size(11.0)
                        .monospace(),
                ),
            );
            for (i, m) in pair.iter().enumerate() {
                let ply = row * 2 + i;
                let is_last = ply == last && !san.is_empty();
                let mut text = RichText::new(m).size(11.5).monospace().color(if is_last {
                    theme::accent()
                } else {
                    theme::text_weak()
                });
                if is_last {
                    text = text.strong();
                }
                ui.add_sized(vec2(60.0, 14.0), egui::Label::new(text));
            }
        });
    }
}

/// Lichess-style material imbalance: the leading side shows the material it
/// is up — as the opponent's captured pieces, drawn with the real board piece
/// set — plus the point difference.
fn material_row(ui: &mut Ui, board: &shakmaty::Board) {
    const ROLES: [(Role, i32); 5] = [
        (Role::Queen, 9),
        (Role::Rook, 5),
        (Role::Bishop, 3),
        (Role::Knight, 3),
        (Role::Pawn, 1),
    ];
    const PIECE: f32 = 22.0;
    // (owner is up, shown as the opponent's piece image).
    let mut white_up: Vec<Role> = Vec::new();
    let mut black_up: Vec<Role> = Vec::new();
    let mut points = 0i32;
    for (role, value) in ROLES {
        let w = i32::from(*board.material_side(ChessColor::White).get(role));
        let b = i32::from(*board.material_side(ChessColor::Black).get(role));
        let d = w - b;
        points += d * value;
        for _ in 0..d.abs() {
            if d > 0 {
                white_up.push(role);
            } else {
                black_up.push(role);
            }
        }
    }

    if white_up.is_empty() && black_up.is_empty() {
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("material even")
                    .color(theme::text_faint())
                    .size(12.0),
            );
        });
        return;
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        let pieces = (white_up.len() + black_up.len()) as f32;
        // Centre the run (pieces overlap slightly, like lichess).
        let run_w = pieces * (PIECE - 6.0) + 6.0 + 34.0;
        ui.add_space(((ui.available_width() - run_w) / 2.0).max(0.0));
        let draw = |ui: &mut Ui, roles: &[Role], color: ChessColor| {
            for &role in roles {
                let (rect, _) = ui.allocate_exact_size(vec2(PIECE - 6.0, PIECE), Sense::hover());
                let rect = Rect::from_min_size(rect.min, vec2(PIECE, PIECE));
                egui::Image::new(board::piece_source(color, role)).paint_at(ui, rect);
            }
        };
        // White is up: show black's captured pieces, and vice versa.
        draw(ui, &white_up, ChessColor::Black);
        draw(ui, &black_up, ChessColor::White);
        ui.add_space(8.0);
        if points != 0 {
            ui.label(
                RichText::new(format!("{points:+}"))
                    .color(theme::text())
                    .size(15.0)
                    .monospace(),
            );
        }
    });
}

// ── Board extras ────────────────────────────────────────────────────────────

fn result_banner(ui: &Ui, board_rect: Rect, result: GameResult, termination: Termination) {
    let text = format!(
        "{} · {}",
        result_label(result),
        termination_label(termination)
    );
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(text, theme::semibold(15.0), theme::text());
    let pad = vec2(14.0, 8.0);
    let size = galley.size() + pad * 2.0;
    let rect = Rect::from_center_size(
        pos2(
            board_rect.center().x,
            board_rect.top() + size.y / 2.0 + 12.0,
        ),
        size,
    );
    painter.rect(
        rect,
        egui::CornerRadius::same(8),
        theme::bg_darkest().gamma_multiply(0.92),
        Stroke::new(1.0, theme::accent()),
        egui::StrokeKind::Inside,
    );
    painter.galley(rect.min + pad, galley, theme::text());
}

fn termination_label(termination: Termination) -> &'static str {
    match termination {
        Termination::Checkmate => "Checkmate",
        Termination::Stalemate => "Stalemate",
        Termination::InsufficientMaterial => "Insufficient material",
        Termination::FiftyMove => "50-move rule",
        Termination::Threefold => "Threefold repetition",
        Termination::MaxMoves => "Move limit",
        Termination::TimeForfeit => "Time forfeit",
        Termination::EngineCrash => "Engine crash",
        Termination::IllegalMove => "Illegal move",
        Termination::AdjudicatedResign => "Adjudication",
        Termination::AdjudicatedDraw => "Adjudication draw",
        Termination::Aborted => "Aborted",
    }
}

// ── Engine column ───────────────────────────────────────────────────────────

fn engine_column(
    ui: &mut Ui,
    backend: &Backend,
    logos: &mut logo::LogoCache,
    state: &ViewState,
    snap: &Snap,
    width: f32,
    height: f32,
) {
    let thinking_white = snap.finished.is_none() && snap.white_to_move;
    let thinking_black = snap.finished.is_none() && !snap.white_to_move;
    let pondering_white = snap.finished.is_none() && !thinking_white && snap.white_pondering;
    let pondering_black = snap.finished.is_none() && !thinking_black && snap.black_pondering;

    // One card holds both engine panels and the graph, so the three read as a
    // single connected unit rather than three floating boxes. The inner content
    // spans the whole board height; equal top/bottom panels keep the graph —
    // and its 0-line — centred on the board's midline.
    let inner_w = width - 2.0 * CARD_PAD;
    let inner_h = height - 2.0 * CARD_PAD;
    // The engine panels are the stars: the graph is a shallow band (~quarter
    // of the height, panels have priority at the minimum), the panels split
    // the rest. Everything sums to exactly the inner height so nothing
    // overflows the card, and the graph's 0-line stays on the board midline.
    let graph_h = (inner_h * 0.24)
        .clamp(GRAPH_MIN, GRAPH_MAX_H)
        .min(inner_h - 2.0 * PANEL_MIN - 2.0 * DIVIDER_H)
        .max(GRAPH_MIN);
    let panel_h = ((inner_h - graph_h - 2.0 * DIVIDER_H) * 0.5).min(PANEL_MAX);
    let graph_h = inner_h - 2.0 * panel_h - 2.0 * DIVIDER_H;

    egui::Frame::new()
        .fill(theme::bg_darkest())
        .stroke(Stroke::new(1.0, theme::stroke()))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(CARD_PAD as i8))
        .show(ui, |ui| {
            ui.set_min_size(vec2(inner_w, inner_h));
            ui.set_max_size(vec2(inner_w, inner_h));
            ui.spacing_mut().item_spacing.y = 0.0;

            panel_slot(ui, inner_w, panel_h, false, |ui| {
                engine_panel(
                    ui,
                    backend,
                    logos,
                    &PanelData {
                        engine: snap.black_id,
                        name: &snap.black_name,
                        is_white: false,
                        search: &snap.black_search,
                        log: &snap.black_log,
                        clock_ms: snap.black_clock_ms,
                        thinking: thinking_black,
                        pondering: pondering_black,
                        search_elapsed: snap.search_elapsed,
                    },
                    inner_w,
                    panel_h,
                );
            });
            column_divider(ui, inner_w);
            let (graph_rect, _) = ui.allocate_exact_size(vec2(inner_w, graph_h), Sense::hover());
            draw_graph(ui, graph_rect, snap, state.range);
            column_divider(ui, inner_w);
            panel_slot(ui, inner_w, panel_h, true, |ui| {
                engine_panel(
                    ui,
                    backend,
                    logos,
                    &PanelData {
                        engine: snap.white_id,
                        name: &snap.white_name,
                        is_white: true,
                        search: &snap.white_search,
                        log: &snap.white_log,
                        clock_ms: snap.white_clock_ms,
                        thinking: thinking_white,
                        pondering: pondering_white,
                        search_elapsed: snap.search_elapsed,
                    },
                    inner_w,
                    panel_h,
                );
            });
        });
}

/// Allocate exactly `w`×`h` for one engine panel and build it in a clipped
/// child `Ui`, so the panel can never paint past its slot — the card (and
/// therefore the column) is always exactly board-height.
fn panel_slot(ui: &mut Ui, w: f32, h: f32, is_white: bool, body: impl FnOnce(&mut Ui)) {
    let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .id_salt(("engine_panel_slot", is_white))
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    body(&mut child);
}

/// A hairline rule between the card's sections (engine panel ↔ graph).
fn column_divider(ui: &mut Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, 9.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().line_segment(
        [pos2(rect.left() + 2.0, y), pos2(rect.right() - 2.0, y)],
        Stroke::new(1.0, theme::stroke()),
    );
}

struct PanelData<'a> {
    engine: EngineId,
    name: &'a str,
    is_white: bool,
    search: &'a LiveSearch,
    /// Rolling search log (one line per completed depth), oldest first.
    log: &'a [SearchLine],
    clock_ms: Option<u64>,
    thinking: bool,
    /// Searching on the opponent's time (`go ponder` active).
    pondering: bool,
    search_elapsed: Option<Duration>,
}

fn engine_panel(
    ui: &mut Ui,
    backend: &Backend,
    logos: &mut logo::LogoCache,
    data: &PanelData<'_>,
    width: f32,
    height: f32,
) {
    let series = if data.is_white {
        theme::graph_white()
    } else {
        theme::graph_black()
    };
    // No border — the surrounding card supplies that. The active engine gets a
    // faint accent wash so it's obvious at a glance whose clock is running.
    let fill = if data.thinking {
        theme::tint(theme::accent(), 0.10)
    } else {
        Color32::TRANSPARENT
    };
    // Header: four sections side by side — logo · name+eval · search stats ·
    // clock — with the divided search log below.
    const HEADER_H: f32 = 72.0;
    egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(8, 8))
        .show(ui, |ui| {
            ui.set_min_size(vec2(width - 16.0, height - 16.0));
            ui.set_max_size(vec2(width - 16.0, height - 16.0));
            ui.spacing_mut().item_spacing.y = 8.0;

            // The header is an exact rect: the logo is painted at the full
            // header height (its bottom lands on the divider below), and the
            // content beside it lives in a clipped child so nothing can ever
            // extend past the header block.
            let (header_rect, _) =
                ui.allocate_exact_size(vec2(width - 16.0, HEADER_H), Sense::hover());
            // 1 · Logo, exactly the header height.
            let logo_rect = Rect::from_min_size(header_rect.min, vec2(HEADER_H, HEADER_H));
            let logo_file = backend
                .engines
                .iter()
                .find(|e| e.id == data.engine)
                .and_then(|e| e.meta.extra.get("logo").cloned());
            let drew = logo_file.is_some_and(|file| {
                logo::draw_fitted(
                    ui,
                    logos,
                    &backend.dirs.logos_dir().join(file),
                    logo_rect,
                    6,
                )
            });
            if !drew {
                widgets::draw_avatar_square_in(ui, logo_rect, data.name, false, 6);
            }
            let content_rect = Rect::from_min_max(
                pos2(header_rect.left() + HEADER_H + 8.0, header_rect.top()),
                header_rect.max,
            );
            let mut header = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(content_rect)
                    .id_salt(("engine_header", data.is_white))
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            header.set_clip_rect(content_rect.intersect(ui.clip_rect()));
            {
                let ui = &mut header;
                // 4 · Clock, pinned right (laid out first so the name
                // truncates against it, but drawn at the right edge).
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    clock_chip(ui, data);
                    ui.add_space(8.0);

                    // 3 · Search stats stacked: depth / nodes / nps.
                    // Fixed width — a `with_layout` child would claim all
                    // remaining space and overlap the name/eval section.
                    ui.allocate_ui_with_layout(
                        vec2(96.0, HEADER_H),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_max_width(96.0);
                            // Three rows must fit the 72-pt header: keep
                            // them tight or the last one clips.
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.add_space(6.0);
                            let depth = match (data.search.depth, data.search.seldepth) {
                                (Some(d), Some(s)) => format!("{d}/{s}"),
                                (Some(d), None) => d.to_string(),
                                _ => "—".to_string(),
                            };
                            stat_row(ui, "depth", &depth);
                            stat_row(ui, "nodes", &format_count(data.search.nodes));
                            stat_row(ui, "nps", &format_count(data.search.nps));
                        },
                    );
                    ui.add_space(10.0);

                    // 2 · Name + version over the big eval, filling the rest.
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.horizontal(|ui| {
                            let (dot, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
                            ui.painter().circle_filled(dot.center(), 4.5, series);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(data.name)
                                        .color(theme::text())
                                        .font(theme::semibold(15.0)),
                                )
                                .truncate(),
                            );
                        });
                        ui.horizontal(|ui| {
                            let (text, color) = match data.search.score {
                                Some(score) => (
                                    format_score(score),
                                    if data.thinking {
                                        series
                                    } else {
                                        theme::text_weak()
                                    },
                                ),
                                None => ("—".to_string(), theme::text_faint()),
                            };
                            // Truncating label: a long eval ("−16.02")
                            // in a narrow card elides instead of
                            // painting over the stats section.
                            ui.add(
                                egui::Label::new(
                                    RichText::new(text)
                                        .color(color)
                                        .font(egui::FontId::new(24.0, egui::FontFamily::Monospace)),
                                )
                                .truncate(),
                            );
                            if data.thinking && ui.available_width() >= 56.0 {
                                ui.label(
                                    RichText::new("thinking…").color(theme::accent()).size(10.5),
                                );
                            } else if data.pondering && ui.available_width() >= 64.0 {
                                ui.label(
                                    RichText::new("pondering…")
                                        .color(theme::text_weak())
                                        .size(10.5),
                                );
                            }
                        });
                    });
                });
            }

            // The engine's search output: one line per completed depth of the
            // current search, newest on top, filling the remaining height.
            if ui.available_height() >= 30.0 {
                column_divider(ui, ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                ScrollArea::vertical()
                    .id_salt(("engine_log", data.engine, data.is_white))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for line in data.log.iter().rev() {
                            search_log_line(ui, line, series);
                        }
                    });
            }
        });
}

/// One small `label value` search-stat row (label faint, value mono). Fixed
/// height so three rows always fit the engine-card header.
fn stat_row(ui: &mut Ui, label: &str, value: &str) {
    ui.allocate_ui_with_layout(
        vec2(96.0, 17.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_sized(
                vec2(34.0, 12.0),
                egui::Label::new(RichText::new(label).color(theme::text_faint()).size(9.5)),
            );
            ui.label(
                RichText::new(value)
                    .color(theme::text())
                    .size(11.5)
                    .monospace(),
            );
        },
    );
}

/// One search-log line: `+0.35  d20/34  0:01  1.4M  <pv…>`, single line,
/// truncating with an ellipsis when the PV doesn't fit.
fn search_log_line(ui: &mut Ui, line: &SearchLine, series: Color32) {
    let score = line.score.map_or("—".to_string(), format_score);
    let depth = match line.seldepth {
        Some(s) => format!("d{}/{}", line.depth, s),
        None => format!("d{}", line.depth),
    };
    let time = format_clock(line.elapsed_ms);
    let nodes = format_count(line.nodes);
    let head = format!("{score:>7}  {depth:<7} {time:>5}  {nodes:>5}  ");
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &head,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(11.0, egui::FontFamily::Monospace),
            color: series.gamma_multiply(0.9),
            ..Default::default()
        },
    );
    job.append(
        &line.pv.join(" "),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(11.0, egui::FontFamily::Monospace),
            color: theme::text_weak(),
            ..Default::default()
        },
    );
    ui.add(egui::Label::new(job).truncate());
}

/// Clock (ticking down while thinking) for clock controls, or time spent on
/// the current move otherwise.
fn clock_chip(ui: &mut Ui, data: &PanelData<'_>) {
    let (text, active) = match data.clock_ms {
        Some(ms) => {
            let remaining = if data.thinking {
                ms.saturating_sub(data.search_elapsed.map_or(0, |e| e.as_millis() as u64))
            } else {
                ms
            };
            (format_clock(remaining), data.thinking)
        }
        None => match (data.thinking, data.search_elapsed) {
            (true, Some(elapsed)) => (format!("{:.1}s", elapsed.as_secs_f32()), true),
            _ => ("—".to_string(), false),
        },
    };
    let (fill, color) = if active {
        (theme::tint(theme::accent(), 0.2), theme::accent())
    } else {
        (Color32::TRANSPARENT, theme::text_weak())
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(Stroke::new(
            1.0,
            if active {
                theme::accent()
            } else {
                theme::stroke()
            },
        ))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(18.0).monospace());
        });
}

fn format_clock(ms: u64) -> String {
    let total_secs = ms / 1000;
    let (h, m, s) = (total_secs / 3600, (total_secs / 60) % 60, total_secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

fn format_score(score: Score) -> String {
    match score {
        Score::Cp(cp) => format!("{:+.2}", f64::from(cp) / 100.0),
        Score::Mate(m) if m >= 0 => format!("M{m}"),
        Score::Mate(m) => format!("−M{}", -m),
    }
}

fn format_count(value: Option<u64>) -> String {
    match value {
        None => "—".into(),
        Some(n) if n >= 1_000_000_000 => format!("{:.1}G", n as f64 / 1e9),
        Some(n) if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        Some(n) if n >= 1_000 => format!("{:.0}k", n as f64 / 1e3),
        Some(n) => n.to_string(),
    }
}

// ── Eval graph ──────────────────────────────────────────────────────────────

/// Raw engine evals (white POV, pawns) against ply, one series per engine.
/// Symmetric range ±R keeps eval 0 at the vertical centre — level with the
/// board midline. Mate scores pin to the rail as square markers.
fn draw_graph(ui: &Ui, rect: Rect, snap: &Snap, range: f32) {
    let painter = ui.painter();
    // A wide, shallow strip reads better than a tower: cap the height and
    // keep it centred, so eval 0 stays on the board midline regardless.
    let rect = Rect::from_center_size(
        rect.center(),
        vec2(rect.width(), rect.height().min(GRAPH_MAX_H)),
    );
    let label_w = 26.0;
    let plot = Rect::from_min_max(
        pos2(rect.left() + label_w, rect.top() + 4.0),
        pos2(rect.right() - 4.0, rect.bottom() - 4.0),
    );
    if plot.height() < 20.0 || plot.width() < 40.0 {
        return;
    }
    let mid_y = plot.center().y;
    let grid = |value: f32| -> f32 { mid_y - (value / range) * (plot.height() / 2.0) };
    let label_font = egui::FontId::proportional(9.0);

    for (value, strong) in [
        (0.0, true),
        (range, false),
        (-range, false),
        (range / 2.0, false),
        (-range / 2.0, false),
    ] {
        let y = grid(value);
        let color = if strong {
            theme::text_faint()
        } else {
            theme::stroke()
        };
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(if strong { 1.2 } else { 0.6 }, color),
        );
        let label = if value == 0.0 {
            "0".to_string()
        } else {
            format!("{value:+}")
        };
        painter.text(
            pos2(plot.left() - 4.0, y),
            Align2::RIGHT_CENTER,
            label,
            label_font.clone(),
            theme::text_faint(),
        );
    }

    if snap.evals.is_empty() {
        // No evals yet: just the empty grid — no placeholder text.
        return;
    }

    let max_ply = snap.evals.last().map_or(30, |p| p.ply).max(30) as f32;
    let x_of = |ply: u32| plot.left() + (ply as f32 / max_ply) * plot.width();
    let y_of = |score: Score| -> f32 {
        match score {
            Score::Cp(cp) => grid((cp as f32 / 100.0).clamp(-range, range)),
            Score::Mate(m) => grid(if m >= 0 { range } else { -range }),
        }
    };

    for is_white in [true, false] {
        let color = if is_white {
            theme::graph_white()
        } else {
            theme::graph_black()
        };
        let mut points: Vec<Pos2> = Vec::new();
        let mut mates: Vec<Pos2> = Vec::new();
        for point in snap.evals.iter().filter(|p| p.by_white == is_white) {
            let p = pos2(x_of(point.ply), y_of(point.score));
            points.push(p);
            if matches!(point.score, Score::Mate(_)) {
                mates.push(p);
            }
        }
        if points.len() >= 2 {
            painter.add(egui::Shape::line(points.clone(), Stroke::new(1.6, color)));
        }
        if let Some(last) = points.last() {
            painter.circle_filled(*last, 2.6, color);
        }
        for p in mates {
            painter.rect_filled(Rect::from_center_size(p, vec2(4.5, 4.5)), 0.0, color);
        }
    }
}
