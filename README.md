<p align="center">
  <img src="docs/design/logo.svg" width="88" alt="Colosseum logo">
</p>

<h1 align="center">Colosseum Chess Engine Testing Suite</h1>

A cross-platform desktop application for running UCI engine-vs-engine chess
tournaments — parallel games, a live board view, and maximum-likelihood Elo
ratings.

![Colosseum — Arena tab with live game view](docs/screenshot.png)

> **v1.0.0 · GPL-3.0-or-later · Rust + egui**

---

## Features

| | |
|---|---|
| **Engine library** | Add one engine or scan a whole folder; auto-detects UCI options over the handshake; per-engine logo, launch args, working directory, env vars; robust option mapping for old/quirky engines |
| **Tournament formats** | Round robin and gauntlet, configurable cycles and games per pair, parallel games |
| **Time controls** | Time per move, sudden death, base + increment, fixed nodes, fixed depth |
| **Pondering** | Full UCI ponder support — engines think on the opponent's time, and the live view shows it |
| **Arena: standings** | Live sortable standings (Elo, Δ, points, W-D-L, avg nps/depth, time/move), head-to-head matrix, termination breakdown, settings summary |
| **Arena: live view** | Full board with bundled piece set, move list with ECO opening names, material balance, per-engine panels (eval, depth, nodes, nps, clocks, Fritz-style search output) and an evaluation graph fed by both thinking *and* pondering engines |
| **Ratings** | Ordo-style joint maximum-likelihood ratings with error bars, anchored to tournament-start values; library write-back per game — for all engines, none, or a chosen subset |
| **Adjudication** | Draw (eval threshold over consecutive moves), resign, max-moves — plus all natural endings |
| **Opening books** | EPD or PGN; sequential or seeded-random order; both engines play each opening from both sides |
| **Endgame tablebases** | Syzygy / Nalimov / Gaviota paths configured once, forwarded to every engine — and switchable off per tournament |
| **Transport** | Start / Stop (drain in-flight games) / Force-Stop (abort & requeue); every tournament resumes after a stop or crash |
| **Export** | Per-game live PGN output, standings/crosstable CSV |
| **Persistence** | SQLite backend; presets for tournament setups; incident reports for engine crashes/timeouts |
| **Portable mode** | Pass `--portable` to keep all data next to the executable |

---

## Download

