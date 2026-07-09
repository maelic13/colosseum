// SPDX-License-Identifier: GPL-3.0-or-later
//! Arena tab: every tournament — the live one and stored history — in one
//! place. A tournament list sits on the right; selecting the active tournament
//! shows the live view (standings, head-to-head, live board, transport
//! controls), selecting a stored one shows its reconstructed results with
//! Resume/Delete/Export actions. Individual finished games aren't browsed here
//! — export the PGN and open it in a dedicated viewer instead.
//!
//! The live Elo column always shows exactly what the configured rating
//! writeback would store in the library: static library ratings ("Never"),
//! joint maximum-likelihood ratings ("All engines"), or a single performance
//! rating with everyone else anchored ("Estimate one engine").

use std::collections::HashMap;
use std::rc::Rc;

use eframe::egui::{self, Color32, DragValue, Layout, RichText, ScrollArea, Ui};
use egui_extras::{Column, TableBuilder};

use colosseum_core::{
    EngineId, Format, PairGameResult, RatingWriteback, Standings, Termination, TimeControl,
    TournamentConfig, TournamentId,
};
use colosseum_engine::{
    InFlightGame, TournamentResults, TournamentRow, TournamentStatus, store::STATUS_FINISHED,
};

use crate::backend::Backend;
use crate::theme;
use crate::widgets;

/// The heavy per-tournament live state, rebuilt only when a game finishes
/// (see [`ResultsTab::capture_live`]).
struct CachedLive {
    rows: Rc<Vec<Row>>,
    standings: Rc<Standings>,
    errors: Rc<Vec<String>>,
    termination_counts: Rc<HashMap<Termination, usize>>,
    config: Rc<TournamentConfig>,
}

