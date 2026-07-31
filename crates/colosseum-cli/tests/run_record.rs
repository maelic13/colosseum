use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use colosseum_cli::{
    OfficialSample, RunDirectory, RunRecord, RunRecorder, RunStatus, built_in_defaults,
    resolve_config,
};
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
fn dropped_owner_records_an_aborted_run_with_zero_official_sample() {
    let root = tempfile::tempdir().unwrap();
    let run = RunDirectory::create_unique(root.path(), "match", &config(root.path()))
        .unwrap()
        .directory;
    {
        let _recorder = RunRecorder::begin(&run, "match").unwrap();
    }
    let record = RunRecord::read(&run.paths().root).unwrap();
    assert_eq!(record.status, RunStatus::Aborted);
    assert_eq!(record.official_sample, OfficialSample::default());
    assert_eq!(record.schema_version, 1);
    assert_eq!(record.stats_version, colosseum_core::rng::RNG_VERSION);
    assert!(
        record
            .host
            .capabilities
            .contains_key("process-tree-containment")
    );
    assert_eq!(record.anomalies[0].code, "workflow-owner-dropped");
}

#[test]
fn terminal_record_keeps_the_official_committed_sample() {
    let root = tempfile::tempdir().unwrap();
    let run = RunDirectory::create_unique(root.path(), "sprt", &config(root.path()))
        .unwrap()
        .directory;
    let mut recorder = RunRecorder::begin(&run, "sprt").unwrap();
    let sample = OfficialSample {
        committed_units: 4,
        scored_games: 4,
        completed_pairs: 2,
        pentanomial: [0, 0, 1, 1, 0],
        unpaired_games: 0,
    };
    recorder.update_sample(sample.clone()).unwrap();
    recorder
        .add_anomaly("clock-resolution", "coarse timer")
        .unwrap();
    recorder.finish(RunStatus::Completed).unwrap();
    let record = RunRecord::read(&run.paths().root).unwrap();
    assert_eq!(record.status, RunStatus::Completed);
    assert_eq!(record.official_sample, sample);
    assert_eq!(record.anomalies.len(), 1);
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
