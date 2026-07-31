use std::fs;
use std::path::Path;

use colosseum_cli::{ConfigError, ValueOrigin, built_in_defaults, resolve_config};
use serde_json::json;

fn write(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
}

#[test]
fn resolves_defaults_parent_child_and_cli_with_stable_origins() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("base.toml");
    let child_dir = root.path().join("child");
    fs::create_dir(&child_dir).unwrap();
    let child = child_dir.join("run.toml");
    write(
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
    write(
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
    write(
        &parent,
        "[match]\ngames = 20\nlabels = [\"a\", \"b\"]\n[match.nested]\na = 1\nb = 2\n",
    );
    write(
        &child,
        "extend = \"parent.toml\"\nunset = [\"/match/nested/b\"]\n[match]\nlabels = [\"c\"]\n[match.nested]\nc = 3\n",
    );
    write(
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
    write(&run, "[engine]\npath = \"bin/engine\"\n");
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
    write(&bad_pointer, "unset = [\"missing-leading-slash\"]\n");
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
    write(&missing_parent, "extend = \"does-not-exist.toml\"\n");
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
    write(&first, "extend = \"second.toml\"\n");
    write(&second, "extend = \"./first.toml\"\n");
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
        write(&deep.path().join(format!("{index}.toml")), &contents);
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