/// Persistent state for the Arena tab.
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
    /// Rows highlighted for bulk actions (ctrl/shift click). The *current*
    /// tournament (`selected`, shown in the main area) is independent.
    multi_selected: std::collections::HashSet<TournamentId>,
    /// Anchor row for shift-range selection.
    select_anchor: Option<TournamentId>,
    /// Bulk delete awaiting modal confirmation.
    bulk_delete: Option<Vec<TournamentId>>,
    /// Rename dialog opened from the list context menu: (target, buffer).
    rename_dialog: Option<(TournamentId, String)>,
    /// Inline rename buffer for the current tournament's header title.
    title_edit: Option<String>,
    /// Focus the inline title editor on the frame it opens.
    title_edit_focus: bool,
    /// Last error message from a DB action.
    error: Option<String>,
    /// Transient note shown after an export action.
    export_note: Option<String>,

    // ── Live-view state ──
    sort: SortState,
    /// Whether the engine-errors bar is expanded (collapsed summary otherwise).
    errors_expanded: bool,
    /// Whether the tournaments list panel is collapsed to a slim strip.
    list_collapsed: bool,
    /// The heavy per-tournament state (standings, rows, errors), rebuilt only
    /// when a game finishes — cloning `Standings` every frame at 30 Hz was
    /// the live view's biggest per-frame cost.
    live_cache: Option<(TournamentId, usize, CachedLive)>,
    /// Head-to-head cell format: `false` = W-D-L counts, `true` = per-game results.
    h2h_per_game: bool,
    /// Loaded-tournament ids last frame (a new one steals the selection).
    known_actives: Vec<TournamentId>,
    /// The unfinished tournament we last auto-loaded on selection, so entering
    /// the tab drops straight into the live view rather than the stored-detail
    /// layer — without re-attempting the load every frame (or after a failure).
    auto_loaded: Option<TournamentId>,
    /// When the list was last re-read from the database (auto-refresh).
    last_refresh: Option<std::time::Instant>,
    /// Per-tournament live-view (Standings | Live lens) states.
    live_views: crate::live_view::LiveViews,
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

        // Drop straight into the live view: auto-load the selected tournament
        // if it is unfinished and not yet loaded (same state as pressing Go's
        // sibling, Resume). Attempted once per selection so a load failure — or
        // the user deliberately not running it — doesn't loop.
        self.auto_load_selected(backend);

        // Right: tournament list — collapsible to a slim strip for more room.
        let mut list_rect: Option<egui::Rect> = None;
        if self.list_collapsed {
            egui::Panel::right("results_list_collapsed")
                .exact_size(24.0)
                .resizable(false)
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(2, 6)))
                .show(ui, |ui| {
                    let count = self.list.as_ref().map_or(0, Vec::len);
                    if widgets::expand_strip(ui, "‹", &format!("Tournaments ({count})")) {
                        self.list_collapsed = false;
                    }
                });
        } else {
            let panel = egui::Panel::right("results_list")
                .default_size(280.0)
                .size_range(220.0..=400.0)
                .resizable(true)
                .frame(egui::Frame::new().inner_margin(egui::Margin {
                    left: 12,
                    right: 8,
                    top: 10,
                    bottom: 8,
                }))
                .show(ui, |ui| {
                    self.list_panel(ui, backend);
                });
            list_rect = Some(panel.response.rect);
        }

        // A click anywhere outside the list drops the bulk selection (the
        // current tournament stays). Skipped while a popup/modal is open, so
        // choosing a context-menu action doesn't wipe its own target set.
        if !self.multi_selected.is_empty()
            && self.bulk_delete.is_none()
            && !egui::Popup::is_any_open(ui.ctx())
            && let Some(rect) = list_rect
            && ui.input(|i| {
                i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p))
            })
        {
            self.multi_selected.clear();
        }

        self.bulk_delete_modal(ui, backend);
        self.rename_modal(ui, backend);

        // Centre: live view for loaded tournaments, stored results otherwise.
        let live_id = self.selected.filter(|id| backend.active(*id).is_some());
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show(ui, |ui| {
                if let Some(id) = live_id {
                    self.live_view(ui, backend, id);
                } else {
                    self.detail_panel(ui, backend);
                }
            });
    }

    /// Load the selected tournament into the live view when it is unfinished
    /// and not already loaded, so opening the tab shows the live view directly
    /// instead of the stored-detail layer. Guarded by `auto_loaded` so each
    /// selection is only tried once (a failed resume falls back to the detail
    /// panel, which offers a manual Resume button).
    fn auto_load_selected(&mut self, backend: &mut Backend) {
        let Some(id) = self.selected else {
            return;
        };
        if self.auto_loaded == Some(id) || backend.active(id).is_some() {
            return;
        }
        let row = self
            .list
            .as_ref()
            .and_then(|l| l.iter().find(|t| t.id == id))
            .cloned();
        let Some(row) = row else {
            return;
        };
        self.auto_loaded = Some(id);
        // Finished tournaments load too: the loaded (live) view is the one
        // with the full standings, head-to-head, terminations and
        // Information rail — no reason a finished tournament should show a
        // stripped-down layer. Loading replays the DB only; no engines spawn
        // and there is nothing left to play.
        if let Err(e) = backend.try_resume(row) {
            self.error = Some(format!("Could not load: {e}"));
        }
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
                    .color(theme::text())
                    .font(theme::semibold(14.0)),
            );
            let count = self.list.as_ref().map_or(0, Vec::len);
            ui.label(
                RichText::new(count.to_string())
                    .color(theme::text_faint())
                    .size(12.0),
            );
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::collapse_button(ui, "›")
                    .on_hover_text("Hide the tournaments list.")
                    .clicked()
                {
                    self.list_collapsed = true;
                }
            });
        });
        if let Some(err) = &self.error {
            ui.label(
                RichText::new(format!("⚠ {err}"))
                    .color(theme::danger())
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
                ui.label(RichText::new("♟").color(theme::text_faint()).size(40.0));
                ui.add_space(6.0);
                ui.label(
                    RichText::new("No tournaments yet")
                        .color(theme::text_weak())
                        .font(theme::semibold(15.0)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Set one up in the Tournament tab.")
                        .color(theme::text_faint())
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
                    let in_multi = self.multi_selected.contains(&row.id);
                    // Live status + progress for loaded tournaments.
                    let live = backend.active(row.id).and_then(|a| {
                        a.snapshot
                            .lock()
                            .ok()
                            .map(|s| (s.status, s.games_finished, s.games_total))
                    });
                    let resp = self.list_row(ui, row, selected, in_multi, live);
                    if resp.clicked() {
                        let mods = ui.input(|i| i.modifiers);
                        if mods.ctrl {
                            // Toggle in the bulk set. Starting a bulk selection
                            // implicitly includes the current tournament — it
                            // reads as selected, so ctrl-click *adds* to it.
                            if self.multi_selected.is_empty()
                                && let Some(cur) = self.selected
                                && cur != row.id
                            {
                                self.multi_selected.insert(cur);
                            }
                            if !self.multi_selected.remove(&row.id) {
                                self.multi_selected.insert(row.id);
                            }
                            self.select_anchor = Some(row.id);
                        } else if mods.shift {
                            let anchor = self.select_anchor.or(self.selected).unwrap_or(row.id);
                            let (mut a, mut b) = (None, None);
                            for (i, r) in list.iter().enumerate() {
                                if r.id == anchor {
                                    a = Some(i);
                                }
                                if r.id == row.id {
                                    b = Some(i);
                                }
                            }
                            if let (Some(a), Some(b)) = (a, b) {
                                let (lo, hi) = (a.min(b), a.max(b));
                                for r in &list[lo..=hi] {
                                    self.multi_selected.insert(r.id);
                                }
                            }
                        } else {
                            self.select_anchor = Some(row.id);
                            self.multi_selected.clear();
                            self.selected = Some(row.id);
                            self.pending_delete = None;
                            // Load immediately (finished tournaments too) —
                            // the loaded view carries the full standings,
                            // head-to-head and Information rail.
                            if backend.active(row.id).is_none()
                                && let Err(e) = backend.try_resume(row.clone())
                            {
                                self.error = Some(format!("Could not load: {e}"));
                            }
                        }
                    }
                    // Right-click selects (into the bulk set when not already
                    // part of it) and opens the actions menu.
                    if resp.secondary_clicked() && !in_multi {
                        self.multi_selected.clear();
                        self.multi_selected.insert(row.id);
                        self.select_anchor = Some(row.id);
                    }
                    let row_id = row.id;
                    resp.context_menu(|ui| {
                        self.tournament_context_menu(ui, backend, row_id, &list);
                    });
                    ui.add_space(4.0);
                }
            });
    }

    /// Right-click actions applied to the whole bulk selection (or just the
    /// clicked row when nothing else is selected).
    fn tournament_context_menu(
        &mut self,
        ui: &mut Ui,
        backend: &mut Backend,
        clicked: TournamentId,
        list: &[TournamentRow],
    ) {
        ui.set_min_width(150.0);
        let targets: Vec<TournamentId> = if self.multi_selected.contains(&clicked) {
            // List order, so bulk actions run top to bottom.
            list.iter()
                .map(|r| r.id)
                .filter(|id| self.multi_selected.contains(id))
                .collect()
        } else {
            vec![clicked]
        };
        let n = targets.len();
        let suffix = if n > 1 {
            format!(" ({n})")
        } else {
            String::new()
        };

        if ui.button(format!("Start{suffix}")).clicked() {
            for id in &targets {
                let row = list.iter().find(|r| r.id == *id);
                if backend.active(*id).is_none()
                    && let Some(row) = row
                    && row.status != STATUS_FINISHED
                    && let Err(e) = backend.try_resume(row.clone())
                {
                    self.error = Some(format!("Could not load: {e}"));
                    continue;
                }
                if let Some(active) = backend.active(*id) {
                    active.handle.go();
                }
            }
            ui.close();
        }
        if ui.button(format!("Stop{suffix}")).clicked() {
            for id in &targets {
                if let Some(active) = backend.active(*id) {
                    active.handle.stop();
                }
            }
            ui.close();
        }
        if ui.button(format!("Force-stop{suffix}")).clicked() {
            for id in &targets {
                if let Some(active) = backend.active(*id) {
                    active.handle.force_stop();
                }
            }
            ui.close();
        }
        if n == 1 && ui.button("Rename…").clicked() {
            let name = list
                .iter()
                .find(|r| r.id == clicked)
                .map(|r| r.name.clone())
                .unwrap_or_default();
            self.rename_dialog = Some((clicked, name));
            ui.close();
        }
        ui.separator();
        if ui
            .button(RichText::new(format!("Delete{suffix}…")).color(theme::danger()))
            .clicked()
        {
            self.bulk_delete = Some(targets);
            ui.close();
        }
    }

    /// The rename dialog (list context menu → Rename…).
    fn rename_modal(&mut self, ui: &Ui, backend: &mut Backend) {
        let Some((id, mut buf)) = self.rename_dialog.clone() else {
            return;
        };
        let mut done = false;
        let mut save = false;
        let modal = egui::Modal::new(egui::Id::new("tournament_rename")).show(ui.ctx(), |ui| {
            ui.set_width(320.0);
            ui.label(
                RichText::new("Rename tournament")
                    .font(theme::semibold(16.0))
                    .color(theme::text()),
            );
            ui.add_space(10.0);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut buf)
                    .desired_width(f32::INFINITY)
                    .margin(egui::Margin::symmetric(8, 6)),
            );
            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                save = true;
            }
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::tinted_button(ui, "Save", theme::accent(), !buf.trim().is_empty())
                    .clicked()
                {
                    save = true;
                }
                ui.add_space(4.0);
                if ui
                    .button(RichText::new("Cancel").color(theme::text()))
                    .clicked()
                {
                    done = true;
                }
            });
        });
        if save {
            self.apply_rename(backend, id, &buf);
            done = true;
        } else {
            // Keep the (possibly edited) buffer for the next frame.
            self.rename_dialog = Some((id, buf));
        }
        if done || modal.should_close() {
            self.rename_dialog = None;
        }
    }

    /// Apply a rename and refresh every cached view of the name.
    fn apply_rename(&mut self, backend: &mut Backend, id: TournamentId, name: &str) {
        let unchanged = self
            .list
            .as_ref()
            .is_some_and(|l| l.iter().any(|r| r.id == id && r.name == name.trim()));
        if name.trim().is_empty() || unchanged {
            return;
        }
        match backend.rename_tournament(id, name) {
            Ok(()) => {
                self.error = None;
                self.live_cache = None;
                self.refresh(backend);
            }
            Err(e) => self.error = Some(format!("Rename failed: {e}")),
        }
    }

    /// The tournament name as an inline-editable title: clicking the name (or
    /// the pencil beside it) swaps it for a text field; Enter or the painted
    /// checkmark saves, Escape or the × cancels.
    fn editable_title(
        &mut self,
        ui: &mut Ui,
        backend: &mut Backend,
        id: TournamentId,
        name: &str,
        size: f32,
    ) {
        if self.title_edit.is_some() {
            let mut confirmed = false;
            let mut cancelled = false;
            if let Some(buf) = &mut self.title_edit {
                let resp = ui.add(
                    egui::TextEdit::singleline(buf)
                        .desired_width(240.0)
                        .margin(egui::Margin::symmetric(8, 4)),
                );
                if self.title_edit_focus {
                    resp.request_focus();
                    self.title_edit_focus = false;
                }
                confirmed = widgets::confirm_button(ui)
                    .on_hover_text("Save name (Enter)")
                    .clicked()
                    || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                cancelled = widgets::cancel_button(ui)
                    .on_hover_text("Cancel (Esc)")
                    .clicked()
                    || ui.input(|i| i.key_pressed(egui::Key::Escape));
            }
            if confirmed {
                if let Some(new_name) = self.title_edit.take() {
                    self.apply_rename(backend, id, &new_name);
                }
            } else if cancelled {
                self.title_edit = None;
            }
        } else {
            let resp = ui
                .add(
                    egui::Label::new(
                        RichText::new(name)
                            .color(theme::text())
                            .font(theme::semibold(size)),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Rename tournament");
            ui.add_space(2.0);
            let pencil = widgets::edit_button(ui).on_hover_text("Rename tournament");
            if resp.clicked() || pencil.clicked() {
                self.title_edit = Some(name.to_string());
                self.title_edit_focus = true;
            }
        }
    }

    /// The bulk-delete confirmation modal (right-click → Delete).
    fn bulk_delete_modal(&mut self, ui: &Ui, backend: &mut Backend) {
        let Some(targets) = self.bulk_delete.clone() else {
            return;
        };
        let names: Vec<String> = self
            .list
            .as_ref()
            .map(|l| {
                l.iter()
                    .filter(|r| targets.contains(&r.id))
                    .map(|r| r.name.clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut done = false;
        let modal = egui::Modal::new(egui::Id::new("bulk_delete")).show(ui.ctx(), |ui| {
            ui.set_width(360.0);
            ui.label(
                RichText::new(format!(
                    "Delete {} tournament{}?",
                    targets.len(),
                    if targets.len() == 1 { "" } else { "s" }
                ))
                .font(theme::semibold(16.0))
                .color(theme::text()),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(names.join(", "))
                    .color(theme::text_weak())
                    .size(12.5),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Running games are aborted; all results are removed permanently.")
                    .color(theme::text_faint())
                    .size(12.0),
            );
            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::tinted_button(ui, "Delete", theme::danger(), true).clicked() {
                    for id in &targets {
                        if let Some(active) = backend.active(*id) {
                            active.handle.force_stop();
                        }
                        backend.close_tournament(*id);
                        if let Err(e) = backend.delete_tournament(*id) {
                            self.error = Some(format!("Delete failed: {e}"));
                        }
                    }
                    self.multi_selected.clear();
                    self.live_cache = None;
                    self.refresh(backend);
                    done = true;
                }
                ui.add_space(4.0);
                if ui
                    .button(RichText::new("Cancel").color(theme::text()))
                    .clicked()
                {
                    done = true;
                }
            });
        });
        if done || modal.should_close() {
            self.bulk_delete = None;
        }
    }

    /// One selectable tournament card; returns the row's interact response.
    fn list_row(
        &self,
        ui: &mut Ui,
        row: &TournamentRow,
        selected: bool,
        in_multi: bool,
        live: Option<(TournamentStatus, usize, usize)>,
    ) -> egui::Response {
        let (fill, stroke) = if selected {
            (
                theme::tint(theme::accent(), 0.12),
                egui::Stroke::new(1.0, theme::tint(theme::accent(), 0.4)),
            )
        } else if in_multi {
            // Bulk-selected but not the current one: a weaker wash.
            (
                theme::tint(theme::accent(), 0.06),
                egui::Stroke::new(1.0, theme::tint(theme::accent(), 0.25)),
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
                    // Truncate the name so it never collides with the status
                    // label on the right (status ≈ 56 pt at 11 pt semibold).
                    let name_w = (ui.available_width() - 64.0).max(40.0);
                    ui.allocate_ui_with_layout(
                        egui::vec2(name_w, 18.0),
                        Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&row.name)
                                        .color(if selected {
                                            theme::accent_bright()
                                        } else {
                                            theme::text()
                                        })
                                        .font(theme::semibold(13.5)),
                                )
                                .truncate(),
                            );
                        },
                    );
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        // Live = games in flight; Finished = all played;
                        // Stopped = loaded or stored with games remaining.
                        let (label, color) = match live {
                            Some((status, ..)) => match status {
                                TournamentStatus::Running | TournamentStatus::Stopping => {
                                    ("● Live", theme::success())
                                }
                                TournamentStatus::Finished => ("Finished", theme::accent()),
                                TournamentStatus::Stopped | TournamentStatus::Idle => {
                                    ("Stopped", theme::warn())
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
                // Second line: games progress (live counts when loaded,
                // stored counts otherwise). Third line: creation date.
                let (finished, total) = match live {
                    Some((_, finished, total)) if total > 0 => (finished, total),
                    _ => (row.games_finished, row.games_total),
                };
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(
                    RichText::new(format!("{finished} / {total} games"))
                        .color(theme::text_weak())
                        .size(11.5),
                );
                ui.label(
                    RichText::new(format_timestamp(&row.created_at))
                        .color(theme::text_faint())
                        .size(10.5),
                );
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
                egui::Shape::rect_filled(resp.rect, egui::CornerRadius::same(6), theme::bg_hover()),
            );
        }
        interact
    }

    // ── Live view (active tournament) ───────────────────────────────────────

    fn live_view(&mut self, ui: &mut Ui, backend: &mut Backend, id: TournamentId) {
        let live = self.capture_live(backend, id);

        // Control bar (top).
        egui::Panel::top("results_live_controls")
            .frame(
                egui::Frame::new()
                    .fill(theme::bg_darkest())
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show(ui, |ui| {
                self.live_control_bar(ui, backend, id, &live);
            });

        // Errors panel (bottom), only when there are errors. Collapsed by
        // default to a one-line summary — same pattern as the Engines tab's
        // tablebases bar — so crashes don't permanently eat vertical space.
        if !live.errors.is_empty() {
            egui::Panel::bottom("results_live_errors")
                .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 8)))
                .show(ui, |ui| {
                    egui::Frame::new()
                        .fill(theme::tint(theme::danger(), 0.08))
                        .stroke(egui::Stroke::new(1.0, theme::tint(theme::danger(), 0.35)))
                        .corner_radius(egui::CornerRadius::same(8))
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            let header = ui
                                .horizontal(|ui| {
                                    widgets::disclosure_triangle(
                                        ui,
                                        self.errors_expanded,
                                        theme::danger(),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "Engine errors ({})",
                                            live.errors.len()
                                        ))
                                        .color(theme::danger())
                                        .font(theme::semibold(12.5)),
                                    );
                                    if !self.errors_expanded
                                        && let Some(last) = live.errors.last()
                                    {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(last)
                                                    .color(theme::text_faint())
                                                    .size(11.5),
                                            )
                                            .truncate(),
                                        );
                                    }
                                })
                                .response;
                            let click = ui.interact(
                                header.rect,
                                egui::Id::new("errors_header"),
                                egui::Sense::click(),
                            );
                            if click
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                            {
                                self.errors_expanded = !self.errors_expanded;
                            }
                            if self.errors_expanded {
                                ui.add_space(4.0);
                                ScrollArea::vertical()
                                    .id_salt("results_errors_scroll")
                                    .max_height(80.0)
                                    .auto_shrink([false, true])
                                    .show(ui, |ui| {
                                        for err in live.errors.iter().rev().take(20) {
                                            ui.label(
                                                RichText::new(err)
                                                    .color(theme::text_weak())
                                                    .size(12.0),
                                            );
                                        }
                                    });
                            }
                        });
                });
        }

        // Live lens: the whole body is the live game view.
        if self.live_views.is_watching(id) {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().inner_margin(egui::Margin::same(10)))
                .show(ui, |ui| {
                    self.live_views
                        .show(ui, backend, id, &live.in_flight_games, live.concurrency);
                });
            return;
        }

        // Side rail (right): in-flight games, termination breakdown, and the
        // tournament's settings — one consistent column instead of a card
        // floating in the table's whitespace.
        egui::Panel::right("results_live_side")
            .default_size(230.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::bg_darkest())
                    .stroke(egui::Stroke::new(1.0, theme::stroke()))
                    .inner_margin(egui::Margin::same(10)),
            )
            .show(ui, |ui| {
                if let Some(game_id) = live_side_panel(ui, &live)
                    && let Some(game) = live.in_flight_games.iter().find(|g| g.game_id == game_id)
                {
                    self.live_views.watch_game(id, game, live.concurrency);
                }
            });

        // Results table (centre) with breathing room on both sides.
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 14,
                right: 14,
                top: 10,
                bottom: 4,
            }))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("results_live_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.results_table(ui, &live);
                        ui.add_space(18.0);
                        self.head_to_head_section(ui, &live);
                        ui.add_space(8.0);
                    });
            });
    }

    /// Snapshot the backend state into owned data, releasing the lock quickly.
    ///
    /// Volatile fields (status, clocks, in-flight games) are read every frame;
    /// the heavy derived state (standings clone, rating rows, errors) only
    /// changes when a game finishes, so it is cached per finished-game count
    /// and shared via `Rc` — this keeps the per-frame cost flat even for
    /// tournaments with tens of thousands of recorded games.
    fn capture_live(&mut self, backend: &Backend, id: TournamentId) -> LiveData {
        let active = backend
            .active(id)
            .expect("live view without a loaded tournament");

        let snap = active.snapshot.lock().unwrap();
        let status = snap.status;
        let finished = snap.games_finished;
        let total = snap.games_total;
        // Play clock: frozen accumulation + the live stretch when running.
        let elapsed = snap.elapsed_active
            + snap
                .running_since
                .map_or(std::time::Duration::ZERO, |t| t.elapsed());
        let total_game_ms = snap.total_game_ms;
        let games_timed = snap.games_timed;
        let in_flight_games = snap.in_flight_games.clone();
        let concurrency = snap.concurrency;

        let cache_hit = self
            .live_cache
            .as_ref()
            .is_some_and(|(cid, n, _)| *cid == id && *n == finished);
        if !cache_hit {
            let standings = Rc::new(snap.standings.clone());
            let elo_model = snap.elo.clone();
            let errors = Rc::new(snap.recent_errors.clone());
            let termination_counts = Rc::new(snap.termination_counts.clone());
            drop(snap);

            // Display ratings = exactly what the writeback mode stores in the
            // library after each game (`None` keeps the start rating on
            // display and shows the tournament-only movement in Δ).
            let prior_of = |eid: EngineId| {
                active
                    .priors
                    .iter()
                    .find(|(pid, _)| *pid == eid)
                    .map_or(1500.0, |(_, p)| *p)
            };
            let ranked = standings.ranked_by_points();
            let rank_of = |id: EngineId| ranked.iter().position(|x| x == &id).map_or(0, |p| p + 1);

            let mut rows: Vec<Row> = active
                .participants
                .iter()
                .map(|p| {
                    let st = standings.standing(p.id);
                    let entry = elo_model.get(&p.id).copied().unwrap_or_default();
                    // The driver's entries already reflect the writeback mode
                    // (anchored engines sit exactly at their start rating).
                    // The Δ chip only shows for engines whose rating this
                    // tournament actually updates — under "Never" no rating
                    // moves, so no engine gets a delta.
                    let show_delta = active.rating_writeback.applies_to(p.id);
                    let (elo, elo_delta, elo_error) =
                        if matches!(active.rating_writeback, RatingWriteback::None) {
                            (prior_of(p.id), None, entry.error)
                        } else {
                            (
                                entry.current,
                                show_delta.then_some(entry.delta),
                                entry.error,
                            )
                        };
                    Row {
                        id: p.id,
                        rank: rank_of(p.id),
                        name: p.name.clone(),
                        version: p.version.clone(),
                        elo,
                        elo_delta,
                        elo_error,
                        points: st.points(),
                        games: st.games(),
                        wins: st.wins,
                        draws: st.draws,
                        losses: st.losses,
                        time_losses: st.time_losses,
                        crash_losses: st.crash_losses,
                        nps: st.avg_nps(),
                        depth: st.avg_depth(),
                        move_ms: st.avg_move_ms(),
                    }
                })
                .collect();
            rows.sort_by_key(|r| r.rank);

            self.live_cache = Some((
                id,
                finished,
                CachedLive {
                    rows: Rc::new(rows),
                    standings,
                    errors,
                    termination_counts,
                    config: Rc::new(active.config.clone()),
                },
            ));
        } else {
            drop(snap);
        }
        let cache = &self.live_cache.as_ref().expect("just ensured").2;

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

        let created_at = self
            .list
            .as_ref()
            .and_then(|l| l.iter().find(|r| r.id == id))
            .map(|r| r.created_at.clone())
            .unwrap_or_default();

        LiveData {
            name: active.name.clone(),
            created_at,
            config: Rc::clone(&cache.config),
            status,
            finished,
            total,
            rows: Rc::clone(&cache.rows),
            standings: Rc::clone(&cache.standings),
            errors: Rc::clone(&cache.errors),
            elapsed,
            total_game_ms,
            games_timed,
            in_flight_games,
            termination_counts: Rc::clone(&cache.termination_counts),
            concurrency,
            gauntlet_seeds,
            per_game_estimate: estimate_game_secs(&active.config.time_control),
            writeback: active.rating_writeback.clone(),
        }
    }

    fn live_control_bar(
        &mut self,
        ui: &mut Ui,
        backend: &mut Backend,
        id: TournamentId,
        live: &LiveData,
    ) {
        let status = live.status;
        let (in_flight, concurrency) = (live.in_flight_games.len(), live.concurrency);

        // ── Row 1: status + name + transport + progress ──
        ui.horizontal_wrapped(|ui| {
            let (label, dot, color) = status_pill_parts(status);
            widgets::status_pill(ui, label, dot, color);
            ui.add_space(6.0);
            self.editable_title(ui, backend, id, &live.name, 15.0);
            ui.add_space(10.0);

            let go_enabled = matches!(status, TournamentStatus::Stopped | TournamentStatus::Idle);
            let stop_enabled = matches!(status, TournamentStatus::Running);
            let force_enabled = matches!(
                status,
                TournamentStatus::Running | TournamentStatus::Stopping
            );

            if widgets::tinted_button(ui, "Start", theme::success(), go_enabled)
                .on_hover_text("Start the tournament (or resume where it stopped).")
                .clicked()
                && let Some(active) = backend.active(id)
            {
                active.handle.go();
            }
            if widgets::tinted_button(ui, "Stop", theme::warn(), stop_enabled)
                .on_hover_text("Stop launching new games; let in-flight games finish.")
                .clicked()
                && let Some(active) = backend.active(id)
            {
                active.handle.stop();
            }
            if widgets::tinted_button(ui, "Force-Stop", theme::danger(), force_enabled)
                .on_hover_text("Abort in-flight games immediately (discarding them).")
                .clicked()
                && let Some(active) = backend.active(id)
            {
                active.handle.force_stop();
            }

            ui.add_space(12.0);

            // Parallel-games limit, adjustable while running or stopped:
            // in-flight games always finish, only the launch rate changes.
            ui.label(
                RichText::new("Parallel")
                    .color(theme::text_weak())
                    .size(13.0),
            );
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
                    .color(theme::text_weak())
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
                    .fill(theme::accent()),
            );

            // Timing: elapsed / avg / ETA (measured average once available,
            // configured time control as the estimate before that, both
            // divided by the parallel-games limit).
            if !live.elapsed.is_zero() {
                let elapsed_secs = live.elapsed.as_secs_f64();
                ui.add_space(8.0);
                ui.label(
                    RichText::new(format!("⏱ {}", format_duration(elapsed_secs)))
                        .color(theme::text_weak())
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
                                .color(theme::text_weak())
                                .size(12.5),
                        );
                    }
                    let remaining = live.total.saturating_sub(live.finished);
                    if remaining > 0 && matches!(status, TournamentStatus::Running) {
                        let lanes = live.concurrency.max(1) as f64;
                        let eta = remaining as f64 * avg / lanes * 1.05;
                        ui.label(
                            RichText::new(format!("ETA ~{}", format_duration(eta)))
                                .color(theme::text_faint())
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

        // ── Row 2: lens switcher (stable, always leftmost) + actions ──
        ui.horizontal_wrapped(|ui| {
            // The Standings|Live switcher and Auto-follow live here — in the
            // normal flow, always in the same place — because a right-pinned
            // layout paints over the wrapped row-1 content in narrow windows.
            self.live_views
                .header_controls(ui, id, in_flight, concurrency);
            ui.add_space(12.0);

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

            // No manual Elo button: the writeback mode set at tournament
            // creation is applied automatically after every game ("Never"
            // means the library is never touched).

            if let Some(note) = &self.export_note {
                ui.label(RichText::new(note).color(theme::text_weak()).size(12.0));
            }

            // Delete — available even while running (force-stops first), for
            // when a run is a write-off rather than something to finish. Kept
            // pinned to the far right, away from the routine actions.
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if self.pending_delete == Some(id) {
                    // Right-to-left: Cancel sits at the edge, Confirm to its left.
                    if ui
                        .button(RichText::new("Cancel").color(theme::text()))
                        .clicked()
                    {
                        self.pending_delete = None;
                    }
                    ui.add_space(2.0);
                    if widgets::tinted_button(ui, "Confirm delete", theme::danger(), true)
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
                        self.live_cache = None;
                        self.refresh(backend);
                    }
                } else if widgets::tinted_button(ui, "Delete", theme::danger(), true)
                    .on_hover_text("Delete this tournament — stops it first if it's running.")
                    .clicked()
                {
                    self.pending_delete = Some(id);
                }
            });
        });
    }

    fn results_table(&mut self, ui: &mut Ui, live: &LiveData) {
        if live.rows.is_empty() {
            ui.add_space(20.0);
            ui.label(
                RichText::new("Waiting for the first game to finish…")
                    .color(theme::text_weak())
                    .size(13.0),
            );
            return;
        }

        let mut rows: Vec<Row> = live.rows.as_ref().clone();
        sort_rows(&mut rows, self.sort);

        let header_h = 30.0;
        let row_h = 30.0;

        TableBuilder::new(ui)
            // The page scrolls as a whole; a nested table scroll area would
            // add a second scrollbar inside the standings.
            .vscroll(false)
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
            // Wide enough for four-digit counts ("1160-267-211") — long
            // tournaments hit five digits of games.
            .column(Column::exact(112.0)) // w-d-l
            .column(Column::exact(90.0)) // nps
            .column(Column::exact(84.0)) // avg depth
            .column(Column::exact(84.0)) // avg time/move
            .column(Column::remainder().at_least(110.0)) // forfeits (far right)
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
                    sortable_header(ui, "Avg nps", SortKey::Nps, &mut self.sort);
                });
                header.col(|ui| sortable_header(ui, "Avg depth", SortKey::Depth, &mut self.sort));
                header
                    .col(|ui| sortable_header(ui, "Time/move", SortKey::MoveTime, &mut self.sort));
                header.col(|ui| {
                    ui.label(strong_header("Forfeits"))
                        .on_hover_text("Losses on time and by crash / illegal move.");
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
                            elo_cell(ui, row.elo, row.elo_error);
                        });
                        tr.col(|ui| {
                            widgets::elo_delta_chip(ui, row.elo_delta);
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(format!("{:.1}", row.points))
                                    .color(theme::accent())
                                    .monospace()
                                    .strong(),
                            );
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(row.games.to_string())
                                    .color(theme::text_weak())
                                    .monospace(),
                            );
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(format!("{}-{}-{}", row.wins, row.draws, row.losses))
                                    .color(theme::text())
                                    .monospace(),
                            );
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(format_nps(row.nps))
                                    .color(theme::text_weak())
                                    .monospace(),
                            );
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(format_depth(row.depth))
                                    .color(theme::text_weak())
                                    .monospace(),
                            );
                        });
                        tr.col(|ui| {
                            ui.label(
                                RichText::new(format_move_time(row.move_ms))
                                    .color(theme::text_weak())
                                    .monospace(),
                            );
                        });
                        tr.col(|ui| {
                            forfeit_cell(ui, row.time_losses, row.crash_losses);
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
                    .color(theme::text())
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
                    .color(theme::accent())
                    .font(theme::semibold(13.0)),
            );
            ui.label(
                RichText::new("vs each opponent:")
                    .color(theme::text_faint())
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
                        ui.label(RichText::new("·").color(theme::text_weak()));
                        ui.label("");
                    } else {
                        ui.label(
                            RichText::new(format!("{:.1} / {}", h2h.points(), h2h.games()))
                                .color(theme::text())
                                .monospace()
                                .size(12.5),
                        );
                        let share = (h2h.points() / f64::from(h2h.games())) as f32;
                        h2h_record_cell(ui, share, &self.h2h_cell_text(live, seed, opp.id));
                    }
                    ui.end_row();
                }
            });
    }

    fn h2h_matrix(&self, ui: &mut Ui, live: &LiveData) {
        ui.label(
            RichText::new("Row engine's record against each column engine.")
                .color(theme::text_weak())
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
                            // Same "name version" identity as the main table,
                            // so the four Rybkas / two Basilisks are distinct.
                            engine_name_label_full(ui, &col.name, &col.version);
                        }
                        ui.end_row();

                        for row in &order {
                            engine_name_label_full(ui, &row.name, &row.version);
                            for col in &order {
                                if row.id == col.id {
                                    ui.label(RichText::new("—").color(theme::text_weak()));
                                } else {
                                    let h2h = live.standings.head_to_head(row.id, col.id);
                                    if h2h.games() == 0 {
                                        ui.label(RichText::new("·").color(theme::text_weak()));
                                    } else {
                                        let share = (h2h.points() / f64::from(h2h.games())) as f32;
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

    // ── Stored-tournament detail (history) ──────────────────────────────────

    fn detail_panel(&mut self, ui: &mut Ui, backend: &mut Backend) {
        // A tournament is always auto-selected when one exists, so reaching here
        // with no selection means the library is empty — greet accordingly.
        let Some(id) = self.selected else {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("♟").color(theme::text_faint()).size(52.0));
                ui.add_space(10.0);
                ui.label(
                    RichText::new("No tournaments yet")
                        .color(theme::text_weak())
                        .font(theme::semibold(17.0)),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Head to the Tournament tab to set engines up and start playing.",
                    )
                    .color(theme::text_faint())
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
                            self.editable_title(ui, backend, row.id, &row.name, 18.0);
                            ui.add_space(8.0);
                            let (label, color) = status_parts(&row.status);
                            widgets::status_pill(ui, label, "●", color);
                        });
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(config_summary(&row))
                                .color(theme::text_weak())
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
                        } else {
                            ui.label(
                                RichText::new("No results to show.")
                                    .color(theme::text_weak())
                                    .size(13.0),
                            );
                        }
                    });
            });
    }

    fn action_bar(&mut self, ui: &mut Ui, backend: &mut Backend, row: &TournamentRow) {
        // Clicking an unfinished row already loads it; this button is the
        // fallback when that failed (e.g. missing engines were re-added).
        let resumable = row.status != STATUS_FINISHED;

        ui.horizontal(|ui| {
            if resumable {
                if widgets::tinted_button(ui, "↩ Resume", theme::success(), true)
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

            if let Some(pgn) = &row.pgn_path {
                if ui
                    .button(RichText::new("Copy PGN path").color(theme::text_weak()))
                    .on_hover_text(pgn.clone())
                    .clicked()
                {
                    ui.ctx().copy_text(pgn.clone());
                }
                ui.add_space(6.0);
            }

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
                ui.label(RichText::new(note).color(theme::text_weak()).size(12.0));
            }

            // Delete is destructive, so keep it pinned to the far right, away
            // from the routine actions and consistent with the live control bar.
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if self.pending_delete == Some(row.id) {
                    // Right-to-left: Cancel sits at the edge, Confirm to its left.
                    if ui
                        .button(RichText::new("Cancel").color(theme::text()))
                        .clicked()
                    {
                        self.pending_delete = None;
                    }
                    ui.add_space(4.0);
                    if widgets::tinted_button(ui, "Confirm delete", theme::danger(), true)
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
                } else if widgets::tinted_button(ui, "Delete", theme::danger(), true)
                    .on_hover_text("Delete this tournament from the database.")
                    .clicked()
                {
                    self.pending_delete = Some(row.id);
                }
            });
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
    /// `None` when this tournament does not update the engine's rating.
    elo_delta: Option<f64>,
    /// ±95% confidence half-width of the ML rating (None before any games).
    elo_error: Option<f64>,
    points: f64,
    games: u32,
    wins: u32,
    draws: u32,
    losses: u32,
    time_losses: u32,
    crash_losses: u32,
    nps: Option<u64>,
    depth: Option<f64>,
    move_ms: Option<f64>,
}

