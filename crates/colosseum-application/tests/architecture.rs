use std::fs;
use std::path::PathBuf;

#[test]
fn application_manifest_has_only_inward_dependencies() {
    let manifest = fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
        .expect("application manifest");
    for forbidden in [
        "colosseum-uci",
        "colosseum-engine",
        "colosseum-gui",
        "tokio",
        "crossbeam",
        "rusqlite",
        "eframe",
        "egui",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "application layer must not depend on {forbidden}"
        );
    }
}

#[test]
fn core_manifest_has_no_entropy_or_outer_layer_dependency() {
    let manifest = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../colosseum-core/Cargo.toml"),
    )
    .expect("core manifest");
    for forbidden in [
        "features = [\"v4\"]",
        "colosseum-application",
        "colosseum-uci",
        "colosseum-engine",
        "colosseum-gui",
        "tokio",
        "rusqlite",
        "eframe",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "core must not contain {forbidden}"
        );
    }
}
