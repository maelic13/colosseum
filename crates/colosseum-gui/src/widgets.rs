//! Shared UI component helpers for the Colosseum design system.

use eframe::egui::{self, Color32, RichText, Ui};

use crate::theme;

/// A pill-shaped navigation tab. Returns `true` when clicked.
pub fn pill_tab(ui: &mut Ui, label: &str, selected: bool) -> bool {
    let text = RichText::new(label).size(14.0);
    let text = if selected {
        text.color(theme::ACCENT_BRIGHT).strong()
    } else {
        text.color(theme::TEXT_WEAK)
    };
    let mut btn = egui::Button::new(text)
        .corner_radius(egui::CornerRadius::same(14))
        .min_size(egui::vec2(0.0, 28.0));
    if selected {
        btn = btn
            .fill(theme::tint(theme::ACCENT, 0.18))
            .stroke(egui::Stroke::NONE);
    } else {
        btn = btn
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE);
    }
    ui.add(btn).clicked()
}

/// A tinted rounded chip showing status: `dot` glyph + `label` text, both in color `c`.
pub fn status_pill(ui: &mut Ui, label: &str, dot: &str, c: Color32) {
    egui::Frame::new()
        .fill(theme::tint(c, 0.16))
        .stroke(egui::Stroke::new(1.0, theme::tint(c, 0.45)))
        .corner_radius(egui::CornerRadius::same(10))
        .inner_margin(egui::Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 5.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(dot).color(c).size(10.0));
                ui.label(RichText::new(label).color(c).size(12.0).strong());
            });
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