/// An owned snapshot of everything the live view renders this frame. The
/// heavy fields are `Rc`-shared from the per-finished-game cache.
struct LiveData {
    name: String,
    /// ISO-8601 creation timestamp (Information section).
    created_at: String,
    /// The tournament's full configuration (settings card, gauntlet layout).
    config: Rc<TournamentConfig>,
    status: TournamentStatus,
    finished: usize,
    total: usize,
    rows: Rc<Vec<Row>>,
    standings: Rc<Standings>,
    errors: Rc<Vec<String>>,
    /// Play time while games were running (frozen when stopped).
    elapsed: std::time::Duration,
    total_game_ms: u64,
    games_timed: usize,
    in_flight_games: Vec<InFlightGame>,
    termination_counts: Rc<HashMap<Termination, usize>>,
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
    /// "Name version" for an engine — the same identity shown in the standings
    /// table, so lists don't render four indistinguishable "Rybka"s.
    fn participant_label(&self, id: EngineId) -> String {
        self.rows
            .iter()
            .find(|r| r.id == id)
            .map(|r| join_name_version(&r.name, &r.version))
            .unwrap_or_else(|| "?".to_string())
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
        rows.into_iter()
            .map(|r| (r.id, join_name_version(&r.name, &r.version)))
            .collect()
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
    Depth,
    MoveTime,
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
                .unwrap_or(0.0)
                .partial_cmp(&b.elo_delta.unwrap_or(0.0))
                .unwrap_or(Ordering::Equal),
            SortKey::Points => a.points.partial_cmp(&b.points).unwrap_or(Ordering::Equal),
            SortKey::Games => a.games.cmp(&b.games),
            SortKey::Nps => a.nps.unwrap_or(0).cmp(&b.nps.unwrap_or(0)),
            SortKey::Depth => a
                .depth
                .unwrap_or(0.0)
                .partial_cmp(&b.depth.unwrap_or(0.0))
                .unwrap_or(Ordering::Equal),
            SortKey::MoveTime => a
                .move_ms
                .unwrap_or(0.0)
                .partial_cmp(&b.move_ms.unwrap_or(0.0))
                .unwrap_or(Ordering::Equal),
        };
        if sort.ascending { ord } else { ord.reverse() }
    });
}

