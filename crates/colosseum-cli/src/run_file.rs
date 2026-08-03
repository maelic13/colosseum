//! Expand optional inheritable TOML run files into the ordinary Clap surface.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use thiserror::Error;

use crate::{ValueOrigin, resolve_config};

const RUN_FILE_FLAG: &str = "--run-file";
const UNSET_FLAG: &str = "--unset-run-option";

#[derive(Debug, Error)]
pub enum RunFileInvocationError {
    #[error("{RUN_FILE_FLAG} requires a path")]
    MissingPath,
    #[error("{RUN_FILE_FLAG} may be supplied only once")]
    DuplicatePath,
    #[error("{UNSET_FLAG} requires an option long name")]
    MissingUnset,
    #[error("could not determine the invocation directory: {0}")]
    CurrentDirectory(std::io::Error),
    #[error(transparent)]
    Resolution(#[from] crate::ConfigError),
    #[error(
        "run file contains unexpected top-level key {0:?}; only `command`, `positionals` and `options` are allowed after inheritance"
    )]
    UnknownTopLevel(String),
    #[error("run-file `command` must be a non-empty string array")]
    InvalidCommand,
    #[error("run-file `positionals` must be an array of strings")]
    InvalidPositionals,
    #[error("run-file `options` must be a table")]
    InvalidOptions,
    #[error(
        "run-file option {name:?} must not start with `-` and may contain only ASCII letters, digits and hyphens"
    )]
    InvalidOptionName { name: String },
    #[error(
        "run-file option {name:?} must be a string, number, boolean or an array of those scalar values"
    )]
    InvalidOptionValue { name: String },
    #[error("run file and command line must provide a command")]
    MissingCommand,
    #[error("run-file path option {option:?} must be a string or string array")]
    InvalidPathOption { option: String },
    #[error("indexed run-file path {value:?} for --engine-cwd must use INDEX:PATH")]
    InvalidIndexedPath { value: String },
}

/// Expand `--run-file` into normal arguments before the real Clap parse.
pub fn expand_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Vec<OsString>, RunFileInvocationError> {
    let original = arguments.into_iter().collect::<Vec<_>>();
    let Some(executable) = original.first().cloned() else {
        return Ok(original);
    };
    let scan = scan_controls(&original[1..])?;
    let Some(run_file) = scan.run_file else {
        return Ok(original);
    };

    let current_directory =
        std::env::current_dir().map_err(RunFileInvocationError::CurrentDirectory)?;
    let first = resolve_run_file(&run_file, &current_directory, &[], &[])?;
    validate_top_level(first.value())?;

    let explicit_flags = explicit_long_names(&original[1..]);
    let mut unsets = scan
        .unsets
        .iter()
        .map(|name| option_pointer(name))
        .collect::<Vec<_>>();
    if let Some(options) = first.value().get("options").and_then(Value::as_object) {
        for name in options.keys() {
            if explicit_flags.contains(name) {
                unsets.push(option_pointer(name));
            }
        }
    }
    unsets.sort();
    unsets.dedup();

    let preliminary = resolve_run_file(&run_file, &current_directory, &unsets, &[])?;
    validate_top_level(preliminary.value())?;
    let paths = path_pointers(preliminary.value())?;
    let resolved = resolve_run_file(&run_file, &current_directory, &unsets, &paths)?;

    let explicit_command = has_explicit_command(&original[1..]);
    let mut expanded = vec![executable];
    if explicit_command {
        expanded.extend(original.into_iter().skip(1));
        emit_options(&resolved, &mut expanded)?;
    } else {
        let command = command_tokens(resolved.value())?;
        if command.is_empty() {
            return Err(RunFileInvocationError::MissingCommand);
        }
        expanded.extend(command.into_iter().map(OsString::from));
        expanded.extend(
            positionals(resolved.value())?
                .into_iter()
                .map(OsString::from),
        );
        emit_options(&resolved, &mut expanded)?;
        expanded.extend(original.into_iter().skip(1));
    }
    Ok(expanded)
}

