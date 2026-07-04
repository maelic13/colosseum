// SPDX-License-Identifier: GPL-3.0-or-later
//! Engine Management tab: add, scan, auto-detect, edit, and delete engines.
//!
//! Layout (en-croissant-inspired): a card **grid** of engines on the left (~half
//! the width), and on the right a split column — the selected engine's identity
//! and UCI options on top, with a **global** endgame-tablebase paths panel pinned
//! to the bottom. Thread/hash/tablebase-path options are intentionally hidden
//! from the per-engine editor: threads and hash are set in the Tournament tab,
//! and tablebase paths are shared globally.
//!
//! Adding an engine (single file or folder scan) spawns a background detection
//! task on the tokio runtime so the GUI thread never blocks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;
use eframe::egui::{self, DragValue, Layout, RichText, ScrollArea, Sense, Ui, UiBuilder};

use colosseum_core::{EngineConfig, EngineId, UciOption, UciOptionValue};
use colosseum_engine::{DetectResult, detect_engine, split_name_version};

use crate::backend::Backend;
use crate::logo;
use crate::theme;
use crate::widgets;

// ── Public tab state ─────────────────────────────────────────────────────────

/// All persistent state for the Engine Management tab.
#[derive(Default)]
pub struct EnginesTab {
    selected_id: Option<EngineId>,
    /// Edit buffer for the selected engine; `None` when nothing is selected.
    edit: Option<EngineEditBuf>,
    /// A running add-single / folder-scan job.
    pending: Option<DetectJob>,
    /// A separate re-detect channel for an already-listed engine.
    redetect_rx: Option<Receiver<Result<DetectResult, String>>>,
    redetect_for: Option<EngineId>,
    /// Error message from the last detection, shown until dismissed.
    detect_error: Option<String>,
    /// Search/filter text for the engine list.
    filter_text: String,
    /// Pending two-step "Delete All" confirmation.
    delete_all_confirm: bool,
    /// Engine awaiting delete confirmation in a modal (from the panel button
    /// or the card context menu).
    pending_delete: Option<EngineId>,
    /// Whether the global tablebases panel is expanded (collapsed by default —
    /// it's a set-once setting that shouldn't hold prime space hostage).
    tb_expanded: bool,
    /// Decoded logo textures, keyed by file path.
    logos: logo::LogoCache,
    /// A single-add result that matches a library engine, awaiting the user's
    /// "Add anyway" / "Cancel" decision.
    dup_single: Option<DupCandidate>,
    /// Folder-scan results that match library engines; shown in one checkbox
    /// dialog when the scan finishes (nothing ticked by default).
    dup_batch: Vec<(DupCandidate, bool)>,
    /// Whether the batch-duplicates dialog is open.
    dup_batch_open: bool,
}

/// A detected engine that duplicates one already in the library.
struct DupCandidate {
    /// The fully-built config, ready to insert if the user confirms.
    cfg: EngineConfig,
    /// Display name of the library engine it matches.
    matches: String,
}

/// How long after the last edit before the buffer is auto-committed.
const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(600);
/// How long the "✓ saved" feedback stays visible.
const SAVED_FLASH: Duration = Duration::from_millis(1600);

/// A deferred action chosen from an engine card's context menu. (Open-folder
/// and copy-path are handled inline in the menu; they don't touch tab state.)
enum CardAction {
    Clone,
    Redetect,
    Delete,
}

impl EnginesTab {
    /// Draw the full tab body.  Call every frame.
    pub fn show(&mut self, ui: &mut Ui, backend: &mut Backend) {
        self.logos.begin_frame();
        // Poll any background work first so results are visible this frame.
        self.poll_detect(backend);
        self.poll_redetect();
        self.autosave_tick(ui.ctx(), backend);
        self.show_delete_modal(ui.ctx(), backend);
        self.show_duplicate_modals(ui.ctx(), backend);

        // Keep frames coming while detection runs in the background — results
        // arrive on a channel and would otherwise wait for the next input event.
        if self.pending.is_some() || self.redetect_rx.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }

        self.show_toolbar(ui, backend);
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(4.0);

        let avail = ui.available_size();

        // ── Left: engine grid (fixed ~50% so the split tracks window size
        // rather than persisting an absolute width that breaks on resize) ──
        egui::Panel::left("engines_grid_panel")
            .resizable(false)
            .exact_size((avail.x * 0.5).max(320.0))
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 2, // room for focus rings at the panel edge
                right: 12,
                top: 2,
                ..Default::default()
            }))
            .show_inside(ui, |ui| {
                self.show_grid(ui, backend);
            });

        // ── Bottom-right: global tablebase paths (collapsible, collapsed by
        // default so the engine panel keeps the vertical space) ──
        let tb_h = if self.tb_expanded { 182.0 } else { 46.0 };
        egui::Panel::bottom("engines_tablebases_panel")
            .resizable(false)
            .exact_size(tb_h)
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 16,
                top: 8,
                ..Default::default()
            }))
            .show_inside(ui, |ui| {
                show_global_tablebases(ui, backend, &mut self.tb_expanded);
            });

        // ── Top-right: the selected engine's panel ──
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 16,
                right: 12,
                top: 4,
                ..Default::default()
            }))
            .show_inside(ui, |ui| {
                if self.edit.is_some() {
                    self.show_engine_panel(ui, backend);
                } else {
                    empty_state(ui);
                }
            });
    }

    /// Commit the edit buffer once it has been dirty for [`AUTOSAVE_DEBOUNCE`]
    /// without further changes, and keep frames coming while a save is pending
    /// or the "saved" flash is showing.
    fn autosave_tick(&mut self, ctx: &egui::Context, backend: &mut Backend) {
        let Some(edit) = &mut self.edit else { return };
        if edit.dirty {
            let since = *edit.dirty_since.get_or_insert_with(Instant::now);
            let elapsed = since.elapsed();
            if elapsed >= AUTOSAVE_DEBOUNCE {
                edit.commit(backend);
            } else {
                ctx.request_repaint_after(AUTOSAVE_DEBOUNCE - elapsed);
            }
        } else if let Some(saved) = edit.saved_at {
            let elapsed = saved.elapsed();
            if elapsed < SAVED_FLASH {
                ctx.request_repaint_after(SAVED_FLASH - elapsed);
            } else {
                edit.saved_at = None;
            }
        }
    }

    /// Immediately commit any pending edit. Called before the buffer is
    /// replaced (selection change, clone) and by the app shell on tab switch
    /// and window close so a debounced edit is never lost.
    pub fn flush_edit(&mut self, backend: &mut Backend) {
        if let Some(edit) = &mut self.edit
            && edit.dirty
        {
            edit.commit(backend);
        }
    }

    /// The delete-confirmation modal, shown while `pending_delete` is set.
    fn show_delete_modal(&mut self, ctx: &egui::Context, backend: &mut Backend) {
        let Some(id) = self.pending_delete else { return };
        let Some(engine) = backend.engines.iter().find(|e| e.id == id) else {
            self.pending_delete = None;
            return;
        };
        let name = widgets::engine_base_name(engine);
        let version = engine.meta.version.trim().to_string();
        let label = if version.is_empty() {
            name
        } else {
            format!("{name} {version}")
        };

        let mut confirmed = false;
        let mut cancelled = false;
        let modal = egui::Modal::new(egui::Id::new("engine_delete_confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(
                RichText::new(format!("Delete {label}?"))
                    .size(16.0)
                    .strong()
                    .color(theme::TEXT),
            );
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "This removes the engine from the library. The executable on disk \
                     is not touched.",
                )
                .color(theme::TEXT_WEAK)
                .size(12.5),
            );
            ui.add_space(14.0);
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::tinted_button(ui, "Delete", theme::DANGER, true).clicked() {
                    confirmed = true;
                }
                ui.add_space(4.0);
                if ui
                    .button(RichText::new("Cancel").color(theme::TEXT))
                    .clicked()
                {
                    cancelled = true;
                }
            });
        });
        if modal.should_close() || cancelled {
            self.pending_delete = None;
        }
        if confirmed {
            logo::remove(&backend.dirs.logos_dir(), id);
            backend.engines.retain(|e| e.id != id);
            backend.save_engines();
            if self.selected_id == Some(id) {
                self.selected_id = None;
                self.edit = None;
            }
            self.pending_delete = None;
        }
    }
}

