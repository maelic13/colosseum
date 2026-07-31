//! Colosseum GUI binary: a cross-platform chess-engine tournament runner.
//!
//! `main` initialises logging, builds the [`Backend`] (storage, runtime, engine
//! library), constructs the native window (theme, icon, restored geometry), and
//! hands control to [`ColosseumApp`]. The tab bodies are filled in Steps 8–10.

// GUI subsystem unconditionally — a console-subsystem binary makes Windows
// flash a console window at every launch (even debug builds are used as the
// day-to-day app here). `attach_parent_console` restores log output when the
// app is started from a terminal.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod backend;
mod board;
mod config;
mod dialog;
mod eco;
mod engines_tab;
mod export_ui;
mod icon;
mod live_view;
mod logo;
mod presets;
mod product;
mod results_tab;
mod runtime_adapter;
mod theme;
mod tournament_tab;
mod update;
mod widgets;

use eframe::egui;

use crate::app::ColosseumApp;
use crate::backend::Backend;
use crate::config::AppDirs;
use crate::product::DISPLAY_NAME;

/// Re-attach stdout/stderr to the parent process's console (if any), so a GUI-
/// subsystem binary still prints tracing output when launched from a terminal.
#[cfg(windows)]
fn attach_parent_console() {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn AttachConsole(process_id: u32) -> i32;
    }
    const ATTACH_PARENT_PROCESS: u32 = u32::MAX;
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Give the process a stable, explicit taskbar identity (AppUserModelID) so
/// Windows treats the app as its own entity and consistently shows its icon,
/// rather than grouping it under a generic host and dropping the icon. Pairs
/// with the exe-embedded icon (see `build.rs`) to make the taskbar reliable.
#[cfg(windows)]
fn set_app_user_model_id() {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(app_id: *const u16) -> i32;
    }
    let id: Vec<u16> = "Colosseum.ChessGUI"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(id.as_ptr());
    }
}

/// Initialise logging to both the console (when attached) and a file in the
/// data directory (`logs/colosseum.log`, rotated once past ~4 MB), so engine
/// problems can be diagnosed after the fact in a windowed build.
fn init_logging(dirs: Option<&AppDirs>) {
    use tracing_subscriber::fmt::writer::MakeWriterExt;
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let file = dirs.and_then(|dirs| {
        let dir = dirs.data_dir.join("logs");
        std::fs::create_dir_all(&dir).ok()?;
        let path = dir.join("colosseum.log");
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > 4 * 1024 * 1024) {
            let prev = dir.join("colosseum.prev.log");
            let _ = std::fs::remove_file(&prev);
            let _ = std::fs::rename(&path, &prev);
        }
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .ok()
    });

    match file {
        Some(file) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(std::io::stderr.and(std::sync::Mutex::new(file)))
            .init(),
        None => tracing_subscriber::fmt().with_env_filter(filter).init(),
    }
}

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    {
        attach_parent_console();
        set_app_user_model_id();
    }

    if std::env::args().any(|argument| argument == "--version") {
        println!("colosseum {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // `--portable` keeps all data (config, database, engines) next to the binary.
    let portable = std::env::args().any(|a| a == "--portable");

    // Resolve the data directories early so the log file and incident reports
    // land in the right place from the very first message.
    let dirs = AppDirs::new(portable).or_else(|| AppDirs::new(true));
    init_logging(dirs.as_ref());
    if let Some(dirs) = &dirs {
        colosseum_engine::incidents::set_dir(dirs.data_dir.join("logs").join("incidents"));
    }
    tracing::info!("─── Colosseum {} starting ───", env!("CARGO_PKG_VERSION"));

    let backend = match Backend::new(portable) {
        Ok(backend) => backend,
        Err(err) => {
            tracing::error!("failed to initialise backend: {err:#}");
            // Surface the failure to the user via a native dialog before exiting.
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Colosseum — startup error")
                .set_description(format!("Could not start Colosseum:\n\n{err:#}"))
                .show();
            std::process::exit(1);
        }
    };

    let viewport = egui::ViewportBuilder::default()
        .with_title(DISPLAY_NAME)
        .with_app_id("colosseum")
        .with_inner_size([backend.config.window_width, backend.config.window_height])
        .with_min_inner_size([860.0, 560.0])
        // NOTE: no `.with_maximized` here — on Windows, maximizing shows the
        // window, which made the hidden window flash empty at startup. The app
        // maximizes at reveal time instead (see `maximize_on_reveal`).
        // Start hidden; the app reveals the window after the first frame is
        // painted, so startup never flashes an empty/unstyled window.
        .with_visible(false)
        .with_icon(icon::icon());

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        DISPLAY_NAME,
        native_options,
        Box::new(|cc| Ok(Box::new(ColosseumApp::new(cc, backend)))),
    )
}
