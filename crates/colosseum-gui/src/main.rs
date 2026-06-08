//! Colosseum GUI binary.
//!
//! Step 2 provides a minimal, runnable two-tab shell so the GUI stack is known to
//! build and launch on each platform. Step 7 adds the modern theme, app icon, and
//! the throttled bridge to the backend event stream; Steps 8–10 fill the tabs.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use colosseum_core::branding::DISPLAY_NAME;
use eframe::egui;

/// Top-level tabs. Tournament is the primary tab.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Tab {
    #[default]
    Tournament,
    Engines,
}

#[derive(Default)]
struct ColosseumApp {
    tab: Tab,
}

impl eframe::App for ColosseumApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(DISPLAY_NAME);
                ui.separator();
                ui.selectable_value(&mut self.tab, Tab::Tournament, "Tournament");
                ui.selectable_value(&mut self.tab, Tab::Engines, "Engines");
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Tournament => {
                ui.heading("Tournament");
                ui.label("Scaffold — the tournament tab is implemented in Step 9.");
            }
            Tab::Engines => {
                ui.heading("Engines");
                ui.label("Scaffold — engine management is implemented in Step 8.");
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(DISPLAY_NAME)
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };

    eframe::run_native(
        DISPLAY_NAME,
        native_options,
        Box::new(|_cc| Ok(Box::new(ColosseumApp::default()))),
    )
}
