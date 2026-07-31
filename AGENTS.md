# Colosseum agent instructions

Read this file, [`CLAUDE.md`](CLAUDE.md), [`PLAN.md`](PLAN.md), and
[`GUIDE.md`](GUIDE.md) before implementation work. `CLAUDE.md` describes the
existing application and conventions; `PLAN.md` is the binding CLI
specification; `GUIDE.md` is the ordered implementation tracker.

## Scope and architecture

- Implement numbered `GUIDE.md` steps in order. Do not skip a phase exit.
- Phase 0 is mandatory before CLI feature code: document the current
  architecture, design the Clean Architecture target, record ADRs, and map the
  migration.
- Dependencies point inward: domain → nothing outward; application use cases →
  domain and ports; adapters/drivers implement ports. The CLI and GUI are
  separate composition roots and must not depend on each other.
- Preserve working UCI, runner, GUI, persistence, and compatibility behaviour.
  Prefer the smallest boundary refactor that satisfies the target architecture.
- The CLI accepts ordinary UCI executables. Do not add engine manifests, custom
  build/bench requirements, compiler inspection, or engine-specific logic.
- Treat existing engine crashes and protocol quirks as real supported input
  conditions. Never hide them by weakening diagnostics or fault classification.
- Follow `docs/design/GUIDELINES.md` for every GUI change.

## Step and commit discipline

One numbered `GUIDE.md` item is the normal unit of work.

1. Start from a clean worktree, or identify and preserve pre-existing user
   changes. Never stage unrelated files.
2. Implement the step, its tests, documentation, migrations, and generated
   files needed for that step.
3. Run the verification required by the step and the proportionate workspace
   checks from `CLAUDE.md`.
4. Demonstrate the exit criterion. A compiling change is not automatically
   complete.
5. Mark the step `[x]` in `GUIDE.md` with the required outcome tag and record
   corresponding status/evidence in `PLAN.md` in the same change.
6. Commit the completed step before starting another numbered step.

Use a short imperative commit subject that names the outcome, preferably with
the step identifier, for example:

```text
Phase 1.2: implement pentanomial variance
```

Do not:

- combine independently completable steps in one commit;
- mark or commit an incomplete step as done;
- amend, rewrite, reset, or discard user commits/changes unless explicitly
  requested;
- add `Co-authored-by`, assistant attribution, or other authorship trailers;
- begin the next numbered step until the current step's commit succeeds.

If a step is blocked, leave it unchecked, document the blocker, and do not
create a misleading completion commit. Small corrective commits discovered
during verification are allowed, but each must have a precise single-purpose
subject.

## Verification baseline

Unless a step specifies more, use:

```text
cargo check --workspace --tests
cargo clippy --workspace
cargo test --workspace --all-targets
```

Run debug and release/platform-specific checks where PLAN or GUIDE requires
them. Documentation-only changes require at least `git diff --check` and
consistency validation; they do not require rerunning unchanged Rust tests.

## Documentation ownership

- `PLAN.md` and `GUIDE.md`: maintainer-facing CLI specification and tracker.
- `README.md` and CLI user documentation: user-facing; no phase numbers or
  internal method argumentation.
- `docs/DEVELOPMENT.md`: implemented build/test/release facts; update it when
  the workspace or release process actually changes, not merely when planned.
- `CHANGELOG.md`: released user-visible changes only.

