<p align="center">
  <img src="docs/logo.svg" width="88" alt="Colosseum logo">
</p>

<h1 align="center">Colosseum CLI</h1>

<p align="center">
  Rigorously test ordinary UCI chess engines from the command line.
</p>

> **Windows · Linux · macOS · GPL-3.0-or-later**

Colosseum CLI answers *is this build better than that build, and can I trust
the answer?* It plays fixed matches, pair-atomic SPRT gates, SPSA tunes,
calibration runs, NPS and scaling measurements, position suites, round robins
and gauntlets, with opening-book tools and statistics replay. It launches
ordinary [UCI](https://www.shredderchess.com/chess-info/features/uci-universal-chess-interface.html)
executables as separate processes: an engine needs no manifest, build
integration or source access.

**Looking for the desktop application?** Colosseum, for running and watching
engine tournaments, is developed in its own repository,
[maelic13/colosseum-gui](https://github.com/maelic13/colosseum-gui), and
drives this CLI.

---

## What it does

Colosseum CLI is built for experiments you can repeat and defend. Every
run records the resolved inputs, the engine executables' hashes, one master
seed, the games, checkpoints and the statistics in a self-contained run
directory. Engine faults are counted and shown, never folded into a score.

## Install

Pick the newest entry on the [CLI release list](https://github.com/maelic13/colosseum/releases?q=tag%3Acli-v),
download the archive for your platform (`colosseum-cli-…-windows-x64.zip`,
`…-windows-arm64.zip`, `…-linux-x64.tar.gz` or `…-macos-arm64.tar.gz`),
extract it anywhere and check it:

```text
colosseum-cli --version
colosseum-cli self-test
```

The archive contains the executable, the licence and the complete guide for
that exact version. On Linux and macOS run `./colosseum-cli` unless the
directory is on your `PATH`. To check the download arrived intact, compare its
SHA-256 with the digest GitHub shows beside that asset on the release page
(`Get-FileHash` on Windows, `sha256sum` on Linux, `shasum -a 256` on macOS).
If you pin a version in automation, pin that digest with it.

To build it yourself instead, see [Build from source](#build-from-source).

## First experiments

Inspect two engines, then play a fixed match between them at 100 ms a move:

```text
colosseum-cli engine check ./candidate
colosseum-cli engine check ./baseline
colosseum-cli match ./candidate ./baseline --games 100 --a-movetime-ms 100 --b-movetime-ms 100
```

Gate a change with a sequential test that stops as soon as the evidence is in:

```text
colosseum-cli sprt ./candidate ./baseline --preset gainer --max-pairs 5000 --book ./openings.epd --dir ./runs/gate-42
```

A schedule draws one opening per colour-reversed pair and refuses to start if
the book cannot cover it, rather than quietly replaying positions it has
already played — so size the book to the run, or pass `--book-wrap` to reuse
openings deliberately.

Tune UCI options with SPSA from a small TOML file that lists the parameters:

```text
colosseum-cli spsa ./engine --tune ./tune.toml --r-end 0.002 --iterations 500 --book ./openings.epd --dir ./runs/tune-1
```

Run directories are created under `./colosseum-runs/` unless `--dir` names
one. Stop a long run with one interrupt and start the same command again to
resume it.

The conditions every experiment in a project shares — the book, the
concurrency, the seed — belong in a run file rather than in each command line,
so every result was measured the same way:

```toml
# colosseum.toml, beside your books/ directory
[options]
concurrency = 4
placement = "auto"
book = "books/openings.epd"
book-order = "random"
seed = 42
```

```text
colosseum-cli --run-file ./colosseum.toml match ./candidate ./baseline --games 100
```

A path inside a run file is relative to the file itself, not to where you run
the command, so the file travels with the project. A run file can carry the
whole command instead, and anything on the command line replaces what the file
says. `colosseum-cli <command> --help` lists every option, and
[run files](docs/cli/run-files.md) covers inheritance and unsetting.

Continue with the [quickstart](docs/cli/quickstart.md), the
[command reference](docs/cli/command-reference.md),
[how to trust a result](docs/cli/trust-results.md) and
[what the CLI needs from an engine](docs/cli/compatibility.md). The
[complete CLI guide](docs/cli/README.md) indexes everything else.

---

## Build from source

Install **Rust 1.88 or newer**, then clone the repository:

```text
git clone https://github.com/maelic13/colosseum.git
cd colosseum
```

Windows on Arm also needs `clang`; the
[development guide](docs/DEVELOPMENT.md#prerequisites) has the details.

To try it straight away:

```text
cargo run -p colosseum-cli -- --help
```

`cargo xtask` is the one entry point for real builds, and the release workflow
calls the same commands, so what you build locally is what a release ships:

```text
cargo xtask build   cli          # the executable, path printed
cargo xtask package cli          # the downloadable archive, into target/dist/
```

`package` also prints the archive's SHA-256 and unpacks it to check it. The
[development guide](docs/DEVELOPMENT.md#one-build-entry-point) covers
cross-target builds and the release checks.

---

## Help and project links

- Something broken or missing? [Open an issue](https://github.com/maelic13/colosseum/issues).
- Release history: [CLI changelog](CHANGELOG-CLI.md)
- Building, testing and releasing from source: [development guide](docs/DEVELOPMENT.md)

---

## Licence

Colosseum CLI is free software under the **GNU General Public License v3.0 or
later** — see [LICENSE](LICENSE). Copyright © 2026 Miloslav Macůrek.
