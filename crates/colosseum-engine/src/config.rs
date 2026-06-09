//! Application configuration and engine-library file I/O.
//!
//! [`AppDirs`] resolves all storage paths once at startup, supporting both
//! OS-standard locations and `--portable` mode (everything next to the
//! executable).  [`AppConfig`] holds user preferences persisted as
//! `config.toml`.  [`EngineLibrary`] loads and saves the engine collection as
//! `engines.json`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use colosseum_core::{
    EngineConfig,
    branding::{APP_DIR_NAME, ORGANIZATION, QUALIFIER},
};
use directories::ProjectDirs;

use crate::error::EngineError;

type Result<T> = std::result::Result<T, EngineError>;

// ---------------------------------------------------------------------------
// AppDirs
// ---------------------------------------------------------------------------

/// Resolved storage directories for the application.
///
/// Create once at startup via [`AppDirs::new`] and pass the instance around to
/// avoid repeated `ProjectDirs` lookups and to centralize the portable-mode
/// decision.
///
/// # Platform paths (non-portable)
/// | OS      | `config_dir`                              | `data_dir`                                |
/// |---------|-------------------------------------------|-------------------------------------------|
/// | Windows | `%APPDATA%\Colosseum`                     | `%APPDATA%\Colosseum`                     |
/// | Linux   | `~/.config/colosseum`                     | `~/.local/share/colosseum`                |
/// | macOS   | `~/Library/Application Support/Colosseum` | `~/Library/Application Support/Colosseum` |
#[derive(Debug, Clone)]
pub struct AppDirs {
    /// Directory for `config.toml` and `engines.json`.
    pub config_dir: PathBuf,
    /// Directory for `colosseum.sqlite` and log files.
    pub data_dir: PathBuf,
}

impl AppDirs {
    /// Resolve application directories.
    ///
    /// When `portable` is `true` (e.g. the app was launched with `--portable`),
    /// all data is stored in the directory that contains the executable, making
    /// the installation self-contained and movable.
    ///
    /// Returns `None` if `portable` is `true` and `current_exe()` fails, or if
    /// `ProjectDirs` cannot determine the user's home directory.
    #[must_use]
    pub fn new(portable: bool) -> Option<Self> {
        if portable {
            let base = std::env::current_exe().ok()?.parent()?.to_path_buf();
            Some(Self {
                config_dir: base.clone(),
                data_dir: base,
            })
        } else {
            let pd = ProjectDirs::from(QUALIFIER, ORGANIZATION, APP_DIR_NAME)?;
            Some(Self {
                config_dir: pd.config_dir().to_path_buf(),
                data_dir: pd.data_dir().to_path_buf(),
            })
        }
    }

    /// Path to the SQLite tournament database.
    #[must_use]
    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("colosseum.sqlite")
    }

    /// Path to the TOML preferences file.
    #[must_use]
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// Path to the JSON engine library file.
    #[must_use]
    pub fn engines_file(&self) -> PathBuf {
        self.config_dir.join("engines.json")
    }

    /// Create both directories if they do not already exist.
    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.data_dir)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// AppConfig
// ---------------------------------------------------------------------------

/// User preferences persisted across sessions as `config.toml`.
///
/// All fields carry sensible defaults so the file may be absent on first run
/// or partially written when future versions add new fields.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Last window width in logical pixels.
    pub window_width: f32,
    /// Last window height in logical pixels.
    pub window_height: f32,
    /// Whether the window was maximized when it was last closed.
    pub window_maximized: bool,
    /// Last directory the user browsed for engine executables.
    pub last_engine_dir: Option<PathBuf>,
    /// Last directory the user browsed for PGN output files.
    pub last_pgn_dir: Option<PathBuf>,
    /// Last directory the user browsed for opening book files (EPD/PGN — Step 10).
    pub last_openings_dir: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            window_width: 1100.0,
            window_height: 720.0,
            window_maximized: false,
            last_engine_dir: None,
            last_pgn_dir: None,
            last_openings_dir: None,
        }
    }
}

impl AppConfig {
    /// Load from `path`.
    ///
    /// Returns [`Default`] when the file does not exist (first run).
    /// Propagates any other I/O or TOML parse error.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).map_err(EngineError::TomlDe),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(EngineError::Io(e)),
        }
    }

    /// Serialize and write to `path`, creating any missing parent directories.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self).map_err(EngineError::TomlSer)?;
        std::fs::write(path, text)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// EngineLibrary
// ---------------------------------------------------------------------------

/// Operations on the engine collection file (`engines.json`).
///
/// The collection is a flat JSON array of [`EngineConfig`] values.  Saves are
/// written atomically (write-to-`.tmp` then rename) to prevent corruption on
/// unexpected exit.
pub struct EngineLibrary;

impl EngineLibrary {
    /// Load the engine collection from `path`.
    ///
    /// Returns an empty `Vec` when the file does not exist (first run).
    pub fn load(path: &Path) -> Result<Vec<EngineConfig>> {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).map_err(EngineError::Serde),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(EngineError::Io(e)),
        }
    }

    /// Persist the engine collection to `path`.
    ///
    /// Writes to a sibling `.json.tmp` file first, then renames atomically.
    /// Creates any missing parent directories.
    pub fn save(engines: &[EngineConfig], path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(engines).map_err(EngineError::Serde)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &text)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_config_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");

        let cfg = AppConfig {
            window_width: 1280.0,
            last_engine_dir: Some(PathBuf::from("/engines")),
            ..AppConfig::default()
        };
        cfg.save(&path).unwrap();

        let loaded = AppConfig::load(&path).unwrap();
        assert!((loaded.window_width - 1280.0).abs() < f32::EPSILON);
        assert_eq!(loaded.last_engine_dir, Some(PathBuf::from("/engines")));
        assert!(!loaded.window_maximized);
    }

    #[test]
    fn app_config_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.toml");
        let cfg = AppConfig::load(&path).unwrap();
        assert!((cfg.window_width - 1100.0).abs() < f32::EPSILON);
        assert!((cfg.window_height - 720.0).abs() < f32::EPSILON);
    }

    #[test]
    fn engine_library_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("engines.json");

        let engines = vec![
            colosseum_core::EngineConfig::new("/engines/sf".into()),
            colosseum_core::EngineConfig::new("/engines/lc0".into()),
        ];
        EngineLibrary::save(&engines, &path).unwrap();

        let loaded = EngineLibrary::load(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].path, engines[0].path);
        assert_eq!(loaded[1].path, engines[1].path);
    }

    #[test]
    fn engine_library_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("no-engines.json");
        let engines = EngineLibrary::load(&path).unwrap();
        assert!(engines.is_empty());
    }

    #[test]
    fn app_dirs_standard_resolves() {
        // Just check that non-portable mode returns Some on any CI machine.
        if let Some(dirs) = AppDirs::new(false) {
            assert!(dirs.database_path().ends_with("colosseum.sqlite"));
            assert!(dirs.config_file().ends_with("config.toml"));
            assert!(dirs.engines_file().ends_with("engines.json"));
        }
        // None is acceptable on minimal container environments that lack a HOME.
    }
}
