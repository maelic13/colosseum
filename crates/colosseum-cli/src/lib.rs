//! Reusable CLI driving-adapter types.

pub mod config;
pub mod engine_args;

pub use config::{ConfigError, ResolvedConfig, ValueOrigin, built_in_defaults, resolve_config};
pub use engine_args::{EngineArgs, EngineArgsError};
