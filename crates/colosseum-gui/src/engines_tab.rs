// SPDX-License-Identifier: GPL-3.0-or-later
//! Engine Management tab: add, scan, auto-detect, edit, and delete engines.
//!
//! The tab is a two-pane split — a resizable engine list on the left and an
//! edit panel on the right.  Adding an engine (single file or folder scan)
//! spawns a background detection task on the tokio runtime so the GUI thread
//! never blocks.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use crossbeam_channel::Receiver;
use eframe::egui::{self, Color32, DragValue, Layout, RichText, ScrollArea, Ui};

use colosseum_core::{EngineConfig, EngineId, UciOption, UciOptionValue};
use colosseum_engine::{DetectResult, detect_engine, split_name_version};

use crate::backend::Backend;
use crate::theme;
use crate::widgets;

// ── Public tab state ─────────────────────────────────────────────────────────

/// All persistent state for the Engine Management tab.
#[derive(Default)]
pub struct EnginesTab {
    selected_id: Option<EngineId>,
    /// Engines checked for bulk delete.
    selected_ids: HashSet<EngineId>,
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
}

impl EnginesTab {
    /// Draw the full tab body.  Call every frame.
    pub fn show(&mut self, ui: &mut Ui, backend: &mut Backend) {
        // Poll any background work first so results are visible this frame.
        self.poll_detect(backend);
        self.poll_redetect();

        self.show_toolbar(ui, backend);

        ui.add_space(4.0);
        ui.separator();
        ui.add_space(2.0);

        // Two-pane layout: resizable list | edit panel.
        egui::Panel::left("engines_list_panel")
            .default_size(270.0)
            .size_range(160.0..=440.0)
            .resizable(true)
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                right: 10,
                ..Default::default()
            }))
            .show_inside(ui, |ui| {
                self.show_list(ui, backend);
            });

        ScrollArea::vertical()
            .id_salt("engines_edit_scroll")
            .show(ui, |ui| {
                if self.edit.is_some() {
                    self.show_edit(ui, backend);
                } else {
                    empty_state(ui);
                }
            });
    }
}

// ── Detection jobs ────────────────────────────────────────────────────────────

/// A background add-single / folder-scan job.
struct DetectJob {
    rx: Receiver<(PathBuf, Result<DetectResult, String>)>,
    total: usize,
    done: usize,
}

