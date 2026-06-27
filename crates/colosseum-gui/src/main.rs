//! Colosseum GUI binary: a cross-platform chess-engine tournament runner.
//!
//! `main` initialises logging, builds the [`Backend`] (storage, runtime, engine
//! library), constructs the native window (theme, icon, restored geometry), and
//! hands control to [`ColosseumApp`]. The tab bodies are filled in Steps 8–10.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod backend;
mod engines_tab;
mod export_ui;
mod history_tab;
mod icon;
mod presets;
mod stats_ui;
mod theme;
mod tournament_tab;
mod widgets;

use colosseum_core::branding::DISPLAY_NAME;
use eframe::egui;

use crate::app::ColosseumApp;
use crate::backend::Backend;

fn main() -> eframe::Result<()> {
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
        .with_maximized(backend.config.window_maximized)
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
