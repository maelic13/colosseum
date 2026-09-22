# Architecture and evidence

Maintainer-facing. The binding specifications are in [`PLAN.md`](../../PLAN.md);
the tracker is [`GUIDE.md`](../../GUIDE.md). This directory holds what those
two point at: the architecture, the decisions and the evidence that each
phase met its exit criterion.

## Architecture

| Document | What |
|---|---|
| [`current-state.md`](current-state.md) | The workspace as found before the CLI work: responsibilities, seams, dependency direction |
| [`dependency-inventory.md`](dependency-inventory.md) | Crate and module dependency graph from `cargo metadata` and `cargo tree` |
| [`target-architecture.md`](target-architecture.md) | Clean Architecture target, layer rules and the migration map |
| [`release-architecture.md`](release-architecture.md) | Independent product versions, tags, artifacts and workflows in one repository |
| [`naming-decision.md`](naming-decision.md) | Naming research behind ADR-0007 to ADR-0009 |
| [`fastchess-mechanics.md`](fastchess-mechanics.md) | Source study of fastchess's process, clock and pipe mechanics, with adopt/decline verdicts |
| [`adr/`](adr/README.md) | Architecture decision records |

## Records and acceptance evidence

| Document | What |
|---|---|
| [`phases-0-9-record.md`](phases-0-9-record.md) | The PLAN text of Phases 0–9 as implemented: progress, accepted results, decisions |
| [`phase-10-record.md`](phase-10-record.md) | Every Phase 10 item with rationale and implementation evidence, including the maintainer decisions |
| [`phase-10-qualification.md`](phase-10-qualification.md) | Real-engine qualification: scale, verdict, ratings, tuning recovery and symmetry figures |
| [`phase-0-review.md`](phase-0-review.md) | Phase 0 review of the current/target architecture and release design |
| `phase-2-exit.md` … `phase-8-exit.md` | Exit evidence per phase |
| [`phase-8-parity.md`](phase-8-parity.md), [`phase-8-gap-decisions.md`](phase-8-gap-decisions.md) | Parity against external runners; feature-gap decisions |
| [`phase-9.4-candidate.md`](phase-9.4-candidate.md), [`phase-9.5-coverage.md`](phase-9.5-coverage.md), [`phase-9.6-usability.md`](phase-9.6-usability.md), [`phase-9.7-release-acceptance.md`](phase-9.7-release-acceptance.md) | Candidate identity, coverage acceptance, usability exercise, release acceptance |

The recorded parity commands and per-phase acceptance manifests are in
[`../fixtures/`](../fixtures/); the vendored statistics fixtures and their
generator are in `tests/fixtures/`.
