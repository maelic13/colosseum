// SPDX-License-Identifier: GPL-3.0-or-later
//! Tournament tab: configure and launch a tournament, then watch it live.
//!
//! Two views share one tab:
//! - **Setup** (no active tournament): engine selection + all tournament
//!   options, with a prominent Start button.
//! - **Live** (a tournament exists): Go / Stop / Force-Stop controls, a progress
//!   readout, a sortable results table, an optional head-to-head matrix, and an
//!   engine-error panel.

use std::path::PathBuf;

use eframe::egui::{self, Color32, DragValue, Layout, RichText, ScrollArea, Ui};
use egui_extras::{Column, TableBuilder};

use colosseum_core::{
    AdjudicationConfig, CommonEngineOptions, DrawAdjudication, EloPolicy, EngineConfig, EngineId,
    Format, OpeningBook, OpeningFormat, OpeningOrder, ResignAdjudication, Standings, StartPosition,
    TimeControl, TimeUnit, TournamentConfig,
};
use colosseum_engine::{EloEntry, TournamentStatus, summarize};

use crate::backend::{Backend, ParticipantInfo};
use crate::theme;
use crate::widgets;

// ── Tab state ─────────────────────────────────────────────────────────────────

/// All persistent state for the Tournament tab.
#[derive(Default)]
pub struct TournamentTab {
    form: TournamentForm,
    sort: SortState,
    show_h2h: bool,
    start_error: Option<String>,
    elo_note: Option<String>,
}

impl TournamentTab {
    /// Draw the tab body. Call every frame.
    pub fn show(&mut self, ui: &mut Ui, backend: &mut Backend) {
        if backend.active.is_some() {
            self.show_live(ui, backend);
        } else {
            self.show_setup(ui, backend);
        }
    }
}

// ── Configuration form ──────────────────────────────────────────────────────────

/// GUI-friendly buffer for a [`TournamentConfig`] plus engine selection.
struct TournamentForm {
    name: String,
    /// Selected engine ids, in selection order (seeding order).
    selected: Vec<EngineId>,

    // Format
    cycles: u32,
    games_per_pair: u32,

    // Time control
    tc_value: f64,
    tc_unit: TimeUnit,

    // Concurrency
    concurrency: usize,

    // Common engine options
    threads_on: bool,
    threads: u32,
    hash_on: bool,
    hash_mb: u32,
    syzygy_path: String,
    syzygy50_on: bool,
    syzygy50: bool,
    ponder: bool,

    // Adjudication
    max_moves_on: bool,
    max_moves: u32,
    draw_on: bool,
    draw_min_ply: u32,
    draw_move_count: u32,
    draw_score_cp: i32,
    resign_on: bool,
    resign_move_count: u32,
    resign_score_cp: i32,

    // Elo
    elo_policy: EloPolicy,
    k_factor: f64,

    // Openings
    openings_on: bool,
    openings_path: String,
    openings_format: OpeningFormat,
    openings_order: OpeningOrder,
    openings_plies: u32,
    openings_count_on: bool,
    openings_count: u32,
    openings_seed: u64,
    /// Cached preview of the currently-selected book (count + sample), recomputed
    /// when the path/format changes.
    openings_preview: Option<Result<(usize, Option<String>), String>>,

    // Output
    pgn_path: String,
}

impl Default for TournamentForm {
    fn default() -> Self {
        Self {
            name: "Round Robin".to_string(),
            selected: Vec::new(),
            cycles: 1,
            games_per_pair: 2,
            tc_value: 100.0,
            tc_unit: TimeUnit::Milliseconds,
            concurrency: 1,
            threads_on: true,
            threads: 1,
            hash_on: false,
            hash_mb: 128,
            syzygy_path: String::new(),
            syzygy50_on: false,
            syzygy50: true,
            ponder: false,
            max_moves_on: false,
            max_moves: 300,
            draw_on: false,
            draw_min_ply: 40,
            draw_move_count: 8,
            draw_score_cp: 8,
            resign_on: false,
            resign_move_count: 4,
            resign_score_cp: 800,
            elo_policy: EloPolicy::PerGame,
            k_factor: 32.0,
            openings_on: false,
            openings_path: String::new(),
            openings_format: OpeningFormat::Epd,
            openings_order: OpeningOrder::Sequential,
            openings_plies: 8,
            openings_count_on: false,
            openings_count: 100,
            openings_seed: 0,
            openings_preview: None,
            pgn_path: String::new(),
        }
    }
}

