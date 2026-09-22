//! Support for explicitly opt-in real-engine smoke tests.
//!
//! This module is compiled only by the `real-engine-smoke` test targets. Those
//! targets require `COLOSSEUM_SMOKE_ENGINE`; required tests never discover a
//! local engine or read this environment variable.

use std::path::{Path, PathBuf};

/// Copy the engine into a fresh temp dir; returns the guard (keep alive) and path.
pub fn copy_to_temp(src: &Path) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let dest = dir.path().join(src.file_name().expect("engine file name"));
    std::fs::copy(src, &dest).expect("copy engine");
    (dir, dest)
}

/// Read the explicit smoke-engine path and make an isolated temporary copy.
///
/// A smoke test requested without a usable engine is a configuration error,
/// not a pass-by-skip: callers intentionally opted into this test tier.
pub fn smoke_engine() -> (tempfile::TempDir, PathBuf) {
    let source = std::env::var("COLOSSEUM_SMOKE_ENGINE")
        .expect("real-engine smoke test requires COLOSSEUM_SMOKE_ENGINE to name a UCI executable");
    let source = PathBuf::from(source);
    assert!(
        source.is_file(),
        "COLOSSEUM_SMOKE_ENGINE must name an existing executable file: {}",
        source.display()
    );
    copy_to_temp(&source)
}
