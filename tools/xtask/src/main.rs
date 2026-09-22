//! One entry point for building, packaging and release-checking Colosseum.
//!
//! ```text
//! cargo xtask build   <gui|cli> [--target <triple>] [--profile release|ci-release]
//! cargo xtask package <gui|cli> [--target <triple>] [--format <list>] [--no-smoke]
//! cargo xtask release-check <gui-vX.Y.Z|cli-vX.Y.Z>
//! ```
//!
//! The two products stay separate: no command builds or packages both, and the
//! CLI archive never contains the GUI. Both release workflows call these
//! commands, so a local archive and a published one come from one recipe.

mod archive;
mod build;
mod check;
mod package;
mod product;
mod util;

use std::process::ExitCode;

use product::{Format, Product};
use util::{Result, repository_root};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let root = repository_root();
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().unwrap_or_default();
    let rest: Vec<String> = arguments.collect();

    match command.as_str() {
        "build" => {
            let options = Options::parse(&rest, &["--target", "--profile"])?;
            let product = options.product()?;
            let target = options.target()?;
            let profile = match options.value("--profile").unwrap_or("release") {
                profile @ ("release" | "ci-release") => profile.to_owned(),
                other => {
                    return Err(format!(
                        "unknown profile `{other}`; expected release or ci-release"
                    ));
                }
            };
            let binary = build::build(&root, product, &target, &profile)?;
            println!("{}", binary.display());
            Ok(())
        }
        "package" => {
            let options = Options::parse(&rest, &["--target", "--format"])?;
            let product = options.product()?;
            let target = options.target()?;
            // Resolved here only to pick the default format; `package` checks
            // the whole request against the product and the platform.
            let (platform, _) =
                colosseum_release::target_platform(&target).map_err(|error| error.to_string())?;
            let formats = match options.value("--format") {
                Some(list) => list
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(Format::parse)
                    .collect::<Result<Vec<_>>>()?,
                None => vec![Format::portable_for(platform)],
            };
            if formats.is_empty() {
                return Err("--format listed no formats".into());
            }
            let produced = package::package(
                &root,
                &package::Request {
                    product,
                    target,
                    formats,
                    smoke: !options.flag("--no-smoke"),
                },
            )?;
            println!("{} artifact(s) in target/dist", produced.len());
            Ok(())
        }
        "release-check" => {
            let tag = rest
                .first()
                .ok_or("release-check needs a tag, for example gui-v1.1.0")?;
            check::release_check(&root, tag)
        }
        "help" | "--help" | "-h" => {
            print_usage();
            Ok(())
        }
        "" => {
            print_usage();
            Err("no command given".into())
        }
        other => {
            print_usage();
            Err(format!("unknown command `{other}`"))
        }
    }
}

fn print_usage() {
    println!(
        "Usage:
  cargo xtask build   <gui|cli> [--target <triple>] [--profile release|ci-release]
  cargo xtask package <gui|cli> [--target <triple>] [--format <list>] [--no-smoke]
  cargo xtask release-check <gui-vX.Y.Z|cli-vX.Y.Z>

The product is a positional argument, and one command handles one product.
`--target` defaults to this host's triple and is always passed to cargo, so
every build lands under target/<triple>/<profile>/. Builds are `--locked`.

`build` compiles and prints the binary's path; it copies nothing.
`package` always uses the release profile, writes artifacts to target/dist/
named <product>-<version>-<platform>-<arch>.<ext>, prints each one's SHA-256,
and reads the portable archive back with its smoke script unless --no-smoke.

--format takes a comma-separated list; it defaults to the platform's portable
archive (zip on Windows, tar.gz elsewhere). Available:
  gui   zip | tar.gz | msi | deb | rpm | dmg | pkg.tar.zst
  cli   zip | tar.gz
A format whose tool is not installed is an error, never a skip.

Examples:
  cargo xtask build cli
  cargo xtask package gui --format zip,msi
  cargo xtask package cli --target aarch64-pc-windows-msvc
  cargo xtask release-check cli-v0.1.0"
    );
}

/// The positional product plus the `--name value` options a command accepts.
struct Options {
    product: Option<String>,
    values: Vec<(String, String)>,
    flags: Vec<String>,
}

impl Options {
    fn parse(arguments: &[String], accepted: &[&str]) -> Result<Self> {
        let mut parsed = Self {
            product: None,
            values: Vec::new(),
            flags: Vec::new(),
        };
        let mut index = 0;
        while index < arguments.len() {
            let argument = &arguments[index];
            if let Some(name) = accepted.iter().find(|name| *name == argument) {
                let value = arguments
                    .get(index + 1)
                    .ok_or_else(|| format!("{name} needs a value"))?;
                parsed.values.push(((*name).to_owned(), value.clone()));
                index += 2;
            } else if argument == "--no-smoke" {
                parsed.flags.push(argument.clone());
                index += 1;
            } else if argument.starts_with('-') {
                return Err(format!("unknown option `{argument}`"));
            } else if parsed.product.is_none() {
                parsed.product = Some(argument.clone());
                index += 1;
            } else {
                return Err(format!("unexpected argument `{argument}`"));
            }
        }
        Ok(parsed)
    }

    fn product(&self) -> Result<Product> {
        let name = self
            .product
            .as_deref()
            .ok_or("this command needs a product: gui or cli")?;
        Product::parse(name)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|flag| flag == name)
    }

    fn target(&self) -> Result<String> {
        match self.value("--target") {
            Some(target) => Ok(target.to_owned()),
            None => util::host_target(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn the_product_is_positional_and_options_are_named() {
        let parsed = Options::parse(
            &arguments(&["gui", "--target", "x86_64-pc-windows-msvc", "--no-smoke"]),
            &["--target", "--format"],
        )
        .unwrap();
        assert_eq!(parsed.product().unwrap(), Product::Gui);
        assert_eq!(parsed.value("--target"), Some("x86_64-pc-windows-msvc"));
        assert!(parsed.flag("--no-smoke"));
    }

    #[test]
    fn a_mistyped_option_or_a_missing_product_is_refused() {
        assert!(Options::parse(&arguments(&["cli", "--targets", "x"]), &["--target"]).is_err());
        assert!(Options::parse(&arguments(&["cli", "--target"]), &["--target"]).is_err());
        assert!(Options::parse(&arguments(&["cli", "gui"]), &["--target"]).is_err());
        assert!(
            Options::parse(&arguments(&[]), &["--target"])
                .unwrap()
                .product()
                .is_err()
        );
        assert!(Product::parse("both").is_err());
    }

    #[test]
    fn a_format_list_is_parsed_and_unknown_entries_are_named() {
        let formats: Result<Vec<Format>> = "zip, msi"
            .split(',')
            .map(str::trim)
            .map(Format::parse)
            .collect();
        assert_eq!(formats.unwrap(), vec![Format::Zip, Format::Msi]);
        assert!(Format::parse("tar.bz2").is_err());
    }
}
