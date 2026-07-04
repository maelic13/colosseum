// SPDX-License-Identifier: GPL-3.0-or-later
//! Translation-runtime support: running Windows UCI engines on macOS/Linux.
//!
//! Three responsibilities live here:
//!
//! 1. **Resolution** — turn an [`EngineConfig`] (path + [`EngineRuntime`]
//!    choice) into concrete [`SpawnOptions`]: either a direct spawn, or
//!    `wine <exe> …` with a per-engine `WINEPREFIX` under the app data dir.
//! 2. **Discovery** — find a usable Wine: the *managed* build Colosseum
//!    downloads into `data_dir/runtimes/`, or a *system* installation (PATH,
//!    well-known locations; on arm64 Linux a Hangover install provides plain
//!    `wine`, so it is discovered the same way).
//! 3. **Managed install** — download the pinned, checksummed portable Wine
//!    build for this platform and unpack it into `data_dir/runtimes/`.
//!
//! Per-engine artifacts (the wineprefix) live in `data_dir/engines/<id>/` and
//! are deleted together with the engine. The prefix is created on first spawn
//! by [`ensure_prefix_for`] (detection at add time normally pays this cost, so
//! tournament games start instantly).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use colosseum_core::{BinaryKind, EngineConfig, EngineId, EngineRuntime, sniff_binary_kind};
use colosseum_uci::SpawnOptions;

use crate::error::EngineError;

/// Environment variables forced onto every Wine spawn (the user's per-engine
/// env can override them): keep the UCI stdout stream clean and skip the
/// Mono/Gecko machinery old console engines never need.
const WINE_BASE_ENV: [(&str, &str); 2] = [
    ("WINEDEBUG", "-all"),
    ("WINEDLLOVERRIDES", "mscoree=;mshtml="),
];

/// How long prefix initialisation (`wineboot -u`) may take on first run.
const WINEBOOT_TIMEOUT: Duration = Duration::from_secs(180);

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Where runtime-related state lives, derived from the app data dir.
#[derive(Debug, Clone)]
pub struct RuntimeEnv {
    pub data_dir: PathBuf,
}

impl RuntimeEnv {
    #[must_use]
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }

    /// Managed runtimes (downloaded Wine builds) root.
    #[must_use]
    pub fn runtimes_dir(&self) -> PathBuf {
        self.data_dir.join("runtimes")
    }

    /// Per-engine generated artifacts (wineprefix, …). Deleted with the engine.
    #[must_use]
    pub fn engine_data_dir(&self, id: EngineId) -> PathBuf {
        self.data_dir.join("engines").join(id.to_string())
    }

    /// The engine's private `WINEPREFIX`.
    #[must_use]
    pub fn prefix_dir(&self, id: EngineId) -> PathBuf {
        self.engine_data_dir(id).join("prefix")
    }

    /// Remove everything generated for an engine (call on engine delete).
    pub fn delete_engine_data(&self, id: EngineId) {
        let dir = self.engine_data_dir(id);
        if dir.exists()
            && let Err(e) = std::fs::remove_dir_all(&dir)
        {
            tracing::warn!("failed to remove engine data dir {}: {e}", dir.display());
        }
    }
}

// ---------------------------------------------------------------------------
// Binary sniffing
// ---------------------------------------------------------------------------