impl EnginesTab {
    /// Drain the add/scan channel; create `EngineConfig`s for successes.
    fn poll_detect(&mut self, backend: &mut Backend) {
        if self.pending.is_none() {
            return;
        }

        // Collect results while holding the borrow; release it before calling
        // `select_engine` (which also borrows `self` mutably).
        let mut new_ids: Vec<EngineId> = Vec::new();
        let mut last_error: Option<String> = None;
        let mut disconnected = false;

        {
            let job = self.pending.as_mut().unwrap();
            loop {
                match job.rx.try_recv() {
                    Ok((path, Ok(result))) => {
                        job.done += 1;
                        let id = add_engine_from_detect(path, result, backend);
                        new_ids.push(id);
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
        } // borrow on self.pending ends here

        if let Some(err) = last_error {
            self.detect_error = Some(err);
        }

        let done = self.pending.as_ref().map_or(0, |j| j.done);
        let total = self.pending.as_ref().map_or(0, |j| j.total);
        if disconnected || done >= total {
            self.pending = None;
        }

        let engines_snap = backend.engines.clone();
        for id in new_ids {
            if self.selected_id.is_none() {
                self.select_engine(&id, &engines_snap);
            }
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
                    // Offer detected identity only for fields the user hasn't filled.
                    if let Some(id_name) = result.name {
                        let (name, version) = split_name_version(&id_name);
                        if edit.name.trim().is_empty() {
                            edit.name = name;
                        }
                        if let Some(version) = version
                            && edit.version.trim().is_empty()
                        {
                            edit.version = version;
                        }
                    }
                    if let Some(author) = result.author
                        && edit.author.trim().is_empty()
                    {
                        edit.author = author;
                    }
                    edit.redetect_pending = false;
                    edit.dirty = true;
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
        self.pending = Some(DetectJob { rx, total, done: 0 });
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

    fn select_engine(&mut self, id: &EngineId, engines: &[EngineConfig]) {
        if let Some(engine) = engines.iter().find(|e| &e.id == id) {
            self.selected_id = Some(*id);
            self.edit = Some(EngineEditBuf::from_engine(engine));
        }
    }
}

// ── Toolbar ───────────────────────────────────────────────────────────────────

impl EnginesTab {
    fn show_toolbar(&mut self, ui: &mut Ui, backend: &mut Backend) {
        ui.horizontal(|ui| {
            let busy = self.pending.is_some();

            // Primary action: Add Engine.
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

            // Secondary action: Scan Folder.
            let folder_resp = ui
                .add_enabled(
                    !busy,
                    egui::Button::new(
                        RichText::new("Scan Folder…").color(theme::TEXT).size(13.5),
                    )
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
                ui.label(
                    RichText::new(format!("⚠ {err}"))
                        .color(theme::DANGER)
                        .size(13.0),
                );
                ui.add_space(4.0);
                if ui
                    .small_button(RichText::new("×").color(theme::TEXT_WEAK))
                    .clicked()
                {
                    self.detect_error = None;
                }
            }
        });
    }
}

// ── Engine list ───────────────────────────────────────────────────────────────

impl EnginesTab {
    fn show_list(&mut self, ui: &mut Ui, backend: &mut Backend) {
        // ── Header: count + Delete All ────────────────────────────────────────
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
                            .small_button(RichText::new("Cancel").color(theme::TEXT_WEAK))
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
            backend.engines.clear();
            backend.save_engines();
            self.selected_id = None;
            self.selected_ids.clear();
            self.edit = None;
            self.delete_all_confirm = false;
        }
        if cancel_delete_all {
            self.delete_all_confirm = false;
        }

        ui.add_space(4.0);

        // ── Filter ────────────────────────────────────────────────────────────
        ui.add(
            egui::TextEdit::singleline(&mut self.filter_text)
                .desired_width(f32::INFINITY)
                .hint_text("🔍 Filter engines…"),
        );
        ui.add_space(4.0);

        // ── Bulk-selection action bar ─────────────────────────────────────────
        if !self.selected_ids.is_empty() {
            let n = self.selected_ids.len();
            let mut do_delete_selected = false;
            let mut do_deselect_all = false;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{n} selected"))
                        .color(theme::ACCENT)
                        .size(12.0),
                );
                ui.add_space(4.0);
                if widgets::tinted_button(ui, &format!("Delete {n}"), theme::DANGER, true)
                    .clicked()
                {
                    do_delete_selected = true;
                }
                ui.add_space(4.0);
                if ui
                    .small_button(RichText::new("Deselect all").color(theme::TEXT_WEAK))
                    .clicked()
                {
                    do_deselect_all = true;
                }
            });
            if do_delete_selected {
                let del_ids = self.selected_ids.clone();
                backend.engines.retain(|e| !del_ids.contains(&e.id));
                backend.save_engines();
                if let Some(id) = self.selected_id
                    && del_ids.contains(&id)
                {
                    self.selected_id = None;
                    self.edit = None;
                }
                self.selected_ids.clear();
            }
            if do_deselect_all {
                self.selected_ids.clear();
            }
            ui.add_space(2.0);
        }

        let filter = self.filter_text.to_lowercase();

        // ── Engine list ───────────────────────────────────────────────────────
        ScrollArea::vertical()
            .id_salt("engines_list_scroll")
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                if backend.engines.is_empty() {
                    ui.add_space(24.0);
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
                            RichText::new("Add an engine with the button above.")
                                .color(theme::TEXT_FAINT)
                                .size(12.5),
                        );
                    });
                    return;
                }

                let mut clicked_id: Option<EngineId> = None;
                let mut toggle_check: Option<(EngineId, bool)> = None;

                for engine in &backend.engines {
                    let display_name = engine_display_name(engine);
                    let stem = engine
                        .path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");

                    if !filter.is_empty()
                        && !display_name.to_lowercase().contains(&filter)
                        && !stem.to_lowercase().contains(&filter)
                    {
                        continue;
                    }

                    let is_primary = self.selected_id.as_ref() == Some(&engine.id);
                    let is_checked = self.selected_ids.contains(&engine.id);
                    let path_missing = !engine.path.exists();
                    let engine_id = engine.id;

                    let (fill, stroke) = if is_primary {
                        (
                            theme::tint(theme::ACCENT, 0.12),
                            egui::Stroke::new(1.0, theme::tint(theme::ACCENT, 0.4)),
                        )
                    } else if is_checked {
                        (
                            theme::tint(theme::DANGER, 0.10),
                            egui::Stroke::new(1.0, theme::tint(theme::DANGER, 0.35)),
                        )
                    } else {
                        (Color32::TRANSPARENT, egui::Stroke::NONE)
                    };

                    let bg_slot = ui.painter().add(egui::Shape::Noop);

                    ui.horizontal(|ui| {
                        // Checkbox for multi-select. The theme draws idle widgets
                        // borderless, which makes a bare checkbox nearly invisible
                        // at rest — give it a sunken fill + visible border locally.
                        let mut checked = is_checked;
                        let toggled = ui
                            .scope(|ui| {
                                let w = &mut ui.visuals_mut().widgets;
                                w.inactive.bg_fill = theme::BG_INPUT;
                                w.inactive.weak_bg_fill = theme::BG_INPUT;
                                w.inactive.bg_stroke =
                                    egui::Stroke::new(1.0, theme::TEXT_FAINT);
                                w.hovered.bg_stroke =
                                    egui::Stroke::new(1.0, theme::ACCENT);
                                ui.checkbox(&mut checked, "").changed()
                            })
                            .inner;
                        if toggled {
                            toggle_check = Some((engine_id, checked));
                        }

                        let row_resp = egui::Frame::new()
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                ui.set_min_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(&display_name)
                                                    .color(if is_primary {
                                                        theme::ACCENT_BRIGHT
                                                    } else {
                                                        theme::TEXT
                                                    })
                                                    .size(13.5),
                                            );
                                            if path_missing {
                                                ui.label(
                                                    RichText::new("⚠")
                                                        .color(theme::WARN)
                                                        .size(12.0),
                                                )
                                                .on_hover_text(
                                                    "Executable not found at this path.",
                                                );
                                            }
                                        });
                                        if !stem.is_empty() {
                                            ui.label(
                                                RichText::new(stem)
                                                    .color(theme::TEXT_WEAK)
                                                    .size(11.5),
                                            );
                                        }
                                    });
                                    ui.with_layout(
                                        Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if let Some(elo) = engine.meta.elo {
                                                ui.label(
                                                    RichText::new(elo.to_string())
                                                        .color(theme::TEXT_FAINT)
                                                        .size(12.0),
                                                );
                                            }
                                        },
                                    );
                                });
                            })
                            .response;

                        let interact = ui.interact(
                            row_resp.rect,
                            egui::Id::new("engine_row").with(engine_id),
                            egui::Sense::click(),
                        );

                        if interact.hovered() && !is_primary {
                            ui.painter().set(
                                bg_slot,
                                egui::Shape::rect_filled(
                                    row_resp.rect,
                                    egui::CornerRadius::same(6),
                                    theme::BG_HOVER,
                                ),
                            );
                        }

                        if interact.clicked() {
                            clicked_id = Some(engine_id);
                        }
                    });

                    ui.add_space(4.0);
                }

                // Apply mutations outside the loop (borrow rules).
                if let Some((id, add)) = toggle_check {
                    if add {
                        self.selected_ids.insert(id);
                    } else {
                        self.selected_ids.remove(&id);
                    }
                }
                if let Some(id) = clicked_id {
                    let snap: Vec<EngineConfig> = backend.engines.clone();
                    self.select_engine(&id, &snap);
                }
            });
    }
}

