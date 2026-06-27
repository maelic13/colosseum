// SPDX-License-Identifier: GPL-3.0-or-later
//! Tournament History tab: browse past and unfinished tournaments stored in the
//! database, inspect their final standings, resume an unfinished one, or delete.
//!
//! The list and the per-tournament results are read from SQLite on demand (on
//! first open, after an action, or via Refresh) and cached — never queried every
//! frame.

use eframe::egui::{self, Color32, Layout, RichText, ScrollArea, Ui};
use egui_extras::{Column, TableBuilder};

use colosseum_core::{
    EloPolicy, EngineId, Format, Standings, TimeControl, TournamentConfig, TournamentId,
};
use colosseum_engine::{
    EloEntry, ResultParticipant, TournamentResults, TournamentRow, store::STATUS_FINISHED,
};

use crate::backend::Backend;
use crate::theme;
use crate::widgets;

/// What the tab asks the shell to do after a frame.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum HistoryAction {
    #[default]
    None,
    /// A tournament was resumed; switch to the Tournament tab to show it live.
    SwitchToTournament,
}

/// Persistent state for the History tab.
#[derive(Default)]
pub struct HistoryTab {
    /// Cached tournament list (most recent first); `None` until first load.
    list: Option<Vec<TournamentRow>>,
    /// Currently selected tournament.
    selected: Option<TournamentId>,
    /// Cached reconstructed results for the selected tournament.
    results: Option<(TournamentId, TournamentResults)>,
    /// Tournament awaiting delete confirmation (two-step inline confirm).
    pending_delete: Option<TournamentId>,
    /// Last error message from a DB action.
    error: Option<String>,
    /// Transient note shown after an export action.
    export_note: Option<String>,
}

