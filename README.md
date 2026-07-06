# Colosseum ♟ Chess Engine Testing Suite

A cross-platform desktop application for running engine-vs-engine chess tournaments
with a **live, sortable results table**.

![Colosseum screenshot — tournament live view](docs/screenshot-placeholder.png)

> **v0.1.0 · GPL-3.0-or-later · Rust + egui**

---

## Features

| | |
|---|---|
| **Engine library** | Add one engine or scan a whole folder; auto-detects UCI options (Threads, Hash, …); stores name, version, and Elo; editable launch args / working directory / env vars |
| **Round-robin tournaments** | Configurable cycles, concurrency, time control (time-per-move from 10 ms up) |
| **Live results table** | Sortable by Name / Elo / Elo Δ / Points / W-D-L; head-to-head matrix; avg NPS |
| **Go / Stop / Force-Stop** | Stop drains in-flight games gracefully; Force-Stop aborts and discards them — tournament stays resumable |
| **Opening books** | EPD or PGN files; sequential or random (deterministic seed); configurable plies; both engines play same opening from both sides |
| **Adjudication** | Draw (eval threshold + consecutive plies), resign (eval threshold), max-move count — all individually toggleable |
| **Elo policy** | Per-game (default), end-of-tournament, or never |
| **Common engine options** | Threads, Hash (MB), SyzygyPath, Syzygy50MoveRule, Ponder (default off) — forwarded to every engine |
| **PGN export** | Appended live per game to a user-chosen file; correct tags for non-standard start positions |
| **Persistence & resume** | SQLite backend; every tournament is resumable after Stop or unexpected exit |
| **Portable mode** | Pass `--portable` to keep all data next to the executable |

---

## Download

Pre-built binaries for every release are on the
[GitHub Releases](https://github.com/releases) page:

| Platform | Download |
|---|---|
| Windows x86-64 | `colosseum-vX.Y.Z-windows-x86_64.msi` (installer) or `.zip` (portable) |
| Windows ARM64 | `colosseum-vX.Y.Z-windows-arm64.msi` (installer) or `.zip` (portable) |
| Linux x86-64 | `colosseum-vX.Y.Z-linux-x86_64.tar.gz` |
| macOS Apple Silicon | `colosseum-vX.Y.Z-macos-aarch64.dmg` or `.tar.gz` |

> **macOS note:** v0.1 is unsigned. On first launch Gatekeeper may block it.
> Open **System Settings → Privacy & Security → Open Anyway**, or run:
> ```bash
> xattr -d com.apple.quarantine /Applications/Colosseum.app
> ```
> See [`docs/macos-signing.md`](docs/macos-signing.md) for the signing roadmap.

> **Linux Flatpak:** a Flatpak manifest is included in `flatpak/` for building
> locally or submitting to Flathub. See the build instructions in that file.

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

### Clone and run (debug build)

```bash
git clone https://github.com/chess_tournament
cd chess_tournament
cargo run --bin colosseum
```

### Optimized release build

```bash
cargo build --release --bin colosseum
# Binary is at:  target/release/colosseum        (Linux / macOS)
#                target\release\colosseum.exe     (Windows)
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

The integration tests in `colosseum-engine` run real Stockfish games.
They look for Stockfish at `D:\chess\engines\stockfish.exe` (Windows) and skip
gracefully when the engine is absent — so CI on other machines still passes.

To run those tests on your machine, either place a Stockfish binary at the path
above, or set the `COLOSSEUM_TEST_ENGINE` environment variable:

```bash
COLOSSEUM_TEST_ENGINE=/usr/games/stockfish cargo test --workspace
```

---

## Data locations

By default Colosseum stores its data in the OS-standard directories:

| Platform | Config (engines.json, config.toml) | Data (tournaments.db, PGN) |
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
│  ├─ colosseum-core/     Pure domain: types, Elo, pairings, adjudication
│  ├─ colosseum-uci/      UCI protocol & async process management (tokio)
│  ├─ colosseum-engine/   Orchestration: runner, scheduler, store, openings
│  └─ colosseum-gui/      eframe/egui GUI — the shipped binary
├─ flatpak/               Flatpak manifest + desktop/AppStream metadata
├─ docs/                  macOS signing guide
└─ .github/workflows/     release (tag-triggered cross-platform packaging)
```

The GUI never blocks on engine I/O: a tokio runtime drives all engine processes;
results are pushed over channels and drained each egui frame (~30 Hz cap so fast
games don't starve the UI).

---

## Releasing

```bash
# Tag a release — the release.yml workflow fires automatically
git tag v0.1.0
git push origin v0.1.0
```

This builds binaries on all four platform/arch combinations, runs smoke tests,
and publishes a GitHub Release with auto-generated release notes.

See `CHANGELOG.md` for the full version history.

---

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
