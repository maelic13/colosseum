//! The two products, and what each one is built and packaged as.

use std::fmt;

use crate::util::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Product {
    Gui,
    Cli,
}

impl Product {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "gui" => Ok(Self::Gui),
            "cli" => Ok(Self::Cli),
            other => Err(format!("unknown product `{other}`; expected gui or cli")),
        }
    }

    /// The cargo package that owns the product's version.
    #[must_use]
    pub const fn package(self) -> &'static str {
        match self {
            Self::Gui => "colosseum-gui",
            Self::Cli => "colosseum-cli",
        }
    }

    /// The one binary the product ships. Building by `--bin` as well as `-p`
    /// keeps a package that grows a second binary from silently shipping it.
    #[must_use]
    pub const fn binary(self) -> &'static str {
        match self {
            Self::Gui => "colosseum",
            Self::Cli => "colosseum-cli",
        }
    }

    /// The name `colosseum-release` knows this product by.
    #[must_use]
    pub const fn release_name(self) -> &'static str {
        match self {
            Self::Gui => "gui",
            Self::Cli => "cli",
        }
    }

    /// The archive smoke script that reads a finished artifact back.
    #[must_use]
    pub const fn smoke_script(self) -> &'static str {
        match self {
            Self::Gui => "tools/release/Smoke-GuiArchive.ps1",
            Self::Cli => "tools/release/Smoke-CliArchive.ps1",
        }
    }
}

impl fmt::Display for Product {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.release_name())
    }
}

/// One packaged output. The portable archives carry the product as a directory
/// tree; the rest are the platform's own installer formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Zip,
    TarGz,
    Msi,
    Deb,
    Rpm,
    Dmg,
    PkgTarZst,
}

impl Format {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "zip" => Ok(Self::Zip),
            "tar.gz" => Ok(Self::TarGz),
            "msi" => Ok(Self::Msi),
            "deb" => Ok(Self::Deb),
            "rpm" => Ok(Self::Rpm),
            "dmg" => Ok(Self::Dmg),
            "pkg.tar.zst" => Ok(Self::PkgTarZst),
            other => Err(format!(
                "unknown format `{other}`; expected one of \
                 zip, tar.gz, msi, deb, rpm, dmg, pkg.tar.zst"
            )),
        }
    }

    /// The artifact's file extension, which is also its name here.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::Msi => "msi",
            Self::Deb => "deb",
            Self::Rpm => "rpm",
            Self::Dmg => "dmg",
            Self::PkgTarZst => "pkg.tar.zst",
        }
    }

    /// Whether this is the portable archive the smoke script reads back.
    #[must_use]
    pub const fn is_portable_archive(self) -> bool {
        matches!(self, Self::Zip | Self::TarGz)
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

    /// Whether a product on a platform can be packaged this way at all.
    ///
    /// The CLI ships portable archives only — it is one executable with its
    /// documentation, and an installer would put a developer tool on a
    /// system path nobody asked for.
    #[must_use]
    pub fn applies_to(self, product: Product, platform: &str, arch: &str) -> bool {
        match (product, self) {
            (Product::Cli, Self::Zip) | (Product::Gui, Self::Zip | Self::Msi) => {
                platform == "windows"
            }
            (Product::Cli, Self::TarGz) => platform != "windows",
            (Product::Gui, Self::TarGz) => platform != "windows",
            (Product::Gui, Self::Deb | Self::Rpm) => platform == "linux",
            (Product::Gui, Self::PkgTarZst) => platform == "linux" && arch == "x64",
            (Product::Gui, Self::Dmg) => platform == "macos",
            (Product::Cli, _) => false,
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
        assert!(Format::TarGz.applies_to(Product::Cli, "linux", "x64"));
        assert!(Format::TarGz.applies_to(Product::Cli, "macos", "arm64"));
        for format in [
            Format::Msi,
            Format::Deb,
            Format::Rpm,
            Format::Dmg,
            Format::PkgTarZst,
        ] {
            assert!(
                !format.applies_to(Product::Cli, "windows", "x64")
                    && !format.applies_to(Product::Cli, "linux", "x64")
                    && !format.applies_to(Product::Cli, "macos", "arm64"),
                "{format} must not apply to the CLI"
            );
        }
    }

    #[test]
    fn each_gui_installer_belongs_to_one_platform() {
        assert!(Format::Msi.applies_to(Product::Gui, "windows", "arm64"));
        assert!(!Format::Msi.applies_to(Product::Gui, "linux", "x64"));
        assert!(Format::Deb.applies_to(Product::Gui, "linux", "x64"));
        assert!(!Format::Deb.applies_to(Product::Gui, "macos", "arm64"));
        assert!(Format::Dmg.applies_to(Product::Gui, "macos", "arm64"));
        assert!(!Format::Dmg.applies_to(Product::Gui, "windows", "x64"));
        // The Arch package is built in an x86-64 container only.
        assert!(Format::PkgTarZst.applies_to(Product::Gui, "linux", "x64"));
        assert!(!Format::PkgTarZst.applies_to(Product::Gui, "linux", "arm64"));
    }

    /// The workflows no longer name a smoke script; they call `package`, and
    /// this is where a product is tied to the script that reads its archive
    /// back. Crossing them would smoke the wrong product and still pass.
    #[test]
    fn each_product_builds_its_own_binary_and_smokes_its_own_archive() {
        assert_eq!(Product::Gui.package(), "colosseum-gui");
        assert_eq!(Product::Gui.binary(), "colosseum");
        assert_eq!(
            Product::Gui.smoke_script(),
            "tools/release/Smoke-GuiArchive.ps1"
        );
        assert_eq!(Product::Cli.package(), "colosseum-cli");
        assert_eq!(Product::Cli.binary(), "colosseum-cli");
        assert_eq!(
            Product::Cli.smoke_script(),
            "tools/release/Smoke-CliArchive.ps1"
        );
        for product in [Product::Gui, Product::Cli] {
            let root = crate::util::repository_root();
            assert!(
                root.join(product.smoke_script()).is_file(),
                "{product} names a smoke script that does not exist"
            );
        }
    }

    #[test]
    fn a_platform_gets_exactly_one_portable_archive() {
        assert_eq!(Format::portable_for("windows"), Format::Zip);
        assert_eq!(Format::portable_for("linux"), Format::TarGz);
        assert_eq!(Format::portable_for("macos"), Format::TarGz);
        assert!(Format::Zip.is_portable_archive() && Format::TarGz.is_portable_archive());
        assert!(!Format::Msi.is_portable_archive());
    }
}
