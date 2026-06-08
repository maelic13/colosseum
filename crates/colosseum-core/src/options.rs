//! UCI option *schema* (as auto-detected from an engine) and *values* (as set by
//! the user). The schema drives which editor widget the GUI shows per option.

use serde::{Deserialize, Serialize};

/// A UCI option declaration parsed from an engine's `option name ...` handshake line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UciOption {
    /// Boolean toggle.
    Check { name: String, default: bool },
    /// Integer within an inclusive `[min, max]` range.
    Spin {
        name: String,
        default: i64,
        min: i64,
        max: i64,
    },
    /// One of a fixed set of string values.
    Combo {
        name: String,
        default: String,
        vars: Vec<String>,
    },
    /// A triggerable action (no stored value).
    Button { name: String },
    /// Free-form string (e.g. a path).
    Str { name: String, default: String },
}

impl UciOption {
    /// The option's name as advertised by the engine.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Check { name, .. }
            | Self::Spin { name, .. }
            | Self::Combo { name, .. }
            | Self::Button { name }
            | Self::Str { name, .. } => name,
        }
    }

    /// The default value for this option as a [`UciOptionValue`], if it has one.
    #[must_use]
    pub fn default_value(&self) -> Option<UciOptionValue> {
        match self {
            Self::Check { default, .. } => Some(UciOptionValue::Check(*default)),
            Self::Spin { default, .. } => Some(UciOptionValue::Spin(*default)),
            Self::Combo { default, .. } => Some(UciOptionValue::Combo(default.clone())),
            Self::Str { default, .. } => Some(UciOptionValue::Str(default.clone())),
            Self::Button { .. } => None,
        }
    }
}

/// A concrete value the user has chosen for a UCI option, sent via `setoption`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UciOptionValue {
    Check(bool),
    Spin(i64),
    Combo(String),
    Str(String),
}

impl UciOptionValue {
    /// Render as the string the engine expects after `setoption name <n> value `.
    #[must_use]
    pub fn as_uci_string(&self) -> String {
        match self {
            Self::Check(b) => b.to_string(),
            Self::Spin(i) => i.to_string(),
            Self::Combo(s) | Self::Str(s) => s.clone(),
        }
    }
}
