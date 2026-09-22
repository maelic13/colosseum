//! Both engines of a game are prepared at once; a setup failure still belongs
//! to the side that failed.
//!
//! Engine B fails its handshake (an over-long protocol line) while engine A
//! prepares normally. B is Black in game 1 and White in game 2, so the fault
//! must name Black, then White, and each game's forensic must be B's.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

#[test]
fn a_setup_failure_is_attributed_to_its_own_side_when_both_prepare_at_once() {
    let root = tempfile::tempdir().unwrap();
    let run = root.path().join("run");
    let engine = Path::new(env!("CARGO_BIN_EXE_colosseum-cli"));
    let output = Command::new(engine)
        .args(["--json", "match", "--games", "2"])
        .arg(engine)
        .arg(engine)
        .args([
            "--a-engine-arg=__uci-stub",
            "--b-engine-arg=__uci-stub",
            "--b-engine-arg=--mode",
            "--b-engine-arg=long-line",
            "--a-label",
            "healthy",
            "--b-label",
            "broken",
            "--a-movetime-ms",
            "20",
            "--b-movetime-ms",
            "20",
            "--max-engine-faults",
            "100",
            "--dir",
        ])
        .arg(&run)
        .output()
        .unwrap();
    assert!(
        output.status.code().is_some(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let journal = std::fs::read_to_string(run.join("games.jsonl")).unwrap();
    let mut sides = journal
        .lines()
        .map(|line| {
            let game = &serde_json::from_str::<Value>(line).unwrap()["game"];
            (
                game["number"].as_u64().unwrap(),
                game["fault"]["side"].as_str().unwrap_or("none").to_owned(),
            )
        })
        .collect::<Vec<_>>();
    sides.sort();
    assert_eq!(
        sides,
        [(1, "black".to_owned()), (2, "white".to_owned())],
        "{journal}"
    );
    let forensics = std::fs::read_dir(run.join("failed-games"))
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(forensics.len(), 2);
    for text in &forensics {
        assert!(text.contains("broken"), "{text}");
        assert!(text.contains("exceeds"), "{text}");
    }
}