impl HistoryTab {
    /// Draw the tab body; returns an action for the shell to act on.
    pub fn show(&mut self, ui: &mut Ui, backend: &mut Backend) -> HistoryAction {
        if self.list.is_none() {
            self.refresh(backend);
        }
        let mut action = HistoryAction::None;

        // Top bar: title, count, refresh.
        egui::Panel::top("history_top")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DARKEST)
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Tournament History")
                            .color(theme::TEXT)
                            .size(15.0)
                            .strong(),
                    );
                    let count = self.list.as_ref().map_or(0, Vec::len);
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(format!("{count} stored"))
                            .color(theme::TEXT_WEAK)
                            .size(12.5),
                    );
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(RichText::new("⟳ Refresh").color(theme::TEXT_WEAK))
                            .clicked()
                        {
                            self.refresh(backend);
                        }
                    });
                });
                if let Some(err) = &self.error {
                    ui.label(
                        RichText::new(format!("⚠ {err}"))
                            .color(theme::DANGER)
                            .size(12.5),
                    );
                }
            });

        // Left: tournament list.
        egui::Panel::left("history_list")
            .default_size(300.0)
            .size_range(220.0..=460.0)
            .resizable(true)
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                right: 12,
                top: 8,
                ..Default::default()
            }))
            .show_inside(ui, |ui| {
                self.list_panel(ui);
            });

        // Centre: selected tournament detail.
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                top: 8,
                left: 4,
                ..Default::default()
            }))
            .show_inside(ui, |ui| {
                action = self.detail_panel(ui, backend);
            });

        action
    }

    /// Reload the tournament list from the database and drop stale caches.
    fn refresh(&mut self, backend: &Backend) {
        let list = backend.list_tournaments();
        // Keep the current selection if it still exists.
        if let Some(sel) = self.selected
            && !list.iter().any(|t| t.id == sel)
        {
            self.selected = None;
            self.results = None;
        }
        if self.selected.is_none() {
            self.selected = list.first().map(|t| t.id);
        }
        self.list = Some(list);
        self.results = None;
        self.pending_delete = None;
    }

    fn list_panel(&mut self, ui: &mut Ui) {
        let Some(list) = self.list.clone() else {
            return;
        };
        if list.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("🏛").color(theme::TEXT_FAINT).size(40.0));
                ui.add_space(6.0);
                ui.label(
                    RichText::new("No tournaments yet")
                        .color(theme::TEXT_WEAK)
                        .size(15.0)
                        .strong(),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Run a tournament to see it here.")
                        .color(theme::TEXT_FAINT)
                        .size(12.5),
                );
            });
            return;
        }

        ScrollArea::vertical()
            .id_salt("history_list_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for row in &list {
                    let selected = self.selected == Some(row.id);
                    if self.list_row(ui, row, selected) {
                        self.selected = Some(row.id);
                        self.results = None;
                        self.pending_delete = None;
                    }
                    ui.add_space(4.0);
                }
            });
    }

    /// Draw one selectable tournament card; returns true if it was clicked.
    fn list_row(&self, ui: &mut Ui, row: &TournamentRow, selected: bool) -> bool {
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
                            .size(13.5)
                            .strong(),
                    );
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        let (label, color) = status_parts(&row.status);
                        ui.label(RichText::new(label).color(color).size(11.0).strong());
                    });
                });
                ui.label(
                    RichText::new(format_timestamp(&row.created_at))
                        .color(theme::TEXT_WEAK)
                        .size(11.5),
                );
            })
            .response;

        let interact = ui.interact(
            resp.rect,
            egui::Id::new("history_row").with(row.id),
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

    fn detail_panel(&mut self, ui: &mut Ui, backend: &mut Backend) -> HistoryAction {
        let Some(id) = self.selected else {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Select a tournament to view its results.")
                        .color(theme::TEXT_WEAK)
                        .size(13.0),
                );
            });
            return HistoryAction::None;
        };

        // Find the row in the cached list (clone the small metadata we need).
        let row = self
            .list
            .as_ref()
            .and_then(|l| l.iter().find(|t| t.id == id))
            .cloned();
        let Some(row) = row else {
            return HistoryAction::None;
        };

        // Load (and cache) the reconstructed results for this tournament.
        if self.results.as_ref().map(|(rid, _)| *rid) != Some(id) {
            match backend.tournament_results(&row) {
                Ok(res) => self.results = Some((id, res)),
                Err(e) => {
                    self.error = Some(format!("Could not load results: {e}"));
                    self.results = None;
                }
            }
        }

        let mut action = HistoryAction::None;

        ScrollArea::vertical()
            .id_salt("history_detail_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Header: name + status + actions.
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&row.name)
                            .color(theme::TEXT)
                            .size(18.0)
                            .strong(),
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
                action = self.action_bar(ui, backend, &row);
                ui.add_space(10.0);

                ui.separator();
                ui.add_space(8.0);

                if let Some((_, res)) = &self.results {
                    results_summary(ui, res);
                    ui.add_space(8.0);
                    standings_table(ui, res);
                } else {
                    ui.label(
                        RichText::new("No results to show.")
                            .color(theme::TEXT_WEAK)
                            .size(13.0),
                    );
                }
            });

        action
    }

    fn action_bar(&mut self, ui: &mut Ui, backend: &mut Backend, row: &TournamentRow) -> HistoryAction {
        let mut action = HistoryAction::None;
        let resumable = row.status != STATUS_FINISHED;
        let busy = backend.is_busy();

        ui.horizontal(|ui| {
            if resumable {
                let enabled = !busy;
                if widgets::tinted_button(ui, "↩ Resume", theme::SUCCESS, enabled)
                    .on_hover_text(if enabled {
                        "Reload this tournament and continue from where it stopped."
                    } else {
                        "Stop the running tournament first."
                    })
                    .clicked()
                {
                    match backend.try_resume(row.clone()) {
                        Ok(()) => action = HistoryAction::SwitchToTournament,
                        Err(e) => self.error = Some(format!("Resume failed: {e}")),
                    }
                }
                ui.add_space(6.0);
            }

            // Delete with a two-step inline confirm.
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
            } else if widgets::tinted_button(ui, "🗑 Delete", theme::DANGER, true)
                .on_hover_text("Delete this tournament from the database.")
                .clicked()
            {
                self.pending_delete = Some(row.id);
            }

            // Copy the configured PGN output path, if any.
            if let Some(pgn) = &row.pgn_path {
                ui.add_space(6.0);
                if ui
                    .button(RichText::new("⧉ Copy PGN path").color(theme::TEXT_WEAK))
                    .on_hover_text(pgn.clone())
                    .clicked()
                {
                    ui.ctx().copy_text(pgn.clone());
                }
            }

            // Export menu: CSV standings / crosstable / game PGN.
            ui.add_space(6.0);
            ui.menu_button(RichText::new("Export ▾").size(13.0), |ui| {
                ui.set_min_width(170.0);
                let have_results =
                    matches!(&self.results, Some((id, _)) if *id == row.id);
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
            if let Some(note) = &self.export_note {
                ui.add_space(6.0);
                ui.label(RichText::new(note).color(theme::TEXT_WEAK).size(12.0));
            }
        });

        action
    }
}

