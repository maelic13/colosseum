use std::fs;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseMetadata {
    pub validation: &'static str,
    pub product: &'static str,
    pub package: &'static str,
    pub version: String,
    pub tag: String,
    pub changelog: &'static str,
    pub artifact_stem: String,
    pub prerelease: bool,
}

#[derive(Debug, Clone, Copy)]
struct Product {
    name: &'static str,
    package: &'static str,
    manifest: &'static str,
    changelog: &'static str,
    other_changelog: &'static str,
    artifact_name: &'static str,
    tag_prefix: &'static str,
}

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("tag must start with gui-v or cli-v")]
    TagPrefix,
    #[error("invalid release semantic version: {0}")]
    Semver(#[from] semver::Error),
    #[error("release versions must not contain build metadata")]
    BuildMetadata,
    #[error("Colosseum CLI publishes stable versions only")]
    CliPrerelease,
    #[error("could not read {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("invalid package manifest {path}: {source}")]
    Manifest {
        path: String,
        source: toml::de::Error,
    },
    #[error("package {package} has version {actual}, tag requests {requested}")]
    VersionMismatch {
        package: &'static str,
        actual: String,
        requested: Version,
    },
    #[error("{changelog} has no release heading for {version}")]
    MissingNotes {
        changelog: &'static str,
        version: Version,
    },
    #[error("product must be gui or cli")]
    Product,
    #[error("invalid CLI artifact platform/architecture pair: {platform}/{arch}")]
    Platform { platform: String, arch: String },
    #[error("unsupported target triple: {0}")]
    Target(String),
    #[error("CLI artifact version {requested} does not match package version {actual}")]
    ArtifactVersion { requested: Version, actual: String },
    #[error("artifact staging directory already exists: {0}")]
    StageExists(String),
    #[error("could not copy {path}: {source}")]
    Copy {
        path: String,
        source: std::io::Error,
    },
}

pub fn validate(root: &Path, tag: &str) -> Result<ReleaseMetadata, MetadataError> {
    let (product, raw_version) = if let Some(version) = tag.strip_prefix("gui-v") {
        (product("gui")?, version)
    } else if let Some(version) = tag.strip_prefix("cli-v") {
        (product("cli")?, version)
    } else {
        return Err(MetadataError::TagPrefix);
    };

    let version = Version::parse(raw_version)?;
    if !version.build.is_empty() {
        return Err(MetadataError::BuildMetadata);
    }
    if product.name == "cli" && !version.pre.is_empty() {
        return Err(MetadataError::CliPrerelease);
    }
    let actual = package_version(root, product)?;
    if actual != version.to_string() {
        return Err(MetadataError::VersionMismatch {
            package: product.package,
            actual,
            requested: version,
        });
    }

    let heading = format!("## [{}]", version);
    let notes = read(root, product.changelog)?;
    if !notes.lines().any(|line| line.trim().starts_with(&heading)) {
        let _misfiled = read(root, product.other_changelog)?
            .lines()
            .any(|line| line.trim().starts_with(&heading));
        return Err(MetadataError::MissingNotes {
            changelog: product.changelog,
            version,
        });
    }

    Ok(metadata(product, version, tag.to_owned(), "release"))
}

/// Validate the package-owned version for an immutable, unpublished candidate.
///
/// Candidates deliberately do not require final changelog headings or tags.
/// They are workflow artifacts tied to a commit SHA, never GitHub Releases.
pub fn candidate(root: &Path, name: &str) -> Result<ReleaseMetadata, MetadataError> {
    let product = product(name)?;
    let version = Version::parse(&package_version(root, product)?)?;
    if !version.build.is_empty() {
        return Err(MetadataError::BuildMetadata);
    }
    if product.name == "cli" && !version.pre.is_empty() {
        return Err(MetadataError::CliPrerelease);
    }
    let proposed_tag = format!("{}{}", product.tag_prefix, version);
    Ok(metadata(product, version, proposed_tag, "candidate"))
}

