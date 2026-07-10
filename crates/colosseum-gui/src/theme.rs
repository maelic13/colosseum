//! The Colosseum theme: a deliberate dark and light palette pair.
//!
//! Dark is a cool slate background with a warm gold accent (evoking the
//! amphitheatre); light is its calm daylight counterpart. Every tab draws from
//! one source of truth: the active [`Palette`], selected by the effective egui
//! theme. Colors are exposed as functions (not constants) so custom-painted
//! chrome follows theme switches at runtime; [`apply`] installs both styles
//! into an egui [`Context`], and [`sync_active`] must run once per frame so
//! the palette tracks the OS theme when the user chooses "System".

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Theme, ThemePreference,
    Visuals,
    style::{Selection, WidgetVisuals, Widgets},
};

// ── Theme choice (persisted setting) ─────────────────────────────────────

/// The user's theme setting: fixed dark/light, or follow the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    Dark,
    Light,
    #[default]
    System,
}

impl ThemeChoice {
    pub const ALL: [Self; 3] = [Self::Dark, Self::Light, Self::System];

    /// Parse the persisted config value; unknown strings fall back to System.
    #[must_use]
    pub fn from_config(s: &str) -> Self {
        match s {
            "dark" => Self::Dark,
            "light" => Self::Light,
            _ => Self::System,
        }
    }

    /// The value stored in `config.toml`.
    #[must_use]
    pub fn as_config(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
            Self::System => "system",
        }
    }

    /// Human-readable menu label.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
            Self::System => "System",
        }
    }

    fn preference(self) -> ThemePreference {
        match self {
            Self::Dark => ThemePreference::Dark,
            Self::Light => ThemePreference::Light,
            Self::System => ThemePreference::System,
        }
    }
}

// ── Palette ──────────────────────────────────────────────────────────────