/// A clickable column header that toggles/sets the sort key.
fn sortable_header(ui: &mut Ui, label: &str, key: SortKey, sort: &mut SortState) {
    let active = sort.key == key;
    // Always lay out an arrow (transparent when inactive) so activating a
    // column never changes the header width and shifts the table.
    let arrow = if active && sort.ascending {
        " ↑"
    } else {
        " ↓"
    };
    let color = if active {
        theme::accent()
    } else {
        theme::text()
    };
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
        .color(theme::text())
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
            color: theme::text(),
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
                color: theme::text_weak(),
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

/// The Elo cell: the rating, with the ML estimate's ±95% interval on hover.
fn elo_cell(ui: &mut Ui, elo: f64, error: Option<f64>) {
    let resp = ui.label(
        RichText::new(format!("{elo:.0}"))
            .color(theme::text())
            .monospace(),
    );
    if let Some(err) = error {
        resp.on_hover_text(format!(
            "{:.0} ± {:.0} (95% confidence, maximum-likelihood estimate)",
            elo, err
        ));
    }
}

/// Forfeit summary ("2× time · 1× crash"), dim dash when clean.
fn forfeit_cell(ui: &mut Ui, time_losses: u32, crash_losses: u32) {
    if time_losses == 0 && crash_losses == 0 {
        ui.label(RichText::new("—").color(theme::text_faint()).size(12.0));
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
            .color(theme::warn())
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
                    .color(theme::text())
                    .monospace()
                    .size(11.5),
            );
        });
}

