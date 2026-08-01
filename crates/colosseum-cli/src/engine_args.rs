use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::PathBuf;

use clap::Args;
use colosseum_application::{CpuAllocation, EngineLaunchSpec, LogicalCpuId, UciOptionValue};
use thiserror::Error;

/// Direct controls for one ordinary UCI executable.
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct EngineArgs {
    /// Path to the UCI engine executable.
    pub executable: PathBuf,

    /// Optional display label; it is not engine identity.
    #[arg(long)]
    pub label: Option<String>,

    /// Argument passed to the engine process; repeat for multiple arguments.
    #[arg(long = "engine-arg", allow_hyphen_values = true)]
    pub arguments: Vec<OsString>,

    /// Working directory used when launching the engine.
    #[arg(long)]
    pub cwd: Option<PathBuf>,

    /// Environment override as KEY=VALUE; repeat for multiple variables.
    #[arg(long = "env", value_name = "KEY=VALUE")]
    pub environment: Vec<String>,

    /// UCI option as NAME=VALUE; repeat for multiple options.
    #[arg(long = "option", value_name = "NAME=VALUE")]
    pub options: Vec<String>,

    /// Trigger a UCI button option by name; repeat for multiple buttons.
    #[arg(long = "button", value_name = "NAME")]
    pub buttons: Vec<String>,

    /// Allocated logical CPUs, for example 0,2-4 or Windows groups 0:0-3,1:0-3.
    #[arg(long, value_name = "LIST")]
    pub cores: Option<String>,
}