// ── Detection jobs ────────────────────────────────────────────────────────────

/// A background add-single / folder-scan job.
struct DetectJob {
    rx: Receiver<(PathBuf, Result<DetectResult, String>)>,
    total: usize,
    done: usize,
    /// True for a single "Add Engine", false for a folder scan — decides
    /// which duplicate dialog is shown.
    single: bool,
}

impl EnginesTab {
    /// Drain the add/scan channel; create `EngineConfig`s for successes.
    fn poll_detect(&mut self, backend: &mut Backend) {
        if self.pending.is_none() {
            return;
        }

        let mut new_ids: Vec<EngineId> = Vec::new();
        let mut last_error: Option<String> = None;
        let mut disconnected = false;

        let single = self.pending.as_ref().is_some_and(|j| j.single);
        let mut duplicates: Vec<DupCandidate> = Vec::new();
        {
            let job = self.pending.as_mut().unwrap();
            loop {
                match job.rx.try_recv() {
                    Ok((path, Ok(result))) => {
                        job.done += 1;
                        let cfg = engine_from_detect(path, result);
                        match duplicate_of(&cfg, &backend.engines) {
                            Some(matches) => duplicates.push(DupCandidate { cfg, matches }),
                            None => {
                                let id = cfg.id;
                                backend.engines.push(cfg);
                                backend.save_engines();
                                new_ids.push(id);
                            }
                        }
                    }
                    Ok((path, Err(msg))) => {
                        job.done += 1;
                        let name = path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("?")
                            .to_string();
                        last_error = Some(format!("{name}: {msg}"));
                    }
                    Err(crossbeam_channel::TryRecvError::Empty) => break,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        if let Some(err) = last_error {
            self.detect_error = Some(err);
        }

        // Route duplicates to the appropriate dialog.
        for dup in duplicates {
            if single {
                self.dup_single = Some(dup);
            } else {
                self.dup_batch.push((dup, false));
            }
        }

        let done = self.pending.as_ref().map_or(0, |j| j.done);
        let total = self.pending.as_ref().map_or(0, |j| j.total);
        if disconnected || done >= total {
            self.pending = None;
            // Folder scan finished: raise the batch dialog if anything matched.
            if !self.dup_batch.is_empty() {
                self.dup_batch_open = true;
            }
        }

        for id in new_ids {
            if self.selected_id.is_none() {
                self.select_engine(id, backend);
            }
        }
    }

    /// The two duplicate dialogs: a Yes/No confirm for a single add, and a
    /// checkbox list (nothing ticked by default) for a folder scan.
    fn show_duplicate_modals(&mut self, ctx: &egui::Context, backend: &mut Backend) {
        // ── Single add ──
        if let Some(dup) = &self.dup_single {
            let title = widgets::engine_base_name(&dup.cfg);
            let version = dup.cfg.meta.version.trim().to_string();
            let file = dup
                .cfg
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string();
            let matches = dup.matches.clone();
            let mut decision: Option<bool> = None;

            let modal = egui::Modal::new(egui::Id::new("dup_single_modal")).show(ctx, |ui| {
                ui.set_width(420.0);
                ui.label(
                    RichText::new("Engine already in library")
                        .color(theme::TEXT)
                        .font(theme::semibold(15.0)),
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!(
                        "{title} {version} ({file}) matches \"{matches}\" in your library.",
                    ))
                    .color(theme::TEXT_WEAK)
                    .size(13.0),
                );
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Add anyway")
                                    .color(theme::BG_DARKEST)
                                    .font(theme::semibold(13.5)),
                            )
                            .fill(theme::ACCENT),
                        )
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    ui.add_space(4.0);
                    if ui
                        .button(RichText::new("Cancel").color(theme::TEXT))
                        .clicked()
                    {
                        decision = Some(false);
                    }
                });
            });
            if modal.should_close() && decision.is_none() {
                decision = Some(false);
            }
            if let Some(add) = decision {
                let dup = self.dup_single.take().unwrap();
                if add {
                    let id = dup.cfg.id;
                    backend.engines.push(dup.cfg);
                    backend.save_engines();
                    self.select_engine(id, backend);
                }
            }
        }

        // ── Folder scan ──
        if !self.dup_batch_open {
            return;
        }
        let mut close = false;
        let mut import = false;
        let modal = egui::Modal::new(egui::Id::new("dup_batch_modal")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.label(
                RichText::new(format!("Duplicates found ({})", self.dup_batch.len()))
                    .color(theme::TEXT)
                    .font(theme::semibold(15.0)),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "These detected engines match ones already in your library. \
                     Tick any you still want to import.",
                )
                .color(theme::TEXT_WEAK)
                .size(12.0),
            );
            ui.add_space(8.0);
            ScrollArea::vertical()
                .id_salt("dup_batch_scroll")
                .max_height(320.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for (dup, checked) in &mut self.dup_batch {
                        ui.horizontal(|ui| {
                            widgets::checkbox(ui, checked, "");
                            let name = widgets::engine_base_name(&dup.cfg);
                            let version = dup.cfg.meta.version.trim();
                            ui.label(
                                RichText::new(if version.is_empty() {
                                    name.clone()
                                } else {
                                    format!("{name} {version}")
                                })
                                .color(theme::TEXT)
                                .font(theme::semibold(13.0)),
                            );
                            ui.label(
                                RichText::new(format!("matches \"{}\"", dup.matches))
                                    .color(theme::TEXT_FAINT)
                                    .size(12.0),
                            );
                        })
                        .response
                        .on_hover_text(dup.cfg.path.to_string_lossy());
                        ui.add_space(2.0);
                    }
                });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("Select all").color(theme::TEXT_WEAK))
                    .clicked()
                {
                    for (_, checked) in &mut self.dup_batch {
                        *checked = true;
                    }
                }
                if ui
                    .button(RichText::new("Deselect all").color(theme::TEXT_WEAK))
                    .clicked()
                {
                    for (_, checked) in &mut self.dup_batch {
                        *checked = false;
                    }
                }
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    let n = self.dup_batch.iter().filter(|(_, c)| *c).count();
                    let label = if n == 0 {
                        "OK (skip all)".to_string()
                    } else {
                        format!("OK (import {n})")
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(label)
                                    .color(theme::BG_DARKEST)
                                    .font(theme::semibold(13.5)),
                            )
                            .fill(theme::ACCENT),
                        )
                        .clicked()
                    {
                        import = true;
                        close = true;
                    }
                });
            });
        });
        if modal.should_close() {
            close = true;
        }
        if close {
            if import {
                let mut first: Option<EngineId> = None;
                for (dup, checked) in self.dup_batch.drain(..) {
                    if checked {
                        first.get_or_insert(dup.cfg.id);
                        backend.engines.push(dup.cfg);
                    }
                }
                backend.save_engines();
                if let Some(id) = first
                    && self.selected_id.is_none()
                {
                    self.select_engine(id, backend);
                }
            } else {
                self.dup_batch.clear();
            }
            self.dup_batch_open = false;
        }
    }

    /// Drain the re-detect channel and update the edit buffer when done.
    fn poll_redetect(&mut self) {
        let (Some(rx), Some(for_id)) = (&self.redetect_rx, &self.redetect_for) else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(result)) => {
                if let Some(edit) = &mut self.edit
                    && &edit.engine_id == for_id
                {
                    edit.detected_options = result.options;
                    // Re-detect is an explicit "refresh identity" request, so
                    // detected values overwrite whatever is in the fields.
                    if let Some(id_name) = result.name {
                        let (name, version) = split_name_version(&id_name);
                        edit.name = name;
                        if let Some(version) = version {
                            edit.version = version;
                        }
                    }
                    if let Some(author) = result.author {
                        edit.author = author;
                    }
                    edit.redetect_pending = false;
                    edit.mark_dirty();
                }
                self.redetect_rx = None;
                self.redetect_for = None;
            }
            Ok(Err(e)) => {
                self.detect_error = Some(format!("Re-detect failed: {e}"));
                if let Some(edit) = &mut self.edit {
                    edit.redetect_pending = false;
                }
                self.redetect_rx = None;
                self.redetect_for = None;
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {}
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                if let Some(edit) = &mut self.edit {
                    edit.redetect_pending = false;
                }
                self.redetect_rx = None;
                self.redetect_for = None;
            }
        }
    }

    fn start_single(&mut self, path: PathBuf, backend: &Backend) {
        let (tx, rx) = crossbeam_channel::bounded(4);
        let p = path.clone();
        backend.runtime.spawn(async move {
            let res = detect_engine(&p).await.map_err(|e| e.to_string());
            let _ = tx.send((p, res));
        });
        self.pending = Some(DetectJob {
            rx,
            total: 1,
            done: 0,
            single: true,
        });
        self.detect_error = None;
    }

    fn start_folder(&mut self, dir: &Path, backend: &Backend) {
        let paths = find_executables(dir);
        if paths.is_empty() {
            self.detect_error = Some("No executable files found in that folder.".to_string());
            return;
        }
        let total = paths.len();
        let (tx, rx) = crossbeam_channel::bounded(total + 4);
        backend.runtime.spawn(async move {
            for path in paths {
                let res = detect_engine(&path).await.map_err(|e| e.to_string());
                if tx.send((path, res)).is_err() {
                    break;
                }
            }
        });
        self.pending = Some(DetectJob {
            rx,
            total,
            done: 0,
            single: false,
        });
        self.detect_error = None;
    }

    fn start_redetect(&mut self, path: PathBuf, engine_id: EngineId, backend: &Backend) {
        let (tx, rx) = crossbeam_channel::bounded(1);
        backend.runtime.spawn(async move {
            let res = detect_engine(&path).await.map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
        self.redetect_rx = Some(rx);
        self.redetect_for = Some(engine_id);
    }

    fn select_engine(&mut self, id: EngineId, backend: &mut Backend) {
        if self.selected_id == Some(id) {
            return;
        }
        // Commit any pending edit of the previously selected engine first.
        self.flush_edit(backend);
        if let Some(engine) = backend.engines.iter().find(|e| e.id == id) {
            self.selected_id = Some(id);
            self.edit = Some(EngineEditBuf::from_engine(engine));
        }
    }
}

// ── Toolbar ───────────────────────────────────────────────────────────────────

impl EnginesTab {
    fn show_toolbar(&mut self, ui: &mut Ui, backend: &mut Backend) {
        ui.horizontal(|ui| {
            let busy = self.pending.is_some();

            let add_resp = ui
                .add_enabled(
                    !busy,
                    egui::Button::new(
                        RichText::new("+ Add Engine")
                            .color(theme::BG_DARKEST)
                            .size(13.5)
                            .strong(),
                    )
                    .fill(theme::ACCENT),
                )
                .on_hover_text("Pick an engine executable and auto-detect its UCI options.");
            if add_resp.clicked() {
                let mut dialog = rfd::FileDialog::new().set_title("Select engine executable");
                if let Some(last) = &backend.config.last_engine_dir {
                    dialog = dialog.set_directory(last);
                }
                if let Some(path) = dialog.pick_file() {
                    if let Some(dir) = path.parent() {
                        backend.config.last_engine_dir = Some(dir.to_path_buf());
                    }
                    self.start_single(path, backend);
                }
            }

            ui.add_space(4.0);

            let folder_resp = ui
                .add_enabled(
                    !busy,
                    egui::Button::new(RichText::new("Scan Folder…").color(theme::TEXT).size(13.5))
                        .fill(theme::BG_ELEVATED)
                        .stroke(egui::Stroke::new(1.0, theme::STROKE)),
                )
                .on_hover_text(
                    "Scan a folder for engine executables and add those that respond to UCI.",
                );
            if folder_resp.clicked() {
                let mut dialog = rfd::FileDialog::new().set_title("Select folder of engines");
                if let Some(last) = &backend.config.last_engine_dir {
                    dialog = dialog.set_directory(last);
                }
                if let Some(dir) = dialog.pick_folder() {
                    backend.config.last_engine_dir = Some(dir.clone());
                    self.start_folder(&dir, backend);
                }
            }

            ui.add_space(16.0);

            if let Some(job) = &self.pending {
                let label = if job.total == 1 {
                    "⏳ Detecting…".to_string()
                } else {
                    format!("⏳ Scanning {}/{}…", job.done, job.total)
                };
                ui.label(RichText::new(label).color(theme::ACCENT).size(13.0));
            }

            if let Some(err) = self.detect_error.clone() {
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(RichText::new("×").color(theme::TEXT_WEAK)))
                        .clicked()
                    {
                        self.detect_error = None;
                    }
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("⚠ {err}"))
                                .color(theme::DANGER)
                                .size(13.0),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&err);
                });
            }
        });
    }
}