/// Sniff an executable's format from its header (first 4 KiB).
#[must_use]
pub fn sniff_binary(path: &Path) -> BinaryKind {
    let Ok(mut f) = std::fs::File::open(path) else {
        return BinaryKind::Unknown;
    };
    let mut buf = [0u8; 4096];
    let mut filled = 0;
    // Read as much of the header as is available (files can be shorter).
    loop {
        match f.read(&mut buf[filled..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => filled += n,
        }
        if filled == buf.len() {
            break;
        }
    }
    sniff_binary_kind(&buf[..filled])
}

/// Whether a binary of this kind needs a translation runtime on *this* host.
/// On Windows every PE runs directly (Prism translates x64/x86 on arm64
/// hosts); elsewhere any Windows binary needs Wine.
#[must_use]
pub fn needs_wine(kind: BinaryKind) -> bool {
    !cfg!(windows) && kind.is_windows()
}

/// Whether a binary of this kind runs through *some* translation layer on
/// this host (drives the warning badge): any Windows PE on macOS/Linux, and
/// on Windows arm64 hosts any non-arm64 PE (Prism emulation).
#[must_use]
pub fn is_translated(kind: BinaryKind) -> bool {
    if cfg!(windows) {
        cfg!(target_arch = "aarch64") && kind.is_windows() && kind != BinaryKind::WindowsArm64
    } else {
        kind.is_windows()
    }
}

/// The name of the translation layer used for a translated binary on this
/// host (badge hover text).
#[must_use]
pub fn translation_layer_name() -> &'static str {
    if cfg!(windows) {
        "Windows built-in emulation (Prism)"
    } else if cfg!(target_os = "macos") {
        "Wine (Rosetta 2)"
    } else {
        "Wine"
    }
}

// ---------------------------------------------------------------------------
// Wine discovery
// ---------------------------------------------------------------------------

/// What Wine installs are visible right now (drives the UI and `Auto`).
#[derive(Debug, Clone, Default)]
pub struct WineStatus {
    /// The managed (Colosseum-downloaded) Wine binary, if installed.
    pub managed: Option<PathBuf>,
    /// A system Wine binary (PATH or well-known location), if any.
    pub system: Option<PathBuf>,
}

impl WineStatus {
    /// Probe both sources once.
    #[must_use]
    pub fn probe(env: &RuntimeEnv) -> Self {
        Self {
            managed: find_managed_wine(env),
            system: find_system_wine(),
        }
    }

    /// The Wine that `Auto` resolves to: managed first, then system.
    #[must_use]
    pub fn best(&self) -> Option<&PathBuf> {
        self.managed.as_ref().or(self.system.as_ref())
    }

    #[must_use]
    pub fn any(&self) -> bool {
        self.managed.is_some() || self.system.is_some()
    }
}

