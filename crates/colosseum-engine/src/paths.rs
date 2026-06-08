//! Storage locations, derived from a single [`ProjectDirs`] decision so the app can
//! be renamed (and a portable mode added) in one place.

use std::path::PathBuf;

use colosseum_core::branding::{APP_DIR_NAME, ORGANIZATION, QUALIFIER};
use directories::ProjectDirs;

/// The platform-appropriate application directories, when they can be determined.
///
/// `ProjectDirs::from("", "", "Colosseum")` yields clean paths:
/// - Windows: `%APPDATA%\Colosseum`
/// - Linux:   `~/.config/colosseum`
/// - macOS:   `~/Library/Application Support/Colosseum`
#[must_use]
pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APP_DIR_NAME)
}

/// Directory for user configuration (`config.toml`, `engines.json`).
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    project_dirs().map(|d| d.config_dir().to_path_buf())
}

/// Directory for application data (SQLite database, logs).
#[must_use]
pub fn data_dir() -> Option<PathBuf> {
    project_dirs().map(|d| d.data_dir().to_path_buf())
}

/// Path to the SQLite database holding tournaments and games.
#[must_use]
pub fn database_path() -> Option<PathBuf> {
    data_dir().map(|d| d.join("colosseum.sqlite"))
}

/// Path to the app config file.
#[must_use]
pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("config.toml"))
}

/// Path to the engine library file.
#[must_use]
pub fn engines_file() -> Option<PathBuf> {
    config_dir().map(|d| d.join("engines.json"))
}
