use colosseum_application::SpsaPreflightError;
use colosseum_cli::{
    RunDirectory, SPSA_SCHEDULE_FILE, SpsaScheduleStoreError, built_in_defaults,
    persist_and_verify_spsa_schedule, resolve_config,
};
use colosseum_core::{SpsaEndSpec, SpsaScheduleArtifact};
use serde_json::{Value, json};

fn schedule(seed: u64) -> SpsaScheduleArtifact {
    SpsaScheduleArtifact::derive(
        5_000,
        0.002,
        seed,
        &[
            SpsaEndSpec {
                name: "Aspiration".into(),
                min: 1,
                max: 500,
                c_end: 0.75,
            },
            SpsaEndSpec {
                name: "Reduction".into(),
                min: -100,
                max: 100,
                c_end: 2.5,
            },
        ],
    )
    .unwrap()
}

fn run(root: &std::path::Path) -> RunDirectory {
    let config = resolve_config(
        built_in_defaults(),
        None,
        json!({"command": "spsa", "seed": 7}),
        &[],
        root,
        &[],
    )
    .unwrap();
    RunDirectory::open_explicit(&root.join("run"), &config, false)
        .unwrap()
        .directory
}

#[test]
fn schedule_is_persisted_with_the_complete_reproducibility_contract() {
    let root = tempfile::tempdir().unwrap();
    let run = run(root.path());
    let expected = schedule(7);
    let verified = persist_and_verify_spsa_schedule(&run, &expected).unwrap();
    assert_eq!(verified.artifact(), &expected);

    let value: Value =
        serde_json::from_slice(&std::fs::read(run.paths().root.join(SPSA_SCHEDULE_FILE)).unwrap())
            .unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["stats_version"], 1);
    assert_eq!(value["schedule"]["iterations"], 5_000);
    assert_eq!(value["r_end"], 0.002);
    assert_eq!(
        value["perturbations"]["algorithm"],
        "chacha12-64-bit-counter-zero-stream-v1"
    );
    assert_eq!(value["perturbations"]["stream_name"], "spsa-perturbations");
    assert_eq!(
        value["perturbations"]["draw_order"],
        "iteration-major-knob-order"
    );
    assert_eq!(value["perturbations"]["master_seed"], 7);
    assert_eq!(value["knobs"][0]["name"], "Aspiration");
}

#[test]
fn a_mutated_written_schedule_cannot_produce_the_preflight_launch_token() {
    let root = tempfile::tempdir().unwrap();
    let run = run(root.path());
    let expected = schedule(7);
    persist_and_verify_spsa_schedule(&run, &expected).unwrap();
    let path = run.paths().root.join(SPSA_SCHEDULE_FILE);
    let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["knobs"][0]["c0"] = json!(123.0);
    std::fs::write(&path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();

    let mut games_launched = 0;
    let result = persist_and_verify_spsa_schedule(&run, &expected);
    if result.is_ok() {
        games_launched += 1;
    }
    assert_eq!(games_launched, 0);
    assert!(matches!(
        result,
        Err(SpsaScheduleStoreError::Preflight(
            SpsaPreflightError::InvalidSchedule(_)
        ))
    ));
    let still_mutated: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(still_mutated["knobs"][0]["c0"], 123.0);
}

#[test]
fn resume_refuses_a_valid_schedule_derived_from_different_inputs() {
    let root = tempfile::tempdir().unwrap();
    let run = run(root.path());
    persist_and_verify_spsa_schedule(&run, &schedule(7)).unwrap();
    assert!(matches!(
        persist_and_verify_spsa_schedule(&run, &schedule(8)),
        Err(SpsaScheduleStoreError::Preflight(
            SpsaPreflightError::WrittenScheduleMismatch
        ))
    ));
}
