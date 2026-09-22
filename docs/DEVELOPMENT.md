# Development guide

Maintainer-facing notes: building from source, running the tests, and cutting
a release. [`README.md`](../README.md) introduces both products;
[`README-CLI.md`](../README-CLI.md) and [`cli/`](cli/README.md) are the CLI
landing page and complete user guide.

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

## One build entry point

`cargo xtask` builds, packages and release-checks both products, and the two
release workflows call the same commands, so a local artifact and a published
one come from one recipe:

```bash
cargo xtask build   <gui|cli> [--target <triple>] [--profile release|ci-release]
cargo xtask package <gui|cli> [--target <triple>] [--format <list>] [--no-smoke]
cargo xtask release-check <gui-vX.Y.Z|cli-vX.Y.Z>
```

The product is positional and one command handles one product: nothing builds
or packages both, and the CLI archive never contains the GUI. `--target`
defaults to the host's triple and is always passed to cargo, so every build
lands under `target/<triple>/<profile>/`; builds are `--locked`.

`build` compiles and prints the binary's path, and copies nothing. `package`
always uses the `release` profile, writes artifacts to `target/dist/` as
`<product>-<version>-<platform>-<arch>.<ext>`, prints each one's SHA-256, and
reads the portable archive back with its smoke script unless `--no-smoke`.
`--format` takes a comma-separated list and defaults to the platform's portable
archive — `zip` on Windows, `tar.gz` elsewhere:

| Product | Formats |
|---|---|
| `gui` | `zip`, `tar.gz`, `msi`, `deb`, `rpm`, `dmg`, `pkg.tar.zst` |
| `cli` | `zip`, `tar.gz` |

A format whose tool is not installed is an error, never a skip. The installer
formats need their platform's tooling — WiX for `msi`, `cargo-deb` and
`cargo-generate-rpm` for Linux, `create-dmg` for `dmg`, `makepkg` for
`pkg.tar.zst` — which is why each one is built on its own runner.

```bash
cargo xtask package cli                       # this host's portable archive
cargo xtask package gui --format zip,msi      # what the Windows release leg runs
cargo xtask release-check cli-v0.1.0
```

On macOS a bare executable opened from Finder always spawns a Terminal window,
so the `dmg` wraps the binary in an app bundle with a Dock icon, stamped with
the product manifest's version. The bundle is ad-hoc signed: fine on the
machine that built it, but distributing it to other Macs requires codesigning
and notarization — see [`macos-signing.md`](macos-signing.md).

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
├─ tools/release/         product tag/version/changelog validation, archive staging and smoke
├─ tools/xtask/           the build/package/release-check entry point both workflows call
├─ tools/docs/            parser-derived CLI command-reference generator
├─ packaging/             Linux desktop entry + icon (.deb / .rpm / Arch assets)
├─ tests/fixtures/        vendored statistics fixtures and their generator
├─ docs/cli/              CLI user guide (shipped in every CLI archive)
├─ docs/architecture/     architecture, ADRs, phase records and acceptance evidence
├─ docs/design/           binding GUI visual guidelines and design assets
├─ docs/                  this guide and the macOS signing notes
└─ .github/workflows/     push/PR CI plus the GUI and CLI release workflows
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
`cli-v<semver>`; validate prepared tags locally with:

```bash
cargo xtask release-check gui-v1.1.0
cargo xtask release-check cli-v0.1.0
```

`release-check` validates the tag's prefix and shape, that the product manifest
carries exactly that version and that the product's changelog has a section for
it, then checks the generated command reference against the parser and the
working tree for whitespace damage.

Push/pull-request CI runs the hermetic workspace on Windows, Linux and macOS in
debug and optimized profiles, and independently builds the headless CLI
artifact. The optimized leg uses `ci-release`, which inherits the shipped
release profile — same `opt-level`, assertions and overflow checks off — but
drops the distribution-only whole-program LTO and single codegen unit, and
keeps symbols so a CI failure still has a backtrace. Those settings change no
behaviour the suite can observe and cost minutes across forty test binaries.
Run it locally the same way:

```bash
cargo test --workspace --all-targets --profile ci-release
```

The shipped binary is still built with the full `release` profile; only the
test legs use `ci-release`.
Product release automation is split between `release-gui.yml` and
`release-cli.yml`; only their final publication jobs receive write permission.
Each verifies the exact artifact list by name and count before publishing it,
so a release carries what its matrix produced and nothing else. Checksums are
generated and re-checked between jobs but are not published as an asset: the
per-asset digests GitHub records are what to compare a download against.

Both lanes also build a candidate on a manual dispatch: every artifact is
built, packaged and smoked, and the bundle is retained as
`colosseum-<product>-candidate-<full-commit-sha>` with its checksums and a
candidate identity file, without a tag and without publishing anything.

### CLI candidate before merge

Push the exact `cli` commit with `[cli candidate]` in its subject. This marker
is needed before the workflow exists on `main`; afterward a manual dispatch of
**Colosseum CLI candidate and release** is equivalent. Ordinary `cli` pushes
skip the heavyweight jobs. A candidate creates no tag or GitHub Release. It
builds Windows x64/Arm64, Linux x64 and macOS Arm64 archives, stages only the
CLI, license, CLI-specific README, CLI changelog and `docs/cli/`, then checks
packaged documentation links and runs version/help/self-test/deterministic JSON
smoke against each unpacked archive. The retained aggregate artifact is named
`colosseum-cli-candidate-<full-commit-sha>` and contains checksums plus a
candidate identity file.

Rerun the candidate after changing CLI code, dependencies, user documentation
or packaging. After acceptance, merge `cli` to `main` using the repository's
preferred merge strategy and tag the resulting stable source with
`cli-v<version>`. The tag workflow rebuilds, smokes and publishes the final
artifacts. The ordinary CI workflow remains responsible for the complete
debug/release workspace test matrix; release packaging does not duplicate it.

## CLI documentation

Canonical CLI user documentation lives under `docs/cli/`. The complete command
reference is generated from the same Clap model as the shipped executable:

```bash
cargo run -p colosseum-docs
cargo run -p colosseum-docs -- --check
```

The first command updates `docs/cli/command-reference.md`; the second is the
read-only CI/release drift gate. Edit parser help rather than the generated file.
GitHub renders the documents from each release tag, and CLI archives contain
the same guide for offline use. The archive's concise `README.md` directs users
to `docs/cli/README.md`; release notes link to the tagged online copy.