impl TournamentForm {
    /// Build the immutable [`TournamentConfig`] from the current form values.
    fn build_config(&self) -> TournamentConfig {
        TournamentConfig {
            format: Format::RoundRobin {
                cycles: self.cycles.max(1),
            },
            games_per_pair: self.games_per_pair.max(1),
            time_control: TimeControl::PerMove {
                ms: self.tc_unit.to_millis(self.tc_value).max(1),
            },
            concurrency: self.concurrency.max(1),
            common: CommonEngineOptions {
                threads: self.threads_on.then_some(self.threads.max(1)),
                hash_mb: self.hash_on.then_some(self.hash_mb),
                syzygy_path: (!self.syzygy_path.trim().is_empty())
                    .then(|| self.syzygy_path.trim().to_string()),
                syzygy_50_move_rule: self.syzygy50_on.then_some(self.syzygy50),
                ponder: self.ponder,
            },
            adjudication: AdjudicationConfig {
                max_moves: self.max_moves_on.then_some(self.max_moves.max(1)),
                draw: self.draw_on.then_some(DrawAdjudication {
                    min_ply: self.draw_min_ply,
                    move_count: self.draw_move_count.max(1),
                    score_cp: self.draw_score_cp.max(0),
                }),
                resign: self.resign_on.then_some(ResignAdjudication {
                    move_count: self.resign_move_count.max(1),
                    score_cp: self.resign_score_cp.max(0),
                }),
            },
            elo_policy: self.elo_policy,
            k_factor: self.k_factor.max(1.0),
            start_position: match self.opening_book() {
                Some(book) => StartPosition::Book(book),
                None => StartPosition::Startpos,
            },
            pgn_output: (!self.pgn_path.trim().is_empty())
                .then(|| PathBuf::from(self.pgn_path.trim())),
        }
    }

    /// The configured [`OpeningBook`], or `None` when openings are disabled or no
    /// file is chosen.
    fn opening_book(&self) -> Option<OpeningBook> {
        if !self.openings_on || self.openings_path.trim().is_empty() {
            return None;
        }
        Some(OpeningBook {
            path: PathBuf::from(self.openings_path.trim()),
            format: self.openings_format,
            order: self.openings_order,
            plies: self.openings_plies.max(1),
            count: self.openings_count_on.then_some(self.openings_count.max(1)),
            seed: self.openings_seed,
        })
    }

    /// Recompute the opening-book preview (count + first label) for the GUI.
    fn refresh_openings_preview(&mut self) {
        self.openings_preview = self.opening_book().map(|book| {
            summarize(&book)
                .map(|s| (s.count, s.first_label))
                .map_err(|e| e.to_string())
        });
    }

    /// Resolve the selected engine ids to their library configs, in seed order.
    fn selected_engines(&self, library: &[EngineConfig]) -> Vec<EngineConfig> {
        self.selected
            .iter()
            .filter_map(|id| library.iter().find(|e| &e.id == id).cloned())
            .collect()
    }

    fn is_selected(&self, id: EngineId) -> bool {
        self.selected.contains(&id)
    }

    fn toggle(&mut self, id: EngineId) {
        if let Some(pos) = self.selected.iter().position(|x| x == &id) {
            self.selected.remove(pos);
        } else {
            self.selected.push(id);
        }
    }

    /// Number of games the configured tournament will schedule.
    fn estimated_games(&self) -> usize {
        let n = self.selected.len();
        if n < 2 {
            return 0;
        }
        // Round-robin: pairs * games_per_pair * cycles.
        let pairs = n * (n - 1) / 2;
        pairs * self.games_per_pair.max(1) as usize * self.cycles.max(1) as usize
    }
}

// ── Setup view ──────────────────────────────────────────────────────────────────

impl TournamentTab {
    fn show_setup(&mut self, ui: &mut Ui, backend: &mut Backend) {
        // Bottom action bar (pinned).
        egui::Panel::bottom("tournament_setup_actions")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DARKEST)
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show_inside(ui, |ui| {
                self.setup_action_bar(ui, backend);
            });

