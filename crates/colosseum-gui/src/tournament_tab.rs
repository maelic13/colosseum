// SPDX-License-Identifier: GPL-3.0-or-later
//! Tournament tab: configure and launch a tournament, then watch it live.
//!
//! Two views share one tab:
//! - **Setup** (no active tournament): engine selection + all tournament
//!   options, with a prominent Start button.
//! - **Live** (a tournament exists): Go / Stop / Force-Stop controls, a progress
//!   readout, a sortable results table, an optional head-to-head matrix, and an
//!   engine-error panel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, DragValue, Layout, RichText, ScrollArea, Ui};

use colosseum_core::{
    AdjudicationConfig, CommonEngineOptions, DrawAdjudication, EloPolicy, EngineConfig, EngineId,
    Format, OpeningBook, OpeningFormat, OpeningOrder, RatingWriteback, ResignAdjudication,
    StartPosition, TimeControl, TimeUnit, TournamentConfig, UciOption, UciOptionValue,
};
use colosseum_engine::{AppConfig, summarize};

use crate::backend::Backend;
use crate::presets::{PresetData, PresetFormatKind, PresetManager, PresetTcKind};
use crate::theme;
use crate::widgets;

// ── Tab state ─────────────────────────────────────────────────────────────────

/// All persistent state for the Tournament tab (setup only — the live view
/// lives in the Results tab).
pub struct TournamentTab {
    form: TournamentForm,
    /// Filter text for the engine-selection list.
    engine_filter: String,
    /// Engine whose per-tournament UCI overrides are being edited in a modal.
    override_editor: Option<EngineId>,
    /// Whether the compatibility-notes modal is open.
    show_compat_notes: bool,
    /// Decoded logo textures for the engine-selection list.
    logos: crate::logo::LogoCache,
    start_error: Option<String>,
    /// Set when a tournament was just started or resumed; the app shell takes
    /// it (via [`Self::take_started`]) to switch to the Results tab.
    just_started: bool,
    /// Manages preset files on disk.
    preset_manager: PresetManager,
    /// Cached list of named presets (refreshed after save/delete).
    presets_cache: Vec<PresetData>,
    /// The name typed into the "Save preset" field.
    preset_save_name: String,
}

impl TournamentTab {
    /// Initialise the tab, loading the last-used config (if any) from disk.
    pub fn new(config_dir: &Path) -> Self {
        let preset_manager = PresetManager::new(config_dir);
        let presets_cache = preset_manager.load_all();
        let mut form = TournamentForm::default();
        if let Some(last) = preset_manager.load_last_used() {
            form.apply_preset(&last);
            // The name identifies one run, not a setting worth restoring —
            // every fresh setup starts as plain "Tournament". Loading a named
            // preset from the menu still applies that preset's name.
            form.name = "Tournament".to_string();
        }
        Self {
            form,
            engine_filter: String::new(),
            override_editor: None,
            show_compat_notes: false,
            logos: crate::logo::LogoCache::default(),
            start_error: None,
            just_started: false,
            preset_manager,
            presets_cache,
            preset_save_name: String::new(),
        }
    }

    /// Draw the tab body. Call every frame.
    pub fn show(&mut self, ui: &mut Ui, backend: &mut Backend) {
        self.logos.begin_frame();
        self.show_setup(ui, backend);
    }

    /// True once, right after a tournament was started or resumed here — the
    /// app shell switches to the Results tab in response.
    pub fn take_started(&mut self) -> bool {
        std::mem::take(&mut self.just_started)
    }
}

// ── Configuration form ──────────────────────────────────────────────────────────

/// Tournament-format kinds offered in the setup UI. Round Robin and Gauntlet are
/// fully implemented; Knockout and SPRT require a result-dependent (dynamic)
/// scheduler and are shown as disabled "planned" options.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FormatKind {
    RoundRobin,
    Gauntlet,
    Knockout,
    Sprt,
}

impl FormatKind {
    fn label(self) -> &'static str {
        match self {
            Self::RoundRobin => "Round Robin",
            Self::Gauntlet => "Gauntlet",
            Self::Knockout => "Knockout (planned)",
            Self::Sprt => "SPRT (planned)",
        }
    }

    /// Whether the format is wired up to a working scheduler.
    fn is_supported(self) -> bool {
        matches!(self, Self::RoundRobin | Self::Gauntlet)
    }
}

/// Time-control kinds offered in the setup UI, each mapping to a [`TimeControl`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum TcKind {
    PerMove,
    SuddenDeath,
    Increment,
    Nodes,
    Depth,
}

impl TcKind {
    fn label(self) -> &'static str {
        match self {
            Self::PerMove => "Time per move",
            Self::SuddenDeath => "Sudden death",
            Self::Increment => "Base + increment",
            Self::Nodes => "Fixed nodes",
            Self::Depth => "Fixed depth",
        }
    }
}

/// How library ratings are updated when the tournament finishes (form-side
/// mirror of [`RatingWriteback`], with the estimate target kept separately).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum WritebackKind {
    /// Ratings are never written back (default).
    #[default]
    Never,
    /// Every participant's final tournament rating is written back.
    All,
    /// Only one engine is updated, to its performance rating against the
    /// others' fixed library ratings.
    Estimate,
}

impl WritebackKind {
    fn label(self) -> &'static str {
        match self {
            Self::Never => "Never",
            Self::All => "All engines",
            Self::Estimate => "Estimate one engine",
        }
    }
}

/// GUI-friendly buffer for a [`TournamentConfig`] plus engine selection.
struct TournamentForm {
    name: String,
    /// Selected engine ids, in selection order (seeding order).
    selected: Vec<EngineId>,

    // Format
    format_kind: FormatKind,
    cycles: u32,
    games_per_pair: u32,
    /// Number of leading (seed) engines for the Gauntlet format.
    gauntlet_seeds: u32,

    // Time control
    tc_kind: TcKind,
    tc_value: f64,
    tc_unit: TimeUnit,
    /// Increment value/unit for the base+increment control.
    tc_inc_value: f64,
    tc_inc_unit: TimeUnit,
    /// Fixed node count per move.
    tc_nodes: u64,
    /// Fixed search depth per move.
    tc_depth: u32,

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
    /// Live in-tournament model cadence; not exposed in the UI (always
    /// per-game so the standings Elo column ticks), kept for preset compat.
    elo_policy: EloPolicy,
    k_factor: f64,
    /// How library ratings are updated when the tournament finishes.
    elo_writeback: WritebackKind,
    /// Target for [`WritebackKind::Estimate`]; falls back to the first
    /// selected engine (the gauntlet engine) when unset or deselected.
    estimate_target: Option<EngineId>,

