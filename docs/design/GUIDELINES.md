# Colosseum Visual Design Guidelines (v1)

Authoritative spec for modernizing the Colosseum GUI (Rust + egui 0.3x / eframe).
Written so that an implementing model can execute each step **without seeing the
original design discussion**. Follow the tokens and component specs exactly;
when egui cannot express something precisely, the "egui mapping" notes say what
to do instead.

Companion assets (same folder):

| File | What it is |
|---|---|
| `palette.svg` | Color token swatch sheet |
| `components.svg` | Component sheet: buttons, pills, chips, badges, tabs, inputs, table |
| `mockup-live.svg` | Target mockup of the Tournament → Live screen |
| `logo.svg` | Refreshed logo mark (source of truth for the icon) |

---

## 1. Analysis of the current style (June 2026, v0.1.0)

**What already works — keep it:**
- The palette in `crates/colosseum-gui/src/theme.rs` (cool slate + warm gold) is
  distinctive and coherent. Do **not** change the base hex values.
- Single-source-of-truth theme module; all tabs import `theme::*`.
- Type scale (22 / 14.5 / 12 / 13.5 mono) is readable.
- Procedural icon and painter-drawn logo (no asset pipeline) — keep that approach.

**What looks dated — fix it (each item maps to a step in §7):**
1. **No surface hierarchy.** Settings form is a flat wall of labels on
   `BG_PANEL`; sections are just gold text labels. → Card-based sections.
2. **Tabs are plain text links** (`selectable_label`). → Pill tabs with a
   tinted-accent selected state.
3. **Status is plain colored text** (`● Running`). → Status pills (tinted
   rounded chips).
4. **No button hierarchy** beyond the Start button. → Primary / secondary /
   tinted-semantic button styles.
5. **Tables are cramped** (24 px rows) and rank is a plain number. → Taller
   rows, elevated sticky header, medal badges for ranks 1–3, delta chips.
6. **Head-to-head matrix is plain text.** → Heat-tinted cells.
7. **Hardcoded one-off colors** (e.g. `Color32::from_rgb(0x6c, 0x76, 0x86)` in
   `tournament_tab.rs`) bypass the theme. → Promote to a `TEXT_FAINT` token.
8. **Default egui font.** → Embed Inter (UI) + JetBrains Mono (numbers).
9. **Engine list rows are bare checkboxes.** → Selectable card rows.
10. **Empty states are two dim lines of text.** → Centered glyph + title + hint.

---

## 2. Design tokens

### 2.1 Color (all `Color32::from_rgb`)

Existing tokens in `theme.rs` — unchanged:

| Token | Hex | Use |
|---|---|---|
| `BG_DARKEST` | `#12151b` | App chrome: header, status bar, action bars |
| `BG_PANEL` | `#1a1e26` | Panel/window background |
| `BG_ELEVATED` | `#232934` | Cards, table headers, idle buttons |
| `BG_HOVER` | `#2c3340` | Hovered surfaces |
| `BG_INPUT` | `#0e1116` | Text fields, progress trough |
| `BG_FAINT` | `#1f242d` | Table stripe |
| `STROKE` | `#323a47` | Hairline borders |
| `TEXT` | `#e7eaf0` | Primary text |
| `TEXT_WEAK` | `#97a1b1` | Secondary text |
| `ACCENT` | `#e0a93b` | Gold accent |
| `ACCENT_BRIGHT` | `#f2c15e` | Accent hover |
| `SUCCESS` | `#5fb873` | Positive |
| `WARN` | `#d9a54f` | Caution |
| `DANGER` | `#db5d52` | Negative |

**New tokens to add to `theme.rs`:**

```rust
/// Tertiary text: hints, disabled-ish captions. (Replaces the one-off
/// Color32::from_rgb(0x6c, 0x76, 0x86) literals.)
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x6c, 0x76, 0x86);

/// Medal colors for table ranks 1-3.
pub const MEDAL_GOLD: Color32 = ACCENT;                       // #e0a93b
pub const MEDAL_SILVER: Color32 = Color32::from_rgb(0xaa, 0xb4, 0xc4);
pub const MEDAL_BRONZE: Color32 = Color32::from_rgb(0xc0, 0x82, 0x55);

/// Tint helper: a color at fractional strength over dark backgrounds.
/// Use for pill/chip fills (0.16) and their strokes (0.45).
pub fn tint(c: Color32, alpha: f32) -> Color32 {
    c.gamma_multiply(alpha)
}
```

