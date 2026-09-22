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
    // The released versions of the two products, each owned by its own
    // manifest: the GUI's first release from this shared code base and the
    // CLI's first release ever.
    assert!(gui.contains("version = \"1.1.0\""));
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
    let source_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let source = source_files(&source_root)
        .into_iter()
        .map(|path| fs::read_to_string(path).unwrap())
        .collect::<String>();
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

#[test]
fn cli_runner_adapter_does_not_pull_in_legacy_database_or_scheduler_code() {
    let manifest =
        fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    assert!(manifest.contains(
        "colosseum-engine = { path = \"../colosseum-engine\", default-features = false, features = [\"platform\", \"runner\"] }"
    ));
    assert!(!manifest.contains("rusqlite"));

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(env!("CARGO"))
        .args(["tree", "-p", "colosseum-cli", "--no-default-features"])
        .current_dir(root)
        .output()
        .expect("cargo tree");
    assert!(output.status.success());
    let tree = String::from_utf8(output.stdout).unwrap();
    for forbidden in ["rusqlite", "libsqlite3-sys", "crossbeam-channel"] {
        assert!(
            !tree.contains(forbidden),
            "independent CLI runner pulled in {forbidden}"
        );
    }
}

#[test]
fn independent_release_lanes_are_complete_and_least_privileged() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(!root.join(".github/workflows/release.yml").exists());
    let gui = fs::read_to_string(root.join(".github/workflows/release-gui.yml")).unwrap();
    let cli = fs::read_to_string(root.join(".github/workflows/release-cli.yml")).unwrap();

    assert!(gui.contains("'gui-v*'"));
    assert!(!gui.contains("'cli-v*'"));
    assert!(cli.contains("'cli-v*'"));
    assert!(!cli.contains("'gui-v*'"));
    assert!(cli.contains("[cli candidate]"));

    // Each lane builds, packages and smokes exactly its own product, through
    // the one build entry point, and never reaches across to the other's.
    assert!(cli.contains("cargo xtask package cli"));
    assert!(!cli.contains("xtask package gui"));
    assert!(gui.contains("cargo xtask package gui"));
    assert!(!gui.contains("xtask package cli"));

    for (workflow, name) in [(&gui, "gui"), (&cli, "cli")] {
        assert!(workflow.contains("permissions:\n  contents: read"), "{name}");
        assert_eq!(workflow.matches("contents: write").count(), 1, "{name}");
        // A candidate proves the whole lane without a tag or a release.
        assert!(workflow.contains("workflow_dispatch:"), "{name}");
        assert!(workflow.contains("CANDIDATE.json"), "{name}");
        // The published set is an exact list checked by name and by count, so
        // a release carries what its matrix produced and nothing else.
        assert!(workflow.contains("expected=("), "{name}");
        assert!(
            workflow.contains(r#"for file in "${expected[@]}"; do test -f "release-artifacts/$file"; done"#),
            "{name}"
        );
        assert!(
            workflow.contains("(cd release-artifacts && sha256sum --check SHA256SUMS)"),
            "{name}"
        );
        // Checksums are generated and re-checked, never published as an asset.
        assert!(!workflow.contains("release-artifacts/SHA256SUMS\n"), "{name}");
    }
}

#[test]
fn repository_latest_release_is_owned_by_the_gui_product_lane() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let gui = fs::read_to_string(root.join(".github/workflows/release-gui.yml")).unwrap();
    let cli = fs::read_to_string(root.join(".github/workflows/release-cli.yml")).unwrap();

    // GitHub keeps exactly one repository-wide "latest" release. Leaving it to
    // chance lets a CLI tag displace the GUI download the front page offers.
    assert!(
        cli.contains("\n          make_latest: false\n"),
        "CLI release lane must never claim the repository-wide latest release"
    );
    assert!(
        gui.contains(
            "\n          make_latest: ${{ needs.validate.outputs.prerelease == 'true' && 'false' || 'true' }}\n"
        ),
        "GUI release lane must claim latest for a stable release and never for a prerelease"
    );
    for workflow in [&gui, &cli] {
        assert_eq!(workflow.matches("make_latest:").count(), 1);
    }
}

#[test]
fn user_facing_documentation_links_product_tag_lists_not_repository_latest() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut documents = vec![
        root.join("README.md"),
        root.join("README-CLI.md"),
        root.join("CHANGELOG.md"),
        root.join("CHANGELOG-CLI.md"),
        root.join("CHANGELOG-GUI.md"),
    ];
    documents.extend(
        fs::read_dir(root.join("docs/cli"))
            .unwrap()
            .map(|entry| entry.unwrap().path()),
    );

    for document in documents {
        let text = fs::read_to_string(&document).unwrap();
        assert!(
            !text.contains("/releases/latest"),
            "{} links the repository-wide latest release",
            document.display()
        );
        // An unfiltered release list mixes both products; product pages link a
        // tag-filtered list instead.
        assert!(
            !text.contains("colosseum/releases)") && !text.contains("colosseum/releases>"),
            "{} links the unscoped repository release list",
            document.display()
        );
    }
}

fn source_files(directory: &std::path::Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(source_files(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    files
}
