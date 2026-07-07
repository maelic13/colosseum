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
    san::SanPlus, uci::UciMove,
};

use colosseum_core::{EngineId, GameId, GameResult, Termination, TournamentId};
use colosseum_engine::live::LiveSearch;
use colosseum_engine::{InFlightGame, LiveGameHandle, Score};

use crate::backend::Backend;
use crate::{board, eco, logo, theme, widgets};

const RAIL_W: f32 = 150.0;
const MOVES_W: f32 = 200.0;
/// The engine/graph column is elastic: at least this wide (so the panels stay
/// legible), but it soaks up whatever horizontal space the board leaves rather
/// than stranding it as dead margin — the wider it gets, the more room the eval
/// graph has to breathe.
const RIGHT_MIN: f32 = 300.0;
const RIGHT_MAX: f32 = 720.0;
const GAP: f32 = 10.0;
/// The eval graph is a wide, shallow strip: cap its painted height and keep
/// it centred so the 0-line stays on the board midline.
const GRAPH_MAX_H: f32 = 300.0;
/// Below this board size the body scrolls instead of shrinking further.
const MIN_BOARD: f32 = 320.0;
/// Hold the result banner this long before auto-following to the next game.
const FOLLOW_DELAY: Duration = Duration::from_secs(2);
/// Graph range snap steps (pawns).
const RANGE_STEPS: [f32; 5] = [1.0, 2.0, 3.0, 5.0, 10.0];

/// Per-tournament live-view states plus the shared logo cache.
#[derive(Default)]
pub struct LiveViews {
    states: HashMap<TournamentId, ViewState>,
    logos: logo::LogoCache,
}

struct ViewState {
    /// `true` = the Live lens is active (vs. Standings).
    watching: bool,
    auto_follow: bool,
    selected: Option<Selected>,
    /// When the selected game was first seen finished (auto-follow delay).
    finished_since: Option<Instant>,
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
    search_elapsed: Option<Duration>,
    white_search: LiveSearch,
    black_search: LiveSearch,
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
        search_elapsed: lg.search_started.map(|t| t.elapsed()),
        white_search: lg.white_search.clone(),
        black_search: lg.black_search.clone(),
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

    /// The `Standings | Live` lens switcher plus (when the rail is hidden)
    /// the auto-follow toggle. Laid out right-to-left in the control bar.
    pub fn header_controls(
        &mut self,
        ui: &mut Ui,
        id: TournamentId,
        in_flight: usize,
        concurrency: usize,
    ) {
        let state = self.state(id, concurrency);
        let live_label = if in_flight > 0 {
            format!("● Live ({in_flight})")
        } else {
            "Live".to_string()
        };
        // Right-to-left: rightmost chip first.
        widgets::choice_chip(ui, &mut state.watching, true, &live_label);
        widgets::choice_chip(ui, &mut state.watching, false, "Standings");
        if state.watching && in_flight < 2 {
            ui.add_space(8.0);
            auto_follow_chip(ui, &mut state.auto_follow);
        }
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

        let rail = games.len() >= 2;
        // Fixed columns to the left of the elastic engine column: the games
        // rail (only with 2+ games), the moves column, and the inter-column
        // gaps (rail↔moves↔board↔right).
        let gaps = GAP * if rail { 3.0 } else { 2.0 };
        let left_cols = MOVES_W + gaps + if rail { RAIL_W } else { 0.0 };
        let avail = ui.available_size();
        // The board is square and height-bound; give it what height allows but
        // never so much width that the engine column drops below its minimum.
        let board_side = (avail.y - 4.0)
            .min(avail.x - left_cols - RIGHT_MIN)
            .max(MIN_BOARD);
        // The engine column takes the remaining width (clamped), so a wide
        // window widens the graph instead of leaving a blank strip on the right.
        let right_w = (avail.x - left_cols - board_side).clamp(RIGHT_MIN, RIGHT_MAX);

        let mut clicked_game: Option<GameId> = None;
        ScrollArea::both()
            .id_salt("live_view_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    // The item spacing IS the column gap — explicit spacers on
                    // top of it would make the row wider than the computed
                    // board size and spill into scrollbars.
                    ui.spacing_mut().item_spacing.x = GAP;
                    // Columns get explicit top-down layouts: `allocate_ui`
                    // would inherit the surrounding left-to-right flow and
                    // spill their contents sideways.
                    let column = egui::Layout::top_down(egui::Align::Min);
                    if rail {
                        ui.allocate_ui_with_layout(vec2(RAIL_W, board_side), column, |ui| {
                            ui.set_min_width(RAIL_W);
                            ui.set_max_width(RAIL_W);
                            clicked_game =
                                games_rail(ui, &games, selected_id, &mut state.auto_follow);
                        });
                    }
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
            });