/// Locate a system-wide `wine` binary: `$PATH`, then well-known locations.
/// On arm64 Linux a Hangover installation ships a regular `wine`, so it is
/// found by the same search. Always `None` on Windows.
#[must_use]
pub fn find_system_wine() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let candidate = dir.join("wine");
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    let well_known: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/opt/homebrew/bin/wine",
            "/usr/local/bin/wine",
            "/Applications/Wine Stable.app/Contents/Resources/wine/bin/wine",
            "/Applications/Wine Devel.app/Contents/Resources/wine/bin/wine",
            "/Applications/Wine Staging.app/Contents/Resources/wine/bin/wine",
        ]
    } else {
        &["/usr/bin/wine", "/usr/local/bin/wine", "/opt/wine/bin/wine"]
    };
    well_known
        .iter()
        .map(PathBuf::from)
        .find(|p| is_executable_file(p))
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Whether Rosetta 2 is installed (always `true` off macOS-arm64, where it is
/// not needed). The managed macOS Wine build is x86-64 and requires it.
#[must_use]
pub fn rosetta_available() -> bool {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    {
        use std::sync::OnceLock;
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            std::process::Command::new("/usr/bin/arch")
                .args(["-x86_64", "/usr/bin/true"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
        })
    }
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    {
        true
    }
}

// ---------------------------------------------------------------------------
// Managed Wine: pinned build + install
// ---------------------------------------------------------------------------

/// A pinned, checksummed portable Wine build for one platform.
#[derive(Debug, Clone, Copy)]
pub struct ManagedWineSpec {
    /// Human-readable Wine version ("11.0").
    pub version: &'static str,
    /// Direct download URL of the `.tar.xz` archive.
    pub url: &'static str,
    /// SHA-256 of the archive, hex-encoded.
    pub sha256: &'static str,
    /// Approximate download size in bytes (progress display before the
    /// server reports a content length).
    pub approx_bytes: u64,
    /// Directory name under `runtimes/` the build is installed into.
    pub install_dir: &'static str,
}

/// The pinned build for this platform, or `None` where no managed download is
/// offered (Windows: not needed; arm64 Linux: install Hangover system-wide —
/// it is then auto-detected as system Wine).
#[must_use]
pub fn managed_wine_spec() -> Option<&'static ManagedWineSpec> {
    #[cfg(target_os = "macos")]
    {
        // x86-64 build; runs through Rosetta 2 on Apple Silicon.
        static SPEC: ManagedWineSpec = ManagedWineSpec {
            version: "11.0",
            url: "https://github.com/Gcenx/macOS_Wine_builds/releases/download/11.0_1/wine-stable-11.0_1-osx64.tar.xz",
            sha256: "b50dc50ec7f41d58b115a6b685d4d1315ba3c797bd3aa0f49213f2703cb82388",
            approx_bytes: 185_303_032,
            install_dir: "wine-11.0-macos",
        };
        Some(&SPEC)
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        // WoW64 build: runs 32-bit Windows engines without 32-bit host libs.
        static SPEC: ManagedWineSpec = ManagedWineSpec {
            version: "11.0",
            url: "https://github.com/Kron4ek/Wine-Builds/releases/download/11.0/wine-11.0-amd64-wow64.tar.xz",
            sha256: "39574efa1132c3ca0d5c77dd2eddbe4a49cca0d6cc2c290ff4924493a1c40314",
            approx_bytes: 73_144_724,
            install_dir: "wine-11.0-linux-x64",
        };
        Some(&SPEC)
    }
    #[cfg(not(any(target_os = "macos", all(target_os = "linux", target_arch = "x86_64"))))]
    {
        None
    }
}

/// Locate the managed Wine binary, if installed and intact.
#[must_use]
pub fn find_managed_wine(env: &RuntimeEnv) -> Option<PathBuf> {
    let spec = managed_wine_spec()?;
    let root = env.runtimes_dir().join(spec.install_dir);
    let manifest = root.join("manifest.json");
    let text = std::fs::read_to_string(manifest).ok()?;
    let rel: serde_json::Value = serde_json::from_str(&text).ok()?;
    let wine = root.join(rel.get("wine")?.as_str()?);
    is_executable_file(&wine).then_some(wine)
}

/// Progress phases reported during a managed install.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallPhase {
    /// `(downloaded_bytes, total_bytes)` follow.
    Downloading,
    /// Verifying + unpacking (no byte counts).
    Unpacking,
}

