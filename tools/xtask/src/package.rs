//! `cargo xtask package <gui|cli>` — the release artifacts, from one recipe.
//!
//! Everything a release publishes is produced here, so a local archive and a
//! published one come from the same code: the release workflows call these
//! commands rather than repeating the staging in YAML. Artifacts land in
//! `target/dist/`, named `<product>-<version>-<platform>-<arch>.<ext>`, and
//! each one's SHA-256 is printed beside it.

use std::path::{Path, PathBuf};

use crate::archive;
use crate::build;
use crate::product::{Format, Product};
use crate::util::{Result, capture, powershell, run, sha256};

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
        match format {
            Format::Zip | Format::TarGz => {
                let stage = stage(
                    root,
                    request.product,
                    &version,
                    platform,
                    arch,
                    &binary,
                    &name,
                )?;
                if format == Format::Zip {
                    archive::zip(&stage, &name, &destination)?;
                } else {
                    archive::tar_gz(&stage, &name, &destination)?;
                }
            }
            Format::Msi => msi(root, &version, &request.target, arch, &destination)?,
            Format::Deb => deb(root, &request.target, &destination)?,
            Format::Rpm => rpm(root, &request.target, &destination)?,
            Format::Dmg => dmg(root, &version, &binary, &destination)?,
            Format::PkgTarZst => pkg_tar_zst(root, &version, &binary, &destination)?,
        }
        if !destination.is_file() {
            return Err(format!("{} was not produced", destination.display()));
        }
        println!("  {}  {}", sha256(&destination)?, destination.display());
        produced.push((format, destination));
    }

    if request.smoke {
        for (format, artifact) in &produced {
            if format.is_portable_archive() {
                smoke(root, request.product, artifact, &version, platform, arch)?;
            }
        }
    }

    // `target/dist` is what a workflow uploads, so it holds artifacts and
    // nothing else once the formats that needed a working tree are done.
    for scratch in ["staging", "dmg-staging", "pkgbuild"] {
        let path = dist.join(scratch);
        if path.exists() {
            std::fs::remove_dir_all(&path)
                .map_err(|error| format!("could not clear {}: {error}", path.display()))?;
        }
    }
    Ok(produced.into_iter().map(|(_, path)| path).collect())
}

/// The allowlisted directory that becomes the portable archive.
///
/// The CLI's contents are the release tool's business — binary, licence,
/// front door and the version-matched guide — so it stages them. The GUI's
/// portable archive is the executable and the licence: its documentation is
/// the application itself.
fn stage(
    root: &Path,
    product: Product,
    version: &str,
    platform: &str,
    arch: &str,
    binary: &Path,
    name: &str,
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
        Product::Gui => {
            let stage = staging.join(name);
            std::fs::create_dir_all(&stage)
                .map_err(|error| format!("could not create {}: {error}", stage.display()))?;
            let executable = if platform == "windows" {
                "colosseum.exe"
            } else {
                "colosseum"
            };
            copy(binary, &stage.join(executable))?;
            copy(&root.join("LICENSE"), &stage.join("LICENSE"))?;
            Ok(stage)
        }
    }
}

fn copy(source: &Path, destination: &Path) -> Result<()> {
    std::fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| {
            format!(
                "could not copy {} to {}: {error}",
                source.display(),
                destination.display()
            )
        })
}

/// The Windows installer, built by WiX from the one `main.wxs`.
///
/// The architecture comes from the artifact's own token, so an arm64 MSI can
/// only ever be built from an arm64 binary.
fn msi(root: &Path, version: &str, target: &str, arch: &str, destination: &Path) -> Result<()> {
    let binary_directory = root.join("target").join(target).join("release");
    let binary_directory = binary_directory
        .canonicalize()
        .map_err(|error| format!("could not resolve {}: {error}", binary_directory.display()))?;
    // A prerelease version such as 1.2.0-rc.1 is not a Windows version; the
    // installer carries the release part, and a prerelease ships no MSI.
    let product_version = version.split('-').next().unwrap_or(version);
    run(
        "wix",
        &[
            "build",
            "crates/colosseum-gui/wix/main.wxs",
            "-arch",
            arch,
            "-d",
            &format!("Version={product_version}"),
            "-d",
            &format!("BinDir={}", strip_verbatim(&binary_directory)),
            "-d",
            &format!("ProjectRoot={}", strip_verbatim(root)),
            "-ext",
            "WixToolset.UI.wixext",
            "-o",
            &destination.display().to_string(),
        ],
        root,
    )?;
    // WiX writes a build database beside the installer. It is a debugging
    // artifact, not something to publish, and leaving it next to the finished
    // files would put it in the release: the workflow uploads this directory
    // by pattern and then checks the artifact list by count.
    let database = destination.with_extension("wixpdb");
    if database.exists() {
        std::fs::remove_file(&database)
            .map_err(|error| format!("could not remove {}: {error}", database.display()))?;
    }
    Ok(())
}

