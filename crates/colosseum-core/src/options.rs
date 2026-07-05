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

/// Known option names (whitespace-insensitive, case-insensitive) that set an
/// engine's thread/CPU **count**. The tournament-wide thread setting is
/// forwarded to whichever of these an engine declares.
///
/// This is a deliberate **allowlist**, not a substring heuristic. A
/// `contains("cpu"/"core"/"thread")` test wrongly matched, in a real 33-engine
/// library, thirteen distinct options where only two were thread counts —
/// including Rybka's "CPU Usage" (a 1–100 % throttle), Rybka's "Score Offset
/// millipawns" and some engines' "ThreadIdlingThreshold" (both numeric Spins
/// that would be silently corrupted), Lc0's "CPuct*" params, and anything
/// containing "Score" (which contains "core"). Setting "CPU Usage" to a thread
/// count of 1 pinned those engines to 1 % CPU (~40× slowdown). An unrecognised
/// name is safely left at the engine's default; it can be set per-engine or
/// added here. New names are cheap to add and misses are visible (the engine
/// simply runs at its default thread count).
const THREAD_COUNT_OPTIONS: &[&str] = &[
    "threads",
    "maxcpus",
    "cpus",
    "cores",
    "maxthreads",
    "numberofthreads",
    "numthreads",
    "corethreads",
];

/// Normalise an option name for matching: strip whitespace, lowercase.
fn normalize_option_name(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

/// True when `name` is one of the recognised thread/CPU-count options.
#[must_use]
pub fn is_thread_option(name: &str) -> bool {
    let n = normalize_option_name(name);
    THREAD_COUNT_OPTIONS.contains(&n.as_str())
}

/// True when `name` is an engine's main transposition-table size option
/// ("Hash", "Hash Size", "Memory"…). Deliberately exact-ish so hash-adjacent
/// options ("Clear Hash", "Hash File") don't match.
#[must_use]
pub fn is_hash_option(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "hash" || n == "hash size" || n == "hashsize" || n == "memory"
}

/// A concrete value the user has chosen for a UCI option, sent via `setoption`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UciOptionValue {
    Check(bool),
    Spin(i64),
    Combo(String),
    Str(String),
    /// Marks a Button option as triggered; sends `setoption name X` (no value) at game start.
    Button,
}

impl UciOptionValue {
    /// Render as the string the engine expects after `setoption name <n> value `.
    /// Panics if called on `Button` — callers should special-case that variant.
    #[must_use]
    pub fn as_uci_string(&self) -> String {
        match self {
            Self::Check(b) => b.to_string(),
            Self::Spin(i) => i.to_string(),
            Self::Combo(s) | Self::Str(s) => s.clone(),
            Self::Button => panic!("Button options have no value string"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_option_matches_real_count_options() {
        // Every spelling a real engine uses, in mixed case / spacing.
        for name in [
            "Threads",
            "Max CPUs",
            "MaxCPUs",
            "CPUs",
            "Cores",
            "Max Threads",
            "Number of Threads",
            "NumThreads",
        ] {
            assert!(is_thread_option(name), "{name} should match");
        }
    }

    #[test]
    fn thread_option_rejects_the_real_library_false_positives() {
        // These all matched the old `contains(...)` heuristic in a real
        // 33-engine library but are NOT thread counts. Several are numeric
        // Spins that would have been silently corrupted to "1".
        for name in [
            "CPU Usage",              // Rybka: a 1–100 % throttle (Spin)
            "Score Offset millipawns", // Rybka 4.1: "sCOREoffset" (Spin)
            "ThreadIdlingThreshold",   // SMP idling knob (Spin)
            "Busy Threads",            // HIARCS: boolean toggle
            "CPuct",                   // Lc0 search param
            "CPuctBase",
            "CPuctFactor",
            "Always Score Main Move",  // has "core" via "Score"
            "DrawScore",
            "ScoreType",
            "Resolve Score Drops",
        ] {
            assert!(!is_thread_option(name), "{name} must NOT match");
        }
    }

    #[test]
    fn hash_option_is_exact_ish() {
        assert!(is_hash_option("Hash"));
        assert!(is_hash_option("Hash Size"));
        assert!(is_hash_option("Memory"));
        assert!(!is_hash_option("Clear Hash"));
        assert!(!is_hash_option("Hash File"));
    }
}