/// Background fill for a head-to-head cell based on score share `s` (0..=1).
fn h2h_cell_fill(s: f32) -> Color32 {
    if s > 0.5 {
        theme::tint(theme::success(), (s - 0.5) * 0.5)
    } else if s < 0.5 {
        theme::tint(theme::danger(), (0.5 - s) * 0.5)
    } else {
        Color32::TRANSPARENT
    }
}

// ── Live side panel (currently playing + termination breakdown) ─────────────

/// Returns a game id when a Playing entry is clicked (opens it in Live view).
fn live_side_panel(ui: &mut Ui, live: &LiveData) -> Option<colosseum_core::GameId> {
    let mut clicked = None;
    ScrollArea::vertical()
        .id_salt("results_side_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if !live.in_flight_games.is_empty() {
                ui.label(
                    RichText::new(format!("● Playing ({})", live.in_flight_games.len()))
                        .color(theme::text())
                        .font(theme::semibold(12.5)),
                );
                ui.add_space(4.0);
                for game in &live.in_flight_games {
                    let white = live.participant_label(game.white);
                    let black = live.participant_label(game.black);
                    let card = egui::Frame::new()
                        .fill(theme::bg_elevated())
                        .corner_radius(egui::CornerRadius::same(4))
                        .inner_margin(egui::Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            // Tight rows: the default vertical spacing reads
                            // as dead space between the two one-line names.
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.label(
                                RichText::new(format!("Round {}", game.round))
                                    .color(theme::text_faint())
                                    .size(10.5),
                            );
                            widgets::side_engine_row(ui, true, &white, theme::text(), 11.5);
                            widgets::side_engine_row(ui, false, &black, theme::text_weak(), 11.5);
                        });
                    let resp = ui
                        .interact(
                            card.response.rect,
                            egui::Id::new("playing_card").with(game.game_id),
                            egui::Sense::click(),
                        )
                        .on_hover_text("Watch live");
                    if resp.hovered() {
                        ui.painter().rect_stroke(
                            card.response.rect,
                            egui::CornerRadius::same(4),
                            egui::Stroke::new(1.0, theme::accent()),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if resp.clicked() {
                        clicked = Some(game.game_id);
                    }
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
                        .color(theme::text())
                        .font(theme::semibold(12.5)),
                );
                ui.add_space(4.0);
                termination_breakdown(ui, &live.termination_counts);
            }

            // The tournament's configuration, always at hand.
            if !live.in_flight_games.is_empty() || !live.termination_counts.is_empty() {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(6.0);
            }
            settings_section(ui, live);
        });
    clicked
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
                .color(theme::text_faint())
                .size(11.0)
                .italics(),
        );
        for (term, count) in relevant {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(termination_label(term))
                        .color(theme::text_weak())
                        .size(12.0),
                );
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(count.to_string())
                            .color(theme::text())
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
    elo_delta: Option<f64>,
    /// ±95% confidence half-width of the ML rating (None before any games).
    elo_error: Option<f64>,
    points: f64,
    games: u32,
    wins: u32,
    draws: u32,
    losses: u32,
    time_losses: u32,
    crash_losses: u32,
    nps: Option<u64>,
    depth: Option<f64>,
    move_ms: Option<f64>,
}