Pre-built packages for every release are on the
[GitHub Releases](https://github.com/maelic13/colosseum/releases) page:

| Platform | Download |
|---|---|
| Windows x86-64 | `.msi` installer or portable `.zip` |
| Windows ARM64 | `.msi` installer or portable `.zip` |
| Linux x86-64 | `.deb` (Debian/Ubuntu), `.rpm` (Fedora/openSUSE), or `.tar.gz` |
| macOS Apple Silicon | `.dmg` or `.tar.gz` |

Intel Macs are not supported.

> **macOS note:** builds are unsigned. On first launch Gatekeeper may block
> the app. Open **System Settings → Privacy & Security → Open Anyway**, or run:
> ```bash
> xattr -d com.apple.quarantine /Applications/Colosseum.app
> ```
> See [`docs/macos-signing.md`](docs/macos-signing.md) for the signing roadmap.

---

## Build & Run

### Prerequisites

| Requirement | Notes |
|---|---|
| **Rust 1.88+** | `rustup update stable` |
| **C linker** | Windows: MSVC build tools; Linux: `gcc`; macOS: Xcode CLT |
| **Linux GUI libs** | `libgtk-3-dev libxcb-*-dev libxkbcommon-dev` (see below) |

#### Install Linux GUI dependencies

```bash
# Debian / Ubuntu
sudo apt-get install -y \
  libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libxkbcommon-dev libssl-dev

# Fedora / RHEL
sudo dnf install gtk3-devel libxcb-devel libxkbcommon-devel openssl-devel

# Arch
sudo pacman -S gtk3 libxcb libxkbcommon openssl
```

### Clone and run

```bash
git clone https://github.com/maelic13/colosseum.git
cd colosseum
cargo run --release --bin colosseum
```

### One-step build scripts

Each script builds the optimized binary and puts the distributable artifact
in `dist/`:

```bash
./build_macos.sh      # dist/Colosseum.app (double-clickable, no Terminal window)
./build_linux.sh      # dist/colosseum
.\build_windows.ps1   # dist\colosseum.exe
```

macOS note: a bare executable opened from Finder always spawns a Terminal
window, so the macOS script wraps the binary in a minimal app bundle with a
Dock icon. The bundle is ad-hoc signed: fine on the machine that built it;
distributing it to other Macs requires codesigning/notarization.

### Run tests

```bash
cargo test --workspace
```

The integration tests in `colosseum-engine` run real engine games. They look
for Stockfish at a developer-machine path and skip gracefully when the engine
is absent — CI on other machines still passes. To run them locally, set:

```bash
COLOSSEUM_TEST_ENGINE=/usr/games/stockfish cargo test --workspace
```

---

## Data locations

By default Colosseum stores its data in the OS-standard directories:

| Platform | Config (engines.json, config.toml) | Data (tournaments.db, logs) |
|---|---|---|
| Windows | `%APPDATA%\Colosseum\` | `%LOCALAPPDATA%\Colosseum\` |
| Linux | `~/.config/colosseum/` | `~/.local/share/colosseum/` |
| macOS | `~/Library/Application Support/Colosseum/` | same |

Pass `--portable` to keep everything next to the executable instead.

---

## Architecture

```
colosseum/
├─ crates/
│  ├─ colosseum-core/     Pure domain: config, pairings, standings, ML ratings, adjudication
│  ├─ colosseum-uci/      UCI protocol & async engine process management (tokio)
│  ├─ colosseum-engine/   Orchestration: scheduler, game runner, SQLite store, openings
│  └─ colosseum-gui/      eframe/egui GUI — the shipped binary
├─ packaging/             Linux desktop entry + icon (.deb / .rpm assets)
├─ docs/                  Design guidelines, macOS signing guide
└─ .github/workflows/     release.yml (tag-triggered cross-platform packaging)
```

The GUI never blocks on engine I/O: a tokio runtime drives all engine
processes; live state is published behind shared snapshots the UI reads each
frame.

---

## Releasing

Create a release by hand on GitHub (**Releases → Draft a new release**):
pick or create the `vX.Y.Z` tag, write the title and description, and
publish. Publishing triggers the `release.yml` workflow, which builds
Windows (x64 + ARM64), Linux, and macOS binaries, runs smoke tests, and
attaches the MSI/ZIP/DEB/RPM/DMG/tar.gz artifacts to that release a few
minutes later. The hand-written description is left untouched.

See [`CHANGELOG.md`](CHANGELOG.md) for the full version history.

---

## Credits & bundled assets

- Chess pieces: [cburnett set](https://github.com/lichess-org/lila/tree/master/public/piece/cburnett)
  by Colin M.L. Burnett — CC BY-SA 3.0 (`crates/colosseum-gui/assets/pieces/cburnett/`)
- Opening names: [lichess chess-openings](https://github.com/lichess-org/chess-openings)
  — CC0 (`crates/colosseum-gui/assets/openings/`)
- Fonts: [Inter](https://rsms.me/inter/) and
  [JetBrains Mono](https://www.jetbrains.com/lp/mono/) — SIL Open Font
  License 1.1 (`crates/colosseum-gui/assets/fonts/`, license texts included)

## License

Colosseum is free software licensed under the
**GNU General Public License v3.0 or later**.  
See [`LICENSE`](LICENSE) for the full text.

```
Copyright (C) 2026  Miloslav Macůrek

This program is free software: you can redistribute it and/or modify it under
the terms of the GNU General Public License as published by the Free Software
Foundation, either version 3 of the License, or (at your option) any later
version.
```
