//! Deterministic run-file inheritance and configuration identity.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const MAX_EXTEND_DEPTH: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ValueOrigin {
    BuiltIn,
    RunFile { file: PathBuf },
    CommandLine { directory: PathBuf },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedConfig {
    value: Value,
    origins: BTreeMap<String, ValueOrigin>,
    canonical_json: Vec<u8>,
    sha256: String,
}

impl ResolvedConfig {
    #[must_use]
    pub fn value(&self) -> &Value {
        &self.value
    }

    #[must_use]
    pub fn canonical_json(&self) -> &[u8] {
        &self.canonical_json
    }

    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    #[must_use]
    pub fn origins(&self) -> &BTreeMap<String, ValueOrigin> {
        &self.origins
    }

    /// Resolve a string-valued path relative to the layer that declared it.
    pub fn resolve_path(&self, pointer: &str) -> Result<PathBuf, ConfigError> {
        resolved_path(&self.value, &self.origins, pointer)
    }

    /// Write the exact bytes that were hashed plus human/audit sidecars.
    pub fn write_to(&self, directory: &Path) -> Result<(), ConfigError> {
        fs::create_dir_all(directory).map_err(|source| ConfigError::Write {
            path: directory.to_path_buf(),
            source,
        })?;
        write(
            &directory.join("resolved-config.json"),
            &self.canonical_json,
        )?;
        write(
            &directory.join("config.sha256"),
            format!("{}  resolved-config.json\n", self.sha256).as_bytes(),
        )?;
        let origins = serde_json::to_vec(&self.origins).expect("origins are serializable");
        write(&directory.join("config-origins.json"), &origins)
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not canonicalize run file {path}; chain: {chain}: {source}")]
    Canonicalize {
        path: PathBuf,
        chain: Chain,
        source: std::io::Error,
    },
    #[error("could not read run file {path}; chain: {chain}: {source}")]
    Read {
        path: PathBuf,
        chain: Chain,
        source: std::io::Error,
    },
    #[error("could not parse run file {path}; chain: {chain}: {source}")]
    Parse {
        path: PathBuf,
        chain: Chain,
        source: Box<toml::de::Error>,
    },
    #[error("run file root must be a table: {0}")]
    RootNotTable(PathBuf),
    #[error("run file {file} has a non-string extend value")]
    InvalidExtend { file: PathBuf },
    #[error("run file {file} has an unset value that is not an array of strings")]
    InvalidUnset { file: PathBuf },
    #[error("run-file inheritance exceeds {MAX_EXTEND_DEPTH} files; chain: {chain}")]
    ExcessiveDepth { chain: Chain },
    #[error("run-file inheritance cycle: {chain}")]
    Cycle { chain: Chain },
    #[error("invalid unset pointer {pointer:?} declared by {declared_by}: {reason}")]
    InvalidPointer {
        declared_by: String,
        pointer: String,
        reason: String,
    },
    #[error("configuration path {0} is not a string")]
    NotStringPath(String),
    #[error("resolved configuration path {0} is not valid UTF-8")]
    NonUtf8Path(String),
    #[error("configuration path {0} has no recorded origin")]
    MissingOrigin(String),
    #[error("relative built-in path {0} has no filesystem origin")]
    RelativeBuiltInPath(String),
    #[error("could not canonicalize invocation directory {path}: {source}")]
    InvocationDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("could not write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain(Vec<PathBuf>);

impl std::fmt::Display for Chain {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = self
            .0
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(" -> ");
        formatter.write_str(&text)
    }
}

#[derive(Debug)]
struct FileLayer {
    file: PathBuf,
    value: Value,
    unset: Vec<String>,
}

#[must_use]
pub fn built_in_defaults() -> Value {
    json!({
        "schema_version": 1,
        "stats_version": 1
    })
}

/// Resolve defaults, an optional inherited TOML run file, and CLI overrides.
/// CLI unsets are applied after the file chain and before CLI values.
pub fn resolve_config(
    defaults: Value,
    run_file: Option<&Path>,
    cli: Value,
    cli_unset: &[String],
    invocation_directory: &Path,
    path_pointers: &[String],
) -> Result<ResolvedConfig, ConfigError> {
    let invocation_directory = dunce::canonicalize(invocation_directory).map_err(|source| {
        ConfigError::InvocationDirectory {
            path: invocation_directory.to_path_buf(),
            source,
        }
    })?;
    let mut value = Value::Object(Map::new());
    let mut origins = BTreeMap::new();
    merge(
        &mut value,
        defaults,
        "",
        &ValueOrigin::BuiltIn,
        &mut origins,
    );

    if let Some(run_file) = run_file {
        let mut layers = Vec::new();
        load_layers(run_file, &mut Vec::new(), &mut layers)?;
        for layer in layers {
            let declared_by = layer.file.display().to_string();
            for pointer in &layer.unset {
                unset(&mut value, pointer, &declared_by, &mut origins)?;
            }
            merge(
                &mut value,
                layer.value,
                "",
                &ValueOrigin::RunFile { file: layer.file },
                &mut origins,
            );
        }
    }

    for pointer in cli_unset {
        unset(&mut value, pointer, "command line", &mut origins)?;
    }
    merge(
        &mut value,
        cli,
        "",
        &ValueOrigin::CommandLine {
            directory: invocation_directory,
        },
        &mut origins,
    );
    for pointer in path_pointers {
        let resolved = resolved_path(&value, &origins, pointer)?;
        let resolved = resolved
            .to_str()
            .ok_or_else(|| ConfigError::NonUtf8Path(pointer.clone()))?
            .to_owned();
        *value
            .pointer_mut(pointer)
            .expect("resolved path pointer was already validated") = Value::String(resolved);
    }
    sort_value(&mut value);
    let canonical_json = serde_json::to_vec(&value).expect("JSON value serialization cannot fail");
    let sha256 = hex(&Sha256::digest(&canonical_json));
    Ok(ResolvedConfig {
        value,
        origins,
        canonical_json,
        sha256,
    })
}

fn resolved_path(
    value: &Value,
    origins: &BTreeMap<String, ValueOrigin>,
    pointer: &str,
) -> Result<PathBuf, ConfigError> {
    let value = value
        .pointer(pointer)
        .and_then(Value::as_str)
        .ok_or_else(|| ConfigError::NotStringPath(pointer.to_owned()))?;
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        match origins.get(pointer) {
            Some(ValueOrigin::RunFile { file }) => file
                .parent()
                .expect("canonical run file has a parent")
                .join(path),
            Some(ValueOrigin::CommandLine { directory }) => directory.join(path),
            Some(ValueOrigin::BuiltIn) => {
                return Err(ConfigError::RelativeBuiltInPath(pointer.to_owned()));
            }
            None => return Err(ConfigError::MissingOrigin(pointer.to_owned())),
        }
    };
    Ok(normalize_absolute_path(&normalize_path(&path)))
}

fn load_layers(
    requested: &Path,
    stack: &mut Vec<PathBuf>,
    layers: &mut Vec<FileLayer>,
) -> Result<(), ConfigError> {
    if stack.len() >= MAX_EXTEND_DEPTH {
        let mut chain = stack.clone();
        chain.push(requested.to_path_buf());
        return Err(ConfigError::ExcessiveDepth {
            chain: Chain(chain),
        });
    }
    let canonical = dunce::canonicalize(requested).map_err(|source| ConfigError::Canonicalize {
        path: requested.to_path_buf(),
        chain: Chain(stack.clone()),
        source,
    })?;
    if let Some(position) = stack.iter().position(|path| path == &canonical) {
        let mut cycle = stack[position..].to_vec();
        cycle.push(canonical);
        return Err(ConfigError::Cycle {
            chain: Chain(cycle),
        });
    }
    stack.push(canonical.clone());
    let text = fs::read_to_string(&canonical).map_err(|source| ConfigError::Read {
        path: canonical.clone(),
        chain: Chain(stack.clone()),
        source,
    })?;
    let document: toml::Value = toml::from_str(&text).map_err(|source| ConfigError::Parse {
        path: canonical.clone(),
        chain: Chain(stack.clone()),
        source: Box::new(source),
    })?;
    let mut value = serde_json::to_value(document).expect("TOML converts to JSON");
    let table = value
        .as_object_mut()
        .ok_or_else(|| ConfigError::RootNotTable(canonical.clone()))?;
    let extend = match table.remove("extend") {
        Some(Value::String(path)) => Some(path),
        Some(_) => return Err(ConfigError::InvalidExtend { file: canonical }),
        None => None,
    };
    let unset = match table.remove("unset") {
        Some(Value::Array(values)) => values
            .into_iter()
            .map(|value| value.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| ConfigError::InvalidUnset {
                file: canonical.clone(),
            })?,
        Some(_) => {
            return Err(ConfigError::InvalidUnset {
                file: canonical.clone(),
            });
        }
        None => Vec::new(),
    };
    if let Some(parent) = extend {
        let parent = canonical
            .parent()
            .expect("canonical file has parent")
            .join(parent);
        load_layers(&parent, stack, layers)?;
    }
    layers.push(FileLayer {
        file: canonical,
        value,
        unset,
    });
    stack.pop();
    Ok(())
}

fn merge(
    target: &mut Value,
    overlay: Value,
    pointer: &str,
    origin: &ValueOrigin,
    origins: &mut BTreeMap<String, ValueOrigin>,
) {
    match overlay {
        Value::Object(source) => {
            if !target.is_object() {
                clear_origins(origins, pointer);
                *target = Value::Object(Map::new());
            }
            if source.is_empty() {
                origins.insert(pointer.to_owned(), origin.clone());
            }
            let target = target.as_object_mut().expect("set to object");
            for (key, value) in source {
                let child_pointer = format!("{pointer}/{}", escape_token(&key));
                let child = target.entry(key).or_insert(Value::Null);
                merge(child, value, &child_pointer, origin, origins);
            }
        }
        replacement => {
            clear_origins(origins, pointer);
            *target = replacement;
            mark_origin(target, pointer, origin, origins);
        }
    }
}

fn mark_origin(
    value: &Value,
    pointer: &str,
    origin: &ValueOrigin,
    origins: &mut BTreeMap<String, ValueOrigin>,
) {
    origins.insert(pointer.to_owned(), origin.clone());
    match value {
        Value::Object(table) => {
            for (key, value) in table {
                mark_origin(
                    value,
                    &format!("{pointer}/{}", escape_token(key)),
                    origin,
                    origins,
                );
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                mark_origin(value, &format!("{pointer}/{index}"), origin, origins);
            }
        }
        _ => {}
    }
}

fn unset(
    value: &mut Value,
    pointer: &str,
    declared_by: &str,
    origins: &mut BTreeMap<String, ValueOrigin>,
) -> Result<(), ConfigError> {
    let tokens = pointer_tokens(pointer).map_err(|reason| ConfigError::InvalidPointer {
        declared_by: declared_by.to_owned(),
        pointer: pointer.to_owned(),
        reason,
    })?;
    let (last, parents) = tokens.split_last().expect("root pointer is rejected");
    let parent_pointer = parents.iter().fold(String::new(), |pointer, token| {
        format!("{pointer}/{}", escape_token(token))
    });
    let mut current = value;
    for token in parents {
        current = descend_mut(current, token).ok_or_else(|| ConfigError::InvalidPointer {
            declared_by: declared_by.to_owned(),
            pointer: pointer.to_owned(),
            reason: format!("parent token {token:?} does not exist"),
        })?;
    }
    let mut array_changed = false;
    let removed = match current {
        Value::Object(table) => table.remove(last).is_some(),
        Value::Array(values) => {
            let removed = parse_index(last)
                .and_then(|index| (index < values.len()).then(|| values.remove(index)))
                .is_some();
            array_changed = removed;
            removed
        }
        _ => false,
    };
    if !removed {
        return Err(ConfigError::InvalidPointer {
            declared_by: declared_by.to_owned(),
            pointer: pointer.to_owned(),
            reason: "target does not exist".into(),
        });
    }
    if array_changed {
        let origin = origins.get(&parent_pointer).cloned();
        clear_origins(origins, &parent_pointer);
        if let Some(origin) = origin {
            mark_origin(current, &parent_pointer, &origin, origins);
        }
    } else {
        clear_origins(origins, pointer);
    }
    Ok(())
}

fn pointer_tokens(pointer: &str) -> Result<Vec<String>, String> {
    if pointer.is_empty() {
        return Err("the document root cannot be unset".into());
    }
    let Some(rest) = pointer.strip_prefix('/') else {
        return Err("RFC 6901 pointers must start with '/'".into());
    };
    rest.split('/').map(unescape_token).collect()
}

fn unescape_token(token: &str) -> Result<String, String> {
    let mut output = String::new();
    let mut characters = token.chars();
    while let Some(character) = characters.next() {
        if character != '~' {
            output.push(character);
            continue;
        }
        match characters.next() {
            Some('0') => output.push('~'),
            Some('1') => output.push('/'),
            Some(other) => return Err(format!("invalid escape ~{other}")),
            None => return Err("trailing '~' escape".into()),
        }
    }
    Ok(output)
}

fn descend_mut<'a>(value: &'a mut Value, token: &str) -> Option<&'a mut Value> {
    match value {
        Value::Object(table) => table.get_mut(token),
        Value::Array(values) => parse_index(token).and_then(|index| values.get_mut(index)),
        _ => None,
    }
}

