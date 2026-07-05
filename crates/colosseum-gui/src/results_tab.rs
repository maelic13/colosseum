// SPDX-License-Identifier: GPL-3.0-or-later
//! Results tab: every tournament — the live one and stored history — in one
//! place. A tournament list sits on the right; selecting the active tournament
//! shows the live view (standings, head-to-head, games, transport controls),
//! selecting a stored one shows its reconstructed results with Resume/Delete/
//! Export actions.
//!
//! The live Elo column always shows exactly what the configured rating
//! writeback would store in the library: static library ratings ("Never"),
//! joint maximum-likelihood ratings ("All engines"), or a single performance
//! rating with everyone else anchored ("Estimate one engine").

use std::collections::HashMap;

use eframe::egui::{self, Color32, DragValue, Layout, RichText, ScrollArea, Ui};
use egui_extras::{Column, TableBuilder};

use colosseum_core::{
    EngineId, Format, PairGameResult, RatingWriteback, Standings, Termination, TimeControl,
    TournamentConfig, TournamentId, performance_rating,
};
use colosseum_engine::{
    EloEntry, GameRow, InFlightGame, TournamentResults, TournamentRow, TournamentStatus,
    store::STATUS_FINISHED,
};

use crate::backend::Backend;
use crate::theme;
use crate::widgets;

/// Per-engine display rating + delta against the library prior.
type DisplayRatings = HashMap<EngineId, (f64, f64)>;

/// Persistent state for the Results tab.
#[derive(Default)]
pub struct ResultsTab {
    /// Cached tournament list (most recent first); `None` until first load.
    list: Option<Vec<TournamentRow>>,
    /// Currently selected tournament.
    selected: Option<TournamentId>,
    /// Cached reconstructed results for the selected (stored) tournament.
    results: Option<(TournamentId, TournamentResults)>,
    /// Tournament awaiting delete confirmation (two-step inline confirm).
    pending_delete: Option<TournamentId>,
    /// Last error message from a DB action.
    error: Option<String>,
    /// Transient note shown after an export action.
    export_note: Option<String>,
    /// Cached games for the selected stored tournament (board viewer).
    games: Option<(TournamentId, Vec<GameRow>)>,
    /// The floating PGN/board viewer (shared by live + history views).
    viewer: crate::viewer::GameViewer,

    // ── Live-view state ──
    sort: SortState,
    show_h2h: bool,
    show_games: bool,
    /// Cached games of a loaded tournament: (id, finished count, rows).
    live_games_cache: Option<(TournamentId, usize, Vec<GameRow>)>,
    elo_note: Option<String>,
    /// Cached display ratings for the live table:
    /// (id, finished count when computed, engine → (rating, delta vs prior)).
    elo_cache: Option<(TournamentId, usize, DisplayRatings)>,
    /// Head-to-head cell format: `false` = W-D-L counts, `true` = per-game results.
    h2h_per_game: bool,
    /// Loaded-tournament ids last frame (a new one steals the selection).
    known_actives: Vec<TournamentId>,
    /// When the list was last re-read from the database (auto-refresh).
    last_refresh: Option<std::time::Instant>,
}

