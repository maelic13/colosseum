# Development guide

Maintainer-facing notes: building from source, running the tests, and cutting
a release. [`README.md`](../README.md) introduces the CLI;
[`packaging/cli/README.md`](../packaging/cli/README.md) (the front page of
every CLI archive) and [`cli/`](cli/README.md) are the CLI landing page and
complete user guide. The desktop application is developed in its own
repository, [maelic13/colosseum-gui](https://github.com/maelic13/colosseum-gui);
the 1.x egui application it replaces stays on `main` until the merge that
ends the programme, and its documents are archived in
[`archive/gui-1.x/`](archive/gui-1.x/README.md).

## Prerequisites

| Requirement | Notes |
|---|---|
| **Rust 1.88+** | `rustup update stable` (edition 2024) |
| **C linker** | Windows: MSVC build tools; Linux: `gcc`; macOS: Xcode CLT |
| **clang** | Windows ARM64 only — see below |

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
cargo run -p colosseum-cli -- --help
```

## One build entry point

`cargo xtask` builds, packages and release-checks the CLI, and the release
workflow calls the same commands, so a local artifact and a published one come
from one recipe:

```bash
cargo xtask build   cli [--target <triple>] [--profile release|ci-release]
cargo xtask package cli [--target <triple>] [--format <list>] [--no-smoke]
cargo xtask release-check <cli-vX.Y.Z>
```

The product is the positional `cli`. `--target` defaults to the host's triple
and is always passed to cargo, so every build lands under
`target/<triple>/<profile>/`; builds are `--locked`.

`build` compiles and prints the binary's path, and copies nothing. `package`
always uses the `release` profile, writes artifacts to `target/dist/` as
`<product>-<version>-<platform>-<arch>.<ext>`, prints each one's SHA-256, and
reads the portable archive back with its smoke script unless `--no-smoke`.
`--format` defaults to the platform's portable archive — `zip` on Windows,
`tar.gz` elsewhere — the only format the CLI ships.

```bash
cargo xtask package cli                       # this host's portable archive
cargo xtask release-check cli-v0.2.0
```

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

How the suite is laid out:

- Logic that needs no process — statistics, schedules, parsers, report and
  progress formatting, resume arithmetic — is unit-tested beside its code in
  `src/`. Where a rule depends on time or the host, the code takes the instant
  or the CPU topology as an input so the test can supply a synthetic one.
- Each crate's integration tests drive only what needs a real process. The
  CLI's are one binary, `crates/colosseum-cli/tests/cli/`, with a module per
  behaviour; they run the `colosseum-cli` binary against the repository's
  fixture engines (`colosseum-uci-fixture` and the hidden `__uci-stub` mode)
  for a handful of games, and stop runs with the hidden
  `--__stop-after-units` hook rather than a timed kill.
- No test asserts a wall-clock upper bound, a throughput or a memory figure,
  or depends on which CPUs the host has; measured time is checked only from
  below.

The phase acceptance manifests under `docs/fixtures/` record the evidence as
it stood when each phase closed. Test files and names they cite predate the
suite's reorganisation by behaviour and are historical.

Real-engine interoperability coverage is a separate, explicit local smoke
tier. It receives a local UCI executable through `COLOSSEUM_SMOKE_ENGINE`,
copies that executable to a temporary directory, and fails if the variable is
absent or invalid rather than passing by skip. `uci_smoke` expects `Threads`
and `Hash`; `runner_smoke` additionally exercises Stockfish-style strength
options. Run the targets appropriate to the selected
engine, for example:

```bash
COLOSSEUM_SMOKE_ENGINE=/path/to/engine \
  cargo test -p colosseum-uci --features real-engine-smoke --test uci_smoke -- --nocapture

COLOSSEUM_SMOKE_ENGINE=/path/to/engine \
  cargo test -p colosseum-engine --features real-engine-smoke --test runner_smoke -- --nocapture
```

On PowerShell, set `$env:COLOSSEUM_SMOKE_ENGINE` before running the same Cargo
commands. These opt-in checks are useful local interoperability evidence only;
they never count as required CI, supported-platform, or release evidence.

Hard CPU affinity has its own opt-in `platform-smoke` tier. `affinity_smoke`
pins two busy fixture processes to one logical CPU each and checks that they
ran only there; it needs a host with enforceable affinity (Windows or Linux)
and fails elsewhere rather than passing by skip:

```bash
cargo test -p colosseum-engine --features platform-smoke --test affinity_smoke
```

## Workspace layout

```
colosseum/
├─ crates/
│  ├─ colosseum-core/     Pure domain rules, statistics and opaque identity values
│  ├─ colosseum-application/ Runtime-neutral use cases and ports
│  ├─ colosseum-uci/      UCI protocol & async engine process management (tokio)
│  ├─ colosseum-engine/   One-game runner, PGN, openings, incident forensics; OS topology and affinity adapters
│  └─ colosseum-cli/      the headless CLI composition root
├─ tools/release/         tag/version/changelog validation, archive staging and smoke
├─ tools/xtask/           the build/package/release-check entry point the workflow calls
├─ tools/docs/            parser-derived CLI command-reference generator
├─ packaging/cli/         README.md, the front page of every CLI archive
├─ tests/fixtures/        vendored statistics fixtures and their generator
├─ docs/cli/              CLI user guide (shipped in every CLI archive)
├─ docs/architecture/     architecture, ADRs, phase records and acceptance evidence
├─ docs/archive/gui-1.x/  the retired egui application's guidelines, changelog and notes
├─ docs/gui-v2/           a pointer to the desktop application's repository
├─ docs/                  this guide and the logo
└─ .github/workflows/     push/PR CI and the CLI release workflow
```

## Product versions and release lanes

The CLI owns its version in `crates/colosseum-cli/Cargo.toml` and its release
notes in [`CHANGELOG-CLI.md`](../CHANGELOG-CLI.md). Tags are `cli-v<semver>`
until the repository swap moves the lane to plain `v<semver>` (ADR-0012);
validate a prepared tag locally with:

```bash
cargo xtask release-check cli-v0.2.0
```

`main` still carries the 1.x desktop application and its `gui-v` lane until the
merge that ends the programme; a `gui-v1.1.x` patch is made there, never here.

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
behaviour the suite can observe and cost minutes across the test binaries.
Run it locally the same way:

```bash
cargo test --workspace --all-targets --profile ci-release
```

The shipped binary is still built with the full `release` profile; only the
test legs use `ci-release`.
Release automation is `release-cli.yml`; only its final publication job
receives write permission. It verifies the exact artifact list by name and
count before publishing it, so a release carries what its matrix produced and
nothing else. Checksums are
generated and re-checked between jobs but are not published as an asset: the
per-asset digests GitHub records are what to compare a download against.

The lane also builds a candidate on a manual dispatch: every artifact is
built, packaged and smoked, and the bundle is retained as
`colosseum-cli-candidate-<full-commit-sha>` with its checksums and a candidate
identity file, without a tag and without publishing anything.

### Candidates and tags

A candidate is made from **Actions → Colosseum CLI candidate and release →
Run workflow**, on `main`, or from a terminal:

```bash
gh workflow run release-cli.yml --ref main
```

A candidate creates no tag or GitHub Release. The CLI lane builds Windows
x64/Arm64, Linux x64 and macOS Arm64 archives, stages only the CLI, license,
CLI-specific README, CLI changelog and `docs/cli/`, then checks packaged
documentation links and runs version/help/self-test/deterministic JSON smoke
against each unpacked archive. Download the retained bundle from the run's **Artifacts** section
to inspect it; Actions keeps it for the repository's artifact retention period.

Rerun a candidate after changing the CLI's code, dependencies, user
documentation or packaging. Once it is accepted, tag the same commit on `main`
with `cli-v<version>` and push the tag. The tag workflow proves the commit is
on `main`, rebuilds and re-smokes every artifact, and only then creates the
GitHub Release with the artifacts attached as
release assets. The ordinary CI workflow remains responsible for the complete
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
