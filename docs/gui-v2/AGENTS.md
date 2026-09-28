# Colosseum desktop application — agent instructions

Read this file, [`PLAN.md`](PLAN.md) and [`GUIDE.md`](GUIDE.md) before any
work. `PLAN.md` is the binding specification of the desktop application and
its relationship to the Colosseum CLI; `GUIDE.md` is the ordered tracker.

## What this repository is

The desktop application is a **client of the Colosseum CLI**. It starts the
CLI as a child process and speaks its front-end protocol; the CLI owns
everything that plays or scores games, this repository owns presentation and
the user's own data (PLAN §G3). The CLI repository's `PLAN.md` §S5.15 is the
protocol's principles and its `docs/cli/protocol.md` the contract. The two
repositories advance together on the joint milestones in PLAN §G9.

Both products are called Colosseum: the desktop application **Colosseum**,
the command-line tool **Colosseum CLI** (`colosseum-cli`). A step number
prefixed `CLI` (`CLI 11.4`) is the CLI repository's; a bare number is a step
here.

| Repository | Until the swap | After the swap (right after 5.1) |
|---|---|---|
| This one | `maelic13/colosseum-gui` | `maelic13/colosseum` |
| The CLI's | `maelic13/colosseum` | `maelic13/colosseum-cli` |

The swap and its same-day checklist are in PLAN §G8
([CLI ADR-0012](https://github.com/maelic13/colosseum/blob/main/docs/architecture/adr/0012-names-and-repositories-after-the-split.md)).

## Scope and architecture

- Implement numbered `GUIDE.md` steps in order. Do not skip a phase exit.
- **Design first.** No screen is implemented before its specification (Phase
  3) is signed off, and no specification is written before the design system
  (1.4) and the protocol (CLI 11.4) exist. A step tempted to build a
  screen early stops instead.
- **Never re-implement the CLI.** No rating, statistic, pairing, clock,
  adjudication, PGN writing or move generation lives here. Positions arrive
  as FEN; results arrive as facts; anything missing is a CLI extension
  requested through the CLI repository's gap analysis, not code here.
- **One component catalogue.** Every screen is composed from the design
  system's components. A new need extends the catalogue and its gallery in
  both themes first.
- **Performance is a requirement with numbers** (PLAN §G5). The UI thread
  only renders; long work runs in the CLI process or off the UI thread;
  every long list is virtualised; live updates coalesce to one render per
  frame; the window is usable before data arrives.
- **Dependencies where they pay.** Every dependency has a line in
  `docs/dependencies.md` saying why; a dependency that overlaps one already
  taken, or adds weight for little, is refused.
- The 1.x application source is read for the inventory only, in a checkout
  of the CLI repository beside this one at `gui-v1.1.0`, commit
  `ba308297f8866dc7aa8fd3172730d941e05b3d68` (the tag is deleted at CLI
  14.1; the commit stays); nothing is copied from it except assets and
  product rules the inventory keeps.
- Treat engine crashes and protocol quirks as real, supported conditions:
  every error the CLI reports has a place on a screen.

## Step and commit discipline

One numbered `GUIDE.md` item is the normal unit of work.

Agents work locally only: create the required commits, but never push,
fetch, create tags, releases or pull requests, or perform other remote
operations. The maintainer owns every remote operation.

`PLAN.md` §G9 records each open step's capability class (`R3`, `R2`, `I2`,
`I1`, `M`, `V`), mirrored on the numbered `GUIDE.md` item; `GUIDE.md`'s
"Current model mapping" table is the single maintainer-edited source that
maps a class to the current Claude model and thinking mode. Whenever
reporting the next step, name it with its class and recommend the mapped
model and mode as `Claude: <model> — <mode>` with a brief task-specific
reason; do not substitute newer models, and never change the active model
automatically. If an `I1`, `M` or `V` step exposes a material design choice
not settled by the plan, stop and continue it as `R2` rather than inventing
the contract; never silently downgrade a recorded class. Keep classes
synchronized between PLAN and GUIDE.

1. Start from a clean worktree, or identify and preserve pre-existing user
   changes. Never stage unrelated files.
2. Implement the step, its tests, documentation and generated files.
3. Run the verification baseline below and whatever the step names.
4. Demonstrate the exit criterion. A compiling change is not complete.
5. Mark the finished step `☑` in `GUIDE.md` in the same change, with the
   evidence the exit asks for. Use `☐` for todo and `◐` only while genuinely
   in progress; do not use GitHub `[ ]` task syntax.
6. Commit the completed step before starting another numbered step.

Use a short imperative commit subject naming the outcome, preferably with
the step identifier, for example `GUI 2.2: component gallery in both themes`.

Do not:

- combine independently completable steps in one commit;
- mark or commit an incomplete step as done;
- amend, rewrite, reset or discard user commits or changes unless asked;
- add `Co-authored-by`, assistant attribution or other authorship trailers;
- begin the next numbered step until the current step's commit succeeds.

If a step is blocked, leave it unchecked, document the blocker, and do not
create a misleading completion commit.

## Verification baseline

Fixed at step 2.1 when the stack is in place and recorded here then. Until
then: `git diff --check` and a consistency read of the documents a step
touched. From 2.1 on, whatever CI enforces, in this order: formatter check,
linter, type check, unit tests, component screenshot tests in both themes,
end-to-end tests against the CLI's synthetic streams, and the protocol
contract test against the pinned CLI's published fixtures. A formatting or
lint failure is a failure.

## Documentation ownership

- `PLAN.md` and `GUIDE.md`: maintainer-facing specification and tracker.
  `PLAN.md` carries the binding specification and only the open work;
  completed phases become one line in `GUIDE.md` and a record under
  `docs/architecture/`.
- `docs/design/`: the design documents (inventory, requirements, concepts,
  wireframes, design system, screens). Signed-off documents change only by a
  dated amendment.
- `docs/architecture/adr/`: decisions, numbered from 0001 in this repository.
- `docs/dependencies.md`: every dependency with one line of reason.
- `README.md` and `CHANGELOG.md`: user-facing; no phase numbers or internal
  argumentation.