        if let Some(gid) = clicked_game
            && let Some(game) = games.iter().find(|g| g.game_id == gid)
        {
            let game = (*game).clone();
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
                    let since = *self.finished_since.get_or_insert(now);
                    if self.auto_follow && now.duration_since(since) >= FOLLOW_DELAY {
                        // The replacement: the earliest game launched after it.
                        let next = games
                            .iter()
                            .filter(|g| g.launch_seq > sel.launch_seq)
                            .min_by_key(|g| g.launch_seq)
                            .map(|g| (*g).clone());
                        if let Some(next) = next {
                            self.select(&next);
                        }
                    }
                } else {
                    self.finished_since = None;
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
            let standard = snap.start_fen.is_none()
                || snap.start_fen.as_deref().is_some_and(is_standard_fen);
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
            RichText::new("No games in flight")
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
fn auto_follow_chip(ui: &mut Ui, auto_follow: &mut bool) {
    let resp = ui
        .selectable_label(
            *auto_follow,
            RichText::new("Auto-follow").size(12.0).color(if *auto_follow {
                theme::accent()
            } else {
                theme::text_weak()
            }),
        )
        .on_hover_text("When the watched game ends, switch to the game that replaces it.");
    if resp.clicked() {
        *auto_follow = !*auto_follow;
    }
}

// ── Games rail ──────────────────────────────────────────────────────────────

fn games_rail(
    ui: &mut Ui,
    games: &[&InFlightGame],
    selected: GameId,
    auto_follow: &mut bool,
) -> Option<GameId> {
    let mut clicked = None;
    ui.label(
        RichText::new(format!("Playing ({})", games.len()))
            .color(theme::text())
            .font(theme::semibold(12.0)),
    );
    auto_follow_chip(ui, auto_follow);
    ui.add_space(4.0);
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

            // Matchup header.
            ui.vertical_centered(|ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(&snap.white_name)
                            .color(theme::text())
                            .font(theme::semibold(13.0)),
                    )
                    .truncate(),
                );
                ui.label(RichText::new("vs").color(theme::text_faint()).size(10.0));
                ui.add(
                    egui::Label::new(
                        RichText::new(&snap.black_name)
                            .color(theme::text())
                            .font(theme::semibold(13.0)),
                    )
                    .truncate(),
                );
                ui.label(
                    RichText::new(format!("Round {}", snap.round))
                        .color(theme::text_faint())
                        .size(10.0),
                );
            });
            if let Some(op) = replay.and_then(|r| r.opening) {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    egui::Frame::new()
                        .stroke(Stroke::new(1.0, theme::stroke()))
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(op.eco)
                                    .color(theme::text_weak())
                                    .size(10.5)
                                    .monospace(),
                            );
                        });
                    ui.label(RichText::new(op.name).color(theme::text_weak()).size(11.0));
                });
            }

            ui.separator();

            // Move list (auto-follows the tail), leaving room for material.
            let list_height = (ui.available_height() - 44.0).max(40.0);
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
/// is up (as the opponent's piece glyphs) plus the point difference.
fn material_row(ui: &mut Ui, board: &shakmaty::Board) {
    const ROLES: [(Role, i32, char, char); 5] = [
        (Role::Queen, 9, '♛', '♕'),
        (Role::Rook, 5, '♜', '♖'),
        (Role::Bishop, 3, '♝', '♗'),
        (Role::Knight, 3, '♞', '♘'),
        (Role::Pawn, 1, '♟', '♙'),
    ];
    let mut white_up = String::new();
    let mut black_up = String::new();
    let mut points = 0i32;
    for (role, value, black_glyph, white_glyph) in ROLES {
        let w = i32::from(*board.material_side(ChessColor::White).get(role));
        let b = i32::from(*board.material_side(ChessColor::Black).get(role));
        let d = w - b;
        points += d * value;
        for _ in 0..d.abs() {
            if d > 0 {
                white_up.push(black_glyph);
            } else {
                black_up.push(white_glyph);
            }
        }
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Material").color(theme::text_faint()).size(10.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if white_up.is_empty() && black_up.is_empty() {
                ui.label(RichText::new("even").color(theme::text_faint()).size(10.5));
                return;
            }
            if points != 0 {
                ui.label(
                    RichText::new(format!("{points:+}"))
                        .color(theme::text_weak())
                        .size(11.0)
                        .monospace(),
                );
            }
            if !black_up.is_empty() {
                ui.label(RichText::new(black_up).color(theme::text_weak()).size(13.0));
            }
            if !white_up.is_empty() {
                ui.label(RichText::new(white_up).color(theme::text()).size(13.0));
            }
        });
    });
}