fn parse_index(token: &str) -> Option<usize> {
    if token.len() > 1 && token.starts_with('0') {
        return None;
    }
    token.parse().ok()
}

fn clear_origins(origins: &mut BTreeMap<String, ValueOrigin>, pointer: &str) {
    let descendant = format!("{pointer}/");
    origins.retain(|key, _| key != pointer && !key.starts_with(&descendant));
}

fn escape_token(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn sort_value(value: &mut Value) {
    match value {
        Value::Object(table) => {
            for value in table.values_mut() {
                sort_value(value);
            }
            table.sort_keys();
        }
        Value::Array(values) => {
            for value in values {
                sort_value(value);
            }
        }
        _ => {}
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let _ = normalized.pop();
            }
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

#[cfg(windows)]
fn normalize_absolute_path(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut suffix = Vec::new();
    while !existing.exists() {
        let Some(name) = existing.file_name() else {
            break;
        };
        suffix.push(name.to_owned());
        let Some(parent) = existing.parent() else {
            break;
        };
        existing = parent;
    }
    let mut normalized = dunce::canonicalize(existing).unwrap_or_else(|_| existing.to_path_buf());
    for component in suffix.into_iter().rev() {
        normalized.push(component);
    }
    normalized
}

#[cfg(not(windows))]
fn normalize_absolute_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    fs::write(path, bytes).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })
}