/// Every color the app paints with, resolved for one theme.
pub struct Palette {
    /// Application background (behind panels; header and status bar).
    pub bg_darkest: Color32,
    /// Panel background (side/top/bottom panels, central panel).
    pub bg_panel: Color32,
    /// Elevated surfaces: cards, table headers, selected rows.
    pub bg_elevated: Color32,
    /// Hovered elevated surface.
    pub bg_hover: Color32,
    /// Sunken inputs (text fields).
    pub bg_input: Color32,
    /// Subtle stripe for alternating table rows.
    pub bg_faint: Color32,
    /// Hairline borders and separators.
    pub stroke: Color32,
    /// Brighter border for interactive widgets at rest (buttons, checkboxes,
    /// combo boxes, text fields). Must read clearly even when the widget sits
    /// on a card that shares its fill color.
    pub border_interactive: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary / muted text.
    pub text_weak: Color32,
    /// Tertiary text: hints, disabled-ish captions.
    pub text_faint: Color32,
    /// Warm gold accent (selection, focus, links, primary actions).
    pub accent: Color32,
    /// Emphasised accent for hover/active.
    pub accent_bright: Color32,
    /// Positive status (wins, finished, "Go").
    pub success: Color32,
    /// Caution status (stopping, warnings).
    pub warn: Color32,
    /// Negative status (errors, "Force-Stop", losses).
    pub danger: Color32,
    /// Live-view eval-graph series colors (white/black engine). Bound to the
    /// engine panels' identity dots so panel ↔ line always match.
    pub graph_white: Color32,
    pub graph_black: Color32,
    /// Medal colors for ranks 2–3 in results tables (rank 1 uses the accent).
    pub medal_silver: Color32,
    pub medal_bronze: Color32,
    /// Muted identity hues for engine monogram avatars (see [`avatar_palette`]).
    pub avatar: [Color32; 6],
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// The original Colosseum dark palette: cool slate + warm gold.
static DARK: Palette = Palette {
    bg_darkest: rgb(0x12, 0x15, 0x1b),
    bg_panel: rgb(0x1a, 0x1e, 0x26),
    bg_elevated: rgb(0x23, 0x29, 0x34),
    bg_hover: rgb(0x2c, 0x33, 0x40),
    bg_input: rgb(0x0e, 0x11, 0x16),
    bg_faint: rgb(0x1f, 0x24, 0x2d),
    stroke: rgb(0x32, 0x3a, 0x47),
    border_interactive: rgb(0x4a, 0x55, 0x66),
    text: rgb(0xe7, 0xea, 0xf0),
    text_weak: rgb(0x97, 0xa1, 0xb1),
    text_faint: rgb(0x6c, 0x76, 0x86),
    accent: rgb(0xe0, 0xa9, 0x3b),
    accent_bright: rgb(0xf2, 0xc1, 0x5e),
    success: rgb(0x5f, 0xb8, 0x73),
    warn: rgb(0xd9, 0xa5, 0x4f),
    danger: rgb(0xdb, 0x5d, 0x52),
    // White = the cool light line, black = the warm dark one — matching the
    // piece colours' intuition (light side ↔ lighter/cooler hue).
    graph_white: rgb(0x37, 0x8a, 0xdd),
    graph_black: rgb(0xef, 0x9f, 0x27),
    medal_silver: rgb(0xaa, 0xb4, 0xc4),
    medal_bronze: rgb(0xc0, 0x82, 0x55),
    avatar: [
        rgb(0xe0, 0xa9, 0x3b), // gold (accent)
        rgb(0x5f, 0xb8, 0x73), // green (success)
        rgb(0x5f, 0x93, 0xd6), // blue
        rgb(0x4f, 0xb5, 0xb8), // teal
        rgb(0xa9, 0x8a, 0xd6), // violet
        rgb(0xd9, 0x8f, 0xa6), // rose
    ],
};

/// The light counterpart: cool paper greys, the same warm gold deepened for
/// contrast, and status hues darkened so they stay readable on white.
static LIGHT: Palette = Palette {
    bg_darkest: rgb(0xe3, 0xe6, 0xeb),
    bg_panel: rgb(0xf3, 0xf4, 0xf7),
    bg_elevated: rgb(0xfc, 0xfc, 0xfd),
    bg_hover: rgb(0xe9, 0xec, 0xf1),
    bg_input: rgb(0xff, 0xff, 0xff),
    bg_faint: rgb(0xea, 0xed, 0xf1),
    stroke: rgb(0xd4, 0xd8, 0xdf),
    border_interactive: rgb(0xae, 0xb6, 0xc2),
    text: rgb(0x1d, 0x25, 0x34),
    text_weak: rgb(0x5b, 0x65, 0x77),
    text_faint: rgb(0x87, 0x91, 0xa1),
    accent: rgb(0xa8, 0x7b, 0x16),
    accent_bright: rgb(0x8a, 0x64, 0x10),
    success: rgb(0x2f, 0x8f, 0x4e),
    warn: rgb(0xa4, 0x76, 0x1f),
    danger: rgb(0xc0, 0x4a, 0x40),
    graph_white: rgb(0x18, 0x5f, 0xa5),
    graph_black: rgb(0xba, 0x75, 0x17),
    medal_silver: rgb(0x7f, 0x8a, 0x9c),
    medal_bronze: rgb(0x9d, 0x64, 0x37),
    avatar: [
        rgb(0xa8, 0x7b, 0x16), // gold (accent)
        rgb(0x2f, 0x8f, 0x4e), // green (success)
        rgb(0x3a, 0x6d, 0xb3), // blue
        rgb(0x2a, 0x8a, 0x8e), // teal
        rgb(0x78, 0x57, 0xb0), // violet
        rgb(0xb2, 0x57, 0x76), // rose
    ],
};

/// Whether the dark palette is currently active. Kept as a global so the
/// hundreds of custom-paint sites can stay plain `theme::text()` calls without
/// threading a context through every helper. Updated by [`sync_active`].
static DARK_ACTIVE: AtomicBool = AtomicBool::new(true);

/// The palette for the currently effective theme.
#[must_use]
pub fn palette() -> &'static Palette {
    if DARK_ACTIVE.load(Ordering::Relaxed) {
        &DARK
    } else {
        &LIGHT
    }
}

/// Whether the dark palette is currently active (for the few paint sites that
/// pick between theme-specific constants, e.g. the board wood tones).
#[must_use]
pub fn is_dark() -> bool {
    DARK_ACTIVE.load(Ordering::Relaxed)
}

// Accessors mirroring the palette fields, so call sites read as
// `theme::accent()` — see [`Palette`] for what each color means.
pub fn bg_darkest() -> Color32 {
    palette().bg_darkest
}
pub fn bg_panel() -> Color32 {
    palette().bg_panel
}
pub fn bg_elevated() -> Color32 {
    palette().bg_elevated
}
pub fn bg_hover() -> Color32 {
    palette().bg_hover
}
pub fn stroke() -> Color32 {
    palette().stroke
}
pub fn border_interactive() -> Color32 {
    palette().border_interactive
}
pub fn text() -> Color32 {
    palette().text
}
pub fn text_weak() -> Color32 {
    palette().text_weak
}
pub fn text_faint() -> Color32 {
    palette().text_faint
}
pub fn accent() -> Color32 {
    palette().accent
}
pub fn accent_bright() -> Color32 {
    palette().accent_bright
}
pub fn success() -> Color32 {
    palette().success
}
pub fn warn() -> Color32 {
    palette().warn
}
pub fn danger() -> Color32 {
    palette().danger
}
pub fn graph_white() -> Color32 {
    palette().graph_white
}
pub fn graph_black() -> Color32 {
    palette().graph_black
}
pub fn medal_gold() -> Color32 {
    palette().accent
}
pub fn medal_silver() -> Color32 {
    palette().medal_silver
}
pub fn medal_bronze() -> Color32 {
    palette().medal_bronze
}