fn build_rows(res: &TournamentResults) -> Vec<ResultRow> {
    let standings: &Standings = &res.standings;
    let ranked = standings.ranked_by_points();
    let rank_of = |id| ranked.iter().position(|x| x == &id).map_or(0, |p| p + 1);
    let seed_of = |id: EngineId| {
        res.seeds
            .iter()
            .find(|(sid, _)| *sid == id)
            .map_or(1500.0, |(_, s)| *s)
    };

    let mut rows: Vec<ResultRow> = res
        .participants
        .iter()
        .map(|p| {
            let st = standings.standing(p.id);
            let e = res.elo.get(&p.id).copied().unwrap_or_default();
            // Same writeback semantics as the live table: under "Never" the
            // Elo column keeps the start rating and no engine gets a Δ;
            // otherwise only engines the tournament updates show one.
            let (elo, elo_delta) = if matches!(res.rating_writeback, RatingWriteback::None) {
                (seed_of(p.id), None)
            } else {
                (
                    e.current,
                    res.rating_writeback.applies_to(p.id).then_some(e.delta),
                )
            };
            ResultRow {
                rank: rank_of(p.id),
                name: p.name.clone(),
                version: p.version.clone(),
                elo,
                elo_delta,
                elo_error: e.error,
                points: st.points(),
                games: st.games(),
                wins: st.wins,
                draws: st.draws,
                losses: st.losses,
                time_losses: st.time_losses,
                crash_losses: st.crash_losses,
                nps: st.avg_nps(),
                depth: st.avg_depth(),
                move_ms: st.avg_move_ms(),
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
    ps.into_iter()
        .map(|p| (p.id, join_name_version(&p.name, &p.version)))
        .collect()
}

fn results_summary(ui: &mut Ui, res: &TournamentResults) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "{} / {} games played",
                res.games_finished, res.games_total
            ))
            .color(theme::text())
            .font(theme::semibold(13.0)),
        );
        ui.add_space(10.0);
        ui.label(
            RichText::new(format!("{} decisive · {} drawn", res.decisive, res.draws))
                .color(theme::text_weak())
                .size(12.5),
        );
    });
}

