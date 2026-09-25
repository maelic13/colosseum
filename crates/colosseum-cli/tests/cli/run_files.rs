//! Run files as a run reads them: what the command line replaces is said, and
//! kept in the run record.

use std::process::Command;

use serde_json::Value;

/// A side's option list on the command line replaces the run file's, as
/// documented; the entries that drops are named on standard error and kept in
/// the run record, so a run that lost `Threads` never loses it silently.
#[test]
fn a_command_line_option_list_that_drops_run_file_entries_is_warned_and_recorded() {
    let root = tempfile::tempdir().unwrap();
    let engine = env!("CARGO_BIN_EXE_colosseum-cli").replace('\\', "/");
    let run_file = root.path().join("gate.toml");
    std::fs::write(
        &run_file,
        format!(
            "command = [\"match\"]\npositionals = [\"{engine}\", \"{engine}\"]\n\
             [options]\na-option = [\"Hash=64\", \"Threads=2\"]\n"
        ),
    )
    .unwrap();
    let run = root.path().join("run");
    let output = Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
        .arg("--run-file")
        .arg(&run_file)
        .args([
            "--a-option",
            "Hash=32",
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            "--games",
            "2",
            "--a-movetime-ms",
            "5",
            "--b-movetime-ms",
            "5",
            "--max-moves",
            "2",
            "--seed",
            "11",
            "--json",
            "--dir",
        ])
        .arg(&run)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let warning = "--a-option on the command line replaces the run file's list and drops Threads=2";
    assert!(stderr.contains(&format!("warning: {warning}")), "{stderr}");
    serde_json::from_slice::<Value>(&output.stdout).expect("stdout stays one JSON value");

    let record: Value =
        serde_json::from_slice(&std::fs::read(run.join("run-record.json")).unwrap()).unwrap();
    let anomalies = record["anomalies"].as_array().unwrap();
    let replaced = anomalies
        .iter()
        .find(|anomaly| anomaly["code"] == "run-file-list-replaced")
        .unwrap_or_else(|| panic!("{anomalies:?}"));
    assert!(
        replaced["message"].as_str().unwrap().starts_with(warning),
        "{replaced}"
    );
}
