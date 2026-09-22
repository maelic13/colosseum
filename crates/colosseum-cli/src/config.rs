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
    Generated,
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

    pub(crate) fn seed(&self) -> Option<&Value> {
        self.value.get("seed")
    }

    pub(crate) fn insert_generated_seed(&mut self, seed: u64) -> Result<(), ConfigError> {
        let table = self
            .value
            .as_object_mut()
            .ok_or(ConfigError::ResolvedRootNotObject)?;
        table.insert("seed".into(), Value::from(seed));
        self.origins.insert("/seed".into(), ValueOrigin::Generated);
        self.rebuild_identity();
        Ok(())
    }

    fn rebuild_identity(&mut self) {
        sort_value(&mut self.value);
        self.canonical_json =
            serde_json::to_vec(&self.value).expect("JSON value serialization cannot fail");
        self.sha256 = hex(&Sha256::digest(&self.canonical_json));
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
    #[error("relative generated path {0} has no filesystem origin")]
    RelativeGeneratedPath(String),
    #[error("resolved configuration root must be an object")]
    ResolvedRootNotObject,
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
        "stats_version": colosseum_core::STATS_VERSION
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
            Some(ValueOrigin::Generated) => {
                return Err(ConfigError::RelativeGeneratedPath(pointer.to_owned()));
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

fn write(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    fs::write(path, bytes).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn write_file(path: &Path, text: &str) {
        fs::write(path, text).unwrap();
    }

    #[test]
    fn resolves_defaults_parent_child_and_cli_with_stable_origins() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base.toml");
        let child_dir = root.path().join("child");
        fs::create_dir(&child_dir).unwrap();
        let child = child_dir.join("run.toml");
        write_file(
            &base,
            r#"
[engine]
path = "engines/base"
arguments = ["--base", "network.nnue"]

[engine.options]
Hash = 64
EvalFile = "base.nnue"

[optional]
keep = true
remove = true
"#,
        );
        write_file(
            &child,
            r#"
extend = "../base.toml"
unset = ["/optional/remove", "/engine/options/EvalFile"]

[engine]
arguments = ["--child"]
book = "books/child.epd"

[engine.options]
Hash = 128
"#,
        );
        let defaults = json!({
            "engine": {"options": {"Threads": 1}},
            "concurrency": 1,
            "schema_version": 1,
            "stats_version": 1
        });
        let cli = json!({
            "concurrency": 4,
            "engine": {
                "cwd": "cli-cwd",
                "options": {"Hash": 256}
            }
        });

        let resolved = resolve_config(
            defaults,
            Some(&child),
            cli,
            &[],
            root.path(),
            &[
                "/engine/path".into(),
                "/engine/book".into(),
                "/engine/cwd".into(),
            ],
        )
        .unwrap();
        assert_eq!(resolved.value()["concurrency"], 4);
        assert_eq!(resolved.value()["engine"]["arguments"], json!(["--child"]));
        assert_eq!(resolved.value()["engine"]["options"]["Threads"], 1);
        assert_eq!(resolved.value()["engine"]["options"]["Hash"], 256);
        assert!(
            resolved.value()["engine"]["options"]
                .get("EvalFile")
                .is_none()
        );
        assert!(resolved.value()["optional"].get("remove").is_none());
        assert!(resolved.value().get("extend").is_none());
        assert!(resolved.value().get("unset").is_none());

        assert_eq!(
            resolved.resolve_path("/engine/path").unwrap(),
            dunce::canonicalize(&base)
                .unwrap()
                .parent()
                .unwrap()
                .join("engines/base")
        );
        assert_eq!(
            resolved.resolve_path("/engine/book").unwrap(),
            dunce::canonicalize(&child)
                .unwrap()
                .parent()
                .unwrap()
                .join("books/child.epd")
        );
        assert_eq!(
            resolved.resolve_path("/engine/cwd").unwrap(),
            dunce::canonicalize(root.path()).unwrap().join("cli-cwd")
        );
        assert!(matches!(
            resolved.origins()["/engine/path"],
            ValueOrigin::RunFile { .. }
        ));
    }

    #[test]
    fn inherited_and_flattened_files_have_identical_canonical_identity() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("parent.toml");
        let child = root.path().join("child.toml");
        let flat = root.path().join("flat.toml");
        write_file(
            &parent,
            "[match]\ngames = 20\nlabels = [\"a\", \"b\"]\n[match.nested]\na = 1\nb = 2\n",
        );
        write_file(
            &child,
            "extend = \"parent.toml\"\nunset = [\"/match/nested/b\"]\n[match]\nlabels = [\"c\"]\n[match.nested]\nc = 3\n",
        );
        write_file(
            &flat,
            "[match]\ngames = 20\nlabels = [\"c\"]\n[match.nested]\na = 1\nc = 3\n",
        );

        let inherited =
            resolve_config(json!({}), Some(&child), json!({}), &[], root.path(), &[]).unwrap();
        let flattened =
            resolve_config(json!({}), Some(&flat), json!({}), &[], root.path(), &[]).unwrap();
        assert_eq!(inherited.canonical_json(), flattened.canonical_json());
        assert_eq!(inherited.sha256(), flattened.sha256());
    }

    #[test]
    fn run_file_and_equivalent_cli_path_normalize_to_identical_json() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("bin")).unwrap();
        let run = root.path().join("run.toml");
        write_file(&run, "[engine]\npath = \"bin/engine\"\n");
        let pointer = vec!["/engine/path".to_owned()];
        let from_file = resolve_config(
            built_in_defaults(),
            Some(&run),
            json!({}),
            &[],
            root.path(),
            &pointer,
        )
        .unwrap();
        let absolute = root.path().join("bin/engine");
        let from_cli = resolve_config(
            built_in_defaults(),
            None,
            json!({"engine": {"path": absolute}}),
            &[],
            root.path(),
            &pointer,
        )
        .unwrap();
        assert_eq!(from_file.canonical_json(), from_cli.canonical_json());
        assert_eq!(from_file.sha256(), from_cli.sha256());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_cli_path_matches_the_run_file_canonical_identity() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let actual = root.path().join("actual");
        fs::create_dir_all(actual.join("bin")).unwrap();
        let alias = root.path().join("alias");
        symlink(&actual, &alias).unwrap();
        let run = actual.join("run.toml");
        write_file(&run, "[engine]\npath = \"bin/engine\"\n");
        let pointer = vec!["/engine/path".to_owned()];

        let from_file = resolve_config(
            built_in_defaults(),
            Some(&run),
            json!({}),
            &[],
            root.path(),
            &pointer,
        )
        .unwrap();
        let from_cli = resolve_config(
            built_in_defaults(),
            None,
            json!({"engine": {"path": alias.join("bin/engine")}}),
            &[],
            root.path(),
            &pointer,
        )
        .unwrap();

        assert_eq!(from_file.canonical_json(), from_cli.canonical_json());
        assert_eq!(from_file.sha256(), from_cli.sha256());
    }

    #[test]
    fn rfc6901_unset_handles_escaped_object_names_and_cli_clearing() {
        let root = tempfile::tempdir().unwrap();
        let resolved = resolve_config(
            json!({"a/b": {"x~y": 1}, "keep": true}),
            None,
            json!({}),
            &["/a~1b/x~0y".into()],
            root.path(),
            &[],
        )
        .unwrap();
        assert_eq!(resolved.value(), &json!({"a/b": {}, "keep": true}));
    }

    #[test]
    fn removing_an_array_element_reindexes_its_origin_metadata() {
        let root = tempfile::tempdir().unwrap();
        let resolved = resolve_config(
            json!({"values": ["a", "b", "c"]}),
            None,
            json!({}),
            &["/values/1".into()],
            root.path(),
            &[],
        )
        .unwrap();
        assert_eq!(resolved.value()["values"], json!(["a", "c"]));
        assert!(matches!(
            resolved.origins()["/values/1"],
            ValueOrigin::BuiltIn
        ));
        assert!(!resolved.origins().contains_key("/values/2"));
    }

    #[test]
    fn bad_pointer_and_unreadable_parent_name_the_declaration_chain() {
        let root = tempfile::tempdir().unwrap();
        let bad_pointer = root.path().join("bad-pointer.toml");
        write_file(&bad_pointer, "unset = [\"missing-leading-slash\"]\n");
        let error = resolve_config(
            built_in_defaults(),
            Some(&bad_pointer),
            json!({}),
            &[],
            root.path(),
            &[],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("bad-pointer.toml"));
        assert!(error.contains("missing-leading-slash"));

        let missing_parent = root.path().join("missing-parent.toml");
        write_file(&missing_parent, "extend = \"does-not-exist.toml\"\n");
        let error = resolve_config(
            built_in_defaults(),
            Some(&missing_parent),
            json!({}),
            &[],
            root.path(),
            &[],
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("missing-parent.toml"));
        assert!(error.contains("does-not-exist.toml"));
    }

    #[test]
    fn canonical_cycle_and_depth_limit_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let first = root.path().join("first.toml");
        let second = root.path().join("second.toml");
        write_file(&first, "extend = \"second.toml\"\n");
        write_file(&second, "extend = \"./first.toml\"\n");
        let error =
            resolve_config(json!({}), Some(&first), json!({}), &[], root.path(), &[]).unwrap_err();
        assert!(matches!(error, ConfigError::Cycle { .. }));
        let message = error.to_string();
        assert!(message.contains("first.toml"));
        assert!(message.contains("second.toml"));

        let deep = tempfile::tempdir().unwrap();
        for index in 0..17 {
            let contents = if index == 16 {
                "value = 1\n".to_owned()
            } else {
                format!("extend = \"{}.toml\"\n", index + 1)
            };
            write_file(&deep.path().join(format!("{index}.toml")), &contents);
        }
        let error = resolve_config(
            json!({}),
            Some(&deep.path().join("0.toml")),
            json!({}),
            &[],
            deep.path(),
            &[],
        )
        .unwrap_err();
        assert!(matches!(error, ConfigError::ExcessiveDepth { .. }));
    }

    #[test]
    fn writes_the_exact_hashed_config_and_origin_sidecar() {
        let root = tempfile::tempdir().unwrap();
        let resolved = resolve_config(
            built_in_defaults(),
            None,
            json!({"engine": {"path": "engine"}}),
            &[],
            root.path(),
            &["/engine/path".into()],
        )
        .unwrap();
        let output = root.path().join("out");
        resolved.write_to(&output).unwrap();

        assert_eq!(
            fs::read(output.join("resolved-config.json")).unwrap(),
            resolved.canonical_json()
        );
        let hash = fs::read_to_string(output.join("config.sha256")).unwrap();
        assert_eq!(
            hash,
            format!("{}  resolved-config.json\n", resolved.sha256())
        );
        let origins: serde_json::Value =
            serde_json::from_slice(&fs::read(output.join("config-origins.json")).unwrap()).unwrap();
        assert_eq!(origins["/engine/path"]["kind"], "command-line");
    }

    #[test]
    fn built_in_relative_paths_require_an_explicit_origin_policy() {
        let root = tempfile::tempdir().unwrap();
        let error = resolve_config(
            json!({"path": "relative"}),
            None,
            json!({}),
            &[],
            root.path(),
            &["/path".into()],
        )
        .unwrap_err();
        assert!(matches!(error, ConfigError::RelativeBuiltInPath(_)));
    }
}
