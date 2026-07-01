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
/// Many engines report their version as trailing tokens of `id name`
/// (`"Stockfish 16.1"`, `"lc0 v0.30.0"`, `"Deep HIARCS 14 WCSC"`). Trailing
/// architecture/platform descriptors (`"64-bit"`, `"x86-64"`, `"SSE42"`, …) are
/// noise and are dropped first. Then the *last* token containing a digit marks
/// the start of the version: that token and everything after it become the
/// version (so suffixes like `"WCSC"` or `"mp"` stay attached), and the tokens
/// before it become the name. A single leading `v`/`V` before a digit is
/// dropped. When no token contains a digit, the whole string (minus noise) is
/// the name and the version is `None` (e.g. `"Fire"`, `"Stash Bot"`).
///
/// Example: `"Critter 1.6a 64-bit"` → name `"Critter"`, version `"1.6a"`.
#[must_use]
pub fn split_name_version(id_name: &str) -> (String, Option<String>) {
    let trimmed = id_name.trim();
    let mut tokens: Vec<&str> = trimmed.split_whitespace().collect();

    // Drop trailing architecture/platform descriptors — they are never the
    // version. Keep at least one token so a bare "x64" engine isn't erased.
    while tokens.len() > 1 && is_arch_noise(tokens[tokens.len() - 1]) {
        tokens.pop();
    }

    // The version starts at the last digit-bearing token (never the first
    // token, which is always part of the name — think "lc0" or "K2").
    let version_start = tokens
        .iter()
        .rposition(|t| t.chars().any(|c| c.is_ascii_digit()))
        .filter(|&i| i > 0);
    if let Some(i) = version_start {
        let mut version_tokens: Vec<&str> = tokens.split_off(i);
        version_tokens[0] = strip_version_prefix(version_tokens[0]);
        return (tokens.join(" "), Some(version_tokens.join(" ")));
    }
    (tokens.join(" "), None)
}

/// True when a token is an architecture/platform/build descriptor rather than a
/// version or part of the engine name. Matched case-insensitively after
/// stripping surrounding brackets.
fn is_arch_noise(token: &str) -> bool {
    let t = token
        .trim_matches(|c| matches!(c, '(' | ')' | '[' | ']'))
        .to_ascii_lowercase();
    // SIMD families come in many numbered spellings (SSE42, SSE4.2, AVX-512,
    // avx2, …): an "sse"/"avx" prefix followed only by digits/./-/_ is noise.
    for prefix in ["sse", "avx"] {
        if let Some(rest) = t.strip_prefix(prefix)
            && rest
                .chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
        {
            return true;
        }
    }
    matches!(
        t.as_str(),
        "64-bit"
            | "32-bit"
            | "64bit"
            | "32bit"
            | "64"
            | "x64"
            | "x86"
            | "x86-64"
            | "x86_64"
            | "amd64"
            | "arm64"
            | "aarch64"
            | "win"
            | "win64"
            | "win32"
            | "windows"
            | "linux"
            | "macos"
            | "osx"
            | "bmi"
            | "bmi2"
            | "pext"
            | "popcnt"
            | "ssse3"
            | "neon"
            | "vnni"
            | "modern"
    )
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
    fn strips_trailing_architecture_noise() {
        // The reported case: "64-bit" is noise, "1.6a" is the real version.
        assert_eq!(
            split_name_version("Critter 1.6a 64-bit"),
            ("Critter".to_string(), Some("1.6a".to_string()))
        );
        // Multiple trailing noise tokens are all dropped.
        assert_eq!(
            split_name_version("Stockfish 16 avx2 x86-64"),
            ("Stockfish".to_string(), Some("16".to_string()))
        );
        // Noise with no version leaves just the name.
        assert_eq!(
            split_name_version("Fire x64"),
            ("Fire".to_string(), None)
        );
    }

    #[test]
    fn strips_numbered_simd_noise() {
        // "SSE42" is an architecture artifact, not the version.
        assert_eq!(
            split_name_version("Deep Rybka 4.1 SSE42"),
            ("Deep Rybka".to_string(), Some("4.1".to_string()))
        );
        assert_eq!(
            split_name_version("Deep Rybka 4 SSE42"),
            ("Deep Rybka".to_string(), Some("4".to_string()))
        );
        assert_eq!(
            split_name_version("Engine 3 AVX-512"),
            ("Engine".to_string(), Some("3".to_string()))
        );
    }

    #[test]
    fn keeps_suffix_words_after_version_number() {
        // Words after the last digit-bearing token belong to the version.
        assert_eq!(
            split_name_version("Deep HIARCS 14 WCSC"),
            ("Deep HIARCS".to_string(), Some("14 WCSC".to_string()))
        );
        assert_eq!(
            split_name_version("Rybka 2.3.2a mp"),
            ("Rybka".to_string(), Some("2.3.2a mp".to_string()))
        );
    }

    #[test]
    fn first_token_is_never_the_version() {
        // Digit-bearing names like "lc0" or "K2" stay names.
        assert_eq!(split_name_version("K2"), ("K2".to_string(), None));
        assert_eq!(
            split_name_version("lc0 v0.30.0"),
            ("lc0".to_string(), Some("0.30.0".to_string()))
        );
    }

    #[test]
    fn keeps_a_bare_architecture_only_name() {
        // Don't erase everything if the whole name looks like noise.
        assert_eq!(split_name_version("x64"), ("x64".to_string(), None));
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
