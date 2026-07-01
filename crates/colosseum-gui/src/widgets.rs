//! Shared UI component helpers for the Colosseum design system.

use eframe::egui::{self, Color32, RichText, Ui};

use colosseum_core::EngineConfig;

use crate::theme;

/// A navigation tab: a rounded rectangle at the app-wide corner radius (6),
/// matching buttons and inputs. Returns `true` when clicked. Idle tabs are
/// transparent and pick up a `BG_HOVER` fill on hover; the selected tab gets a
/// tinted-accent fill with brightened, bold text.
pub fn pill_tab(ui: &mut Ui, label: &str, selected: bool) -> bool {
    let radius = egui::CornerRadius::same(6);
    // Reserve a slot so the pill fill paints *behind* the label.
    let bg_slot = ui.painter().add(egui::Shape::Noop);

    let text_color = if selected {
        theme::ACCENT_BRIGHT
    } else {
        theme::TEXT_WEAK
    };
    let mut text = RichText::new(label).size(14.0).color(text_color);
    if selected {
        text = text.strong();
    }

    let resp = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(14, 6))
        .corner_radius(radius)
        .show(ui, |ui| ui.label(text))
        .response;

    let id = ui.id().with(("pill_tab", label));
    let click = ui
        .interact(resp.rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    let fill = if selected {
        theme::tint(theme::ACCENT, 0.18)
    } else if click.hovered() {
        theme::BG_HOVER
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        ui.painter()
            .set(bg_slot, egui::Shape::rect_filled(resp.rect, radius, fill));
    }

    click.clicked()
}

/// A tinted rounded chip showing status: `dot` glyph + `label` text, both in
/// color `c`. Rounded rectangle at the app-wide radius, like everything else.
pub fn status_pill(ui: &mut Ui, label: &str, dot: &str, c: Color32) {
    egui::Frame::new()
        .fill(theme::tint(c, 0.16))
        .stroke(egui::Stroke::new(1.0, theme::tint(c, 0.45)))
        .corner_radius(egui::CornerRadius::same(6))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(dot).color(c).size(10.0));
                ui.label(RichText::new(label).color(c).size(12.0).strong());
            });
        });
}

/// A small static text chip (rounded, tinted) — for version tags, counts, and
/// other inline metadata. Text and border both take color `c` at tint strength.
pub fn chip(ui: &mut Ui, text: &str, c: Color32) {
    egui::Frame::new()
        .fill(theme::tint(c, 0.14))
        .stroke(egui::Stroke::new(1.0, theme::tint(c, 0.4)))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(c).size(11.0));
        });
}

/// A tinted semantic button (success / warn / danger). Uses the tint convention.
pub fn tinted_button(ui: &mut Ui, label: &str, c: Color32, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).color(c).size(13.5).strong())
            .fill(theme::tint(c, 0.16))
            .stroke(egui::Stroke::new(1.0, theme::tint(c, 0.45))),
    )
}

/// A card-framed section with a title row, optional subtitle, and a body closure.
/// Adds 12 px vertical gap after itself.
pub fn section_card<R>(
    ui: &mut Ui,
    title: &str,
    subtitle: Option<&str>,
    body: impl FnOnce(&mut Ui) -> R,
) -> R {
    let r = egui::Frame::new()
        .fill(theme::BG_ELEVATED)
        .stroke(egui::Stroke::new(1.0, theme::STROKE))
        .corner_radius(egui::CornerRadius::same(8))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).color(theme::TEXT).size(14.0).strong());
            if let Some(s) = subtitle {
                ui.label(RichText::new(s).color(theme::TEXT_WEAK).size(11.5));
            }
            ui.add_space(8.0);
            body(ui)
        })
        .inner;
    ui.add_space(12.0);
    r
}

/// Medal badge for ranks 1–3; plain dim number for rank ≥ 4.
pub fn rank_badge(ui: &mut Ui, rank: usize) {
    let medal = match rank {
        1 => Some(theme::MEDAL_GOLD),
        2 => Some(theme::MEDAL_SILVER),
        3 => Some(theme::MEDAL_BRONZE),
        _ => None,
    };
    match medal {
        Some(c) => {
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
            ui.painter().circle(
                rect.center(),
                9.0,
                theme::tint(c, 0.2),
                egui::Stroke::new(1.0, theme::tint(c, 0.5)),
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                rank.to_string(),
                egui::FontId::proportional(11.0),
                c,
            );
        }
        None => {
            ui.label(
                RichText::new(rank.to_string())
                    .color(theme::TEXT_FAINT)
                    .monospace(),
            );
        }
    }
}

// ── Engine identity helpers (shared by the Engines tab and tournament setup) ──

/// The engine's display name: its UCI `id name` (minus version), or the file
/// stem when no name was detected.
pub fn engine_base_name(e: &EngineConfig) -> String {
    if e.meta.name.trim().is_empty() {
        e.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string()
    } else {
        e.meta.name.clone()
    }
}

/// Secondary line for an engine row: the author (from the UCI `id author`) when
/// known, falling back to the executable file name.
pub fn engine_subtitle(e: &EngineConfig) -> String {
    let author = e.meta.extra.get("author").map(|s| s.trim()).unwrap_or("");
    if author.is_empty() {
        e.path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string()
    } else {
        author.to_string()
    }
}

/// First alphanumeric character of `name`, uppercased, for a monogram avatar.
pub fn engine_initial(name: &str) -> char {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('?')
}

