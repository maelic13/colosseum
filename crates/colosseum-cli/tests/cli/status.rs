use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use colosseum_cli::{RunDirectory, RunRecorder, RunStatus, built_in_defaults, resolve_config};
use serde_json::json;

fn config(root: &Path) -> colosseum_cli::ResolvedConfig {
    resolve_config(
        built_in_defaults(),
        None,
        json!({"command": "match"}),
        &[],
        root,
        &[],
    )
    .unwrap()
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    std::fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = if path.is_file() {
                std::fs::read(&path).unwrap()
            } else {
                Vec::new()
            };
            (path, bytes)
        })
        .collect()
}

#[test]
fn status_is_json_clean_and_strictly_read_only() {
    let root = tempfile::tempdir().unwrap();
    let run = RunDirectory::create_unique(root.path(), "match", &config(root.path()))
        .unwrap()
        .directory;
    RunRecorder::begin(&run, "match")
        .unwrap()
        .finish(RunStatus::Cancelled)
        .unwrap();
    let before = files(&run.paths().root);
    let output = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .args(["status", "--json"])
        .arg(&run.paths().root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "run-status");
    assert_eq!(value["record"]["status"], "cancelled");
    assert_eq!(files(&run.paths().root), before);
}