fn standings_table(ui: &mut Ui, res: &TournamentResults) {
    let rows = build_rows(res);
    if rows.is_empty() {
        ui.label(
            RichText::new("No participants recorded.")
                .color(theme::text_weak())
                .size(13.0),
        );
        return;
    }

    let header_h = 28.0;
    let row_h = 28.0;
    TableBuilder::new(ui)
        // The page scrolls as a whole — no nested table scrollbar.
        .vscroll(false)
        .striped(true)
        .cell_layout(Layout::left_to_right(egui::Align::Center))
        .column(Column::exact(40.0)) // rank
        .column(Column::initial(210.0).at_least(130.0).clip(true)) // engine
        .column(Column::exact(60.0)) // elo
        .column(Column::exact(76.0)) // elo delta
        .column(Column::exact(60.0)) // points
        .column(Column::exact(52.0)) // games
        .column(Column::exact(92.0)) // w-d-l
        .column(Column::exact(90.0)) // nps
        .column(Column::exact(84.0)) // avg depth
        .column(Column::exact(84.0)) // avg time/move
        .column(Column::remainder().at_least(110.0)) // forfeits (far right)
        .header(header_h, |mut header| {
            for label in [
                "#",
                "Engine",
                "Elo",
                "Δ",
                "Pts",
                "Gms",
                "W-D-L",
                "Avg nps",
                "Avg depth",
                "Time/move",
                "Forfeits",
            ] {
                header.col(|ui| {
                    ui.label(
                        RichText::new(label)
                            .color(theme::text())
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
                        elo_cell(ui, row.elo, row.elo_error);
                    });
                    tr.col(|ui| widgets::elo_delta_chip(ui, row.elo_delta));
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(format!("{:.1}", row.points))
                                .color(theme::accent())
                                .monospace()
                                .strong(),
                        );
                    });
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(row.games.to_string())
                                .color(theme::text_weak())
                                .monospace(),
                        );
                    });
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(format!("{}-{}-{}", row.wins, row.draws, row.losses))
                                .color(theme::text())
                                .monospace(),
                        );
                    });
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(format_nps(row.nps))
                                .color(theme::text_weak())
                                .monospace(),
                        );
                    });
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(format_depth(row.depth))
                                .color(theme::text_weak())
                                .monospace(),
                        );
                    });
                    tr.col(|ui| {
                        ui.label(
                            RichText::new(format_move_time(row.move_ms))
                                .color(theme::text_weak())
                                .monospace(),
                        );
                    });
                    tr.col(|ui| {
                        forfeit_cell(ui, row.time_losses, row.crash_losses);
                    });
                });
            }
        });
}

