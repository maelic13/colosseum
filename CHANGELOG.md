# Changelog

All notable changes to Colosseum are documented here.  
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).  
Versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [1.0.1] — 2026-07-10

A packaging and release-tooling patch; no functional changes to the app.

### Fixed
- macOS `.dmg` is now a proper installer: the disk image opens to a branded
  Finder window (light background with the gold-ring motif) with the
  Colosseum app icon and a drag-to-`/Applications` shortcut, carries a
  custom volume icon, and the bundled `Colosseum.app` shows its app icon.
  Previously the CI-built DMG contained a bare, icon-less `.app` and no
  Applications shortcut.
- The app now reports its version as 1.0.1, so a fresh install no longer
  claims an update is available

### Changed
- CI: GitHub Actions dependencies bumped to their latest majors (Node 24
  runtime), Homebrew tap-trust warning silenced

---

## [1.0.0] — 2026-07-09

The first stable release. Since 0.1.0 the app has been redesigned around the
**Arena** tab (live standings + live game view), gained real ratings math,
UCI pondering, and Linux packages.

### Added

#### Arena (replaces the Results tab)
- Live game view with a `Standings | Live` switcher: full-size board
  (bundled cburnett pieces), move list with ECO opening names (embedded
  lichess openings database), material balance, and per-engine panels —
  logo, live eval, depth/nodes/nps, big ticking clocks, and a Fritz-style
  per-depth search output log
- Evaluation graph between the engine panels, fed by **both** engines every
  ply — including the eval of the engine pondering on its opponent's time
- Games rail for parallel play with auto-follow; when a watched game ends,
  auto-follow jumps to the newly launched replacement game from move one
- Standings lens: sortable table (Elo, Δ, points, W-D-L, avg nps, avg depth,
  time/move, forfeits), head-to-head matrix (counts or per-game results),
  termination breakdown, and a settings summary in the side rail
- Tournament management: rename (inline in the header or via the list's
  right-click menu), multi-select with ctrl/shift click, context-menu
  Start / Stop / Force-stop / Delete for single or bulk selections
- Collapsible tournaments list and games rail; auto-load of the newest
  tournament on entry
- Incident forensics: engine crashes/timeouts/illegal moves write a per-game
  report (UCI transcript + stderr) under `data/logs/incidents/`

#### Ratings
- Ordo-style joint maximum-likelihood ratings, recomputed from the full
  standings after every game (never incremental K-factor), anchored to the
  participants' tournament-start ratings, with Fisher-information error bars
- Bayesian damping: virtual draws against each engine's prior keep a single
  upset from producing absurd rating swings
- Library write-back modes: **Never**, **All engines**, or **Chosen
  engines** (checkbox picker); applied after every finished game — the
  non-chosen engines stay anchored, visually and in the math

#### Engines & tournament setup
- Full UCI **pondering**: engines think on the opponent's time
  (`go ponder` / `ponderhit`), bail-outs and prediction misses handled;
  the live view shows "pondering…" with streaming search output
- **Use endgame tablebases** tournament option: off withholds every
  tablebase option from the engines without touching the global paths
- Gauntlet format (seeds vs. the field) alongside round robin
- Time controls: sudden death, base + increment, fixed nodes, fixed depth —
  in addition to time per move
- Global endgame-tablebase configuration (Syzygy / Nalimov / Gaviota paths,
  probe caches, 50-move rule, compression) shared by all engines
- Engine logos (auto-matched from a logo folder), change-executable, clone,
  multi-select with context menu (re-detect, open containing folder, delete)
- Allowlist-based UCI option mapping: thread/hash values are forwarded to
  whatever the engine actually calls them, clamped to declared ranges —
  and never to look-alikes (Rybka's "CPU Usage" throttle stays untouched)
- Config presets + last-used configuration restored on start
- Per-engine tournament overrides (beat library and common options)

#### App
- About dialog with a real **Check for updates** (queries GitHub releases,
  opens the download page; degrades gracefully offline)
- Theme setting: Dark / Light / System, persisted
- Avg depth and time/move statistics per engine, persisted per game
- One-transaction schedule inserts (large tournaments no longer freeze the
  UI on start)

### Changed
- Engine identity is "name version" everywhere a single string names an
  engine (PGN tags, live view, standings, error messages)
- Tournament deletion force-stops and unloads a running tournament first;
  bulk delete handles the currently loaded tournament correctly
- Store connections use WAL plus a busy timeout — concurrent GUI/driver
  writes no longer fail spuriously
- Force-stopped games are requeued at the front of the schedule and replay
  in launch order on resume; round numbering stays correct
- Termination counts and play clocks survive stop/resume
- Linux packaging: `.deb` and `.rpm` packages (with desktop entry and icon)
  replace the Flatpak manifest

### Fixed
- Engines reporting `nps 0` (e.g. Fruit) get their speed derived from
  nodes/time
- Live "Playing" panel shows all in-flight games and drops killed ones
  after Force-Stop
- Countless live-view layout fixes: no stray scrollbars above the minimum
  size, stable engine-card headers at any width, hover-stable widgets

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