        // Engine selection (left).
        egui::Panel::left("tournament_engine_select")
            .default_size(280.0)
            .size_range(200.0..=440.0)
            .resizable(true)
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                right: 12,
                ..Default::default()
            }))
            .show_inside(ui, |ui| {
                self.engine_selection(ui, backend);
            });

        // Settings (centre, scrollable).
        egui::CentralPanel::default()
            .frame(egui::Frame::new())
            .show_inside(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("tournament_settings_scroll")
                    .show(ui, |ui| {
                        self.settings_form(ui);
                    });
            });
    }

    fn setup_action_bar(&mut self, ui: &mut Ui, backend: &mut Backend) {
        ui.horizontal(|ui| {
            let count = self.form.selected.len();
            let ready = count >= 2;
            let games = self.form.estimated_games();

            let start = ui.add_enabled(
                ready,
                egui::Button::new(
                    RichText::new("▶  Start Tournament")
                        .color(theme::BG_DARKEST)
                        .size(15.0)
                        .strong(),
                )
                .fill(theme::ACCENT)
                .min_size(egui::vec2(0.0, 32.0)),
            );

            if start.clicked() {
                self.try_start(backend);
            }

            ui.add_space(12.0);

            if ready {
                ui.label(
                    RichText::new(format!("{count} engines · {games} games"))
                        .color(theme::TEXT_WEAK)
                        .size(13.0),
                );
            } else {
                ui.label(
                    RichText::new("Select at least two engines to start.")
                        .color(theme::WARN)
                        .size(13.0),
                );
            }

            if let Some(err) = self.start_error.clone() {
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(RichText::new("×").color(theme::TEXT_WEAK))
                        .clicked()
                    {
                        self.start_error = None;
                    }
                    ui.label(
                        RichText::new(format!("⚠ {err}"))
                            .color(theme::DANGER)
                            .size(13.0),
                    );
                });
            }
        });
    }

    fn try_start(&mut self, backend: &mut Backend) {
        let engines = self.form.selected_engines(&backend.engines);
        if engines.len() < 2 {
            self.start_error = Some("Select at least two engines.".to_string());
            return;
        }
        let config = self.form.build_config();
        let name = if self.form.name.trim().is_empty() {
            "Tournament"
        } else {
            self.form.name.trim()
        };
        match backend.start_tournament(name, config, engines) {
            Ok(()) => {
                self.start_error = None;
                self.sort = SortState::default();
                self.elo_note = None;
            }
            Err(e) => self.start_error = Some(e.to_string()),
        }
    }

    fn engine_selection(&mut self, ui: &mut Ui, backend: &Backend) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Engines")
                    .color(theme::TEXT)
                    .size(15.0)
                    .strong(),
            );
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{} selected", self.form.selected.len()))
                        .color(theme::ACCENT)
                        .size(12.5),
                );
            });
        });
        ui.add_space(2.0);

        ui.horizontal(|ui| {
            if ui
                .small_button(RichText::new("Select all").color(theme::TEXT_WEAK))
                .clicked()
            {
                self.form.selected = backend.engines.iter().map(|e| e.id).collect();
            }
            if ui
                .small_button(RichText::new("Clear").color(theme::TEXT_WEAK))
                .clicked()
            {
                self.form.selected.clear();
            }
        });
        ui.add_space(4.0);
        ui.separator();

        ScrollArea::vertical()
            .id_salt("tournament_engine_list")
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
                            RichText::new("Add engines in the Engines tab to get started.")
                                .color(theme::TEXT_FAINT)
                                .size(12.5),
                        );
                    });
                    return;
                }

                for engine in &backend.engines {
                    let selected = self.form.is_selected(engine.id);
                    let name = engine_display_name(engine);
                    let sub = if engine.meta.version.is_empty() {
                        engine
                            .path
                            .file_name()
                            .and_then(|s| s.to_str())
                            .unwrap_or("")
                            .to_string()
                    } else {
                        format!("v{}", engine.meta.version)
                    };

                    let (fill, stroke) = if selected {
                        (
                            theme::tint(theme::ACCENT, 0.12),
                            egui::Stroke::new(1.0, theme::tint(theme::ACCENT, 0.4)),
                        )
                    } else {
                        (Color32::TRANSPARENT, egui::Stroke::NONE)
                    };

                    let row_resp = egui::Frame::new()
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(egui::CornerRadius::same(6))
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            ui.horizontal(|ui| {
                                let mut checked = selected;
                                if ui.checkbox(&mut checked, "").changed() {
                                    self.form.toggle(engine.id);
                                }
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&name)
                                            .color(if selected {
                                                theme::ACCENT_BRIGHT
                                            } else {
                                                theme::TEXT
                                            })
                                            .size(13.5),
                                    );
                                    if !sub.is_empty() {
                                        ui.label(
                                            RichText::new(&sub)
                                                .color(theme::TEXT_WEAK)
                                                .size(11.5),
                                        );
                                    }
                                });
                            });
                        })
                        .response;

                    let interact = ui.interact(
                        row_resp.rect,
                        egui::Id::new("sel_engine_row").with(engine.id),
                        egui::Sense::click(),
                    );

                    if interact.hovered() && !selected {
                        ui.painter().rect_filled(
                            row_resp.rect,
                            egui::CornerRadius::same(6),
                            theme::BG_HOVER,
                        );
                    }

                    if interact.clicked() {
                        self.form.toggle(engine.id);
                    }

                    ui.add_space(4.0);
                }
            });
    }

    fn settings_form(&mut self, ui: &mut Ui) {
        let f = &mut self.form;

        // ── Tournament (Name + Format + Concurrency) ──
        widgets::section_card(ui, "Tournament", None, |ui| {
            egui::Grid::new("tc_tournament_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    field_label(ui, "Name");
                    ui.add(
                        egui::TextEdit::singleline(&mut f.name)
                            .desired_width(260.0)
                            .hint_text("Tournament name"),
                    );
                    ui.end_row();

                    field_label(ui, "Format");
                    ui.label(RichText::new("Round Robin").color(theme::TEXT));
                    ui.end_row();

                    field_label(ui, "Cycles");
                    ui.add(DragValue::new(&mut f.cycles).range(1..=20).speed(0.1))
                        .on_hover_text("How many times the full schedule repeats.");
                    ui.end_row();

                    field_label(ui, "Games / pair");
                    ui.add(
                        DragValue::new(&mut f.games_per_pair)
                            .range(1..=20)
                            .speed(0.1),
                    )
                    .on_hover_text("Games each pair plays per cycle (2 = both colours).");
                    ui.end_row();

                    field_label(ui, "Parallel games");
                    let mut c = f.concurrency as u32;
                    if ui
                        .add(DragValue::new(&mut c).range(1..=256).speed(0.1))
                        .changed()
                    {
                        f.concurrency = c as usize;
                    }
                    ui.end_row();
                });
        });

        // ── Time Control ──
        widgets::section_card(ui, "Time Control", None, |ui| {
            ui.horizontal(|ui| {
                field_label(ui, "Per move");
                ui.add(
                    DragValue::new(&mut f.tc_value)
                        .range(1.0..=600_000.0)
                        .speed(1.0),
                );
                egui::ComboBox::from_id_salt("tc_unit")
                    .selected_text(f.tc_unit.label())
                    .width(64.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut f.tc_unit, TimeUnit::Milliseconds, "ms");
                        ui.selectable_value(&mut f.tc_unit, TimeUnit::Seconds, "s");
                        ui.selectable_value(&mut f.tc_unit, TimeUnit::Minutes, "min");
                    });
                let ms = f.tc_unit.to_millis(f.tc_value).max(1);
                ui.label(
                    RichText::new(format!("= {ms} ms"))
                        .color(theme::TEXT_WEAK)
                        .size(12.0),
                );
            });
        });

        // ── Engine Options (Common + Syzygy + Ponder) ──
        widgets::section_card(
            ui,
            "Engine Options",
            Some("Forwarded to every engine as UCI options."),
            |ui| {
                egui::Grid::new("tc_common_grid")
                    .num_columns(3)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        ui.checkbox(&mut f.threads_on, "");
                        field_label(ui, "Threads");
                        ui.add_enabled(
                            f.threads_on,
                            DragValue::new(&mut f.threads).range(1..=1024).speed(0.1),
                        );
                        ui.end_row();

                        ui.checkbox(&mut f.hash_on, "");
                        field_label(ui, "Hash (MB)");
                        ui.add_enabled(
                            f.hash_on,
                            DragValue::new(&mut f.hash_mb)
                                .range(1..=1_048_576)
                                .speed(1.0),
                        );
                        ui.end_row();

                        ui.checkbox(&mut f.syzygy50_on, "");
                        field_label(ui, "Syzygy50MoveRule");
                        ui.add_enabled_ui(f.syzygy50_on, |ui| {
                            ui.checkbox(&mut f.syzygy50, "on");
                        });
                        ui.end_row();
                    });

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    field_label(ui, "SyzygyPath");
                    ui.add(
                        egui::TextEdit::singleline(&mut f.syzygy_path)
                            .desired_width(240.0)
                            .hint_text("tablebase folder (optional)"),
                    );
                    if ui
                        .small_button(RichText::new("Browse…").color(theme::TEXT_WEAK))
                        .clicked()
                        && let Some(dir) = rfd::FileDialog::new()
                            .set_title("Select Syzygy tablebase folder")
                            .pick_folder()
                    {
                        f.syzygy_path = dir.to_string_lossy().to_string();
                    }
                });
                ui.add_space(4.0);
                ui.checkbox(&mut f.ponder, "Ponder").on_hover_text(
                    "Let engines think on the opponent's time. Off keeps fast games fair.",
                );
            },
        );

        // ── Adjudication ──
        widgets::section_card(
            ui,
            "Adjudication",
            Some("Natural endings (mate, stalemate, 50-move, repetition) are always detected."),
            |ui| {
                ui.horizontal(|ui| {
                    ui.checkbox(&mut f.max_moves_on, "Max moves");
                    ui.add_enabled(
                        f.max_moves_on,
                        DragValue::new(&mut f.max_moves).range(1..=2000).speed(1.0),
                    )
                    .on_hover_text("Declare a draw after this many full moves.");
                });

                ui.checkbox(&mut f.draw_on, "Draw adjudication");
                ui.add_enabled_ui(f.draw_on, |ui| {
                    egui::Grid::new("tc_draw_grid")
                        .num_columns(2)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            field_label(ui, "Min ply");
                            ui.add(
                                DragValue::new(&mut f.draw_min_ply)
                                    .range(0..=400)
                                    .speed(1.0),
                            );
                            ui.end_row();
                            field_label(ui, "Moves");
                            ui.add(
                                DragValue::new(&mut f.draw_move_count)
                                    .range(1..=50)
                                    .speed(0.1),
                            );
                            ui.end_row();
                            field_label(ui, "Score ≤ (cp)");
                            ui.add(
                                DragValue::new(&mut f.draw_score_cp)
                                    .range(0..=200)
                                    .speed(0.5),
                            );
                            ui.end_row();
                        });
                });

                ui.checkbox(&mut f.resign_on, "Resign (win/loss) adjudication");
                ui.add_enabled_ui(f.resign_on, |ui| {
                    egui::Grid::new("tc_resign_grid")
                        .num_columns(2)
                        .spacing([10.0, 4.0])
                        .show(ui, |ui| {
                            field_label(ui, "Moves");
                            ui.add(
                                DragValue::new(&mut f.resign_move_count)
                                    .range(1..=50)
                                    .speed(0.1),
                            );
                            ui.end_row();
                            field_label(ui, "Score ≥ (cp)");
                            ui.add(
                                DragValue::new(&mut f.resign_score_cp)
                                    .range(0..=10_000)
                                    .speed(5.0),
                            );
                            ui.end_row();
                        });
                });
            },
        );

        // ── Elo ──
        widgets::section_card(ui, "Elo", None, |ui| {
            egui::Grid::new("tc_elo_grid")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    field_label(ui, "Update policy");
                    egui::ComboBox::from_id_salt("elo_policy")
                        .selected_text(elo_policy_label(f.elo_policy))
                        .width(180.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut f.elo_policy,
                                EloPolicy::PerGame,
                                "After every game",
                            );
                            ui.selectable_value(
                                &mut f.elo_policy,
                                EloPolicy::EndOfTournament,
                                "At end of tournament",
                            );
                            ui.selectable_value(&mut f.elo_policy, EloPolicy::Never, "Never");
                        });
                    ui.end_row();

                    field_label(ui, "K-factor");
                    ui.add_enabled(
                        f.elo_policy != EloPolicy::Never,
                        DragValue::new(&mut f.k_factor)
                            .range(1.0..=100.0)
                            .speed(0.5),
                    );
                    ui.end_row();
                });
        });

        // ── Openings ──
        widgets::section_card(ui, "Openings", None, |ui| {
            if ui
                .checkbox(&mut f.openings_on, "Use an opening book")
                .on_hover_text(
                    "Draw one starting position per engine pair (both colours share it). \
                     Without a book, every game starts from the standard position.",
                )
                .changed()
            {
                f.refresh_openings_preview();
            }

            if f.openings_on {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    field_label(ui, "File");
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut f.openings_path)
                                .desired_width(220.0)
                                .hint_text("EPD or PGN file"),
                        )
                        .changed()
                    {
                        f.refresh_openings_preview();
                    }
                    if ui
                        .small_button(RichText::new("Browse…").color(theme::TEXT_WEAK))
                        .clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_title("Choose opening book")
                            .add_filter("Openings", &["epd", "pgn"])
                            .add_filter("All files", &["*"])
                            .pick_file()
                    {
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            f.openings_format = OpeningFormat::from_extension(ext);
                        }
                        f.openings_path = path.to_string_lossy().to_string();
                        f.refresh_openings_preview();
                    }
                });

                egui::Grid::new("tc_openings_grid")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        field_label(ui, "Format");
                        ui.horizontal(|ui| {
                            let mut changed = ui
                                .selectable_value(
                                    &mut f.openings_format,
                                    OpeningFormat::Epd,
                                    "EPD",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut f.openings_format,
                                    OpeningFormat::Pgn,
                                    "PGN",
                                )
                                .changed();
                            if changed {
                                f.refresh_openings_preview();
                            }
                        });
                        ui.end_row();

                        field_label(ui, "Order");
                        ui.horizontal(|ui| {
                            let mut changed = ui
                                .selectable_value(
                                    &mut f.openings_order,
                                    OpeningOrder::Sequential,
                                    "Sequential",
                                )
                                .changed();
                            changed |= ui
                                .selectable_value(
                                    &mut f.openings_order,
                                    OpeningOrder::Random,
                                    "Random",
                                )
                                .changed();
                            if f.openings_order == OpeningOrder::Random {
                                ui.add_space(6.0);
                                field_label(ui, "seed");
                                changed |= ui
                                    .add(DragValue::new(&mut f.openings_seed).speed(1.0))
                                    .changed();
                            }
                            if changed {
                                f.refresh_openings_preview();
                            }
                        });
                        ui.end_row();

                        if f.openings_format == OpeningFormat::Pgn {
                            field_label(ui, "Plies from PGN");
                            if ui
                                .add(
                                    DragValue::new(&mut f.openings_plies)
                                        .range(1..=60)
                                        .speed(0.2),
                                )
                                .on_hover_text("Half-moves to play out from each PGN game.")
                                .changed()
                            {
                                f.refresh_openings_preview();
                            }
                            ui.end_row();
                        }

                        field_label(ui, "Limit count");
                        ui.horizontal(|ui| {
                            let mut changed =
                                ui.checkbox(&mut f.openings_count_on, "").changed();
                            changed |= ui
                                .add_enabled(
                                    f.openings_count_on,
                                    DragValue::new(&mut f.openings_count)
                                        .range(1..=100_000)
                                        .speed(1.0),
                                )
                                .changed();
                            if changed {
                                f.refresh_openings_preview();
                            }
                        });
                        ui.end_row();
                    });

                match &f.openings_preview {
                    Some(Ok((count, sample))) => {
                        ui.label(
                            RichText::new(format!("✓ {count} openings loaded"))
                                .color(theme::SUCCESS)
                                .size(12.0),
                        );
                        if let Some(label) = sample {
                            ui.label(
                                RichText::new(format!("e.g. {}", truncate(label, 60)))
                                    .color(theme::TEXT_WEAK)
                                    .size(11.5),
                            );
                        }
                    }
                    Some(Err(e)) => {
                        ui.label(
                            RichText::new(format!("⚠ {e}"))
                                .color(theme::DANGER)
                                .size(12.0),
                        );
                    }
                    None => {
                        ui.label(
                            RichText::new("Choose a file to preview its openings.")
                                .color(theme::TEXT_WEAK)
                                .size(11.5),
                        );
                    }
                }
            }
        });

        // ── Output ──
        widgets::section_card(ui, "Output", None, |ui| {
            ui.horizontal(|ui| {
                field_label(ui, "PGN file");
                ui.add(
                    egui::TextEdit::singleline(&mut f.pgn_path)
                        .desired_width(240.0)
                        .hint_text("append finished games here (optional)"),
                );
                if ui
                    .small_button(RichText::new("Browse…").color(theme::TEXT_WEAK))
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_title("Choose PGN output file")
                        .add_filter("PGN", &["pgn"])
                        .save_file()
                {
                    f.pgn_path = path.to_string_lossy().to_string();
                }
            });
        });

        ui.add_space(4.0);
    }
}

