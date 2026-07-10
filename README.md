<p align="center">
  <img src="docs/design/logo.svg" width="88" alt="Colosseum logo">
</p>

<h1 align="center">Colosseum</h1>

<p align="center">
  Run chess engines against each other — and watch them play.
</p>

![Colosseum — Arena tab with live game view](docs/screenshot.png)

> **v1.0.2 · Windows · Linux · macOS · Free and open source (GPL-3.0)**

---

## What it does

- **Play engines against each other** — round robin or gauntlet, many games at once.
- **Watch live** — full board, move list with opening names, evaluation graph, clocks, and each engine's search output.
- **Get real ratings** — Elo computed from all results at once (with error bars), not a running K-factor tally.
- **Any UCI engine** — add one executable or scan a whole folder; options are detected automatically.
- **Set it up your way** — time controls from bullet to fixed depth, opening books, adjudication rules, endgame tablebases, pondering.
- **Nothing gets lost** — every game is saved as it finishes; stop any time and resume later.
- **Take the results with you** — live PGN output and CSV export of standings and crosstables.

---

## Download & install

Get the file for your system from the
[latest release](https://github.com/maelic13/colosseum/releases/latest).
Release downloads are the only supported way to install Colosseum.

### Windows

| Your PC | File | How to install |
|---|---|---|
| Intel or AMD | `…-windows-x86_64.msi` | Double-click, then follow the installer |
| Arm (Snapdragon) | `…-windows-arm64.msi` | Double-click, then follow the installer |
| No install wanted | `…-windows-….zip` | Unzip and run `colosseum.exe` |

### Linux (64-bit)

| Your distribution | File | How to install |
|---|---|---|
| Ubuntu, Debian, Mint, Pop!_OS | `….deb` | `sudo apt install ./colosseum-….deb` |
| Fedora, RHEL, openSUSE | `….rpm` | `sudo dnf install ./colosseum-….rpm` |
| Arch, CachyOS, EndeavourOS, Manjaro | `….pkg.tar.zst` | `sudo pacman -U ./colosseum-….pkg.tar.zst` |
| Anything else | `….tar.gz` | Extract and run `./colosseum` |

### macOS (Apple Silicon)

Download `…-macos-aarch64.dmg`, open it, and drag Colosseum to Applications.
Intel Macs are not supported.

The app is not signed yet, so macOS blocks it on first launch. Open
**System Settings → Privacy & Security** and click **Open Anyway**.

---

## Getting started

1. **Engines tab** — click *Add Engine* and pick an engine executable
   (or *Scan Folder* to add many at once). Set a starting Elo if you know it.
2. **Tournament tab** — tick the engines you want, choose a format and time
   control, then hit *Start Tournament*.
3. **Arena tab** — watch the standings fill in, or switch to *Live* to follow
   a game move by move.

Tips:

- Keep **Parallel games** at or below your CPU core count — engines that share
  cores lose on time.
- **Update ratings** decides whose library Elo the tournament changes. Testing
  one new engine? Set it to *Chosen engines* and pick only that one.
- Stopped a tournament? Select it in the Arena list and press *Start* to
  continue where it left off.

---

## Where your files are

| | Windows | Linux | macOS |
|---|---|---|---|
| Settings & engines | `%APPDATA%\Colosseum\` | `~/.config/colosseum/` | `~/Library/Application Support/Colosseum/` |
| Games & logs | `%APPDATA%\Colosseum\` | `~/.local/share/colosseum/` | `~/Library/Application Support/Colosseum/` |

Start Colosseum with `--portable` to keep everything next to the program instead.

---

## Help & more

- Something broken or missing? [Open an issue](https://github.com/maelic13/colosseum/issues).
- What changed in each version: [CHANGELOG.md](CHANGELOG.md)
- Building from source: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)

---

## Credits & license

Chess pieces by Colin M.L. Burnett ([cburnett](https://github.com/lichess-org/lila/tree/master/public/piece/cburnett),
CC BY-SA 3.0) · opening names from [lichess chess-openings](https://github.com/lichess-org/chess-openings)
(CC0) · fonts [Inter](https://rsms.me/inter/) and [JetBrains Mono](https://www.jetbrains.com/lp/mono/)
(SIL OFL 1.1).

Colosseum is free software under the **GNU General Public License v3.0 or
later** — see [LICENSE](LICENSE).
Copyright © 2026 Miloslav Macůrek.
