# Changelog

All notable changes to Colosseum are documented here.  
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).  
Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

#### Windows engines on macOS & Linux (Wine)
- Classic Windows-only UCI engines (Rybka, Houdini, Critter, Shredder, …) now
  run on macOS (via Rosetta 2) and Linux x86-64. Both 64-bit and 32-bit
  `.exe` engines are supported (WoW64 builds of Wine).
- On first add of a Windows engine, a dialog offers to download a pinned,
  SHA-256-verified portable Wine build into the app data folder (nothing is
  installed system-wide). Declining skips those engines; the question is asked
  again on the next add. A system-installed Wine is detected automatically and
  used when present (on arm64 Linux, install Hangover — it provides `wine`).
- Per-engine `Runtime` dropdown in the engine detail panel (Auto / Native /
  managed Wine / system Wine). `Auto` is the default and recommended.
- Each Wine engine gets its own isolated wineprefix under the app data dir,
  created on add and removed when the engine is deleted.
- Engine cards show a warning badge (e.g. `⚠ WIN x64`) for any binary that is
  not native to the host — including x64 engines on Windows ARM64, which run
  through the OS's built-in Prism emulation out of the box.
- Folder scan now recognizes `.exe` files on macOS/Linux.

#### Packaging
- Linux `.deb`, `.rpm`, and Arch (`.pkg.tar.zst`) packages built via nfpm.

### Removed
- Flatpak manifest and Flatpak distribution (replaced by native Linux
  packages; the Flatpak sandbox conflicts with launching Wine).

### Known issues
- Under Wine on macOS a few engines stall in search (stuck at depth 1) even
  though they handshake fine: Rybka 2.3/3/4, Houdini 1.5a, Deep Junior
  Yokohama. Verified working there: Critter 1.6a, Fruit 2.1, Deep Shredder 13,
  Deep HIARCS 14. Appears to be an upstream Wine-on-macOS issue with these
  engines' console-polling loops (reproduced on Wine 11.0 stable, 11.10 devel
  and staging, with and without esync).

---

## [0.1.0] — 2026-06-09

### Added

#### Engine Management
- Engine library tab: add a single executable or scan an entire folder
- Auto-detection via UCI handshake: reads `id name`, `id author`, and all `option` lines
- Typed option editors per UCI option type (check, spin, combo, string, button)
- Editable metadata: name, version, Elo, launch args, working directory, env vars
- Two-click delete confirmation; re-detect button refreshes options in place
- Engines persisted to `engines.json` in the OS config directory

#### Tournaments
- Round-robin format with configurable cycles and games-per-pair
- Time control: time-per-move with value + unit picker (ms / s / min); down to 10 ms/move
- Concurrency: configurable parallel games (semaphore-limited)
- Common forwarded engine options: Threads, Hash (MB), SyzygyPath, Syzygy50MoveRule,
  Ponder (default off for fair fast games)
- Adjudication: draw (eval threshold + consecutive plies), resign/win (eval threshold),
  max-move count — each individually toggleable
- Elo policy: per-game (default), end-of-tournament, or never

#### Opening Books
- EPD files: one position per line; extra opcodes ignored; FEN validated via shakmaty
- PGN files: first N plies of each game become one opening line
- Opening order: sequential or random (deterministic SplitMix64 shuffle from a seed,
  reproducible across resume)
- Optional count cap; live preview (count + sample label) while picking
- Both engines play the same opening from both sides per encounter

#### Live Results Table
- Updates after every game without blocking the GUI
- Columns: engine name / version / Elo / Elo Δ / points / games played / W-D-L /
  head-to-head matrix / avg NPS
- Click any column header to sort ascending; click again for descending
- Engine errors and crash messages surfaced in a dedicated error panel

#### Controls
- **Go**: start a new tournament or resume a stopped one
- **Stop**: graceful drain — no new games started, in-flight games count as real results
- **Force-Stop**: immediate kill of all engine processes; in-flight games discarded;
  tournament remains resumable from completed games only

#### Persistence & Resume
- SQLite database per data directory (or `--portable` for next-to-executable)
- Full tournament config snapshot stored for reproducibility and audit
- Opening FEN + pre-moves persisted per game; resume reuses the same openings
- Per-game PGN appended live to the user-chosen output file
- Correct FEN tags and fullmove numbering in PGN for non-standard start positions

#### GUI
- Dark theme with warm gold accent; custom procedurally-drawn arena icon
- Scalable two-pane layouts (egui `SidePanel`) that fill any window size
- Window geometry persisted and restored on next launch
- Confirm-on-close dialog while a tournament is running
- High-DPI support via eframe's native scaling

#### Distribution
- Single self-contained native binary; no runtime, no installer required for portable use
- Release CI: Windows MSI + zip, Linux tar.gz, macOS DMG + tar.gz
  (triggered by `v*.*.*` git tags)
- Flatpak manifest for Linux distribution (GNOME Platform 46)
- `docs/macos-signing.md`: guide for future codesign + notarization
- `--portable` flag: keeps all data next to the executable

### Technical notes
- Rust workspace: `colosseum-core`, `colosseum-uci`, `colosseum-engine`, `colosseum-gui`
- MSRV: Rust 1.88 (edition 2024)
- 70 unit + integration tests; tests use real Stockfish (skipped gracefully when absent)
- Guaranteed child-process cleanup on game end and app exit (Windows job objects /
  Unix process groups via `tokio::process`)

---

[Unreleased]: https://github.com/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/releases/tag/v0.1.0
