//! The top-level Colosseum application: tab shell, header, status bar, live
//! backend bridge, window-state persistence, and confirm-on-close.
//!
//! The tab bodies themselves arrive in later steps (Engines = Step 8,
//! Tournament = Step 9, openings = Step 10); this module owns the chrome and the
//! cross-cutting behaviour that all tabs share.

use eframe::egui::{self, Align, Layout, RichText, Ui, ViewportCommand};

use colosseum_core::branding::DISPLAY_NAME;
use colosseum_engine::TournamentStatus;

use crate::backend::Backend;
use crate::engines_tab::EnginesTab;
use crate::history_tab::{HistoryAction, HistoryTab};
use crate::theme;
use crate::tournament_tab::TournamentTab;
use crate::widgets;

/// Top-level tabs. Tournament is the primary, default tab.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Tab {
    #[default]
    Tournament,
    Engines,
    History,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Tournament => "Tournament",
            Tab::Engines => "Engines",
            Tab::History => "History",
        }
    }
}

/// Where the close-confirm flow currently stands.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum CloseState {
    /// Normal operation; no close pending.
    #[default]
    Open,
    /// A close was requested while a tournament was busy; awaiting the user's choice.
    Confirming,
    /// The user confirmed; the window may close this frame.
    Closing,
}

/// The Colosseum GUI application.
pub struct ColosseumApp {
    backend: Backend,
    tab: Tab,
    close: CloseState,
    engines_tab: EnginesTab,
    tournament_tab: TournamentTab,
    history_tab: HistoryTab,
}

impl ColosseumApp {
    /// Construct the app, applying the theme to the egui context.
    pub fn new(cc: &eframe::CreationContext<'_>, backend: Backend) -> Self {
        theme::apply(&cc.egui_ctx);
        Self {
            backend,
            tab: Tab::default(),
            close: CloseState::Open,
            engines_tab: EnginesTab::default(),
            tournament_tab: TournamentTab::default(),
            history_tab: HistoryTab::default(),
        }
    }

