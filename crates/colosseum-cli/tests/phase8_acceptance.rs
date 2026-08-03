use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Acceptance {
    schema_version: u32,
    phase: String,
    accepted_on: String,
    gates: Vec<Gate>,
}

#[derive(Debug, Deserialize)]
struct Gate {
    id: String,
    evidence: String,
    owner: String,
}

#[test]
fn acceptance_manifest_names_every_phase_eight_exit_gate_and_owner() {
    let acceptance: Acceptance = serde_json::from_str(include_str!(
        "../../../docs/fixtures/phase8/acceptance.json"
    ))
    .unwrap();
    assert_eq!(acceptance.schema_version, 1);
    assert_eq!(acceptance.phase, "8");
    assert_eq!(acceptance.accepted_on, "2026-08-03");

    let expected = BTreeSet::from([
        "current-external-runners",
        "exact-candidate-identity",
        "shared-field-parity",
        "reasoned-divergences",
        "complete-gap-decisions",
        "adopted-ponder-protocol",
        "workspace-regression",
    ]);
    let actual = acceptance
        .gates
        .iter()
        .map(|gate| gate.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), acceptance.gates.len());
    assert!(
        acceptance
            .gates
            .iter()
            .all(|gate| !gate.evidence.is_empty() && !gate.owner.is_empty())
    );
}
