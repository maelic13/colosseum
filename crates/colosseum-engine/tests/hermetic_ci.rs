//! Regression guard for the required, repository-only test tier.
//!
//! The real-engine targets are intentionally isolated behind Cargo's
//! `real-engine-smoke` feature. These assertions make that boundary visible to
//! the ordinary test suite, so a local-path fallback cannot quietly return.

const ENGINE_MANIFEST: &str = include_str!("../Cargo.toml");
const UCI_MANIFEST: &str = include_str!("../../colosseum-uci/Cargo.toml");
const ENGINE_COMMON: &str = include_str!("common/mod.rs");
const RUNNER_SMOKE: &str = include_str!("runner.rs");
const SCHEDULER_SMOKE: &str = include_str!("scheduler.rs");
const UCI_SMOKE: &str = include_str!("../../colosseum-uci/tests/stockfish.rs");
const CI_WORKFLOW: &str = include_str!("../../../.github/workflows/ci.yml");

fn assert_smoke_target(manifest: &str, name: &str, path: &str) {
    let declaration = format!(
        "[[test]]\nname = \"{name}\"\npath = \"{path}\"\nrequired-features = [\"real-engine-smoke\"]"
    );
    assert!(
        manifest.contains(&declaration),
        "{name} must stay behind the real-engine-smoke feature"
    );
}

#[test]
fn real_engine_smoke_targets_are_excluded_from_required_tests() {
    assert_smoke_target(ENGINE_MANIFEST, "runner_smoke", "tests/runner.rs");
    assert_smoke_target(ENGINE_MANIFEST, "scheduler_smoke", "tests/scheduler.rs");
    assert_smoke_target(UCI_MANIFEST, "uci_smoke", "tests/stockfish.rs");

    assert!(
        CI_WORKFLOW.contains("cargo test --workspace --all-targets"),
        "required CI must run the complete default test tier"
    );
    assert!(
        !CI_WORKFLOW.contains("real-engine-smoke")
            && !CI_WORKFLOW.contains("COLOSSEUM_SMOKE_ENGINE"),
        "required CI must not opt into real-engine smoke tests"
    );
}

#[test]
fn smoke_support_has_no_machine_specific_engine_fallback() {
    for (name, source) in [
        ("engine smoke support", ENGINE_COMMON),
        ("runner smoke tests", RUNNER_SMOKE),
        ("scheduler smoke tests", SCHEDULER_SMOKE),
        ("UCI smoke tests", UCI_SMOKE),
    ] {
        assert!(
            !source.contains("COLOSSEUM_TEST_ENGINE"),
            "{name} still uses the retired implicit environment variable"
        );
        assert!(
            !source.contains(r"D:\chess") && !source.contains("D:/chess"),
            "{name} still contains a developer-machine engine path"
        );
    }

    assert!(ENGINE_COMMON.contains("COLOSSEUM_SMOKE_ENGINE"));
    assert!(UCI_SMOKE.contains("COLOSSEUM_SMOKE_ENGINE"));
}
