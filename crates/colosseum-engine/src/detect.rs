// SPDX-License-Identifier: GPL-3.0-or-later
//! One-shot engine detection: spawn, handshake, collect identity + options, quit.
//!
//! Used by the Engine Management tab when the user adds an engine by path or
//! scans a folder. The result populates the engine's metadata and
//! detected-options schema stored in [`colosseum_core::EngineConfig`].

use std::path::Path;
use std::time::Duration;

use colosseum_core::UciOption;
use colosseum_uci::{EngineProcess, SpawnOptions};

use crate::error::EngineError;

/// Identity and option declarations collected during one handshake run.
#[derive(Debug, Clone, Default)]
pub struct DetectResult {
    /// The value of the engine's `id name` line, if reported.
    pub name: Option<String>,
    /// The value of the engine's `id author` line, if reported.
    pub author: Option<String>,
    /// All `option` declarations emitted before `uciok`.
    pub options: Vec<UciOption>,
}

/// Spawn the engine at `path`, complete the `uci` / `isready` handshake,
/// collect identity and option declarations, then quit.
///
/// The handshake times out after **10 seconds** if the engine never sends
/// `uciok`. The engine is killed on drop regardless of how `detect_engine`
/// returns.
pub async fn detect_engine(path: &Path) -> Result<DetectResult, EngineError> {
    let opts = SpawnOptions::new(path);
    let mut proc = EngineProcess::spawn(opts).await?;
    proc.handshake(Duration::from_secs(10)).await?;

    let result = DetectResult {
        name: proc.name().map(str::to_string),
        author: proc.author().map(str::to_string),
        options: proc.options().to_vec(),
    };

    // Best-effort graceful quit; if it times out the engine is killed on drop.
    let _ = proc.quit(Duration::from_secs(2)).await;
    Ok(result)
}

/// Split a UCI `id name` into a display name and an optional version.
///
/// Many engines report their version as the trailing token of `id name`
/// (`"Stockfish 16.1"`, `"lc0 v0.30.0"`, `"Stockfish dev-20231041"`). When the
/// last whitespace-separated token contains a digit it is treated as the
/// version — a single leading `v`/`V` before a digit is dropped — and the
/// remaining tokens become the name. Otherwise the whole string is the name and
/// the version is `None` (e.g. `"Fire"`, `"Stash Bot"`).
#[must_use]
pub fn split_name_version(id_name: &str) -> (String, Option<String>) {
    let trimmed = id_name.trim();
    let mut tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.len() >= 2 {
        let last = tokens[tokens.len() - 1];
        if last.chars().any(|c| c.is_ascii_digit()) {
            tokens.pop();
            return (tokens.join(" "), Some(strip_version_prefix(last).to_string()));
        }
    }
    (trimmed.to_string(), None)
}

/// Drop a single leading `v`/`V` when it immediately precedes a digit
/// (`"v0.30.0"` → `"0.30.0"`); otherwise return the token unchanged.
fn strip_version_prefix(token: &str) -> &str {
    if let Some(rest) = token.strip_prefix(['v', 'V'])
        && rest.starts_with(|c: char| c.is_ascii_digit())
    {
        rest
    } else {
        token
    }
}

#[cfg(test)]
mod tests {
    use super::split_name_version;

    #[test]
    fn splits_trailing_numeric_version() {
        assert_eq!(
            split_name_version("Stockfish 16.1"),
            ("Stockfish".to_string(), Some("16.1".to_string()))
        );
        assert_eq!(
            split_name_version("Komodo 14"),
            ("Komodo".to_string(), Some("14".to_string()))
        );
    }

    #[test]
    fn strips_leading_v_prefix() {
        assert_eq!(
            split_name_version("lc0 v0.30.0"),
            ("lc0".to_string(), Some("0.30.0".to_string()))
        );
    }

    #[test]
    fn keeps_non_numeric_trailing_token_as_name() {
        assert_eq!(
            split_name_version("Stash Bot"),
            ("Stash Bot".to_string(), None)
        );
        assert_eq!(split_name_version("Fire"), ("Fire".to_string(), None));
    }

    #[test]
    fn treats_mixed_trailing_token_as_version() {
        assert_eq!(
            split_name_version("Stockfish dev-20231041"),
            ("Stockfish".to_string(), Some("dev-20231041".to_string()))
        );
    }

    #[test]
    fn multi_word_name_with_version() {
        assert_eq!(
            split_name_version("Mr Bob 1.0.0"),
            ("Mr Bob".to_string(), Some("1.0.0".to_string()))
        );
    }

    #[test]
    fn handles_empty_and_whitespace() {
        assert_eq!(split_name_version("   "), (String::new(), None));
        assert_eq!(
            split_name_version("  Stockfish 16  "),
            ("Stockfish".to_string(), Some("16".to_string()))
        );
    }
}