// ── Board extras ────────────────────────────────────────────────────────────

fn result_banner(ui: &Ui, board_rect: Rect, result: GameResult, termination: Termination) {
    let text = format!("{} · {}", result_label(result), termination_label(termination));
    let painter = ui.painter();
    let galley = painter.layout_no_wrap(text, theme::semibold(15.0), theme::text());
    let pad = vec2(14.0, 8.0);
    let size = galley.size() + pad * 2.0;
    let rect = Rect::from_center_size(
        pos2(board_rect.center().x, board_rect.top() + size.y / 2.0 + 12.0),
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
    let pos = state.replay.as_ref().map(|r| &r.pos);

    // One card holds both engine panels and the graph, so the three read as a
    // single connected unit rather than three floating boxes. The inner content
    // spans the whole board height; equal top/bottom panels keep the graph —
    // and its 0-line — centred on the board's midline.
    const PAD: f32 = 12.0;
    const DIVIDER_H: f32 = 9.0;
    let inner_w = width - 2.0 * PAD;
    let inner_h = height - 2.0 * PAD;
    // Two equal panels, two dividers, and the graph sum to exactly the inner
    // height (so nothing overflows the card even at the minimum board size),
    // keeping the graph — hence its 0-line — on the board's midline.
    let panel_h = ((inner_h - 80.0) * 0.5).clamp(96.0, 230.0);
    let graph_h = (inner_h - 2.0 * panel_h - 2.0 * DIVIDER_H).max(60.0);

    egui::Frame::new()
        .fill(theme::bg_darkest())
        .stroke(Stroke::new(1.0, theme::stroke()))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::same(PAD as i8))
        .show(ui, |ui| {
            ui.set_min_size(vec2(inner_w, inner_h));
            ui.set_max_size(vec2(inner_w, inner_h));
            ui.spacing_mut().item_spacing.y = 0.0;

            engine_panel(
                ui,
                backend,
                logos,
                &PanelData {
                    engine: snap.black_id,
                    name: &snap.black_name,
                    is_white: false,
                    search: &snap.black_search,
                    clock_ms: snap.black_clock_ms,
                    thinking: thinking_black,
                    search_elapsed: snap.search_elapsed,
                    pos_hint: if thinking_black { pos } else { None },
                },
                inner_w,
                panel_h,
            );
            column_divider(ui, inner_w);
            let (graph_rect, _) = ui.allocate_exact_size(vec2(inner_w, graph_h), Sense::hover());
            draw_graph(ui, graph_rect, snap, state.range);
            column_divider(ui, inner_w);
            engine_panel(
                ui,
                backend,
                logos,
                &PanelData {
                    engine: snap.white_id,
                    name: &snap.white_name,
                    is_white: true,
                    search: &snap.white_search,
                    clock_ms: snap.white_clock_ms,
                    thinking: thinking_white,
                    search_elapsed: snap.search_elapsed,
                    pos_hint: if thinking_white { pos } else { None },
                },
                inner_w,
                panel_h,
            );
        });
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
    clock_ms: Option<u64>,
    thinking: bool,
    search_elapsed: Option<Duration>,
    /// Current position for PV → SAN conversion (thinking side only — the
    /// idle side's PV is from an earlier position and shows as raw UCI).
    pos_hint: Option<&'a Chess>,
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
    egui::Frame::new()
        .fill(fill)
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(6, 8))
        .show(ui, |ui| {
            ui.set_min_size(vec2(width - 12.0, height - 16.0));
            ui.set_max_size(vec2(width - 12.0, height - 16.0));
            ui.spacing_mut().item_spacing.y = 8.0;

            // Header: logo/avatar · series dot · name (with version) · clock.
            ui.horizontal(|ui| {
                let (rect, _) = logo::slot(ui, 24.0, Sense::hover());
                let logo_file = backend
                    .engines
                    .iter()
                    .find(|e| e.id == data.engine)
                    .and_then(|e| e.meta.extra.get("logo").cloned());
                let drew = logo_file.is_some_and(|file| {
                    logo::draw_fitted(ui, logos, &backend.dirs.logos_dir().join(file), rect, 4)
                });
                if !drew {
                    widgets::draw_avatar_square_in(ui, rect, data.name, false, 4);
                }
                let (dot, _) = ui.allocate_exact_size(vec2(9.0, 9.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 4.5, series);
                ui.add(
                    egui::Label::new(
                        RichText::new(data.name)
                            .color(theme::text())
                            .font(theme::semibold(13.5)),
                    )
                    .truncate(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    clock_chip(ui, data);
                });
            });

            // Eval + state.
            ui.horizontal(|ui| {
                let (text, color) = match data.search.score {
                    Some(score) => (
                        format_score(score),
                        if data.thinking { series } else { theme::text_weak() },
                    ),
                    None => ("—".to_string(), theme::text_faint()),
                };
                ui.label(
                    RichText::new(text)
                        .color(color)
                        .font(egui::FontId::new(22.0, egui::FontFamily::Monospace)),
                );
                if data.thinking {
                    ui.label(RichText::new("thinking…").color(theme::accent()).size(10.0));
                } else {
                    ui.label(
                        RichText::new("last search")
                            .color(theme::text_faint())
                            .size(10.0),
                    );
                }
            });

            // Depth / nodes / nps.
            ui.horizontal(|ui| {
                let depth = match (data.search.depth, data.search.seldepth) {
                    (Some(d), Some(s)) => format!("{d}/{s}"),
                    (Some(d), None) => d.to_string(),
                    _ => "—".to_string(),
                };
                ui.label(
                    RichText::new(format!("Depth {depth}"))
                        .color(theme::text_weak())
                        .size(11.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "{} · {} nps",
                            format_count(data.search.nodes),
                            format_count(data.search.nps)
                        ))
                        .color(theme::text_weak())
                        .size(11.0)
                        .monospace(),
                    );
                });
            });

            // Principal variation (SAN when convertible, else raw UCI).
            if !data.search.pv.is_empty() {
                let pv_text = data
                    .pos_hint
                    .and_then(|pos| pv_to_san(pos, &data.search.pv, 8))
                    .unwrap_or_else(|| {
                        data.search
                            .pv
                            .iter()
                            .take(8)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" ")
                    });
                ui.add(
                    egui::Label::new(
                        RichText::new(pv_text)
                            .color(theme::text_faint())
                            .size(10.0)
                            .monospace(),
                    )
                    .truncate(),
                );
            }
        });
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
            if active { theme::accent() } else { theme::stroke() },
        ))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(color).size(12.0).monospace());
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

/// Convert a UCI principal variation to SAN from `pos`; `None` if no move
/// applies (stale PV from an earlier position).
fn pv_to_san(pos: &Chess, pv: &[String], max: usize) -> Option<String> {
    let mut pos = pos.clone();
    let mut out: Vec<String> = Vec::new();
    for uci in pv.iter().take(max) {
        let Some(m) = uci
            .parse::<UciMove>()
            .ok()
            .and_then(|m| m.to_move(&pos).ok())
        else {
            break;
        };
        out.push(SanPlus::from_move(pos.clone(), m).to_string());
        pos.play_unchecked(m);
    }
    if out.is_empty() { None } else { Some(out.join(" ")) }
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
        painter.text(
            plot.center(),
            Align2::CENTER_CENTER,
            "eval · white POV",
            egui::FontId::proportional(9.5),
            theme::text_faint(),
        );
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