/// Muted identity hues for engine monogram avatars. An engine is assigned one by
/// hashing its name, so it keeps the same color across sessions — giving the list
/// the at-a-glance scannability that logo-equipped GUIs get from real engine art,
/// without an asset pipeline. Kept desaturated so they sit calmly on the theme
/// and never compete with the gold accent.
#[must_use]
pub fn avatar_palette() -> [Color32; 6] {
    palette().avatar
}

/// Alpha-blend `c` over the background at fractional strength.
/// Use fill = `tint(c, 0.16)`, stroke = `tint(c, 0.45)`, text = `c`.
pub fn tint(c: Color32, alpha: f32) -> Color32 {
    c.gamma_multiply(alpha)
}

// ── Application ──────────────────────────────────────────────────────────

/// Install the Colosseum theme (colors, spacing, rounding, type scale) into
/// the given egui context: both the dark and light styles, plus the user's
/// theme preference. Call once at startup.
pub fn apply(ctx: &egui::Context, choice: ThemeChoice) {
    install_fonts(ctx);
    for theme in [Theme::Dark, Theme::Light] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = visuals(theme);

        // Generous, even spacing for an uncluttered feel.
        let spacing = &mut style.spacing;
        spacing.item_spacing = egui::vec2(8.0, 8.0);
        spacing.button_padding = egui::vec2(12.0, 6.0);
        spacing.menu_margin = egui::Margin::same(10);
        spacing.indent = 18.0;
        spacing.interact_size.y = 28.0;
        // Solid scrollbars reserve their own lane instead of floating over the
        // content, so they never cover cards or option rows.
        spacing.scroll = egui::style::ScrollStyle::solid();
        spacing.scroll.bar_width = 8.0;
        // Draw the handle with the foreground (text) colors instead of the
        // widget background fill — the fill is near-white in the light theme,
        // which made scrollbars invisible on light panels. Full text color is
        // far too loud, though, and solid bars ignore the opacity knobs; so
        // run in floating mode with the whole bar width allocated (the bar
        // still reserves its own lane, covering nothing) and soften the
        // handle to the usual translucent grey of native scrollbars.
        spacing.scroll.foreground_color = true;
        spacing.scroll.floating = true;
        spacing.scroll.floating_width = 8.0;
        spacing.scroll.floating_allocated_width = 8.0;
        spacing.scroll.dormant_handle_opacity = 0.30;
        spacing.scroll.active_handle_opacity = 0.45;
        spacing.scroll.interact_handle_opacity = 0.65;
        spacing.scroll.dormant_background_opacity = 0.0;
        spacing.scroll.active_background_opacity = 0.0;
        spacing.scroll.interact_background_opacity = 0.0;

        // A clear, readable type scale.
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(20.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(14.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into();

        // Labels are UI chrome, not documents — don't let clicks select their
        // text (card titles, section headers). Text fields are unaffected.
        style.interaction.selectable_labels = false;

        ctx.set_style_of(theme, style);
    }
    set_choice(ctx, choice);
}

/// Switch the theme preference at runtime (both styles are already installed
/// by [`apply`]; egui resolves "System" against the OS setting itself).
pub fn set_choice(ctx: &egui::Context, choice: ThemeChoice) {
    ctx.set_theme(choice.preference());
    sync_active(ctx);
}

/// Mirror egui's effective theme into the active palette. Call at the top of
/// every frame: with "System" the OS can flip the theme at any time, and the
/// custom-painted chrome must follow in the same frame.
pub fn sync_active(ctx: &egui::Context) {
    DARK_ACTIVE.store(ctx.theme() == Theme::Dark, Ordering::Relaxed);
}

/// Embed Inter (UI) and JetBrains Mono (numbers) so text renders identically on
/// every platform, with egui's default fonts kept as fallbacks for symbols and
/// emoji. Registers an extra `"semibold"` family because egui ships a single
/// weight per family — `RichText::strong()` only brightens the color.
fn install_fonts(ctx: &egui::Context) {
    use eframe::egui::{FontData, FontDefinitions};
    use std::sync::Arc;

    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-Regular.otf"
        ))),
    );
    fonts.font_data.insert(
        "Inter-SemiBold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Inter-SemiBold.otf"
        ))),
    );
    fonts.font_data.insert(
        "JetBrainsMono".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/JetBrainsMono-Regular.ttf"
        ))),
    );

    if let Some(prop) = fonts.families.get_mut(&FontFamily::Proportional) {
        prop.insert(0, "Inter".into());
    }
    if let Some(mono) = fonts.families.get_mut(&FontFamily::Monospace) {
        mono.insert(0, "JetBrainsMono".into());
    }
    // Semibold family: Inter-SemiBold first, then the proportional fallbacks
    // (so symbols inside semibold text still resolve).
    let mut semibold = vec!["Inter-SemiBold".to_owned()];
    if let Some(prop) = fonts.families.get(&FontFamily::Proportional) {
        semibold.extend(prop.iter().skip(1).cloned());
    }
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), semibold);

    ctx.set_fonts(fonts);
}

