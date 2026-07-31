use std::path::PathBuf;

fn main() {
    let mut arguments = std::env::args_os().skip(1);
    let Some(tag) = arguments.next() else {
        eprintln!("usage: colosseum-release <gui-vSEMVER|cli-vSEMVER> [root]");
        std::process::exit(2);
    };
    let root = arguments.next().map_or_else(
        || std::env::current_dir().expect("current directory"),
        PathBuf::from,
    );
    if arguments.next().is_some() {
        eprintln!("too many arguments");
        std::process::exit(2);
    }
    let tag = tag.to_string_lossy();
    match colosseum_release::validate(&root, &tag) {
        Ok(metadata) => println!("{}", serde_json::to_string(&metadata).unwrap()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
