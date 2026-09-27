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

`<GUI>` in these files stands for the desktop application's name, decided
at step 11.1 of this repository's plan; `<gui-repo>` for its repository.

## Hand-over (step 11.2)

1. Replace `<GUI>` and `<gui-repo>` with the 11.1 names in the three files.
2. The maintainer creates the empty remote repository and commits the three
   files at its root as the first commit (`Seed the <GUI> repository from
   Colosseum`), followed by the licence (GPL-3.0-or-later, as here).
3. Reduce this folder to a pointer (`README.md` only, naming the repository)
   and mention the new application in the root `README.md`.
4. From then on the GUI repository's `GUIDE.md` tracks GUI Phases 1–5; this
   repository's `GUIDE.md` tracks Phases 11, 13 and 14 and the joint
   milestones (PLAN §S8).

The GUI repository reads this repository twice: the 1.x application source
at the `gui-v1.1.0` tag for the inventory (GUI 1.1), and the released CLI
binary, schema and fixtures for everything else. It never links a crate from
here.