/// A semibold [`FontId`] at `size` — real bold text (see [`install_fonts`]).
pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

/// The Colosseum [`Visuals`] for one theme.
fn visuals(theme: Theme) -> Visuals {
    let (mut v, p) = match theme {
        Theme::Dark => (Visuals::dark(), &DARK),
        Theme::Light => (Visuals::light(), &LIGHT),
    };

    v.panel_fill = p.bg_panel;
    v.window_fill = p.bg_panel;
    v.faint_bg_color = p.bg_faint;
    v.extreme_bg_color = p.bg_input;
    v.window_stroke = Stroke::new(1.0, p.stroke);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.hyperlink_color = p.accent;

    v.selection = Selection {
        bg_fill: p.accent.gamma_multiply(0.32),
        stroke: Stroke::new(1.0, p.accent_bright),
    };

    v.widgets = widgets(p);

    // Soft shadows; keep the UI flat and calm (lighter in light mode, where
    // dark shadows would read as heavy smudges on paper-grey panels).
    let (popup_alpha, window_alpha) = match theme {
        Theme::Dark => (120, 140),
        Theme::Light => (40, 55),
    };
    v.popup_shadow.color = Color32::from_black_alpha(popup_alpha);
    v.window_shadow.color = Color32::from_black_alpha(window_alpha);

    v
}

/// Widget colors across interaction states.
fn widgets(p: &Palette) -> Widgets {
    let radius = CornerRadius::same(6);
    let text = Stroke::new(1.0, p.text);
    let text_weak = Stroke::new(1.0, p.text_weak);

    Widgets {
        // Backgrounds of non-interactive areas (labels live here).
        noninteractive: WidgetVisuals {
            bg_fill: p.bg_panel,
            weak_bg_fill: p.bg_panel,
            bg_stroke: Stroke::new(1.0, p.stroke),
            fg_stroke: text_weak,
            corner_radius: radius,
            expansion: 0.0,
        },
        // Idle interactive widgets (buttons at rest). A visible border is
        // essential: buttons and checkboxes frequently sit on cards that share
        // their fill color, so without a stroke they vanish until hovered.
        inactive: WidgetVisuals {
            bg_fill: p.bg_elevated,
            weak_bg_fill: p.bg_elevated,
            bg_stroke: Stroke::new(1.0, p.border_interactive),
            fg_stroke: text,
            corner_radius: radius,
            expansion: 0.0,
        },
        // Hovered. Zero expansion: growing widgets on hover shifts layouts and
        // lets the extra pixel clip against panel edges.
        hovered: WidgetVisuals {
            bg_fill: p.bg_hover,
            weak_bg_fill: p.bg_hover,
            bg_stroke: Stroke::new(1.0, p.stroke),
            fg_stroke: Stroke::new(1.5, p.text),
            corner_radius: radius,
            expansion: 0.0,
        },
        // Pressed / active.
        active: WidgetVisuals {
            bg_fill: p.accent.gamma_multiply(0.30),
            weak_bg_fill: p.accent.gamma_multiply(0.30),
            bg_stroke: Stroke::new(1.0, p.accent),
            fg_stroke: Stroke::new(1.5, p.text),
            corner_radius: radius,
            expansion: 0.0,
        },
        // Open menus / selected.
        open: WidgetVisuals {
            bg_fill: p.bg_elevated,
            weak_bg_fill: p.bg_elevated,
            bg_stroke: Stroke::new(1.0, p.stroke),
            fg_stroke: text,
            corner_radius: radius,
            expansion: 0.0,
        },
    }
}