// ── Small helpers ───────────────────────────────────────────────────────────

fn status_pill_parts(status: TournamentStatus) -> (&'static str, &'static str, Color32) {
    match status {
        TournamentStatus::Running => ("Running", "●", theme::success()),
        TournamentStatus::Stopping => ("Stopping", "●", theme::warn()),
        TournamentStatus::Stopped => ("Stopped", "●", theme::text_weak()),
        TournamentStatus::Finished => ("Finished", "●", theme::accent()),
        TournamentStatus::Idle => ("Idle", "○", theme::text_faint()),
    }
}

/// Stored status string → (display label, color). A stored "running" row has
/// no live driver, so nothing is actually playing: it shows as Stopped.
fn status_parts(status: &str) -> (&'static str, Color32) {
    match status {
        "finished" => ("Finished", theme::accent()),
        _ => ("Stopped", theme::warn()),
    }
}

/// The tournament-settings card shown beside the live standings: everything
/// configured in the Tournament tab, at a glance. Rendered as a section of
/// the standings side rail (Playing · Terminations · Settings).
fn settings_section(ui: &mut Ui, live: &LiveData) {
    let c: &TournamentConfig = &live.config;
    let name_of = |id: EngineId| live.participant_label(id);

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
            format!("{} + {}", clock_str(base_ms), clock_str(inc_ms))
        }
        TimeControl::Nodes { nodes } => format!("{nodes} nodes/move"),
        TimeControl::Depth { depth } => format!("depth {depth}/move"),
    };
    let draw = c.adjudication.draw.map_or("off".to_string(), |d| {
        format!(
            "|cp| ≤ {} for {} moves (ply ≥ {})",
            d.score_cp, d.move_count, d.min_ply
        )
    });
    let resign = c.adjudication.resign.map_or("off".to_string(), |r| {
        format!("|cp| ≥ {} for {} moves", r.score_cp, r.move_count)
    });
    let max_moves = c
        .adjudication
        .max_moves
        .map_or("off".to_string(), |m| m.to_string());
    let openings = match &c.start_position {
        colosseum_core::StartPosition::Startpos => "standard start".to_string(),
        colosseum_core::StartPosition::Book(book) => book
            .path
            .file_name()
            .map_or_else(|| "book".to_string(), |f| f.to_string_lossy().into_owned()),
    };
    let ratings = match &live.writeback {
        RatingWriteback::None => "never updated".to_string(),
        RatingWriteback::All => "all engines follow".to_string(),
        RatingWriteback::Chosen(ids) => {
            if ids.is_empty() {
                "chosen: none".to_string()
            } else {
                let names: Vec<String> = ids.iter().map(|id| name_of(*id)).collect();
                format!("follow: {}", names.join(", "))
            }
        }
        RatingWriteback::Estimate(id) => format!("follow: {}", name_of(*id)),
    };
    let pgn = c.pgn_output.as_ref().map(|p| {
        p.file_name().map_or_else(
            || p.to_string_lossy().into_owned(),
            |f| f.to_string_lossy().into_owned(),
        )
    });

    ui.label(
        RichText::new("Information")
            .color(theme::text())
            .font(theme::semibold(12.5)),
    );
    ui.add_space(4.0);
    let row = |ui: &mut Ui, label: &str, value: &str| {
        ui.horizontal_top(|ui| {
            ui.add_sized(
                egui::vec2(72.0, 15.0),
                egui::Label::new(RichText::new(label).color(theme::text_faint()).size(11.0)),
            );
            ui.add(
                egui::Label::new(RichText::new(value).color(theme::text_weak()).size(11.0)).wrap(),
            );
        });
    };
    if !live.created_at.is_empty() {
        row(ui, "Created", &format_timestamp(&live.created_at));
    }
    row(ui, "Format", &format);
    row(ui, "Time control", &tc);
    row(ui, "Games/pair", &c.games_per_pair.to_string());
    row(ui, "Parallel", &live.concurrency.to_string());
    row(ui, "Max moves", &max_moves);
    row(ui, "Draw adj.", &draw);
    row(ui, "Resign adj.", &resign);
    row(ui, "Ponder", if c.common.ponder { "on" } else { "off" });
    row(
        ui,
        "Tablebases",
        if c.common.tablebases { "on" } else { "off" },
    );
    row(ui, "Openings", &openings);
    if let Some(pgn) = &pgn {
        row(ui, "PGN file", pgn);
    }
    row(ui, "Ratings", &ratings);
}

/// Compact one-line config description: format · time control · games/pair.
fn live_config_summary(c: &TournamentConfig) -> String {
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
    format!("{format} · {tc} · {} games/pair", c.games_per_pair)
}

/// The stored-detail header summary: config plus when it was created.
fn config_summary(row: &TournamentRow) -> String {
    format!(
        "{} · started {}",
        live_config_summary(&row.config),
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

/// Render a duration in seconds as a compact human string ("400ms", "45s",
/// "12m", "1h 05m", "2d 3h").
fn format_duration(secs: f64) -> String {
    let ms = (secs * 1000.0).round().max(0.0) as u64;
    if ms < 1000 {
        // Sub-second (e.g. fast sudden-death games) — show milliseconds
        // rather than rounding down to a useless "0s".
        return format!("{ms}ms");
    }
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

/// Mean search depth: one decimal, em-dash when never reported.
fn format_depth(depth: Option<f64>) -> String {
    match depth {
        Some(d) => format!("{d:.1}"),
        None => "\u{2014}".to_string(),
    }
}

/// Mean time per move: milliseconds below one second, else seconds with one
/// decimal; em-dash when no moves were timed.
fn format_move_time(ms: Option<f64>) -> String {
    match ms {
        Some(ms) if ms < 1000.0 => format!("{ms:.0}ms"),
        Some(ms) => format!("{:.1}s", ms / 1000.0),
        None => "\u{2014}".to_string(),
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

/// "Name version" (version omitted when blank) — the engine identity used
/// consistently across tables, lists and panels.
fn join_name_version(name: &str, version: &str) -> String {
    let version = version.trim();
    if version.is_empty() {
        name.to_string()
    } else {
        format!("{name} {version}")
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
            elo_delta: Some(0.0),
            elo_error: None,
            points,
            games: 0,
            wins: 0,
            draws: 0,
            losses: 0,
            time_losses: 0,
            crash_losses: 0,
            nps,
            depth: None,
            move_ms: None,
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
