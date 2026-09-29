//! The product this repository builds, and what it is packaged as.

use std::fmt;

use crate::util::Result;

/// The product. One variant since the desktop application left for its own
/// repository; the positional `cli` argument stays so the release workflow
/// and every documented command keep working unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Product {
    Cli,
}

impl Product {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "cli" => Ok(Self::Cli),
            other => Err(format!("unknown product `{other}`; expected cli")),
        }
    }

    /// The cargo package that owns the product's version.
    #[must_use]
    pub const fn package(self) -> &'static str {
        match self {
            Self::Cli => "colosseum-cli",
        }
    }

    /// The one binary the product ships. Building by `--bin` as well as `-p`
    /// keeps a package that grows a second binary from silently shipping it.
    #[must_use]
    pub const fn binary(self) -> &'static str {
        match self {
            Self::Cli => "colosseum-cli",
        }
    }

    /// The name `colosseum-release` knows this product by.
    #[must_use]
    pub const fn release_name(self) -> &'static str {
        match self {
            Self::Cli => "cli",
        }
    }

    /// The archive smoke script that reads a finished artifact back.
    #[must_use]
    pub const fn smoke_script(self) -> &'static str {
        match self {
            Self::Cli => "tools/release/Smoke-CliArchive.ps1",
        }
    }
}

impl fmt::Display for Product {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.release_name())
    }
}

/// One packaged output: the portable archive of a platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zip,
    TarGz,
}

impl Format {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "zip" => Ok(Self::Zip),
            "tar.gz" => Ok(Self::TarGz),
            other => Err(format!(
                "unknown format `{other}`; expected one of zip, tar.gz"
            )),
        }
    }

    /// The artifact's file extension, which is also its name here.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
        }
    }

    /// The portable archive of a platform: what `--format` defaults to.
    #[must_use]
    pub fn portable_for(platform: &str) -> Self {
        if platform == "windows" {
            Self::Zip
        } else {
            Self::TarGz
        }
    }

    /// Whether the product on a platform is packaged this way.
    ///
    /// The CLI ships portable archives only — it is one executable with its
    /// documentation, and an installer would put a developer tool on a
    /// system path nobody asked for.
    #[must_use]
    pub fn applies_to(self, product: Product, platform: &str, _arch: &str) -> bool {
        match (product, self) {
            (Product::Cli, Self::Zip) => platform == "windows",
            (Product::Cli, Self::TarGz) => platform != "windows",
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.extension())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cli_ships_only_the_platform_portable_archive() {
        assert!(Format::Zip.applies_to(Product::Cli, "windows", "x64"));
        assert!(!Format::TarGz.applies_to(Product::Cli, "windows", "x64"));
        assert!(Format::TarGz.applies_to(Product::Cli, "linux", "x64"));
        assert!(Format::TarGz.applies_to(Product::Cli, "macos", "arm64"));
        assert!(!Format::Zip.applies_to(Product::Cli, "linux", "x64"));
    }

    /// The workflow calls `package`, and this is where the product is tied to
    /// the script that reads its archive back.
    #[test]
    fn the_product_builds_its_own_binary_and_smokes_its_own_archive() {
        assert_eq!(Product::Cli.package(), "colosseum-cli");
        assert_eq!(Product::Cli.binary(), "colosseum-cli");
        assert_eq!(
            Product::Cli.smoke_script(),
            "tools/release/Smoke-CliArchive.ps1"
        );
        let root = crate::util::repository_root();
        assert!(
            root.join(Product::Cli.smoke_script()).is_file(),
            "the smoke script does not exist"
        );
    }

    #[test]
    fn a_platform_gets_exactly_one_portable_archive() {
        assert_eq!(Format::portable_for("windows"), Format::Zip);
        assert_eq!(Format::portable_for("linux"), Format::TarGz);
        assert_eq!(Format::portable_for("macos"), Format::TarGz);
    }
}
