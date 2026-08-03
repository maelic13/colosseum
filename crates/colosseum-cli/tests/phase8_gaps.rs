use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GapFixture {
    schema_version: u32,
    phase: String,
    criterion: String,
    decisions: Vec<GapDecision>,
}

#[derive(Debug, Deserialize)]
struct GapDecision {
    id: String,
    decision: String,
    release: String,
    reason: String,
    scope: String,
    evidence: String,
}

#[test]
fn every_phase_eight_gap_has_one_reasoned_decision() {
    let fixture: GapFixture =
        serde_json::from_str(include_str!("../../../docs/fixtures/phase8/gaps.json")).unwrap();
    assert_eq!(fixture.schema_version, 1);
    assert_eq!(fixture.phase, "8.2");
    assert!(fixture.criterion.contains("general engine developer"));

    let expected = BTreeSet::from([
        "ponder",
        "chess960",
        "harness-syzygy-adjudication",
        "additional-tournament-formats",
        "additional-output-formats",
        "dedicated-datagen-command",
    ]);
    let actual = fixture
        .decisions
        .iter()
        .map(|decision| decision.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), fixture.decisions.len());
    assert!(fixture.decisions.iter().all(|decision| {
        matches!(decision.decision.as_str(), "adopt" | "decline" | "defer")
            && matches!(decision.release.as_str(), "1.0" | "post-1.0")
            && !decision.reason.is_empty()
            && !decision.scope.is_empty()
            && !decision.evidence.is_empty()
    }));

    let ponder = fixture
        .decisions
        .iter()
        .find(|decision| decision.id == "ponder")
        .unwrap();
    assert_eq!(ponder.decision, "adopt");
    assert_eq!(ponder.release, "1.0");
    assert!(ponder.scope.contains("clock controls only"));
}