impl ResultsTab {
    /// Draw the tab body. Call every frame.
    pub fn show(&mut self, ui: &mut Ui, backend: &mut Backend) {
        // Auto-refresh: the list is a cheap query, so re-read it at most once
        // a second (statuses change as tournaments run) and immediately after
        // any action. No manual Refresh button needed.
        let stale = self
            .last_refresh
            .is_none_or(|t| t.elapsed().as_secs_f32() > 1.0);
        if self.list.is_none() || stale {
            self.refresh(backend);
        }

        // Follow loaded tournaments: a newly started or resumed one steals
        // the selection so its live view appears right away.
        let active_ids: Vec<TournamentId> = backend.actives.iter().map(|a| a.handle.id).collect();
        for id in &active_ids {
            if !self.known_actives.contains(id) {
                self.selected = Some(*id);
            }
        }
        self.known_actives = active_ids;

        // Right: tournament list.
        egui::Panel::right("results_list")
            .default_size(280.0)
            .size_range(220.0..=400.0)
            .resizable(true)
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 12,
                right: 8,
                top: 10,
                bottom: 8,
            }))
            .show_inside(ui, |ui| {
                self.list_panel(ui, backend);
            });

        // Centre: live view for loaded tournaments, stored results otherwise.
        let live_id = self.selected.filter(|id| backend.active(*id).is_some());
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| {
                if let Some(id) = live_id {
                    self.live_view(ui, backend, id);
                } else {
                    self.detail_panel(ui, backend);
                }
            });

        // The board viewer floats above everything.
        self.viewer.ui(ui.ctx());
    }

    /// Reload the tournament list from the database. Detail caches stay —
    /// they are keyed by tournament id and rebuilt only when it changes.
    fn refresh(&mut self, backend: &Backend) {
        let list = backend.list_tournaments();
        if let Some(sel) = self.selected
            && !list.iter().any(|t| t.id == sel)
        {
            self.selected = None;
        }
        if self.selected.is_none() {
            self.selected = list.first().map(|t| t.id);
        }
        self.list = Some(list);
        self.last_refresh = Some(std::time::Instant::now());
    }

    // ── Tournament list (right panel) ───────────────────────────────────────

    fn list_panel(&mut self, ui: &mut Ui, backend: &mut Backend) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Tournaments")
                    .color(theme::TEXT)
                    .font(theme::semibold(14.0)),
            );
            let count = self.list.as_ref().map_or(0, Vec::len);
            ui.label(
                RichText::new(count.to_string())
                    .color(theme::TEXT_FAINT)
                    .size(12.0),
            );
        });
        if let Some(err) = &self.error {
            ui.label(
                RichText::new(format!("⚠ {err}"))
                    .color(theme::DANGER)
                    .size(12.0),
            );
        }
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);

        let Some(list) = self.list.clone() else {
            return;
        };
        if list.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("♟").color(theme::TEXT_FAINT).size(40.0));
                ui.add_space(6.0);
                ui.label(
                    RichText::new("No tournaments yet")
                        .color(theme::TEXT_WEAK)
                        .font(theme::semibold(15.0)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Set one up in the Tournament tab.")
                        .color(theme::TEXT_FAINT)
                        .size(12.5),
                );
            });
            return;
        }

        ScrollArea::vertical()
            .id_salt("results_list_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in &list {
                    let selected = self.selected == Some(row.id);
                    // Live status + progress for loaded tournaments.
                    let live = backend.active(row.id).and_then(|a| {
                        a.snapshot
                            .lock()
                            .ok()
                            .map(|s| (s.status, s.games_finished, s.games_total))
                    });
                    if self.list_row(ui, row, selected, live) {
                        self.selected = Some(row.id);
                        self.pending_delete = None;
                        // Unfinished tournaments load immediately — same
                        // state as pressing Resume (stopped, ready to Go).
                        if row.status != STATUS_FINISHED
                            && backend.active(row.id).is_none()
                            && let Err(e) = backend.try_resume(row.clone())
                        {
                            self.error = Some(format!("Could not load: {e}"));
                        }
                    }
                    ui.add_space(4.0);
                }
            });
    }

    /// One selectable tournament card; returns true when clicked.
    fn list_row(
        &self,
        ui: &mut Ui,
        row: &TournamentRow,
        selected: bool,
        live: Option<(TournamentStatus, usize, usize)>,
    ) -> bool {
        let (fill, stroke) = if selected {
            (
                theme::tint(theme::ACCENT, 0.12),
                egui::Stroke::new(1.0, theme::tint(theme::ACCENT, 0.4)),
            )
        } else {
            (Color32::TRANSPARENT, egui::Stroke::NONE)
        };

        let bg_slot = ui.painter().add(egui::Shape::Noop);
        let resp = egui::Frame::new()
            .fill(fill)
            .stroke(stroke)
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&row.name)
                            .color(if selected {
                                theme::ACCENT_BRIGHT
                            } else {
                                theme::TEXT
                            })
                            .font(theme::semibold(13.5)),
                    );
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        // Live = games in flight; Finished = all played;
                        // Stopped = loaded or stored with games remaining.
                        let (label, color) = match live {
                            Some((status, ..)) => match status {
                                TournamentStatus::Running | TournamentStatus::Stopping => {
                                    ("● Live", theme::SUCCESS)
                                }
                                TournamentStatus::Finished => ("Finished", theme::ACCENT),
                                TournamentStatus::Stopped | TournamentStatus::Idle => {
                                    ("Stopped", theme::WARN)
                                }
                            },
                            None => status_parts(&row.status),
                        };
                        ui.label(
                            RichText::new(label)
                                .color(color)
                                .font(theme::semibold(11.0)),
                        );
                    });
                });
                // Second line: progress for loaded tournaments, date otherwise.
                match live {
                    Some((_, finished, total)) if total > 0 => {
                        ui.label(
                            RichText::new(format!("{finished} / {total} games"))
                                .color(theme::TEXT_WEAK)
                                .size(11.5),
                        );
                    }
                    _ => {
                        ui.label(
                            RichText::new(format_timestamp(&row.created_at))
                                .color(theme::TEXT_WEAK)
                                .size(11.5),
                        );
                    }
                }
            })
            .response;

        let interact = ui.interact(
            resp.rect,
            egui::Id::new("results_row").with(row.id),
            egui::Sense::click(),
        );
        if interact.hovered() && !selected {
            ui.painter().set(
                bg_slot,
                egui::Shape::rect_filled(resp.rect, egui::CornerRadius::same(6), theme::BG_HOVER),
            );
        }
        interact.clicked()
    }

    // ── Live view (active tournament) ───────────────────────────────────────

    fn live_view(&mut self, ui: &mut Ui, backend: &mut Backend, id: TournamentId) {
        let live = self.capture_live(backend, id);

        // Refresh the cached game list when expanded and a game has finished.
        if self.show_games {
            let stale = self
                .live_games_cache
                .as_ref()
                .map(|(cid, n, _)| (*cid, *n))
                != Some((id, live.finished));
            if stale {
                self.live_games_cache = Some((id, live.finished, backend.list_games(id)));
            }
        }

        // Control bar (top).
        egui::Panel::top("results_live_controls")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DARKEST)
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show_inside(ui, |ui| {
                self.live_control_bar(ui, backend, id, &live);
            });

        // Errors panel (bottom), only when there are errors.
        if !live.errors.is_empty() {
            egui::Panel::bottom("results_live_errors")
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 8)))
                .show_inside(ui, |ui| {
                    egui::Frame::new()
                        .fill(theme::tint(theme::DANGER, 0.08))
                        .stroke(egui::Stroke::new(1.0, theme::tint(theme::DANGER, 0.35)))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                RichText::new(format!("Engine errors ({})", live.errors.len()))
                                    .color(theme::DANGER)
                                    .font(theme::semibold(12.5)),
                            );
                            ScrollArea::vertical()
                                .id_salt("results_errors_scroll")
                                .max_height(80.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    for err in live.errors.iter().rev().take(20) {
                                        ui.label(
                                            RichText::new(err).color(theme::TEXT_WEAK).size(12.0),
                                        );
                                    }
                                });
                        });
                });
        }

        // Side panel: in-flight games + termination breakdown.
        if !live.in_flight_games.is_empty() || !live.termination_counts.is_empty() {
            egui::Panel::right("results_live_side")
                .default_size(210.0)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(theme::BG_DARKEST)
                        .stroke(egui::Stroke::new(1.0, theme::STROKE))
                        .inner_margin(egui::Margin::same(10)),
                )
                .show_inside(ui, |ui| {
                    live_side_panel(ui, &live);
                });
        }

        // Results table (centre) with breathing room on both sides.
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 14,
                right: 14,
                top: 10,
                bottom: 4,
            }))
            .show_inside(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("results_live_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.results_table(ui, &live);
                        if live.rows.len() == 2 {
                            ui.add_space(14.0);
                            crate::stats_ui::match_stats_card(
                                ui,
                                &live.crosstable_order(),
                                &live.standings,
                            );
                        }
                        if self.show_h2h {
                            ui.add_space(18.0);
                            self.head_to_head_section(ui, &live);
                        }
                        if self.show_games {
                            ui.add_space(18.0);
                            self.live_games_section(ui, &live);
                        }
                        ui.add_space(8.0);
                    });
            });
    }

    /// Snapshot the backend state into owned data, releasing the lock quickly,
    /// and join in the display ratings for the configured writeback mode.
    fn capture_live(&mut self, backend: &Backend, id: TournamentId) -> LiveData {
        let active = backend
            .active(id)
            .expect("live view without a loaded tournament");

        let snap = active.snapshot.lock().unwrap();
        let status = snap.status;
        let standings = snap.standings.clone();
        let elo_incremental = snap.elo.clone();
        let finished = snap.games_finished;
        let total = snap.games_total;
        let errors = snap.recent_errors.clone();
        let started_at = snap.started_at;
        let total_game_ms = snap.total_game_ms;
        let games_timed = snap.games_timed;
        let in_flight_games = snap.in_flight_games.clone();
        let termination_counts = snap.termination_counts.clone();
        let concurrency = snap.concurrency;
        drop(snap);

        // Display ratings = exactly what the writeback mode would store.
        let ratings = self.display_ratings(backend, id, &standings, finished, &elo_incremental);

        let ranked = standings.ranked_by_points();
        let rank_of = |id: EngineId| ranked.iter().position(|x| x == &id).map_or(0, |p| p + 1);

        let mut rows: Vec<Row> = active
            .participants
            .iter()
            .map(|p| {
                let st = standings.standing(p.id);
                let (elo, elo_delta) = ratings.get(&p.id).copied().unwrap_or((1500.0, 0.0));
                Row {
                    id: p.id,
                    rank: rank_of(p.id),
                    name: p.name.clone(),
                    version: p.version.clone(),
                    elo,
                    elo_delta,
                    points: st.points(),
                    games: st.games(),
                    wins: st.wins,
                    draws: st.draws,
                    losses: st.losses,
                    time_losses: st.time_losses,
                    crash_losses: st.crash_losses,
                    nps: st.avg_nps(),
                }
            })
            .collect();
        rows.sort_by_key(|r| r.rank);

        // Gauntlet: the seed engines are the first N participants.
        let gauntlet_seeds: Vec<EngineId> = match active.config.format {
            Format::Gauntlet { seeds, .. } => active
                .participants
                .iter()
                .take(seeds.max(1) as usize)
                .map(|p| p.id)
                .collect(),
            Format::RoundRobin { .. } => Vec::new(),
        };

        LiveData {
            name: active.name.clone(),
            status,
            finished,
            total,
            rows,
            standings,
            errors,
            started_at,
            total_game_ms,
            games_timed,
            in_flight_games,
            termination_counts,
            concurrency,
            gauntlet_seeds,
            per_game_estimate: estimate_game_secs(&active.config.time_control),
            writeback: active.rating_writeback,
        }
    }

    /// Ratings for the live Elo column, cached per finished-game count.
    ///
    /// The invariant that matters: these are *exactly* the numbers the
    /// configured writeback stores in the library, so the table is never
    /// misleading about what the engines will end up with.
    fn display_ratings(
        &mut self,
        backend: &Backend,
        id: TournamentId,
        standings: &Standings,
        finished: usize,
        incremental: &HashMap<EngineId, EloEntry>,
    ) -> DisplayRatings {
        if let Some((cid, n, map)) = &self.elo_cache
            && *cid == id
            && *n == finished
        {
            return map.clone();
        }
        let priors = backend.active_priors(id);
        let writeback = backend
            .active(id)
            .map_or(RatingWriteback::None, |a| a.rating_writeback);

        let map: DisplayRatings = match writeback {
            // Nothing will be written: show library ratings, no drift. The
            // incremental model still feeds the Δ column so the run is
            // informative — but only there, clearly marked as tournament-only.
            RatingWriteback::None => priors
                .iter()
                .map(|&(id, prior)| {
                    let delta = incremental.get(&id).map_or(0.0, |e| e.delta);
                    (id, (prior, delta))
                })
                .collect(),
            RatingWriteback::All => {
                let ml = colosseum_core::ml_ratings(standings, &priors);
                priors
                    .iter()
                    .map(|&(id, prior)| {
                        let r = ml.get(&id).copied().unwrap_or(prior);
                        (id, (r, r - prior))
                    })
                    .collect()
            }
            RatingWriteback::Estimate(target) => priors
                .iter()
                .map(|&(id, prior)| {
                    if id == target {
                        let results: Vec<(f64, f64, u32)> = priors
                            .iter()
                            .filter(|(opp, _)| *opp != target)
                            .map(|&(opp, opp_prior)| {
                                let h2h = standings.head_to_head(target, opp);
                                (opp_prior, h2h.points(), h2h.games())
                            })
                            .collect();
                        let perf = performance_rating(&results).unwrap_or(prior);
                        (id, (perf, perf - prior))
                    } else {
                        // Anchored: never moves, not even in the live view.
                        (id, (prior, 0.0))
                    }
                })
                .collect(),
        };
        self.elo_cache = Some((id, finished, map.clone()));
        map
    }

    fn live_control_bar(
        &mut self,
        ui: &mut Ui,
        backend: &mut Backend,
        id: TournamentId,
        live: &LiveData,
    ) {
        let status = live.status;

        // ── Row 1: status + name + transport + progress + timing ──
        ui.horizontal_wrapped(|ui| {
            let (label, dot, color) = status_pill_parts(status);
            widgets::status_pill(ui, label, dot, color);
            ui.add_space(6.0);
            ui.label(
                RichText::new(&live.name)
                    .color(theme::TEXT)
                    .font(theme::semibold(15.0)),
            );
            ui.add_space(10.0);

            let go_enabled = matches!(status, TournamentStatus::Stopped | TournamentStatus::Idle);
            let stop_enabled = matches!(status, TournamentStatus::Running);
            let force_enabled = matches!(
                status,
                TournamentStatus::Running | TournamentStatus::Stopping
            );

            if widgets::tinted_button(ui, "Go", theme::SUCCESS, go_enabled)
                .on_hover_text("Resume the tournament.")
                .clicked()
                && let Some(active) = backend.active(id)
            {
                active.handle.go();
            }
            if widgets::tinted_button(ui, "Stop", theme::WARN, stop_enabled)
                .on_hover_text("Stop launching new games; let in-flight games finish.")
                .clicked()
                && let Some(active) = backend.active(id)
            {
                active.handle.stop();
            }
            if widgets::tinted_button(ui, "Force-Stop", theme::DANGER, force_enabled)
                .on_hover_text("Abort in-flight games immediately (discarding them).")
                .clicked()
                && let Some(active) = backend.active(id)
            {
                active.handle.force_stop();
            }

            ui.add_space(12.0);

            // Parallel-games limit, adjustable while running or stopped:
            // in-flight games always finish, only the launch rate changes.
            ui.label(RichText::new("Parallel").color(theme::TEXT_WEAK).size(13.0));
            let mut lanes = live.concurrency as u32;
            if ui
                .add(DragValue::new(&mut lanes).range(1..=256).speed(0.1))
                .on_hover_text(
                    "How many games run at once. Lowering it never cancels \
                     running games; new games just wait for a free slot.",
                )
                .changed()
            {
                backend.set_active_concurrency(id, lanes as usize);
            }

            ui.add_space(12.0);

            // Progress: caption + slim bar.
            ui.label(
                RichText::new(format!("{} / {} games", live.finished, live.total))
                    .color(theme::TEXT_WEAK)
                    .size(13.0),
            );
            let frac = if live.total == 0 {
                0.0
            } else {
                live.finished as f32 / live.total as f32
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(150.0)
                    .desired_height(6.0)
                    .corner_radius(4.0)
                    .fill(theme::ACCENT),
            );

            // Timing: elapsed / avg / ETA (measured average once available,
            // configured time control as the estimate before that, both
            // divided by the parallel-games limit).
            if let Some(started) = live.started_at {
                let elapsed_secs = started.elapsed().as_secs_f64();
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("⏱ {}", format_duration(elapsed_secs)))
                        .color(theme::TEXT_WEAK)
                        .size(12.5),
                );
                let avg_secs = if live.games_timed > 0 {
                    Some(live.total_game_ms as f64 / live.games_timed as f64 / 1000.0)
                } else {
                    live.per_game_estimate
                };
                if let Some(avg) = avg_secs {
                    if live.games_timed > 0 {
                        ui.label(
                            RichText::new(format!("avg {}/game", format_duration(avg)))
                                .color(theme::TEXT_WEAK)
                                .size(12.5),
                        );
                    }
                    let remaining = live.total.saturating_sub(live.finished);
                    if remaining > 0 && matches!(status, TournamentStatus::Running) {
                        let lanes = live.concurrency.max(1) as f64;
                        let eta = remaining as f64 * avg / lanes * 1.05;
                        ui.label(
                            RichText::new(format!("ETA ~{}", format_duration(eta)))
                                .color(theme::TEXT_FAINT)
                                .size(12.0),
                        )
                        .on_hover_text(format!(
                            "{remaining} games left, {} in parallel, {} per game \
                             ({}).",
                            live.concurrency,
                            format_duration(avg),
                            if live.games_timed > 0 {
                                "measured average"
                            } else {
                                "estimated from the time control"
                            }
                        ));
                    }
                }
            }
        });

        ui.add_space(8.0);

        // ── Row 2: view toggles + actions ──
        ui.horizontal_wrapped(|ui| {
            let new_enabled = !crate::backend::is_busy(status);

            ui.toggle_value(&mut self.show_games, RichText::new("Games").size(13.0))
                .on_hover_text("Browse finished games and open them in the board viewer.");
            ui.toggle_value(
                &mut self.show_h2h,
                RichText::new("Head-to-head").size(13.0),
            );

            // Export menu: CSV standings / crosstable / game PGN.
            let export_resp = ui.menu_button(RichText::new("Export    ").size(13.0), |ui| {
                ui.set_min_width(170.0);
                if ui.button("Standings (CSV)").clicked() {
                    self.export_note =
                        crate::export_ui::export_standings_csv(&live.name, &live.export_rows());
                    ui.close();
                }
                if ui.button("Crosstable (CSV)").clicked() {
                    self.export_note = crate::export_ui::export_crosstable_csv(
                        &live.name,
                        &live.crosstable_order(),
                        &live.standings,
                    );
                    ui.close();
                }
                if ui.button("Game PGN").clicked() {
                    let pgn = backend.collect_pgn(id).unwrap_or_default();
                    self.export_note = crate::export_ui::export_pgn(&live.name, &pgn);
                    ui.close();
                }
            });
            widgets::dropdown_arrow(ui, export_resp.response.rect);

            ui.add_space(16.0);

            // Manual writeback — label matches the configured mode so it never
            // writes something different from what the table shows.
            match live.writeback {
                RatingWriteback::Estimate(target) => {
                    let target_name = live.participant_name(target).to_string();
                    if widgets::tinted_button(
                        ui,
                        &format!("Apply {target_name} estimate → Library"),
                        theme::ACCENT,
                        new_enabled,
                    )
                    .on_hover_text(
                        "Write the estimated rating to the library now. The \
                         other engines' ratings stay untouched.",
                    )
                    .clicked()
                    {
                        match backend.apply_estimate_to_library(id, target) {
                            Some(elo) => {
                                self.elo_cache = None;
                                self.elo_note =
                                    Some(format!("{target_name} rated {elo} in the library"));
                            }
                            None => {
                                self.elo_note = Some("No games to estimate from yet.".to_string());
                            }
                        }
                    }
                }
                _ => {
                    if widgets::tinted_button(ui, "Apply Elo → Library", theme::ACCENT, new_enabled)
                        .on_hover_text(
                            "Write the maximum-likelihood ratings shown in the Elo \
                             column to the engine library.",
                        )
                        .clicked()
                    {
                        let n = backend.apply_active_elo_to_library(id);
                        self.elo_cache = None;
                        self.elo_note = Some(format!("Elo applied ({n} engines updated)"));
                    }
                }
            }

            if ui
                .add_enabled(
                    new_enabled,
                    egui::Button::new(RichText::new("Close").size(13.0))
                        .fill(theme::BG_ELEVATED)
                        .stroke(egui::Stroke::new(1.0, theme::STROKE)),
                )
                .on_hover_text(if new_enabled {
                    "Unload this tournament (its results stay in the list; \
                     resume any time by clicking it)."
                } else {
                    "Stop the tournament first."
                })
                .clicked()
            {
                backend.close_tournament(id);
                self.live_games_cache = None;
                self.elo_cache = None;
                self.refresh(backend);
            }

            // Delete — available even while running (force-stops first), for
            // when a run is a write-off rather than something to finish.
            ui.add_space(4.0);
            if self.pending_delete == Some(id) {
                if widgets::tinted_button(ui, "Confirm delete", theme::DANGER, true)
                    .on_hover_text(
                        "Stop this tournament and permanently remove it and its games.",
                    )
                    .clicked()
                {
                    if let Some(active) = backend.active(id) {
                        active.handle.force_stop();
                    }
                    backend.close_tournament(id);
                    match backend.delete_tournament(id) {
                        Ok(()) => self.error = None,
                        Err(e) => self.error = Some(format!("Delete failed: {e}")),
                    }
                    self.pending_delete = None;
                    self.live_games_cache = None;
                    self.elo_cache = None;
                    self.refresh(backend);
                }
                ui.add_space(2.0);
                if ui
                    .button(RichText::new("Cancel").color(theme::TEXT))
                    .clicked()
                {
                    self.pending_delete = None;
                }
            } else if widgets::tinted_button(ui, "Delete", theme::DANGER, true)
                .on_hover_text("Delete this tournament — stops it first if it's running.")
                .clicked()
            {
                self.pending_delete = Some(id);
            }

            if let Some(note) = &self.export_note {
                ui.label(RichText::new(note).color(theme::TEXT_WEAK).size(12.0));
            }
            if let Some(note) = &self.elo_note {
                ui.label(RichText::new(note).color(theme::SUCCESS).size(12.0));
            }
        });
    }

    fn results_table(&mut self, ui: &mut Ui, live: &LiveData) {
        if live.rows.is_empty() {
            ui.add_space(20.0);
            ui.label(
                RichText::new("Waiting for the first game to finish…")
                    .color(theme::TEXT_WEAK)
                    .size(13.0),
            );
            return;
        }

        let mut rows = live.rows.clone();
        sort_rows(&mut rows, self.sort);

        let header_h = 30.0;
        let row_h = 30.0;

        TableBuilder::new(ui)
            .striped(true)
            .cell_layout(Layout::left_to_right(egui::Align::Center))
            // Fixed widths (not Column::auto): auto re-measures cell content every
            // frame, so live-updating numbers make the columns visibly jump.
            .column(Column::exact(40.0)) // rank
            .column(Column::initial(210.0).at_least(130.0).clip(true)) // engine (name + version)
            .column(Column::exact(60.0)) // elo
            .column(Column::exact(76.0)) // elo delta chip
            .column(Column::exact(60.0)) // points
            .column(Column::exact(52.0)) // games
            .column(Column::exact(92.0)) // w-d-l
            .column(Column::exact(110.0)) // forfeits (time / crash losses)
            .column(Column::remainder().at_least(80.0)) // nps
            .header(header_h, |mut header| {
                header.col(|ui| {
                    ui.label(strong_header("#"));
                });
                header.col(|ui| {
                    sortable_header(ui, "Engine", SortKey::Name, &mut self.sort);
                });
                header.col(|ui| {
                    sortable_header(ui, "Elo", SortKey::Elo, &mut self.sort);
                });
                header.col(|ui| {
                    sortable_header(ui, "Δ", SortKey::EloDelta, &mut self.sort);
                });
                header.col(|ui| {
                    sortable_header(ui, "Pts", SortKey::Points, &mut self.sort);
                });
                header.col(|ui| {
                    sortable_header(ui, "Gms", SortKey::Games, &mut self.sort);
                });
                header.col(|ui| {
                    ui.label(strong_header("W-D-L"));
                });
                header.col(|ui| {
                    ui.label(strong_header("Forfeits")).on_hover_text(
                        "Losses on time and by crash / illegal move.",
                    );
                });
                header.col(|ui| {
                    sortable_header(ui, "Avg nps", SortKey::Nps, &mut self.sort);
                });
            })
            .body(|mut body| {
                for row in &rows {
                    body.row(row_h, |mut tr| {
                        tr.col(|ui| {
                            widgets::rank_badge(ui, row.rank);
                        });
                        tr.col(|ui| {
                            engine_name_label(ui, &row.name, &row.version);
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(format!("{:.0}", row.elo))
                                    .color(theme::TEXT)
                                    .monospace(),
                            );
                        });
                        tr.col(|ui| {
                            widgets::elo_delta_chip(ui, row.elo_delta);
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(format!("{:.1}", row.points))
                                    .color(theme::ACCENT)
                                    .monospace()
                                    .strong(),
                            );
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(row.games.to_string())
                                    .color(theme::TEXT_WEAK)
                                    .monospace(),
                            );
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{}-{}-{}",
                                    row.wins, row.draws, row.losses
                                ))
                                .color(theme::TEXT)
                                .monospace(),
                            );
                        });
                        tr.col(|ui| {
                            forfeit_cell(ui, row.time_losses, row.crash_losses);
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(format_nps(row.nps))
                                    .color(theme::TEXT_WEAK)
                                    .monospace(),
                            );
                        });
                    });
                }
            });
    }

    // ── Head-to-head ────────────────────────────────────────────────────────

    fn head_to_head_section(&mut self, ui: &mut Ui, live: &LiveData) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Head-to-head")
                    .color(theme::TEXT)
                    .font(theme::semibold(14.0)),
            );
            ui.add_space(10.0);
            widgets::choice_chip(ui, &mut self.h2h_per_game, false, "W-D-L");
            widgets::choice_chip(ui, &mut self.h2h_per_game, true, "Results")
                .on_hover_text("Individual game results in played order (1 0 ½).");
        });
        ui.add_space(6.0);

        // Gauntlet with a single seed gets a focused per-opponent layout;
        // everything else gets the full crosstable.
        if let [seed] = live.gauntlet_seeds.as_slice() {
            self.gauntlet_h2h(ui, live, *seed);
        } else {
            self.h2h_matrix(ui, live);
        }
    }

    fn gauntlet_h2h(&self, ui: &mut Ui, live: &LiveData, seed: EngineId) {
        let Some(seed_row) = live.rows.iter().find(|r| r.id == seed) else {
            return;
        };
        // Seed header: name + total score.
        ui.horizontal(|ui| {
            engine_name_label_full(ui, &seed_row.name, &seed_row.version);
            ui.add_space(8.0);
            ui.label(
                RichText::new(format!("{:.1} / {}", seed_row.points, seed_row.games))
                    .color(theme::ACCENT)
                    .font(theme::semibold(13.0)),
            );
            ui.label(
                RichText::new("vs each opponent:")
                    .color(theme::TEXT_FAINT)
                    .size(12.0),
            );
        });
        ui.add_space(6.0);

        let mut opponents: Vec<&Row> = live.rows.iter().filter(|r| r.id != seed).collect();
        opponents.sort_by_key(|r| r.rank);

        egui::Grid::new("gauntlet_h2h")
            .striped(true)
            .spacing([14.0, 6.0])
            .show(ui, |ui| {
                for opp in opponents {
                    let h2h = live.standings.head_to_head(seed, opp.id);
                    engine_name_label_full(ui, &opp.name, &opp.version);
                    if h2h.games() == 0 {
                        ui.label(RichText::new("·").color(theme::TEXT_WEAK));
                        ui.label("");
                    } else {
                        ui.label(
                            RichText::new(format!("{:.1} / {}", h2h.points(), h2h.games()))
                                .color(theme::TEXT)
                                .monospace()
                                .size(12.5),
                        );
                        let share = (h2h.points() / f64::from(h2h.games())) as f32;
                        h2h_record_cell(
                            ui,
                            share,
                            &self.h2h_cell_text(live, seed, opp.id),
                        );
                    }
                    ui.end_row();
                }
            });
    }

    fn h2h_matrix(&self, ui: &mut Ui, live: &LiveData) {
        ui.label(
            RichText::new("Row engine's record against each column engine.")
                .color(theme::TEXT_WEAK)
                .size(11.5),
        );
        ui.add_space(6.0);

        let order: Vec<&Row> = {
            let mut r: Vec<&Row> = live.rows.iter().collect();
            r.sort_by_key(|row| row.rank);
            r
        };

        ScrollArea::horizontal()
            .id_salt("results_h2h_scroll")
            // Shrink vertically: a horizontal scroll area that claims all
            // remaining height leaves a huge dead gap under the matrix.
            .auto_shrink([false, true])
            .show(ui, |ui| {
                egui::Grid::new("results_h2h_matrix")
                    .striped(true)
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("");
                        for col in &order {
                            ui.label(
                                RichText::new(short_name(&col.name))
                                    .color(theme::TEXT_WEAK)
                                    .size(11.5),
                            );
                        }
                        ui.end_row();

                        for row in &order {
                            ui.label(
                                RichText::new(short_name(&row.name))
                                    .color(theme::TEXT)
                                    .size(12.0)
                                    .strong(),
                            );
                            for col in &order {
                                if row.id == col.id {
                                    ui.label(RichText::new("—").color(theme::TEXT_WEAK));
                                } else {
                                    let h2h = live.standings.head_to_head(row.id, col.id);
                                    if h2h.games() == 0 {
                                        ui.label(RichText::new("·").color(theme::TEXT_WEAK));
                                    } else {
                                        let share =
                                            (h2h.points() / f64::from(h2h.games())) as f32;
                                        h2h_record_cell(
                                            ui,
                                            share,
                                            &self.h2h_cell_text(live, row.id, col.id),
                                        );
                                    }
                                }
                            }
                            ui.end_row();
                        }
                    });
            });
    }

    /// Cell text for a head-to-head record, honouring the format toggle.
    fn h2h_cell_text(&self, live: &LiveData, engine: EngineId, opponent: EngineId) -> String {
        if self.h2h_per_game {
            let results = live.standings.pair_results(engine, opponent);
            let mut s = String::new();
            for (i, r) in results.iter().enumerate() {
                if i > 0 {
                    s.push(' ');
                }
                s.push_str(match r {
                    PairGameResult::Win => "1",
                    PairGameResult::Draw => "½",
                    PairGameResult::Loss => "0",
                });
            }
            s
        } else {
            let h2h = live.standings.head_to_head(engine, opponent);
            format!("{}-{}-{}", h2h.wins, h2h.draws, h2h.losses)
        }
    }

    /// Expandable list of the active tournament's games with a View button each.
    fn live_games_section(&mut self, ui: &mut Ui, live: &LiveData) {
        let order: Vec<usize> = self
            .live_games_cache
            .as_ref()
            .map(|(_, _, games)| {
                games
                    .iter()
                    .enumerate()
                    .filter(|(_, g)| g.pgn.as_deref().is_some_and(|p| !p.trim().is_empty()))
                    .map(|(i, _)| i)
                    .rev()
                    .collect()
            })
            .unwrap_or_default();

        ui.label(
            RichText::new(format!("Games ({})", order.len()))
                .color(theme::TEXT)
                .font(theme::semibold(13.5)),
        );
        if order.is_empty() {
            ui.label(
                RichText::new("No finished games yet.")
                    .color(theme::TEXT_WEAK)
                    .size(12.5),
            );
            return;
        }
        ui.add_space(6.0);

        let mut view_idx: Option<usize> = None;
        if let Some((_, _, games)) = &self.live_games_cache {
            for &i in &order {
                let g = &games[i];
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("R{}", g.round))
                            .color(theme::TEXT_FAINT)
                            .monospace()
                            .size(12.0),
                    );
                    let result = g
                        .result
                        .map(|r| r.pgn().to_string())
                        .unwrap_or_else(|| "…".to_string());
                    ui.label(
                        RichText::new(format!(
                            "{} vs {}  {}",
                            live.participant_name(g.white),
                            live.participant_name(g.black),
                            result
                        ))
                        .color(theme::TEXT_WEAK)
                        .size(12.5),
                    );
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("View").clicked() {
                            view_idx = Some(i);
                        }
                    });
                });
                ui.add_space(2.0);
            }
        }

        if let Some(i) = view_idx
            && let Some((_, _, games)) = &self.live_games_cache
        {
            let g = &games[i];
            let white = live.participant_name(g.white).to_string();
            let black = live.participant_name(g.black).to_string();
            self.viewer.open_game(g, &white, &black);
        }
    }

    // ── Stored-tournament detail (history) ──────────────────────────────────

    fn detail_panel(&mut self, ui: &mut Ui, backend: &mut Backend) {
        let Some(id) = self.selected else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Select a tournament to view its results.")
                        .color(theme::TEXT_WEAK)
                        .size(13.0),
                );
            });
            return;
        };

        let row = self
            .list
            .as_ref()
            .and_then(|l| l.iter().find(|t| t.id == id))
            .cloned();
        let Some(row) = row else {
            return;
        };

        if self.results.as_ref().map(|(rid, _)| *rid) != Some(id) {
            match backend.tournament_results(&row) {
                Ok(res) => self.results = Some((id, res)),
                Err(e) => {
                    self.error = Some(format!("Could not load results: {e}"));
                    self.results = None;
                }
            }
        }
        if self.games.as_ref().map(|(rid, _)| *rid) != Some(id) {
            self.games = Some((id, backend.list_games(id)));
        }

        ScrollArea::vertical()
            .id_salt("results_detail_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: 14,
                        right: 14,
                        top: 10,
                        bottom: 8,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&row.name)
                                    .color(theme::TEXT)
                                    .font(theme::semibold(18.0)),
                            );
                            ui.add_space(8.0);
                            let (label, color) = status_parts(&row.status);
                            widgets::status_pill(ui, label, "●", color);
                        });
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(config_summary(&row))
                                .color(theme::TEXT_WEAK)
                                .size(12.5),
                        );

                        ui.add_space(10.0);
                        self.action_bar(ui, backend, &row);
                        ui.add_space(10.0);

                        ui.separator();
                        ui.add_space(8.0);

                        if let Some((_, res)) = &self.results {
                            results_summary(ui, res);
                            ui.add_space(8.0);
                            standings_table(ui, res);
                            if res.participants.len() == 2 {
                                ui.add_space(12.0);
                                crate::stats_ui::match_stats_card(
                                    ui,
                                    &crosstable_order(res),
                                    &res.standings,
                                );
                            }
                        } else {
                            ui.label(
                                RichText::new("No results to show.")
                                    .color(theme::TEXT_WEAK)
                                    .size(13.0),
                            );
                        }
                        self.games_section(ui);
                    });
            });
    }

    /// List the stored tournament's games with a View button each.
    fn games_section(&mut self, ui: &mut Ui) {
        let count = self.games.as_ref().map_or(0, |(_, g)| g.len());
        if count == 0 {
            return;
        }

        let names: Vec<(EngineId, String)> = self
            .results
            .as_ref()
            .map(|(_, res)| {
                res.participants
                    .iter()
                    .map(|p| (p.id, p.name.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let name_of = |id: EngineId| -> String {
            names
                .iter()
                .find(|(pid, _)| *pid == id)
                .map(|(_, n)| n.clone())
                .unwrap_or_else(|| "?".to_string())
        };

        ui.add_space(14.0);
        ui.separator();
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!("Games ({count})"))
                .color(theme::TEXT)
                .font(theme::semibold(13.5)),
        );
        ui.add_space(6.0);

        let mut view_idx: Option<usize> = None;
        if let Some((_, games)) = &self.games {
            ScrollArea::vertical()
                .id_salt("results_games_scroll")
                .auto_shrink([false, true])
                .max_height(220.0)
                .show(ui, |ui| {
                    for (i, g) in games.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("R{}", g.round))
                                    .color(theme::TEXT_FAINT)
                                    .monospace()
                                    .size(12.0),
                            );
                            let result = g
                                .result
                                .map(|r| r.pgn().to_string())
                                .unwrap_or_else(|| "…".to_string());
                            ui.label(
                                RichText::new(format!(
                                    "{} vs {}  {}",
                                    name_of(g.white),
                                    name_of(g.black),
                                    result
                                ))
                                .color(theme::TEXT_WEAK)
                                .size(12.5),
                            );
                            let has_pgn =
                                g.pgn.as_deref().is_some_and(|p| !p.trim().is_empty());
                            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .add_enabled(has_pgn, egui::Button::new("View"))
                                    .clicked()
                                {
                                    view_idx = Some(i);
                                }
                            });
                        });
                        ui.add_space(2.0);
                    }
                });
        }

        if let Some(i) = view_idx
            && let Some((_, games)) = &self.games
        {
            let g = &games[i];
            let white = name_of(g.white);
            let black = name_of(g.black);
            self.viewer.open_game(g, &white, &black);
        }
    }

    fn action_bar(&mut self, ui: &mut Ui, backend: &mut Backend, row: &TournamentRow) {
        // Clicking an unfinished row already loads it; this button is the
        // fallback when that failed (e.g. missing engines were re-added).
        let resumable = row.status != STATUS_FINISHED;

        ui.horizontal_wrapped(|ui| {
            if resumable {
                if widgets::tinted_button(ui, "↩ Resume", theme::SUCCESS, true)
                    .on_hover_text("Reload this tournament and continue from where it stopped.")
                    .clicked()
                {
                    match backend.try_resume(row.clone()) {
                        Ok(()) => {
                            // The live view takes over via the active-follow
                            // logic on the next frame.
                        }
                        Err(e) => self.error = Some(format!("Resume failed: {e}")),
                    }
                }
                ui.add_space(6.0);
            }

            if self.pending_delete == Some(row.id) {
                if widgets::tinted_button(ui, "Confirm delete", theme::DANGER, true)
                    .on_hover_text("Permanently remove this tournament and its games.")
                    .clicked()
                {
                    match backend.delete_tournament(row.id) {
                        Ok(()) => {
                            self.error = None;
                            self.pending_delete = None;
                            self.refresh(backend);
                        }
                        Err(e) => {
                            self.error = Some(format!("Delete failed: {e}"));
                            self.pending_delete = None;
                        }
                    }
                }
                ui.add_space(4.0);
                if ui
                    .button(RichText::new("Cancel").color(theme::TEXT))
                    .clicked()
                {
                    self.pending_delete = None;
                }
            } else if widgets::tinted_button(ui, "Delete", theme::DANGER, true)
                .on_hover_text("Delete this tournament from the database.")
                .clicked()
            {
                self.pending_delete = Some(row.id);
            }

            if let Some(pgn) = &row.pgn_path {
                ui.add_space(6.0);
                if ui
                    .button(RichText::new("Copy PGN path").color(theme::TEXT_WEAK))
                    .on_hover_text(pgn.clone())
                    .clicked()
                {
                    ui.ctx().copy_text(pgn.clone());
                }
            }

            ui.add_space(6.0);
            let export_resp = ui.menu_button(RichText::new("Export    ").size(13.0), |ui| {
                ui.set_min_width(170.0);
                let have_results = matches!(&self.results, Some((id, _)) if *id == row.id);
                if ui
                    .add_enabled(have_results, egui::Button::new("Standings (CSV)"))
                    .clicked()
                {
                    let note = match &self.results {
                        Some((_, res)) => {
                            crate::export_ui::export_standings_csv(&row.name, &export_rows(res))
                        }
                        None => Some("No results loaded.".to_string()),
                    };
                    self.export_note = note;
                    ui.close();
                }
                if ui
                    .add_enabled(have_results, egui::Button::new("Crosstable (CSV)"))
                    .clicked()
                {
                    let note = match &self.results {
                        Some((_, res)) => crate::export_ui::export_crosstable_csv(
                            &row.name,
                            &crosstable_order(res),
                            &res.standings,
                        ),
                        None => Some("No results loaded.".to_string()),
                    };
                    self.export_note = note;
                    ui.close();
                }
                if ui.button("Game PGN").clicked() {
                    let pgn = backend.collect_pgn(row.id).unwrap_or_default();
                    self.export_note = crate::export_ui::export_pgn(&row.name, &pgn);
                    ui.close();
                }
            });
            widgets::dropdown_arrow(ui, export_resp.response.rect);
            if let Some(note) = &self.export_note {
                ui.add_space(6.0);
                ui.label(RichText::new(note).color(theme::TEXT_WEAK).size(12.0));
            }
        });
    }
}