fn resolve_run_file(
    run_file: &Path,
    current_directory: &Path,
    unsets: &[String],
    path_pointers: &[String],
) -> Result<crate::ResolvedConfig, crate::ConfigError> {
    resolve_config(
        json!({}),
        Some(run_file),
        json!({}),
        unsets,
        current_directory,
        path_pointers,
    )
}

#[derive(Debug)]
struct ControlScan {
    run_file: Option<PathBuf>,
    unsets: Vec<String>,
}

fn scan_controls(arguments: &[OsString]) -> Result<ControlScan, RunFileInvocationError> {
    let mut run_file = None;
    let mut unsets = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let text = arguments[index].to_string_lossy();
        if text == RUN_FILE_FLAG {
            let value = arguments
                .get(index + 1)
                .ok_or(RunFileInvocationError::MissingPath)?;
            if run_file.replace(PathBuf::from(value)).is_some() {
                return Err(RunFileInvocationError::DuplicatePath);
            }
            index += 2;
            continue;
        }
        if let Some(value) = text.strip_prefix("--run-file=") {
            if value.is_empty() {
                return Err(RunFileInvocationError::MissingPath);
            }
            if run_file.replace(PathBuf::from(value)).is_some() {
                return Err(RunFileInvocationError::DuplicatePath);
            }
            index += 1;
            continue;
        }
        if text == UNSET_FLAG {
            let value = arguments
                .get(index + 1)
                .ok_or(RunFileInvocationError::MissingUnset)?;
            unsets.push(normalize_option_name(&value.to_string_lossy())?);
            index += 2;
            continue;
        }
        if let Some(value) = text.strip_prefix("--unset-run-option=") {
            if value.is_empty() {
                return Err(RunFileInvocationError::MissingUnset);
            }
            unsets.push(normalize_option_name(value)?);
        }
        index += 1;
    }
    Ok(ControlScan { run_file, unsets })
}

fn validate_top_level(value: &Value) -> Result<(), RunFileInvocationError> {
    let object = value
        .as_object()
        .ok_or_else(|| RunFileInvocationError::UnknownTopLevel("<non-table>".into()))?;
    for key in object.keys() {
        if !matches!(key.as_str(), "command" | "positionals" | "options") {
            return Err(RunFileInvocationError::UnknownTopLevel(key.clone()));
        }
    }
    if let Some(command) = object.get("command") {
        let command = command
            .as_array()
            .ok_or(RunFileInvocationError::InvalidCommand)?;
        if command.is_empty() || command.iter().any(|value| value.as_str().is_none()) {
            return Err(RunFileInvocationError::InvalidCommand);
        }
    }
    positionals(value)?;
    if let Some(options) = object.get("options") {
        let options = options
            .as_object()
            .ok_or(RunFileInvocationError::InvalidOptions)?;
        for (name, value) in options {
            normalize_option_name(name)?;
            validate_option_value(name, value)?;
        }
    }
    Ok(())
}

fn normalize_option_name(name: &str) -> Result<String, RunFileInvocationError> {
    let normalized = name.trim().to_owned();
    if normalized.is_empty()
        || normalized != name
        || normalized.starts_with('-')
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(RunFileInvocationError::InvalidOptionName {
            name: name.to_owned(),
        });
    }
    Ok(normalized)
}

fn validate_option_value(name: &str, value: &Value) -> Result<(), RunFileInvocationError> {
    let scalar = |value: &Value| value.is_string() || value.is_number() || value.is_boolean();
    if scalar(value)
        || value
            .as_array()
            .is_some_and(|values| values.iter().all(scalar))
    {
        Ok(())
    } else {
        Err(RunFileInvocationError::InvalidOptionValue {
            name: name.to_owned(),
        })
    }
}

fn command_tokens(value: &Value) -> Result<Vec<String>, RunFileInvocationError> {
    match value.get("command") {
        None => Ok(Vec::new()),
        Some(command) => command
            .as_array()
            .ok_or(RunFileInvocationError::InvalidCommand)?
            .iter()
            .map(|token| {
                token
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(RunFileInvocationError::InvalidCommand)
            })
            .collect(),
    }
}