/// Extract only the selected product/version release-note section.
pub fn release_notes(root: &Path, tag: &str) -> Result<String, MetadataError> {
    let metadata = validate(root, tag)?;
    let text = read(root, metadata.changelog)?;
    let heading = format!("## [{}]", metadata.version);
    let mut collecting = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if line.trim().starts_with("## [") {
            if collecting {
                break;
            }
            collecting = line.trim().starts_with(&heading);
            continue;
        }
        if collecting {
            lines.push(line);
        }
    }
    // A changelog may separate versions with a horizontal rule; it belongs
    // between sections, not at the end of one release's notes.
    while lines
        .last()
        .is_some_and(|line| line.trim().is_empty() || line.trim() == "---")
    {
        lines.pop();
    }
    Ok(lines.join("\n").trim().to_owned() + "\n")
}

/// Map a Rust target triple to the artifact's platform and architecture
/// tokens.
///
/// One mapping for both products and every caller — the build entry point, the
/// release workflows and the archive smoke — so an artifact's name can never
/// disagree with the triple it was built for.
pub fn target_platform(target: &str) -> Result<(&'static str, &'static str), MetadataError> {
    let platform = if target.contains("windows") {
        "windows"
    } else if target.contains("linux") {
        "linux"
    } else if target.contains("darwin") {
        "macos"
    } else {
        return Err(MetadataError::Target(target.to_owned()));
    };
    let arch = if target.starts_with("x86_64") {
        "x64"
    } else if target.starts_with("aarch64") {
        "arm64"
    } else {
        return Err(MetadataError::Target(target.to_owned()));
    };
    Ok((platform, arch))
}

/// Build the exact allowlisted directory later archived by the platform job.
pub fn stage_cli(
    root: &Path,
    raw_version: &str,
    platform: &str,
    arch: &str,
    binary: &Path,
    output: &Path,
) -> Result<PathBuf, MetadataError> {
    // The released matrix, in the one naming scheme both products use:
    // <product>-<version>-<windows|linux|macos>-<x64|arm64>.
    if !matches!(
        (platform, arch),
        ("windows", "x64") | ("windows", "arm64") | ("linux", "x64") | ("macos", "arm64")
    ) {
        return Err(MetadataError::Platform {
            platform: platform.into(),
            arch: arch.into(),
        });
    }
    let requested = Version::parse(raw_version)?;
    let product = product("cli")?;
    let actual = package_version(root, product)?;
    if actual != requested.to_string() {
        return Err(MetadataError::ArtifactVersion { requested, actual });
    }
    let name = format!("colosseum-cli-{raw_version}-{platform}-{arch}");
    let stage = output.join(name);
    if stage.exists() {
        return Err(MetadataError::StageExists(stage.display().to_string()));
    }
    fs::create_dir_all(stage.join("docs/cli")).map_err(|source| MetadataError::Copy {
        path: stage.display().to_string(),
        source,
    })?;
    let binary_name = if platform == "windows" {
        "colosseum-cli.exe"
    } else {
        "colosseum-cli"
    };
    copy(binary, &stage.join(binary_name))?;
    copy(&root.join("LICENSE"), &stage.join("LICENSE"))?;
    copy(
        &root.join("packaging/cli/README.md"),
        &stage.join("README.md"),
    )?;
    copy(
        &root.join("CHANGELOG-CLI.md"),
        &stage.join("CHANGELOG-CLI.md"),
    )?;
    copy_tree(&root.join("docs/cli"), &stage.join("docs/cli"))?;
    Ok(stage)
}

fn metadata(
    product: Product,
    version: Version,
    tag: String,
    validation: &'static str,
) -> ReleaseMetadata {
    ReleaseMetadata {
        validation,
        product: product.name,
        package: product.package,
        version: version.to_string(),
        tag,
        changelog: product.changelog,
        artifact_stem: format!("{}-{version}", product.artifact_name),
        prerelease: !version.pre.is_empty(),
    }
}

fn product(name: &str) -> Result<Product, MetadataError> {
    match name {
        "gui" => Ok(Product {
            name: "gui",
            package: "colosseum-gui",
            manifest: "crates/colosseum-gui/Cargo.toml",
            changelog: "CHANGELOG-GUI.md",
            other_changelog: "CHANGELOG-CLI.md",
            artifact_name: "colosseum-gui",
            tag_prefix: "gui-v",
        }),
        "cli" => Ok(Product {
            name: "cli",
            package: "colosseum-cli",
            manifest: "crates/colosseum-cli/Cargo.toml",
            changelog: "CHANGELOG-CLI.md",
            other_changelog: "CHANGELOG-GUI.md",
            artifact_name: "colosseum-cli",
            tag_prefix: "cli-v",
        }),
        _ => Err(MetadataError::Product),
    }
}

