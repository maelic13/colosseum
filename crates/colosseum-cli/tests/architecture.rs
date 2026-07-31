use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

#[test]
fn cli_dependency_graph_contains_no_gui_or_windowing_package() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1"])
        .current_dir(root)
        .output()
        .expect("cargo metadata");
    assert!(output.status.success());
    let metadata: Value = serde_json::from_slice(&output.stdout).unwrap();
    let packages = metadata["packages"].as_array().unwrap();
    let resolve = metadata["resolve"]["nodes"].as_array().unwrap();

    let names: HashMap<&str, &str> = packages
        .iter()
        .map(|package| {
            (
                package["id"].as_str().unwrap(),
                package["name"].as_str().unwrap(),
            )
        })
        .collect();
    let edges: HashMap<&str, Vec<&str>> = resolve
        .iter()
        .map(|node| {
            (
                node["id"].as_str().unwrap(),
                node["dependencies"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| id.as_str().unwrap())
                    .collect(),
            )
        })
        .collect();
    let root_id = names
        .iter()
        .find_map(|(id, name)| (*name == "colosseum-cli").then_some(*id))
        .unwrap();
    let mut pending = vec![root_id];
    let mut visited = HashSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let name = names[id];
        assert!(
            !matches!(
                name,
                "colosseum-gui" | "eframe" | "egui" | "egui-winit" | "winit"
            ),
            "headless CLI depends on forbidden windowing package {name}"
        );
        pending.extend(edges.get(id).into_iter().flatten().copied());
    }
}

#[test]
fn product_versions_are_owned_by_product_manifests() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let workspace = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    assert!(!workspace.contains("[workspace.package]\nversion"));

    let gui = fs::read_to_string(root.join("crates/colosseum-gui/Cargo.toml")).unwrap();
    let cli = fs::read_to_string(root.join("crates/colosseum-cli/Cargo.toml")).unwrap();
    assert!(gui.contains("version = \"1.0.2\""));
    assert!(cli.contains("version = \"0.1.0\""));
    assert!(
        !gui.lines()
            .any(|line| line.trim_start().starts_with("version.workspace"))
    );
    assert!(
        !cli.lines()
            .any(|line| line.trim_start().starts_with("version.workspace"))
    );
}

#[test]
fn cli_source_has_no_gui_state_or_app_directory_access() {
    let source =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/main.rs")).unwrap();
    for forbidden in [
        "AppDirs",
        "EngineLibrary",
        "ProjectDirs",
        "APPDATA",
        "engines.json",
        "colosseum.sqlite",
    ] {
        assert!(
            !source.contains(forbidden),
            "CLI source contains {forbidden}"
        );
    }
}