// ── Live data ───────────────────────────────────────────────────────────────

/// One results-table row: standings joined with rating + identity.
#[derive(Clone)]
struct Row {
    id: EngineId,
    rank: usize,
    name: String,
    version: String,
    elo: f64,
    elo_delta: f64,
    points: f64,
    games: u32,
    wins: u32,
    draws: u32,
    losses: u32,
    time_losses: u32,
    crash_losses: u32,
    nps: Option<u64>,
}

/// An owned snapshot of everything the live view renders this frame.
struct LiveData {
    name: String,
    status: TournamentStatus,
    finished: usize,
    total: usize,
    rows: Vec<Row>,
    standings: Standings,
    errors: Vec<String>,
    started_at: Option<std::time::Instant>,
    total_game_ms: u64,
    games_timed: usize,
    in_flight_games: Vec<InFlightGame>,
    termination_counts: HashMap<Termination, usize>,
    /// Current parallel-games limit.
    concurrency: usize,
    /// Gauntlet seed engines (empty for round robin).
    gauntlet_seeds: Vec<EngineId>,
    /// Config-based per-game length estimate (ETA fallback before any game
    /// finishes); `None` for node/depth-limited play.
    per_game_estimate: Option<f64>,
    writeback: RatingWriteback,
}

impl LiveData {
    fn participant_name(&self, id: EngineId) -> &str {
        self.rows
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.name.as_str())
            .unwrap_or("?")
    }

    /// Rows in rank order, shaped for CSV export.
    fn export_rows(&self) -> Vec<colosseum_core::ExportRow> {
        let mut rows: Vec<&Row> = self.rows.iter().collect();
        rows.sort_by_key(|r| r.rank);
        rows.into_iter()
            .map(|r| colosseum_core::ExportRow {
                rank: r.rank,
                name: r.name.clone(),
                version: r.version.clone(),
                elo: r.elo,
                elo_delta: r.elo_delta,
                points: r.points,
                games: r.games,
                wins: r.wins,
                draws: r.draws,
                losses: r.losses,
                nps: r.nps,
            })
            .collect()
    }

    /// (id, name) pairs in rank order, for the crosstable header/rows.
    fn crosstable_order(&self) -> Vec<(EngineId, String)> {
        let mut rows: Vec<&Row> = self.rows.iter().collect();
        rows.sort_by_key(|r| r.rank);
        rows.into_iter().map(|r| (r.id, r.name.clone())).collect()
    }
}

