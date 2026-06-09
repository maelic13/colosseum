//! A deliberate, modern dark theme for Colosseum.
//!
//! The palette is a cool slate background with a warm gold accent (evoking the
//! amphitheatre). Colors are exposed as constants so every tab draws from one
//! source of truth; [`apply`] installs the theme into an egui [`Context`].

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals,
    style::{Selection, WidgetVisuals, Widgets},
};

// ── Palette ──────────────────────────────────────────────────────────────

/// Application background (behind panels).
pub const BG_DARKEST: Color32 = Color32::from_rgb(0x12, 0x15, 0x1b);
/// Panel background (side/top/bottom panels, central panel).
pub const BG_PANEL: Color32 = Color32::from_rgb(0x1a, 0x1e, 0x26);
/// Elevated surfaces: cards, table headers, selected rows.
pub const BG_ELEVATED: Color32 = Color32::from_rgb(0x23, 0x29, 0x34);
/// Hovered elevated surface.
pub const BG_HOVER: Color32 = Color32::from_rgb(0x2c, 0x33, 0x40);
/// Sunken inputs (text fields, the darkest interactive fill).
pub const BG_INPUT: Color32 = Color32::from_rgb(0x0e, 0x11, 0x16);
/// Subtle stripe for alternating table rows.
pub const BG_FAINT: Color32 = Color32::from_rgb(0x1f, 0x24, 0x2d);

/// Hairline borders and separators.
pub const STROKE: Color32 = Color32::from_rgb(0x32, 0x3a, 0x47);

/// Primary text.
pub const TEXT: Color32 = Color32::from_rgb(0xe7, 0xea, 0xf0);
/// Secondary / muted text.
pub const TEXT_WEAK: Color32 = Color32::from_rgb(0x97, 0xa1, 0xb1);

/// Warm gold accent (selection, focus, links, primary actions).
pub const ACCENT: Color32 = Color32::from_rgb(0xe0, 0xa9, 0x3b);
/// Brighter accent for hover/active.
pub const ACCENT_BRIGHT: Color32 = Color32::from_rgb(0xf2, 0xc1, 0x5e);

/// Positive status (wins, finished, "Go").
pub const SUCCESS: Color32 = Color32::from_rgb(0x5f, 0xb8, 0x73);
/// Caution status (stopping, warnings).
pub const WARN: Color32 = Color32::from_rgb(0xd9, 0xa5, 0x4f);
/// Negative status (errors, "Force-Stop", losses).
pub const DANGER: Color32 = Color32::from_rgb(0xdb, 0x5d, 0x52);

// ── Application ──────────────────────────────────────────────────────────

/// Install the Colosseum theme (colors, spacing, rounding, type scale) into the
/// given egui context. Call once at startup.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    style.visuals = visuals();

    // Generous, even spacing for an uncluttered feel.
    let spacing = &mut style.spacing;
    spacing.item_spacing = egui::vec2(8.0, 8.0);
    spacing.button_padding = egui::vec2(10.0, 6.0);
    spacing.menu_margin = egui::Margin::same(6);
    spacing.indent = 18.0;
    spacing.interact_size.y = 26.0;
    spacing.scroll.bar_width = 10.0;

    // A clear, readable type scale.
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(22.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.5, FontFamily::Monospace),
        ),
    ]
    .into();

    ctx.set_style(style);
}

/// The Colosseum dark [`Visuals`].
fn visuals() -> Visuals {
    let mut v = Visuals::dark();

    v.panel_fill = BG_PANEL;
    v.window_fill = BG_PANEL;
    v.faint_bg_color = BG_FAINT;
    v.extreme_bg_color = BG_INPUT;
    v.window_stroke = Stroke::new(1.0, STROKE);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.hyperlink_color = ACCENT;

    v.selection = Selection {
        bg_fill: ACCENT.gamma_multiply(0.32),
        stroke: Stroke::new(1.0, ACCENT_BRIGHT),
    };

    v.widgets = widgets();

    // Soft shadows; keep the UI flat and calm.
    v.popup_shadow.color = Color32::from_black_alpha(120);
    v.window_shadow.color = Color32::from_black_alpha(140);

    v
}

/// Widget colors across interaction states.
fn widgets() -> Widgets {
    let radius = CornerRadius::same(6);
    let text = Stroke::new(1.0, TEXT);
    let text_weak = Stroke::new(1.0, TEXT_WEAK);

    Widgets {
        // Backgrounds of non-interactive areas (labels live here).
        noninteractive: WidgetVisuals {
            bg_fill: BG_PANEL,
            weak_bg_fill: BG_PANEL,
            bg_stroke: Stroke::new(1.0, STROKE),
            fg_stroke: text_weak,
            corner_radius: radius,
            expansion: 0.0,
        },
        // Idle interactive widgets (buttons at rest).
        inactive: WidgetVisuals {
            bg_fill: BG_ELEVATED,
            weak_bg_fill: BG_ELEVATED,
            bg_stroke: Stroke::NONE,
            fg_stroke: text,
            corner_radius: radius,
            expansion: 0.0,
        },
        // Hovered.
        hovered: WidgetVisuals {
            bg_fill: BG_HOVER,
            weak_bg_fill: BG_HOVER,
            bg_stroke: Stroke::new(1.0, STROKE),
            fg_stroke: Stroke::new(1.5, TEXT),
            corner_radius: radius,
            expansion: 1.0,
        },
        // Pressed / active.
        active: WidgetVisuals {
            bg_fill: ACCENT.gamma_multiply(0.30),
            weak_bg_fill: ACCENT.gamma_multiply(0.30),
            bg_stroke: Stroke::new(1.0, ACCENT),
            fg_stroke: Stroke::new(1.5, TEXT),
            corner_radius: radius,
            expansion: 1.0,
        },
        // Open menus / selected.
        open: WidgetVisuals {
            bg_fill: BG_ELEVATED,
            weak_bg_fill: BG_ELEVATED,
            bg_stroke: Stroke::new(1.0, STROKE),
            fg_stroke: text,
            corner_radius: radius,
            expansion: 0.0,
        },
    }
}
