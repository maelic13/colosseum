# Phase 8.2 — remaining runner-gap decisions

The decision rule is whether a general engine developer needs a capability,
tempered by whether Colosseum can implement and validate it correctly for 1.0.
Feature count or parity with one external runner is not itself a reason to add
surface area. The machine-readable record is
[`docs/fixtures/phase8/gaps.json`](../fixtures/phase8/gaps.json).

## Ponder

**Adopt for 1.0.** Pondering is part of ordinary UCI engine behavior and is a
real test condition for time management and protocol correctness. The shared
runner already implements predicted replies, `go ponder`, `ponderhit`, missed
prediction `stop`, live output draining and opponent-time clock accounting.

The CLI now exposes `--ponder` on fixed matches, SPRT, calibration, SPSA and
live tournaments. It is explicit and defaults off. It is allowed only when
every arm uses a base/increment game clock: fixed movetime, node and depth
limits do not provide a meaningful opponent-clock budget and would make a
ponder hit change the nominal fixed work. Colosseum controls the `Ponder` UCI
option from this flag and records the choice in resolved configuration, run
record and final report. A deterministic UCI fixture exercises an actual
ponder hit without an external engine.

## Chess960

**Defer until after 1.0.** This is useful to developers of Chess960-capable
engines, so it is not declined. Correct support crosses several current
standard-chess boundaries: variant-aware FEN/opening validation, castling move
encoding, `UCI_Chess960` negotiation, PGN `Variant`/setup tags, suite behavior
and external-runner parity fixtures. Merely forwarding `UCI_Chess960=true`
would accept inputs the harness can misinterpret and is not a valid feature.

The future feature must enter through the domain position/variant model and be
covered by castling positions for king-between-rooks, king/rook already on
destination squares, both colours and both castling sides.

## Harness-side Syzygy adjudication

**Defer until after 1.0.** Engine-side Syzygy, Gaviota and other tablebase
options remain ordinary forwarded UCI options. Harness probing is separate
mechanism and needs a tablebase port plus explicit WDL versus DTZ semantics,
50-move-rule policy, castling/en-passant handling, available-piece-set audit,
probe-failure classification and cross-platform fixtures. Adding a probing
library just to claim the checkbox would expand the trusted adjudication base
without improving the 1.0 statistical core.

## Additional tournament formats

**Decline for 1.0.** Round-robin and multi-seed gauntlet are the static formats
needed for comparative engine development. Swiss, knockout and ladder formats
are useful event-management features, but no generic regression, tuning or
rating workflow currently requires their result-dependent scheduling. A future
format needs a concrete developer workflow and deterministic crash/resume
contract before changing the shared schedule model.

## Additional output formats

**Decline for 1.0.** Versioned JSON is the machine contract, PGN carries games,
and RFC-4180 CSV carries standings and crosstables. Together they cover the
identified consumers without introducing another schema. A new format should
name the consumer that cannot use those artifacts and specify its compatibility
contract.

## Dedicated datagen command

**Decline for 1.0.** A finite fixed-node or fixed-depth self-play corpus is a
normal `match`: it already has deterministic seeds and opening order,
colour-paired starts, concurrency/placement, durable recovery, game identities
and PGN. Trainer-specific binary records, position filtering and labels remain
engine-project policy.

The 1.0 documentation will give the match recipe. Revisit a separate command
only when concrete engine-independent requirements exceed it, such as corpus
sharding, cross-shard deterministic IDs, deduplication, controlled
randomisation or effectively unbounded horizons.
