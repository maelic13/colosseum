//! Expand optional inheritable TOML run files into the ordinary Clap surface.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::{Value, json};
use thiserror::Error;

use crate::{ValueOrigin, resolve_config};

const RUN_FILE_FLAG: &str = "--run-file";
const UNSET_FLAG: &str = "--unset-run-option";

/// Run-file options that the command line replaces when it gives their
/// alternative: `(run-file option, command-line flag)`. The two state one
/// quantity two ways and the parser refuses both, so a run file's horizon in
/// iterations yields to a budget in games given for this invocation, and the
/// other way round, as a repeated option does.
const ALTERNATIVE_OPTIONS: &[(&str, &str)] =
    &[("iterations", "total-games"), ("total-games", "iterations")];

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

/// A run file's list option that the command line replaced, losing entries
/// the run file named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacedRunFileList {
    /// The option's long name, without the leading dashes.
    pub option: String,
    /// The run file's entries the command line no longer names, in run-file
    /// order.
    pub dropped: Vec<String>,
}

impl ReplacedRunFileList {
    /// The warning an operator reads, and the run record keeps.
    #[must_use]
    pub fn message(&self) -> String {
        format!(
            "--{} on the command line replaces the run file's list and drops {}; repeat them on the command line to keep them",
            self.option,
            self.dropped.join(", ")
        )
    }
}

/// What this invocation's command line replaced in its run file, set once
/// when the arguments are expanded.
static REPLACED_LISTS: OnceLock<Vec<ReplacedRunFileList>> = OnceLock::new();

/// The run-file lists this invocation's command line replaced; empty without
/// a run file or before the arguments were expanded.
pub fn replaced_run_file_lists() -> &'static [ReplacedRunFileList] {
    REPLACED_LISTS.get().map_or(&[], Vec::as_slice)
}