// ── Sorting ─────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortKey {
    Rank,
    Name,
    Elo,
    EloDelta,
    Points,
    Games,
    Nps,
}

#[derive(Clone, Copy)]
struct SortState {
    key: SortKey,
    ascending: bool,
}

impl Default for SortState {
    fn default() -> Self {
        Self {
            key: SortKey::Rank,
            ascending: true,
        }
    }
}

fn sort_rows(rows: &mut [Row], sort: SortState) {
    use std::cmp::Ordering;
    rows.sort_by(|a, b| {
        let ord = match sort.key {
            SortKey::Rank => a.rank.cmp(&b.rank),
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortKey::Elo => a.elo.partial_cmp(&b.elo).unwrap_or(Ordering::Equal),
            SortKey::EloDelta => a
                .elo_delta
                .partial_cmp(&b.elo_delta)
                .unwrap_or(Ordering::Equal),
            SortKey::Points => a.points.partial_cmp(&b.points).unwrap_or(Ordering::Equal),
            SortKey::Games => a.games.cmp(&b.games),
            SortKey::Nps => a.nps.unwrap_or(0).cmp(&b.nps.unwrap_or(0)),
        };
        if sort.ascending { ord } else { ord.reverse() }
    });
}

/// A clickable column header that toggles/sets the sort key.
fn sortable_header(ui: &mut Ui, label: &str, key: SortKey, sort: &mut SortState) {
    let active = sort.key == key;
    // Always lay out an arrow (transparent when inactive) so activating a
    // column never changes the header width and shifts the table.
    let arrow = if active && sort.ascending { " ↑" } else { " ↓" };
    let color = if active { theme::ACCENT } else { theme::TEXT };
    let fmt = |color| egui::TextFormat {
        font_id: theme::semibold(12.5),
        color,
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::default();
    job.append(label, 0.0, fmt(color));
    job.append(
        arrow,
        0.0,
        fmt(if active { color } else { Color32::TRANSPARENT }),
    );
    if ui.add(egui::Button::new(job).frame(false)).clicked() {
        if active {
            sort.ascending = !sort.ascending;
        } else {
            sort.key = key;
            sort.ascending = matches!(key, SortKey::Name | SortKey::Rank);
        }
    }
}

fn strong_header(label: &str) -> RichText {
    RichText::new(label)
        .color(theme::TEXT)
        .font(theme::semibold(12.5))
}

// ── Shared cell renderers ───────────────────────────────────────────────────

/// The "Name version" layout job: semibold name, weak version.
fn engine_name_job(name: &str, version: &str) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        name,
        0.0,
        egui::TextFormat {
            font_id: theme::semibold(13.0),
            color: theme::TEXT,
            ..Default::default()
        },
    );
    let version = version.trim();
    if !version.is_empty() {
        job.append(
            version,
            5.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(12.0),
                color: theme::TEXT_WEAK,
                ..Default::default()
            },
        );
    }
    job
}

