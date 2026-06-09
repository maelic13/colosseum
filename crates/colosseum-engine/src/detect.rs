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
