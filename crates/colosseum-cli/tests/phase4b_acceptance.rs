use std::collections::BTreeSet;

use serde_json::Value;

const ACCEPTANCE: &str = include_str!("../../../docs/fixtures/phase4b/acceptance.json");
const SPRT_RUNNER: &str = include_str!("../src/sprt_runner.rs");
const COMMAND_LINE: &str = include_str!("command_line.rs");
const MAIN: &str = include_str!("../src/main.rs");
const STATISTICS: &str = include_str!("../../colosseum-core/tests/statistics_fixtures.rs");

#[test]
fn acceptance_manifest_names_every_phase_4b_exit_gate_and_test_owner() {
    let manifest: Value = serde_json::from_str(ACCEPTANCE).unwrap();
    assert_eq!(manifest["schema_version"], 1);
    assert_eq!(manifest["phase"], "4B");
    let gates = manifest["gates"].as_array().unwrap();
    let ids = gates
        .iter()
        .map(|gate| gate["id"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        ids,
        BTreeSet::from([
            "analytic-statistics-oracle",
            "automation-exit-map",
            "completion-order-invariance",
            "configuration-exit",
            "engine-fault-matrix",
            "external-terminal-replay",
            "finite-cap-inconclusive",
            "h0-h1-terminal-cuts",
            "invalid-and-infrastructure-exits",
            "live-shared-field-parity",
            "workspace-regression",
        ])
    );
    assert_eq!(ids.len(), gates.len(), "duplicate Phase 4B gate ID");
    for gate in gates {
        assert!(!gate["evidence"].as_str().unwrap().trim().is_empty());
    }

    for symbol in [
        "worker_completion_order_cannot_change_either_terminal_sample",
        "boundary_pair_is_official_and_every_later_completion_is_separate",
        "losing_stream_crosses_h0",
        "strict_sprt_fault_policy_covers_every_engine_fault_and_rejects_infrastructure",
    ] {
        assert!(SPRT_RUNNER.contains(symbol), "missing SPRT test {symbol}");
    }
    for symbol in [
        "sprt_live_report_is_pair_atomic_durable_and_inconclusive_at_its_cap",
        "sprt_live_exit_distinguishes_invalid_and_infrastructure_error",
        "sprt_refuses_missing_or_invalid_design_before_launch",
    ] {
        assert!(COMMAND_LINE.contains(symbol), "missing CLI test {symbol}");
    }
    assert!(MAIN.contains("sprt_terminal_classes_have_distinct_automation_exit_codes"));
    for symbol in [
        "analytic_fixture_covers_every_phase_one_statistics_contract",
        "phase_4b_ordered_fastchess_stream_has_the_same_terminal_pair",
        "phase_4b_controlled_live_parity_compares_only_shared_fields",
    ] {
        assert!(STATISTICS.contains(symbol), "missing oracle test {symbol}");
    }
}