/// Download, verify, and unpack the managed Wine build. **Blocking** — run on
/// a background thread. `progress(phase, done, total)` is called repeatedly
/// during the download and once when unpacking starts.
///
/// Idempotent: an existing intact install is returned unchanged.
pub fn install_managed_wine(
    env: &RuntimeEnv,
    progress: &(dyn Fn(InstallPhase, u64, u64) + Sync),
) -> Result<PathBuf, EngineError> {
    let spec = managed_wine_spec().ok_or_else(|| {
        EngineError::Runtime("no managed Wine build is available for this platform".into())
    })?;
    if let Some(existing) = find_managed_wine(env) {
        return Ok(existing);
    }

    let runtimes = env.runtimes_dir();
    std::fs::create_dir_all(&runtimes)?;
    let archive_path = runtimes.join(format!("{}.tar.xz.part", spec.install_dir));

    // ── Download, hashing as we stream to disk ──
    let response = ureq::get(spec.url)
        .call()
        .map_err(|e| EngineError::Runtime(format!("download failed: {e}")))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(spec.approx_bytes);
    let mut reader = response.into_body().into_reader();

    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    let mut out = std::fs::File::create(&archive_path)?;
    let mut buf = [0u8; 64 * 1024];
    let mut downloaded: u64 = 0;
    loop {
        let n = std::io::Read::read(&mut reader, &mut buf)
            .map_err(|e| EngineError::Runtime(format!("download interrupted: {e}")))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut out, &buf[..n])?;
        hasher.update(&buf[..n]);
        downloaded += n as u64;
        progress(InstallPhase::Downloading, downloaded, total.max(downloaded));
    }
    drop(out);

    let digest = format!("{:x}", hasher.finalize());
    if !digest.eq_ignore_ascii_case(spec.sha256) {
        let _ = std::fs::remove_file(&archive_path);
        return Err(EngineError::Runtime(format!(
            "checksum mismatch for downloaded Wine (expected {}, got {digest}) — \
             download corrupted or tampered with; not installed",
            spec.sha256
        )));
    }

    // ── Unpack into a staging dir, then rename into place ──
    progress(InstallPhase::Unpacking, 0, 0);
    let staging = runtimes.join(format!(".staging-{}", spec.install_dir));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    std::fs::create_dir_all(&staging)?;
    {
        let file = std::fs::File::open(&archive_path)?;
        let xz = xz2::read::XzDecoder::new(std::io::BufReader::new(file));
        let mut archive = tar::Archive::new(xz);
        archive.set_preserve_permissions(true);
        archive
            .unpack(&staging)
            .map_err(|e| EngineError::Runtime(format!("unpack failed: {e}")))?;
    }
    let _ = std::fs::remove_file(&archive_path);

    // Locate the wine binary inside the unpacked tree and record it.
    let wine_rel = find_wine_binary_rel(&staging).ok_or_else(|| {
        EngineError::Runtime("no `bin/wine` found inside the downloaded archive".into())
    })?;
    let manifest = serde_json::json!({
        "version": spec.version,
        "wine": wine_rel,
    });
    std::fs::write(
        staging.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    let final_dir = runtimes.join(spec.install_dir);
    if final_dir.exists() {
        std::fs::remove_dir_all(&final_dir)?;
    }
    std::fs::rename(&staging, &final_dir)?;

    find_managed_wine(env).ok_or_else(|| {
        EngineError::Runtime("managed Wine installed but its binary is not runnable".into())
    })
}