/// Windows canonicalisation returns a `\\?\` path, which WiX does not accept.
fn strip_verbatim(path: &Path) -> String {
    let text = path.display().to_string();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

fn deb(root: &Path, target: &str, destination: &Path) -> Result<()> {
    run(
        "cargo",
        &[
            "deb",
            "-p",
            "colosseum-gui",
            "--no-build",
            "--target",
            target,
            "-o",
            &destination.display().to_string(),
        ],
        root,
    )
}

fn rpm(root: &Path, target: &str, destination: &Path) -> Result<()> {
    run(
        "cargo",
        &[
            "generate-rpm",
            "-p",
            "crates/colosseum-gui",
            "--target",
            target,
            "-o",
            &destination.display().to_string(),
        ],
        root,
    )
}

/// The macOS disk image, with the `.app` bundle it installs.
///
/// The bundle's version is the product manifest's, read through the release
/// tool — never the workspace manifest, which carries no version at all and
/// once left the bundle stamped with an empty one.
fn dmg(root: &Path, version: &str, binary: &Path, destination: &Path) -> Result<()> {
    let staging = root.join("target").join("dist").join("dmg-staging");
    if staging.exists() {
        std::fs::remove_dir_all(&staging)
            .map_err(|error| format!("could not clear {}: {error}", staging.display()))?;
    }
    let app = staging.join("Colosseum.app");
    std::fs::create_dir_all(app.join("Contents/MacOS"))
        .and_then(|()| std::fs::create_dir_all(app.join("Contents/Resources")))
        .map_err(|error| format!("could not create {}: {error}", app.display()))?;
    copy(binary, &app.join("Contents/MacOS/colosseum"))?;

    let iconset = staging.join("colosseum.iconset");
    std::fs::create_dir_all(&iconset)
        .map_err(|error| format!("could not create {}: {error}", iconset.display()))?;
    let master = staging.join("icon.png");
    run(
        "sips",
        &[
            "-s",
            "format",
            "png",
            "crates/colosseum-gui/assets/colosseum.ico",
            "--out",
            &master.display().to_string(),
        ],
        root,
    )?;
    for size in [16, 32, 128, 256] {
        for (pixels, suffix) in [(size, String::new()), (size * 2, "@2x".to_owned())] {
            let out = iconset.join(format!("icon_{size}x{size}{suffix}.png"));
            run(
                "sips",
                &[
                    "-z",
                    &pixels.to_string(),
                    &pixels.to_string(),
                    &master.display().to_string(),
                    "--out",
                    &out.display().to_string(),
                ],
                root,
            )?;
        }
    }
    let icns = app.join("Contents/Resources/colosseum.icns");
    run(
        "iconutil",
        &[
            "-c",
            "icns",
            &iconset.display().to_string(),
            "-o",
            &icns.display().to_string(),
        ],
        root,
    )?;

    std::fs::write(app.join("Contents/Info.plist"), info_plist(version))
        .map_err(|error| format!("could not write Info.plist: {error}"))?;
    // Apple Silicon refuses to run an entirely unsigned bundle; an ad-hoc
    // signature is what an unnotarized release can offer.
    run(
        "codesign",
        &[
            "--force",
            "--deep",
            "--sign",
            "-",
            &app.display().to_string(),
        ],
        root,
    )?;

    let background = staging.join("dmg-background.tiff");
    run(
        "tiffutil",
        &[
            "-cathidpicheck",
            "crates/colosseum-gui/assets/dmg/dmg-background.png",
            "crates/colosseum-gui/assets/dmg/dmg-background@2x.png",
            "-out",
            &background.display().to_string(),
        ],
        root,
    )?;
    let volume = format!("Colosseum {version}");
    let created = run(
        "create-dmg",
        &[
            "--volname",
            &volume,
            "--volicon",
            &icns.display().to_string(),
            "--background",
            &background.display().to_string(),
            "--window-pos",
            "200",
            "120",
            "--window-size",
            "640",
            "400",
            "--icon-size",
            "128",
            "--icon",
            "Colosseum.app",
            "160",
            "200",
            "--hide-extension",
            "Colosseum.app",
            "--app-drop-link",
            "480",
            "200",
            "--no-internet-enable",
            &destination.display().to_string(),
            &app.display().to_string(),
        ],
        root,
    );
    if created.is_err() || !destination.is_file() {
        // create-dmg's AppleScript layout step is occasionally flaky in CI.
        // A plain image with the /Applications symlink still installs.
        println!("  create-dmg did not produce an image; falling back to hdiutil");
        let link = staging.join("Applications");
        if !link.exists() {
            run(
                "ln",
                &["-s", "/Applications", &link.display().to_string()],
                root,
            )?;
        }
        run(
            "hdiutil",
            &[
                "create",
                "-volname",
                &volume,
                "-srcfolder",
                &staging.display().to_string(),
                "-ov",
                "-format",
                "UDZO",
                &destination.display().to_string(),
            ],
            root,
        )?;
    }
    Ok(())
}

fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>    <string>colosseum</string>
  <key>CFBundleIconFile</key>      <string>colosseum</string>
  <key>CFBundleIdentifier</key>    <string>com.colosseum.Colosseum</string>
  <key>CFBundleName</key>          <string>Colosseum</string>
  <key>CFBundleDisplayName</key>   <string>Colosseum</string>
  <key>CFBundleVersion</key>       <string>{version}</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundlePackageType</key>   <string>APPL</string>
  <key>CFBundleSignature</key>     <string>????</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
"#
    )
}