    /// Per-engine UCI overrides for this tournament only (highest precedence;
    /// beats library options and the tournament's common options).
    overrides: HashMap<EngineId, std::collections::BTreeMap<String, UciOptionValue>>,

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
            name: "Tournament".to_string(),
            selected: Vec::new(),
            format_kind: FormatKind::RoundRobin,
            cycles: 1,
            games_per_pair: 2,
            gauntlet_seeds: 1,
            tc_kind: TcKind::PerMove,
            tc_value: 100.0,
            tc_unit: TimeUnit::Milliseconds,
            tc_inc_value: 1.0,
            tc_inc_unit: TimeUnit::Seconds,
            tc_nodes: 100_000,
            tc_depth: 12,
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
            elo_writeback: WritebackKind::Never,
            estimate_target: None,
            overrides: HashMap::new(),
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
            format: self.build_format(),
            games_per_pair: self.games_per_pair.clamp(1, 2),
            time_control: self.build_time_control(),
            concurrency: self.concurrency.max(1),
            common: CommonEngineOptions {
                threads: self.threads_on.then_some(self.threads.max(1)),
                hash_mb: self.hash_on.then_some(self.hash_mb),
                syzygy_path: (!self.syzygy_path.trim().is_empty())
                    .then(|| self.syzygy_path.trim().to_string()),
                // Managed globally with the tablebase paths (Engines tab).
                syzygy_50_move_rule: None,
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
            // Live model always ticks per game so the standings Elo column is
            // informative; whether anything is *written back* is separate.
            elo_policy: EloPolicy::PerGame,
            k_factor: self.k_factor.max(1.0),
            rating_writeback: match self.elo_writeback {
                WritebackKind::Never => RatingWriteback::None,
                WritebackKind::All => RatingWriteback::All,
                WritebackKind::Estimate => match self.estimate_engine() {
                    Some(id) => RatingWriteback::Estimate(id),
                    None => RatingWriteback::None,
                },
            },
            start_position: match self.opening_book() {
                Some(book) => StartPosition::Book(book),
                None => StartPosition::Startpos,
            },
            pgn_output: (!self.pgn_path.trim().is_empty())
                .then(|| PathBuf::from(self.pgn_path.trim())),
            // Only overrides for engines actually in the tournament.
            engine_overrides: self
                .overrides
                .iter()
                .filter(|(id, map)| self.selected.contains(id) && !map.is_empty())
                .map(|(id, map)| (*id, map.clone()))
                .collect(),
        }
    }

    /// Map the form's format selection to a core [`Format`]. Unsupported kinds fall
    /// back to Round Robin (the UI prevents selecting them, but be defensive).
    fn build_format(&self) -> Format {
        let cycles = self.cycles.max(1);
        match self.format_kind {
            FormatKind::Gauntlet => Format::Gauntlet {
                seeds: self.gauntlet_seeds.max(1),
                cycles,
            },
            _ => Format::RoundRobin { cycles },
        }
    }

    /// Map the form's time-control selection to a core [`TimeControl`].
    fn build_time_control(&self) -> TimeControl {
        match self.tc_kind {
            TcKind::PerMove => TimeControl::PerMove {
                ms: self.tc_unit.to_millis(self.tc_value).max(1),
            },
            TcKind::SuddenDeath => TimeControl::SuddenDeath {
                base_ms: self.tc_unit.to_millis(self.tc_value).max(1),
            },
            TcKind::Increment => TimeControl::Increment {
                base_ms: self.tc_unit.to_millis(self.tc_value).max(1),
                inc_ms: self.tc_inc_unit.to_millis(self.tc_inc_value),
            },
            TcKind::Nodes => TimeControl::Nodes {
                nodes: self.tc_nodes.max(1),
            },
            TcKind::Depth => TimeControl::Depth {
                depth: self.tc_depth.max(1),
            },
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

    /// The engine whose rating the Estimate writeback updates: the explicit
    /// pick while it stays selected, otherwise the first selected engine
    /// (which is also the gauntlet engine).
    fn estimate_engine(&self) -> Option<EngineId> {
        self.estimate_target
            .filter(|id| self.selected.contains(id))
            .or_else(|| self.selected.first().copied())
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

    /// Capture the current form state as a named [`PresetData`].
    fn to_preset(&self, preset_name: String) -> PresetData {
        PresetData {
            preset_name,
            tournament_name: self.name.clone(),
            format_kind: match self.format_kind {
                FormatKind::RoundRobin => PresetFormatKind::RoundRobin,
                FormatKind::Gauntlet => PresetFormatKind::Gauntlet,
                FormatKind::Knockout => PresetFormatKind::Knockout,
                FormatKind::Sprt => PresetFormatKind::Sprt,
            },
            cycles: self.cycles,
            games_per_pair: self.games_per_pair,
            gauntlet_seeds: self.gauntlet_seeds,
            tc_kind: match self.tc_kind {
                TcKind::PerMove => PresetTcKind::PerMove,
                TcKind::SuddenDeath => PresetTcKind::SuddenDeath,
                TcKind::Increment => PresetTcKind::Increment,
                TcKind::Nodes => PresetTcKind::Nodes,
                TcKind::Depth => PresetTcKind::Depth,
            },
            tc_value: self.tc_value,
            tc_unit: self.tc_unit,
            tc_inc_value: self.tc_inc_value,
            tc_inc_unit: self.tc_inc_unit,
            tc_nodes: self.tc_nodes,
            tc_depth: self.tc_depth,
            concurrency: self.concurrency,
            threads_on: self.threads_on,
            threads: self.threads,
            hash_on: self.hash_on,
            hash_mb: self.hash_mb,
            syzygy_path: self.syzygy_path.clone(),
            syzygy50_on: self.syzygy50_on,
            syzygy50: self.syzygy50,
            ponder: self.ponder,
            max_moves_on: self.max_moves_on,
            max_moves: self.max_moves,
            draw_on: self.draw_on,
            draw_min_ply: self.draw_min_ply,
            draw_move_count: self.draw_move_count,
            draw_score_cp: self.draw_score_cp,
            resign_on: self.resign_on,
            resign_move_count: self.resign_move_count,
            resign_score_cp: self.resign_score_cp,
            elo_policy: self.elo_policy,
            k_factor: self.k_factor,
            elo_writeback: match self.elo_writeback {
                WritebackKind::Never => "never",
                WritebackKind::All => "all",
                WritebackKind::Estimate => "estimate",
            }
            .to_string(),
            openings_on: self.openings_on,
            openings_path: self.openings_path.clone(),
            openings_format: self.openings_format,
            openings_order: self.openings_order,
            openings_plies: self.openings_plies,
            openings_count_on: self.openings_count_on,
            openings_count: self.openings_count,
            openings_seed: self.openings_seed,
            pgn_path: self.pgn_path.clone(),
        }
    }

    /// Apply a preset's settings to this form.  Engine selection and the
    /// openings preview cache are not touched; the preview is re-derived from
    /// the new openings settings.
    fn apply_preset(&mut self, p: &PresetData) {
        self.name = p.tournament_name.clone();
        self.format_kind = match p.format_kind {
            PresetFormatKind::RoundRobin => FormatKind::RoundRobin,
            PresetFormatKind::Gauntlet => FormatKind::Gauntlet,
            PresetFormatKind::Knockout => FormatKind::Knockout,
            PresetFormatKind::Sprt => FormatKind::Sprt,
        };
        self.cycles = p.cycles;
        // Repetition beyond both colours is what Cycles is for.
        self.games_per_pair = p.games_per_pair.clamp(1, 2);
        self.gauntlet_seeds = p.gauntlet_seeds;
        self.tc_kind = match p.tc_kind {
            PresetTcKind::PerMove => TcKind::PerMove,
            PresetTcKind::SuddenDeath => TcKind::SuddenDeath,
            PresetTcKind::Increment => TcKind::Increment,
            PresetTcKind::Nodes => TcKind::Nodes,
            PresetTcKind::Depth => TcKind::Depth,
        };
        self.tc_value = p.tc_value;
        self.tc_unit = p.tc_unit;
        self.tc_inc_value = p.tc_inc_value;
        self.tc_inc_unit = p.tc_inc_unit;
        self.tc_nodes = p.tc_nodes;
        self.tc_depth = p.tc_depth;
        self.concurrency = p.concurrency;
        self.threads_on = p.threads_on;
        self.threads = p.threads;
        self.hash_on = p.hash_on;
        self.hash_mb = p.hash_mb;
        self.syzygy_path = p.syzygy_path.clone();
        self.syzygy50_on = p.syzygy50_on;
        self.syzygy50 = p.syzygy50;
        self.ponder = p.ponder;
        self.max_moves_on = p.max_moves_on;
        self.max_moves = p.max_moves;
        self.draw_on = p.draw_on;
        self.draw_min_ply = p.draw_min_ply;
        self.draw_move_count = p.draw_move_count;
        self.draw_score_cp = p.draw_score_cp;
        self.resign_on = p.resign_on;
        self.resign_move_count = p.resign_move_count;
        self.resign_score_cp = p.resign_score_cp;
        self.elo_policy = p.elo_policy;
        self.k_factor = p.k_factor;
        self.elo_writeback = match p.elo_writeback.as_str() {
            "all" => WritebackKind::All,
            "estimate" => WritebackKind::Estimate,
            _ => WritebackKind::Never,
        };
        self.openings_on = p.openings_on;
        self.openings_path = p.openings_path.clone();
        self.openings_format = p.openings_format;
        self.openings_order = p.openings_order;
        self.openings_plies = p.openings_plies;
        self.openings_count_on = p.openings_count_on;
        self.openings_count = p.openings_count;
        self.openings_seed = p.openings_seed;
        self.pgn_path = p.pgn_path.clone();
        self.refresh_openings_preview();
    }

    /// Number of games the configured tournament will schedule.
    fn estimated_games(&self) -> usize {
        let n = self.selected.len();
        if n < 2 {
            return 0;
        }
        let games_per_pair = self.games_per_pair.clamp(1, 2) as usize;
        let cycles = self.cycles.max(1) as usize;
        let pairs = match self.format_kind {
            FormatKind::Gauntlet => {
                // seeds * opponents, with seeds clamped to leave ≥1 opponent.
                let seeds = (self.gauntlet_seeds.max(1) as usize).min(n - 1);
                seeds * (n - seeds)
            }
            // Round Robin (and the unsupported kinds, which fall back to it).
            _ => n * (n - 1) / 2,
        };
        pairs * games_per_pair * cycles
    }

    /// Rough wall-clock estimate for one game in seconds, from the time
    /// control alone. `None` for nodes/depth controls (speed depends entirely
    /// on hardware and engine). Assumes ~[`EST_MOVES_PER_SIDE`] moves per
    /// side; sudden-death games are costed at the full budget (upper bound).
    fn estimated_game_secs(&self) -> Option<f64> {
        let per_side_ms = match self.tc_kind {
            TcKind::PerMove => {
                self.tc_unit.to_millis(self.tc_value).max(1) as f64 * EST_MOVES_PER_SIDE
            }
            TcKind::SuddenDeath => self.tc_unit.to_millis(self.tc_value).max(1) as f64,
            TcKind::Increment => {
                self.tc_unit.to_millis(self.tc_value).max(1) as f64
                    + self.tc_inc_unit.to_millis(self.tc_inc_value) as f64 * EST_MOVES_PER_SIDE
            }
            TcKind::Nodes | TcKind::Depth => return None,
        };
        Some(2.0 * per_side_ms / 1000.0)
    }

    /// Estimated wall-clock duration of the whole tournament in seconds,
    /// accounting for parallel games plus a little scheduling overhead.
    fn estimated_duration_secs(&self) -> Option<f64> {
        let games = self.estimated_games();
        if games == 0 {
            return None;
        }
        let per_game = self.estimated_game_secs()?;
        let lanes = self.concurrency.max(1) as f64;
        Some(per_game * games as f64 / lanes * 1.05)
    }
}

/// Assumed average moves per side per game for duration estimates. Fast
/// engine-vs-engine games typically finish (or get adjudicated) around here.
const EST_MOVES_PER_SIDE: f64 = 60.0;

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
        if rs == 0 { format!("{m}m") } else { format!("{m}m {rs:02}s") }
    } else if s < 86_400 {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}d {}h", s / 86_400, (s % 86_400) / 3600)
    }
}

