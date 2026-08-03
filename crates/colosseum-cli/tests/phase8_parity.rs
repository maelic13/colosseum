use std::collections::BTreeSet;

use serde::Deserialize;
use sha2::{Digest, Sha256};

const FASTCHESS: &[u8] = include_bytes!(
    "../../../tests/fixtures/statistics/external/phase4b5-live-fastchess.console.txt"
);
const CUTECHESS: &[u8] = include_bytes!(
    "../../../tests/fixtures/statistics/external/phase4b5-live-cutechess.console.txt"
);
const COLOSSEUM: &[u8] =
    include_bytes!("../../../tests/fixtures/statistics/external/phase8-colosseum.json");

#[derive(Debug, Deserialize)]
struct ParityFixture {
    schema_version: u32,
    oracle_matrix: String,
    shared_fields: Vec<String>,
    excluded_fields: Vec<String>,
    exclusion_reason: String,
    version_sources: Vec<VersionSource>,
    observations: Vec<Observation>,
    divergences: Vec<Divergence>,
}

#[derive(Debug, Deserialize)]
struct VersionSource {
    runner: String,
    source: String,
    latest_confirmed: bool,
}

#[derive(Debug, Deserialize)]
struct Observation {
    runner: String,
    artifact_sha256: String,
    games: u32,
    wins: u32,
    draws: u32,
    losses: u32,
    complete_pairs: u32,
    pentanomial: Option<[u32; 5]>,
    faults: u32,
}

#[derive(Debug, Deserialize)]
struct Divergence {
    field: String,
    classification: String,
    reason: String,
}

#[derive(Debug, Deserialize)]
struct ColosseumProjection {
    candidate_commit: String,
    candidate_sha256: String,
    raw_result_sha256: String,
    status: String,
    exit_code: i32,
    games: u32,
    wins: u32,
    draws: u32,
    losses: u32,
    complete_pairs: u32,
    pentanomial: [u32; 5],
    colour_split: ColourSplit,
    faults: Faults,
}

#[derive(Debug, Deserialize)]
struct ColourSplit {
    a_as_white: [u32; 3],
    a_as_black: [u32; 3],
}

#[derive(Debug, Deserialize)]
struct Faults {
    engine_a: u32,
    engine_b: u32,
    time_losses_a: u32,
    time_losses_b: u32,
    infrastructure: u32,
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn finished_games(output: &[u8]) -> Vec<(&str, &str)> {
    std::str::from_utf8(output)
        .unwrap()
        .lines()
        .filter(|line| line.starts_with("Finished game "))
        .map(|line| {
            let colours = line.split_once('(').unwrap().1.split_once(')').unwrap().0;
            let (white, black) = colours.split_once(" vs ").unwrap();
            assert!(line.contains(": 1/2-1/2 {Draw by adjudication}"));
            (white, black)
        })
        .collect()
}

#[test]
fn frozen_external_outputs_prove_shared_game_outcomes() {
    for output in [FASTCHESS, CUTECHESS] {
        let games = finished_games(output);
        assert_eq!(games.len(), 8);
        for (index, colours) in games.iter().enumerate() {
            assert_eq!(
                *colours,
                if index % 2 == 0 {
                    ("A", "B")
                } else {
                    ("B", "A")
                }
            );
        }
    }
}

#[test]
fn release_candidate_matches_the_external_oracles_on_shared_fields() {
    let fixture: ParityFixture =
        serde_json::from_str(include_str!("../../../docs/fixtures/phase8/parity.json")).unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(
        fixture.oracle_matrix,
        "tests/fixtures/statistics/oracle-matrix.md"
    );
    assert!(
        include_str!("../../../tests/fixtures/statistics/oracle-matrix.md")
            .contains("Compare shared model fields")
    );

    let artifacts = [
        ("fastchess", FASTCHESS),
        ("cutechess-cli", CUTECHESS),
        ("colosseum-cli", COLOSSEUM),
    ];
    for (runner, bytes) in artifacts {
        let observation = fixture
            .observations
            .iter()
            .find(|observation| observation.runner == runner)
            .unwrap();
        assert_eq!(sha256(bytes), observation.artifact_sha256);
        assert_eq!(
            (
                observation.games,
                observation.wins,
                observation.draws,
                observation.losses,
                observation.complete_pairs,
                observation.faults,
            ),
            (8, 0, 8, 0, 4, 0)
        );
    }
    assert_eq!(
        fixture
            .observations
            .iter()
            .filter_map(|observation| observation.pentanomial)
            .collect::<Vec<_>>(),
        vec![[0, 0, 4, 0, 0], [0, 0, 4, 0, 0]]
    );

    let shared = fixture
        .shared_fields
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        shared,
        BTreeSet::from([
            "game count",
            "complete pair count",
            "colour reversal",
            "W/D/L",
            "draw ratio",
            "termination class",
            "engine faults",
        ])
    );
    assert_eq!(fixture.excluded_fields.len(), 5);
    assert!(fixture.exclusion_reason.contains("zero variance"));
}

#[test]
fn candidate_identity_versions_and_divergences_are_durable() {
    let fixture: ParityFixture =
        serde_json::from_str(include_str!("../../../docs/fixtures/phase8/parity.json")).unwrap();
    let projection: ColosseumProjection = serde_json::from_slice(COLOSSEUM).unwrap();

    assert_eq!(
        projection.candidate_commit,
        "86fc42b442d0f2a354a1fcc1ec5c09cad47a0f43"
    );
    assert_eq!(
        projection.candidate_sha256,
        "652e1c41cb16261c15a07cdcd1f18cfbf855957b0b7e57794006eee80e97a16f"
    );
    assert_eq!(
        projection.raw_result_sha256,
        "572a5cbb38d3c2ded8f767b47f2f4d528a83e458183d45a8f9f7dd9e4af0e7e6"
    );
    assert_eq!(projection.status, "inconclusive");
    assert_eq!(projection.exit_code, 4);
    assert_eq!(
        (
            projection.games,
            projection.wins,
            projection.draws,
            projection.losses,
            projection.complete_pairs,
            projection.pentanomial,
        ),
        (8, 0, 8, 0, 4, [0, 0, 4, 0, 0])
    );
    assert_eq!(projection.colour_split.a_as_white, [0, 4, 0]);
    assert_eq!(projection.colour_split.a_as_black, [0, 4, 0]);
    assert_eq!(
        projection.faults.engine_a
            + projection.faults.engine_b
            + projection.faults.time_losses_a
            + projection.faults.time_losses_b
            + projection.faults.infrastructure,
        0
    );

    assert_eq!(fixture.version_sources.len(), 2);
    assert!(fixture.version_sources.iter().all(|source| {
        source.latest_confirmed
            && source.source.starts_with("https://github.com/")
            && matches!(source.runner.as_str(), "fastchess" | "cutechess-cli")
    }));
    assert_eq!(fixture.divergences.len(), 2);
    assert!(fixture.divergences.iter().all(|divergence| {
        !divergence.field.is_empty()
            && !divergence.classification.is_empty()
            && !divergence.reason.is_empty()
    }));
}