    /// Mirror the live window geometry into the persisted config so it can be
    /// restored next launch.
    fn capture_window_state(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            let vp = i.viewport();
            if let Some(rect) = vp.inner_rect {
                let size = rect.size();
                if size.x > 0.0 && size.y > 0.0 {
                    self.backend.config.window_width = size.x;
                    self.backend.config.window_height = size.y;
                }
            }
            if let Some(maximized) = vp.maximized {
                self.backend.config.window_maximized = maximized;
            }
        });
    }

    /// Drive the close lifecycle: intercept a close request while a tournament
    /// is busy and show a confirm modal; otherwise persist state and let the
    /// window close.
    fn handle_close(&mut self, ctx: &egui::Context) {
        let close_requested = ctx.input(|i| i.viewport().close_requested());

        if close_requested {
            if self.close == CloseState::Closing {
                // Already confirmed — allow this close to proceed.
                self.backend.save_config();
                return;
            }
            if self.backend.is_busy() {
                // Veto the close and ask what to do with the running tournament.
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.close = CloseState::Confirming;
            } else {
                self.backend.save_config();
                // Let the close proceed (do not cancel).
            }
        }

        if self.close == CloseState::Confirming {
            self.show_close_confirm(ctx);
        }
    }

    /// The "a tournament is running — really quit?" modal.
    fn show_close_confirm(&mut self, ctx: &egui::Context) {
        let mut decision: Option<CloseDecision> = None;

        let modal = egui::Modal::new(egui::Id::new("close_confirm")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.label(RichText::new("Quit Colosseum?").size(18.0).strong().color(theme::TEXT));
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "A tournament is still running. Choose how to handle the games in \
                     progress before quitting.",
                )
                .color(theme::TEXT_WEAK),
            );
            ui.add_space(16.0);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Right-to-left: add rightmost item first.
                if widgets::tinted_button(ui, "Force-stop & quit", theme::DANGER, true)
                    .on_hover_text("Abort in-flight games (discarding them) and quit immediately.")
                    .clicked()
                {
                    decision = Some(CloseDecision::ForceStopAndQuit);
                }
                ui.add_space(4.0);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Stop & quit")
                                .color(theme::BG_DARKEST)
                                .strong(),
                        )
                        .fill(theme::ACCENT),
                    )
                    .on_hover_text(
                        "Let in-flight games finish and record their results, then quit.",
                    )
                    .clicked()
                {
                    decision = Some(CloseDecision::StopAndQuit);
                }
                ui.add_space(4.0);
                if ui
                    .add(
                        egui::Button::new(RichText::new("Keep running").color(theme::TEXT))
                            .fill(theme::BG_ELEVATED)
                            .stroke(egui::Stroke::new(1.0, theme::STROKE)),
                    )
                    .clicked()
                {
                    decision = Some(CloseDecision::Cancel);
                }
            });
        });

        if modal.should_close() {
            decision.get_or_insert(CloseDecision::Cancel);
        }

        if let Some(decision) = decision {
            match decision {
                CloseDecision::Cancel => self.close = CloseState::Open,
                CloseDecision::StopAndQuit => {
                    if let Some(active) = &self.backend.active {
                        active.handle.stop();
                    }
                    self.finish_close(ctx);
                }
                CloseDecision::ForceStopAndQuit => {
                    if let Some(active) = &self.backend.active {
                        active.handle.force_stop();
                    }
                    self.finish_close(ctx);
                }
            }
        }
    }

    /// Persist state and request the window to actually close.
    fn finish_close(&mut self, ctx: &egui::Context) {
        self.close = CloseState::Closing;
        self.backend.save_config();
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    /// The top header: app title, primary tabs, live status pill.
    fn header(&mut self, ui: &mut Ui) {
        egui::Panel::top("header")
            .frame(
                egui::Frame::default()
                    .fill(theme::BG_DARKEST)
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    logo(ui);
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(DISPLAY_NAME)
                            .size(17.0)
                            .strong()
                            .color(theme::TEXT),
                    );
                    ui.add_space(20.0);

                    for tab in [Tab::Tournament, Tab::Engines, Tab::History] {
                        let selected = self.tab == tab;
                        if widgets::pill_tab(ui, tab.label(), selected) {
                            self.tab = tab;
                        }
                        ui.add_space(4.0);
                    }

                    // Right-aligned: live tournament status pill.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (label, dot, color) = match self.backend.status() {
                            Some(TournamentStatus::Running) => ("Running", "●", theme::SUCCESS),
                            Some(TournamentStatus::Stopping) => ("Stopping", "●", theme::WARN),
                            Some(TournamentStatus::Stopped) => ("Stopped", "●", theme::TEXT_WEAK),
                            Some(TournamentStatus::Finished) => ("Finished", "●", theme::ACCENT),
                            Some(TournamentStatus::Idle) | None => ("Idle", "○", theme::TEXT_FAINT),
                        };
                        widgets::status_pill(ui, label, dot, color);
                    });
                });
            });
    }

    /// The bottom status bar: tournament status pill + engine count + version.
    fn status_bar(&self, ui: &mut Ui) {
        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::default()
                    .fill(theme::BG_DARKEST)
                    .inner_margin(egui::Margin::symmetric(14, 6)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    let (label, dot, color) = match self.backend.status() {
                        Some(TournamentStatus::Running) => ("Running", "●", theme::SUCCESS),
                        Some(TournamentStatus::Stopping) => ("Stopping", "●", theme::WARN),
                        Some(TournamentStatus::Stopped) => ("Stopped", "●", theme::TEXT_WEAK),
                        Some(TournamentStatus::Finished) => ("Finished", "●", theme::ACCENT),
                        Some(TournamentStatus::Idle) | None => ("Idle", "○", theme::TEXT_FAINT),
                    };
                    widgets::status_pill(ui, label, dot, color);

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} engines", self.backend.engines.len()))
                                .color(theme::TEXT_WEAK)
                                .size(12.0),
                        );
                        ui.label(RichText::new("·").color(theme::TEXT_FAINT).size(12.0));
                        ui.label(
                            RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .color(theme::TEXT_WEAK)
                                .size(12.0),
                        );
                    });
                });
            });
    }

    /// The central tab body.
    fn body(&mut self, ui: &mut Ui) {
        egui::Frame::default()
            .fill(theme::BG_PANEL)
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| match self.tab {
                Tab::Tournament => {
                    self.tournament_tab.show(ui, &mut self.backend);
                }
                Tab::Engines => {
                    self.engines_tab.show(ui, &mut self.backend);
                }
                Tab::History => {
                    if self.history_tab.show(ui, &mut self.backend)
                        == HistoryAction::SwitchToTournament
                    {
                        self.tab = Tab::Tournament;
                    }
                }
            });
    }
}

impl eframe::App for ColosseumApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if let Some(interval) = self.backend.poll() {
            ctx.request_repaint_after(interval);
        }

        self.capture_window_state(&ctx);

        self.header(ui);
        self.status_bar(ui);
        self.body(ui);

        self.handle_close(&ctx);
    }
}

/// What the user chose in the close-confirm modal.
#[derive(Clone, Copy)]
enum CloseDecision {
    Cancel,
    StopAndQuit,
    ForceStopAndQuit,
}

/// Paint the amphitheatre logo (two concentric gold rings) inline in the header.
/// Drawn with the painter so it always renders, regardless of available fonts.
fn logo(ui: &mut egui::Ui) {
    let size = egui::vec2(20.0, 20.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    let center = rect.center();
    painter.circle_stroke(center, 8.5, egui::Stroke::new(2.2, theme::ACCENT));
    painter.circle_stroke(
        center,
        4.0,
        egui::Stroke::new(1.8, theme::ACCENT.gamma_multiply(0.8)),
    );
}
