//! Shared test support: locate a real engine and copy it to a temp dir so the
//! original is never touched. Tests skip gracefully when no engine is available.

use std::path::{Path, PathBuf};

/// Locate a UCI engine: `COLOSSEUM_TEST_ENGINE` env, else the known dev path.
pub fn locate_engine() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("COLOSSEUM_TEST_ENGINE") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Some(path);
        }
    }
    let default = PathBuf::from(r"D:\chess\engines\stockfish.exe");
    default.exists().then_some(default)
}

/// Copy the engine into a fresh temp dir; returns the guard (keep alive) and path.
pub fn copy_to_temp(src: &Path) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let dest = dir.path().join(src.file_name().expect("engine file name"));
    std::fs::copy(src, &dest).expect("copy engine");
    (dir, dest)
}

/// Convenience: a temp engine copy, or `None` to skip the test.
pub fn engine_or_skip() -> Option<(tempfile::TempDir, PathBuf)> {
    let src = locate_engine()?;
    Some(copy_to_temp(&src))
}