fn positionals(value: &Value) -> Result<Vec<String>, RunFileInvocationError> {
    match value.get("positionals") {
        None => Ok(Vec::new()),
        Some(positionals) => positionals
            .as_array()
            .ok_or(RunFileInvocationError::InvalidPositionals)?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or(RunFileInvocationError::InvalidPositionals)
            })
            .collect(),
    }
}

fn emit_options(
    resolved: &crate::ResolvedConfig,
    output: &mut Vec<OsString>,
) -> Result<(), RunFileInvocationError> {
    let Some(options) = resolved.value().get("options") else {
        return Ok(());
    };
    let options = options
        .as_object()
        .ok_or(RunFileInvocationError::InvalidOptions)?;
    for (name, value) in options {
        let flag = format!("--{name}");
        match value {
            Value::Bool(true) => output.push(OsString::from(flag)),
            Value::Bool(false) => {}
            Value::Array(values) => {
                for (index, value) in values.iter().enumerate() {
                    output.push(OsString::from(&flag));
                    output.push(OsString::from(option_scalar(
                        resolved,
                        name,
                        value,
                        Some(index),
                    )?));
                }
            }
            value => {
                output.push(OsString::from(flag));
                output.push(OsString::from(option_scalar(resolved, name, value, None)?));
            }
        }
    }
    Ok(())
}

fn option_scalar(
    resolved: &crate::ResolvedConfig,
    name: &str,
    value: &Value,
    index: Option<usize>,
) -> Result<String, RunFileInvocationError> {
    let pointer = match index {
        Some(index) => format!("/options/{name}/{index}"),
        None => format!("/options/{name}"),
    };
    if name == "engine-cwd" {
        let raw = value
            .as_str()
            .ok_or_else(|| RunFileInvocationError::InvalidPathOption {
                option: name.to_owned(),
            })?;
        let (engine, path) =
            raw.split_once(':')
                .ok_or_else(|| RunFileInvocationError::InvalidIndexedPath {
                    value: raw.to_owned(),
                })?;
        let resolved_path = resolve_relative_to_origin(resolved, &pointer, Path::new(path))?;
        return Ok(format!("{engine}:{}", resolved_path.display()));
    }
    scalar_text(value).ok_or_else(|| RunFileInvocationError::InvalidOptionValue {
        name: name.to_owned(),
    })
}

fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

fn explicit_long_names(arguments: &[OsString]) -> BTreeSet<String> {
    arguments
        .iter()
        .filter_map(|argument| argument.to_str())
        .filter_map(|argument| argument.strip_prefix("--"))
        .map(|argument| argument.split_once('=').map_or(argument, |(name, _)| name))
        .filter(|name| !matches!(*name, "run-file" | "unset-run-option"))
        .map(str::to_owned)
        .collect()
}

fn has_explicit_command(arguments: &[OsString]) -> bool {
    const COMMANDS: &[&str] = &[
        "book",
        "calibrate",
        "capabilities",
        "engine",
        "gauntlet",
        "match",
        "nps",
        "self-test",
        "sprt",
        "spsa",
        "stats",
        "status",
        "suite",
        "tournament",
    ];
    arguments.iter().any(|argument| {
        argument
            .to_str()
            .is_some_and(|argument| COMMANDS.contains(&argument))
    })
}

fn option_pointer(name: &str) -> String {
    format!("/options/{}", name.replace('~', "~0").replace('/', "~1"))
}

fn path_pointers(value: &Value) -> Result<Vec<String>, RunFileInvocationError> {
    let command = command_tokens(value)?;
    let mut pointers = positional_path_indices(&command)
        .into_iter()
        .filter(|index| {
            value
                .get("positionals")
                .and_then(Value::as_array)
                .is_some_and(|values| *index < values.len())
        })
        .map(|index| format!("/positionals/{index}"))
        .collect::<Vec<_>>();
    if let Some(options) = value.get("options").and_then(Value::as_object) {
        for (name, option) in options {
            if path_option_names().contains(name.as_str()) {
                match option {
                    Value::String(_) => pointers.push(option_pointer(name)),
                    Value::Array(values) if values.iter().all(Value::is_string) => {
                        pointers.extend(
                            values
                                .iter()
                                .enumerate()
                                .map(|(index, _)| format!("{}/{index}", option_pointer(name))),
                        );
                    }
                    _ => {
                        return Err(RunFileInvocationError::InvalidPathOption {
                            option: name.clone(),
                        });
                    }
                }
            }
        }
    }
    Ok(pointers)
}

