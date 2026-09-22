//! Shared helpers: the repository root, running a tool, and hashing a file.

use std::path::{Path, PathBuf};
use std::process::Command;

pub type Result<T> = std::result::Result<T, String>;

/// The repository root, resolved from this package's own manifest directory so
/// every command behaves the same whatever the current directory is.
#[must_use]
pub fn repository_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or(manifest)
}

/// The triple this host compiles for by default, read from the toolchain
/// rather than guessed, so `--target` is always explicit further down.
pub fn host_target() -> Result<String> {
    let output = Command::new("rustc")
        .arg("-vV")
        .output()
        .map_err(|error| format!("could not run rustc: {error}"))?;
    if !output.status.success() {
        return Err("rustc -vV failed".into());
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .map(str::trim)
        .map(str::to_owned)
        .ok_or_else(|| "rustc -vV printed no host triple".into())
}

/// Run a tool to completion, inheriting its output, and fail loudly.
///
/// A missing tool is an error naming it, never a skip: a package that quietly
/// produces fewer files than asked for is how a release loses an installer.
pub fn run(program: &str, arguments: &[&str], working_directory: &Path) -> Result<()> {
    println!("  $ {program} {}", arguments.join(" "));
    let status = Command::new(program)
        .args(arguments)
        .current_dir(working_directory)
        .status()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                format!("`{program}` is not installed or not on PATH, and this format needs it")
            } else {
                format!("could not run `{program}`: {error}")
            }
        })?;
    if !status.success() {
        return Err(format!(
            "`{program} {}` failed with {status}",
            arguments.join(" ")
        ));
    }
    Ok(())
}

/// Run a tool and capture its standard output.
pub fn capture(program: &str, arguments: &[&str], working_directory: &Path) -> Result<String> {
    let output = Command::new(program)
        .args(arguments)
        .current_dir(working_directory)
        .output()
        .map_err(|error| format!("could not run `{program}`: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "`{program} {}` failed with {}:\n{}",
            arguments.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The SHA-256 of a finished artifact, printed beside it so a local archive
/// can be compared with a published one without a second tool.
pub fn sha256(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

/// PowerShell, which the archive smoke scripts are written in.
pub fn powershell() -> Result<&'static str> {
    for candidate in ["pwsh", "powershell"] {
        if Command::new(candidate)
            .arg("-NoProfile")
            .arg("-Command")
            .arg("exit 0")
            .status()
            .is_ok()
        {
            return Ok(candidate);
        }
    }
    Err("PowerShell (`pwsh`) is required to smoke an archive; \
         install it or pass --no-smoke"
        .into())
}