**Tint convention** (used everywhere below): a *tinted* surface of semantic
color `C` = fill `tint(C, 0.16)`, stroke `Stroke::new(1.0, tint(C, 0.45))`,
foreground text `C` itself. Never put saturated semantic colors as large fills.

### 2.2 Typography

Embed two fonts (both SIL OFL 1.1 — ship the license file next to the fonts):

- **Inter** (variable or Regular + SemiBold statics) — all proportional text.
  Source: <https://github.com/rsms/inter/releases> (`Inter-Regular.otf`,
  `Inter-SemiBold.otf`).
- **JetBrains Mono** (`JetBrainsMono-Regular.ttf`) — all monospace/numeric.
  Source: <https://github.com/JetBrains/JetBrainsMono/releases>.

Place files in `crates/colosseum-gui/assets/fonts/` plus `OFL.txt`, embed with
`include_bytes!`, and register in `theme::apply` via `FontDefinitions`:
Inter-Regular as the first `Proportional` font, JetBrainsMono as the first
`Monospace` font. Register `Inter-SemiBold` as an extra family named
`"semibold"` for headings if `.strong()` rendering looks too faux-bold;
otherwise `.strong()` is acceptable.

Type scale (update `text_styles` in `theme::apply`):

| Style | Size | Notes |
|---|---|---|
| Heading | 20.0 | Screen/card titles (was 22 — slightly tighter with Inter) |
| Body | 14.0 | Default |
| Button | 14.0 | |
| Small | 12.0 | Captions, status bar |
| Monospace | 13.0 | Numeric table cells |

Custom sizes used by components: section-card title 14 strong; field label 13;
hint 11.5 `TEXT_FAINT`; table header 12 strong.

### 2.3 Spacing & radii

Spacing scale: **4 / 8 / 12 / 16 / 24**. Concretely:

- `spacing.item_spacing = vec2(8.0, 8.0)` (unchanged)
- Panel inner margin: 16 (unchanged); header/status-bar margins unchanged.
- Card inner margin: 14; gap between cards: 12.
- `spacing.interact_size.y = 28.0` (was 26).
- `spacing.button_padding = vec2(12.0, 6.0)` (was 10,6).

Radii:

| Radius | Use |
|---|---|
| 4 | Chips, progress bar |
| 6 | Buttons, inputs (widget default — unchanged) |
| 8 | Cards, menus |
| 10 | Windows/modals (unchanged) |
| 999 (full) | Pills (tabs, status) |

---

## 3. Component specs

All snippets assume `use crate::theme;` and egui ≥ 0.31 (`CornerRadius`,
`Frame::new()`, integer `Margin`). Add the shared helpers to a new
`crates/colosseum-gui/src/widgets.rs` module so all tabs reuse them.

### 3.1 Pill tab (header navigation)

Replaces `selectable_label` tabs in `app.rs::header`.

- Geometry: text padding 14 horizontal / 6 vertical, `CornerRadius::same(14)`.
- Selected: fill `tint(ACCENT, 0.18)`, text `ACCENT_BRIGHT` strong, no stroke.
- Idle: transparent fill, text `TEXT_WEAK`.
- Hovered (idle only): fill `BG_HOVER`, text `TEXT`.

```rust
/// A pill-shaped tab button. Returns true on click.
pub fn pill_tab(ui: &mut Ui, label: &str, selected: bool) -> bool {
    let text = RichText::new(label).size(14.0);
    let text = if selected { text.color(theme::ACCENT_BRIGHT).strong() }
               else { text.color(theme::TEXT_WEAK) };
    let mut btn = egui::Button::new(text)
        .corner_radius(egui::CornerRadius::same(14))
        .min_size(egui::vec2(0.0, 28.0));
    if selected {
        btn = btn.fill(theme::tint(theme::ACCENT, 0.18)).stroke(egui::Stroke::NONE);
    } else {
        btn = btn.fill(Color32::TRANSPARENT).stroke(egui::Stroke::NONE);
    }
    ui.add(btn).clicked()
}
```