// ── Live view ───────────────────────────────────────────────────────────────────

impl TournamentTab {
    fn show_live(&mut self, ui: &mut Ui, backend: &mut Backend) {
        // Snapshot the backend state into owned data, releasing the lock quickly.
        let live = LiveData::capture(backend);

        // Control bar (top).
        egui::Panel::top("tournament_live_controls")
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_DARKEST)
                    .inner_margin(egui::Margin::symmetric(14, 10)),
            )
            .show_inside(ui, |ui| {
                self.live_control_bar(ui, backend, &live);
            });

        // Errors panel (bottom), only when there are errors.
        if !live.errors.is_empty() {
            egui::Panel::bottom("tournament_errors")
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
                                    .size(12.5)
                                    .strong(),
                            );
                            ScrollArea::vertical()
                                .id_salt("tournament_errors_scroll")
                                .max_height(80.0)
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

        // Results table (centre).
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                top: 8,
                ..Default::default()
            }))
            .show_inside(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("tournament_results_scroll")
                    .show(ui, |ui| {
                        self.results_table(ui, &live);
                        if self.show_h2h {
                            ui.add_space(16.0);
                            head_to_head_matrix(ui, &live);
                        }
                    });
            });
    }

    fn live_control_bar(&mut self, ui: &mut Ui, backend: &mut Backend, live: &LiveData) {
        ui.horizontal(|ui| {
            // Status pill + tournament name.
            let (label, dot, color) = status_pill_parts(live.status);
            widgets::status_pill(ui, label, dot, color);
            ui.add_space(8.0);
            ui.label(
                RichText::new(&live.name)
                    .color(theme::TEXT)
                    .size(15.0)
                    .strong(),
            );

            ui.add_space(12.0);

            // Tinted action buttons.
            let status = live.status;
            let go_enabled = matches!(status, TournamentStatus::Stopped | TournamentStatus::Idle);
            let stop_enabled = matches!(status, TournamentStatus::Running);
            let force_enabled = matches!(
                status,
                TournamentStatus::Running | TournamentStatus::Stopping
            );

            if widgets::tinted_button(ui, "▶ Go", theme::SUCCESS, go_enabled)
                .on_hover_text("Resume the tournament.")
                .clicked()
                && let Some(active) = &backend.active
            {
                active.handle.go();
            }

            if widgets::tinted_button(ui, "⏸ Stop", theme::WARN, stop_enabled)
                .on_hover_text("Stop launching new games; let in-flight games finish.")
                .clicked()
                && let Some(active) = &backend.active
            {
                active.handle.stop();
            }

            if widgets::tinted_button(ui, "⏹ Force-Stop", theme::DANGER, force_enabled)
                .on_hover_text("Abort in-flight games immediately (discarding them).")
                .clicked()
                && let Some(active) = &backend.active
            {
                active.handle.force_stop();
            }

            ui.add_space(14.0);

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
                    .desired_width(160.0)
                    .desired_height(6.0)
                    .corner_radius(4.0)
                    .fill(theme::ACCENT),
            );

            // Right side: apply-elo + h2h toggle + New Tournament.
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                let new_enabled = !crate::backend::is_busy(status);
                if ui
                    .add_enabled(
                        new_enabled,
                        egui::Button::new(RichText::new("New Tournament").size(13.0))
                            .fill(theme::BG_ELEVATED)
                            .stroke(egui::Stroke::new(1.0, theme::STROKE)),
                    )
                    .on_hover_text(if new_enabled {
                        "Return to setup to configure another tournament."
                    } else {
                        "Stop the tournament first."
                    })
                    .clicked()
                {
                    backend.clear_active();
                }
                ui.add_space(6.0);
                ui.toggle_value(
                    &mut self.show_h2h,
                    RichText::new("Head-to-head").size(13.0),
                );
                ui.add_space(6.0);
                if widgets::tinted_button(ui, "Apply Elo → Library", theme::ACCENT, new_enabled)
                    .on_hover_text("Write tournament Elo ratings back to the engine library.")
                    .clicked()
                {
                    let n = backend.apply_active_elo_to_library();
                    self.elo_note = Some(format!("Elo applied ({n} engines updated)"));
                }
                if let Some(note) = &self.elo_note {
                    ui.label(RichText::new(note).color(theme::SUCCESS).size(12.0));
                }
            });
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
            .column(Column::auto().at_least(34.0)) // rank
            .column(Column::initial(170.0).at_least(110.0).clip(true)) // name
            .column(Column::auto().at_least(54.0)) // version
            .column(Column::auto().at_least(56.0)) // elo
            .column(Column::auto().at_least(72.0)) // elo delta chip
            .column(Column::auto().at_least(54.0)) // points
            .column(Column::auto().at_least(48.0)) // games
            .column(Column::auto().at_least(82.0)) // w-d-l
            .column(Column::remainder().at_least(80.0)) // nps
            .header(header_h, |mut header| {
                header.col(|ui| {
                    ui.label(strong_header("#"));
                });
                header.col(|ui| {
                    sortable_header(ui, "Engine", SortKey::Name, &mut self.sort);
                });
                header.col(|ui| {
                    ui.label(strong_header("Ver"));
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
            })
            .body(|mut body| {
                for row in &rows {
                    body.row(row_h, |mut tr| {
                        tr.col(|ui| {
                            widgets::rank_badge(ui, row.rank);
                        });
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
}

// ── Head-to-head matrix ─────────────────────────────────────────────────────────

fn head_to_head_matrix(ui: &mut Ui, live: &LiveData) {
    ui.label(
        RichText::new("Head-to-head")
            .color(theme::TEXT)
            .size(14.0)
            .strong(),
    );
    ui.label(
        RichText::new("Row engine's record (W–D–L) against each column engine.")
            .color(theme::TEXT_WEAK)
            .size(11.5),
    );
    ui.add_space(6.0);

    // Order by current standings rank.
    let order: Vec<&Row> = {
        let mut r: Vec<&Row> = live.rows.iter().collect();
        r.sort_by_key(|row| row.rank);
        r
    };

    ScrollArea::horizontal()
        .id_salt("h2h_scroll")
        .show(ui, |ui| {
            egui::Grid::new("h2h_matrix")
                .striped(true)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    // Header row.
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
                                    let games = h2h.games() as f32;
                                    let s = (h2h.wins as f32 + 0.5 * h2h.draws as f32) / games;
                                    let fill = h2h_cell_fill(s);
                                    egui::Frame::new()
                                        .fill(fill)
                                        .corner_radius(egui::CornerRadius::same(4))
                                        .inner_margin(egui::Margin::symmetric(6, 2))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}-{}-{}",
                                                    h2h.wins, h2h.draws, h2h.losses
                                                ))
                                                .color(theme::TEXT)
                                                .monospace()
                                                .size(11.5),
                                            );
                                        });
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
        });
}

