# GUI v2 repository seed

This folder is the first commit of the desktop application's own repository,
prepared here because the decision to separate the two products
([ADR-0011](../architecture/adr/0011-desktop-application-as-separate-cli-client.md))
was taken here. It is not part of the CLI product and nothing in this
repository builds from it.

| File | Becomes |
|---|---|
| [`AGENTS.md`](AGENTS.md) | the GUI repository's agent rules |
| [`PLAN.md`](PLAN.md) | the GUI repository's binding plan (its §G9 holds the steps) |
| [`GUIDE.md`](GUIDE.md) | the GUI repository's ordered tracker |

The names are the ones step 11.1 decided
([ADR-0012](../architecture/adr/0012-names-and-repositories-after-the-split.md)):
the desktop application is **Colosseum**, its repository
`maelic13/colosseum-gui` until the swap after its first release, when it
takes `maelic13/colosseum` and this repository becomes
`maelic13/colosseum-cli`.

## Hand-over (step 11.2)

1. The three files are final under those names.
2. The maintainer creates the empty remote repository
   `maelic13/colosseum-gui` and commits the three files at its root, with
   this repository's `LICENSE` (GPL-3.0-or-later), as the first commit
   (`Seed the Colosseum desktop repository`).
3. Reduce this folder to a pointer (`README.md` only, naming the repository)
   and mention the new application in the root `README.md`.
4. From then on the GUI repository's `GUIDE.md` tracks GUI Phases 1–5; this
   repository's `GUIDE.md` tracks Phases 11, 13 and 14 and the joint
   milestones (PLAN §S8).

The GUI repository reads this repository twice: the 1.x application source
at `gui-v1.1.0` (commit `ba30829`; the tag is deleted at 14.1) for the
inventory (GUI 1.1), and the released CLI binary, schema and fixtures for
everything else. It never links a crate from here.
