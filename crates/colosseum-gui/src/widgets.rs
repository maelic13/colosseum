//! Shared UI component helpers for the Colosseum design system.

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, DragValue, RichText, Ui};

use colosseum_core::{EngineConfig, UciOption, UciOptionValue};

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
    let text = if selected {
        RichText::new(label)
            .font(theme::semibold(14.0))
            .color(text_color)
    } else {
        RichText::new(label).size(14.0).color(text_color)
    };
    // Always reserve the (wider) semibold width so selecting a tab doesn't
    // shift its neighbours.
    let reserve = ui
        .painter()
        .layout_no_wrap(label.to_owned(), theme::semibold(14.0), Color32::WHITE)
        .size()
        .x;

    let resp = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(14, 6))
        .corner_radius(radius)
        .show(ui, |ui| {
            ui.add_sized([reserve, 18.0], egui::Label::new(text).selectable(false))
        })
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
                ui.label(RichText::new(label).color(c).font(theme::semibold(12.0)));
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
        egui::Button::new(RichText::new(label).color(c).font(theme::semibold(13.5)))
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
            ui.label(
                RichText::new(title)
                    .color(theme::TEXT)
                    .font(theme::semibold(14.0)),
            );
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

/// True when `engine` matches the (lowercased) filter. Word-wise: every
/// whitespace-separated term must match somewhere in name/version/author/file,
/// so "shredder 12" finds Shredder 12 even though no single field contains
/// the whole phrase.
pub fn engine_matches(e: &EngineConfig, filter: &str) -> bool {
    let haystack = format!(
        "{}\n{}\n{}\n{}",
        engine_base_name(e).to_lowercase(),
        e.meta.version.to_lowercase(),
        engine_subtitle(e).to_lowercase(),
        e.path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase(),
    );
    filter
        .split_whitespace()
        .all(|term| haystack.contains(term))
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
        dropdown_arrow(ui, resp.rect);
        inner.map(|i| i.inner)
    })
    .inner
}

/// Paint the small dropdown triangle at the right edge of a button `rect`
/// (font-safe, see the glyph policy). The button's label should reserve room
/// with trailing spaces.
pub fn dropdown_arrow(ui: &Ui, rect: egui::Rect) {
    let c = egui::pos2(rect.right() - 12.0, rect.center().y + 1.0);
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
}

/// The standard small "×" clear button (as used on the tablebase path rows).
/// Place it to the right of the field it clears; show it only when there is
/// something to clear.
pub fn clear_button(ui: &mut Ui) -> egui::Response {
    ui.add(egui::Button::new(RichText::new("×").color(theme::TEXT_WEAK)))
}

/// The standard filter/search field: fixed 28 pt height with comfortable
/// inner padding (identical everywhere), plus the shared "×" clear button to
/// its right as soon as there is text. Returns the text field's response.
pub fn filter_field(ui: &mut Ui, text: &mut String, width: f32, hint: &str) -> egui::Response {
    let resp = ui.add_sized(
        [width, 28.0],
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .margin(egui::Margin::symmetric(8, 6)),
    );
    if !text.is_empty() && clear_button(ui).on_hover_text("Clear filter").clicked() {
        text.clear();
    }
    resp
}

/// Sort order for engine lists and grids, persisted as a config string
/// (`"name"` / `"elo"` / `"author"`). Shared by the Engines-tab card grid and
/// the tournament-setup engine list.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EngineSort {
    Name,
    Elo,
    Author,
}

impl EngineSort {
    pub const ALL: [Self; 3] = [Self::Name, Self::Elo, Self::Author];

    pub fn from_config(s: &str) -> Self {
        match s {
            "elo" => Self::Elo,
            "author" => Self::Author,
            _ => Self::Name,
        }
    }

    pub fn as_config(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Elo => "elo",
            Self::Author => "author",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Elo => "Elo",
            Self::Author => "Author",
        }
    }
}

