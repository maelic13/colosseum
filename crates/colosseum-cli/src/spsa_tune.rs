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