/// Stable identity color for an engine, chosen by hashing its display name so it
/// stays the same across sessions (FNV-1a over the name's bytes).
pub fn avatar_color(name: &str) -> Color32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    let palette = theme::AVATAR_PALETTE;
    palette[(h as usize) % palette.len()]
}

/// Paint a circular monogram avatar into `rect`: a tinted disc carrying the
/// engine's initial in its identity color. `emphasized` brightens the fill for
/// the selected row / edit header.
pub fn draw_avatar_in(ui: &Ui, rect: egui::Rect, name: &str, emphasized: bool) {
    let c = avatar_color(name);
    let d = rect.width().min(rect.height());
    let fill = theme::tint(c, if emphasized { 0.28 } else { 0.18 });
    ui.painter().circle(
        rect.center(),
        d / 2.0,
        fill,
        egui::Stroke::new(1.0, theme::tint(c, 0.5)),
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        engine_initial(name).to_string(),
        egui::FontId::proportional(d * 0.46),
        c,
    );
}

/// Paint a rounded-square monogram avatar into `rect` — the same silhouette as
/// an uploaded logo, so monogram and image engines look consistent in the
/// Engines tab. `emphasized` brightens the fill.
pub fn draw_avatar_square_in(
    ui: &Ui,
    rect: egui::Rect,
    name: &str,
    emphasized: bool,
    corner_radius: u8,
) {
    let c = avatar_color(name);
    let fill = theme::tint(c, if emphasized { 0.28 } else { 0.18 });
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(corner_radius),
        fill,
        egui::Stroke::new(1.0, theme::tint(c, 0.5)),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        engine_initial(name).to_string(),
        egui::FontId::proportional(rect.height().min(rect.width()) * 0.46),
        c,
    );
}

/// A checkbox drawn as a small rounded *square* (radius 3). The global widget
/// corner radius (6) makes egui's ~16 px checkbox read as a circle, breaking
/// the app's rounded-rectangle language — use this instead of `ui.checkbox`.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, label: &str) -> egui::Response {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for v in [
            &mut w.noninteractive,
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
            &mut w.open,
        ] {
            v.corner_radius = egui::CornerRadius::same(3);
        }
        ui.checkbox(checked, label)
    })
    .inner
}

/// A dropdown selector: a button showing the current value that opens a menu
/// popup with the choices (put `selectable_label`s or `selectable_value`s in
/// `add_contents`). Use this instead of `egui::ComboBox` everywhere — combo
/// popups wrap their items in a `ScrollArea` that shows a phantom scrollbar
/// even for tiny lists; menu popups never scroll unless they hit the screen
/// edge. Returns the closure result while the popup is open.
pub fn select<R>(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash,
    current: &str,
    min_width: f32,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    ui.push_id(id_salt, |ui| {
        // Trailing spaces reserve room for the painted dropdown arrow.
        let button = egui::Button::new(RichText::new(format!("{current}    ")))
            .min_size(egui::vec2(min_width, ui.spacing().interact_size.y));
        let (resp, inner) =
            egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
                ui.set_min_width((min_width - 8.0).max(60.0));
                ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                add_contents(ui)
            });
        // Painted dropdown arrow (font-safe, see the glyph policy).
        let c = egui::pos2(resp.rect.right() - 12.0, resp.rect.center().y + 1.0);
        let r = 3.5;
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                c + egui::vec2(-r, -r * 0.6),
                c + egui::vec2(r, -r * 0.6),
                c + egui::vec2(0.0, r * 0.9),
            ],
            theme::TEXT_WEAK,
            egui::Stroke::NONE,
        ));
        inner.map(|i| i.inner)
    })
    .inner
}

/// The standard small "×" clear button (as used on the tablebase path rows).
/// Place it to the right of the field it clears; show it only when there is
/// something to clear.
pub fn clear_button(ui: &mut Ui) -> egui::Response {
    ui.small_button(RichText::new("×").color(theme::TEXT_WEAK))
}

/// A small painted disclosure triangle (▶/▼ without relying on font glyphs).
pub fn disclosure_triangle(ui: &mut Ui, open: bool, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    let c = rect.center();
    let r = 4.0;
    let points = if open {
        vec![
            c + egui::vec2(-r, -r * 0.5),
            c + egui::vec2(r, -r * 0.5),
            c + egui::vec2(0.0, r * 0.75),
        ]
    } else {
        vec![
            c + egui::vec2(-r * 0.5, -r),
            c + egui::vec2(r * 0.75, 0.0),
            c + egui::vec2(-r * 0.5, r),
        ]
    };
    ui.painter()
        .add(egui::Shape::convex_polygon(points, color, egui::Stroke::NONE));
}

/// Allocate a `diameter`² slot and paint a monogram avatar into it.
pub fn engine_avatar(ui: &mut Ui, name: &str, diameter: f32, emphasized: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::hover());
    draw_avatar_in(ui, rect, name, emphasized);
}

/// Small tinted chip for Elo Δ values. Zero-ish delta shows a plain dim zero.
pub fn elo_delta_chip(ui: &mut Ui, delta: f64) {
    if delta.abs() <= 0.05 {
        ui.label(RichText::new("0").color(theme::TEXT_FAINT).monospace());
        return;
    }
    let (c, sign) = if delta > 0.0 {
        (theme::SUCCESS, "+")
    } else {
        (theme::DANGER, "")
    };
    egui::Frame::new()
        .fill(theme::tint(c, 0.16))
        .stroke(egui::Stroke::new(1.0, theme::tint(c, 0.45)))
        .corner_radius(egui::CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("{sign}{delta:.0}"))
                    .color(c)
                    .monospace()
                    .size(12.0),
            );
        });
}
