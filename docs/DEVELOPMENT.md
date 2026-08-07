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
cargo run -p colosseum-cli -- --help
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
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p colosseum-docs -- --check
```

Those commands are the required hermetic suite: they use only inputs owned by
the repository. They do not discover an installed engine, read an engine-path
environment variable, or establish a platform or release claim.

Real-engine interoperability coverage is a separate, explicit local smoke
tier. It receives a local UCI executable through `COLOSSEUM_SMOKE_ENGINE`,
copies that executable to a temporary directory, and fails if the variable is
absent or invalid rather than passing by skip. `uci_smoke` expects `Threads`
and `Hash`; `runner_smoke` and `scheduler_smoke` additionally exercise
Stockfish-style strength options. Run the targets appropriate to the selected
engine, for example:

```bash
COLOSSEUM_SMOKE_ENGINE=/path/to/engine \
  cargo test -p colosseum-uci --features real-engine-smoke --test uci_smoke -- --nocapture

COLOSSEUM_SMOKE_ENGINE=/path/to/engine \
  cargo test -p colosseum-engine --features real-engine-smoke --test runner_smoke -- --nocapture

COLOSSEUM_SMOKE_ENGINE=/path/to/engine \
  cargo test -p colosseum-engine --features real-engine-smoke --test scheduler_smoke -- --nocapture
```

On PowerShell, set `$env:COLOSSEUM_SMOKE_ENGINE` before running the same Cargo
commands. These opt-in checks are useful local interoperability evidence only;
they never count as required CI, supported-platform, or release evidence.

GUI and live-view changes need a real run as well: launch the app, play a short
tournament (two engines, 100 ms/move), and delete the test tournament
afterwards.

## Workspace layout

```
colosseum/
├─ crates/
│  ├─ colosseum-core/     Pure domain rules, statistics and opaque identity values
│  ├─ colosseum-application/ Runtime-neutral use cases and ports
│  ├─ colosseum-uci/      UCI protocol & async engine process management (tokio)
│  ├─ colosseum-engine/   Runner/store plus OS topology and affinity adapters
│  ├─ colosseum-gui/      eframe/egui GUI composition root
│  └─ colosseum-cli/      independent headless CLI composition root
├─ tools/release/         product tag/version/changelog validation
├─ tools/docs/            parser-derived CLI command-reference generator
├─ packaging/             Linux desktop entry + icon (.deb / .rpm / Arch assets)
├─ docs/                  This guide, design guidelines, macOS signing notes
└─ .github/workflows/     push/PR CI plus the legacy GUI release workflow
```

The GUI never blocks on engine I/O: a tokio runtime drives all engine
processes, and live state is published behind shared snapshots the UI reads
each frame. Visual rules for the GUI are binding and live in
[`design/GUIDELINES.md`](design/GUIDELINES.md).

## Product versions and release lanes

The GUI and CLI own independent explicit versions in
`crates/colosseum-gui/Cargo.toml` and `crates/colosseum-cli/Cargo.toml`.
Their release notes are similarly separate in
[`CHANGELOG-GUI.md`](../CHANGELOG-GUI.md) and
[`CHANGELOG-CLI.md`](../CHANGELOG-CLI.md). Product tags use `gui-v<semver>` and
`cli-v<semver>`; validate a prepared tag locally with:

```bash
cargo run -p colosseum-release -- gui-v1.0.2
```

Push/pull-request CI runs the hermetic workspace on Windows, Linux and macOS in
debug and release profiles, and independently builds the headless CLI artifact.
Product release automation is split between `release-gui.yml` and
`release-cli.yml`; only their final publication jobs receive write permission.

### CLI candidate before merge

Push the exact `cli` commit with `[cli candidate]` in its subject. This marker
is needed before the workflow exists on `main`; afterward a manual dispatch of
**Colosseum CLI candidate and release** is equivalent. Ordinary `cli` pushes
skip the heavyweight jobs. A candidate creates no tag or GitHub Release. It builds
Windows x64/Arm64, Linux x64 and macOS Arm64 archives, stages only the CLI,
license, README and `docs/cli/`, then runs version/help/self-test/deterministic
JSON smoke against each unpacked archive. The retained aggregate artifact is
named `colosseum-cli-candidate-<full-commit-sha>` and contains checksums plus a
candidate identity file.

Before the final candidate, merge current `main` into `cli`. Any subsequent
code, dependency, CLI documentation, packaging-helper or workflow change
requires a new candidate. After acceptance, merge without squashing away the
tested commit. Create `cli-v<version>` only when the accepted commit is
reachable from `main`; the tag workflow rebuilds, smokes and publishes the
stable artifacts. Tracker/evidence-only commits may follow the candidate when
they cannot affect an archive input.

## CLI documentation

Canonical CLI user documentation lives under `docs/cli/`. The complete command
reference is generated from the same Clap model as the shipped executable:

```bash
cargo run -p colosseum-docs
cargo run -p colosseum-docs -- --check
```

The first command updates `docs/cli/command-reference.md`; the second is the
read-only CI/release drift gate. Edit parser help rather than the generated file.
