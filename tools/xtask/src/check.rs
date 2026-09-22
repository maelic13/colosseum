//! `cargo xtask release-check <tag>` — everything that must hold before a tag.

use std::path::Path;

use crate::util::{Result, run};

/// Hold a tag to the repository it claims to describe.
///
/// The release tool answers the version questions — the tag's prefix and
/// shape, that the product manifest carries exactly that version, and that the
/// product's changelog has a section for it. The rest is drift: a generated
/// command reference that no longer matches the parser, and whitespace damage
/// a diff would carry into the tag.
pub fn release_check(root: &Path, tag: &str) -> Result<()> {
    let metadata = colosseum_release::validate(root, tag).map_err(|error| error.to_string())?;
    println!(
        "{tag}: {} {} — manifest and {} agree",
        metadata.package, metadata.version, metadata.changelog
    );

    run(
        "cargo",
        &[
            "run",
            "--locked",
            "-q",
            "-p",
            "colosseum-docs",
            "--",
            "--check",
        ],
        root,
    )?;
    run("git", &["diff", "--check"], root)?;

    println!("release check passed for {tag}");
    Ok(())
}
