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
use crate::results_tab::ResultsTab;
use crate::theme;
use crate::tournament_tab::TournamentTab;
use crate::widgets;

/// Top-level tabs. Tournament is the primary, default tab.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum Tab {
    #[default]
    Tournament,
    Engines,
    Arena,
}

impl Tab {
    fn label(self) -> &'static str {
        match self {
            Tab::Tournament => "Tournament",
            Tab::Engines => "Engines",
            Tab::Arena => "Arena",
        }
    }
}

/// The About dialog's update-check flow.
#[derive(Default)]
enum AboutUpdate {
    /// Nothing checked yet; the button is showing.
    #[default]
    Idle,
    /// A background check is running; polled every frame.
    Checking(crate::update::UpdateCheck),
    /// The check finished with this result.
    Done(crate::update::UpdateStatus),
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
    results_tab: ResultsTab,
    /// About dialog: open + the state of the "check for updates" flow.
    about_open: bool,
    about_update: AboutUpdate,
    /// Frames painted so far while the window is still hidden. The window is
    /// revealed only after a couple of frames have actually been presented,
    /// so no unpainted surface is ever shown (startup flash).
    frames_before_reveal: u8,
    /// Whether to maximize at reveal time. On Windows, winit can only maximize
    /// a window by *showing* it, so `with_maximized(true)` in the builder made
    /// the hidden window flash maximized-and-empty for a frame at startup.
    /// Captured at construction because `capture_window_state` overwrites the
    /// config field on every frame, including the hidden ones.
    maximize_on_reveal: bool,
}