### 3.2 Status pill

Replaces plain `● Running` labels (header/status bar/live control bar).

- Rounded-full chip: fill `tint(C, 0.16)`, stroke `tint(C, 0.45)` 1px,
  inner padding 10×3, dot `●` + label, both colored `C`, size 12.
- Status → color: Running `SUCCESS`, Stopping `WARN`, Stopped `TEXT_WEAK`,
  Finished `ACCENT`, Idle `TEXT_FAINT` (hollow dot `○`).

```rust
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
```

### 3.3 Buttons

Four styles. Heights: 32 for the primary action bar, default elsewhere.

| Style | Fill | Stroke | Text | Use |
|---|---|---|---|---|
| **Primary** | `ACCENT` (hover `ACCENT_BRIGHT` — egui hover handles via manual check or accept static) | none | `BG_DARKEST` strong | Start Tournament, Save |
| **Secondary** | `BG_ELEVATED` | `STROKE` 1px | `TEXT` | New Tournament, Browse…, default |
| **Tinted success** | `tint(SUCCESS,0.16)` | `tint(SUCCESS,0.45)` | `SUCCESS` | ▶ Go |
| **Tinted warn** | `tint(WARN,0.16)` | `tint(WARN,0.45)` | `WARN` | ⏸ Stop |
| **Tinted danger** | `tint(DANGER,0.16)` | `tint(DANGER,0.45)` | `DANGER` | ⏹ Force-Stop, Delete |

```rust
pub fn tinted_button(ui: &mut Ui, label: &str, c: Color32, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled,
        egui::Button::new(RichText::new(label).color(c).size(13.5).strong())
            .fill(theme::tint(c, 0.16))
            .stroke(egui::Stroke::new(1.0, theme::tint(c, 0.45))))
}
```

Disabled state: egui dims automatically; that is acceptable.

### 3.4 Section card

Replaces the bare gold `section()` labels in the tournament settings form, and
wraps the edit panel groups in the Engines tab.

- Frame: fill `BG_ELEVATED`, stroke `STROKE` 1px, `CornerRadius::same(8)`,
  inner margin 14, full available width.
- Title row inside the card: title `TEXT` 14 strong; optional one-line subtitle
  `TEXT_WEAK` 11.5 directly under it; 8px space before the body.
- 12px vertical space between consecutive cards.

```rust
pub fn section_card<R>(ui: &mut Ui, title: &str, subtitle: Option<&str>,
                       body: impl FnOnce(&mut Ui) -> R) -> R {
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
        }).inner;
    ui.add_space(12.0);
    r
}
```

Inside cards, inputs sit on `BG_INPUT` (egui `extreme_bg_color` — already set),
which now reads as "sunken into the card". Grid spacing inside cards:
`[12.0, 8.0]`.

### 3.5 Results table

- Header: 30px tall, background `BG_ELEVATED` (paint via header column
  backgrounds or wrap the table in a card — preferred: wrap whole table in a
  borderless card frame fill `BG_PANEL`, and give the header row labels 12px
  strong `TEXT_WEAK`; sorted column label `ACCENT` + `▲/▼`).
- Rows: **30px** tall (was 24), striped `BG_FAINT` (already on).
- Rank column: ranks 1–3 render a **medal badge** — an 18px circle, fill
  `tint(MEDAL_*, 0.2)`, text `MEDAL_*` 11px strong, centered. Rank ≥ 4: plain
  `TEXT_FAINT` monospace number.

```rust
pub fn rank_badge(ui: &mut Ui, rank: usize) {
    let medal = match rank {
        1 => Some(theme::MEDAL_GOLD),
        2 => Some(theme::MEDAL_SILVER),
        3 => Some(theme::MEDAL_BRONZE),
        _ => None,
    };
    match medal {
        Some(c) => {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
            ui.painter().circle(rect.center(), 9.0, theme::tint(c, 0.2),
                                egui::Stroke::new(1.0, theme::tint(c, 0.5)));
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER,
                              rank.to_string(),
                              egui::FontId::proportional(11.0), c);
        }
        None => { ui.label(RichText::new(rank.to_string())
                    .color(theme::TEXT_FAINT).monospace()); }
    }
}
```