// ── Engine grid ─────────────────────────────────────────────────────────────

impl EnginesTab {
    fn show_grid(&mut self, ui: &mut Ui, backend: &mut Backend) {
        // Header: count + Delete All.
        let engine_count = backend.engines.len();
        let mut do_delete_all = false;
        let mut cancel_delete_all = false;
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{engine_count} engine{}",
                    if engine_count == 1 { "" } else { "s" }
                ))
                .color(theme::TEXT_WEAK)
                .size(12.0),
            );
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                // Sort selector (rightmost; persisted across sessions).
                if widgets::engine_sort_select(
                    ui,
                    "engines_sort_select",
                    &mut backend.config.engines_sort,
                ) {
                    backend.save_config();
                }
                ui.add_space(8.0);

                if engine_count > 0 {
                    if self.delete_all_confirm {
                        if widgets::tinted_button(ui, "Confirm", theme::DANGER, true)
                            .on_hover_text("Permanently remove every engine from the library.")
                            .clicked()
                        {
                            do_delete_all = true;
                        }
                        ui.add_space(2.0);
                        if ui
                            .add(egui::Button::new(RichText::new("Cancel").color(theme::TEXT_WEAK)))
                            .clicked()
                        {
                            cancel_delete_all = true;
                        }
                    } else if widgets::tinted_button(ui, "Delete All", theme::DANGER, true)
                        .on_hover_text("Remove all engines from the library.")
                        .clicked()
                    {
                        self.delete_all_confirm = true;
                    }
                }
            });
        });

        if do_delete_all {
            let logos_dir = backend.dirs.logos_dir();
            for e in &backend.engines {
                logo::remove(&logos_dir, e.id);
            }
            backend.engines.clear();
            backend.save_engines();
            self.selected_id = None;
            self.edit = None;
            self.delete_all_confirm = false;
        }
        if cancel_delete_all {
            self.delete_all_confirm = false;
        }

        ui.add_space(6.0);
        // Compact search field; the standard "×" clear button appears to its
        // right as soon as there is text (same control as the tablebase rows).
        ui.horizontal(|ui| {
            widgets::filter_field(ui, &mut self.filter_text, 280.0, "🔍 Filter engines…");
        });
        ui.add_space(10.0);

        let filter = self.filter_text.to_lowercase();
        let logos_dir = backend.dirs.logos_dir();

        ScrollArea::vertical()
            .id_salt("engines_grid_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if backend.engines.is_empty() {
                    ui.add_space(28.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("♟").color(theme::TEXT_FAINT).size(40.0));
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("No engines yet")
                                .color(theme::TEXT_WEAK)
                                .size(15.0)
                                .strong(),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Add an engine with the buttons above.")
                                .color(theme::TEXT_FAINT)
                                .size(12.5),
                        );
                    });
                    return;
                }

                // Filtered engine indices, sorted per the user's chosen order.
                let mut visible: Vec<usize> = backend
                    .engines
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| filter.is_empty() || widgets::engine_matches(e, &filter))
                    .map(|(i, _)| i)
                    .collect();
                widgets::sort_engine_indices(
                    &backend.engines,
                    &mut visible,
                    widgets::EngineSort::from_config(&backend.config.engines_sort),
                );

                if visible.is_empty() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No engines match your filter.")
                                .color(theme::TEXT_WEAK)
                                .size(13.0),
                        );
                    });
                    return;
                }

                // Responsive column count: spacious cards targeting ~360 px
                // (en-croissant-like: fewer, bigger cards; see GUIDELINES §3.9).
                let gap = 12.0;
                let avail_w = ui.available_width();
                let cols = (((avail_w + gap) / (360.0 + gap)).floor() as usize).clamp(1, 3);
                let card_w = ((avail_w - gap * (cols as f32 - 1.0)) / cols as f32).max(240.0);
                let card_h = 98.0;

                ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
                let mut clicked: Option<EngineId> = None;
                let mut action: Option<(EngineId, CardAction)> = None;
                for chunk in visible.chunks(cols) {
                    ui.horizontal(|ui| {
                        for &i in chunk {
                            let engine = &backend.engines[i];
                            let id = engine.id;
                            let path = engine.path.clone();
                            let resp = self.engine_card(ui, engine, &logos_dir, card_w, card_h);
                            // Right-click selects too, so the context menu's
                            // target is always the visible selection.
                            if resp.clicked() || resp.secondary_clicked() {
                                clicked = Some(id);
                            }
                            resp.context_menu(|ui| {
                                // Menus default to tight spacing; give the
                                // items the same breathing room as the rest
                                // of the UI.
                                ui.spacing_mut().item_spacing.y = 4.0;
                                ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                                if ui.button("Clone").clicked() {
                                    action = Some((id, CardAction::Clone));
                                    ui.close();
                                }
                                if ui.button("Re-detect").clicked() {
                                    action = Some((id, CardAction::Redetect));
                                    ui.close();
                                }
                                ui.separator();
                                if ui.button("Open containing folder").clicked() {
                                    if let Some(dir) = path.parent() {
                                        open_folder(dir);
                                    }
                                    ui.close();
                                }
                                if ui.button("Copy path").clicked() {
                                    ui.ctx().copy_text(path.to_string_lossy().to_string());
                                    ui.close();
                                }
                                ui.separator();
                                if ui
                                    .button(RichText::new("Delete Engine…").color(theme::DANGER))
                                    .clicked()
                                {
                                    action = Some((id, CardAction::Delete));
                                    ui.close();
                                }
                            });
                        }
                    });
                }
                if let Some(id) = clicked {
                    self.select_engine(id, backend);
                }
                if let Some((id, act)) = action {
                    self.handle_card_action(id, act, backend);
                }
            });
    }

    /// Apply a context-menu action chosen on an engine card.
    fn handle_card_action(&mut self, id: EngineId, action: CardAction, backend: &mut Backend) {
        match action {
            CardAction::Clone => self.clone_engine(id, backend),
            CardAction::Redetect => {
                self.select_engine(id, backend);
                if self.redetect_rx.is_none()
                    && let Some(edit) = &mut self.edit
                    && edit.engine_id == id
                    && !edit.redetect_pending
                {
                    edit.redetect_pending = true;
                    let path = edit.path.clone();
                    self.start_redetect(path, id, backend);
                }
            }
            CardAction::Delete => self.pending_delete = Some(id),
        }
    }

    /// Duplicate the engine `id` (including an independent copy of its logo)
    /// and select the new entry.
    fn clone_engine(&mut self, id: EngineId, backend: &mut Backend) {
        self.flush_edit(backend);
        let logos_dir = backend.dirs.logos_dir();
        let Some(src) = backend.engines.iter().find(|e| e.id == id) else {
            return;
        };
        let mut cloned = src.clone();
        cloned.id = colosseum_core::EngineId::new();
        // Copy the logo file so the clone owns an independent copy.
        if let Some(file) = cloned.meta.extra.get("logo").cloned() {
            match logo::import(&logos_dir, cloned.id, &logos_dir.join(&file)) {
                Ok(new_file) => {
                    cloned.meta.extra.insert("logo".into(), new_file);
                }
                Err(_) => {
                    cloned.meta.extra.remove("logo");
                }
            }
        }
        let suffix = " (copy)";
        if !cloned.meta.name.ends_with(suffix) {
            cloned.meta.name.push_str(suffix);
        }
        let new_id = cloned.id;
        backend.engines.push(cloned);
        backend.save_engines();
        self.select_engine(new_id, backend);
    }

    /// Draw one engine card at a fixed `w`×`h` size (so every card in the grid
    /// lines up exactly) and return its response: click = select, right-click
    /// opens the context menu built by the caller.
    fn engine_card(
        &mut self,
        ui: &mut Ui,
        engine: &EngineConfig,
        logos_dir: &Path,
        w: f32,
        h: f32,
    ) -> egui::Response {
        let is_sel = self.selected_id == Some(engine.id);
        let name = widgets::engine_base_name(engine);
        let version = engine.meta.version.trim().to_string();
        let subtitle = widgets::engine_subtitle(engine);
        let path_missing = !engine.path.exists();
        let logo_file = engine.meta.extra.get("logo").cloned();
        let elo = engine.meta.elo;

        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
        if !ui.is_rect_visible(rect) {
            return resp;
        }

        let fill = if is_sel {
            theme::tint(theme::ACCENT, 0.12)
        } else if resp.hovered() {
            theme::BG_HOVER
        } else {
            theme::BG_ELEVATED
        };
        let stroke = if is_sel {
            egui::Stroke::new(1.0, theme::tint(theme::ACCENT, 0.45))
        } else {
            egui::Stroke::new(1.0, theme::STROKE)
        };
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(8),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );

        // Interior (GUIDELINES §3.9): logo + name/subtitle on top, labeled
        // ELO / VERSION stat columns pinned to the bottom.
        let content = rect.shrink(12.0);

        let top = egui::Rect::from_min_size(content.min, egui::vec2(content.width(), 38.0));
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(top)
                .layout(Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 10.0;

        let (logo_rect, _) = logo::slot(&mut child, 36.0, Sense::hover());
        let drawn = logo_file.as_ref().is_some_and(|f| {
            logo::draw_fitted(&mut child, &mut self.logos, &logos_dir.join(f), logo_rect, 6)
        });
        if !drawn {
            widgets::draw_avatar_square_in(&child, logo_rect, &name, is_sel, 6);
        }

        child.vertical(|ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.add(
                    egui::Label::new(
                        RichText::new(&name)
                            .color(if is_sel {
                                theme::ACCENT_BRIGHT
                            } else {
                                theme::TEXT
                            })
                            .font(theme::semibold(15.0)),
                    )
                    .truncate(),
                );
                if path_missing {
                    ui.label(RichText::new("⚠").color(theme::WARN).size(13.0))
                        .on_hover_text("Executable not found at this path.");
                }
            });
            if !subtitle.is_empty() {
                ui.add(
                    egui::Label::new(RichText::new(&subtitle).color(theme::TEXT_WEAK).size(11.5))
                        .truncate(),
                );
            }
        });

        // Bottom stat columns: ELO (left) and VERSION (right-aligned).
        let stats = egui::Rect::from_min_max(
            egui::pos2(content.min.x, content.max.y - 27.0),
            content.max,
        );
        let mut srow = ui.new_child(
            UiBuilder::new()
                .max_rect(stats)
                .layout(Layout::left_to_right(egui::Align::TOP)),
        );
        let col_w = stats.width() * 0.5;
        let stat_value = |v: Option<String>| match v {
            Some(v) if !v.is_empty() => RichText::new(v)
                .color(theme::TEXT)
                .font(theme::semibold(13.5)),
            _ => RichText::new("—").color(theme::TEXT_FAINT).size(13.5),
        };
        srow.allocate_ui_with_layout(
            egui::vec2(col_w, stats.height()),
            Layout::top_down(egui::Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(RichText::new("ELO").color(theme::TEXT_FAINT).size(11.0));
                ui.label(stat_value(elo.map(|e| e.to_string())));
            },
        );
        srow.allocate_ui_with_layout(
            egui::vec2(srow.available_width(), stats.height()),
            Layout::top_down(egui::Align::Max),
            |ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(RichText::new("VERSION").color(theme::TEXT_FAINT).size(11.0));
                let ver = Some(version.clone()).filter(|v| !v.is_empty());
                ui.add(egui::Label::new(stat_value(ver)).truncate());
            },
        );

        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }
}

