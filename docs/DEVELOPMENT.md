# Development guide

Maintainer-facing notes: building from source, running the tests, and cutting
a release. User documentation lives in [`README.md`](../README.md).

## Prerequisites

| Requirement | Notes |
|---|---|
| **Rust 1.88+** | `rustup update stable` (edition 2024) |
| **C linker** | Windows: MSVC build tools; Linux: `gcc`; macOS: Xcode CLT |
| **clang** | Windows ARM64 only — see below |
| **GUI libraries** | Linux only — see below |

### Linux GUI dependencies

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

### Windows ARM64: clang

On ARM64 Windows the `ring` crate compiles its assembly with `clang`; MSVC
alone fails with `ToolNotFound: failed to find tool "clang"`. Install
[LLVM](https://releases.llvm.org/) (`winget install LLVM.LLVM`) and make sure
it is on `PATH` — the installer does not add it by default — or point `CC` at
it for the build:

```powershell
$env:CC = "C:\Program Files\LLVM\bin\clang.exe"
cargo build --release
```

x86-64 Windows is unaffected, and GitHub's ARM runners ship LLVM, so CI needs
no extra step.

## Build and run

```bash
git clone https://github.com/maelic13/colosseum.git
cd colosseum
cargo run --release --bin colosseum
```

One-step scripts put a distributable artifact in `dist/`:

```bash
./build_macos.sh      # dist/Colosseum.app (double-clickable, no Terminal window)
./build_linux.sh      # dist/colosseum
.\build_windows.ps1   # dist\colosseum.exe
```

On macOS a bare executable opened from Finder always spawns a Terminal window,
so the script wraps the binary in a minimal app bundle with a Dock icon. The
bundle is ad-hoc signed: fine on the machine that built it, but distributing it
to other Macs requires codesigning and notarization — see
[`macos-signing.md`](macos-signing.md).

## Tests

```bash
cargo check --workspace --tests
cargo clippy --workspace
cargo test --workspace
```

The integration tests in `colosseum-engine` play real engine games. They look
for Stockfish at a developer-machine path and skip gracefully when it is
absent, so CI elsewhere still passes. To run them locally:

```bash
COLOSSEUM_TEST_ENGINE=/usr/games/stockfish cargo test --workspace
```

GUI and live-view changes need a real run as well: launch the app, play a short
tournament (two engines, 100 ms/move), and delete the test tournament
afterwards.

## Workspace layout

```
colosseum/
├─ crates/
│  ├─ colosseum-core/     Pure domain: config, pairings, standings, ML ratings, adjudication
│  ├─ colosseum-uci/      UCI protocol & async engine process management (tokio)
│  ├─ colosseum-engine/   Orchestration: scheduler, game runner, SQLite store, openings
│  └─ colosseum-gui/      eframe/egui GUI — the shipped binary
├─ packaging/             Linux desktop entry + icon (.deb / .rpm / Arch assets)
├─ docs/                  This guide, design guidelines, macOS signing notes
└─ .github/workflows/     release.yml (release-triggered cross-platform packaging)
```

The GUI never blocks on engine I/O: a tokio runtime drives all engine
processes, and live state is published behind shared snapshots the UI reads
each frame. Visual rules for the GUI are binding and live in
[`design/GUIDELINES.md`](design/GUIDELINES.md).

## Releasing

1. Bump `version` in the workspace `Cargo.toml`, update
   [`CHANGELOG.md`](../CHANGELOG.md) and the version line in the README.
2. Commit as `Version X.Y.Z` and push.
3. On GitHub: **Releases → Draft a new release**, create the `vX.Y.Z` tag,
   write the description, and publish.

Publishing triggers `release.yml`, which builds Windows (x64 + ARM64), Linux,
and macOS binaries, runs smoke tests, and attaches the
MSI / ZIP / DEB / RPM / pkg.tar.zst / DMG / tar.gz assets to that release a few
minutes later. The hand-written description is never touched.
