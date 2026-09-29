//! `cargo xtask package cli` — the release artifacts, from one recipe.
//!
//! Everything a release publishes is produced here, so a local archive and a
//! published one come from the same code: the release workflow calls these
//! commands rather than repeating the staging in YAML. Artifacts land in
//! `target/dist/`, named `<product>-<version>-<platform>-<arch>.<ext>`, and
//! each one's SHA-256 is printed beside it.

use std::path::{Path, PathBuf};

use crate::archive;
use crate::build;
use crate::product::{Format, Product};
use crate::util::{Result, powershell, run, sha256};

/// Everything one `package` invocation needs to know.
pub struct Request {
    pub product: Product,
    pub target: String,
    pub formats: Vec<Format>,
    pub smoke: bool,
}

pub fn package(root: &Path, request: &Request) -> Result<Vec<PathBuf>> {
    let (platform, arch) =
        colosseum_release::target_platform(&request.target).map_err(|error| error.to_string())?;
    let metadata = colosseum_release::candidate(root, request.product.release_name())
        .map_err(|error| error.to_string())?;
    let version = metadata.version;
    // `<product>-<version>` comes from the release tool, which reads the
    // product's own manifest — never the workspace's, which has no version.
    let name = format!("{}-{platform}-{arch}", metadata.artifact_stem);

    for format in &request.formats {
        if !format.applies_to(request.product, platform, arch) {
            return Err(format!(
                "the {} cannot be packaged as {format} on {platform}-{arch}",
                request.product
            ));
        }
    }

    // `package` always ships the release profile: an artifact built any other
    // way is not the artifact.
    let binary = build::build(root, request.product, &request.target, "release")?;

    let dist = root.join("target").join("dist");
    std::fs::create_dir_all(&dist)
        .map_err(|error| format!("could not create {}: {error}", dist.display()))?;

    let mut produced = Vec::new();
    for &format in &request.formats {
        let destination = dist.join(format!("{name}.{}", format.extension()));
        if destination.exists() {
            std::fs::remove_file(&destination)
                .map_err(|error| format!("could not replace {}: {error}", destination.display()))?;
        }
        println!("packaging {}", destination.display());
        let stage = stage(root, request.product, &version, platform, arch, &binary)?;
        match format {
            Format::Zip => archive::zip(&stage, &name, &destination)?,
            Format::TarGz => archive::tar_gz(&stage, &name, &destination)?,
        }
        if !destination.is_file() {
            return Err(format!("{} was not produced", destination.display()));
        }
        println!("  {}  {}", sha256(&destination)?, destination.display());
        produced.push(destination);
    }

    if request.smoke {
        for artifact in &produced {
            smoke(root, request.product, artifact, &version, platform, arch)?;
        }
    }

    // `target/dist` is what a workflow uploads, so it holds artifacts and
    // nothing else once staging is done.
    let staging = dist.join("staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .map_err(|error| format!("could not clear {}: {error}", staging.display()))?;
    }
    Ok(produced)
}

/// The allowlisted directory that becomes the portable archive: the release
/// tool's business — binary, licence, front door and the version-matched
/// guide.
fn stage(
    root: &Path,
    product: Product,
    version: &str,
    platform: &str,
    arch: &str,
    binary: &Path,
) -> Result<PathBuf> {
    let staging = root.join("target").join("dist").join("staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .map_err(|error| format!("could not clear {}: {error}", staging.display()))?;
    }
    std::fs::create_dir_all(&staging)
        .map_err(|error| format!("could not create {}: {error}", staging.display()))?;

    match product {
        Product::Cli => {
            colosseum_release::stage_cli(root, version, platform, arch, binary, &staging)
                .map_err(|error| error.to_string())
        }
    }
}

/// Read the finished artifact back the way a user would.
fn smoke(
    root: &Path,
    product: Product,
    archive: &Path,
    version: &str,
    platform: &str,
    arch: &str,
) -> Result<()> {
    println!("smoking {}", archive.display());
    run(
        powershell()?,
        &[
            "-NoProfile",
            "-File",
            product.smoke_script(),
            "-Archive",
            &archive.display().to_string(),
            "-Version",
            version,
            "-Platform",
            platform,
            "-Architecture",
            arch,
        ],
        root,
    )
}