/// "Name version" in one truncating table cell.
fn engine_name_label(ui: &mut Ui, name: &str, version: &str) {
    ui.add(egui::Label::new(engine_name_job(name, version)).truncate());
}

/// "Name version" at its natural width (grids/headers — truncation there
/// collapses the label to a few characters).
fn engine_name_label_full(ui: &mut Ui, name: &str, version: &str) {
    ui.add(egui::Label::new(engine_name_job(name, version)));
}

/// Forfeit summary ("2× time · 1× crash"), dim dash when clean.
fn forfeit_cell(ui: &mut Ui, time_losses: u32, crash_losses: u32) {
    if time_losses == 0 && crash_losses == 0 {
        ui.label(RichText::new("—").color(theme::TEXT_FAINT).size(12.0));
        return;
    }
    let mut parts: Vec<String> = Vec::new();
    if time_losses > 0 {
        parts.push(format!("{time_losses}× time"));
    }
    if crash_losses > 0 {
        parts.push(format!("{crash_losses}× crash"));
    }
    ui.label(
        RichText::new(parts.join(" · "))
            .color(theme::WARN)
            .size(12.0),
    )
    .on_hover_text("Losses on time / by crash or illegal move.");
}

/// A tinted head-to-head record cell (green = winning share, red = losing).
fn h2h_record_cell(ui: &mut Ui, score_share: f32, text: &str) {
    egui::Frame::new()
        .fill(h2h_cell_fill(score_share))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .color(theme::TEXT)
                    .monospace()
                    .size(11.5),
            );
        });
}

