use std::collections::BTreeSet;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Acceptance {
    schema_version: u32,
    phase: String,
    gates: Vec<Gate>,
}

#[derive(Debug, Deserialize)]
struct Gate {
    id: String,
    evidence: String,
}

#[test]
fn acceptance_manifest_names_every_phase_five_exit_gate_and_test_owner() {
    let acceptance: Acceptance = serde_json::from_str(include_str!(
        "../../../docs/fixtures/phase5/acceptance.json"
    ))
    .unwrap();
    assert_eq!(acceptance.schema_version, 1);
    assert_eq!(acceptance.phase, "5");
    let expected = BTreeSet::from([
        "schedule-rng-rounding-properties",
        "written-schedule-preflight",
        "hard-audit-matrix",
        "pair-atomic-fault-policy",
        "exact-kill-resume",
        "synthetic-convergence",
        "plan-arithmetic-and-timing",
        "status-diagnostics-and-read-only-live-snapshot",
        "unedited-result-to-verified-gate",
        "workspace-regression",
    ]);
    let actual = acceptance
        .gates
        .iter()
        .map(|gate| gate.id.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected);
    assert!(
        acceptance
            .gates
            .iter()
            .all(|gate| !gate.evidence.trim().is_empty())
    );
}
