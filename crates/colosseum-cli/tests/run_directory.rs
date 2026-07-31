use colosseum_cli::{RunDirectory, RunDirectoryError, built_in_defaults, resolve_config};
use serde::{Deserialize, Serialize};
use serde_json::json;

fn config(root: &std::path::Path, value: u64) -> colosseum_cli::ResolvedConfig {
    resolve_config(
        built_in_defaults(),
        None,
        json!({"value": value}),
        &[],
        root,
        &[],
    )
    .unwrap()
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Checkpoint {
    committed: u64,
}

#[test]
fn default_runs_are_unique_and_self_contained() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), 1);
    let first = RunDirectory::create_unique(root.path(), "match", &config).unwrap();
    let second = RunDirectory::create_unique(root.path(), "match", &config).unwrap();
    assert_ne!(first.directory.paths().root, second.directory.paths().root);
    for run in [first, second] {
        assert!(
            run.directory
                .paths()
                .root
                .starts_with(root.path().join("colosseum-runs"))
        );
        assert!(
            run.directory
                .paths()
                .root
                .join("resolved-config.json")
                .is_file()
        );
        assert_eq!(run.directory.config_sha256(), config.sha256());
        assert!(!run.resumed);
    }
}

#[test]
fn explicit_directory_resumes_and_never_truncates_logs() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("chosen-run");
    let initial_config = config(root.path(), 1);
    let first = RunDirectory::open_explicit(&path, &initial_config, false).unwrap();
    first.directory.append_log(b"first\n").unwrap();
    let resumed = RunDirectory::open_explicit(&path, &initial_config, false).unwrap();
    assert!(resumed.resumed);
    resumed.directory.append_log(b"second\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(path.join("run.log")).unwrap(),
        "first\nsecond\n"
    );

    let mismatch = config(root.path(), 2);
    assert!(matches!(
        RunDirectory::open_explicit(&path, &mismatch, false),
        Err(RunDirectoryError::ConfigMismatch { .. })
    ));
}

#[test]
fn restart_archives_the_complete_previous_run() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("chosen-run");
    let old = config(root.path(), 1);
    let first = RunDirectory::open_explicit(&path, &old, false).unwrap();
    first.directory.append_log(b"old evidence\n").unwrap();
    first
        .directory
        .write_checkpoint(&Checkpoint { committed: 7 })
        .unwrap();

    let new = config(root.path(), 2);
    let restarted = RunDirectory::open_explicit(&path, &new, true).unwrap();
    let archive = restarted.archived.unwrap();
    assert_eq!(
        std::fs::read_to_string(archive.join("run.log")).unwrap(),
        "old evidence\n"
    );
    assert!(archive.join("checkpoint.json").is_file());
    assert!(!path.join("run.log").exists());
    assert_eq!(restarted.directory.config_sha256(), new.sha256());
}

#[test]
fn corrupted_current_checkpoint_recovers_previous_generation() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), 1);
    let run = RunDirectory::create_unique(root.path(), "sprt", &config)
        .unwrap()
        .directory;
    run.write_checkpoint(&Checkpoint { committed: 1 }).unwrap();
    run.write_checkpoint(&Checkpoint { committed: 2 }).unwrap();
    assert_eq!(run.read_checkpoint::<Checkpoint>().unwrap().committed, 2);
    std::fs::write(&run.paths().checkpoint, b"torn").unwrap();
    assert_eq!(run.read_checkpoint::<Checkpoint>().unwrap().committed, 1);
    std::fs::write(&run.paths().previous_checkpoint, b"also torn").unwrap();
    assert!(matches!(
        run.read_checkpoint::<Checkpoint>(),
        Err(RunDirectoryError::NoValidCheckpoint { .. })
    ));
}
