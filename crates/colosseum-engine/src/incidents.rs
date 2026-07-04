// SPDX-License-Identifier: GPL-3.0-or-later
//! Incident reports: when a game ends abnormally (crash, illegal move, time
//! forfeit), the runner writes a plain-text forensic file — engines, position,
//! move list, the offending detail, and both engines' recent UCI traffic plus
//! stderr output — so the *cause* can actually be established instead of
//! guessed.
//!
//! The directory is set once at startup by the GUI (`<data dir>/logs/incidents`).
//! When unset (unit tests, headless use), writing is a silent no-op.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

static DIR: OnceLock<PathBuf> = OnceLock::new();
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// Set the directory incident files are written into (once, at startup).
pub fn set_dir(dir: PathBuf) {
    let _ = DIR.set(dir);
}

/// Write one incident report; returns the file name (not the full path) for
/// inclusion in user-facing error messages. `None` when no directory is set
/// or the write fails (never disrupts the game flow).
pub fn write(stub: &str, contents: &str) -> Option<String> {
    let dir = DIR.get()?;
    std::fs::create_dir_all(dir).ok()?;
    // Unique, sortable, filesystem-safe name.
    let now = time::OffsetDateTime::now_utc();
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let stub: String = stub
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let name = format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}-{seq:03}-{stub}.txt",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second(),
    );
    let path = dir.join(&name);
    match std::fs::write(&path, contents) {
        Ok(()) => Some(name),
        Err(err) => {
            tracing::warn!(target: "incidents", "failed to write incident file: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_without_dir_is_noop() {
        // DIR may or may not be set by other tests; only assert no panic.
        let _ = write("a b/c", "contents");
    }
}