/// Expand `--run-file` into normal arguments before the real Clap parse, and
/// warn on standard error about each run-file list the command line replaced
/// at the cost of entries it named. The warnings are also kept for the run
/// record.
pub fn expand_arguments(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<Vec<OsString>, RunFileInvocationError> {
    let (expanded, replaced) = expand_arguments_reporting(arguments)?;
    for list in &replaced {
        eprintln!("warning: {}", list.message());
    }
    let _ = REPLACED_LISTS.set(replaced);
    Ok(expanded)
}

/// [`expand_arguments`] without the side effects: the expanded arguments and
/// the run-file lists the command line replaced.
pub fn expand_arguments_reporting(
    arguments: impl IntoIterator<Item = OsString>,
) -> Result<(Vec<OsString>, Vec<ReplacedRunFileList>), RunFileInvocationError> {
    let original = arguments.into_iter().collect::<Vec<_>>();
    let Some(executable) = original.first().cloned() else {
        return Ok((original, Vec::new()));
    };
    let scan = scan_controls(&original[1..])?;
    let Some(run_file) = scan.run_file else {
        return Ok((original, Vec::new()));
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
    let mut replaced = Vec::new();
    if let Some(options) = first.value().get("options").and_then(Value::as_object) {
        for (name, value) in options {
            let replaced_by_alternative =
                ALTERNATIVE_OPTIONS.iter().any(|(option, alternative)| {
                    option == name && explicit_flags.contains(*alternative)
                });
            if explicit_flags.contains(name) || replaced_by_alternative {
                unsets.push(option_pointer(name));
            }
            if explicit_flags.contains(name)
                && let Value::Array(entries) = value
            {
                let run_file = entries.iter().filter_map(scalar_text).collect::<Vec<_>>();
                let command_line = explicit_values(&original[1..], name);
                let dropped = dropped_entries(&run_file, &command_line);
                if !dropped.is_empty() {
                    replaced.push(ReplacedRunFileList {
                        option: name.clone(),
                        dropped,
                    });
                }
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
    Ok((expanded, replaced))
}

/// The run file's entries a command-line list no longer names. An entry of
/// the form `NAME=VALUE` is named by its `NAME`, so giving `Hash=128` where
/// the run file had `Hash=64` changes a value rather than dropping it.
fn dropped_entries(run_file: &[String], command_line: &[String]) -> Vec<String> {
    let key = |entry: &str| {
        entry
            .split_once('=')
            .map_or(entry, |(name, _)| name)
            .trim()
            .to_owned()
    };
    let named = command_line
        .iter()
        .map(|entry| key(entry))
        .collect::<BTreeSet<_>>();
    run_file
        .iter()
        .filter(|entry| !named.contains(&key(entry)))
        .cloned()
        .collect()
}

/// Every value the command line gives the long option `name`, in either the
/// `--name value` or the `--name=value` form.
fn explicit_values(arguments: &[OsString], name: &str) -> Vec<String> {
    let flag = format!("--{name}");
    let prefix = format!("{flag}=");
    let mut values = Vec::new();
    let mut arguments = arguments.iter().filter_map(|argument| argument.to_str());
    while let Some(argument) = arguments.next() {
        if let Some(value) = argument.strip_prefix(&prefix) {
            values.push(value.to_owned());
        } else if argument == flag
            && let Some(value) = arguments.next()
        {
            values.push(value.to_owned());
        }
    }
    values
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
        "seed-from",
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
        ["spsa", "status" | "history"] | ["status"] | ["stats"] => vec![0],
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

    /// A run file's `a-option` list, expanded against a command line.
    fn replaced_lists(root: &Path, command_line: &[&str]) -> Vec<ReplacedRunFileList> {
        let run = root.join("run.toml");
        fs::write(
            &run,
            "command = [\"match\"]\npositionals = [\"a\", \"b\"]\n[options]\ngames = 4\n\
             a-option = [\"Hash=64\", \"Threads=2\", \"Ponder=false\"]\n",
        )
        .unwrap();
        let mut arguments = vec![
            OsString::from("colosseum-cli"),
            OsString::from("--run-file"),
            run.into_os_string(),
        ];
        arguments.extend(command_line.iter().map(OsString::from));
        expand_arguments_reporting(arguments).unwrap().1
    }

    #[test]
    fn a_command_line_list_that_drops_run_file_entries_is_reported_by_name() {
        let root = tempfile::tempdir().unwrap();
        let replaced = replaced_lists(root.path(), &["--a-option", "Hash=128"]);
        assert_eq!(
            replaced,
            [ReplacedRunFileList {
                option: "a-option".into(),
                dropped: vec!["Threads=2".into(), "Ponder=false".into()],
            }]
        );
        assert_eq!(
            replaced[0].message(),
            "--a-option on the command line replaces the run file's list and drops Threads=2, Ponder=false; repeat them on the command line to keep them"
        );
        // The `--name=value` form is the same list.
        assert_eq!(
            replaced_lists(root.path(), &["--a-option=Threads=4"])[0].dropped,
            ["Hash=64", "Ponder=false"]
        );
    }

    #[test]
    fn a_command_line_list_that_names_every_entry_or_a_scalar_is_not_reported() {
        let root = tempfile::tempdir().unwrap();
        // New values for every entry the run file named drop nothing.
        let every = [
            "--a-option",
            "Hash=128",
            "--a-option",
            "Threads=4",
            "--a-option",
            "Ponder=true",
        ];
        assert!(replaced_lists(root.path(), &every).is_empty());
        // A scalar the command line replaces is an ordinary override.
        assert!(replaced_lists(root.path(), &["--games", "8"]).is_empty());
        // Without a run file there is nothing to replace.
        let (_, replaced) =
            expand_arguments_reporting([OsString::from("colosseum-cli"), OsString::from("match")])
                .unwrap();
        assert!(replaced.is_empty());
    }

    /// Every option and positional that names a path resolves relative to the
    /// run file that declares it; a new path option or read-only command that
    /// is left out of the tables would silently resolve against the working
    /// directory instead.
    #[test]
    fn a_seed_source_and_a_history_run_directory_are_run_file_paths() {
        let seeded = serde_json::json!({
            "command": ["spsa"],
            "positionals": ["engine"],
            "options": {"seed-from": "../tunes/v1", "tune": "tune.toml"}
        });
        assert_eq!(
            path_pointers(&seeded).unwrap(),
            ["/positionals/0", "/options/seed-from", "/options/tune"]
        );
        let history = serde_json::json!({
            "command": ["spsa", "history"],
            "positionals": ["../tunes/v1"]
        });
        assert_eq!(path_pointers(&history).unwrap(), ["/positionals/0"]);
    }

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

        let expanded = expand_arguments_reporting([
            OsString::from("colosseum-cli"),
            OsString::from("--run-file"),
            run.into_os_string(),
            OsString::from("--option"),
            OsString::from("Hash=128"),
        ])
        .unwrap()
        .0;
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
    fn a_budget_on_the_command_line_replaces_the_run_files_horizon_and_back() {
        let root = tempfile::tempdir().unwrap();
        let expand = |file: &str, extra: [&str; 2]| {
            let run = root.path().join("tune.toml");
            fs::write(&run, file).unwrap();
            expand_arguments_reporting(
                [
                    OsString::from("colosseum-cli"),
                    OsString::from("--run-file"),
                    run.into_os_string(),
                ]
                .into_iter()
                .chain(extra.map(OsString::from)),
            )
            .unwrap()
            .0
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
        };
        let text = expand(
            "command = [\"spsa\"]\n[options]\niterations = 5000\ngames-per-iteration = 32\n",
            ["--total-games", "168000"],
        );
        assert!(
            !text.iter().any(|value| value == "--iterations"),
            "{text:?}"
        );
        assert!(
            text.windows(2)
                .any(|pair| pair == ["--total-games", "168000"])
        );
        assert!(
            text.windows(2)
                .any(|pair| pair == ["--games-per-iteration", "32"])
        );

        let text = expand(
            "command = [\"spsa\"]\n[options]\ntotal-games = 168000\n",
            ["--iterations", "10"],
        );
        assert!(
            !text.iter().any(|value| value == "--total-games"),
            "{text:?}"
        );
        assert!(text.windows(2).any(|pair| pair == ["--iterations", "10"]));
    }

    #[test]
    fn explicit_command_uses_shared_options_without_file_positionals() {
        let root = tempfile::tempdir().unwrap();
        let run = root.path().join("common.toml");
        fs::write(&run, "[options]\nmovetime-ms = 100\n").unwrap();
        let expanded = expand_arguments_reporting([
            OsString::from("colosseum-cli"),
            OsString::from("--run-file"),
            run.into_os_string(),
            OsString::from("match"),
            OsString::from("candidate"),
            OsString::from("baseline"),
            OsString::from("--games"),
            OsString::from("20"),
        ])
        .unwrap()
        .0;
        let text = expanded
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(text.windows(2).any(|pair| pair == ["--movetime-ms", "100"]));
    }
}
