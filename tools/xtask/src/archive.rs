//! The two portable archive formats, written from the staged directory.
//!
//! Both are produced here rather than by a shelled-out tool, so a zip built on
//! a Linux host and one built on Windows have the same entries, and so
//! `package` needs nothing installed to make the archive a user downloads.
//! Each archive holds exactly one top-level directory, named after the
//! archive, which is what the smoke scripts check first.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::util::Result;

/// Every file under `stage`, as (path on disk, path inside the archive).
fn entries(stage: &Path, root_name: &str) -> Result<Vec<(PathBuf, String)>> {
    let mut found = Vec::new();
    let mut pending = vec![(stage.to_path_buf(), root_name.to_owned())];
    while let Some((directory, prefix)) = pending.pop() {
        let listing = std::fs::read_dir(&directory)
            .map_err(|error| format!("could not read {}: {error}", directory.display()))?;
        for entry in listing {
            let entry = entry
                .map_err(|error| format!("could not read {}: {error}", directory.display()))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let inside = format!("{prefix}/{name}");
            if entry.path().is_dir() {
                pending.push((entry.path(), inside));
            } else {
                found.push((entry.path(), inside));
            }
        }
    }
    // Deterministic order, so two archives of the same tree are comparable.
    found.sort_by(|left, right| left.1.cmp(&right.1));
    Ok(found)
}

/// Whether a staged file is the product's executable, which must stay
/// executable inside a tar built on any host.
fn is_executable(inside: &str) -> bool {
    let name = inside.rsplit('/').next().unwrap_or(inside);
    matches!(
        name,
        "colosseum" | "colosseum-cli" | "colosseum.exe" | "colosseum-cli.exe"
    )
}

pub fn zip(stage: &Path, root_name: &str, destination: &Path) -> Result<()> {
    let file = File::create(destination)
        .map_err(|error| format!("could not create {}: {error}", destination.display()))?;
    let mut writer = zip::ZipWriter::new(BufWriter::new(file));
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    writer
        .add_directory(root_name, options)
        .map_err(|error| format!("could not write the archive root: {error}"))?;
    for (path, inside) in entries(stage, root_name)? {
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        writer
            .start_file(&inside, options)
            .map_err(|error| format!("could not add {inside}: {error}"))?;
        writer
            .write_all(&bytes)
            .map_err(|error| format!("could not write {inside}: {error}"))?;
    }
    writer
        .finish()
        .map_err(|error| format!("could not finish {}: {error}", destination.display()))?;
    Ok(())
}

pub fn tar_gz(stage: &Path, root_name: &str, destination: &Path) -> Result<()> {
    let file = File::create(destination)
        .map_err(|error| format!("could not create {}: {error}", destination.display()))?;
    let encoder = flate2::write::GzEncoder::new(BufWriter::new(file), flate2::Compression::best());
    let mut builder = tar::Builder::new(encoder);
    for (path, inside) in entries(stage, root_name)? {
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        // Modes are set explicitly rather than copied from disk: the host that
        // builds a tarball may be Windows, which has no executable bit, and a
        // downloaded `colosseum` that cannot be run is not a release.
        header.set_mode(if is_executable(&inside) { 0o755 } else { 0o644 });
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append_data(&mut header, &inside, bytes.as_slice())
            .map_err(|error| format!("could not add {inside}: {error}"))?;
    }
    let encoder = builder
        .into_inner()
        .map_err(|error| format!("could not finish {}: {error}", destination.display()))?;
    let mut file = encoder
        .finish()
        .map_err(|error| format!("could not compress {}: {error}", destination.display()))?;
    file.flush()
        .map_err(|error| format!("could not flush {}: {error}", destination.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let stage = root.path().join("colosseum-cli-1.2.3-linux-x64");
        std::fs::create_dir_all(stage.join("docs/cli")).unwrap();
        std::fs::write(stage.join("colosseum-cli"), "binary").unwrap();
        std::fs::write(stage.join("LICENSE"), "licence").unwrap();
        std::fs::write(stage.join("docs/cli/README.md"), "docs").unwrap();
        (root, stage)
    }

    #[test]
    fn a_zip_holds_one_named_root_and_every_staged_file() {
        let (root, stage) = staged();
        let name = "colosseum-cli-1.2.3-linux-x64";
        let destination = root.path().join("out.zip");
        zip(&stage, name, &destination).unwrap();

        let reader = File::open(&destination).unwrap();
        let mut archive = zip::ZipArchive::new(reader).unwrap();
        let mut names: Vec<String> = (0..archive.len())
            .map(|index| archive.by_index(index).unwrap().name().to_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                format!("{name}/"),
                format!("{name}/LICENSE"),
                format!("{name}/colosseum-cli"),
                format!("{name}/docs/cli/README.md"),
            ]
        );
    }

    #[test]
    fn a_tarball_keeps_the_executable_bit_whatever_the_host_is() {
        let (root, stage) = staged();
        let name = "colosseum-cli-1.2.3-linux-x64";
        let destination = root.path().join("out.tar.gz");
        tar_gz(&stage, name, &destination).unwrap();

        let reader = flate2::read::GzDecoder::new(File::open(&destination).unwrap());
        let mut archive = tar::Archive::new(reader);
        let mut seen = Vec::new();
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            let path = entry.path().unwrap().to_string_lossy().into_owned();
            let mode = entry.header().mode().unwrap();
            seen.push((path, mode));
        }
        seen.sort();
        assert_eq!(
            seen,
            vec![
                (format!("{name}/LICENSE"), 0o644),
                (format!("{name}/colosseum-cli"), 0o755),
                (format!("{name}/docs/cli/README.md"), 0o644),
            ]
        );
    }
}
