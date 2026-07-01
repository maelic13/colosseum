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
/// Brighter border for interactive widgets at rest (buttons, checkboxes, combo
/// boxes, text fields). Must read clearly even when the widget sits on a card
/// that shares its fill color — otherwise resting controls look invisible.
pub const BORDER_INTERACTIVE: Color32 = Color32::from_rgb(0x4a, 0x55, 0x66);

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

/// Tertiary text: hints, disabled-ish captions.
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x6c, 0x76, 0x86);

/// Medal colors for ranks 1–3 in results tables.
pub const MEDAL_GOLD: Color32 = ACCENT;
pub const MEDAL_SILVER: Color32 = Color32::from_rgb(0xaa, 0xb4, 0xc4);
pub const MEDAL_BRONZE: Color32 = Color32::from_rgb(0xc0, 0x82, 0x55);

/// Muted identity hues for engine monogram avatars. An engine is assigned one by
/// hashing its name, so it keeps the same color across sessions — giving the list
/// the at-a-glance scannability that logo-equipped GUIs get from real engine art,
/// without an asset pipeline. Kept desaturated so they sit calmly on the slate
/// theme and never compete with the gold accent.
pub const AVATAR_PALETTE: [Color32; 6] = [
    ACCENT,                               // gold
    SUCCESS,                              // green
    Color32::from_rgb(0x5f, 0x93, 0xd6), // blue
    Color32::from_rgb(0x4f, 0xb5, 0xb8), // teal
    Color32::from_rgb(0xa9, 0x8a, 0xd6), // violet
    Color32::from_rgb(0xd9, 0x8f, 0xa6), // rose
];

/// Alpha-blend `c` over a dark background at fractional strength.
/// Use fill = `tint(c, 0.16)`, stroke = `tint(c, 0.45)`, text = `c`.
pub fn tint(c: Color32, alpha: f32) -> Color32 {
    c.gamma_multiply(alpha)
}

// ── Application ──────────────────────────────────────────────────────────

/// Install the Colosseum theme (colors, spacing, rounding, type scale) into the
/// given egui context. Call once at startup.
pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut style = (*ctx.global_style()).clone();

    style.visuals = visuals();

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

    // Labels are UI chrome, not documents — don't let clicks select their text
    // (card titles, section headers). Text fields are unaffected.
    style.interaction.selectable_labels = false;

    ctx.set_global_style(style);
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
        // Idle interactive widgets (buttons at rest). A visible border is
        // essential: buttons and checkboxes frequently sit on cards that share
        // their fill color, so without a stroke they vanish until hovered.
        inactive: WidgetVisuals {
            bg_fill: BG_ELEVATED,
            weak_bg_fill: BG_ELEVATED,
            bg_stroke: Stroke::new(1.0, BORDER_INTERACTIVE),
            fg_stroke: text,
            corner_radius: radius,
            expansion: 0.0,
        },
        // Hovered. Zero expansion: growing widgets on hover shifts layouts and
        // lets the extra pixel clip against panel edges.
        hovered: WidgetVisuals {
            bg_fill: BG_HOVER,
            weak_bg_fill: BG_HOVER,
            bg_stroke: Stroke::new(1.0, STROKE),
            fg_stroke: Stroke::new(1.5, TEXT),
            corner_radius: radius,
            expansion: 0.0,
        },
        // Pressed / active.
        active: WidgetVisuals {
            bg_fill: ACCENT.gamma_multiply(0.30),
            weak_bg_fill: ACCENT.gamma_multiply(0.30),
            bg_stroke: Stroke::new(1.0, ACCENT),
            fg_stroke: Stroke::new(1.5, TEXT),
            corner_radius: radius,
            expansion: 0.0,
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
