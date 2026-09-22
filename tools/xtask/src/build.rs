//! `cargo xtask build <gui|cli>` — compile exactly one product.

use std::path::{Path, PathBuf};

use crate::product::Product;
use crate::util::{Result, run};

/// Compile one product for one target and return the binary's path.
///
/// `--target` is always passed, even for the host, so the output is at
/// `target/<triple>/<profile>/` whatever the host is and no caller needs two
/// cases. `--locked` is not optional: a release artifact is built from the
/// committed lockfile or it is not a release artifact.
pub fn build(root: &Path, product: Product, target: &str, profile: &str) -> Result<PathBuf> {
    run(
        "cargo",
        &[
            "build",
            "--locked",
            "--profile",
            profile,
            "-p",
            product.package(),
            "--bin",
            product.binary(),
            "--target",
            target,
        ],
        root,
    )?;
    let binary = binary_path(root, product, target, profile);
    if !binary.is_file() {
        return Err(format!(
            "cargo reported success but {} does not exist",
            binary.display()
        ));
    }
    Ok(binary)
}

/// Where cargo puts the product's binary for this target and profile.
#[must_use]
pub fn binary_path(root: &Path, product: Product, target: &str, profile: &str) -> PathBuf {
    let suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    root.join("target")
        .join(target)
        .join(profile)
        .join(format!("{}{suffix}", product.binary()))
}
