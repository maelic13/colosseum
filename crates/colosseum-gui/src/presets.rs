// SPDX-License-Identifier: GPL-3.0-or-later
//! Config-preset persistence: save/load named tournament configs + last-used
//! config memory.
//!
//! Presets are stored as JSON files inside `<config_dir>/presets/`.  The
//! last-used config is a single `<config_dir>/last_used_config.json`.
//!
//! [`PresetManager`] handles all file I/O and is completely decoupled from the
//! GUI widgets — the tournament tab handles the conversion between GUI form
//! state and [`PresetData`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use colosseum_core::{OpeningFormat, OpeningOrder, TimeUnit};

// ── Mirror enums ────────────────────────────────────────────────────────────────

/// Serialisable mirror of the GUI's `FormatKind`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PresetFormatKind {
    #[default]
    RoundRobin,
    Gauntlet,
    Knockout,
    Sprt,
}

/// Serialisable mirror of the GUI's `TcKind`.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum PresetTcKind {
    #[default]
    PerMove,
    SuddenDeath,
    Increment,
    Nodes,
    Depth,
}

// ── PresetData ──────────────────────────────────────────────────────────────────

/// A serialisable snapshot of the tournament setup form (excluding engine
/// selection, which is session-specific, and the openings preview, which is
/// derived).
///
/// `#[serde(default)]` + manual `Default` means that older preset files that
/// lack newer fields will deserialise cleanly with sensible fallbacks.
#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct PresetData {
    pub preset_name: String,
    pub tournament_name: String,

    // Format
    pub format_kind: PresetFormatKind,
    pub cycles: u32,
    pub games_per_pair: u32,
    pub gauntlet_seeds: u32,

    // Time control
    pub tc_kind: PresetTcKind,
    pub tc_value: f64,
    pub tc_unit: TimeUnit,
    pub tc_inc_value: f64,
    pub tc_inc_unit: TimeUnit,
    pub tc_nodes: u64,
    pub tc_depth: u32,

    // Concurrency
    pub concurrency: usize,

    // Common engine options
    pub threads_on: bool,
    pub threads: u32,
    pub hash_on: bool,
    pub hash_mb: u32,
    pub syzygy_path: String,
    pub syzygy50_on: bool,
    pub syzygy50: bool,
    pub ponder: bool,
    #[serde(default = "default_true")]
    pub tablebases: bool,

    // Adjudication
    pub max_moves_on: bool,
    pub max_moves: u32,
    pub draw_on: bool,
    pub draw_min_ply: u32,
    pub draw_move_count: u32,
    pub draw_score_cp: i32,
    pub resign_on: bool,
    pub resign_move_count: u32,
    pub resign_score_cp: i32,

    // Elo
    /// Library-rating writeback mode: "never" (default), "all", "estimate".
    /// The estimate target engine is session state and is not persisted.
    #[serde(default)]
    pub elo_writeback: String,

    // Openings
    pub openings_on: bool,
    pub openings_path: String,
    pub openings_format: OpeningFormat,
    pub openings_order: OpeningOrder,
    pub openings_plies: u32,
    pub openings_count_on: bool,
    pub openings_count: u32,
    pub openings_seed: u64,

    // Output
    pub pgn_path: String,
}

fn default_true() -> bool {
    true
}

impl Default for PresetData {
    fn default() -> Self {
        Self {
            preset_name: String::new(),
            tournament_name: "Round Robin".to_string(),
            format_kind: PresetFormatKind::RoundRobin,
            cycles: 1,
            games_per_pair: 2,
            gauntlet_seeds: 1,
            tc_kind: PresetTcKind::PerMove,
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
            tablebases: true,
            max_moves_on: false,
            max_moves: 300,
            draw_on: false,
            draw_min_ply: 40,
            draw_move_count: 8,
            draw_score_cp: 8,
            resign_on: false,
            resign_move_count: 4,
            resign_score_cp: 800,
            elo_writeback: String::new(),
            openings_on: false,
            openings_path: String::new(),
            openings_format: OpeningFormat::Epd,
            openings_order: OpeningOrder::Sequential,
            openings_plies: 8,
            openings_count_on: false,
            openings_count: 100,
            openings_seed: 0,
            pgn_path: String::new(),
        }
    }
}

// ── PresetManager ───────────────────────────────────────────────────────────────

/// Manages preset files on disk.
pub struct PresetManager {
    presets_dir: PathBuf,
    last_used_path: PathBuf,
}

impl PresetManager {
    /// Create a manager rooted at `config_dir`.
    pub fn new(config_dir: &Path) -> Self {
        Self {
            presets_dir: config_dir.join("presets"),
            last_used_path: config_dir.join("last_used_config.json"),
        }
    }