// ── Setup view ──────────────────────────────────────────────────────────────────

impl TournamentTab {
    /// Modal editor for one engine's per-tournament UCI overrides.
    fn show_override_editor(&mut self, ctx: &egui::Context, backend: &Backend) {
        let Some(id) = self.override_editor else {
            return;
        };
        let Some(engine) = backend.engines.iter().find(|e| e.id == id) else {
            self.override_editor = None;
            return;
        };
        let name = widgets::engine_base_name(engine);
        let version = engine.meta.version.trim();
        let title = if version.is_empty() {
            name
        } else {
            format!("{name} {version}")
        };

        // Detected options with the engine's saved (library) values and the
        // tournament-wide Threads/Hash/Ponder as the displayed defaults, so
        // an untouched row shows exactly what would apply anyway.
        let common_threads = self.form.threads_on.then_some(self.form.threads);
        let common_hash = self.form.hash_on.then_some(self.form.hash_mb);
        let common_ponder = self.form.ponder;
        let opts = effective_options(engine, common_threads, common_hash, common_ponder);
        let overrides = self.form.overrides.entry(id).or_default();

        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("tournament_override_editor")).show(ctx, |ui| {
            ui.set_width(560.0);
            ui.label(
                RichText::new(format!("Tournament UCI options — {title}"))
                    .color(theme::text())
                    .font(theme::semibold(15.0)),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "Apply to this tournament only. Overrides beat both the engine's saved \
                     options and the tournament-wide Threads / Hash / Ponder.",
                )
                .color(theme::text_weak())
                .size(12.0),
            );
            ui.add_space(8.0);

            if opts.is_empty() {
                ui.label(
                    RichText::new("No UCI options detected for this engine.")
                        .color(theme::text_faint())
                        .size(12.5),
                );
            } else {
                let mut _dirty = false;
                ScrollArea::vertical()
                    .id_salt("override_editor_scroll")
                    .max_height(360.0)
                    // Fill the modal's width so the scrollbar sits at its
                    // right edge instead of hugging the grid content.
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        egui::Grid::new("override_editor_grid")
                            .num_columns(3)
                            .spacing([10.0, 7.0])
                            .show(ui, |ui| {
                                for opt in &opts {
                                    widgets::uci_option_row(ui, opt, overrides, &mut _dirty);
                                    if overrides.contains_key(opt.name()) {
                                        if ui
                                            .add(egui::Button::new(RichText::new("×").color(theme::text_faint())))
                                            .on_hover_text("Remove this override.")
                                            .clicked()
                                        {
                                            overrides.remove(opt.name());
                                        }
                                    } else {
                                        ui.label("");
                                    }
                                    ui.end_row();
                                }
                            });
                    });
            }

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let n = overrides.len();
                ui.label(
                    RichText::new(format!(
                        "{n} option{} overridden",
                        if n == 1 { "" } else { "s" }
                    ))
                    .color(if n > 0 { theme::accent() } else { theme::text_faint() })
                    .size(12.0),
                );
                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Done")
                                    .color(theme::bg_darkest())
                                    .font(theme::semibold(13.5)),
                            )
                            .fill(theme::accent()),
                        )
                        .clicked()
                    {
                        close = true;
                    }
                    ui.add_space(4.0);
                    if ui
                        .add_enabled(
                            n > 0,
                            egui::Button::new(
                                RichText::new("Reset all").color(theme::text_weak()),
                            ),
                        )
                        .on_hover_text("Remove every override — the engine plays with its \
                             saved options and the tournament-wide settings.")
                        .clicked()
                    {
                        overrides.clear();
                    }
                });
            });
        });

        if close || modal.should_close() {
            // Drop empty maps so the row indicator disappears cleanly.
            if self
                .form
                .overrides
                .get(&id)
                .is_some_and(std::collections::BTreeMap::is_empty)
            {
                self.form.overrides.remove(&id);
            }
            self.override_editor = None;
        }
    }

    /// Modal listing every compatibility note at once (badge click).
    fn show_compat_notes_modal(&mut self, ctx: &egui::Context, backend: &Backend) {
        if !self.show_compat_notes {
            return;
        }
        let notes = compatibility_notes(&self.form, &backend.engines);
        if notes.is_empty() {
            self.show_compat_notes = false;
            return;
        }

        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("compat_notes_modal")).show(ctx, |ui| {
            ui.set_width(480.0);
            ui.label(
                RichText::new("Compatibility notes")
                    .color(theme::text())
                    .font(theme::semibold(15.0)),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(
                    "These engines can't fully honour the current tournament settings. \
                     The tournament will still run — with the adjustments below.",
                )
                .color(theme::text_weak())
                .size(12.0),
            );
            ui.add_space(8.0);
            ScrollArea::vertical()
                .id_salt("compat_notes_scroll")
                .max_height(320.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for note in &notes {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("⚠").color(theme::warn()).size(13.0));
                            ui.label(RichText::new(note).color(theme::text()).size(13.0));
                        });
                        ui.add_space(4.0);
                    }
                });
            ui.add_space(8.0);
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Close")
                                .color(theme::bg_darkest())
                                .font(theme::semibold(13.5)),
                        )
                        .fill(theme::accent()),
                    )
                    .clicked()
                {
                    close = true;
                }
            });
        });
        if close || modal.should_close() {
            self.show_compat_notes = false;
        }
    }

    fn show_setup(&mut self, ui: &mut Ui, backend: &mut Backend) {
        self.show_override_editor(ui.ctx(), backend);
        self.show_compat_notes_modal(ui.ctx(), backend);

        // Bottom action bar (pinned).
        egui::Panel::bottom("tournament_setup_actions")
            .frame(
                egui::Frame::new()
                    .fill(theme::bg_darkest())
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

        // Settings (centre, scrollable). Left margin keeps the section cards
        // clear of the engine panel's separator line.
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 12,
                ..Default::default()
            }))
            .show_inside(ui, |ui| {
                ScrollArea::vertical()
                    .id_salt("tournament_settings_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.settings_form(ui, &backend.engines);
                    });
            });
    }

    fn setup_action_bar(&mut self, ui: &mut Ui, backend: &mut Backend) {
        ui.horizontal(|ui| {
            let count = self.form.selected.len();
            let ready = count >= 2;
            let games = self.form.estimated_games();

            // Secondary actions left; the primary commit action sits at the
            // bottom-right (desktop convention), with its context beside it.
            self.presets_menu(ui);

            if let Some(err) = self.start_error.clone() {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(format!("⚠ {err}"))
                        .color(theme::danger())
                        .size(13.0),
                );
                if ui
                    .add(egui::Button::new(RichText::new("×").color(theme::text_weak())))
                    .clicked()
                {
                    self.start_error = None;
                }
            }

            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                let start = ui.add_enabled(
                    ready,
                    egui::Button::new(
                        RichText::new("Start Tournament")
                            .color(theme::bg_darkest())
                            .font(theme::semibold(15.0)),
                    )
                    .fill(theme::accent())
                    .min_size(egui::vec2(0.0, 32.0)),
                );
                if start.clicked() {
                    self.try_start(backend);
                }
                ui.add_space(12.0);

                if ready {
                    // Engines that can't fully honour the tournament settings —
                    // visible before Start, click for the full list, never silent.
                    let notes = compatibility_notes(&self.form, &backend.engines);
                    if !notes.is_empty() {
                        let badge = ui.add(
                            egui::Button::new(
                                RichText::new(format!(
                                    "⚠ {} compatibility note{}",
                                    notes.len(),
                                    if notes.len() == 1 { "" } else { "s" }
                                ))
                                .color(theme::warn())
                                .size(13.0),
                            )
                            .fill(theme::tint(theme::warn(), 0.10))
                            .stroke(egui::Stroke::new(1.0, theme::tint(theme::warn(), 0.30)))
                            .corner_radius(egui::CornerRadius::same(6)),
                        );
                        if badge.on_hover_text("Click for details.").clicked() {
                            self.show_compat_notes = true;
                        }
                        ui.add_space(10.0);
                    }
                    let duration = self
                        .form
                        .estimated_duration_secs()
                        .map(|d| format!(" · ~{}", format_duration(d)))
                        .unwrap_or_default();
                    ui.label(
                        RichText::new(format!("{count} engines · {games} games{duration}"))
                            .color(theme::text_weak())
                            .size(13.0),
                    );
                } else {
                    ui.label(
                        RichText::new("Select at least two engines to start.")
                            .color(theme::warn())
                            .size(13.0),
                    );
                }
            });
        });
    }

    /// Dropdown menu for saving/loading/deleting named presets.
    fn presets_menu(&mut self, ui: &mut Ui) {
        // Collect actions from inside the closure so we can act after it returns.
        let mut load_idx: Option<usize> = None;
        let mut delete_name: Option<String> = None;
        let mut saved = false;

        // Close only on outside clicks: the default close-on-any-click made the
        // whole menu vanish as soon as the preset-name field was clicked.
        let (menu_resp, _) = egui::containers::menu::MenuButton::from_button(egui::Button::new(
            RichText::new("Presets    ").size(13.0),
        ))
        .config(
            egui::containers::menu::MenuConfig::new()
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
        )
        .ui(ui, |ui| {
                // Fixed width: the preset list's ScrollArea doesn't shrink
                // horizontally (`auto_shrink[0] = false`), so without a
                // ceiling the popup expands all the way to the screen edge.
                ui.set_width(300.0);

                // ── Save section ──────────────────────────────────────────
                ui.label(
                    RichText::new("Save current config as:")
                        .color(theme::text_weak())
                        .size(11.5),
                );
                ui.horizontal(|ui| {
                    // Let the name field take the row minus the Save button.
                    let save_w = 52.0;
                    ui.add(
                        egui::TextEdit::singleline(&mut self.preset_save_name)
                            .desired_width(ui.available_width() - save_w - ui.spacing().item_spacing.x)
                            .hint_text(if self.form.name.is_empty() {
                                "Preset name"
                            } else {
                                &self.form.name
                            }),
                    );
                    let effective = if self.preset_save_name.trim().is_empty() {
                        self.form.name.trim().to_string()
                    } else {
                        self.preset_save_name.trim().to_string()
                    };
                    if ui
                        .add_enabled(!effective.is_empty(), egui::Button::new("Save"))
                        .clicked()
                    {
                        let data = self.form.to_preset(effective);
                        if let Err(e) = self.preset_manager.save_preset(&data) {
                            tracing::warn!("failed to save preset: {e}");
                        } else {
                            self.presets_cache = self.preset_manager.load_all();
                            self.preset_save_name.clear();
                            saved = true;
                        }
                    }
                });

                // ── Saved presets list ────────────────────────────────────
                ui.separator();

                let cache: Vec<PresetData> = self.presets_cache.clone();
                if cache.is_empty() {
                    ui.label(
                        RichText::new("No presets saved yet.")
                            .color(theme::text_faint())
                            .size(12.0),
                    );
                } else {
                    // Let the list grow with its content up to roughly half the
                    // window before scrolling. `min_scrolled_height` matters:
                    // its 64 px default let the upward-opening popup squeeze
                    // the list to a couple of rows plus a scrollbar even
                    // though there was plenty of screen space to grow into.
                    let max_h = (ui.ctx().content_rect().height() * 0.5).max(200.0);
                    ScrollArea::vertical()
                        .id_salt("preset_list_scroll")
                        .max_height(max_h)
                        .min_scrolled_height(max_h)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for (i, preset) in cache.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(&preset.preset_name).size(12.5),
                                            )
                                            .min_size(egui::vec2(160.0, 0.0))
                                            .frame(false),
                                        )
                                        .on_hover_text("Load this preset")
                                        .clicked()
                                    {
                                        load_idx = Some(i);
                                        ui.close();
                                    }
                                    ui.with_layout(
                                        Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(egui::Button::new(RichText::new("×").color(theme::text_weak())))
                                                .on_hover_text("Delete preset")
                                                .clicked()
                                            {
                                                delete_name =
                                                    Some(preset.preset_name.clone());
                                            }
                                        },
                                    );
                                });
                            }
                        });
                }

                if saved {
                    ui.close();
                }
            },
        );
        widgets::dropdown_arrow(ui, menu_resp.rect);

        // Apply deferred actions.
        if let Some(i) = load_idx
            && let Some(preset) = self.presets_cache.get(i).cloned()
        {
            self.form.apply_preset(&preset);
        }
        if let Some(name) = delete_name {
            if let Err(e) = self.preset_manager.delete_preset(&name) {
                tracing::warn!("failed to delete preset '{name}': {e}");
            }
            self.presets_cache = self.preset_manager.load_all();
        }
    }

    fn try_start(&mut self, backend: &mut Backend) {
        let mut engines = self.form.selected_engines(&backend.engines);
        if engines.len() < 2 {
            self.start_error = Some("Select at least two engines.".to_string());
            return;
        }
        // Inject the global endgame-tablebase paths (managed in the Engines tab)
        // into each engine that declares a matching UCI option.
        apply_global_tablebases(&mut engines, &backend.config);
        let config = self.form.build_config();
        let name = if self.form.name.trim().is_empty() {
            "Tournament"
        } else {
            self.form.name.trim()
        };
        match backend.start_tournament(name, config, engines) {
            Ok(()) => {
                self.start_error = None;
                self.just_started = true;
                // Persist the current form so the next session starts with the
                // same settings.
                let last = self.form.to_preset(String::new());
                self.preset_manager.save_last_used(&last);
            }
            Err(e) => self.start_error = Some(e.to_string()),
        }
    }

    fn engine_selection(&mut self, ui: &mut Ui, backend: &mut Backend) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Engines")
                    .color(theme::text())
                    .font(theme::semibold(15.0)),
            );
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{} selected", self.form.selected.len()))
                        .color(theme::accent())
                        .size(12.5),
                );
            });
        });
        ui.add_space(4.0);

        // Filter + bulk actions. "Select all" respects the active filter so
        // it can be used to select a filtered family of engines.
        ui.horizontal(|ui| {
            widgets::filter_field(
                ui,
                &mut self.engine_filter,
                (ui.available_width() - 34.0).max(80.0),
                "🔍 Filter…",
            );
        });
        let filter = self.engine_filter.to_lowercase();
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            // All three controls share the standard interact height so the
            // row reads as one consistent group.
            let h = ui.spacing().interact_size.y;
            if ui
                .add(
                    egui::Button::new(RichText::new("Select all").color(theme::text_weak()))
                        .min_size(egui::vec2(0.0, h)),
                )
                .on_hover_text("Select every engine matching the filter.")
                .clicked()
            {
                for e in &backend.engines {
                    if (filter.is_empty() || widgets::engine_matches(e, &filter))
                        && !self.form.is_selected(e.id)
                    {
                        self.form.selected.push(e.id);
                    }
                }
            }
            if ui
                .add(
                    egui::Button::new(RichText::new("Clear").color(theme::text_weak()))
                        .min_size(egui::vec2(0.0, h)),
                )
                .clicked()
            {
                self.form.selected.clear();
            }
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                // Sort selector (persisted; independent of the Engines tab).
                if widgets::engine_sort_select(
                    ui,
                    "tournament_engines_sort_select",
                    &mut backend.config.tournament_engines_sort,
                ) {
                    backend.save_config();
                }
            });
        });
        ui.add_space(4.0);
        ui.separator();

        let logos_dir = backend.dirs.logos_dir();
        ScrollArea::vertical()
            .id_salt("tournament_engine_list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if backend.engines.is_empty() {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("♟").color(theme::text_faint()).size(40.0));
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("No engines yet")
                                .color(theme::text_weak())
                                .font(theme::semibold(15.0)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Add engines in the Engines tab to get started.")
                                .color(theme::text_faint())
                                .size(12.5),
                        );
                    });
                    return;
                }

                // Filtered indices, sorted per the persisted order (shared
                // comparator with the Engines tab).
                let mut visible: Vec<usize> = (0..backend.engines.len())
                    .filter(|&i| {
                        filter.is_empty()
                            || widgets::engine_matches(&backend.engines[i], &filter)
                    })
                    .collect();
                widgets::sort_engine_indices(
                    &backend.engines,
                    &mut visible,
                    widgets::EngineSort::from_config(&backend.config.tournament_engines_sort),
                );

                let any_shown = !visible.is_empty();
                for i in visible {
                    let engine = &backend.engines[i];
                    if self.engine_row(ui, engine, &logos_dir) {
                        self.form.toggle(backend.engines[i].id);
                    }
                    ui.add_space(3.0);
                }
                if !any_shown {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("No engines match the filter.")
                                .color(theme::text_weak())
                                .size(12.5),
                        );
                    });
                }
            });
    }

    /// One compact selectable engine row (checkbox + logo + name + Elo).
    /// Returns `true` when the selection should toggle.
    fn engine_row(&mut self, ui: &mut Ui, engine: &EngineConfig, logos_dir: &Path) -> bool {
        let selected = self.form.is_selected(engine.id);
        let name = widgets::engine_base_name(engine);
        let version = engine.meta.version.trim().to_string();
        let path_missing = !engine.path.exists();
        let logo_file = engine.meta.extra.get("logo").cloned();

        let row_h = 36.0;
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_h),
            egui::Sense::click(),
        );
        if !ui.is_rect_visible(rect) {
            return resp.clicked();
        }

        // Geometric containment, not `hovered()`: child widgets (the "…"
        // button) take hover ownership, which would flicker the row state.
        let row_hovered = resp.contains_pointer();
        let fill = if selected {
            theme::tint(theme::accent(), 0.12)
        } else if row_hovered {
            theme::bg_hover()
        } else {
            Color32::TRANSPARENT
        };
        let stroke = if selected {
            egui::Stroke::new(1.0, theme::tint(theme::accent(), 0.4))
        } else {
            egui::Stroke::NONE
        };
        ui.painter().rect(
            rect,
            egui::CornerRadius::same(6),
            fill,
            stroke,
            egui::StrokeKind::Inside,
        );

        let content = rect.shrink2(egui::vec2(8.0, 5.0));
        // Unique id salt per row: child widgets otherwise draw auto-ids from
        // the shared parent Ui, which can transiently collide during relayout.
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("engine_row", engine.id))
                .max_rect(content)
                .layout(Layout::left_to_right(egui::Align::Center)),
        );
        child.spacing_mut().item_spacing.x = 8.0;

        let mut checked = selected;
        let check_changed = widgets::checkbox(&mut child, &mut checked, "").changed();

        let (logo_rect, _) = crate::logo::slot(&mut child, 24.0, egui::Sense::hover());
        let drawn = logo_file.as_ref().is_some_and(|file| {
            crate::logo::draw_fitted(
                &mut child,
                &mut self.logos,
                &logos_dir.join(file),
                logo_rect,
                4,
            )
        });
        if !drawn {
            widgets::draw_avatar_square_in(&child, logo_rect, &name, selected, 4);
        }

        // Elo pinned right; name + version take the rest, truncated.
        let n_overrides = self
            .form
            .overrides
            .get(&engine.id)
            .map_or(0, std::collections::BTreeMap::len);
        let mut open_overrides = false;
        child.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            // Per-tournament UCI options: bordered dots button revealed on
            // hover (fixed slot so rows never shift); the right-click menu
            // stays as the power-user path.
            let (slot, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
            if row_hovered || n_overrides > 0 {
                let dots = widgets::dots_button(ui, slot, ("engine_opts", engine.id), n_overrides > 0)
                    .on_hover_text("Tournament UCI options");
                if dots.clicked() {
                    open_overrides = true;
                }
            }
            if let Some(elo) = engine.meta.elo {
                ui.label(
                    RichText::new(elo.to_string())
                        .color(theme::text_faint())
                        .monospace()
                        .size(12.0),
                );
            }
            if n_overrides > 0 {
                ui.label(RichText::new("●").color(theme::accent()).size(9.0))
                    .on_hover_text(format!(
                        "{n_overrides} UCI option{} overridden for this tournament.",
                        if n_overrides == 1 { "" } else { "s" }
                    ));
            }
            ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.add(
                    egui::Label::new(
                        RichText::new(&name)
                            .color(if selected {
                                theme::accent_bright()
                            } else {
                                theme::text()
                            })
                            .font(theme::semibold(13.0)),
                    )
                    .truncate(),
                );
                if !version.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(version).color(theme::text_weak()).size(11.5),
                        )
                        .truncate(),
                    );
                }
                if path_missing {
                    ui.label(RichText::new("⚠").color(theme::warn()).size(12.0))
                        .on_hover_text("Executable not found at this path.");
                }
            });
        });

        if open_overrides {
            self.override_editor = Some(engine.id);
        }

        // Right-click: per-tournament UCI overrides for this engine.
        resp.context_menu(|ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
            if ui.button("Tournament UCI options…").clicked() {
                self.override_editor = Some(engine.id);
                ui.close();
            }
            if ui
                .add_enabled(
                    n_overrides > 0,
                    egui::Button::new("Clear tournament overrides"),
                )
                .clicked()
            {
                self.form.overrides.remove(&engine.id);
                ui.close();
            }
        });

        if row_hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp.clicked() || check_changed
    }

    fn settings_form(&mut self, ui: &mut Ui, engines: &[EngineConfig]) {
        let f = &mut self.form;

        // Two balanced card columns when there's room, so the options and the
        // Start bar fit without scrolling on a normal window; stacked on
        // narrow panels.
        if ui.available_width() >= 720.0 {
            ui.columns(2, |cols| {
                Self::section_tournament(&mut cols[0], f, engines);
                Self::section_engine_options(&mut cols[0], f);
                Self::section_elo(&mut cols[0], f, engines);
                Self::section_output(&mut cols[0], f);
                Self::section_time_control(&mut cols[1], f);
                Self::section_adjudication(&mut cols[1], f);
                Self::section_openings(&mut cols[1], f);
            });
        } else {
            Self::section_tournament(ui, f, engines);
            Self::section_time_control(ui, f);
            Self::section_engine_options(ui, f);
            Self::section_adjudication(ui, f);
            Self::section_elo(ui, f, engines);
            Self::section_openings(ui, f);
            Self::section_output(ui, f);
        }
        ui.add_space(4.0);
    }

    /// Tournament identity + format + schedule size.
    fn section_tournament(ui: &mut Ui, f: &mut TournamentForm, engines: &[EngineConfig]) {
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
                    widgets::select(ui, "format_kind", f.format_kind.label(), 200.0, |ui| {
                            for kind in [
                                FormatKind::RoundRobin,
                                FormatKind::Gauntlet,
                                FormatKind::Knockout,
                                FormatKind::Sprt,
                            ] {
                                if kind.is_supported() {
                                    ui.selectable_value(&mut f.format_kind, kind, kind.label());
                                } else {
                                    ui.add_enabled(
                                        false,
                                        egui::Button::selectable(false, kind.label()),
                                    )
                                    .on_hover_text(
                                        "Needs a result-dependent (dynamic) scheduler — \
                                         planned for a future step.",
                                    );
                                }
                            }
                        });
                    ui.end_row();

                    if f.format_kind == FormatKind::Gauntlet {
                        ui.label(
                            RichText::new("Gauntlet engine")
                                .color(theme::text_weak())
                                .size(13.0),
                        )
                        .on_hover_text(
                            "Plays every other selected engine; the others don't \
                             play each other.",
                        );
                        let current = f
                            .selected
                            .first()
                            .and_then(|id| engines.iter().find(|e| e.id == *id))
                            .map_or_else(
                                || "select engines first".to_string(),
                                engine_display_name,
                            );
                        widgets::select(ui, "gauntlet_engine", &current, 200.0, |ui| {
                            for id in f.selected.clone() {
                                let Some(engine) = engines.iter().find(|e| e.id == id) else {
                                    continue;
                                };
                                if ui
                                    .selectable_label(
                                        f.selected.first() == Some(&id),
                                        engine_display_name(engine),
                                    )
                                    .clicked()
                                {
                                    // The gauntlet engine is the first seed:
                                    // move it to the front of selection order.
                                    f.selected.retain(|x| *x != id);
                                    f.selected.insert(0, id);
                                    f.gauntlet_seeds = 1;
                                    ui.close();
                                }
                            }
                        });
                        ui.end_row();
                    }

                    field_label(ui, "Cycles");
                    ui.add(DragValue::new(&mut f.cycles).range(1..=10_000).speed(0.2))
                        .on_hover_text(
                            "How many times the full schedule repeats. Large engine-testing \
                             runs routinely use hundreds or thousands.",
                        );
                    ui.end_row();

                    field_label(ui, "Games / pair");
                    widgets::select(
                        ui,
                        "games_per_pair",
                        if f.games_per_pair >= 2 {
                            "2 (both colours)"
                        } else {
                            "1"
                        },
                        140.0,
                        |ui| {
                            if ui.selectable_label(f.games_per_pair == 1, "1").clicked() {
                                f.games_per_pair = 1;
                                ui.close();
                            }
                            if ui
                                .selectable_label(f.games_per_pair >= 2, "2 (both colours)")
                                .clicked()
                            {
                                f.games_per_pair = 2;
                                ui.close();
                            }
                        },
                    );
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

            // Live schedule size: how many games this configuration plays.
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            let games = f.estimated_games();
            if games == 0 {
                ui.label(
                    RichText::new("Select at least two engines to see the schedule.")
                        .color(theme::text_faint())
                        .size(12.5),
                );
            } else {
                // "8 engines · 120 games · ~1h 32m" — the whole-tournament
                // summary lives here with the schedule; the per-game figure is
                // in the Time Control card.
                let n_engines = f.selected.len();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let dot = |ui: &mut Ui| {
                        ui.label(RichText::new("·").color(theme::text_faint()).size(12.5));
                    };
                    ui.label(
                        RichText::new(format!(
                            "{n_engines} engine{}",
                            if n_engines == 1 { "" } else { "s" }
                        ))
                        .color(theme::text())
                        .font(theme::semibold(12.5)),
                    );
                    dot(ui);
                    ui.label(
                        RichText::new(format!(
                            "{games} game{}",
                            if games == 1 { "" } else { "s" }
                        ))
                        .color(theme::text())
                        .font(theme::semibold(12.5)),
                    );
                    if let Some(total) = f.estimated_duration_secs() {
                        let lanes = f.concurrency.max(1);
                        dot(ui);
                        ui.label(
                            RichText::new(format!("~{}", format_duration(total)))
                                .color(theme::text_weak())
                                .size(12.5),
                        )
                        .on_hover_text(format!(
                            "Estimated total length: {games} games, {lanes} in parallel. \
                             Assumes ~{EST_MOVES_PER_SIDE:.0} moves per side per game, \
                             plus a small scheduling overhead.",
                        ));
                    }
                });
            }
        });
    }

    /// Time control type + values, plus the single-game length estimate (it
    /// lives here because it is a function of the time control alone).
    fn section_time_control(ui: &mut Ui, f: &mut TournamentForm) {
        widgets::section_card(ui, "Time Control", None, |ui| {
            ui.horizontal(|ui| {
                field_label(ui, "Type");
                widgets::select(ui, "tc_kind", f.tc_kind.label(), 170.0, |ui| {
                    for kind in [
                        TcKind::PerMove,
                        TcKind::SuddenDeath,
                        TcKind::Increment,
                        TcKind::Nodes,
                        TcKind::Depth,
                    ] {
                        ui.selectable_value(&mut f.tc_kind, kind, kind.label());
                    }
                });
            });
            ui.add_space(6.0);

            match f.tc_kind {
                TcKind::PerMove => {
                    time_value_row(ui, "Per move", &mut f.tc_value, &mut f.tc_unit, "tc_unit");
                    tc_hint(ui, "Fixed thinking time for every move.");
                }
                TcKind::SuddenDeath => {
                    time_value_row(ui, "Base", &mut f.tc_value, &mut f.tc_unit, "tc_unit");
                    tc_hint(ui, "One clock for the whole game; running out loses.");
                }
                TcKind::Increment => {
                    time_value_row(ui, "Base", &mut f.tc_value, &mut f.tc_unit, "tc_unit");
                    time_value_row(
                        ui,
                        "Increment",
                        &mut f.tc_inc_value,
                        &mut f.tc_inc_unit,
                        "tc_inc_unit",
                    );
                    tc_hint(ui, "Game clock plus a bonus after every move.");
                }
                TcKind::Nodes => {
                    ui.horizontal(|ui| {
                        field_label(ui, "Nodes / move");
                        ui.add(
                            DragValue::new(&mut f.tc_nodes)
                                .range(1..=1_000_000_000)
                                .speed(100.0),
                        );
                    });
                    tc_hint(ui, "Fixed node count per move; fully deterministic.");
                }
                TcKind::Depth => {
                    ui.horizontal(|ui| {
                        field_label(ui, "Depth / move");
                        ui.add(DragValue::new(&mut f.tc_depth).range(1..=100).speed(0.1));
                    });
                    tc_hint(ui, "Fixed search depth per move.");
                }
            }

            // Single-game estimate — a function of this time control only.
            // The whole-tournament figure lives in the Tournament card.
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);
            match f.estimated_game_secs() {
                Some(d) => {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(
                            RichText::new("1 game:")
                                .color(theme::text_weak())
                                .size(12.5),
                        );
                        ui.label(
                            RichText::new(format!("~{}", format_duration(d)))
                                .color(theme::text())
                                .font(theme::semibold(12.5)),
                        )
                        .on_hover_text(format!(
                            "Estimated length of one game with this time control. \
                             Assumes ~{EST_MOVES_PER_SIDE:.0} moves per side; \
                             sudden-death games are costed at their full clock budget.",
                        ));
                    });
                }
                None => {
                    ui.label(
                        RichText::new("1 game: depends on engine speed")
                            .color(theme::text_faint())
                            .size(12.5),
                    );
                }
            }
        });
    }

    /// Common UCI options forwarded to every engine.
    fn section_engine_options(ui: &mut Ui, f: &mut TournamentForm) {
        widgets::section_card(
            ui,
            "Engine Options",
            Some("Forwarded to every engine as UCI options."),
            |ui| {
                egui::Grid::new("tc_common_grid")
                    .num_columns(3)
                    .spacing([8.0, 6.0])
                    .show(ui, |ui| {
                        widgets::checkbox(ui, &mut f.threads_on, "");
                        field_label(ui, "Threads");
                        ui.add_enabled(
                            f.threads_on,
                            DragValue::new(&mut f.threads).range(1..=1024).speed(0.1),
                        );
                        ui.end_row();

                        widgets::checkbox(ui, &mut f.hash_on, "");
                        field_label(ui, "Hash (MB)");
                        ui.add_enabled(
                            f.hash_on,
                            DragValue::new(&mut f.hash_mb)
                                .range(1..=1_048_576)
                                .speed(1.0),
                        );
                        ui.end_row();

                    });

                ui.add_space(4.0);
                widgets::checkbox(ui, &mut f.ponder, "Ponder").on_hover_text(
                    "Let engines think on the opponent's time. Off keeps fast games fair.",
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Tablebases (paths, caches, Syzygy 50-move rule & probe limit, \
                         Gaviota compression) are configured globally in the Engines tab.",
                    )
                    .color(theme::text_faint())
                    .size(11.5),
                );
            },
        );
    }

    /// Draw/resign/max-moves adjudication rules.
    fn section_adjudication(ui: &mut Ui, f: &mut TournamentForm) {
        widgets::section_card(
            ui,
            "Adjudication",
            Some("Natural endings (mate, stalemate, 50-move, repetition) are always detected."),
            |ui| {
                ui.horizontal(|ui| {
                    widgets::checkbox(ui, &mut f.max_moves_on, "Max moves");
                    ui.add_enabled(
                        f.max_moves_on,
                        DragValue::new(&mut f.max_moves).range(1..=2000).speed(1.0),
                    )
                    .on_hover_text("Declare a draw after this many full moves.");
                });

                widgets::checkbox(ui, &mut f.draw_on, "Draw adjudication");
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

                widgets::checkbox(ui, &mut f.resign_on, "Resign (win/loss) adjudication");
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
    }

    /// Elo update policy for the library ratings.
    fn section_elo(ui: &mut Ui, f: &mut TournamentForm, engines: &[EngineConfig]) {
        widgets::section_card(
            ui,
            "Elo",
            Some("Update engine library ratings from this tournament's results."),
            |ui| {
                egui::Grid::new("tc_elo_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        field_label(ui, "Update ratings");
                        widgets::select(
                            ui,
                            "elo_writeback",
                            f.elo_writeback.label(),
                            200.0,
                            |ui| {
                                for (kind, hint) in [
                                    (
                                        WritebackKind::Never,
                                        "Library ratings stay untouched (default).",
                                    ),
                                    (
                                        WritebackKind::All,
                                        "All participants are re-rated jointly from every \
                                         result (maximum likelihood, anchored to their \
                                         current average) when the tournament finishes.",
                                    ),
                                    (
                                        WritebackKind::Estimate,
                                        "Only one engine is rated, from its results \
                                         against the others' fixed ratings. Ideal for \
                                         gauntleting a new engine.",
                                    ),
                                ] {
                                    ui.selectable_value(&mut f.elo_writeback, kind, kind.label())
                                        .on_hover_text(hint);
                                }
                            },
                        );
                        ui.end_row();

                        match f.elo_writeback {
                            WritebackKind::Never | WritebackKind::All => {}
                            WritebackKind::Estimate => {
                                ui.label(
                                    RichText::new("Engine")
                                        .color(theme::text_weak())
                                        .size(13.0),
                                )
                                .on_hover_text(
                                    "Gets its performance rating against the other \
                                     engines' library ratings, which stay unchanged.",
                                );
                                let current = f
                                    .estimate_engine()
                                    .and_then(|id| engines.iter().find(|e| e.id == id))
                                    .map_or_else(
                                        || "select engines first".to_string(),
                                        engine_display_name,
                                    );
                                widgets::select(ui, "estimate_target", &current, 200.0, |ui| {
                                    let picked = f.estimate_engine();
                                    for id in f.selected.clone() {
                                        let Some(engine) =
                                            engines.iter().find(|e| e.id == id)
                                        else {
                                            continue;
                                        };
                                        if ui
                                            .selectable_label(
                                                picked == Some(id),
                                                engine_display_name(engine),
                                            )
                                            .clicked()
                                        {
                                            f.estimate_target = Some(id);
                                            ui.close();
                                        }
                                    }
                                });
                                ui.end_row();
                            }
                        }
                    });
            },
        );
    }

    /// Opening-book configuration + preview.
    fn section_openings(ui: &mut Ui, f: &mut TournamentForm) {
        widgets::section_card(ui, "Openings", None, |ui| {
            if widgets::checkbox(ui, &mut f.openings_on, "Use an opening book")
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
                        .add(egui::Button::new(RichText::new("Browse…").color(theme::text_weak())))
                        .clicked()
                        && let Some(path) = crate::dialog::file_dialog()
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
                            let mut changed = widgets::choice_chip(
                                ui,
                                &mut f.openings_format,
                                OpeningFormat::Epd,
                                "EPD",
                            )
                            .changed();
                            changed |= widgets::choice_chip(
                                ui,
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
                            let mut changed = widgets::choice_chip(
                                ui,
                                &mut f.openings_order,
                                OpeningOrder::Sequential,
                                "Sequential",
                            )
                            .changed();
                            changed |= widgets::choice_chip(
                                ui,
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
                                widgets::checkbox(ui, &mut f.openings_count_on, "").changed();
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
                            RichText::new(format!("{count} openings loaded"))
                                .color(theme::success())
                                .size(12.0),
                        );
                        if let Some(label) = sample {
                            ui.label(
                                RichText::new(format!("e.g. {}", truncate(label, 60)))
                                    .color(theme::text_weak())
                                    .size(11.5),
                            );
                        }
                    }
                    Some(Err(e)) => {
                        ui.label(
                            RichText::new(format!("⚠ {e}"))
                                .color(theme::danger())
                                .size(12.0),
                        );
                    }
                    None => {
                        ui.label(
                            RichText::new("Choose a file to preview its openings.")
                                .color(theme::text_weak())
                                .size(11.5),
                        );
                    }
                }
            }
        });
    }

    /// PGN output configuration.
    fn section_output(ui: &mut Ui, f: &mut TournamentForm) {
        widgets::section_card(ui, "Output", None, |ui| {
            ui.horizontal(|ui| {
                field_label(ui, "PGN file");
                ui.add(
                    egui::TextEdit::singleline(&mut f.pgn_path)
                        .desired_width(240.0)
                        .hint_text("optional"),
                )
                .on_hover_text("Finished games are appended to this PGN file as they end.");
                if ui
                    .add(egui::Button::new(RichText::new("Browse…").color(theme::text_weak())))
                    .clicked()
                    && let Some(path) = crate::dialog::file_dialog()
                        .set_title("Choose PGN output file")
                        .add_filter("PGN", &["pgn"])
                        .save_file()
                {
                    f.pgn_path = path.to_string_lossy().to_string();
                }
            });
        });
    }
}