- **Elo Δ chip**: small rounded chip (radius 4, padding 6×1): positive →
  `tint(SUCCESS,0.16)` fill, `SUCCESS` text `+N`; negative → DANGER variant;
  |Δ| ≤ 0.05 → no chip, plain `TEXT_FAINT` `0`.
- Points column stays `ACCENT` strong monospace. NPS and Games stay
  `TEXT_WEAK` monospace.

### 3.6 Head-to-head heatmap

In `head_to_head_matrix`, give every played cell a tinted background by score
share `s = (wins + 0.5*draws) / games`:

- `s > 0.5`: fill `tint(SUCCESS, (s - 0.5) * 0.5)` (max 0.25 at s=1).
- `s < 0.5`: fill `tint(DANGER, (0.5 - s) * 0.5)`.
- `s == 0.5` or no games: no fill.
- Cell text stays as today (`W-D-L` monospace 11.5). Implement by wrapping each
  cell label in a small `egui::Frame` with radius 4 and padding 6×2.

### 3.7 Progress bar

- 6px tall, `CornerRadius::same(4)`, trough `BG_INPUT`, fill `ACCENT`,
  width 160 (unchanged). egui mapping: `ProgressBar::new(frac)
  .desired_width(160.0).desired_height(6.0).corner_radius(4.0)
  .fill(theme::ACCENT)`. Keep the `x / y games` caption to its left
  (`TEXT_WEAK` 13).

### 3.8 Engine list rows (selection + library)

Replace bare checkbox rows with **selectable card rows**, full width:

- Container: `Frame` radius 6, inner margin 10×8.
- Selected: fill `tint(ACCENT, 0.12)`, stroke `tint(ACCENT, 0.4)`.
- Idle: transparent; hovered: `BG_HOVER`.
- Content: row 1 = engine name `TEXT` 13.5 (selected: `ACCENT_BRIGHT`);
  row 2 = version or filename `TEXT_WEAK` 11.5.
- In the Tournament setup list, prepend the checkbox; clicking anywhere on the
  card toggles selection (`Sense::click` on the frame response).
- 4px gap between rows.

### 3.9 Empty states

Centered vertically ~30% down, all centered text:

1. A large dim glyph (e.g. `♟` 40px `TEXT_FAINT`, or paint the two-ring logo
   at 40px with `tint(ACCENT, 0.35)`).
