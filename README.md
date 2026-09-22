<p align="center">
  <img src="docs/design/logo.svg" width="88" alt="Colosseum logo">
</p>

<h1 align="center">Colosseum</h1>

<p align="center">
  Run, watch and rigorously test ordinary UCI chess engines.
</p>

> **Windows · Linux · macOS · GPL-3.0-or-later**

Colosseum is two programs that share one engine-playing core:

| | **Colosseum GUI** — `colosseum` | **Colosseum CLI** — `colosseum-cli` |
|---|---|---|
| Question it answers | *Which of my engines is the strongest?* | *Is this build better than that build, and can I trust the answer?* |
| Made for | Tournaments between many engines, watched live | Engine development: gates, tuning, speed measurement, repeatable experiments |
| Runs as | Desktop application | Headless command line, scriptable |
| Highlights | Live boards, round robins and gauntlets, ratings with error bars, engine library, PGN and CSV export | Fixed matches, pair-atomic SPRT, SPSA tuning, calibration, NPS and scaling, position suites, tournaments, book tools, statistics replay |
| Releases | [`gui-v…` release list](https://github.com/maelic13/colosseum/releases?q=tag%3Agui-v) | [`cli-v…` release list](https://github.com/maelic13/colosseum/releases?q=tag%3Acli-v) |
| Documentation | This page and the [GUI changelog](CHANGELOG-GUI.md) | [Complete CLI guide](docs/cli/README.md) and the [CLI changelog](CHANGELOG-CLI.md) |

Both launch ordinary [UCI](https://www.shredderchess.com/chess-info/features/uci-universal-chess-interface.html)
executables as separate processes. An engine needs no manifest, build
integration or source access. The two products are versioned, packaged and
released independently; install either one or both.

**Not sure which you want?** If you are choosing between engines you did not
write, or you want to watch games, take the GUI. If you are developing an
engine and need to know whether a change helped, take the CLI.

---

## Colosseum GUI

The desktop application plays many games at once and shows them live. It
detects engine options, supports opening books, adjudication, tablebases and
pondering, keeps every finished game, and resumes an interrupted tournament.

![Colosseum — Arena tab with live game view](docs/screenshot.png)

### Install

Pick the newest entry on the [GUI release list](https://github.com/maelic13/colosseum/releases?q=tag%3Agui-v)
and download the file for your machine.

| Platform | File | How to install |
|---|---|---|
| Windows, Intel or AMD | `colosseum-gui-…-windows-x64.msi` | Double-click and follow the installer |
| Windows, Arm (Snapdragon) | `colosseum-gui-…-windows-arm64.msi` | Double-click and follow the installer |
| Windows, no installer | `colosseum-gui-…-windows-….zip` | Unzip and run `colosseum.exe` |
| Ubuntu, Debian, Mint, Pop!_OS | `colosseum-gui-…-linux-x64.deb` | `sudo apt install ./colosseum-gui-….deb` |
| Fedora, RHEL, openSUSE | `colosseum-gui-…-linux-x64.rpm` | `sudo dnf install ./colosseum-gui-….rpm` |
| Arch, CachyOS, EndeavourOS, Manjaro | `colosseum-gui-…-linux-x64.pkg.tar.zst` | `sudo pacman -U ./colosseum-gui-….pkg.tar.zst` |
| Other Linux | `colosseum-gui-…-linux-x64.tar.gz` | Extract and run `./colosseum` |
| macOS, Apple Silicon | `colosseum-gui-…-macos-arm64.dmg` | Open and drag Colosseum to Applications |

Intel Macs are not supported. The macOS app is not signed, so the first launch
is blocked: open **System Settings → Privacy & Security** and click
**Open Anyway**.

### First tournament

1. **Engines tab** — *Add Engine* and pick an executable, or *Scan Folder* to
   add many at once. Set a starting Elo if you know one.
2. **Tournament tab** — tick the engines, choose a format and a time control,
   press *Start Tournament*.
3. **Arena tab** — watch the standings fill, or switch to *Live* to follow a
   game move by move.

Tips:

- Keep **Parallel games** at or below your CPU core count. Engines that share
  a core lose on time.
- **Update ratings** decides whose library Elo the tournament changes. Testing
  one new engine? Choose *Chosen engines* and pick only that one.
- Stopped a tournament? Select it in the Arena list and press *Start*; it
  continues where it left off.
- A game whose engine could not be started at all is shown as aborted and
  not scored. An engine that starts and then crashes, plays an illegal move
  or runs out of time loses that game.

### Where the GUI keeps its files

| | Windows | Linux | macOS |
|---|---|---|---|
| Settings and engine library | `%APPDATA%\Colosseum\` | `~/.config/colosseum/` | `~/Library/Application Support/Colosseum/` |
| Games and logs | `%APPDATA%\Colosseum\` | `~/.local/share/colosseum/` | `~/Library/Application Support/Colosseum/` |

Start Colosseum with `--portable` to keep everything next to the program.
`colosseum --version` prints the installed version.

---

## Colosseum CLI

The command-line tool is built for experiments you can repeat and defend. Every
run records the resolved inputs, the engine executables' hashes, one master
seed, the games, checkpoints and the statistics in a self-contained run
directory. Engine faults are counted and shown, never folded into a score.

### Install

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
directory is on your `PATH`.

To build from source instead, install Rust 1.88 or newer and run:

```text
git clone https://github.com/maelic13/colosseum.git
cd colosseum
cargo build --release -p colosseum-cli --bin colosseum-cli
```

The executable is `target/release/colosseum-cli` (`.exe` on Windows).

### First experiments

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

Tune UCI options with SPSA from a small TOML file that lists the parameters:

```text
colosseum-cli spsa ./engine --tune ./tune.toml --r-end 0.002 --iterations 500 --book ./openings.epd --dir ./runs/tune-1
```

Run directories are created under `./colosseum-runs/` unless `--dir` names
one. Stop a long run with one interrupt and start the same command again to
resume it. Options that every experiment in a project shares can live in a
run file; `colosseum-cli <command> --help` lists every option.

Continue with the [quickstart](docs/cli/quickstart.md), the
[command reference](docs/cli/command-reference.md),
[how to trust a result](docs/cli/trust-results.md) and
[what the CLI needs from an engine](docs/cli/compatibility.md). The
[complete CLI guide](docs/cli/README.md) indexes everything else.

---

## Help and project links

- Something broken or missing? [Open an issue](https://github.com/maelic13/colosseum/issues).
- Release histories: [GUI changelog](CHANGELOG-GUI.md) · [CLI changelog](CHANGELOG-CLI.md)
- Building, testing and releasing from source: [development guide](docs/DEVELOPMENT.md)

---

## Credits and licence

Chess pieces by Colin M.L. Burnett ([cburnett](https://github.com/lichess-org/lila/tree/master/public/piece/cburnett),
CC BY-SA 3.0) · opening names from [lichess chess-openings](https://github.com/lichess-org/chess-openings)
(CC0) · fonts [Inter](https://rsms.me/inter/) and [JetBrains Mono](https://www.jetbrains.com/lp/mono/)
(SIL OFL 1.1).

Colosseum is free software under the **GNU General Public License v3.0 or
later** — see [LICENSE](LICENSE). Copyright © 2026 Miloslav Macůrek.