/// The Arch package, built by `makepkg` from a generated PKGBUILD.
fn pkg_tar_zst(root: &Path, version: &str, binary: &Path, destination: &Path) -> Result<()> {
    let build_directory = root.join("target").join("dist").join("pkgbuild");
    if build_directory.exists() {
        std::fs::remove_dir_all(&build_directory)
            .map_err(|error| format!("could not clear {}: {error}", build_directory.display()))?;
    }
    std::fs::create_dir_all(&build_directory)
        .map_err(|error| format!("could not create {}: {error}", build_directory.display()))?;
    copy(binary, &build_directory.join("colosseum"))?;
    copy(
        &root.join("packaging/colosseum.desktop"),
        &build_directory.join("colosseum.desktop"),
    )?;
    copy(
        &root.join("packaging/colosseum.png"),
        &build_directory.join("colosseum.png"),
    )?;
    copy(&root.join("LICENSE"), &build_directory.join("LICENSE"))?;

    // `pkgver` may not contain a hyphen (1.2.0-rc.1 becomes 1.2.0_rc.1).
    let pkgver = version.replace('-', "_");
    std::fs::write(build_directory.join("PKGBUILD"), pkgbuild(&pkgver))
        .map_err(|error| format!("could not write PKGBUILD: {error}"))?;

    // makepkg refuses to run as root, which is the packaging container's
    // default user, so the build directory is handed to an unprivileged one.
    let as_root = capture("id", &["-u"], root)?.trim() == "0";
    if as_root {
        // The user may already exist from an earlier format in the same run.
        run("useradd", &["-m", "builder"], root).ok();
        let directory = build_directory.display().to_string();
        run("chown", &["-R", "builder", &directory], root)?;
        run(
            "su",
            &[
                "builder",
                "-c",
                &format!("cd '{directory}' && makepkg --nodeps"),
            ],
            root,
        )?;
    } else {
        run("makepkg", &["--nodeps"], &build_directory)?;
    }

    // The exact name, never a glob: makepkg may build a companion package.
    let built = build_directory.join(format!("colosseum-{pkgver}-1-x86_64.pkg.tar.zst"));
    if !built.is_file() {
        return Err(format!("makepkg did not produce {}", built.display()));
    }
    std::fs::rename(&built, destination)
        .map_err(|error| format!("could not move the Arch package: {error}"))?;
    Ok(())
}

fn pkgbuild(pkgver: &str) -> String {
    format!(
        r#"pkgname=colosseum
pkgver={pkgver}
pkgrel=1
pkgdesc="UCI chess engine tournament GUI"
arch=('x86_64')
url="https://github.com/maelic13/colosseum"
license=('GPL-3.0-or-later')
depends=('gtk3' 'libxkbcommon')
# Arch builds a companion -debug package by default; a release binary carries
# no debug symbols, so it would only be an empty second package.
options=('!debug')
source=('colosseum' 'colosseum.desktop' 'colosseum.png' 'LICENSE')
sha256sums=('SKIP' 'SKIP' 'SKIP' 'SKIP')

package() {{
  install -Dm755 "$srcdir/colosseum"         "$pkgdir/usr/bin/colosseum"
  install -Dm644 "$srcdir/colosseum.desktop" "$pkgdir/usr/share/applications/colosseum.desktop"
  install -Dm644 "$srcdir/colosseum.png"     "$pkgdir/usr/share/icons/hicolor/256x256/apps/colosseum.png"
  install -Dm644 "$srcdir/LICENSE"           "$pkgdir/usr/share/licenses/colosseum/LICENSE"
}}
"#
    )
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