// ── Edit buffer ───────────────────────────────────────────────────────────────

struct EngineEditBuf {
    engine_id: EngineId,

    name: String,
    version: String,
    author: String,
    elo_str: String,
    /// Stored logo file name (under the GUI `logos/` dir), if any.
    logo: Option<String>,

    path: PathBuf,
    args_str: String,
    working_dir_str: String,

    env_rows: Vec<[String; 2]>,
    new_env_key: String,
    new_env_val: String,

    detected_options: Vec<UciOption>,
    option_overrides: BTreeMap<String, UciOptionValue>,

    dirty: bool,
    /// When the buffer first became dirty (starts the autosave debounce).
    dirty_since: Option<Instant>,
    /// When the last auto-commit happened (drives the "✓ saved" flash).
    saved_at: Option<Instant>,
    redetect_pending: bool,
}

impl EngineEditBuf {
    fn from_engine(e: &EngineConfig) -> Self {
        Self {
            engine_id: e.id,
            name: e.meta.name.clone(),
            version: e.meta.version.clone(),
            author: e.meta.extra.get("author").cloned().unwrap_or_default(),
            elo_str: e.meta.elo.map(|v| v.to_string()).unwrap_or_default(),
            logo: e.meta.extra.get("logo").cloned(),
            path: e.path.clone(),
            args_str: e.args.join(" "),
            working_dir_str: e
                .working_dir
                .as_deref()
                .and_then(|p| p.to_str())
                .unwrap_or("")
                .to_string(),
            env_rows: e.env.iter().map(|(k, v)| [k.clone(), v.clone()]).collect(),
            new_env_key: String::new(),
            new_env_val: String::new(),
            detected_options: e.detected_options.clone(),
            option_overrides: e.options.clone(),
            dirty: false,
            dirty_since: None,
            saved_at: None,
            redetect_pending: false,
        }
    }

