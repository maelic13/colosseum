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
mod engines_tab;
mod export_ui;
mod icon;
mod logo;
mod presets;
mod results_tab;
mod stats_ui;
mod theme;
mod tournament_tab;
mod viewer;
mod widgets;

use colosseum_core::branding::DISPLAY_NAME;
use eframe::egui;

use crate::app::ColosseumApp;
use crate::backend::Backend;

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

fn main() -> eframe::Result<()> {
    #[cfg(windows)]
    attach_parent_console();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // `--portable` keeps all data (config, database, engines) next to the binary.
    let portable = std::env::args().any(|a| a == "--portable");

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
