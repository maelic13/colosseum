use std::path::PathBuf;

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let Some(action) = arguments.next() else {
        usage();
        std::process::exit(2);
    };
    let action = action.to_string_lossy();
    let result = match action.as_ref() {
        "candidate" => {
            let Some(product) = arguments.next() else {
                return fail_usage();
            };
            let root = optional_root(&mut arguments);
            ensure_done(&mut arguments);
            colosseum_release::candidate(&root, &product.to_string_lossy())
                .map(|metadata| serde_json::to_string(&metadata).unwrap())
        }
        "notes" => {
            let Some(tag) = arguments.next() else {
                return fail_usage();
            };
            let Some(output) = arguments.next() else {
                return fail_usage();
            };
            let root = optional_root(&mut arguments);
            ensure_done(&mut arguments);
            colosseum_release::release_notes(&root, &tag.to_string_lossy()).and_then(|notes| {
                std::fs::write(&output, notes).map_err(|source| {
                    colosseum_release::MetadataError::Copy {
                        path: PathBuf::from(&output).display().to_string(),
                        source,
                    }
                })?;
                Ok(PathBuf::from(output).display().to_string())
            })
        }
        "stage-cli" => {
            let Some(version) = arguments.next() else {
                return fail_usage();
            };
            let Some(platform) = arguments.next() else {
                return fail_usage();
            };
            let Some(arch) = arguments.next() else {
                return fail_usage();
            };
            let Some(binary) = arguments.next() else {
                return fail_usage();
            };
            let Some(output) = arguments.next() else {
                return fail_usage();
            };
            let root = optional_root(&mut arguments);
            ensure_done(&mut arguments);
            colosseum_release::stage_cli(
                &root,
                &version.to_string_lossy(),
                &platform.to_string_lossy(),
                &arch.to_string_lossy(),
                &PathBuf::from(binary),
                &PathBuf::from(output),
            )
            .map(|path| path.display().to_string())
        }
        tag => {
            let root = optional_root(&mut arguments);
            ensure_done(&mut arguments);
            colosseum_release::validate(&root, tag)
                .map(|metadata| serde_json::to_string(&metadata).unwrap())
        }
    };
    match result {
        Ok(output) => println!("{output}"),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

fn optional_root(arguments: &mut impl Iterator<Item = std::ffi::OsString>) -> PathBuf {
    arguments.next().map_or_else(
        || std::env::current_dir().expect("current directory"),
        PathBuf::from,
    )
}

fn ensure_done(arguments: &mut impl Iterator<Item = std::ffi::OsString>) {
    if arguments.next().is_some() {
        fail_usage();
    }
}

fn fail_usage() {
    usage();
    std::process::exit(2);
}

fn usage() {
    eprintln!("usage: colosseum-release <gui-vSEMVER|cli-vSEMVER> [root]");
    eprintln!("       colosseum-release candidate <gui|cli> [root]");
    eprintln!("       colosseum-release notes <tag> <output> [root]");
    eprintln!(
        "       colosseum-release stage-cli <version> <platform> <arch> <binary> <output> [root]"
    );
}