    /// Flag the buffer as changed and restart the autosave debounce window.
    fn mark_dirty(&mut self) {
        self.dirty = true;
        self.dirty_since = Some(Instant::now());
    }

    /// Write the buffer back to the matching engine and persist.
    fn commit(&mut self, backend: &mut Backend) {
        let Some(engine) = backend.engines.iter_mut().find(|e| e.id == self.engine_id) else {
            return;
        };
        engine.meta.name = self.name.clone();
        engine.meta.version = self.version.clone();
        set_or_remove(&mut engine.meta.extra, "author", self.author.trim());
        match self.logo.as_deref() {
            Some(l) if !l.trim().is_empty() => {
                engine.meta.extra.insert("logo".into(), l.to_string());
            }
            _ => {
                engine.meta.extra.remove("logo");
            }
        }
        engine.meta.elo = self.elo_str.trim().parse::<i32>().ok();
        engine.args = self
            .args_str
            .split_whitespace()
            .map(str::to_string)
            .collect();
        engine.working_dir = if self.working_dir_str.trim().is_empty() {
            None
        } else {
            Some(PathBuf::from(self.working_dir_str.trim()))
        };
        engine.env = self
            .env_rows
            .iter()
            .filter(|[k, _]| !k.trim().is_empty())
            .map(|[k, v]| (k.clone(), v.clone()))
            .collect();
        engine.options = self.option_overrides.clone();
        engine.detected_options = self.detected_options.clone();
        backend.save_engines();
        self.dirty = false;
        self.dirty_since = None;
        self.saved_at = Some(Instant::now());
    }
}

// ── Engine panel (top-right) ──────────────────────────────────────────────────

impl EnginesTab {
    fn show_engine_panel(&mut self, ui: &mut Ui, backend: &mut Backend) {
        let Some(mut edit) = self.edit.take() else {
            return;
        };
        let logos_dir = backend.dirs.logos_dir();

        let mut do_delete = false;
        let mut do_clone = false;
        let mut do_redetect = false;
        let mut pick_logo = false;
        let mut clear_logo = false;
        let can_redetect = !edit.redetect_pending && self.redetect_rx.is_none();

        // Pinned action row at the bottom of the panel, so it stays visible
        // no matter how small the window gets. Changes save automatically, so
        // the row only carries Clone and Delete.
        egui::Panel::bottom("engine_actions_panel")
            .resizable(false)
            .exact_size(48.0)
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 2, // room for focus/hover rings at the panel edge
                top: 8,
                bottom: 10,
                ..Default::default()
            }))
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .button(RichText::new("Clone").color(theme::TEXT_WEAK).size(13.0))
                        .on_hover_text("Duplicate this engine entry with a new identity.")
                        .clicked()
                    {
                        do_clone = true;
                    }
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::tinted_button(ui, "Delete Engine", theme::DANGER, true)
                            .clicked()
                        {
                            do_delete = true;
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| {
                // The whole detail column scrolls (header included), so small
                // windows can still reach every field.
                ScrollArea::vertical()
                    .id_salt("engine_detail_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.engine_header(
                            ui,
                            &mut edit,
                            &logos_dir,
                            &mut pick_logo,
                            &mut clear_logo,
                        );
                        ui.add_space(8.0);
                        launch_section(ui, &mut edit);
                        ui.add_space(8.0);
                        uci_options_section(ui, &mut edit, can_redetect, &mut do_redetect);
                        // Breathing room between the last option row and the
                        // pinned action row / section divider below.
                        ui.add_space(14.0);
                    });
            });

        // ── Deferred actions ──
        if pick_logo {
            let mut dlg = rfd::FileDialog::new().set_title("Choose engine logo").add_filter(
                "Images",
                &["png", "jpg", "jpeg", "webp", "bmp", "gif", "ico"],
            );
            if let Some(last) = &backend.config.last_engine_dir {
                dlg = dlg.set_directory(last);
            }
            if let Some(src) = dlg.pick_file() {
                match logo::import(&logos_dir, edit.engine_id, &src) {
                    Ok(file) => {
                        edit.logo = Some(file);
                        edit.mark_dirty();
                    }
                    Err(e) => self.detect_error = Some(format!("Logo import failed: {e}")),
                }
            }
        }
        if clear_logo {
            logo::remove(&logos_dir, edit.engine_id);
            edit.logo = None;
            edit.mark_dirty();
        }

        if do_delete {
            self.pending_delete = Some(edit.engine_id);
        }

        if do_redetect {
            let path = edit.path.clone();
            let id = edit.engine_id;
            edit.redetect_pending = true;
            self.start_redetect(path, id, backend);
        }

        let engine_id = edit.engine_id;
        self.edit = Some(edit);

        if do_clone {
            self.clone_engine(engine_id, backend);
        }
    }

    /// The identity header: editable name/version/author/elo on the left, a
    /// clickable logo box on the right.
    fn engine_header(
        &mut self,
        ui: &mut Ui,
        edit: &mut EngineEditBuf,
        logos_dir: &Path,
        pick_logo: &mut bool,
        clear_logo: &mut bool,
    ) {
        let header_name = if edit.name.trim().is_empty() {
            edit.path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("?")
                .to_string()
        } else {
            edit.name.trim().to_string()
        };

        // Fixed-geometry header: the identity fields and the logo slot sit at
        // the same place with the same size for every engine, so switching
        // between engines never makes the layout jump. Only the logo *image*
        // varies in size (aspect-fitted, centered in its slot).
        const HEADER_H: f32 = 176.0;
        const LOGO_MAX_W: f32 = 220.0;
        const LOGO_MAX_H: f32 = 128.0;
        const REMOVE_ROW_H: f32 = 26.0;

        let avail = ui.available_width();
        let (header_rect, _) =
            ui.allocate_exact_size(egui::vec2(avail, HEADER_H), Sense::hover());
        let id_width = (avail * 0.5).max(240.0).min((avail - 96.0).max(160.0));

        // ── Identity column (fixed position, fixed rows) ──
        let id_rect = egui::Rect::from_min_size(header_rect.min, egui::vec2(id_width, HEADER_H));
        let mut idui = ui.new_child(
            UiBuilder::new()
                .max_rect(id_rect)
                .layout(Layout::top_down(egui::Align::Min)),
        );
        {
            let ui = &mut idui;
                ui.set_width(id_width);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 7.0;
                    ui.label(
                        RichText::new(&header_name)
                            .font(theme::semibold(18.0))
                            .color(theme::TEXT),
                    );
                    if !edit.version.trim().is_empty() {
                        widgets::chip(ui, edit.version.trim(), theme::ACCENT);
                    }
                    if !edit.dirty
                        && edit
                            .saved_at
                            .is_some_and(|t| t.elapsed() < SAVED_FLASH)
                    {
                        ui.label(
                            RichText::new("saved")
                                .color(theme::SUCCESS)
                                .size(11.5)
                                .italics(),
                        );
                    }
                    if edit.redetect_pending {
                        ui.label(
                            RichText::new("⏳ detecting…")
                                .color(theme::ACCENT)
                                .size(11.5),
                        );
                    }
                });
                ui.add_space(6.0);

                egui::Grid::new("identity_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        field_label(ui, "Name");
                        if ui
                            .add(text_field(&mut edit.name).hint_text("e.g. Stockfish"))
                            .changed()
                        {
                            edit.mark_dirty();
                        }
                        ui.end_row();

                        field_label(ui, "Version");
                        if ui
                            .add(text_field(&mut edit.version).hint_text("e.g. 16.1"))
                            .changed()
                        {
                            edit.mark_dirty();
                        }
                        ui.end_row();

                        field_label(ui, "Author");
                        if ui
                            .add(text_field(&mut edit.author).hint_text("from the engine's UCI id"))
                            .changed()
                        {
                            edit.mark_dirty();
                        }
                        ui.end_row();

                        field_label(ui, "Elo");
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut edit.elo_str)
                                    .desired_width(90.0)
                                    .hint_text("optional"),
                            )
                            .changed()
                        {
                            edit.mark_dirty();
                        }
                        ui.end_row();
                    });
        }

        // ── Logo slot: a fixed region right of the identity column. The
        // image is aspect-fitted (up to 220×128) and centered on a constant
        // point; the Remove row is always reserved so the image doesn't move
        // when the button appears/disappears. ──
        let logo_region = egui::Rect::from_min_max(
            egui::pos2(id_rect.max.x + 16.0, header_rect.min.y),
            header_rect.max,
        );
        let max_w = (logo_region.width() - 8.0).clamp(48.0, LOGO_MAX_W);
        let max_h = LOGO_MAX_H.min(HEADER_H - REMOVE_ROW_H - 4.0);
        let (box_w, box_h) = match edit
            .logo
            .as_ref()
            .and_then(|f| self.logos.natural_size(&logos_dir.join(f)))
        {
            Some(nat) => {
                let scale = (max_w / nat.x).min(max_h / nat.y);
                ((nat.x * scale).round(), (nat.y * scale).round())
            }
            None => {
                let s = max_w.min(max_h);
                (s, s)
            }
        };
        let stack_h = max_h + REMOVE_ROW_H; // constant → constant center point
        let stack_top = logo_region.min.y + ((HEADER_H - stack_h) / 2.0).max(0.0);
        let logo_rect = egui::Rect::from_min_size(
            egui::pos2(
                (logo_region.center().x - box_w / 2.0).max(logo_region.min.x),
                stack_top + (max_h - box_h) / 2.0,
            ),
            egui::vec2(box_w, box_h),
        );

        let resp = ui
            .interact(
                logo_rect,
                egui::Id::new("engine_logo_slot").with(edit.engine_id),
                Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Click to choose a logo image");
        let drawn = edit.logo.as_ref().is_some_and(|f| {
            logo::draw_fitted(ui, &mut self.logos, &logos_dir.join(f), logo_rect, 8)
        });
        if drawn {
            ui.painter().rect_stroke(
                logo_rect,
                egui::CornerRadius::same(8),
                egui::Stroke::new(1.0, theme::STROKE),
                egui::StrokeKind::Inside,
            );
        } else {
            widgets::draw_avatar_square_in(ui, logo_rect, &header_name, true, 8);
        }
        if resp.clicked() {
            *pick_logo = true;
        }
        if edit.logo.is_some() {
            let btn_rect = egui::Rect::from_center_size(
                egui::pos2(
                    logo_region.center().x,
                    stack_top + max_h + REMOVE_ROW_H / 2.0,
                ),
                egui::vec2(80.0, 22.0),
            );
            if ui
                .put(
                    btn_rect,
                    egui::Button::new(
                        RichText::new("Remove").color(theme::TEXT_WEAK).size(11.0),
                    ),
                )
                .clicked()
            {
                *clear_logo = true;
            }
        }
    }
}