impl ColosseumApp {
    /// Construct the app, applying the theme to the egui context.
    pub fn new(cc: &eframe::CreationContext<'_>, backend: Backend) -> Self {
        // SVG piece images (live view board) load through egui's image loaders.
        egui_extras::install_image_loaders(&cc.egui_ctx);
        theme::apply(
            &cc.egui_ctx,
            theme::ThemeChoice::from_config(&backend.config.theme),
        );

        // Capture the window/display handles so native file dialogs open on the
        // same monitor as Colosseum instead of drifting to another screen.
        use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
        if let (Ok(window), Ok(display)) = (cc.window_handle(), cc.display_handle()) {
            crate::dialog::set_parent(crate::dialog::DialogParent::new(
                window.as_raw(),
                display.as_raw(),
            ));
        }

        let tournament_tab = TournamentTab::new(&backend.dirs.config_dir);
        let maximize_on_reveal = backend.config.window_maximized;
        Self {
            backend,
            tab: Tab::default(),
            close: CloseState::Open,
            engines_tab: EnginesTab::default(),
            tournament_tab,
            results_tab: ResultsTab::default(),
            about_open: false,
            about_update: AboutUpdate::Idle,
            frames_before_reveal: 0,
            maximize_on_reveal,
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
                self.engines_tab.flush_edit(&mut self.backend);
                self.backend.save_config();
                return;
            }
            if self.backend.is_busy() {
                // Veto the close and ask what to do with the running tournament.
                ctx.send_viewport_cmd(ViewportCommand::CancelClose);
                self.close = CloseState::Confirming;
            } else {
                self.engines_tab.flush_edit(&mut self.backend);
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
            ui.label(
                RichText::new("Quit Colosseum?")
                    .size(18.0)
                    .strong()
                    .color(theme::text()),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "A tournament is still running. Choose how to handle the games in \
                     progress before quitting.",
                )
                .color(theme::text_weak()),
            );
            ui.add_space(16.0);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // Right-to-left: add rightmost item first.
                if widgets::tinted_button(ui, "Force-stop & quit", theme::danger(), true)
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
                                .color(theme::bg_darkest())
                                .strong(),
                        )
                        .fill(theme::accent()),
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
                        egui::Button::new(RichText::new("Keep running").color(theme::text()))
                            .fill(theme::bg_elevated())
                            .stroke(egui::Stroke::new(1.0, theme::stroke())),
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
                    for active in &self.backend.actives {
                        active.handle.stop();
                    }
                    self.finish_close(ctx);
                }
                CloseDecision::ForceStopAndQuit => {
                    for active in &self.backend.actives {
                        active.handle.force_stop();
                    }
                    self.finish_close(ctx);
                }
            }
        }
    }

    /// The About dialog (Firefox-style): mark, name + version, update check
    /// against the GitHub releases (see [`crate::update`]).
    fn show_about(&mut self, ctx: &egui::Context) {
        if !self.about_open {
            return;
        }
        // Collect a finished background check before drawing.
        if let AboutUpdate::Checking(check) = &self.about_update {
            if let Some(status) = check.poll() {
                self.about_update = AboutUpdate::Done(status);
            } else {
                // Keep polling while the request is in flight.
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        let modal = egui::Modal::new(egui::Id::new("about_dialog")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                // The amphitheatre mark, painted large.
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
                let painter = ui.painter();
                painter.circle_stroke(rect.center(), 24.0, egui::Stroke::new(5.0, theme::accent()));
                painter.circle_stroke(
                    rect.center(),
                    11.0,
                    egui::Stroke::new(4.0, theme::accent().gamma_multiply(0.8)),
                );
                ui.add_space(10.0);
                ui.label(
                    RichText::new(DISPLAY_NAME)
                        .font(theme::semibold(22.0))
                        .color(theme::text()),
                );
                ui.label(
                    RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .color(theme::text_weak())
                        .size(13.0),
                );
                ui.add_space(14.0);

                match &self.about_update {
                    AboutUpdate::Idle => {
                        if widgets::tinted_button(ui, "Check for updates", theme::accent(), true)
                            .clicked()
                        {
                            self.about_update =
                                AboutUpdate::Checking(crate::update::UpdateCheck::start());
                        }
                    }
                    AboutUpdate::Checking(_) => {
                        ui.horizontal(|ui| {
                            // Center the pair inside the fixed-width dialog.
                            ui.add_space((ui.available_width() - 160.0).max(0.0) / 2.0);
                            ui.add(egui::Spinner::new().size(14.0));
                            ui.label(
                                RichText::new("Checking for updates…")
                                    .color(theme::text_weak())
                                    .size(12.5),
                            );
                        });
                    }
                    AboutUpdate::Done(crate::update::UpdateStatus::UpToDate) => {
                        ui.label(
                            RichText::new("You're up to date")
                                .color(theme::success())
                                .font(theme::semibold(13.5)),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{DISPLAY_NAME} v{} is the latest version.",
                                env!("CARGO_PKG_VERSION")
                            ))
                            .color(theme::text_weak())
                            .size(12.0),
                        );
                    }
                    AboutUpdate::Done(crate::update::UpdateStatus::UpdateAvailable {
                        version,
                        url,
                    }) => {
                        ui.label(
                            RichText::new(format!("Version {version} is available"))
                                .color(theme::accent())
                                .font(theme::semibold(13.5)),
                        );
                        ui.add_space(6.0);
                        if widgets::tinted_button(ui, "Open download page", theme::accent(), true)
                            .clicked()
                        {
                            ctx.open_url(egui::OpenUrl::new_tab(url));
                        }
                    }
                    AboutUpdate::Done(crate::update::UpdateStatus::Failed) => {
                        ui.label(
                            RichText::new("Couldn't check for updates — are you online?")
                                .color(theme::text_weak())
                                .size(12.5),
                        );
                        ui.add_space(6.0);
                        if widgets::tinted_button(ui, "Try again", theme::accent(), true).clicked()
                        {
                            self.about_update =
                                AboutUpdate::Checking(crate::update::UpdateCheck::start());
                        }
                    }
                }

                ui.add_space(14.0);
                ui.separator();
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "UCI chess-engine tournaments: round robin & gauntlet, \
                         parallel games, live boards, ML ratings.",
                    )
                    .color(theme::text_weak())
                    .size(12.0),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Free software under GPL-3.0-or-later.")
                        .color(theme::text_faint())
                        .size(11.5),
                );
                ui.hyperlink_to(
                    RichText::new("github.com/maelic13/colosseum").size(11.5),
                    "https://github.com/maelic13/colosseum",
                );
                ui.add_space(6.0);
                ui.label(
                    RichText::new(
                        "Chess pieces by Colin M.L. Burnett (CC BY-SA 3.0) · \
                         opening names from the Lichess openings database (CC0) · \
                         Inter and JetBrains Mono fonts (SIL OFL 1.1).",
                    )
                    .color(theme::text_faint())
                    .size(10.5),
                );
                ui.add_space(10.0);
                if ui
                    .add(
                        egui::Button::new(RichText::new("Close").color(theme::text()))
                            .fill(theme::bg_elevated())
                            .stroke(egui::Stroke::new(1.0, theme::stroke())),
                    )
                    .clicked()
                {
                    self.about_open = false;
                }
                ui.add_space(6.0);
            });
        });
        if modal.should_close() {
            self.about_open = false;
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
                    .fill(theme::bg_darkest())
                    .inner_margin(egui::Margin::symmetric(16, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // The logo + name is the app's "menu": clicking it opens
                    // the About dialog (version, update check).
                    let brand = ui
                        .scope(|ui| {
                            ui.horizontal(|ui| {
                                logo(ui);
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(DISPLAY_NAME)
                                        .size(17.0)
                                        .strong()
                                        .color(theme::text()),
                                );
                            });
                        })
                        .response;
                    let brand = ui.interact(
                        brand.rect,
                        egui::Id::new("about_brand"),
                        egui::Sense::click(),
                    );
                    if brand
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text("About Colosseum")
                        .clicked()
                    {
                        self.about_open = true;
                        self.about_update = AboutUpdate::Idle;
                    }
                    ui.add_space(20.0);

                    for tab in [Tab::Tournament, Tab::Arena, Tab::Engines] {
                        let selected = self.tab == tab;
                        if widgets::pill_tab(ui, tab.label(), selected) {
                            if self.tab == Tab::Engines && tab != Tab::Engines {
                                // Commit any debounced engine edit before leaving.
                                self.engines_tab.flush_edit(&mut self.backend);
                            }
                            self.tab = tab;
                        }
                        ui.add_space(4.0);
                    }

                    // Right-aligned: live tournament status pill.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (label, dot, color) = match self.backend.status() {
                            Some(TournamentStatus::Running) => ("Running", "●", theme::success()),
                            Some(TournamentStatus::Stopping) => ("Stopping", "●", theme::warn()),
                            Some(TournamentStatus::Stopped) => ("Stopped", "●", theme::text_weak()),
                            Some(TournamentStatus::Finished) => ("Finished", "●", theme::accent()),
                            Some(TournamentStatus::Idle) | None => {
                                ("Idle", "○", theme::text_faint())
                            }
                        };
                        widgets::status_pill(ui, label, dot, color);
                    });
                });
            });
    }

    /// The bottom status bar: theme switcher, version + engine count. (The
    /// tournament status pill lives only in the header — one source of truth,
    /// visible on every tab.)
    fn status_bar(&mut self, ui: &mut Ui) {
        egui::Panel::bottom("status_bar")
            .frame(
                egui::Frame::default()
                    .fill(theme::bg_darkest())
                    .inner_margin(egui::Margin::symmetric(14, 6)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    self.theme_switcher(ui);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} engines", self.backend.engines.len()))
                                .color(theme::text_weak())
                                .size(12.0),
                        );
                        ui.label(RichText::new("·").color(theme::text_faint()).size(12.0));
                        ui.label(
                            RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .color(theme::text_weak())
                                .size(12.0),
                        );
                    });
                });
            });
    }

    /// A compact theme dropdown (Dark / Light / System) in the status bar,
    /// using the app-standard [`widgets::select`] so the popup gets the same
    /// padding and alignment as every other dropdown.
    fn theme_switcher(&mut self, ui: &mut Ui) {
        let current = theme::ThemeChoice::from_config(&self.backend.config.theme);
        widgets::select(ui, "theme_select", current.label(), 96.0, |ui| {
            for choice in theme::ThemeChoice::ALL {
                if ui
                    .selectable_label(choice == current, choice.label())
                    .clicked()
                {
                    self.backend.config.theme = choice.as_config().to_string();
                    theme::set_choice(ui.ctx(), choice);
                    self.backend.save_config();
                    ui.close();
                }
            }
        });
    }

    /// The central tab body. A `CentralPanel` (not a bare frame) so content is
    /// clipped to the space between the header and status bar — a bare frame
    /// let tall content paint over the status bar at small window sizes.
    fn body(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(theme::bg_panel())
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ui, |ui| match self.tab {
                Tab::Tournament => {
                    self.tournament_tab.show(ui, &mut self.backend);
                    // A freshly started tournament switches to its live view.
                    if self.tournament_tab.take_started() {
                        self.tab = Tab::Arena;
                    }
                }
                Tab::Engines => {
                    self.engines_tab.show(ui, &mut self.backend);
                }
                Tab::Arena => {
                    self.results_tab.show(ui, &mut self.backend);
                }
            });
    }
}

