# Colosseum — Progress Guide

Quick, glanceable progress tracker. Each step is one commit. See [PLAN.md](PLAN.md) for full
detail. Model labels: **Small** = Sonnet 4.6 / GPT-5.5 medium · **Large** = Opus 4.8 high
thinking / GPT-5.5 extra high.

## Phase A — Foundation
- [x] **Step 0** — Initialize repository (git, `.gitignore`, GPL-3.0 `LICENSE`) · _Small_
- [x] **Step 1** — Author `PLAN.md` + `GUIDE.md` · _Small_
- [x] **Step 2** — Architecture scaffold (workspace, crates, deps, public seams, CI stub) · _Large_

## Phase B — Backend (logic, no UI)
- [x] **Step 3** — `colosseum-core`: domain types, Elo, pairings, adjudication, standings + unit tests · _Large_
- [x] **Step 4** — `colosseum-uci`: UCI protocol & process management + temp-Stockfish tests · _Large_
- [x] **Step 5** — `colosseum-engine`: scheduler, Go/Stop/Force-Stop, PGN, persistence + integration tests · _Large_
- [x] **Step 6** — Persistence & resume wiring; `--portable` mode · _Small_

## Phase C — GUI
- [x] **Step 7** — GUI scaffold, modern theme + app icon, backend bridge, close-confirm · _Large/Small_
- [x] **Step 8** — Engine Management tab (add, add-folder, auto-detect & edit options, metadata) · _Small_
- [x] **Step 9** — Tournament tab: options, Go/Stop/Force-Stop, **live sortable results table** · _Large_
- [x] **Step 10** — Starting positions / openings (EPD + PGN, integrated UI) — final feature · _Large_

## Phase D — Release
- [x] **Step 11** — Cross-platform packaging & release (cargo-dist; Flatpak/MSI/DMG/tarball) · _Small_
- [x] **Step 12** — README, CHANGELOG, docs polish · _Small_

---

### v0.1.0 shipped ✓

All 12 steps complete. See [README.md](README.md) for build & run instructions,
[CHANGELOG.md](CHANGELOG.md) for what's in v0.1.0, and [PLAN.md](PLAN.md) for the
full architecture reference.

---

## Phase E — Post-v1 enhancements (v0.2+)

Ordered by value/effort; see [PLAN.md §11](PLAN.md) for detail.

- [x] **Step 13** — Engine identity: detect `author` into `meta.extra`, parse version from `id name`, show Author field · _Small_
- [x] **Step 14** — Apply/sync resulting Elo from a tournament back to the engine library · _Small_
- [x] **Step 15** — Per-game timing → live **avg game time / elapsed / throughput / ETA**; set `games.started_at` · _Small_
- [x] **Step 16** — Wire **Resume** into the GUI (`resume_tournament` already exists) · _Small_
- [x] **Step 17** — **Tournament History** tab (`list`/`load`/`delete`/`resume`) · _Large_
- [x] **Step 18** — Live **"currently playing"** panel (in-flight games) · _Small_
- [x] **Step 19** — **Termination breakdown** summary in live view · _Small_
- [x] **Step 20** — Engines-tab usability: broken-path indicator, clone, search/filter+sort, per-option reset, open-folder, Button-option handling · _Small_
- [x] **Step 21** — Time controls: increment / sudden-death / nodes / depth · _Large_
- [x] **Step 22** — Gauntlet format + honest Format control (knockout / SPRT deferred — need a dynamic scheduler) · _Large_
- [x] **Step 23** — Config presets + remember last-used config · _Small_
- [x] **UI hardening** — systemic fixes for poorly-visible controls and layout jitter (see [PLAN.md §11](PLAN.md)) · _Small_
- [x] **Step 24** — Output & analysis: CSV export, export-PGN-now, SPRT/LOS/error bars, PGN/board viewer · _Large_
  - [x] **24a** — CSV standings/crosstable export + export-PGN-now (Export ▾ menu in live + history)
  - [x] **24b** — SPRT / LOS / Elo error bars (match-stats card for 2-engine tournaments)
  - [x] **24c** — Built-in PGN/board viewer (History → Games → View; live view → Games toggle)