/// Sort library indices by the given order: Name = alphabetical (then
/// version), Elo = highest first (unrated last), Author = alphabetical
/// (missing last), each falling back to the name key.
pub fn sort_engine_indices(engines: &[EngineConfig], indices: &mut [usize], sort: EngineSort) {
    let name_key = |e: &EngineConfig| {
        (
            engine_base_name(e).to_lowercase(),
            e.meta.version.to_lowercase(),
        )
    };
    match sort {
        EngineSort::Name => {
            indices.sort_by_key(|&i| name_key(&engines[i]));
        }
        EngineSort::Elo => {
            indices.sort_by_key(|&i| {
                let e = &engines[i];
                (
                    std::cmp::Reverse(e.meta.elo.unwrap_or(i32::MIN)),
                    name_key(e),
                )
            });
        }
        EngineSort::Author => {
            indices.sort_by_key(|&i| {
                let e = &engines[i];
                let author = e
                    .meta
                    .extra
                    .get("author")
                    .map(|a| a.trim().to_lowercase())
                    .unwrap_or_default();
                (author.is_empty(), author, name_key(e))
            });
        }
    }
}

/// A small select button for choosing an [`EngineSort`], persisting the choice
/// into the given config string when it changes. Returns `true` on change.
pub fn engine_sort_select(ui: &mut Ui, id_salt: &str, config_value: &mut String) -> bool {
    let mut sort = EngineSort::from_config(config_value);
    let prev = sort;
    select(ui, id_salt, &format!("Sort: {}", sort.label()), 110.0, |ui| {
        for s in EngineSort::ALL {
            if ui.selectable_label(sort == s, s.label()).clicked() {
                sort = s;
                ui.close();
            }
        }
    });
    if sort != prev {
        *config_value = sort.as_config().to_string();
        true
    } else {
        false
    }
}

/// A small always-framed choice chip for inline either/or pickers.
///
/// Never use `ui.selectable_value` / `selectable_label` in a row layout:
/// egui's selectable label has no frame while idle and gains frame padding
/// when hovered, so it grows and shifts everything to its right. This chip
/// keeps a frame (and therefore its size) in every state. Returns a response
/// that reports `changed` when clicking switched the value.
pub fn choice_chip<T: PartialEq>(
    ui: &mut Ui,
    current: &mut T,
    value: T,
    label: &str,
) -> egui::Response {
    let selected = *current == value;
    let text = RichText::new(label).size(12.5).color(if selected {
        theme::ACCENT_BRIGHT
    } else {
        theme::TEXT_WEAK
    });
    let mut button = egui::Button::new(text).corner_radius(egui::CornerRadius::same(4));
    if selected {
        button = button
            .fill(theme::tint(theme::ACCENT, 0.15))
            .stroke(egui::Stroke::new(1.0, theme::tint(theme::ACCENT, 0.4)));
    }
    let mut resp = ui.add(button);
    if resp.clicked() && !selected {
        *current = value;
        resp.mark_changed();
    }
    resp
}