fn path_option_names() -> BTreeSet<&'static str> {
    [
        "a-build",
        "a-cwd",
        "against",
        "apply",
        "apply-executable",
        "b-build",
        "b-cwd",
        "baseline",
        "book",
        "cwd",
        "dir",
        "engine",
        "executable",
        "tune",
    ]
    .into_iter()
    .collect()
}

fn positional_path_indices(command: &[String]) -> Vec<usize> {
    match command
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["capabilities"] | ["self-test"] => vec![],
        ["match"] | ["calibrate"] | ["sprt"] => vec![0, 1],
        ["spsa"] | ["nps"] | ["engine", "inspect" | "check"] => vec![0],
        ["spsa", "status"] | ["status"] | ["stats"] => vec![0],
        ["book", "slice"] => vec![0, 1],
        ["book", "hash" | "stats" | "verify"] => vec![0],
        ["suite"] => vec![0, 1],
        _ => vec![],
    }
}

fn resolve_relative_to_origin(
    resolved: &crate::ResolvedConfig,
    pointer: &str,
    path: &Path,
) -> Result<PathBuf, RunFileInvocationError> {
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    match resolved.origins().get(pointer) {
        Some(ValueOrigin::RunFile { file }) => Ok(file
            .parent()
            .expect("canonical run file has a parent")
            .join(path)),
        Some(ValueOrigin::CommandLine { directory }) => Ok(directory.join(path)),
        Some(ValueOrigin::BuiltIn | ValueOrigin::Generated) | None => {
            Err(RunFileInvocationError::InvalidPathOption {
                option: "engine-cwd".into(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn expands_inherited_options_and_cli_replaces_repeated_values() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base.toml");
        let run = root.path().join("gate.toml");
        fs::write(
            &base,
            "[options]\ngames = 100\noption = [\"Hash=64\", \"Threads=1\"]\n",
        )
        .unwrap();
        fs::write(
            &run,
            "extend = \"base.toml\"\ncommand = [\"match\"]\npositionals = [\"candidate\", \"baseline\"]\n[options]\nmovetime-ms = 100\n",
        )
        .unwrap();

        let expanded = expand_arguments([
            OsString::from("colosseum-cli"),
            OsString::from("--run-file"),
            run.into_os_string(),
            OsString::from("--option"),
            OsString::from("Hash=128"),
        ])
        .unwrap();
        let text = expanded
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(text[1], "match");
        assert!(text[2].ends_with("candidate"));
        assert!(text[3].ends_with("baseline"));
        assert!(text.windows(2).any(|pair| pair == ["--games", "100"]));
        assert!(text.windows(2).any(|pair| pair == ["--movetime-ms", "100"]));
        assert!(!text.iter().any(|value| value == "Hash=64"));
        assert!(text.iter().any(|value| value == "Hash=128"));
    }

    #[test]
    fn explicit_command_uses_shared_options_without_file_positionals() {
        let root = tempfile::tempdir().unwrap();
        let run = root.path().join("common.toml");
        fs::write(&run, "[options]\nmovetime-ms = 100\n").unwrap();
        let expanded = expand_arguments([
            OsString::from("colosseum-cli"),
            OsString::from("--run-file"),
            run.into_os_string(),
            OsString::from("match"),
            OsString::from("candidate"),
            OsString::from("baseline"),
            OsString::from("--games"),
            OsString::from("20"),
        ])
        .unwrap();
        let text = expanded
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(text.windows(2).any(|pair| pair == ["--movetime-ms", "100"]));
    }
}