// ── Standings table ───────────────────────────────────────────────────────────

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
    nps: Option<u64>,
}

fn build_rows(res: &TournamentResults) -> Vec<ResultRow> {
    let standings: &Standings = &res.standings;
    let ranked = standings.ranked_by_points();
    let rank_of = |id| ranked.iter().position(|x| x == &id).map_or(0, |p| p + 1);

    let mut rows: Vec<ResultRow> = res
        .participants
        .iter()
        .map(|p: &ResultParticipant| {
            let st = standings.standing(p.id);
            let e = res.elo.get(&p.id).copied().unwrap_or(EloEntry::default());
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
    let mut ps: Vec<&ResultParticipant> = res.participants.iter().collect();
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
            .size(13.0)
            .strong(),
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
        // Fixed widths (not Column::auto) so columns don't jump as content varies.
        .column(Column::exact(40.0)) // rank
        .column(Column::initial(170.0).at_least(110.0).clip(true)) // name
        .column(Column::exact(64.0)) // version
        .column(Column::exact(60.0)) // elo
        .column(Column::exact(76.0)) // elo delta
        .column(Column::exact(60.0)) // points
        .column(Column::exact(52.0)) // games
        .column(Column::exact(92.0)) // w-d-l
        .column(Column::remainder().at_least(80.0)) // nps
        .header(header_h, |mut header| {
            for label in ["#", "Engine", "Ver", "Elo", "Δ", "Pts", "Gms", "W-D-L", "Avg nps"] {
                header.col(|ui| {
                    ui.label(RichText::new(label).color(theme::TEXT).size(12.5).strong());
                });
            }
        })
        .body(|mut body| {
            for row in &rows {
                body.row(row_h, |mut tr| {
                    tr.col(|ui| widgets::rank_badge(ui, row.rank));
                    tr.col(|ui| {
                        ui.label(RichText::new(&row.name).color(theme::TEXT).strong());
                    });
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(&row.version)
                                .color(theme::TEXT_WEAK)
                                .size(12.5),
                        );
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

/// Status string → (display label, color).
fn status_parts(status: &str) -> (&'static str, Color32) {
    match status {
        "finished" => ("Finished", theme::ACCENT),
        "running" => ("Running", theme::SUCCESS),
        "stopped" => ("Stopped", theme::WARN),
        _ => ("Unknown", theme::TEXT_WEAK),
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
    let elo = match c.elo_policy {
        EloPolicy::PerGame => "Elo per-game",
        EloPolicy::EndOfTournament => "Elo at end",
        EloPolicy::Never => "no Elo",
    };
    format!(
        "{format} · {tc} · {} games/pair · {elo} · started {}",
        c.games_per_pair,
        format_timestamp(&row.created_at)
    )
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

fn format_nps(nps: Option<u64>) -> String {
    match nps {
        None => "—".to_string(),
        Some(n) if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        Some(n) if n >= 1_000 => format!("{:.0}k", n as f64 / 1_000.0),
        Some(n) => n.to_string(),
    }
}
