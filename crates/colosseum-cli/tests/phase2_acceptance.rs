use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Manifest {
    phase: u64,
    gates: Vec<Gate>,
}

#[derive(Debug, Deserialize)]
struct Gate {
    id: String,
    owner: String,
}

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

fn copy_fixture(root: &Path, name: &str) -> PathBuf {
    let extension = if cfg!(windows) { ".exe" } else { "" };
    let destination = root.join(format!("{name}{extension}"));
    std::fs::copy(env!("CARGO_BIN_EXE_colosseum-uci-fixture"), &destination).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&destination).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&destination, permissions).unwrap();
    }
    destination
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut output = BTreeMap::new();
    if !root.exists() {
        return output;
    }
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            output.extend(snapshot(&path));
        } else {
            output.insert(path.clone(), std::fs::read(path).unwrap());
        }
    }
    output
}

#[test]
fn two_independent_ordinary_uci_paths_pass_without_descriptors_or_options() {
    let root = tempfile::tempdir().unwrap();
    for name in ["engine-a", "engine-b"] {
        let engine = copy_fixture(root.path(), name);
        let output = cli()
            .args(["engine", "check", "--json"])
            .arg(engine)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["type"], "engine-compliance");
        assert_eq!(value["report"]["success"], true);
    }
}

#[test]
fn isolated_cli_dry_run_never_reads_or_writes_gui_application_state() {
    let root = tempfile::tempdir().unwrap();
    let invocation = root.path().join("invocation");
    let gui_state = root.path().join("sentinel-gui-state");
    std::fs::create_dir_all(&invocation).unwrap();
    std::fs::create_dir_all(&gui_state).unwrap();
    std::fs::write(gui_state.join("engines.json"), b"sentinel engine library").unwrap();
    std::fs::write(gui_state.join("colosseum.sqlite"), b"sentinel database").unwrap();
    let before = snapshot(&gui_state);

    let output = cli()
        .args(["engine", "inspect", "--dry-run", "--json", "missing-engine"])
        .current_dir(&invocation)
        .env("APPDATA", &gui_state)
        .env("LOCALAPPDATA", &gui_state)
        .env("XDG_CONFIG_HOME", &gui_state)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(snapshot(&gui_state), before);
    assert_eq!(std::fs::read_dir(&invocation).unwrap().count(), 0);
}

#[test]
fn acceptance_manifest_names_every_required_gate_owner() {
    let manifest: Manifest = serde_json::from_str(include_str!(
        "../../../docs/fixtures/phase2/acceptance.json"
    ))
    .unwrap();
    assert_eq!(manifest.phase, 2);
    let expected = [
        "path-only-engines",
        "configuration-equivalence",
        "named-random-streams",
        "durable-recovery",
        "published-self-test",
        "process-hardening",
        "architecture-independence",
        "gui-regression",
    ];
    assert_eq!(manifest.gates.len(), expected.len());
    for id in expected {
        let gate = manifest.gates.iter().find(|gate| gate.id == id).unwrap();
        assert!(!gate.owner.trim().is_empty());
    }
}