impl eframe::App for ColosseumApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Track the effective theme before painting anything: with "System"
        // the OS can flip dark/light between frames, and the custom-painted
        // chrome (theme::* colors) must follow in the same frame.
        theme::sync_active(&ctx);

        if let Some(interval) = self.backend.poll() {
            ctx.request_repaint_after(interval);
        }

        self.capture_window_state(&ctx);

        self.header(ui);
        self.status_bar(ui);
        self.body(ui);

        self.show_about(&ctx);
        self.handle_close(&ctx);

        // The window starts hidden (see `main.rs`); keep pumping frames and
        // reveal only after a couple have been fully painted, so the user
        // never sees an unpainted surface flash at startup.
        if self.frames_before_reveal < 3 {
            self.frames_before_reveal += 1;
            if self.frames_before_reveal == 3 {
                // Maximizing shows the window on Windows, so it must happen
                // here (content already painted), never in the builder.
                if self.maximize_on_reveal {
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(true));
                }
                ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(ViewportCommand::Focus);
            } else {
                ctx.request_repaint();
            }
        }
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
    painter.circle_stroke(center, 8.5, egui::Stroke::new(2.2, theme::accent()));
    painter.circle_stroke(
        center,
        4.0,
        egui::Stroke::new(1.8, theme::accent().gamma_multiply(0.8)),
    );
}
