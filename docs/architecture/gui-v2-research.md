# GUI v2 — research and recommendation

Written 2026-09-25 for the maintainer's decision on the desktop
application's future: whether the three-tab design is right, whether the
application looks and works like a modern tool, and whether egui is the
right technology for a re-implementation on the harness (PLAN §Phase 11).
This note records the evidence and a recommendation; it decides nothing.

## 1. What the application is today

Three tabs in one window — **Tournament** (setup), **Arena** (standings and
the live board), **Engines** (library) — on eframe/egui 0.35, about
15,000 lines in `crates/colosseum-gui`, of which the three tab modules are
8,800 and `widgets.rs` and `theme.rs` another 1,500. It plays games through
its own scheduler and SQLite store, not the harness.

The visual guidelines (`docs/design/GUIDELINES.md` §7) are, read as a list,
a catalogue of the framework's limits rather than of design decisions:

- widgets must never change size on hover or selection, because egui's
  selectable label gains padding on hover and shifts everything to its right;
- `egui::ComboBox` is banned for a phantom scrollbar; a `MenuButton` stands in;
- `Column::auto` is banned on live data because immediate-mode layout jitters;
- real bold needed a second embedded font family (egui had one weight per
  family until 0.34's variable-font support);
- a glyph allowlist, because arrows and checkmarks render as tofu;
- images cached per physical pixel size with Lanczos, because GPU minification
  of logos was ugly;
- panels rather than frames because frames do not clip; a two-point inner
  margin so focus rings survive panel edges; startup hidden for one frame to
  avoid a flash.

Each is a reasonable workaround. Together they are the cost of building a
data-heavy application on a toolkit designed for tools and game overlays:
the application carries its own widget library, and every new screen
re-learns the same constraints. egui has improved in 2026 (0.34 moved to the
skrifa font engine with hinting and variable fonts; 0.35 added CSS-like
classes; wgpu is the default backend; AccessKit accessibility works), but the
immediate-mode fundamentals behind the rules above have not changed.

Usability, from the screenshot and the guidelines: a tournament is set up on
one tab and watched on another, so the two halves of one task live apart;
the Arena tab stacks tournament controls, a standings/live switch, a
tournaments list and a games rail into one screen; there is no browsing of
finished games in the application (users export PGN); the tournaments list
and the engine library are lists without a detail pane; nothing in the
application reads a CLI run directory, so a tune or a gate has no desktop
view at all.

## 2. The style reference: ChessBase for Mac

The pre-release page (macland.chessbase.com, release announced for November
2026) shows one window with:

- a **sidebar** on the left, in sections (Analysis board, Preparation,
  Repertoire, Library; Train: video courses, tactics, Playchess; then the
  databases as a list; the account at the bottom) — "the sidebar puts
  analysis, training, databases and the cloud one click away";
- a **toolbar** above the content with search and filters;
- **list + detail** as the standard layout: the game list narrows as filters
  change and the selected game opens beside it with board and notation;
- a light, native macOS look (system typography, hairline separators, blue
  board), compact rows, nothing decorative.

That is the macOS split-view idiom (sidebar / list / detail), not a chess
idea. It is what a Colosseum user would recognise as "modern": navigation
by place in a sidebar instead of tabs, a persistent detail pane, and every
list searchable. It is reproducible in any technology below; the question
is which one reproduces it with the least fighting.

## 3. Options for the product shape

**A. Keep the desktop application, keep tabs.** Cheapest; does not address
the complaint.

**B. Desktop application with a sidebar, list + detail, one window
(recommended).** Sections that follow the harness's own nouns:

| Sidebar | Content | Detail pane |
|---|---|---|
| Arena | Everything running now: one card per run with its live boards; click a board to follow it | The followed game: board, clocks, both engines' search, evaluation graph |
| Tournaments | Searchable list of tournaments (running, stopped, finished) | Standings, crosstable, games list with a board viewer, terminations, settings, export; a "New tournament" sheet |
| Engines | Library as a list with logo, name, version, Elo, last used | Inspector: executable, options with the UCI schema, logo, rating history, "check" result |
| Runs (later) | The CLI's run directories under a chosen folder: matches, gates, tunes | `status`, progress, `spsa history` as a chart, `result.json` figures |
| Settings (bottom) | Appearance, storage, updates, defaults | |

Games become browsable because the harness writes `games.pgn` per run; the
Arena stops being the only place to look at a game.

**C. No desktop application: the harness serves a web dashboard.** A
`colosseum-cli serve` and a browser. Attractive for watching a tournament
from another machine, and it is how OpenBench and fishtest present engine
testing. On its own it is the wrong product for the maintainer's own use
(an engine library, file pickers, a tournament wizard, system tray) and for
the Windows users the installers serve. **But it is why the technology
choice below matters:** a web front end can be both the desktop
application's UI and, later, the dashboard the harness serves. A
Rust-native toolkit cannot become a dashboard.

## 4. Technology candidates

Evaluated against what this application needs: dense tables that sort and
scroll (standings, games, engine options), a chess board with SVG pieces
drawn at any size, a live-updating evaluation chart, per-move search lines
at ten updates a second across several games, a sidebar / list / detail
layout, light and dark themes, embedded fonts, packaging for Windows x64 and
arm64, macOS arm64 and three Linux formats, and — because the maintainer
builds through coding agents — how reliably an agent produces correct,
good-looking code in it. Licensing is not a constraint: Colosseum is
GPL-3.0-or-later and every candidate is compatible.

Sources: the 2026 Rust GUI survey (blog.wybxc.cc, 2026-08-22, tested 50
libraries on macOS for IME and screen-reader support), the projects'
release pages, and the discussion of iced 0.14 on Hacker News.

| | egui (stay) | iced 0.14 | Slint 1.18 | GPUI + gpui-component 0.6 | Tauri 2 + web front end | Dioxus native | Xilem |
|---|---|---|---|---|---|---|---|
| Model | immediate | retained, Elm | retained, declarative DSL | retained tree, GPU | system WebView, HTML/CSS/TS | React-like, WGPU HTML renderer | reactive on Masonry |
| Maturity | 0.36, monthly patches | 0.14 after >1 year | 1.18, ~quarterly minors since 1.0 (2023) | pre-1.0, versions follow Zed | 2.11 stable; 3.0 alpha adds a bundled Chromium (CEF) runtime | 0.7 stable on WebView; native renderer experimental, 0.8 alpha | alpha |
| Text and fonts | good since 0.34 | good | good | good (Zed) | best available | — | good |
| Tables, virtual lists | hand-built | limited, a stated priority | `ListView` virtualised; `StandardTableView` text-only cells, custom tables built from `ListView` | virtualised table with resizable, sortable columns; dock layouts | any: TanStack Table, AG Grid, plain CSS grid | — | limited |
| Layout that never shifts on hover | no (the §7 rules) | yes | yes | yes | yes | yes | yes |
| Theming, native feel | hand-built theme | custom styling, "less intuitive than CSS" | ships `fluent`, `cupertino`, `material`, `cosmic` widget styles | one polished modern style (Zed-like), semantic tokens, many themes | CSS; native window chrome via Tauri; macOS overlay title bar | CSS | — |
| Chart | hand-drawn | hand-drawn | `Path` | built-in charts | uPlot, Recharts, ECharts | — | — |
| Accessibility, IME | works (AccessKit) | none yet | works | works with caveats (`text!` macro) | works (platform) | — | works |
| Windows arm64, macOS arm64, Linux | all | all | all (SDKs published) | all; a Windows arm64 rendering regression was found and fixed in 2026; Linux via Vulkan | all; Linux is WebKitGTK (the weak platform) until CEF | — | — |
| Binary, memory | small | ~2 s start, ~70 MB reported | small | large compile, GPU | ~10 MB plus the WebView's memory | | |
| Agent competence | good (large corpus) | moderate | moderate (DSL, good docs and live preview) | low: docs thin, API churn, survey author gave up without gpui-component | highest of any UI technology | moderate | low |
| Precedent in chess | this application | — | — | — | **En Croissant** (Tauri 2, React 19, Mantine), the reference the 1.x modernisation followed | — | — |
| Survey verdict | winner (immediate mode) | short of the circle (no accessibility) | winner (retained mode) | winner after the gpui-component addendum | "reasonable if a WebView is acceptable" | — | short of the circle |

Not tabled: Qt through cxx-qt (mature, but a C++ toolchain and Qt
packaging on four targets for a two-person project), Flutter through
rinf or flutter_rust_bridge (a second SDK and language, a UI toolkit as
good as the web's, but nothing else in the workspace gains from it),
Makepad, Freya, Floem, Vizia, KAS (each failed IME or accessibility or is
unmaintained in the survey).

## 5. Recommendation

**Build v2 as a Tauri 2 application: the harness library in Rust, the
interface in TypeScript with React.** Slint is the fallback if a second
language is unacceptable. Stay on egui only if the decision is to change
the design and nothing else.

Why Tauri over the Rust-native toolkits:

1. **Maturity is the complaint, and the web platform is the most mature UI
   technology there is.** Text, layout, tables, virtual scrolling, charts,
   animation, theming, DevTools and accessibility are solved, not
   re-implemented per toolkit. Nothing in §1's list of workarounds exists
   there.
2. **Design iteration is fast and verifiable.** The front end runs in a
   browser with hot reload; a design can be reviewed as a page before the
   Rust side exists; screenshots and DOM inspection are available to an
   agent through a browser tool. Slint has a live preview; GPUI has nothing
   comparable.
3. **Agents are best at it.** A React + TypeScript code base is the one an
   agent writes most reliably and most idiomatically, which is how this
   repository is built. GPUI is the opposite case in the survey's own words.
4. **The domain precedent is exact.** En Croissant, the chess GUI the 1.x
   modernisation took its cues from, is Tauri 2 + React + Mantine and ships
   on the same four platforms.
5. **It keeps option C open.** The same front end can later be served by
   `colosseum-cli` as a dashboard for a machine running a tune overnight.
6. **The harness already speaks JSON.** Every command has `--json`, `spsa
   status` and `spsa history` are read-only JSON views, and Phase 11.1's
   observer port publishes snapshots; Tauri events carrying those snapshots
   at 5–10 Hz are the whole IPC. `tauri-specta` generates typed TypeScript
   bindings from the Rust commands so the boundary cannot drift.

What it costs, honestly:

- **Two languages.** Rust for the harness, tournament logic, engine
  library, files and processes; TypeScript for everything the user sees.
  The boundary is thin (commands and events), and the CLI's JSON contract
  already defines most of it.
- **Linux rendering.** WebKitGTK is the weakest WebView (GPU quirks, older
  CSS). En Croissant lives with it; Tauri 3's bundled Chromium runtime
  (alpha, 2026) removes it at the cost of ~100 MB per bundle. Windows
  (WebView2, Chromium) and macOS (WKWebView) are fine.
- **Memory.** A WebView costs 100–200 MB. The engines a tournament runs
  cost more.
- **A full rewrite of 15,000 lines**, which is what the maintainer offered.
  Nothing from the egui code base carries over except the domain: the
  board and piece assets (SVG, reusable), ECO names, the theme's colour and
  spacing tokens (become CSS variables), and every rule in the guidelines
  that is about the product rather than about egui.

Why not the runners-up:

- **Slint** is the best pure-Rust choice: stable releases, a real DSL with
  a live previewer and a language server, IME and accessibility that work,
  native-looking widget styles per platform, small binaries. It loses on
  tables (std cells are text-only, so the standings and crosstable are
  custom), charts (hand-drawn paths), the component ecosystem (small), and
  agent fluency. Choose it if a TypeScript front end is a non-starter; the
  design in §3 fits it as well.
- **GPUI + gpui-component** produces the most polished-looking Rust
  application today and has exactly the components this design needs
  (virtual table, dock layout, sidebar, charts). It is pre-1.0 with its
  versioning tied to Zed's, thin documentation, one component vendor, and
  the survey author could not use it without the component library. Watch
  it; do not bet a rewrite on it in 2026.
- **iced** has no accessibility, limited tables, a slow cadence and
  reported startup and memory costs; it improves on egui only in retained
  layout.
- **Dioxus native and Xilem** are experimental or alpha.

## 6. What v2 looks like, concretely

- **Shell.** Tauri 2, React 19, TypeScript, Vite. Components from one
  system (Mantine, as En Croissant, or shadcn/ui on Tailwind); Inter and
  JetBrains Mono embedded; CSS variables for the theme tokens; light and
  dark following the system with an override.
- **Board.** lichess's `chessground` (GPL-3.0, the board every lichess
  page uses) with the cburnett pieces already in the repository; or a small
  in-house SVG board — 64 rectangles and piece images — if chessground's
  interaction model is more than a viewer needs.
- **Charts.** uPlot for the live evaluation graph (built for streaming
  data) and the SPSA trajectory; Recharts or ECharts if richer charts are
  wanted later.
- **Tables.** TanStack Table with virtualisation for standings, games and
  the engine library; column order and sort persisted as today.
- **IPC.** Tauri commands generated with `tauri-specta`; one event stream
  per run for live snapshots; the harness's JSON documents passed through
  unchanged where a CLI view already exists (`status`, `spsa history`).
- **Rust side.** `colosseum-harness` (Phase 11.1) plus a thin Tauri
  composition root: engine library, configuration and paths (already
  GUI-owned), tournament creation and resume through the harness driver,
  the observer port fanned out as events, file dialogs through Tauri's
  plugin, the updater through Tauri's plugin (replacing `update.rs`).
- **Packaging.** Tauri's bundler produces msi/nsis, dmg, deb, rpm and
  AppImage; `cargo xtask package gui` wraps it so the artifact names and
  the release workflow stay as they are; the Arch package keeps its
  PKGBUILD.
- **Retired.** egui, eframe, egui_extras, the SQLite game store (history
  index only), `engine::scheduler`, `widgets.rs`, `theme.rs`, and the
  guidelines' §7. A new `GUIDELINES.md` for v2 describes the design system,
  not the workarounds.

## 7. How it would fit the plan

Phase 11 keeps its purpose (one game-playing mechanism) and changes its
shape. A proposal, not a decision:

- **11.1 Harness library** — unchanged; it is the prerequisite for any
  GUI, and the CLI benefits alone.
- **11.2 v2 shell and Arena** — a new `crates/colosseum-app` (Tauri) beside
  the existing GUI: sidebar, engine library read-only, one tournament run
  through the harness with the live Arena. Exit: a real two-engine
  tournament watched live in the v2 window.
- **11.3 Tournaments and Engines to parity** — setup sheet, standings,
  crosstable, games browser, export, engine inspector with options and
  logos, resume, rating writeback; the 1.x SQLite history opened read-only
  and migrated to the history index. Exit: every 1.1.0 workflow, the
  stored-data rating parity within 0.01 Elo.
- **11.4 Retire the egui application and the scheduler** — remove
  `colosseum-gui`, `engine::scheduler` and the game-store path; the Tauri
  crate takes the `colosseum` binary name and package identity; xtask and
  the release workflow updated; ADR for the single mechanism and the
  technology change.
- **11.5 GUI 2.0.0** — release; the changelog records the new design, the
  adjudication default, the run directories and the migration.
- **Later, 12.x or a Phase 13:** the Runs section (CLI run directories in
  the desktop), then the served dashboard if it is wanted.

The 1.x application stays in maintenance on `main` until 11.4; a defect
found in use is still a `gui-v1.1.x` patch.

## 8. Open questions for the maintainer

1. A TypeScript front end, or pure Rust with Slint? (Everything else in
   this note follows from that answer.)
2. Component system: Mantine (En Croissant's choice, batteries included)
   or shadcn/ui + Tailwind (more control, more assembly)?
3. Should v2's first release already show CLI run directories (matches,
   gates, tunes), or is that a later phase?
4. Light theme by default, following the system, as ChessBase for Mac?
   The 1.x application is dark-first.
5. Is a served dashboard (option C) a goal at all, or only an option to
   keep open?

## Sources

- A 2026 Survey of Rust GUI Libraries — https://blog.wybxc.cc/blog/rust-gui-survey-2026/
- Discussion of the survey — https://lobste.rs/s/83yugk/2026_survey_rust_gui_libraries
- ChessBase for Mac — https://macland.chessbase.com/
- egui releases — https://github.com/emilk/egui/releases
- iced releases and the 0.14 discussion — https://github.com/iced-rs/iced/releases · https://news.ycombinator.com/item?id=46185323
- Slint releases and 1.18 — https://github.com/slint-ui/slint/releases · https://slint.dev/blog/slint-1.18-released
- gpui-component — https://github.com/longbridge/gpui-component · https://gpui-kit.com
- Zed Windows arm64 rendering issue (fixed) — https://github.com/zed-industries/zed/issues/49588
- Tauri releases and the CEF runtime — https://v2.tauri.app/release/ · https://github.com/tauri-apps/tauri/releases/tag/tauri-runtime-cef-v3.0.0-alpha.1
- Dioxus releases — https://github.com/DioxusLabs/dioxus/releases
- Xilem — https://github.com/linebender/xilem
- En Croissant — https://github.com/franciscoBSalgueiro/en-croissant