/// Collapsible launch/environment editor (path, args, working dir, env vars).
fn launch_section(ui: &mut Ui, edit: &mut EngineEditBuf) {
    egui::CollapsingHeader::new(RichText::new("Launch & environment").size(13.0).strong())
        .id_salt("launch_section")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("edit_launch")
                .num_columns(2)
                .spacing([10.0, 6.0])
                .show(ui, |ui| {
                    field_label(ui, "Path");
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(folder) = edit.path.parent() {
                            let folder = folder.to_path_buf();
                            if ui
                                .add(egui::Button::new(RichText::new("Open folder").color(theme::TEXT_WEAK)))
                                .on_hover_text("Open the folder containing this engine.")
                                .clicked()
                            {
                                open_folder(&folder);
                            }
                        }
                        let path_missing = !edit.path.exists();
                        ui.allocate_ui_with_layout(
                            egui::vec2(ui.available_width(), ui.spacing().interact_size.y),
                            Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                if path_missing {
                                    ui.label(RichText::new("⚠").color(theme::WARN).size(13.0))
                                        .on_hover_text("Executable not found at this path.");
                                }
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(edit.path.to_string_lossy())
                                            .color(if path_missing {
                                                theme::WARN
                                            } else {
                                                theme::TEXT_WEAK
                                            })
                                            .size(12.0)
                                            .monospace(),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(edit.path.to_string_lossy());
                            },
                        );
                    });
                    ui.end_row();

                    field_label(ui, "Args");
                    if ui
                        .add(
                            text_field(&mut edit.args_str)
                                .hint_text("extra arguments (space-separated)"),
                        )
                        .changed()
                    {
                        edit.mark_dirty();
                    }
                    ui.end_row();

                    field_label(ui, "Work dir");
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        let browse_clicked = ui
                            .add(egui::Button::new(RichText::new("Browse…").color(theme::TEXT_WEAK)))
                            .clicked();
                        if ui
                            .add(
                                text_field(&mut edit.working_dir_str)
                                    .hint_text("defaults to engine directory"),
                            )
                            .changed()
                        {
                            edit.mark_dirty();
                        }
                        if browse_clicked
                            && let Some(dir) = rfd::FileDialog::new()
                                .set_title("Select working directory")
                                .pick_folder()
                        {
                            edit.working_dir_str = dir.to_string_lossy().to_string();
                            edit.mark_dirty();
                        }
                    });
                    ui.end_row();
                });

            ui.add_space(6.0);
            ui.label(
                RichText::new("Environment variables")
                    .color(theme::TEXT_WEAK)
                    .size(12.0)
                    .strong(),
            );
            ui.add_space(4.0);

            let mut remove_idx: Option<usize> = None;
            let mut env_changed = false;
            for (i, row) in edit.env_rows.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut row[0])
                                .desired_width(150.0)
                                .hint_text("KEY"),
                        )
                        .changed()
                    {
                        env_changed = true;
                    }
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new(RichText::new("×").color(theme::DANGER)))
                            .clicked()
                        {
                            remove_idx = Some(i);
                        }
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut row[1])
                                    .desired_width(f32::INFINITY)
                                    .hint_text("value"),
                            )
                            .changed()
                        {
                            env_changed = true;
                        }
                    });
                });
            }
            if let Some(i) = remove_idx {
                edit.env_rows.remove(i);
                env_changed = true;
            }
            if env_changed {
                edit.mark_dirty();
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut edit.new_env_key)
                        .desired_width(150.0)
                        .hint_text("NEW KEY"),
                );
                let key_nonempty = !edit.new_env_key.trim().is_empty();
                let mut do_add = false;
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            key_nonempty,
                            egui::Button::new(RichText::new("+").color(theme::ACCENT)),
                        )
                        .clicked()
                    {
                        do_add = true;
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut edit.new_env_val)
                            .desired_width(f32::INFINITY)
                            .hint_text("value"),
                    );
                });
                if do_add {
                    edit.env_rows.push([
                        std::mem::take(&mut edit.new_env_key),
                        std::mem::take(&mut edit.new_env_val),
                    ]);
                    edit.mark_dirty();
                }
            });
        });
}

