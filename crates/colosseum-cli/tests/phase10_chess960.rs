//! Chess960 is a non-goal, and a request for it is refused rather than tried.
//!
//! Attempting the variant would be worse than declining it: castling in
//! Chess960 is encoded as king-onto-rook, which this harness reads as an
//! ordinary move, so a forwarded `UCI_Chess960=true` produces games that look
//! scored and are not.

use std::path::Path;
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_colosseum-cli"))
}

/// A Chess960 start position in Shredder castling notation.
const SHUFFLED: &str = "bqnbrkrn/pppppppp/8/8/8/8/PPPPPPPP/BQNBRKRN w GEge - 0 1";
const STANDARD: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

#[test]
fn every_option_path_refuses_a_chess960_request_by_name() {
    let invocations: Vec<Vec<&str>> = vec![
        vec![
            "match",
            "--games",
            "2",
            "a",
            "b",
            "--a-option",
            "UCI_Chess960=true",
        ],
        vec![
            "match",
            "--games",
            "2",
            "a",
            "b",
            "--b-option",
            "UCI_Chess960=true",
        ],
        vec![
            "sprt",
            "a",
            "b",
            "--preset",
            "gainer",
            "--max-pairs",
            "2",
            "--a-option",
            "UCI_Chess960=true",
        ],
        vec![
            "tournament",
            "run",
            "--engine",
            "a",
            "--engine",
            "b",
            "--option",
            "UCI_Chess960=true",
        ],
        vec![
            "tournament",
            "run",
            "--engine",
            "a",
            "--engine",
            "b",
            "--engine-option",
            "1:UCI_Chess960=true",
        ],
    ];
    for invocation in invocations {
        let output = cli()
            .args(&invocation)
            .args(["--dry-run", "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{invocation:?} was accepted");
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(
            stderr.contains("Chess960") && stderr.contains("standard chess only"),
            "{invocation:?} refusal is unclear: {stderr}"
        );
    }
}

/// Turning the option off is not a request for the variant, so it is ordinary.
#[test]
fn switching_chess960_off_is_an_ordinary_forwarded_option() {
    let output = cli()
        .args([
            "match",
            "--games",
            "2",
            "a",
            "b",
            "--a-option",
            "UCI_Chess960=false",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_chess960_castling_encoding_is_rejected_rather_than_reinterpreted() {
    let root = tempfile::tempdir().unwrap();
    let book = root.path().join("mixed.epd");
    std::fs::write(&book, format!("{SHUFFLED}\n{STANDARD}\n")).unwrap();

    let output = cli()
        .args(["book", "verify"])
        .arg(&book)
        .arg("--json")
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["audit"]["candidates"], 2);
    assert_eq!(value["audit"]["usable"], 1);
    // The one-based index of the shuffled position, named rather than dropped.
    assert_eq!(value["audit"]["rejected_indices"][0], 1);

    // The same position in a suite is a malformed entry, never a search.
    let positions = root.path().join("positions.fen");
    std::fs::write(&positions, format!("{SHUFFLED}\n")).unwrap();
    let output = cli()
        .arg("suite")
        .arg(Path::new(env!("CARGO_BIN_EXE_colosseum-cli")))
        .arg(&positions)
        .args(["--movetime-ms", "5", "--dry-run", "--json"])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let entry = &value["resolved_configuration"]["design"]["entries"][0];
    assert_eq!(entry["entry"], "malformed");
    assert!(
        entry["reason"].as_str().unwrap().contains("castling"),
        "{entry}"
    );
}