// ── Small helpers ───────────────────────────────────────────────────────────────

fn field_label(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::text_weak()).size(13.0));
}

/// The one-line explanation under the time-control fields.
fn tc_hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::text_weak()).size(11.5));
}

/// "Name version" display string for an engine (version omitted when empty).
fn engine_display_name(e: &EngineConfig) -> String {
    let name = widgets::engine_base_name(e);
    let version = e.meta.version.trim();
    if version.is_empty() {
        name
    } else {
        format!("{name} {version}")
    }
}

/// Human-readable notes about selected engines that can't fully honour the
/// tournament settings — no silent failures. An engine whose per-tournament
/// override covers the setting is considered handled and not reported.
fn compatibility_notes(form: &TournamentForm, engines: &[EngineConfig]) -> Vec<String> {
    use colosseum_core::{is_hash_option, is_thread_option};
    let mut notes = Vec::new();

    // CPU oversubscription: more busy engine threads than logical cores
    // starves engines of CPU — games are wall-clock timed, so that shows up
    // as time forfeits, and fragile engines misbehave under starvation.
    let cores = std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
    let threads_per = if form.threads_on {
        form.threads.max(1) as usize
    } else {
        1
    };
    let demand = form.concurrency.max(1) * threads_per;
    if demand > cores {
        notes.push(format!(
            "{} parallel games × {} thread{} = {demand} busy engine threads, but this \
             machine has {cores} logical cores. Expect time losses and unstable engines \
             from CPU starvation — lower Parallel games or Threads.",
            form.concurrency.max(1),
            threads_per,
            if threads_per == 1 { "" } else { "s" },
        ));
    }
    for e in engines.iter().filter(|e| form.selected.contains(&e.id)) {
        let name = {
            let n = widgets::engine_base_name(e);
            let v = e.meta.version.trim();
            if v.is_empty() {
                n
            } else {
                format!("{n} {v}")
            }
        };
        if !e.path.exists() {
            notes.push(format!(
                "{name}: executable not found — its games will fail"
            ));
        }
        let overridden = |pred: fn(&str) -> bool| {
            form.overrides
                .get(&e.id)
                .is_some_and(|m| m.keys().any(|k| pred(k)))
        };
        if form.threads_on && form.threads > 1 && !overridden(is_thread_option) {
            let thread_opts: Vec<&UciOption> = e
                .detected_options
                .iter()
                .filter(|o| is_thread_option(o.name()))
                .collect();
            if thread_opts.is_empty() {
                notes.push(format!(
                    "{name}: no thread option — runs single-threaded \
                     (Threads {} requested)",
                    form.threads
                ));
            } else {
                for o in thread_opts {
                    if let UciOption::Spin { max, .. } = o
                        && i64::from(form.threads) > *max
                    {
                        notes.push(format!(
                            "{name}: {} will be capped at {max} (Threads {} requested)",
                            o.name(),
                            form.threads
                        ));
                    }
                }
            }
        }
        if form.hash_on && !overridden(is_hash_option) {
            let hash_opts: Vec<&UciOption> = e
                .detected_options
                .iter()
                .filter(|o| is_hash_option(o.name()))
                .collect();
            if hash_opts.is_empty() {
                notes.push(format!(
                    "{name}: no Hash option — Hash {} MB is not applied",
                    form.hash_mb
                ));
            } else {
                for o in hash_opts {
                    if let UciOption::Spin { min, max, .. } = o {
                        if i64::from(form.hash_mb) > *max {
                            notes.push(format!(
                                "{name}: {} will be capped at {max} MB \
                                 (Hash {} requested)",
                                o.name(),
                                form.hash_mb
                            ));
                        } else if i64::from(form.hash_mb) < *min {
                            notes.push(format!(
                                "{name}: {} will be raised to {min} MB \
                                 (Hash {} requested)",
                                o.name(),
                                form.hash_mb
                            ));
                        }
                    }
                }
            }
        }
    }
    notes
}