// ── Edit buffer ───────────────────────────────────────────────────────────────

struct EngineEditBuf {
    engine_id: EngineId,

    // Identity
    name: String,
    version: String,
    author: String,
    elo_str: String,

    // Launch
    path: PathBuf,
    args_str: String,
    working_dir_str: String,

    // Env vars
    env_rows: Vec<[String; 2]>,
    new_env_key: String,
    new_env_val: String,

    // UCI options
    detected_options: Vec<UciOption>,
    option_overrides: BTreeMap<String, UciOptionValue>,

    // UI state
    dirty: bool,
    delete_confirm: bool,
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
            delete_confirm: false,
            redetect_pending: false,
        }
    }

    /// Write the buffer back to the matching engine and persist.
    fn commit(&mut self, backend: &mut Backend) {
        let Some(engine) = backend.engines.iter_mut().find(|e| e.id == self.engine_id) else {
            return;
        };
        engine.meta.name = self.name.clone();
        engine.meta.version = self.version.clone();
        if self.author.trim().is_empty() {
            engine.meta.extra.remove("author");
        } else {
            engine
                .meta
                .extra
                .insert("author".to_string(), self.author.trim().to_string());
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
    }
}

// ── Edit panel ────────────────────────────────────────────────────────────────

impl EnginesTab {
    fn show_edit(&mut self, ui: &mut Ui, backend: &mut Backend) {
        // Take to avoid simultaneous mutable borrows of `self`.
        let Some(mut edit) = self.edit.take() else {
            return;
        };

        let mut do_delete = false;
        let mut do_redetect = false;
        let mut do_clone = false;

        // ─ Heading ─
        ui.horizontal(|ui| {
            let name = engine_display_name_from_parts(&edit.name, &edit.version, &edit.path);
            ui.label(RichText::new(&name).size(19.0).strong().color(theme::TEXT));
            if edit.dirty {
                ui.label(
                    RichText::new("● unsaved")
                        .color(theme::WARN)
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
        ui.add_space(8.0);

        // ─ Identity ─
        widgets::section_card(ui, "Identity", None, |ui| {
            egui::Grid::new("edit_identity")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    field_label(ui, "Name");
                    if ui
                        .add(text_field(&mut edit.name).hint_text("e.g. Stockfish 16"))
                        .changed()
                    {
                        edit.dirty = true;
                    }
                    ui.end_row();

                    field_label(ui, "Version");
                    if ui
                        .add(text_field(&mut edit.version).hint_text("e.g. 16"))
                        .changed()
                    {
                        edit.dirty = true;
                    }
                    ui.end_row();

                    field_label(ui, "Author");
                    if ui
                        .add(
                            text_field(&mut edit.author)
                                .hint_text("detected from the engine's UCI id"),
                        )
                        .changed()
                    {
                        edit.dirty = true;
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
                        edit.dirty = true;
                    }
                    ui.end_row();
                });
        });

        // ─ Launch ─
        widgets::section_card(ui, "Launch", None, |ui| {
            egui::Grid::new("edit_launch")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    field_label(ui, "Path");
                    // RTL: "Open folder" anchors right; the path label then fills the
                    // remaining width, left-aligned and truncating so it never overflows.
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(folder) = edit.path.parent() {
                            let folder = folder.to_path_buf();
                            if ui
                                .small_button(RichText::new("Open folder").color(theme::TEXT_WEAK))
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
                        edit.dirty = true;
                    }
                    ui.end_row();

                    field_label(ui, "Work dir");
                    // RTL so the Browse button anchors right and the text field fills remaining space.
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        let browse_clicked = ui
                            .small_button(RichText::new("Browse…").color(theme::TEXT_WEAK))
                            .clicked();
                        if ui
                            .add(
                                text_field(&mut edit.working_dir_str)
                                    .hint_text("defaults to engine directory"),
                            )
                            .changed()
                        {
                            edit.dirty = true;
                        }
                        if browse_clicked
                            && let Some(dir) = rfd::FileDialog::new()
                                .set_title("Select working directory")
                                .pick_folder()
                        {
                            edit.working_dir_str = dir.to_string_lossy().to_string();
                            edit.dirty = true;
                        }
                    });
                    ui.end_row();
                });

            // ─ Environment variables ─
            ui.add_space(8.0);
            ui.label(
                RichText::new("Environment Variables")
                    .color(theme::TEXT_WEAK)
                    .size(12.0)
                    .strong(),
            );
            ui.add_space(4.0);

            let mut remove_idx: Option<usize> = None;
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
                        edit.dirty = true;
                    }
                    // RTL: × button anchors right, value field fills the rest.
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(RichText::new("×").color(theme::DANGER))
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
                            edit.dirty = true;
                        }
                    });
                });
            }
            if let Some(i) = remove_idx {
                edit.env_rows.remove(i);
                edit.dirty = true;
            }
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut edit.new_env_key)
                        .desired_width(150.0)
                        .hint_text("NEW KEY"),
                );
                // RTL: + button anchors right, value field fills the rest.
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
                    edit.dirty = true;
                }
            });
        });

        // ─ UCI Options ─
        let can_redetect = !edit.redetect_pending && self.redetect_rx.is_none();
        widgets::section_card(ui, "UCI Options", None, |ui| {
            ui.horizontal(|ui| {
                if edit.detected_options.is_empty() {
                    ui.label(
                        RichText::new(
                            "No options detected yet — use Re-detect to query the engine.",
                        )
                        .color(theme::TEXT_WEAK)
                        .size(12.5)
                        .italics(),
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
                                .size(12.5)
                                .color(theme::TEXT_WEAK),
                            )
                            .fill(theme::BG_ELEVATED)
                            .stroke(egui::Stroke::new(1.0, theme::STROKE)),
                        )
                        .on_hover_text(
                            "Re-run the UCI handshake to refresh options and identity.",
                        )
                        .clicked()
                    {
                        do_redetect = true;
                    }
                    if !edit.option_overrides.is_empty() {
                        ui.add_space(4.0);
                        if ui
                            .small_button(RichText::new("Reset all").color(theme::TEXT_WEAK))
                            .on_hover_text("Remove all option overrides, reverting to engine defaults.")
                            .clicked()
                        {
                            edit.option_overrides.clear();
                            edit.dirty = true;
                        }
                    }
                });
            });

            if !edit.detected_options.is_empty() {
                ui.add_space(6.0);
                egui::Grid::new("uci_opts_grid")
                    .num_columns(3)
                    .spacing([8.0, 5.0])
                    .show(ui, |ui| {
                        let options = edit.detected_options.clone();
                        for opt in &options {
                            show_option_row(
                                ui,
                                opt,
                                &mut edit.option_overrides,
                                &mut edit.dirty,
                            );
                            // Per-option reset button (×) — only when overridden.
                            let has_override = edit.option_overrides.contains_key(opt.name());
                            if has_override {
                                if ui
                                    .small_button(RichText::new("×").color(theme::TEXT_FAINT))
                                    .on_hover_text("Reset to engine default.")
                                    .clicked()
                                {
                                    edit.option_overrides.remove(opt.name());
                                    edit.dirty = true;
                                }
                            } else {
                                ui.label(""); // keep grid columns aligned
                            }
                            ui.end_row();
                        }
                    });
            }
        });

        // ─ Action row ─
        ui.horizontal(|ui| {
            // Primary: Save
            if ui
                .add_enabled(
                    edit.dirty,
                    egui::Button::new(
                        RichText::new("Save Changes")
                            .color(theme::BG_DARKEST)
                            .size(13.5)
                            .strong(),
                    )
                    .fill(theme::ACCENT),
                )
                .clicked()
            {
                edit.commit(backend);
            }

            ui.add_space(4.0);

            if ui
                .button(RichText::new("Clone").color(theme::TEXT_WEAK).size(13.0))
                .on_hover_text("Duplicate this engine entry with a new identity.")
                .clicked()
            {
                do_clone = true;
            }

            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if edit.delete_confirm {
                    if widgets::tinted_button(ui, "Confirm Delete", theme::DANGER, true)
                        .clicked()
                    {
                        do_delete = true;
                    }
                    ui.add_space(4.0);
                    if ui
                        .button(RichText::new("Cancel").color(theme::TEXT_WEAK).size(13.0))
                        .clicked()
                    {
                        edit.delete_confirm = false;
                    }
                } else if widgets::tinted_button(ui, "Delete Engine", theme::DANGER, true)
                    .clicked()
                {
                    edit.delete_confirm = true;
                }
            });
        });

        // ─ Execute deferred actions ─

        if do_delete {
            backend.engines.retain(|e| e.id != edit.engine_id);
            backend.save_engines();
            self.selected_id = None;
            // edit is dropped; don't restore it
            return;
        }

        if do_clone
            && let Some(src) = backend.engines.iter().find(|e| e.id == edit.engine_id)
        {
            let mut cloned = src.clone();
            cloned.id = colosseum_core::EngineId::new();
            let suffix = " (copy)";
            if !cloned.meta.name.ends_with(suffix) {
                cloned.meta.name.push_str(suffix);
            }
            let new_id = cloned.id;
            backend.engines.push(cloned);
            backend.save_engines();
            let snap = backend.engines.clone();
            self.select_engine(&new_id, &snap);
        }

        if do_redetect {
            let path = edit.path.clone();
            let id = edit.engine_id;
            edit.redetect_pending = true;
            self.start_redetect(path, id, backend);
        }

        self.edit = Some(edit);
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Create an `EngineConfig` from detection results and push it to `backend.engines`.
/// Returns the new engine's `EngineId`.
fn add_engine_from_detect(path: PathBuf, result: DetectResult, backend: &mut Backend) -> EngineId {
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
    let id = cfg.id;
    backend.engines.push(cfg);
    backend.save_engines();
    id
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

/// Display name shown in the engine list: "Name Version" (or file stem if name empty).
fn engine_display_name(e: &EngineConfig) -> String {
    let base = if e.meta.name.is_empty() {
        e.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string()
    } else {
        e.meta.name.clone()
    };
    if !e.meta.version.is_empty() {
        format!("{base} {}", e.meta.version)
    } else {
        base
    }
}

fn engine_display_name_from_parts(name: &str, version: &str, path: &Path) -> String {
    let base = if name.trim().is_empty() {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string()
    } else {
        name.to_string()
    };
    if !version.trim().is_empty() {
        format!("{base} {version}")
    } else {
        base
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

/// Draw one UCI option row (label | editor).  Sets `dirty` if the value changed.
fn show_option_row(
    ui: &mut Ui,
    opt: &UciOption,
    overrides: &mut BTreeMap<String, UciOptionValue>,
    dirty: &mut bool,
) {
    ui.label(RichText::new(opt.name()).color(theme::TEXT_WEAK).size(13.0));

    match opt {
        UciOption::Check { name, default } => {
            let mut val = matches!(overrides.get(name), Some(UciOptionValue::Check(true)))
                || (overrides.get(name).is_none() && *default);

            if ui.checkbox(&mut val, "").changed() {
                overrides.insert(name.clone(), UciOptionValue::Check(val));
                *dirty = true;
            }
        }

        UciOption::Spin {
            name,
            default,
            min,
            max,
        } => {
            let current = match overrides.get(name) {
                Some(UciOptionValue::Spin(v)) => *v,
                _ => *default,
            };
            let mut val = current;
            let resp = ui.add(DragValue::new(&mut val).range(*min..=*max).speed(1.0));
            if resp.changed() {
                overrides.insert(name.clone(), UciOptionValue::Spin(val));
                *dirty = true;
            }
            ui.label(
                RichText::new(format!("({min}–{max})"))
                    .color(theme::TEXT_FAINT)
                    .size(11.5),
            );
        }

        UciOption::Combo {
            name,
            default,
            vars,
        } => {
            let current = match overrides.get(name) {
                Some(UciOptionValue::Combo(s)) => s.clone(),
                _ => default.clone(),
            };
            let mut selected = current.clone();
            egui::ComboBox::from_id_salt(egui::Id::new("opt_combo").with(name))
                .selected_text(&selected)
                .width(200.0)
                .show_ui(ui, |ui| {
                    for v in vars {
                        if ui.selectable_value(&mut selected, v.clone(), v).clicked() {
                            overrides.insert(name.clone(), UciOptionValue::Combo(selected.clone()));
                            *dirty = true;
                        }
                    }
                });
        }

        UciOption::Str { name, default } => {
            let current = match overrides.get(name) {
                Some(UciOptionValue::Str(s)) => s.clone(),
                _ => default.clone(),
            };
            let mut val = current;
            if ui
                .add(
                    egui::TextEdit::singleline(&mut val)
                        .desired_width(240.0)
                        .hint_text(default),
                )
                .changed()
            {
                overrides.insert(name.clone(), UciOptionValue::Str(val));
                *dirty = true;
            }
        }

        UciOption::Button { name } => {
            let armed = matches!(overrides.get(name), Some(UciOptionValue::Button));
            let label = if armed { "✓ armed" } else { "arm" };
            let color = if armed { theme::SUCCESS } else { theme::TEXT_WEAK };
            if ui
                .small_button(RichText::new(label).color(color))
                .on_hover_text(if armed {
                    format!("Will send 'setoption name {name}' at game start. Click to disarm.")
                } else {
                    format!("Arm to send 'setoption name {name}' to the engine at game start.")
                })
                .clicked()
            {
                if armed {
                    overrides.remove(name);
                } else {
                    overrides.insert(name.clone(), UciOptionValue::Button);
                }
                *dirty = true;
            }
        }
    }
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
        ui.add_space(80.0);
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
            RichText::new("Pick an engine from the list, or add one with the buttons above.")
                .color(theme::TEXT_FAINT)
                .size(12.5),
        );
    });
}
