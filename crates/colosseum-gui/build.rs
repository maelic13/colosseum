//! Build script: embed the application icon into the Windows executable.
//!
//! Setting the window icon at runtime (`ViewportBuilder::with_icon`) is not
//! enough on Windows — the taskbar button and Alt-Tab entry fall back to the
//! *executable's* embedded icon, and when the runtime icon isn't applied in
//! time (the window starts hidden and is revealed a few frames later) the
//! taskbar shows a generic/blank icon. Embedding an icon resource in the .exe
//! gives Windows a stable icon it always has available.
//!
//! Failure to compile the resource (e.g. no resource compiler on the build
//! host) is non-fatal: the build still succeeds and the app falls back to the
//! runtime-set icon.

fn main() {
    // Only meaningful for a Windows *target*.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    // `winresource` is only available as a build-dependency on a Windows
    // *host* (see `[target.'cfg(windows)'.build-dependencies]` in Cargo.toml),
    // so gate its use on the host to keep the build script compiling on other
    // platforms (e.g. cross-compiling to Windows from macOS/Linux).
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=assets/colosseum.ico");

        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/colosseum.ico");
        if let Err(err) = res.compile() {
            // Don't break the build — just note it and keep the runtime icon.
            println!("cargo:warning=failed to embed exe icon resource: {err}");
        }
    }
}
