//! Mapping from persisted GUI library entries to the shared runtime boundary.

use colosseum_application::{
    CpuAllocation, EngineLaunchSpec, RuntimeParticipant, UciOptionValue as RuntimeOptionValue,
};
use colosseum_core::{EngineConfig, ParticipantId, UciOptionValue};

/// Losslessly map launch controls while deliberately excluding GUI metadata,
/// detected schemas, saved ratings and library identity from the launch spec.
#[must_use]
pub fn runtime_participant(engine: &EngineConfig) -> RuntimeParticipant {
    let label = match (engine.meta.name.trim(), engine.meta.version.trim()) {
        ("", "") => None,
        (name, "") => Some(name.to_owned()),
        ("", version) => Some(version.to_owned()),
        (name, version) => Some(format!("{name} {version}")),
    };

    RuntimeParticipant {
        // Preserve a lossless correlation for GUI result/writeback mapping,
        // while keeping the identifier out of EngineLaunchSpec itself.
        id: ParticipantId::from_uuid(engine.id.as_uuid()),
        launch: EngineLaunchSpec {
            executable: engine.path.clone(),
            arguments: engine.args.clone(),
            working_directory: engine.working_dir.clone(),
            environment: engine.env.clone(),
            label,
            options: engine
                .options
                .iter()
                .map(|(name, value)| (name.clone(), runtime_option(value)))
                .collect(),
            allocated_cpus: CpuAllocation::Unrestricted,
        },
    }
}

fn runtime_option(value: &UciOptionValue) -> RuntimeOptionValue {
    match value {
        UciOptionValue::Check(value) => RuntimeOptionValue::Check(*value),
        UciOptionValue::Spin(value) => RuntimeOptionValue::Spin(*value),
        UciOptionValue::Combo(value) => RuntimeOptionValue::Combo(value.clone()),
        UciOptionValue::Str(value) => RuntimeOptionValue::String(value.clone()),
        UciOptionValue::Button => RuntimeOptionValue::Button,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use colosseum_core::{EngineConfig, EngineId, EngineMeta, UciOption};

    use super::*;

    #[test]
    fn maps_launch_controls_without_gui_metadata_or_schema() {
        let mut engine = EngineConfig::new(EngineId::from_u128(7), "bin/engine".into());
        engine.meta = EngineMeta {
            name: "Rarog".into(),
            version: "1.0".into(),
            elo: Some(3100),
            extra: BTreeMap::from([("logo".into(), "rarog.png".into())]),
        };
        engine.args = vec!["--uci".into()];
        engine.working_dir = Some("bin".into());
        engine.env.insert("NNUE".into(), "network.nnue".into());
        engine
            .options
            .insert("Hash".into(), UciOptionValue::Spin(128));
        engine.detected_options.push(UciOption::Spin {
            name: "Hash".into(),
            default: 16,
            min: 1,
            max: 4096,
        });

        let runtime = runtime_participant(&engine);
        assert_eq!(runtime.id, ParticipantId::from_u128(7));
        assert_eq!(runtime.launch.label.as_deref(), Some("Rarog 1.0"));
        assert_eq!(runtime.launch.arguments, ["--uci"]);
        assert_eq!(
            runtime.launch.working_directory.as_deref(),
            Some(std::path::Path::new("bin"))
        );
        assert_eq!(runtime.launch.environment["NNUE"], "network.nnue");
        assert_eq!(
            runtime.launch.options["Hash"],
            RuntimeOptionValue::Spin(128)
        );

        let json = serde_json::to_string(&runtime.launch).unwrap();
        for forbidden in ["elo", "logo", "detected_options", "3100", "4096"] {
            assert!(!json.contains(forbidden), "launch spec leaked {forbidden}");
        }
    }

    #[test]
    fn same_executable_with_different_options_remains_distinct() {
        let mut first = EngineConfig::new(EngineId::from_u128(1), "engine".into());
        first
            .options
            .insert("Threads".into(), UciOptionValue::Spin(1));
        let mut second = EngineConfig::new(EngineId::from_u128(2), "engine".into());
        second
            .options
            .insert("Threads".into(), UciOptionValue::Spin(2));

        let first = runtime_participant(&first);
        let second = runtime_participant(&second);
        assert_eq!(first.launch.executable, second.launch.executable);
        assert_ne!(first.id, second.id);
        assert_ne!(first.launch.options, second.launch.options);
    }
}