// ── Live data capture ───────────────────────────────────────────────────────────

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
}

impl LiveData {
    fn capture(backend: &Backend) -> Self {
        let active = backend
            .active
            .as_ref()
            .expect("capture called with an active tournament");

        let (status, standings, elo, finished, total, errors) = {
            let snap = active.snapshot.lock().unwrap();
            (
                snap.status,
                snap.standings.clone(),
                snap.elo.clone(),
                snap.games_finished,
                snap.games_total,
                snap.recent_errors.clone(),
            )
        };

        // Rank map by points (descending).
        let ranked = standings.ranked_by_points();
        let rank_of = |id: EngineId| ranked.iter().position(|x| x == &id).map_or(0, |p| p + 1);

        let mut rows: Vec<Row> = active
            .participants
            .iter()
            .map(|p: &ParticipantInfo| {
                let st = standings.standing(p.id);
                let e = elo.get(&p.id).copied().unwrap_or(EloEntry::default());
                Row {
                    id: p.id,
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

        // Default order: by rank.
        rows.sort_by_key(|r| r.rank);

        Self {
            name: active.name.clone(),
            status,
            finished,
            total,
            rows,
            standings,
            errors,
        }
    }
}

// ── Sorting ─────────────────────────────────────────────────────────────────────

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
    let arrow = if active {
        if sort.ascending { " ▲" } else { " ▼" }
    } else {
        ""
    };
    let text = RichText::new(format!("{label}{arrow}"))
        .color(if active { theme::ACCENT } else { theme::TEXT })
        .size(12.5)
        .strong();
    if ui.add(egui::Button::new(text).frame(false)).clicked() {
        if active {
            sort.ascending = !sort.ascending;
        } else {
            sort.key = key;
            // Sensible default direction per column.
            sort.ascending = matches!(key, SortKey::Name | SortKey::Rank);
        }
    }
}

fn strong_header(label: &str) -> RichText {
    RichText::new(label).color(theme::TEXT).size(12.5).strong()
}

// ── Small helpers ───────────────────────────────────────────────────────────────

fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::TEXT_WEAK).size(13.0));
}

