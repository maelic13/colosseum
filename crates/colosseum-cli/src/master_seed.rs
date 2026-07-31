use serde::Serialize;
use thiserror::Error;

use crate::{ConfigError, ResolvedConfig};

pub trait MasterSeedEntropy {
    fn generate(&self) -> Result<u64, String>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OsMasterSeedEntropy;

impl MasterSeedEntropy for OsMasterSeedEntropy {
    fn generate(&self) -> Result<u64, String> {
        let mut bytes = [0; 8];
        getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
        Ok(u64::from_le_bytes(bytes))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MasterSeedResolution {
    pub value: u64,
    pub generated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedMasterSeedSource(pub u64);

impl colosseum_application::MasterSeedSource for ResolvedMasterSeedSource {
    fn master_seed(&self) -> Result<u64, colosseum_application::ApplicationError> {
        Ok(self.0)
    }
}

#[derive(Debug, Error)]
pub enum MasterSeedError {
    #[error("configured seed must be an unsigned 64-bit integer")]
    InvalidConfiguredSeed,
    #[error("could not obtain operating-system entropy for the master seed: {0}")]
    Entropy(String),
    #[error(transparent)]
    Config(#[from] ConfigError),
}

/// Keep a supplied seed or generate one and insert it before config identity is
/// recorded. The returned config hash therefore always covers the used seed.
pub fn ensure_master_seed(
    mut config: ResolvedConfig,
    entropy: &dyn MasterSeedEntropy,
) -> Result<(ResolvedConfig, MasterSeedResolution), MasterSeedError> {
    if let Some(seed) = config.seed() {
        let value = seed
            .as_u64()
            .ok_or(MasterSeedError::InvalidConfiguredSeed)?;
        return Ok((
            config,
            MasterSeedResolution {
                value,
                generated: false,
            },
        ));
    }
    let value = entropy.generate().map_err(MasterSeedError::Entropy)?;
    config.insert_generated_seed(value)?;
    Ok((
        config,
        MasterSeedResolution {
            value,
            generated: true,
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::resolve_config;

    struct FixedEntropy(u64);

    impl MasterSeedEntropy for FixedEntropy {
        fn generate(&self) -> Result<u64, String> {
            Ok(self.0)
        }
    }

    #[test]
    fn supplied_seed_is_retained_without_entropy() {
        struct FailingEntropy;
        impl MasterSeedEntropy for FailingEntropy {
            fn generate(&self) -> Result<u64, String> {
                Err("must not be called".into())
            }
        }
        let config = resolve_config(
            json!({}),
            None,
            json!({"seed": 42}),
            &[],
            Path::new("."),
            &[],
        )
        .unwrap();
        let (config, seed) = ensure_master_seed(config, &FailingEntropy).unwrap();
        assert_eq!(seed.value, 42);
        assert!(!seed.generated);
        assert_eq!(config.value()["seed"], 42);
    }

    #[test]
    fn generated_seed_is_inserted_before_hashing_and_recording() {
        let config = resolve_config(
            json!({"schema_version": 1}),
            None,
            json!({}),
            &[],
            Path::new("."),
            &[],
        )
        .unwrap();
        let old_hash = config.sha256().to_owned();
        let (config, seed) = ensure_master_seed(config, &FixedEntropy(u64::MAX)).unwrap();
        assert_eq!(seed.value, u64::MAX);
        assert!(seed.generated);
        assert_eq!(config.value()["seed"], u64::MAX);
        assert_ne!(config.sha256(), old_hash);
        assert!(
            config
                .canonical_json()
                .windows(20)
                .any(|window| { window == u64::MAX.to_string().as_bytes() })
        );
        let output = tempfile::tempdir().unwrap();
        config.write_to(output.path()).unwrap();
        let recorded: serde_json::Value = serde_json::from_slice(
            &std::fs::read(output.path().join("resolved-config.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(recorded["seed"], u64::MAX);
    }

    #[test]
    fn invalid_configured_seed_is_rejected() {
        for value in [json!(-1), json!("42"), json!(1.5)] {
            let config = resolve_config(
                json!({}),
                None,
                json!({"seed": value}),
                &[],
                Path::new("."),
                &[],
            )
            .unwrap();
            assert!(matches!(
                ensure_master_seed(config, &FixedEntropy(1)),
                Err(MasterSeedError::InvalidConfiguredSeed)
            ));
        }
    }
}
