// SPDX-License-Identifier: GPL-3.0-or-later
//! Live smoke test of the Wine runtime path: spawn composition, per-engine
//! wineprefix creation, and a full UCI detection handshake through Wine.
//!
//! Needs a real Wine and a real Windows UCI engine, so it only runs when both
//! are provided via environment variables (skipped silently otherwise):
//!
//! ```bash
//! COLOSSEUM_WINE=/path/to/bin/wine \
//! COLOSSEUM_WIN_ENGINE="/path/to/Engine.exe" \
//! cargo test -p colosseum-engine --test wine_smoke -- --nocapture
//! ```

use std::path::PathBuf;

use colosseum_core::{BinaryKind, EngineConfig, EngineRuntime};
use colosseum_engine::runtime::{RuntimeEnv, find_managed_wine, managed_wine_spec, sniff_binary};
use colosseum_engine::{DetectResult, detect_engine_config};

#[test]
fn windows_engine_detects_through_wine() {
    let (Ok(wine), Ok(exe)) = (
        std::env::var("COLOSSEUM_WINE"),
        std::env::var("COLOSSEUM_WIN_ENGINE"),
    ) else {
        eprintln!("skipping wine_smoke: set COLOSSEUM_WINE and COLOSSEUM_WIN_ENGINE");
        return;
    };
    let Some(spec) = managed_wine_spec() else {
        eprintln!("skipping wine_smoke: no managed Wine on this platform");
        return;
    };

    // Fake a managed install whose manifest points at the provided Wine
    // (the manifest path is joined to the install root; an absolute path
    // replaces it, which keeps this test hermetic).
    let dir = tempfile::tempdir().unwrap();
    let env = RuntimeEnv::new(dir.path());
    let install = env.runtimes_dir().join(spec.install_dir);
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(
        install.join("manifest.json"),
        serde_json::json!({ "version": spec.version, "wine": wine }).to_string(),
    )
    .unwrap();
    assert_eq!(find_managed_wine(&env), Some(PathBuf::from(&wine)));

    // Provisional config exactly as the GUI's add flow builds it.
    let mut cfg = EngineConfig::new(PathBuf::from(&exe));
    cfg.binary = Some(sniff_binary(&cfg.path));
    assert!(
        matches!(
            cfg.binary,
            Some(BinaryKind::WindowsX64 | BinaryKind::WindowsX86)
        ),
        "COLOSSEUM_WIN_ENGINE must be a Windows PE binary, got {:?}",
        cfg.binary
    );
    cfg.runtime = EngineRuntime::WineManaged;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let result: DetectResult = rt
        .block_on(detect_engine_config(&cfg, &env))
        .expect("detection through Wine should succeed");

    eprintln!("detected: {:?} by {:?}", result.name, result.author);
    assert!(result.name.is_some(), "engine should report `id name`");
    assert!(!result.options.is_empty(), "engine should declare options");
    // The prefix was created under the engine's private data dir. (Registry
    // files are flushed by wineserver a few seconds after shutdown, so only
    // check that the prefix directory itself was populated.)
    assert!(
        std::fs::read_dir(env.prefix_dir(cfg.id))
            .map(|mut d| d.next().is_some())
            .unwrap_or(false),
        "wineprefix should exist and be non-empty"
    );
}