fn engine_display_name(e: &EngineConfig) -> String {
    if e.meta.name.is_empty() {
        e.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string()
    } else {
        e.meta.name.clone()
    }
}

fn elo_policy_label(p: EloPolicy) -> &'static str {
    match p {
        EloPolicy::PerGame => "After every game",
        EloPolicy::EndOfTournament => "At end of tournament",
        EloPolicy::Never => "Never",
    }
}

fn status_pill_parts(status: TournamentStatus) -> (&'static str, &'static str, Color32) {
    match status {
        TournamentStatus::Running => ("Running", "●", theme::SUCCESS),
        TournamentStatus::Stopping => ("Stopping", "●", theme::WARN),
        TournamentStatus::Stopped => ("Stopped", "●", theme::TEXT_WEAK),
        TournamentStatus::Finished => ("Finished", "●", theme::ACCENT),
        TournamentStatus::Idle => ("Idle", "○", theme::TEXT_FAINT),
    }
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

fn format_nps(nps: Option<u64>) -> String {
    match nps {
        None => "—".to_string(),
        Some(n) if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        Some(n) if n >= 1_000 => format!("{:.0}k", n as f64 / 1_000.0),
        Some(n) => n.to_string(),
    }
}

/// Truncate `s` to at most `max` characters, appending an ellipsis if cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// Truncate a name for compact matrix headers.
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
            nps,
        }
    }

    #[test]
    fn default_form_matches_config_defaults() {
        let form = TournamentForm::default();
        let cfg = form.build_config();
        let default = TournamentConfig::default();
        // The form's defaults reproduce the canonical TournamentConfig default.
        assert_eq!(cfg.games_per_pair, default.games_per_pair);
        assert_eq!(cfg.concurrency, default.concurrency);
        assert_eq!(cfg.common.threads, Some(1));
        assert_eq!(cfg.common.hash_mb, None);
        assert!(!cfg.common.ponder);
        assert_eq!(cfg.time_control, TimeControl::PerMove { ms: 100 });
        assert_eq!(cfg.elo_policy, EloPolicy::PerGame);
        assert!(cfg.adjudication.max_moves.is_none());
        assert!(cfg.adjudication.draw.is_none());
        assert!(cfg.adjudication.resign.is_none());
        assert!(cfg.pgn_output.is_none());
        // Openings off by default => standard start position.
        assert_eq!(cfg.start_position, colosseum_core::StartPosition::Startpos);
    }

    #[test]
    fn opening_book_maps_into_config() {
        // Disabled, or enabled without a path => no book.
        let mut form = TournamentForm {
            openings_on: true,
            ..TournamentForm::default()
        };
        assert!(form.opening_book().is_none());

        form.openings_path = "C:/books/silver.epd".to_string();
        form.openings_format = OpeningFormat::Epd;
        form.openings_order = OpeningOrder::Random;
        form.openings_seed = 7;
        form.openings_count_on = true;
        form.openings_count = 50;
        let book = form.opening_book().expect("a book");
        assert_eq!(book.format, OpeningFormat::Epd);
        assert_eq!(book.order, OpeningOrder::Random);
        assert_eq!(book.seed, 7);
        assert_eq!(book.count, Some(50));

        // It flows into the built config.
        match form.build_config().start_position {
            colosseum_core::StartPosition::Book(b) => assert_eq!(b.count, Some(50)),
            colosseum_core::StartPosition::Startpos => panic!("expected a book"),
        }
    }

    #[test]
    fn time_unit_conversion_into_config() {
        let mut form = TournamentForm {
            tc_value: 2.0,
            tc_unit: TimeUnit::Seconds,
            ..TournamentForm::default()
        };
        assert_eq!(
            form.build_config().time_control,
            TimeControl::PerMove { ms: 2_000 }
        );
        form.tc_unit = TimeUnit::Minutes;
        form.tc_value = 1.0;
        assert_eq!(
            form.build_config().time_control,
            TimeControl::PerMove { ms: 60_000 }
        );
        // The fast lower bound the responsiveness probe relies on.
        form.tc_unit = TimeUnit::Milliseconds;
        form.tc_value = 10.0;
        assert_eq!(
            form.build_config().time_control,
            TimeControl::PerMove { ms: 10 }
        );
    }

    #[test]
    fn adjudication_toggles_populate_config() {
        let form = TournamentForm {
            max_moves_on: true,
            max_moves: 200,
            draw_on: true,
            resign_on: true,
            ..TournamentForm::default()
        };
        let adj = form.build_config().adjudication;
        assert_eq!(adj.max_moves, Some(200));
        assert!(adj.draw.is_some());
        assert!(adj.resign.is_some());
    }

    #[test]
    fn estimated_games_round_robin() {
        let mut form = TournamentForm {
            selected: vec![EngineId::new(), EngineId::new(), EngineId::new()],
            ..TournamentForm::default()
        };
        // 3 engines -> 3 pairs * 2 games/pair * 1 cycle = 6.
        assert_eq!(form.estimated_games(), 6);
        form.cycles = 2;
        assert_eq!(form.estimated_games(), 12);
        form.selected.truncate(1);
        assert_eq!(form.estimated_games(), 0);
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
}