/// The per-engine UCI options editor (condensed). Thread/hash/tablebase-path
/// options are filtered out — those are managed globally / by the Tournament tab.
fn uci_options_section(
    ui: &mut Ui,
    edit: &mut EngineEditBuf,
    can_redetect: bool,
    do_redetect: &mut bool,
) {
    let shown: Vec<UciOption> = edit
        .detected_options
        .iter()
        .filter(|o| !is_globally_managed_option(o.name()))
        .cloned()
        .collect();
    let hidden = edit.detected_options.len() - shown.len();

    ui.horizontal(|ui| {
        ui.label(
            RichText::new("UCI Options")
                .color(theme::TEXT)
                .font(theme::semibold(13.0)),
        );
        if hidden > 0 {
            ui.label(
                RichText::new(format!("· {hidden} managed elsewhere"))
                    .color(theme::TEXT_FAINT)
                    .size(11.0),
            )
            .on_hover_text(
                "Threads, Hash and Ponder are set per tournament in the Tournament tab; \
                 tablebase paths and probe caches in the Endgame Tablebases panel below.",
            );
        }
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    can_redetect,
                    egui::Button::new(
                        RichText::new(if edit.redetect_pending {
                            "Detecting…"
                        } else {
                            "Re-detect"
                        })
                        .size(12.0)
                        .color(theme::TEXT_WEAK),
                    )
                    .fill(theme::BG_ELEVATED)
                    .stroke(egui::Stroke::new(1.0, theme::STROKE)),
                )
                .on_hover_text("Re-run the UCI handshake to refresh options and identity.")
                .clicked()
            {
                *do_redetect = true;
            }
            if !edit.option_overrides.is_empty()
                && ui
                    .add(egui::Button::new(RichText::new("Reset all").color(theme::TEXT_WEAK)))
                    .on_hover_text("Remove all option overrides, reverting to engine defaults.")
                    .clicked()
            {
                edit.option_overrides.clear();
                edit.mark_dirty();
            }
        });
    });

    if shown.is_empty() {
        ui.add_space(4.0);
        ui.label(
            RichText::new(if edit.detected_options.is_empty() {
                "No options detected yet — use Re-detect to query the engine."
            } else {
                "No engine-specific options (all are managed globally)."
            })
            .color(theme::TEXT_WEAK)
            .size(12.0)
            .italics(),
        );
        return;
    }

    ui.add_space(6.0);
    let mut changed = false;

    // Lay the options out in 1–3 columns depending on available width, so a
    // wide panel doesn't leave a single skinny column with acres of empty
    // space. The column count is driven by the *widest actual row* (long
    // option names and range hints vary wildly between engines), so columns
    // never collide. Short lists stay single-column (≥ ~5 rows per column).
    let needed = shown
        .iter()
        .map(|o| option_row_width(ui, o))
        .fold(320.0_f32, f32::max);
    let by_width = ((ui.available_width() + 24.0) / (needed + 24.0)).floor() as usize;
    let by_rows = shown.len().div_ceil(5).max(1);
    let n_cols = by_width.clamp(1, 3).min(by_rows);
    let per_col = shown.len().div_ceil(n_cols);

    if n_cols == 1 {
        options_grid(ui, &shown, edit, &mut changed, 0);
    } else {
        ui.columns(n_cols, |cols| {
            for (ci, chunk) in shown.chunks(per_col).enumerate() {
                options_grid(&mut cols[ci], chunk, edit, &mut changed, ci);
            }
        });
    }
    if changed {
        edit.mark_dirty();
    }
}

/// Estimated on-screen width of one option row (label + editor + range hint +
/// reset column + grid spacing), used to pick a collision-free column count.
fn option_row_width(ui: &Ui, opt: &UciOption) -> f32 {
    let text_w = |s: &str, size: f32| {
        ui.painter()
            .layout_no_wrap(
                s.to_owned(),
                egui::FontId::proportional(size),
                egui::Color32::WHITE,
            )
            .size()
            .x
    };
    let label_w = text_w(opt.name(), 13.0);
    let value_w = match opt {
        UciOption::Spin { min, max, .. } => {
            56.0 + 10.0 + text_w(&format!("({min}–{max})"), 11.5)
        }
        UciOption::Combo { .. } => 200.0,
        UciOption::Str { .. } => 240.0,
        UciOption::Check { .. } => 24.0,
        UciOption::Button { .. } => 150.0,
    };
    label_w + value_w + 20.0 + 40.0 // reset column + inter-cell spacing
}

/// One column of the UCI options editor: a grid of (label | editor | reset)
/// rows. Sets `changed` when any value was modified. Content is clipped
/// **horizontally** to the column so an extreme row can never paint over its
/// neighbor — vertical clipping must stay untouched or scroll areas break
/// (the grid lives inside one, so its vertical extent is unbounded).
fn options_grid(
    ui: &mut Ui,
    opts: &[UciOption],
    edit: &mut EngineEditBuf,
    changed: &mut bool,
    salt: usize,
) {
    let mut clip = ui.clip_rect();
    clip.min.x = clip.min.x.max(ui.max_rect().min.x - 2.0);
    clip.max.x = clip.max.x.min(ui.max_rect().max.x + 2.0);
    ui.set_clip_rect(clip);
    egui::Grid::new(("uci_opts_grid", salt))
        .num_columns(3)
        .spacing([10.0, 7.0])
        .show(ui, |ui| {
            for opt in opts {
                widgets::uci_option_row(ui, opt, &mut edit.option_overrides, changed);
                if edit.option_overrides.contains_key(opt.name()) {
                    if ui
                        .add(egui::Button::new(RichText::new("×").color(theme::TEXT_FAINT)))
                        .on_hover_text("Reset to engine default.")
                        .clicked()
                    {
                        edit.option_overrides.remove(opt.name());
                        *changed = true;
                    }
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
}

// ── Global tablebases (bottom-right) ──────────────────────────────────────────

/// Global endgame-tablebase directories, shared by every engine. Stored in
/// [`AppConfig`] (not per-engine) and applied at tournament start.
///
/// Rendered as a collapsible card pinned to the bottom of the right column:
/// collapsed it is a one-line header with a per-format set/unset summary, so
/// the rarely-touched paths don't hold vertical space hostage.
fn show_global_tablebases(ui: &mut Ui, backend: &mut Backend, expanded: &mut bool) {
    egui::Frame::new()
        .fill(theme::BG_ELEVATED)
        .stroke(egui::Stroke::new(1.0, theme::STROKE))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // Clickable header row toggles the panel.
            let header = ui
                .horizontal(|ui| {
                    widgets::disclosure_triangle(ui, *expanded, theme::TEXT_FAINT);
                    ui.label(
                        RichText::new("Endgame Tablebases")
                            .color(theme::TEXT)
                            .font(theme::semibold(14.0)),
                    );
                    ui.label(
                        RichText::new("· shared by all engines")
                            .color(theme::TEXT_FAINT)
                            .size(11.5),
                    );
                    // Right side: per-format set/unset summary. Skipped when
                    // the panel is too narrow for it to fit without colliding
                    // with the title.
                    if ui.available_width() > 240.0 {
                        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                            for (label, value) in [
                                ("Gaviota", &backend.config.gaviota_path),
                                ("Nalimov", &backend.config.nalimov_path),
                                ("Syzygy", &backend.config.syzygy_path),
                            ] {
                                let set =
                                    value.as_deref().is_some_and(|s| !s.trim().is_empty());
                                let (mark, color) = if set {
                                    ("●", theme::SUCCESS)
                                } else {
                                    ("○", theme::TEXT_FAINT)
                                };
                                ui.label(RichText::new(mark).color(color).size(10.0));
                                ui.label(
                                    RichText::new(label).color(theme::TEXT_WEAK).size(11.5),
                                );
                                ui.add_space(6.0);
                            }
                        });
                    }
                })
                .response;
            let click = ui.interact(
                header.rect,
                egui::Id::new("tb_panel_header"),
                Sense::click(),
            );
            if click.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if click.clicked() {
                *expanded = !*expanded;
            }

            if !*expanded {
                return;
            }
            ui.add_space(8.0);

            let mut changed = false;
            egui::Grid::new("global_tb_grid")
                .num_columns(2)
                .spacing([10.0, 10.0])
                .show(ui, |ui| {
                    // NB: extras draw inside a right-to-left layout — add
                    // controls in reverse visual order.
                    let cfg = &mut backend.config;
                    changed |= tablebase_row(ui, "Syzygy", &mut cfg.syzygy_path, |ui| {
                        let mut ch = widgets::checkbox(ui, &mut cfg.syzygy_50_move_rule, "50-move rule")
                            .on_hover_text(
                                "Tablebase scores respect the 50-move rule (FIDE-correct). \
                                 Off counts \"cursed\" wins as wins.",
                            )
                            .changed();
                        ui.add_space(8.0);
                        ch |= ui
                            .add(
                                DragValue::new(&mut cfg.syzygy_probe_limit)
                                    .range(3..=7)
                                    .speed(0.1),
                            )
                            .on_hover_text(
                                "Probe positions with up to this many pieces — match the \
                                 tablebase files you actually have (5/6/7-man).",
                            )
                            .changed();
                        ui.label(
                            RichText::new("Probe limit").color(theme::TEXT_FAINT).size(11.5),
                        );
                        ch
                    });
                    changed |= tablebase_row(ui, "Nalimov", &mut cfg.nalimov_path, |ui| {
                        cache_extra(ui, "Nalimov", &mut cfg.nalimov_cache_mb)
                    });
                    changed |= tablebase_row(ui, "Gaviota", &mut cfg.gaviota_path, |ui| {
                        let mut ch = false;
                        widgets::select(
                            ui,
                            "gaviota_compression",
                            &cfg.gaviota_compression.clone(),
                            64.0,
                            |ui| {
                                for scheme in ["cp0", "cp1", "cp2", "cp3", "cp4"] {
                                    if ui
                                        .selectable_label(
                                            cfg.gaviota_compression == scheme,
                                            scheme,
                                        )
                                        .clicked()
                                    {
                                        cfg.gaviota_compression = scheme.to_string();
                                        ch = true;
                                        ui.close();
                                    }
                                }
                            },
                        );
                        ui.label(
                            RichText::new("Compression").color(theme::TEXT_FAINT).size(11.5),
                        )
                        .on_hover_text(
                            "Compression scheme of the Gaviota files on disk (cp4 is the \
                             common download).",
                        );
                        ui.add_space(8.0);
                        ch |= cache_extra(ui, "Gaviota", &mut cfg.gaviota_cache_mb);
                        ch
                    });
                });
            if changed {
                backend.save_config();
            }
        });
}

