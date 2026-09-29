# Colosseum 1.x desktop application (archived)

The egui desktop application, released as Colosseum GUI 1.0.0 to 1.1.0, was
built and released from this repository. Its successor is developed in
[maelic13/colosseum-gui](https://github.com/maelic13/colosseum-gui) as a client
of the CLI (ADR-0011), and the 1.x code left this repository's `dev` branch at
step 11.2.1. `main` keeps it, and its `gui-v` release lane, until the merge that
ends the programme (ADR-0012); the source stays in history at `gui-v1.1.0`
(commit `ba308297f8866dc7aa8fd3172730d941e05b3d68`).

| File | What it was |
|---|---|
| [`CHANGELOG-GUI.md`](CHANGELOG-GUI.md) | The user-facing changelog, 0.1.0 to 1.1.0 |
| [`design/`](design/GUIDELINES.md) | The binding visual guidelines and design sheets; the new application's inventory splits them into product rules and egui workarounds |
| [`macos-signing.md`](macos-signing.md) | Notes on signing and notarising the macOS `.dmg` |
| [`screenshot.png`](screenshot.png) | The Arena tab with the live game view |

These are records. Nothing here is maintained or built.