/// The engine's detected options with its saved (library) values and the
/// tournament-wide Threads/Hash/Ponder substituted as the displayed defaults
/// — i.e. what actually applies when no tournament override is set.
fn effective_options(
    engine: &EngineConfig,
    common_threads: Option<u32>,
    common_hash: Option<u32>,
    common_ponder: bool,
) -> Vec<UciOption> {
    use colosseum_core::{is_hash_option, is_thread_option};
    engine
        .detected_options
        .iter()
        .cloned()
        .map(|mut opt| {
            // Library-saved value first…
            if let Some(saved) = engine.options.get(opt.name()) {
                match (&mut opt, saved) {
                    (UciOption::Check { default, .. }, UciOptionValue::Check(v)) => *default = *v,
                    (UciOption::Spin { default, .. }, UciOptionValue::Spin(v)) => *default = *v,
                    (UciOption::Combo { default, .. }, UciOptionValue::Combo(v)) => {
                        *default = v.clone();
                    }
                    (UciOption::Str { default, .. }, UciOptionValue::Str(v)) => {
                        *default = v.clone();
                    }
                    _ => {}
                }
            }
            // …then the tournament-wide values on top (matching what the
            // scheduler forwards, including the range clamp).
            match &mut opt {
                UciOption::Spin {
                    name,
                    default,
                    min,
                    max,
                } => {
                    if let Some(t) = common_threads
                        && is_thread_option(name)
                    {
                        *default = i64::from(t).clamp(*min, *max);
                    }
                    if let Some(h) = common_hash
                        && is_hash_option(name)
                    {
                        *default = i64::from(h).clamp(*min, *max);
                    }
                }
                UciOption::Check { name, default } if name == "Ponder" => {
                    *default = common_ponder;
                }
                _ => {}
            }
            opt
        })
        .collect()
}

