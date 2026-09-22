//! Strict TOML adapter for the ordered SPSA parameter vector.

use std::fs;
use std::path::{Path, PathBuf};

use colosseum_application::SpsaTune;
use thiserror::Error;

/// Parse a tune file without launching an engine. Live UCI-schema binding is an
/// application use case because the parsed TOML is only a requested vector.
pub fn load_spsa_tune(path: &Path) -> Result<SpsaTune, SpsaTuneFileError> {
    let text = fs::read_to_string(path).map_err(|source| SpsaTuneFileError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    toml::from_str(&text).map_err(|source| SpsaTuneFileError::Parse {
        path: path.to_path_buf(),
        source: Box::new(source),
    })
}

#[derive(Debug, Error)]
pub enum SpsaTuneFileError {
    #[error("could not read SPSA tune file {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not parse SPSA tune file {path}: {source}")]
    Parse {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
}

#[cfg(test)]
mod tests {
    use colosseum_application::{EngineInspection, UciOptionSchema};

    use super::*;

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
}