fn package_version(root: &Path, product: Product) -> Result<String, MetadataError> {
    let manifest_text = read(root, product.manifest)?;
    let document: toml::Value =
        toml::from_str(&manifest_text).map_err(|source| MetadataError::Manifest {
            path: product.manifest.into(),
            source,
        })?;
    Ok(document["package"]["version"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

fn copy(source: &Path, destination: &Path) -> Result<(), MetadataError> {
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|source_error| MetadataError::Copy {
            path: source.display().to_string(),
            source: source_error,
        })
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), MetadataError> {
    for entry in fs::read_dir(source).map_err(|source_error| MetadataError::Copy {
        path: source.display().to_string(),
        source: source_error,
    })? {
        let entry = entry.map_err(|source_error| MetadataError::Copy {
            path: source.display().to_string(),
            source: source_error,
        })?;
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            fs::create_dir_all(&target).map_err(|source_error| MetadataError::Copy {
                path: target.display().to_string(),
                source: source_error,
            })?;
            copy_tree(&entry.path(), &target)?;
        } else {
            copy(&entry.path(), &target)?;
        }
    }
    Ok(())
}

fn read(root: &Path, relative: &str) -> Result<String, MetadataError> {
    fs::read_to_string(root.join(relative)).map_err(|source| MetadataError::Read {
        path: relative.into(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for package in ["colosseum-gui", "colosseum-cli"] {
            fs::create_dir_all(root.path().join("crates").join(package)).unwrap();
            fs::write(
                root.path().join("crates").join(package).join("Cargo.toml"),
                format!("[package]\nname = \"{package}\"\nversion = \"1.2.3\"\n"),
            )
            .unwrap();
        }
        fs::write(root.path().join("CHANGELOG-GUI.md"), "## [1.2.3]\n").unwrap();
        fs::write(root.path().join("CHANGELOG-CLI.md"), "## [1.2.3]\n").unwrap();
        root
    }

    #[test]
    fn routes_both_product_tags() {
        let root = fixture();
        let gui = validate(root.path(), "gui-v1.2.3").unwrap();
        let cli = validate(root.path(), "cli-v1.2.3").unwrap();
        assert_eq!(gui.package, "colosseum-gui");
        assert_eq!(gui.artifact_stem, "colosseum-gui-1.2.3");
        assert_eq!(cli.package, "colosseum-cli");
        assert_eq!(cli.artifact_stem, "colosseum-cli-1.2.3");
        assert_eq!(cli.validation, "release");
    }

    /// One naming scheme for both products. The tokens are the artifact's,
    /// not the triple's: a download called `…-windows-x64.zip` says what it
    /// runs on, where `…-x86_64-pc-windows-msvc` says how it was compiled.
    #[test]
    fn every_released_triple_maps_to_the_one_naming_scheme() {
        for (target, expected) in [
            ("x86_64-pc-windows-msvc", ("windows", "x64")),
            ("aarch64-pc-windows-msvc", ("windows", "arm64")),
            ("x86_64-unknown-linux-gnu", ("linux", "x64")),
            ("aarch64-unknown-linux-gnu", ("linux", "arm64")),
            ("x86_64-apple-darwin", ("macos", "x64")),
            ("aarch64-apple-darwin", ("macos", "arm64")),
        ] {
            assert_eq!(target_platform(target).unwrap(), expected, "{target}");
        }
        for unsupported in ["i686-pc-windows-msvc", "x86_64-unknown-freebsd", "nonsense"] {
            assert!(matches!(
                target_platform(unsupported),
                Err(MetadataError::Target(_))
            ));
        }
    }

    /// The scheme is the contract, so the tokens it replaced are refused
    /// rather than quietly staged under a name nothing else expects.
    #[test]
    fn staging_refuses_a_pair_outside_the_released_matrix() {
        let root = fixture();
        let binary = root.path().join("built-cli");
        fs::write(&binary, "binary").unwrap();
        let output = root.path().join("dist");
        for (platform, arch) in [
            ("linux", "x86_64"),
            ("macos", "aarch64"),
            ("windows", "x86_64"),
            ("freebsd", "x64"),
        ] {
            assert!(
                matches!(
                    stage_cli(root.path(), "1.2.3", platform, arch, &binary, &output),
                    Err(MetadataError::Platform { .. })
                ),
                "{platform}/{arch} must be refused"
            );
        }
    }

    #[test]
    fn rejects_wrong_version_and_build_metadata() {
        let root = fixture();
        assert!(matches!(
            validate(root.path(), "gui-v1.2.4"),
            Err(MetadataError::VersionMismatch { .. })
        ));
        assert!(matches!(
            validate(root.path(), "cli-v1.2.3+local"),
            Err(MetadataError::BuildMetadata)
        ));
        assert!(matches!(
            validate(root.path(), "cli-v1.2.3-rc.1"),
            Err(MetadataError::CliPrerelease)
        ));
    }

    #[test]
    fn never_borrows_notes_from_the_other_lane() {
        let root = fixture();
        fs::write(root.path().join("CHANGELOG-CLI.md"), "## [Unreleased]\n").unwrap();
        assert!(matches!(
            validate(root.path(), "cli-v1.2.3"),
            Err(MetadataError::MissingNotes { .. })
        ));
    }

    #[test]
    fn candidate_uses_manifest_without_claiming_release_readiness() {
        let root = fixture();
        fs::write(root.path().join("CHANGELOG-CLI.md"), "## [Unreleased]\n").unwrap();
        let cli = candidate(root.path(), "cli").unwrap();
        assert_eq!(cli.validation, "candidate");
        assert_eq!(cli.tag, "cli-v1.2.3");
        assert!(matches!(
            candidate(root.path(), "other"),
            Err(MetadataError::Product)
        ));
    }

    #[test]
    fn extracts_only_the_matching_product_notes() {
        let root = fixture();
        fs::write(
            root.path().join("CHANGELOG-CLI.md"),
            "# CLI\n\n## [Unreleased]\n\nFuture.\n\n## [1.2.3]\n\nCLI notes.\n\n## [1.2.2]\n\nOld.\n",
        )
        .unwrap();
        assert_eq!(
            release_notes(root.path(), "cli-v1.2.3").unwrap(),
            "CLI notes.\n"
        );
    }

    #[test]
    fn notes_end_before_the_rule_that_separates_versions() {
        let root = fixture();
        fs::write(
            root.path().join("CHANGELOG-GUI.md"),
            "# GUI\n\n---\n\n## [1.2.3] - 2026-01-01\n\nGUI notes.\n\n---\n\n## [1.2.2]\n\nOld.\n",
        )
        .unwrap();
        assert_eq!(
            release_notes(root.path(), "gui-v1.2.3").unwrap(),
            "GUI notes.\n"
        );
    }

    #[test]
    fn stages_only_cli_binary_license_front_door_and_offline_docs() {
        let root = fixture();
        fs::write(root.path().join("LICENSE"), "license").unwrap();
        fs::create_dir_all(root.path().join("packaging/cli")).unwrap();
        fs::write(root.path().join("packaging/cli/README.md"), "cli readme").unwrap();
        fs::write(root.path().join("CHANGELOG-CLI.md"), "cli changes").unwrap();
        fs::create_dir_all(root.path().join("docs/cli/formats")).unwrap();
        fs::write(root.path().join("docs/cli/quickstart.md"), "quick").unwrap();
        fs::write(root.path().join("docs/cli/formats/run.md"), "run").unwrap();
        let binary = root.path().join("built-cli");
        fs::write(&binary, "binary").unwrap();
        let output = root.path().join("dist");
        let stage = stage_cli(root.path(), "1.2.3", "linux", "x64", &binary, &output).unwrap();
        assert_eq!(
            fs::read_to_string(stage.join("colosseum-cli")).unwrap(),
            "binary"
        );
        assert!(stage.join("LICENSE").is_file());
        assert_eq!(
            fs::read_to_string(stage.join("README.md")).unwrap(),
            "cli readme"
        );
        assert_eq!(
            fs::read_to_string(stage.join("CHANGELOG-CLI.md")).unwrap(),
            "cli changes"
        );
        assert!(stage.join("docs/cli/quickstart.md").is_file());
        assert!(stage.join("docs/cli/formats/run.md").is_file());
        assert!(matches!(
            stage_cli(root.path(), "1.2.3", "linux", "x64", &binary, &output),
            Err(MetadataError::StageExists(_))
        ));
    }
}