    /// Save `data` as a named preset.  The filename is a sanitised slug of
    /// `data.preset_name`; an existing file with the same slug is overwritten.
    pub fn save_preset(&self, data: &PresetData) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.presets_dir)?;
        let path = self
            .presets_dir
            .join(format!("{}.json", slug(&data.preset_name)));
        let json = serde_json::to_string_pretty(data)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load all saved presets, sorted by name.  Files that fail to parse are
    /// silently skipped.
    pub fn load_all(&self) -> Vec<PresetData> {
        let Ok(entries) = std::fs::read_dir(&self.presets_dir) else {
            return Vec::new();
        };
        let mut presets: Vec<PresetData> = entries
            .filter_map(|e| {
                let entry = e.ok()?;
                let path = entry.path();
                if path.extension()?.to_str()? != "json" {
                    return None;
                }
                let text = std::fs::read_to_string(path).ok()?;
                serde_json::from_str(&text).ok()
            })
            .collect();
        presets.sort_by(|a, b| a.preset_name.cmp(&b.preset_name));
        presets
    }

    /// Delete the preset whose slug matches `name`.  Silently succeeds if
    /// the file does not exist.
    pub fn delete_preset(&self, name: &str) -> anyhow::Result<()> {
        let path = self.presets_dir.join(format!("{}.json", slug(name)));
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Overwrite the last-used config file.  Failures are logged but not
    /// propagated — a missing last-used file is harmless.
    pub fn save_last_used(&self, data: &PresetData) {
        match serde_json::to_string_pretty(data) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.last_used_path, json) {
                    tracing::warn!("could not save last-used config: {e}");
                }
            }
            Err(e) => tracing::warn!("could not serialise last-used config: {e}"),
        }
    }

    /// Load the last-used config, if any.
    pub fn load_last_used(&self) -> Option<PresetData> {
        let text = std::fs::read_to_string(&self.last_used_path).ok()?;
        match serde_json::from_str(&text) {
            Ok(data) => Some(data),
            Err(e) => {
                tracing::warn!("could not parse last-used config: {e}");
                None
            }
        }
    }
}

/// Convert a preset name into a safe filename slug.
fn slug(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let s = cleaned.trim().replace(' ', "_").to_lowercase();
    if s.is_empty() {
        "unnamed".to_string()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_data_round_trips() {
        let data = PresetData {
            preset_name: "Fast nodes".to_string(),
            tc_kind: PresetTcKind::Nodes,
            tc_nodes: 50_000,
            games_per_pair: 4,
            ..Default::default()
        };

        let json = serde_json::to_string_pretty(&data).unwrap();
        let loaded: PresetData = serde_json::from_str(&json).unwrap();
        assert_eq!(loaded.preset_name, "Fast nodes");
        assert!(matches!(loaded.tc_kind, PresetTcKind::Nodes));
        assert_eq!(loaded.tc_nodes, 50_000);
        assert_eq!(loaded.games_per_pair, 4);
    }

    #[test]
    fn preset_data_missing_fields_use_defaults() {
        let partial = r#"{"preset_name":"Minimal","games_per_pair":6}"#;
        let loaded: PresetData = serde_json::from_str(partial).unwrap();
        assert_eq!(loaded.preset_name, "Minimal");
        assert_eq!(loaded.games_per_pair, 6);
        assert_eq!(loaded.concurrency, 1); // from Default
        assert!(matches!(loaded.tc_kind, PresetTcKind::PerMove));
    }

    #[test]
    fn slug_sanitises_names() {
        assert_eq!(slug("Fast + Increment 1+0.1"), "fast___increment_1_0_1");
        assert_eq!(slug("  "), "unnamed");
        assert_eq!(slug(""), "unnamed");
        assert_eq!(slug("My Preset"), "my_preset");
    }

    #[test]
    fn preset_manager_save_load_delete() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = PresetManager::new(dir.path());

        assert!(mgr.load_all().is_empty());

        let p = PresetData {
            preset_name: "Alpha".to_string(),
            games_per_pair: 10,
            ..Default::default()
        };
        mgr.save_preset(&p).unwrap();

        let p2 = PresetData {
            preset_name: "Beta".to_string(),
            concurrency: 4,
            ..Default::default()
        };
        mgr.save_preset(&p2).unwrap();

        let all = mgr.load_all();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].preset_name, "Alpha");
        assert_eq!(all[0].games_per_pair, 10);
        assert_eq!(all[1].preset_name, "Beta");
        assert_eq!(all[1].concurrency, 4);

        mgr.delete_preset("Alpha").unwrap();
        let remaining = mgr.load_all();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].preset_name, "Beta");
    }

    #[test]
    fn last_used_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let mgr = PresetManager::new(dir.path());

        assert!(mgr.load_last_used().is_none());

        let p = PresetData {
            tournament_name: "My run".to_string(),
            concurrency: 3,
            ..Default::default()
        };
        mgr.save_last_used(&p);

        let loaded = mgr.load_last_used().unwrap();
        assert_eq!(loaded.tournament_name, "My run");
        assert_eq!(loaded.concurrency, 3);
    }
}