/// Background fill for a head-to-head cell based on score share `s` (0..=1).
fn h2h_cell_fill(s: f32) -> Color32 {
    if s > 0.5 {
        theme::tint(theme::SUCCESS, (s - 0.5) * 0.5)
    } else if s < 0.5 {
        theme::tint(theme::DANGER, (0.5 - s) * 0.5)
    } else {
        Color32::TRANSPARENT
    }
}

// ── Live side panel (currently playing + termination breakdown) ─────────────

fn live_side_panel(ui: &mut Ui, live: &LiveData) {
    ScrollArea::vertical()
        .id_salt("results_side_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if !live.in_flight_games.is_empty() {
                ui.label(
                    RichText::new(format!("● Playing ({})", live.in_flight_games.len()))
                        .color(theme::TEXT)
                        .font(theme::semibold(12.5)),
                );
                ui.add_space(4.0);
                for game in &live.in_flight_games {
                    let white = live.participant_name(game.white);
                    let black = live.participant_name(game.black);
                    egui::Frame::new()
                        .fill(theme::BG_ELEVATED)
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                RichText::new(format!("Round {}", game.round))
                                    .color(theme::TEXT_FAINT)
                                    .size(10.5),
                            );
                            ui.label(
                                RichText::new(format!("⬜ {}", short_name(white)))
                                    .color(theme::TEXT)
                                    .size(11.5),
                            );
                            ui.label(
                                RichText::new(format!("⬛ {}", short_name(black)))
                                    .color(theme::TEXT_WEAK)
                                    .size(11.5),
                            );
                        });
                    ui.add_space(3.0);
                }
            }

            if !live.termination_counts.is_empty() {
                if !live.in_flight_games.is_empty() {
                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(6.0);
                }
                ui.label(
                    RichText::new("Terminations")
                        .color(theme::TEXT)
                        .font(theme::semibold(12.5)),
                );
                ui.add_space(4.0);
                termination_breakdown(ui, &live.termination_counts);
            }
        });
}

