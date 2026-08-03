use colosseum_application::{
    EngineInspection, EngineLaunchSpec, InspectEngine, RuntimeParticipant, SpsaTuneError,
    UciOptionSchema,
};
use colosseum_cli::{SpsaTuneFileError, load_spsa_tune};
use colosseum_core::ParticipantId;
use colosseum_uci::UciSessionFactory;

#[test]
fn strict_tune_toml_preserves_the_declared_parameter_order() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tune.toml");
    std::fs::write(
        &path,
        r#"
[[parameters]]
name = "Reduction"
initial = 12
min = 0
max = 64
c_end = 0.5

[[parameters]]
name = "Aspiration"
initial = 20
min = 1
max = 128
c_end = 1.25
"#,
    )
    .unwrap();

    let tune = load_spsa_tune(&path).unwrap();
    assert_eq!(
        tune.parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["Reduction", "Aspiration"]
    );
    let bound = tune
        .bind_live_schema(&EngineInspection {
            name: Some("fixture".into()),
            author: None,
            options: vec![
                UciOptionSchema::Spin {
                    name: "Reduction".into(),
                    default: 8,
                    min: 0,
                    max: 128,
                },
                UciOptionSchema::Spin {
                    name: "Aspiration".into(),
                    default: 16,
                    min: 0,
                    max: 256,
                },
            ],
            diagnostics: Vec::new(),
        })
        .unwrap();
    assert_eq!(bound.initial_centers(), [12.0, 20.0]);
    assert_eq!(bound.parameters[1].advertised.default, 16);
}

#[test]
fn malformed_or_unknown_tune_fields_are_rejected_before_schema_binding() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("bad.toml");
    std::fs::write(
        &path,
        r#"
[[parameters]]
name = "Reduction"
initial = 12
min = 0
max = 64
c_end = 0.5
unexpected = true
"#,
    )
    .unwrap();
    assert!(matches!(
        load_spsa_tune(&path),
        Err(SpsaTuneFileError::Parse { .. })
    ));
}

#[test]
fn parsed_tune_must_bind_to_live_spin_options() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tune.toml");
    std::fs::write(
        &path,
        r#"
[[parameters]]
name = "Use NNUE"
initial = 1
min = 0
max = 2
c_end = 0.5
"#,
    )
    .unwrap();
    let tune = load_spsa_tune(&path).unwrap();
    let inspection = EngineInspection {
        name: None,
        author: None,
        options: vec![UciOptionSchema::Check {
            name: "Use NNUE".into(),
            default: true,
        }],
        diagnostics: Vec::new(),
    };
    assert_eq!(
        tune.bind_live_schema(&inspection),
        Err(SpsaTuneError::OptionIsNotSpin {
            name: "Use NNUE".into(),
            advertised_kind: "check"
        })
    );
}

#[test]
fn ordinary_uci_handshake_is_the_live_authority_for_tune_option_binding() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("tune.toml");
    std::fs::write(
        &path,
        r#"
[[parameters]]
name = "Hash"
initial = 16
min = 1
max = 1024
c_end = 1.0
"#,
    )
    .unwrap();
    let participant = RuntimeParticipant {
        id: ParticipantId::from_u128(53),
        launch: EngineLaunchSpec::path_only(std::path::PathBuf::from(env!(
            "CARGO_BIN_EXE_colosseum-uci-fixture"
        ))),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let inspection = runtime
        .block_on(InspectEngine::execute(&UciSessionFactory, &participant))
        .unwrap();
    let bound = load_spsa_tune(&path)
        .unwrap()
        .bind_live_schema(&inspection)
        .unwrap();
    assert_eq!(bound.parameters[0].advertised.name, "Hash");
    assert_eq!(bound.parameters[0].advertised.min, 1);
    assert_eq!(bound.parameters[0].advertised.max, 1024);
}