impl EngineArgs {
    /// Resolve CLI spellings into the runtime launch boundary. Paths are kept
    /// relative here; the configuration resolver owns path origins in 2.4.
    pub fn resolve(&self) -> Result<EngineLaunchSpec, EngineArgsError> {
        if self.label.as_deref() == Some("") {
            return Err(EngineArgsError::EmptyName {
                kind: "engine label",
            });
        }
        let environment = unique_pairs(&self.environment, "environment")?;
        let mut options = BTreeMap::new();
        for (name, value) in unique_pairs(&self.options, "UCI option")? {
            options.insert(name, UciOptionValue::String(value));
        }
        for name in &self.buttons {
            validate_name(name, "UCI button")?;
            if options
                .insert(name.clone(), UciOptionValue::Button)
                .is_some()
            {
                return Err(EngineArgsError::Duplicate {
                    kind: "UCI option",
                    name: name.clone(),
                });
            }
        }

        let arguments = self
            .arguments
            .iter()
            .map(|argument| {
                argument
                    .clone()
                    .into_string()
                    .map_err(EngineArgsError::NonUtf8Argument)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let allocated_cpus = self.cores.as_deref().map_or_else(
            || Ok(CpuAllocation::Unrestricted),
            |value| parse_cpu_list(value).map(CpuAllocation::Enforced),
        )?;

        Ok(EngineLaunchSpec {
            executable: self.executable.clone(),
            arguments,
            working_directory: self.cwd.clone(),
            environment,
            label: self.label.clone(),
            options,
            allocated_cpus,
        })
    }
}

#[derive(Debug, Error)]
pub enum EngineArgsError {
    #[error("{kind} must use NAME=VALUE syntax")]
    MissingEquals { kind: &'static str },
    #[error("{kind} name must not be empty")]
    EmptyName { kind: &'static str },
    #[error("duplicate {kind} name: {name}")]
    Duplicate { kind: &'static str, name: String },
    #[error("engine argument is not valid UTF-8: {0:?}")]
    NonUtf8Argument(OsString),
    #[error("invalid logical CPU value: {0}")]
    InvalidCore(String),
    #[error("logical CPU range is descending: {0}")]
    DescendingCoreRange(String),
    #[error("logical CPU range is unreasonably large: {0}")]
    ExcessiveCoreRange(String),
    #[error("logical CPU {group}:{number} is allocated more than once")]
    DuplicateCore { group: u16, number: u32 },
    #[error("logical CPU list must not be empty")]
    EmptyCoreList,
}

fn unique_pairs(
    values: &[String],
    kind: &'static str,
) -> Result<BTreeMap<String, String>, EngineArgsError> {
    let mut output = BTreeMap::new();
    for item in values {
        let Some((name, value)) = item.split_once('=') else {
            return Err(EngineArgsError::MissingEquals { kind });
        };
        validate_name(name, kind)?;
        if output.insert(name.to_owned(), value.to_owned()).is_some() {
            return Err(EngineArgsError::Duplicate {
                kind,
                name: name.to_owned(),
            });
        }
    }
    Ok(output)
}

fn validate_name(name: &str, kind: &'static str) -> Result<(), EngineArgsError> {
    if name.is_empty() {
        Err(EngineArgsError::EmptyName { kind })
    } else {
        Ok(())
    }
}

pub fn parse_cpu_list(value: &str) -> Result<Vec<LogicalCpuId>, EngineArgsError> {
    if value.is_empty() {
        return Err(EngineArgsError::EmptyCoreList);
    }
    let mut seen = BTreeSet::new();
    let mut cores = Vec::new();
    for component in value.split(',') {
        if component.is_empty() {
            return Err(EngineArgsError::InvalidCore(component.into()));
        }
        let (group, numbers) = if let Some((group, numbers)) = component.split_once(':') {
            let group = group
                .parse::<u16>()
                .map_err(|_| EngineArgsError::InvalidCore(component.into()))?;
            if numbers.contains(':') {
                return Err(EngineArgsError::InvalidCore(component.into()));
            }
            (group, numbers)
        } else {
            (0, component)
        };
        let (start, end) = if let Some((start, end)) = numbers.split_once('-') {
            let start = core_number(start, component)?;
            let end = core_number(end, component)?;
            if start > end {
                return Err(EngineArgsError::DescendingCoreRange(component.into()));
            }
            if end - start > 65_535 {
                return Err(EngineArgsError::ExcessiveCoreRange(component.into()));
            }
            (start, end)
        } else {
            let core = core_number(numbers, component)?;
            (core, core)
        };
        for number in start..=end {
            let cpu = LogicalCpuId { group, number };
            if !seen.insert(cpu) {
                return Err(EngineArgsError::DuplicateCore { group, number });
            }
            cores.push(cpu);
        }
    }
    Ok(cores)
}

fn core_number(value: &str, component: &str) -> Result<u32, EngineArgsError> {
    value
        .parse()
        .map_err(|_| EngineArgsError::InvalidCore(component.into()))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(flatten)]
        engine: EngineArgs,
    }

    #[test]
    fn resolves_every_direct_engine_control() {
        let parsed = Harness::try_parse_from([
            "test",
            "engines/rarog.exe",
            "--label",
            "Rarog tuned",
            "--engine-arg=--uci",
            "--engine-arg",
            "network.nnue",
            "--cwd",
            "engines",
            "--env",
            "RUST_LOG=warn",
            "--option",
            "Hash=256",
            "--option",
            "EvalFile=net=1.nnue",
            "--button",
            "Clear Hash",
            "--cores",
            "0,0:2-4,1:7",
        ])
        .unwrap();
        let launch = parsed.engine.resolve().unwrap();

        assert_eq!(launch.executable, PathBuf::from("engines/rarog.exe"));
        assert_eq!(launch.label.as_deref(), Some("Rarog tuned"));
        assert_eq!(launch.arguments, ["--uci", "network.nnue"]);
        assert_eq!(launch.working_directory, Some(PathBuf::from("engines")));
        assert_eq!(launch.environment["RUST_LOG"], "warn");
        assert_eq!(
            launch.options["EvalFile"],
            UciOptionValue::String("net=1.nnue".into())
        );
        assert_eq!(launch.options["Clear Hash"], UciOptionValue::Button);
        assert_eq!(
            launch.allocated_cpus,
            CpuAllocation::Enforced(vec![
                LogicalCpuId::from(0),
                LogicalCpuId {
                    group: 0,
                    number: 2
                },
                LogicalCpuId {
                    group: 0,
                    number: 3
                },
                LogicalCpuId {
                    group: 0,
                    number: 4
                },
                LogicalCpuId {
                    group: 1,
                    number: 7
                },
            ])
        );
    }

    #[test]
    fn path_only_is_sufficient() {
        let parsed = Harness::try_parse_from(["test", "engine"]).unwrap();
        assert_eq!(
            parsed.engine.resolve().unwrap(),
            EngineLaunchSpec::path_only("engine".into())
        );
    }

    #[test]
    fn rejects_ambiguous_duplicates_and_bad_core_lists() {
        for arguments in [
            vec!["test", "engine", "--env", "A=1", "--env", "A=2"],
            vec!["test", "engine", "--option", "Hash=1", "--button", "Hash"],
            vec!["test", "engine", "--cores", "4-2"],
            vec!["test", "engine", "--cores", "0,0"],
            vec!["test", "engine", "--cores", "0:0,0"],
            vec!["test", "engine", "--cores", "1:0,1:0"],
            vec!["test", "engine", "--cores", "1:0:2"],
            vec!["test", "engine", "--cores", ""],
        ] {
            let parsed = Harness::try_parse_from(arguments).unwrap();
            assert!(parsed.engine.resolve().is_err());
        }
    }
}
