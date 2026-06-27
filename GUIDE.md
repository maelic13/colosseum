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
- [ ] **Step 24** — Output & analysis: CSV export, export-PGN-now, SPRT/LOS/error bars, PGN/board viewer · _Large_
- [ ] **Step 25** — Cleanup: remove the unused SQLite `engines` table + dead `Store` engine methods · _Small_

---

### Deferred (architecture-ready)
- [ ] Error-bar / Ordo rating recompute (see Step 24)
- [ ] Engine process pool
- [ ] Tablebase-based adjudication (optional feature, off by default)
- [ ] macOS notarization · UCI_Chess960 · localization