fn termination_breakdown(ui: &mut Ui, counts: &HashMap<Termination, usize>) {
    let groups: &[(&str, &[Termination])] = &[
        (
            "Natural",
            &[
                Termination::Checkmate,
                Termination::Stalemate,
                Termination::FiftyMove,
                Termination::Threefold,
                Termination::InsufficientMaterial,
                Termination::MaxMoves,
            ],
        ),
        (
            "Adjudicated",
            &[Termination::AdjudicatedDraw, Termination::AdjudicatedResign],
        ),
        (
            "Errors",
            &[
                Termination::TimeForfeit,
                Termination::EngineCrash,
                Termination::IllegalMove,
                Termination::Aborted,
            ],
        ),
    ];

    for (group_label, terms) in groups {
        let relevant: Vec<(Termination, usize)> = terms
            .iter()
            .filter_map(|t| counts.get(t).map(|&n| (*t, n)))
            .collect();
        if relevant.is_empty() {
            continue;
        }
        ui.label(
            RichText::new(*group_label)
                .color(theme::TEXT_FAINT)
                .size(11.0)
                .italics(),
        );
        for (term, count) in relevant {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(termination_label(term))
                        .color(theme::TEXT_WEAK)
                        .size(12.0),
                );
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(count.to_string())
                            .color(theme::TEXT)
                            .size(12.0)
                            .monospace(),
                    );
                });
            });
        }
        ui.add_space(3.0);
    }
}

fn termination_label(t: Termination) -> &'static str {
    match t {
        Termination::Checkmate => "Checkmate",
        Termination::Stalemate => "Stalemate",
        Termination::FiftyMove => "50-move rule",
        Termination::Threefold => "Threefold rep.",
        Termination::InsufficientMaterial => "Insuff. mat.",
        Termination::AdjudicatedDraw => "Adj. draw",
        Termination::AdjudicatedResign => "Adj. resign",
        Termination::MaxMoves => "Max moves",
        Termination::TimeForfeit => "Time forfeit",
        Termination::EngineCrash => "Engine crash",
        Termination::IllegalMove => "Illegal move",
        Termination::Aborted => "Aborted",
    }
}

// ── Stored-results table ────────────────────────────────────────────────────

/// One read-only results row: standings joined with rating + identity.
struct ResultRow {
    rank: usize,
    name: String,
    version: String,
    elo: f64,
    elo_delta: f64,
    points: f64,
    games: u32,
    wins: u32,
    draws: u32,
    losses: u32,
    time_losses: u32,
    crash_losses: u32,
    nps: Option<u64>,
}

fn build_rows(res: &TournamentResults) -> Vec<ResultRow> {
    let standings: &Standings = &res.standings;
    let ranked = standings.ranked_by_points();
    let rank_of = |id| ranked.iter().position(|x| x == &id).map_or(0, |p| p + 1);

    let mut rows: Vec<ResultRow> = res
        .participants
        .iter()
        .map(|p| {
            let st = standings.standing(p.id);
            let e = res.elo.get(&p.id).copied().unwrap_or_default();
            ResultRow {
                rank: rank_of(p.id),
                name: p.name.clone(),
                version: p.version.clone(),
                elo: e.current,
                elo_delta: e.delta,
                points: st.points(),
                games: st.games(),
                wins: st.wins,
                draws: st.draws,
                losses: st.losses,
                time_losses: st.time_losses,
                crash_losses: st.crash_losses,
                nps: st.avg_nps(),
            }
        })
        .collect();
    rows.sort_by_key(|r| r.rank);
    rows
}

/// Reconstructed rows shaped for CSV export (rank order).
fn export_rows(res: &TournamentResults) -> Vec<colosseum_core::ExportRow> {
    build_rows(res)
        .into_iter()
        .map(|r| colosseum_core::ExportRow {
            rank: r.rank,
            name: r.name,
            version: r.version,
            elo: r.elo,
            elo_delta: r.elo_delta,
            points: r.points,
            games: r.games,
            wins: r.wins,
            draws: r.draws,
            losses: r.losses,
            nps: r.nps,
        })
        .collect()
}

/// (id, name) pairs in rank order for the crosstable.
fn crosstable_order(res: &TournamentResults) -> Vec<(EngineId, String)> {
    let ranked = res.standings.ranked_by_points();
    let rank_of = |id: EngineId| ranked.iter().position(|x| *x == id).unwrap_or(usize::MAX);
    let mut ps: Vec<_> = res.participants.iter().collect();
    ps.sort_by_key(|p| rank_of(p.id));
    ps.into_iter().map(|p| (p.id, p.name.clone())).collect()
}

fn results_summary(ui: &mut Ui, res: &TournamentResults) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{} / {} games played",
                res.games_finished, res.games_total
            ))
            .color(theme::TEXT)
            .font(theme::semibold(13.0)),
        );
        ui.add_space(10.0);
        ui.label(
            RichText::new(format!("{} decisive · {} drawn", res.decisive, res.draws))
                .color(theme::TEXT_WEAK)
                .size(12.5),
        );
    });
}