/// Small bordered icon button with three painted dots ("more options"), sized
/// to `rect`. Dots are painted (not the "…" glyph, which sits on the baseline
/// and reads as bottom-aligned in a small control). `emphasized` tints it
/// accent, e.g. when the row carries overrides.
pub fn dots_button(
    ui: &mut Ui,
    rect: egui::Rect,
    id_salt: impl std::hash::Hash,
    emphasized: bool,
) -> egui::Response {
    let resp = ui.interact(rect, ui.id().with(id_salt), egui::Sense::click());
    let hovered = resp.hovered();
    let fill = if hovered {
        theme::BG_HOVER
    } else {
        theme::BG_ELEVATED
    };
    let stroke_color = if emphasized {
        theme::ACCENT
    } else if hovered {
        theme::BORDER_INTERACTIVE
    } else {
        theme::STROKE
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(4),
        fill,
        egui::Stroke::new(1.0, stroke_color),
        egui::StrokeKind::Inside,
    );
    let dot_color = if emphasized {
        theme::ACCENT
    } else if hovered {
        theme::TEXT
    } else {
        theme::TEXT_WEAK
    };
    let c = rect.center();
    for dx in [-4.0, 0.0, 4.0] {
        ui.painter()
            .circle_filled(c + egui::vec2(dx, 0.0), 1.3, dot_color);
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
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

/// Draw one editable UCI option row (label | editor | range hint) writing
/// changed values into `overrides`. Sets `dirty` when a value changed. Used
/// by the Engines tab editor and the per-tournament override dialog.
pub fn uci_option_row(
    ui: &mut Ui,
    opt: &UciOption,
    overrides: &mut BTreeMap<String, UciOptionValue>,
    dirty: &mut bool,
) {
    ui.label(RichText::new(opt.name()).color(theme::TEXT_WEAK).size(13.0));

    match opt {
        UciOption::Check { name, default } => {
            let mut val = matches!(overrides.get(name), Some(UciOptionValue::Check(true)))
                || (overrides.get(name).is_none() && *default);

            if checkbox(ui, &mut val, "").changed() {
                overrides.insert(name.clone(), UciOptionValue::Check(val));
                *dirty = true;
            }
        }

        UciOption::Spin {
            name,
            default,
            min,
            max,
        } => {
            let current = match overrides.get(name) {
                Some(UciOptionValue::Spin(v)) => *v,
                _ => *default,
            };
            let mut val = current;
            let resp = ui.add(DragValue::new(&mut val).range(*min..=*max).speed(1.0));
            if resp.changed() {
                overrides.insert(name.clone(), UciOptionValue::Spin(val));
                *dirty = true;
            }
            ui.label(
                RichText::new(format!("({min}–{max})"))
                    .color(theme::TEXT_FAINT)
                    .size(11.5),
            );
        }

        UciOption::Combo {
            name,
            default,
            vars,
        } => {
            let current = match overrides.get(name) {
                Some(UciOptionValue::Combo(s)) => s.clone(),
                _ => default.clone(),
            };
            let mut selected = current.clone();
            select(ui, ("opt_combo", name), &current, 200.0, |ui| {
                for v in vars {
                    if ui.selectable_value(&mut selected, v.clone(), v).clicked() {
                        overrides.insert(name.clone(), UciOptionValue::Combo(selected.clone()));
                        *dirty = true;
                    }
                }
            });
        }

        UciOption::Str { name, default } => {
            let current = match overrides.get(name) {
                Some(UciOptionValue::Str(s)) => s.clone(),
                _ => default.clone(),
            };
            let mut val = current;
            if ui
                .add(
                    egui::TextEdit::singleline(&mut val)
                        .desired_width(240.0)
                        .hint_text(default),
                )
                .changed()
            {
                overrides.insert(name.clone(), UciOptionValue::Str(val));
                *dirty = true;
            }
        }

        UciOption::Button { name } => {
            // UCI "button" options are one-shot actions with no value. The
            // GUI can't press one on a dead engine, so the toggle means:
            // "send this action to the engine at the start of every game".
            let armed = matches!(overrides.get(name), Some(UciOptionValue::Button));
            let label = if armed {
                "● runs at game start"
            } else {
                "run at game start"
            };
            let color = if armed { theme::SUCCESS } else { theme::TEXT_WEAK };
            if ui
                .add(egui::Button::new(RichText::new(label).color(color)))
                .on_hover_text(if armed {
                    format!(
                        "'{name}' will be triggered (setoption) at the start of every \
                         game. Click to turn off."
                    )
                } else {
                    format!(
                        "'{name}' is an action the engine exposes. Turn this on to \
                         trigger it (setoption) at the start of every game."
                    )
                })
                .clicked()
            {
                if armed {
                    overrides.remove(name);
                } else {
                    overrides.insert(name.clone(), UciOptionValue::Button);
                }
                *dirty = true;
            }
        }
    }
}