/// One tablebase path row: label + folder field + clear + per-format extras +
/// Browse. `extras` draws the format-specific settings (cache size, probe
/// limit, 50-move rule, compression) and returns `true` when one changed.
/// Returns `true` when any value changed.
fn tablebase_row(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<String>,
    extras: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    field_label(ui, label);
    let mut s = value.clone().unwrap_or_default();
    let mut changed = false;
    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
        if ui
            .add(egui::Button::new(RichText::new("Browse…").color(theme::TEXT_WEAK)))
            .clicked()
            && let Some(dir) = rfd::FileDialog::new()
                .set_title(format!("Select {label} tablebase folder"))
                .pick_folder()
        {
            s = dir.to_string_lossy().to_string();
            changed = true;
        }
        ui.add_space(10.0);
        changed |= extras(ui);
        ui.add_space(4.0);
        if !s.is_empty()
            && widgets::clear_button(ui)
                .on_hover_text("Clear this path.")
                .clicked()
        {
            s.clear();
            changed = true;
        }
        if ui
            .add(
                egui::TextEdit::singleline(&mut s)
                    .desired_width(f32::INFINITY)
                    .hint_text("tablebase folder"),
            )
            .changed()
        {
            changed = true;
        }
    });
    if changed {
        *value = if s.trim().is_empty() {
            None
        } else {
            Some(s.trim().to_string())
        };
    }
    ui.end_row();
    changed
}

/// A probe-cache size control (label + MB drag), used by the Nalimov and
/// Gaviota rows. Remember: this draws inside a right-to-left layout, so add
/// items in reverse visual order.
fn cache_extra(ui: &mut Ui, label: &str, mb: &mut u32) -> bool {
    ui.label(RichText::new("MB").color(theme::TEXT_FAINT).size(11.5));
    let changed = ui
        .add(DragValue::new(mb).range(1..=1024).speed(1.0))
        .on_hover_text(format!(
            "Probe-cache size forwarded to engines' {label} cache option."
        ))
        .changed();
    ui.label(RichText::new("Cache").color(theme::TEXT_FAINT).size(11.5));
    changed
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Whether a UCI option is managed outside the per-engine editor: thread/CPU
/// count, hash and Ponder (Tournament tab — the scheduler always forwards
/// them, so a per-engine value would be silently overridden) or an
/// endgame-tablebase *path* / *probe-cache size* (global Tablebases panel).
fn is_globally_managed_option(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let threads = n.contains("thread") || n.contains("cpu") || n == "cores" || n == "core";
    let hash = n == "hash" || n == "hash size" || n == "hashsize" || n == "memory";
    let ponder = n == "ponder";
    let tb = n.contains("syzygy") || n.contains("gaviota") || n.contains("nalimov");
    let tb_cfg = tb
        && (n.contains("path")
            || n.contains("cache")
            || n.contains("50move")
            || n.contains("probelimit")
            || n.contains("compression"));
    threads || hash || ponder || tb_cfg
}

/// Insert `value` under `key` (trimmed) or remove the key when empty.
fn set_or_remove(map: &mut BTreeMap<String, String>, key: &str, value: &str) {
    if value.trim().is_empty() {
        map.remove(key);
    } else {
        map.insert(key.to_string(), value.trim().to_string());
    }
}

/// Create an `EngineConfig` from detection results (not yet in the library).
fn engine_from_detect(path: PathBuf, result: DetectResult) -> EngineConfig {
    let mut cfg = EngineConfig::new(path);
    match result.name {
        Some(id_name) => {
            let (name, version) = split_name_version(&id_name);
            cfg.meta.name = name;
            if let Some(version) = version {
                cfg.meta.version = version;
            }
        }
        None => {
            cfg.meta.name = cfg
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Unknown")
                .to_string();
        }
    }
    if let Some(author) = result.author {
        cfg.meta.extra.insert("author".to_string(), author);
    }
    cfg.detected_options = result.options;
    cfg
}

/// If `cfg` duplicates a library engine, return that engine's display name.
///
/// A duplicate is the same executable path, or the same detected identity
/// (name + version, case-insensitive; never matched on an empty name).
fn duplicate_of(cfg: &EngineConfig, library: &[EngineConfig]) -> Option<String> {
    let norm_path = |p: &Path| p.to_string_lossy().to_lowercase();
    let name = cfg.meta.name.trim().to_lowercase();
    let version = cfg.meta.version.trim().to_lowercase();
    library
        .iter()
        .find(|e| {
            norm_path(&e.path) == norm_path(&cfg.path)
                || (!name.is_empty()
                    && e.meta.name.trim().to_lowercase() == name
                    && e.meta.version.trim().to_lowercase() == version)
        })
        .map(|e| {
            let n = widgets::engine_base_name(e);
            let v = e.meta.version.trim();
            if v.is_empty() {
                n
            } else {
                format!("{n} {v}")
            }
        })
}

/// Enumerate executable files (non-recursive) in `dir`.
fn find_executables(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_executable(p))
        .collect();
    paths.sort();
    paths
}

fn is_executable(path: &Path) -> bool {
    #[cfg(windows)]
    {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
}

/// Dim label for a grid row's left column.
fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::TEXT_WEAK).size(13.0));
}

/// A `TextEdit::singleline` filling available width.
fn text_field(buf: &mut String) -> egui::TextEdit<'_> {
    egui::TextEdit::singleline(buf).desired_width(f32::INFINITY)
}

/// Open the system file manager to the given folder.
fn open_folder(path: &std::path::Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

/// Placeholder shown in the right pane when no engine is selected.
fn empty_state(ui: &mut Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(72.0);
        ui.label(RichText::new("♟").color(theme::TEXT_FAINT).size(40.0));
        ui.add_space(8.0);
        ui.label(
            RichText::new("No engine selected")
                .color(theme::TEXT_WEAK)
                .size(15.0)
                .strong(),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Pick an engine from the grid, or add one with the buttons above.")
                .color(theme::TEXT_FAINT)
                .size(12.5),
        );
    });
}