/// Find `…/bin/wine` under `root` (skipping Wine's bundled mono/gecko trees)
/// and return it as a `/`-joined path relative to `root`.
fn find_wine_binary_rel(root: &Path) -> Option<String> {
    fn walk(dir: &Path, depth: usize) -> Option<PathBuf> {
        if depth > 6 {
            return None;
        }
        let entries = std::fs::read_dir(dir).ok()?;
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                if name != "share" {
                    subdirs.push(path);
                }
            } else if name == "wine"
                && dir.file_name().is_some_and(|d| d == "bin")
                && is_executable_file(&path)
            {
                return Some(path);
            }
        }
        subdirs.into_iter().find_map(|d| walk(&d, depth + 1))
    }
    let abs = walk(root, 0)?;
    let rel = abs.strip_prefix(root).ok()?;
    Some(
        rel.components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// Remove the managed runtime installation (shared across engines; call only
/// from an explicit user action).
pub fn remove_managed_wine(env: &RuntimeEnv) -> Result<(), EngineError> {
    if let Some(spec) = managed_wine_spec() {
        let dir = env.runtimes_dir().join(spec.install_dir);
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Spawn composition
// ---------------------------------------------------------------------------

/// Turn an engine's config into concrete [`SpawnOptions`], resolving its
/// [`EngineRuntime`] choice against what is installed right now.
pub fn spawn_options(engine: &EngineConfig, env: &RuntimeEnv) -> Result<SpawnOptions, EngineError> {
    let direct = || SpawnOptions {
        path: engine.path.clone(),
        args: engine.args.clone(),
        working_dir: engine.working_dir.clone(),
        env: engine.env.clone().into_iter().collect(),
    };

    let wine = match engine.runtime {
        EngineRuntime::Native => None,
        EngineRuntime::WineManaged => Some(find_managed_wine(env).ok_or_else(|| {
            EngineError::Runtime(
                "this engine is set to the managed Wine, but it is not installed".into(),
            )
        })?),
        EngineRuntime::WineSystem => Some(find_system_wine().ok_or_else(|| {
            EngineError::Runtime(
                "this engine is set to system Wine, but no `wine` was found".into(),
            )
        })?),
        EngineRuntime::Auto => {
            let kind = engine.binary.unwrap_or_else(|| sniff_binary(&engine.path));
            if !needs_wine(kind) {
                None
            } else {
                let status = WineStatus::probe(env);
                Some(status.best().cloned().ok_or_else(|| {
                    EngineError::Runtime(
                        "this Windows engine needs Wine, but none is installed \
                         (install it from the Engines tab)"
                            .into(),
                    )
                })?)
            }
        }
    };

    let Some(wine) = wine else {
        return Ok(direct());
    };

    let mut spawn_env: BTreeMap<String, String> = WINE_BASE_ENV
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    spawn_env.insert(
        "WINEPREFIX".to_string(),
        env.prefix_dir(engine.id).to_string_lossy().into_owned(),
    );
    // The user's per-engine env wins over our defaults.
    for (k, v) in &engine.env {
        spawn_env.insert(k.clone(), v.clone());
    }

    let mut args = vec![engine.path.to_string_lossy().into_owned()];
    args.extend(engine.args.iter().cloned());

    Ok(SpawnOptions {
        path: wine,
        args,
        // Default to the exe's folder so companion files (DLL engines,
        // `.ini`s old engines write next to themselves) resolve.
        working_dir: engine
            .working_dir
            .clone()
            .or_else(|| engine.path.parent().map(Path::to_path_buf)),
        env: spawn_env,
    })
}

/// If `spawn` targets Wine with a `WINEPREFIX` that does not exist yet,
/// initialise it (`wineboot -u`). Cheap no-op otherwise. Serialised globally
/// so two concurrent first-spawns of the same engine cannot race.
pub async fn ensure_prefix_for(spawn: &SpawnOptions) -> Result<(), EngineError> {
    let Some(prefix) = spawn.env.get("WINEPREFIX") else {
        return Ok(());
    };
    let prefix_path = PathBuf::from(prefix);
    if prefix_path.join("system.reg").exists() {
        return Ok(());
    }

    static BOOT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = BOOT_LOCK.lock().await;
    if prefix_path.join("system.reg").exists() {
        return Ok(());
    }

    std::fs::create_dir_all(&prefix_path)?;
    let mut cmd = tokio::process::Command::new(&spawn.path);
    cmd.args(["wineboot", "-u"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    for (k, v) in &spawn.env {
        cmd.env(k, v);
    }
    let status = tokio::time::timeout(WINEBOOT_TIMEOUT, async { cmd.spawn()?.wait().await })
        .await
        .map_err(|_| EngineError::Runtime("Wine prefix initialisation timed out".into()))??;
    if !status.success() {
        return Err(EngineError::Runtime(format!(
            "Wine prefix initialisation failed (wineboot exited with {status})"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use colosseum_core::EngineRuntime;

    fn engine_at(path: &str) -> EngineConfig {
        EngineConfig::new(PathBuf::from(path))
    }

    #[test]
    fn native_engine_spawns_directly() {
        let dir = tempfile::tempdir().unwrap();
        let env = RuntimeEnv::new(dir.path());
        let mut engine = engine_at("/engines/stockfish");
        engine.binary = Some(BinaryKind::Native);
        let spawn = spawn_options(&engine, &env).unwrap();
        assert_eq!(spawn.path, PathBuf::from("/engines/stockfish"));
        assert!(spawn.args.is_empty());
        assert!(spawn.env.is_empty());
    }

    #[test]
    fn pinned_native_runtime_never_uses_wine() {
        let dir = tempfile::tempdir().unwrap();
        let env = RuntimeEnv::new(dir.path());
        let mut engine = engine_at("/engines/rybka.exe");
        engine.binary = Some(BinaryKind::WindowsX64);
        engine.runtime = EngineRuntime::Native;
        let spawn = spawn_options(&engine, &env).unwrap();
        assert_eq!(spawn.path, PathBuf::from("/engines/rybka.exe"));
    }

    #[cfg(unix)]
    #[test]
    fn windows_engine_composes_wine_spawn() {
        let dir = tempfile::tempdir().unwrap();
        let env = RuntimeEnv::new(dir.path());

        // Fake a managed wine install so resolution succeeds hermetically.
        let spec_dir = env
            .runtimes_dir()
            .join(managed_wine_spec().unwrap().install_dir);
        std::fs::create_dir_all(spec_dir.join("bin")).unwrap();
        let wine = spec_dir.join("bin/wine");
        std::fs::write(&wine, "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wine, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(
            spec_dir.join("manifest.json"),
            r#"{"version":"11.0","wine":"bin/wine"}"#,
        )
        .unwrap();

        let mut engine = engine_at("/engines/Rybka 3 mp x64.exe");
        engine.binary = Some(BinaryKind::WindowsX64);
        engine.args = vec!["--extra".into()];
        engine.env.insert("WINEDEBUG".into(), "+all".into());

        let spawn = spawn_options(&engine, &env).unwrap();
        assert_eq!(spawn.path, wine);
        assert_eq!(
            spawn.args,
            vec![
                "/engines/Rybka 3 mp x64.exe".to_string(),
                "--extra".to_string()
            ]
        );
        // Per-engine prefix under the app data dir.
        assert_eq!(
            spawn.env.get("WINEPREFIX").map(PathBuf::from),
            Some(env.prefix_dir(engine.id))
        );
        // User env overrides the defaults.
        assert_eq!(spawn.env.get("WINEDEBUG").map(String::as_str), Some("+all"));
        assert_eq!(
            spawn.env.get("WINEDLLOVERRIDES").map(String::as_str),
            Some("mscoree=;mshtml=")
        );
        // Working dir defaults to the exe's folder.
        assert_eq!(spawn.working_dir, Some(PathBuf::from("/engines")));
    }

    #[test]
    fn auto_without_wine_gives_actionable_error() {
        if cfg!(windows) {
            return; // On Windows PE binaries always spawn directly.
        }
        let dir = tempfile::tempdir().unwrap();
        let env = RuntimeEnv::new(dir.path());
        let mut engine = engine_at("/engines/houdini.exe");
        engine.binary = Some(BinaryKind::WindowsX64);
        // No managed install and (very likely) a PATH without wine would still
        // find a system wine on dev machines — pin to managed to stay hermetic.
        engine.runtime = EngineRuntime::WineManaged;
        let err = spawn_options(&engine, &env).unwrap_err();
        assert!(err.to_string().contains("not installed"));
    }

    #[test]
    fn ensure_prefix_is_noop_without_wineprefix() {
        let spawn = SpawnOptions::new("/bin/true");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async { ensure_prefix_for(&spawn).await })
            .unwrap();
    }

    #[test]
    fn sniffs_real_binaries_kinds() {
        // Our own test binary is native on any platform.
        let me = std::env::current_exe().unwrap();
        assert_eq!(sniff_binary(&me), BinaryKind::Native);
        assert_eq!(sniff_binary(Path::new("/nonexistent")), BinaryKind::Unknown);
    }

    #[test]
    fn delete_engine_data_removes_dir() {
        let dir = tempfile::tempdir().unwrap();
        let env = RuntimeEnv::new(dir.path());
        let id = EngineId::new();
        std::fs::create_dir_all(env.prefix_dir(id)).unwrap();
        env.delete_engine_data(id);
        assert!(!env.engine_data_dir(id).exists());
    }
}
