//! Centralized naming so the app can be renamed in exactly one place.
//!
//! To rename the application:
//!  1. Change [`DISPLAY_NAME`] (and [`APP_DIR_NAME`] if you also want to move the
//!     data directory — that requires a one-time migration of existing user data).
//!  2. Optionally rename the `colosseum-*` crates / package names in `Cargo.toml`.
//!
//! Version comes from the building crate via `env!("CARGO_PKG_VERSION")`.

/// Human-facing application name (window title, About, etc.).
pub const DISPLAY_NAME: &str = "Colosseum";

/// Application directory name used by `directories::ProjectDirs`.
/// Changing this moves the config/data directories.
pub const APP_DIR_NAME: &str = "Colosseum";

/// `ProjectDirs` qualifier (reverse-domain prefix). Empty keeps paths clean
/// (e.g. `%APPDATA%\Colosseum`, `~/.config/colosseum`).
pub const QUALIFIER: &str = "";

/// `ProjectDirs` organization segment. Empty avoids a redundant nested folder.
pub const ORGANIZATION: &str = "";