2. Title: `TEXT_WEAK` 15 strong (e.g. "No engines yet").
3. Hint: `TEXT_FAINT` 12.5 (e.g. "Add engines in the Engines tab to get
   started.").
4. Optionally a secondary button (e.g. "Go to Engines").

### 3.10 Modal (close-confirm)

Keep current structure; adjust:
- Width 400. Heading 18 strong, then 8px space.
- Body text `TEXT_WEAK`.
- Button row, right-aligned, order left→right: `Keep running` (secondary),
  `Stop & quit` (primary), `Force-stop & quit` (tinted danger).

---

## 4. Screen-by-screen changes

### 4.1 Header (`app.rs::header`)
- Keep `BG_DARKEST` fill; bump inner margin to `symmetric(16, 8)`.
- Logo + app name unchanged (name 17 strong — slightly smaller, calmer).
- Remove the vertical `separator()`; use 20px space, then pill tabs (§3.1)
  with 4px between pills.
- Right-align a status pill (§3.2) mirroring tournament status so it is
  visible from any tab.
- Add a 1px bottom hairline: `STROKE` (egui: `Frame::new().fill(..)` +
  draw `painter().hline` at panel bottom, or set the panel's
  `show_separator_line` default which already exists — verify it uses `STROKE`).

### 4.2 Status bar
- Replace `● Running` text with a status pill (small variant ok).
- Replace storage/engine-count separators with `·` middots, `TEXT_FAINT` 12.

### 4.3 Tournament → Setup
- Left engine panel: keep header row; rows become selectable cards (§3.8).
- Center form: every `section(...)` block becomes a `section_card` (§3.4):
  Name + Format merge into one "Tournament" card; then "Time Control",
  "Engine Options" (Common options + Syzygy + Ponder), "Adjudication",
  "Elo", "Openings", "Output".
- Bottom action bar: keep `BG_DARKEST`; Start button = primary (§3.3) height
  32; warning/error text unchanged but error uses a tinted-danger chip.

### 4.4 Tournament → Live
- Control bar: status pill + tournament name 15 strong; Go/Stop/Force-Stop as
  tinted buttons (§3.3); progress per §3.7; right side unchanged except
  "Head-to-head" toggle becomes a secondary toggle button.
- Results table per §3.5; H2H per §3.6.
- Engine-errors panel: wrap in a tinted-danger card (fill `tint(DANGER,0.08)`,
  stroke `tint(DANGER,0.35)`, radius 8) instead of a bare bottom panel fill.

### 4.5 Engines tab
- Toolbar buttons: "Add engine…" = primary, "Scan folder…" = secondary,
  progress text unchanged.
- List rows per §3.8 (selected = the engine being edited).
- Edit panel: group into section cards — "Identity" (name, version, Elo),
  "Launch" (path, args, working dir, env), "UCI Options" (detected options
  table), action row (Save = primary, Re-detect = secondary,
  Delete = tinted danger).
- Empty state per §3.9.

---

## 5. Logo & icon

Keep the two-ring amphitheatre mark; refresh per `logo.svg`:
- Outer ring picks up a subtle vertical gold gradient `#f2c15e → #cf942c`
  (egui icon mapping: lerp the gold by `y` position in `icon.rs::icon()`).
- Inner ring stays flat `#b8882f`.
- Background `#1a1e26` rounded square, corner radius 18.75% of size (48/256 —
  unchanged).
- Ring proportions unchanged (`r_outer 0.34·s`, `r_inner 0.185·s`).

The in-app header logo (`app.rs::logo`) stays painter-drawn and flat — no
change needed beyond what exists.

---

## 6. Things that must NOT change

- Base palette hex values (§2.1 existing tokens).
- Layout structure: top header / bottom status bar / central tabs; left panels.
- All behavior: sorting, controls enablement, close-confirm flow, tooltips.
- The painter-drawn logo/icon approach (no PNG assets, except fonts).
- Accessibility floor: never render text below 11px; semantic colors must
  always pair color with a glyph or text (already true — keep it).

---

## 7. Implementation plan (ordered, independently verifiable)

Each step compiles and passes `cargo clippy --workspace` and `cargo test
--workspace` on its own.

1. **Tokens + helpers.** Add `TEXT_FAINT`, `MEDAL_*`, `tint()` to `theme.rs`;
   create `widgets.rs` with `pill_tab`, `status_pill`, `tinted_button`,
   `section_card`, `rank_badge`; replace the `0x6c,0x76,0x86` literals with
   `theme::TEXT_FAINT`. Update `interact_size.y = 28.0`,
   `button_padding = (12,6)`.
2. **Fonts.** Add `assets/fonts/` (Inter Regular/SemiBold, JetBrains Mono
   Regular, OFL.txt), embed in `theme::apply`, set type scale per §2.2.
3. **Chrome.** Header pill tabs + header status pill + status-bar pill (§4.1,
   §4.2). Modal polish (§3.10).
4. **Live view.** Tinted control buttons, progress bar, table (row height,
   rank badges, Δ chips), H2H heatmap, error card (§4.4).
5. **Setup view.** Section cards for the form, selectable engine card rows,
   primary Start button (§4.3).
6. **Engines tab.** Toolbar hierarchy, card rows, edit-panel cards, empty
   state (§4.5).
7. **Icon gradient** (§5) — optional, last.

Acceptance for every step: screenshot the affected screen at 1280×800 and
compare against `mockup-live.svg` / `components.svg`; verify no behavior
changed (existing tests still pass).