- [x] **Step 25** — Cleanup: remove the unused SQLite `engines` table + dead `Store` engine methods · _Small_
- [x] **Step 26** — Engines-tab visual refresh: monogram avatars, version chips, author-as-subtitle, redesigned edit header (en-croissant-inspired, UCI-sourced identity) · _Small_
- [x] **Step 27** — Board live view fix (responsive wrapped control bar + finished-only Games list), shared engine-row widgets in tournament setup, pill-tab hover, overflow/wrap fixes, resize/fullscreen hardening · _Small_
- [x] **Step 28** — Engines tab restructure: 50% card grid + split right panel, engine logos (pick/copy/aspect-fit render), scrollable filtered UCI options (hide threads/hash/TB paths), global Syzygy/Gaviota/Nalimov tablebase paths applied at tournament start · _Large_
- [x] **Step 29** — Engines tab polish: fixed-size aligned cards (sorted A–Z), autosave with debounce (Save button removed), pinned Clone/Delete row, card context menu (clone / re-detect / open folder / copy path / delete-with-modal), high-quality logo rendering (per-size Lanczos textures), half-width identity fields + larger logo, smarter name/version detection (SIMD-noise stripping, multi-token versions like "14 WCSC") · _Medium_
- [x] **Step 30** — Engines tab en-croissant card redesign (GUIDELINES §3.9): spacious 98-pt cards targeting ~360 pt width (1–3 columns), name + author subtitle on top, labeled ELO / VERSION stat columns at the bottom, no chips inside cards; repaint-while-detecting fix (background detect results no longer wait for the next input event) · _Small_
- [x] **Step 31** — Engines tab polish round 2: solid non-overlapping scrollbars (global), rounded-corner logo rendering (textured `RectShape`), logo centered in header space with a stable Remove button, compact filter field with inline ✕ clear, multi-column UCI options on wide panels, collapsible Endgame Tablebases bar (collapsed by default with per-format ✓/— summary), body clipped via `CentralPanel` so the status bar survives small windows · _Small_
- [x] **Step 32** — Embedded fonts + Engines tab polish round 3: Inter (UI) / Inter-SemiBold (real bold via `theme::semibold`) / JetBrains Mono (numbers) embedded for identical cross-platform rendering, missing-glyph "tofu squares" eliminated (× clear button, ●/○ tablebase summary, painted disclosure triangle, plain "saved" flash), zero hover expansion (no more clipped focus rings / shifting buttons), whole detail column scrollable, rounded-square monogram avatars matching logo silhouette, aspect-adaptive header logo box (wide logos use the width), persisted grid sort (Name / Elo / Author in `AppConfig::engines_sort`), tablebases summary hidden when too narrow · _Medium_
- [x] **Step 33** — Micro-polish: sort popup sized to its three entries (no phantom scrollbar), context menu padding (`menu_margin` 10 + roomier item spacing), `selectable_labels = false` theme-wide so card/label text isn't text-selectable; verified UCI defaults display against a live engine dump (Shredder's odd values — Ponder=true, UCI_Elo=1400 — are genuinely what the engine declares) · _Small_
- [x] **Step 34** — Tablebase probe caches + startup polish: global Nalimov/Gaviota cache sizes (`NalimovCache` / `GaviotaTbCache`, default 32 MB, on `AppConfig`, edited inline in the tablebases bar, injected at tournament start when the matching path is set, hidden from per-engine editors), sort selector switched to a `MenuButton` popup (ComboBox popups force a ScrollArea that flashed a scrollbar), window starts hidden and is revealed after the first painted frame (no startup blink); design decisions consolidated as GUIDELINES §7 for the Tournament/History redesign · _Small_
- [x] **Step 35** — UCI options measured layout: column count derived from the widest actual row (label + editor + range hint measured via galley layout) instead of a fixed 420-pt estimate, so engines with long option names (Rybka's milipawn set) fall back to fewer columns and never collide; each column clipped as a hard guarantee; tablebases rows reordered Syzygy → Nalimov → Gaviota · _Small_
- [x] **Step 36** — Scroll/header fixes: options-column clip made horizontal-only (the vertical clip broke scrolling inside the detail ScrollArea — options were cut mid-row and unreachable at small windows), fixed-geometry engine header (identity fields and logo slot at identical position/size for every engine; logo image aspect-fitted and centered on a constant point, Remove row always reserved so nothing shifts), word-wise engine filter ("shredder 12" matches name+version terms independently), startup reveal delayed to the third pumped frame to kill the residual first-paint blink · _Small_
- [x] **Step 37** — Shape-language unification: header tabs and status pills are rounded rectangles at the app-wide radius 6 (no more pill radius 14/10), checkboxes are rounded squares radius 3 via new `widgets::checkbox` (egui's default radius 6 made ~16 px boxes read as circles — converted everywhere incl. Tournament tab), and one standard `widgets::clear_button` ("×" right of the field, appears when non-empty) shared by the tablebase rows and the engine filter · _Small_
- [x] **Step 38** — Scrollbar-free dropdowns app-wide: new `widgets::select` (MenuButton-based dropdown with painted arrow) replaces every `egui::ComboBox` — UCI combo options, tournament Format/Time-Control/Elo-policy/time-unit selectors, engines sort — because combo popups force a ScrollArea that shows a phantom scrollbar even for 3–5 items · _Small_
- [x] **Step 39** — Ponder hidden from the per-engine UCI editor: the scheduler always forwards the tournament's Ponder setting (overriding any per-engine value), so the per-engine checkbox was non-functional; Ponder joins Threads/Hash as "managed elsewhere" (set per tournament in Engine Options), tooltip updated to list all globally-managed options; plus 14 pt padding after the last UCI option row (before the pinned Clone/Delete divider) · _Tiny_

---

### Deferred (architecture-ready)
- [ ] Error-bar / Ordo rating recompute (see Step 24)
- [ ] Engine process pool
- [ ] Tablebase-based adjudication (optional feature, off by default)
- [ ] macOS notarization · UCI_Chess960 · localization
