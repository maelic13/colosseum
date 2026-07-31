use std::fs;
use std::path::Path;

use semver::Version;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReleaseMetadata {
    pub product: &'static str,
    pub package: &'static str,
    pub version: String,
    pub tag: String,
    pub changelog: &'static str,
    pub artifact_stem: String,
    pub prerelease: bool,
}

#[derive(Debug, Error)]
pub enum MetadataError {
    #[error("tag must start with gui-v or cli-v")]
    TagPrefix,
    #[error("invalid release semantic version: {0}")]
    Semver(#[from] semver::Error),
    #[error("release versions must not contain build metadata")]
    BuildMetadata,
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
}

pub fn validate(root: &Path, tag: &str) -> Result<ReleaseMetadata, MetadataError> {
    let (product, package, manifest, changelog, other_changelog, artifact_name, raw_version) =
        if let Some(version) = tag.strip_prefix("gui-v") {
            (
                "gui",
                "colosseum-gui",
                "crates/colosseum-gui/Cargo.toml",
                "CHANGELOG-GUI.md",
                "CHANGELOG-CLI.md",
                "colosseum",
                version,
            )
        } else if let Some(version) = tag.strip_prefix("cli-v") {
            (
                "cli",
                "colosseum-cli",
                "crates/colosseum-cli/Cargo.toml",
                "CHANGELOG-CLI.md",
                "CHANGELOG-GUI.md",
                "colosseum-cli",
                version,
            )
        } else {
            return Err(MetadataError::TagPrefix);
        };

    let version = Version::parse(raw_version)?;
    if !version.build.is_empty() {
        return Err(MetadataError::BuildMetadata);
    }
    let manifest_text = read(root, manifest)?;
    let document: toml::Value =
        toml::from_str(&manifest_text).map_err(|source| MetadataError::Manifest {
            path: manifest.into(),
            source,
        })?;
    let actual = document["package"]["version"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    if actual != version.to_string() {
        return Err(MetadataError::VersionMismatch {
            package,
            actual,
            requested: version,
        });
    }

    let heading = format!("## [{}]", version);
    let notes = read(root, changelog)?;
    if !notes.lines().any(|line| line.trim().starts_with(&heading)) {
        // Read the other lane too: this catches a missing/misfiled notes file
        // without ever accepting it as the requested product's notes.
        let _misfiled = read(root, other_changelog)?
            .lines()
            .any(|line| line.trim().starts_with(&heading));
        return Err(MetadataError::MissingNotes { changelog, version });
    }

    Ok(ReleaseMetadata {
        product,
        package,
        version: version.to_string(),
        tag: tag.to_owned(),
        changelog,
        artifact_stem: format!("{artifact_name}-{version}"),
        prerelease: !version.pre.is_empty(),
    })
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
        assert_eq!(gui.artifact_stem, "colosseum-1.2.3");
        assert_eq!(cli.package, "colosseum-cli");
        assert_eq!(cli.artifact_stem, "colosseum-cli-1.2.3");
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
}