fn standings_table(ui: &mut Ui, res: &TournamentResults) {
    let rows = build_rows(res);
    if rows.is_empty() {
        ui.label(
            RichText::new("No participants recorded.")
                .color(theme::TEXT_WEAK)
                .size(13.0),
        );
        return;
    }

    let header_h = 28.0;
    let row_h = 28.0;
    TableBuilder::new(ui)
        .striped(true)
        .cell_layout(Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(40.0)) // rank
        .column(Column::initial(210.0).at_least(130.0).clip(true)) // engine
        .column(Column::exact(60.0)) // elo
        .column(Column::exact(76.0)) // elo delta
        .column(Column::exact(60.0)) // points
        .column(Column::exact(52.0)) // games
        .column(Column::exact(92.0)) // w-d-l
        .column(Column::exact(110.0)) // forfeits
        .column(Column::remainder().at_least(80.0)) // nps
        .header(header_h, |mut header| {
            for label in [
                "#", "Engine", "Elo", "Δ", "Pts", "Gms", "W-D-L", "Forfeits", "Avg nps",
            ] {
                header.col(|ui| {
                    ui.label(
                        RichText::new(label)
                            .color(theme::TEXT)
                            .font(theme::semibold(12.5)),
                    );
                });
            }
        })
        .body(|mut body| {
            for row in &rows {
                body.row(row_h, |mut tr| {
                    tr.col(|ui| widgets::rank_badge(ui, row.rank));
                    tr.col(|ui| {
                        engine_name_label(ui, &row.name, &row.version);
                    });
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(format!("{:.0}", row.elo))
                                .color(theme::TEXT)
                                .monospace(),
                        );
                    });
                    tr.col(|ui| widgets::elo_delta_chip(ui, row.elo_delta));
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(format!("{:.1}", row.points))
                                .color(theme::ACCENT)
                                .monospace()
                                .strong(),
                        );
                    });
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(row.games.to_string())
                                .color(theme::TEXT_WEAK)
                                .monospace(),
                        );
                    });
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(format!("{}-{}-{}", row.wins, row.draws, row.losses))
                                .color(theme::TEXT)
                                .monospace(),
                        );
                    });
                    tr.col(|ui| {
                        forfeit_cell(ui, row.time_losses, row.crash_losses);
                    });
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(format_nps(row.nps))
                                .color(theme::TEXT_WEAK)
                                .monospace(),
                        );
                    });
                });
            }
        });
}

// ── Small helpers ───────────────────────────────────────────────────────────

fn status_pill_parts(status: TournamentStatus) -> (&'static str, &'static str, Color32) {
    match status {
        TournamentStatus::Running => ("Running", "●", theme::SUCCESS),
        TournamentStatus::Stopping => ("Stopping", "●", theme::WARN),
        TournamentStatus::Stopped => ("Stopped", "●", theme::TEXT_WEAK),
        TournamentStatus::Finished => ("Finished", "●", theme::ACCENT),
        TournamentStatus::Idle => ("Idle", "○", theme::TEXT_FAINT),
    }
}

/// Stored status string → (display label, color). A stored "running" row has
/// no live driver, so nothing is actually playing: it shows as Stopped.
fn status_parts(status: &str) -> (&'static str, Color32) {
    match status {
        "finished" => ("Finished", theme::ACCENT),
        _ => ("Stopped", theme::WARN),
    }
}

/// Compact one-line config description for the detail header.
fn config_summary(row: &TournamentRow) -> String {
    let c: &TournamentConfig = &row.config;
    let format = match c.format {
        Format::RoundRobin { cycles } if cycles > 1 => format!("Round Robin ×{cycles}"),
        Format::RoundRobin { .. } => "Round Robin".to_string(),
        Format::Gauntlet { seeds, cycles } if cycles > 1 => {
            format!("Gauntlet ({seeds} seed) ×{cycles}")
        }
        Format::Gauntlet { seeds, .. } => format!("Gauntlet ({seeds} seed)"),
    };
    let tc = match c.time_control {
        TimeControl::PerMove { ms } => format!("{ms} ms/move"),
        TimeControl::SuddenDeath { base_ms } => format!("{} sudden death", clock_str(base_ms)),
        TimeControl::Increment { base_ms, inc_ms } => {
            format!("{}+{}", clock_str(base_ms), clock_str(inc_ms))
        }
        TimeControl::Nodes { nodes } => format!("{nodes} nodes/move"),
        TimeControl::Depth { depth } => format!("depth {depth}/move"),
    };
    format!(
        "{format} · {tc} · {} games/pair · started {}",
        c.games_per_pair,
        format_timestamp(&row.created_at)
    )
}

/// Config-based single-game length estimate in seconds (`None` for node- or
/// depth-limited play, whose duration depends on engine speed). Mirrors the
/// setup form's estimate: ~60 moves per side.
fn estimate_game_secs(tc: &TimeControl) -> Option<f64> {
    const EST_MOVES_PER_SIDE: f64 = 60.0;
    let per_side_ms = match tc {
        TimeControl::PerMove { ms } => (*ms).max(1) as f64 * EST_MOVES_PER_SIDE,
        TimeControl::SuddenDeath { base_ms } => (*base_ms).max(1) as f64,
        TimeControl::Increment { base_ms, inc_ms } => {
            (*base_ms).max(1) as f64 + *inc_ms as f64 * EST_MOVES_PER_SIDE
        }
        TimeControl::Nodes { .. } | TimeControl::Depth { .. } => return None,
    };
    Some(2.0 * per_side_ms / 1000.0)
}

/// Render a millisecond clock value compactly (e.g. `60s`, `1.5s`, `500ms`).
fn clock_str(ms: u64) -> String {
    if ms >= 1000 {
        let s = ms as f64 / 1000.0;
        if (s.fract()).abs() < f64::EPSILON {
            format!("{s:.0}s")
        } else {
            format!("{s:.1}s")
        }
    } else {
        format!("{ms}ms")
    }
}

/// Render an ISO-8601 timestamp as `YYYY-MM-DD HH:MM` (best-effort).
fn format_timestamp(ts: &str) -> String {
    if ts.len() >= 16 {
        ts[..16].replace('T', " ")
    } else {
        ts.to_string()
    }
}

/// Render a duration in seconds as a compact human string ("45s", "12m",
/// "1h 05m", "2d 3h").
fn format_duration(secs: f64) -> String {
    let s = secs.round().max(0.0) as u64;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        let m = s / 60;
        let rs = s % 60;
        if rs == 0 {
            format!("{m}m")
        } else {
            format!("{m}m {rs:02}s")
        }
    } else if s < 86_400 {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}d {}h", s / 86_400, (s % 86_400) / 3600)
    }
}

fn format_nps(nps: Option<u64>) -> String {
    match nps {
        None => "—".to_string(),
        Some(n) if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        Some(n) if n >= 1_000 => format!("{:.0}k", n as f64 / 1_000.0),
        Some(n) => n.to_string(),
    }
}

fn short_name(name: &str) -> String {
    if name.chars().count() <= 10 {
        name.to_string()
    } else {
        let s: String = name.chars().take(9).collect();
        format!("{s}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: EngineId, points: f64, elo: f64, nps: Option<u64>) -> Row {
        Row {
            id,
            rank: 0,
            name: "E".to_string(),
            version: String::new(),
            elo,
            elo_delta: 0.0,
            points,
            games: 0,
            wins: 0,
            draws: 0,
            losses: 0,
            time_losses: 0,
            crash_losses: 0,
            nps,
        }
    }

    #[test]
    fn sort_rows_by_points_descending() {
        let (a, b, c) = (EngineId::new(), EngineId::new(), EngineId::new());
        let mut rows = vec![
            row(a, 1.0, 1500.0, None),
            row(b, 3.0, 1500.0, None),
            row(c, 2.0, 1500.0, None),
        ];
        sort_rows(
            &mut rows,
            SortState {
                key: SortKey::Points,
                ascending: false,
            },
        );
        assert_eq!(rows[0].id, b);
        assert_eq!(rows[1].id, c);
        assert_eq!(rows[2].id, a);
    }

    #[test]
    fn sort_rows_by_nps_handles_missing() {
        let (a, b) = (EngineId::new(), EngineId::new());
        let mut rows = vec![row(a, 0.0, 1500.0, None), row(b, 0.0, 1500.0, Some(5))];
        sort_rows(
            &mut rows,
            SortState {
                key: SortKey::Nps,
                ascending: true,
            },
        );
        // None is treated as 0, so it sorts first ascending.
        assert_eq!(rows[0].id, a);
        assert_eq!(rows[1].id, b);
    }

    #[test]
    fn nps_formatting() {
        assert_eq!(format_nps(None), "—");
        assert_eq!(format_nps(Some(500)), "500");
        assert_eq!(format_nps(Some(12_000)), "12k");
        assert_eq!(format_nps(Some(2_500_000)), "2.5M");
    }

    #[test]
    fn per_game_estimate_matches_time_control() {
        // 100 ms/move, 60 moves/side → 12 s.
        let est = estimate_game_secs(&TimeControl::PerMove { ms: 100 }).unwrap();
        assert!((est - 12.0).abs() < 1e-9);
        assert!(estimate_game_secs(&TimeControl::Nodes { nodes: 1 }).is_none());
    }
}
