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
- [ ] **Step 11** — Cross-platform packaging & release (cargo-dist; Flatpak/MSI/DMG/tarball) · _Small_
- [ ] **Step 12** — README, CHANGELOG, docs polish · _Small_

---

### Deferred (post-v1, architecture-ready)
- [ ] Tournament History tab UI (`list`/`load`/`delete`/`resume`)
- [ ] Game / board viewer (PGN replay)
- [ ] More formats: gauntlet, SPRT, knockout
- [ ] Error-bar / Ordo rating recompute
- [ ] Engine process pool
- [ ] Tablebase-based adjudication (optional feature, off by default)
- [ ] macOS notarization · UCI_Chess960 · localization
