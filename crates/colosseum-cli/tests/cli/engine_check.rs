use std::process::Command;

#[test]
fn an_ordinary_uci_path_passes_without_descriptors_or_options() {
    let output = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .args(["engine", "check", "--json"])
        .arg(env!("CARGO_BIN_EXE_colosseum-uci-fixture"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["type"], "engine-compliance");
    assert_eq!(value["report"]["success"], true);
}