/// A labelled value + unit row for entering a duration (value DragValue, unit
/// dropdown, and a resolved "= N ms" hint).
fn time_value_row(
    ui: &mut Ui,
    label: &str,
    value: &mut f64,
    unit: &mut TimeUnit,
    id_salt: &str,
) {
    ui.horizontal(|ui| {
        field_label(ui, label);
        ui.add(DragValue::new(value).range(0.0..=600_000.0).speed(1.0));
        widgets::select(ui, id_salt, unit.label(), 64.0, |ui| {
            ui.selectable_value(unit, TimeUnit::Milliseconds, "ms");
            ui.selectable_value(unit, TimeUnit::Seconds, "s");
            ui.selectable_value(unit, TimeUnit::Minutes, "min");
        });
    });
}

/// Forward the global endgame-tablebase directories to every engine that
/// declares a matching path option (`SyzygyPath` / `GaviotaTbPath` /
/// `NalimovPath`, matched loosely by name). Empty paths are skipped. When a
/// path is set, the matching probe-cache size (`NalimovCache` /
/// `GaviotaTbCache`) is forwarded too, clamped to the option's declared range.
fn apply_global_tablebases(engines: &mut [EngineConfig], config: &AppConfig) {
    let path_set =
        |p: &Option<String>| p.as_deref().map(str::trim).is_some_and(|s| !s.is_empty());

    for engine in engines {
        let inserts: Vec<(String, UciOptionValue)> = engine
            .detected_options
            .iter()
            .filter_map(|opt| {
                let n = opt.name().to_ascii_lowercase();
                if n.contains("path") {
                    let path = if n.contains("syzygy") {
                        config.syzygy_path.as_ref()
                    } else if n.contains("gaviota") {
                        config.gaviota_path.as_ref()
                    } else if n.contains("nalimov") {
                        config.nalimov_path.as_ref()
                    } else {
                        None
                    };
                    return path
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|s| (opt.name().to_string(), UciOptionValue::Str(s.to_string())));
                }
                if n.contains("cache") {
                    let mb = if n.contains("gaviota") && path_set(&config.gaviota_path) {
                        config.gaviota_cache_mb
                    } else if n.contains("nalimov") && path_set(&config.nalimov_path) {
                        config.nalimov_cache_mb
                    } else {
                        return None;
                    };
                    let value = match opt {
                        UciOption::Spin { min, max, .. } => i64::from(mb).clamp(*min, *max),
                        _ => i64::from(mb),
                    };
                    return Some((opt.name().to_string(), UciOptionValue::Spin(value)));
                }
                // Syzygy policy options, applied with the Syzygy path.
                if n.contains("syzygy") && path_set(&config.syzygy_path) {
                    if n.contains("50move") {
                        return Some((
                            opt.name().to_string(),
                            UciOptionValue::Check(config.syzygy_50_move_rule),
                        ));
                    }
                    if n.contains("probelimit") {
                        let value = match opt {
                            UciOption::Spin { min, max, .. } => {
                                i64::from(config.syzygy_probe_limit).clamp(*min, *max)
                            }
                            _ => i64::from(config.syzygy_probe_limit),
                        };
                        return Some((opt.name().to_string(), UciOptionValue::Spin(value)));
                    }
                }
                // Gaviota compression scheme, applied with the Gaviota path —
                // only when the engine's combo actually offers the scheme.
                if n.contains("gaviota")
                    && n.contains("compression")
                    && path_set(&config.gaviota_path)
                    && let UciOption::Combo { vars, .. } = opt
                    && vars.contains(&config.gaviota_compression)
                {
                    return Some((
                        opt.name().to_string(),
                        UciOptionValue::Combo(config.gaviota_compression.clone()),
                    ));
                }
                None
            })
            .collect();
        for (name, value) in inserts {
            engine.options.insert(name, value);
        }
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
#[cfg(test)]
mod tests {
    use super::*;

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
    fn duration_formatting() {
        // Sub-second (fast sudden-death games) shows ms, not a rounded "0s".
        assert_eq!(format_duration(0.4), "400ms");
        assert_eq!(format_duration(0.05), "50ms");
        assert_eq!(format_duration(0.999), "999ms");
        assert_eq!(format_duration(1.0), "1s");
        assert_eq!(format_duration(45.0), "45s");
        assert_eq!(format_duration(60.0), "1m");
        assert_eq!(format_duration(150.0), "2m 30s");
        assert_eq!(format_duration(3_900.0), "1h 05m");
        assert_eq!(format_duration(200_000.0), "2d 7h");
    }

    #[test]
    fn games_per_pair_clamped_to_two() {
        let form = TournamentForm {
            games_per_pair: 20, // e.g. from an old preset
            ..TournamentForm::default()
        };
        assert_eq!(form.build_config().games_per_pair, 2);
    }

    #[test]
    fn estimated_duration_scales_with_lanes() {
        let mut form = TournamentForm {
            selected: vec![EngineId::new(), EngineId::new()],
            tc_kind: TcKind::PerMove,
            tc_value: 1000.0,
            tc_unit: TimeUnit::Milliseconds,
            ..TournamentForm::default()
        };
        // 1 pair * 2 games; 1s/move * 60 moves * 2 sides = 120 s per game.
        let d1 = form.estimated_duration_secs().unwrap();
        form.concurrency = 2;
        let d2 = form.estimated_duration_secs().unwrap();
        assert!((d1 / d2 - 2.0).abs() < 1e-9);
        // Nodes-based control has no wall-clock estimate.
        form.tc_kind = TcKind::Nodes;
        assert!(form.estimated_duration_secs().is_none());
    }
}
