# Phase 9.7 final CLI release acceptance

Date: 2026-08-07

Colosseum CLI 0.1.0 is accepted for merge and stable publication.  This record
does not create a tag or GitHub Release; those remain maintainer-owned remote
operations after the acceptance commit is pushed and merged.

## Final candidate

| Field | Accepted value |
|---|---|
| Source commit | `f0e318555482cc9769eb0682d0ebf3141ce54916` |
| Candidate workflow | `31209582676` |
| Aggregate SHA-256 | `4ba416cd91c41bc7742bcc83fab2bc7ba4c51ef3a37457d6d85dd85069267582` |
| Product / version | `colosseum-cli` / `0.1.0` |

The aggregate contained exactly `CANDIDATE.json`, `SHA256SUMS` and the four
archives below.  Every downloaded archive matched `SHA256SUMS`; the workflow
had already staged and smoked each exact archive independently.

| Platform archive | SHA-256 |
|---|---|
| `colosseum-cli-0.1.0-windows-x86_64.zip` | `b25f9eaed42da8f65eee8273c5177cd33e385ca4b5d5d60c5c386c27230aa45c` |
| `colosseum-cli-0.1.0-windows-arm64.zip` | `c1eb7756df223268fb8865f6fb98d3b4bbe4389744c334103b29ccb2132adab9` |
| `colosseum-cli-0.1.0-linux-x86_64.tar.gz` | `931fe2d056ad17a476c6e1ac807d9da4401797d9d3e41469bfa2a0840d125bf7` |
| `colosseum-cli-0.1.0-macos-aarch64.tar.gz` | `21cd1a91d9619b9ac7066fce66812e808ad3ec4781c5f1f1ce4e8d47dfb314f0` |

The packaged README names CLI 0.1.0 as released, links the product-scoped tag
and gives archive-first installation.  The tested executable hashes were
`a0b26025bd4ee2cb96c8f6bfe9d5385bf0048dd984b2dd18a7c85ae5dd6f6d3d`
for Windows x86-64 and
`aee38483368fdf60f14eace766011716a18dd7d9333390186c9eef019472b543`
for Linux x86-64.

## Two-operating-system validation gates

Rarog commit `8f35647630eb57839f0435e94b3c58858bd91711` and Basilisk commit
`3cbf90ba5f7b6c7aab2c25e8066ba1417b273447` were each built for and run on
Windows and Ubuntu WSL Linux.  The exact extracted candidate executable for
that operating system drove every gate.  Linux builds came from clean
`git archive` exports in temporary native-Linux directories; no engine or
Colosseum manifest was introduced.

Each gate repeated the Phase 8.1 oracle conditions: same engine on both arms,
depth 1, four pairs, seed 123, placement off and deliberately early draw
adjudication.  An exit code of 4 is the required capped-inconclusive outcome,
not a failure.

| Gate | Result SHA-256 | Accepted projection |
|---|---|---|
| Windows / Rarog | `0e79418ab909b14448e7247d47bc430fbdfbc2b9958cec069ce63cf09e13d9c2` | 8 draws, 4 pairs, `[0,0,4,0,0]`, adjudicated draws, zero faults, exit 4 |
| Windows / Basilisk | `1ff65ba2300bab810f5493d7bb3cdccedc3dafaac611a802c36dfda969d5cb04` | same |
| Linux / Rarog | `6b5930014d7e0c618ceb7ed170e2beaab22808afe5ab6bb8ba7ceaa390dd10bf` | same |
| Linux / Basilisk | `1562555c26fd279f9b9c7aed24d73b5535e7ba39b1e5ef99ed0c9d3c0dabd947` | same |

This agrees exactly with Phase 8.1 on all shared fields: game and pair counts,
colour reversal, W/D/L, termination, faults and pentanomial projection.

## Version and publication contract

- `cargo run --locked -p colosseum-release -- cli-v0.1.0` accepts the stable
  product tag, package version, product-owned changelog and artifact stem.
- `CHANGELOG-CLI.md` has an independently owned dated `0.1.0` section.
- Generated product release notes have SHA-256
  `d961c4768313381b3c094390fedbab9124a21d3a24052a4a726bbbd05ef8c645`.
- The tag workflow will rebuild, exact-archive smoke and publish the four CLI
  archives plus checksums only after the accepted source is on `main`.
- The acceptance documentation added after the candidate is outside the CLI
  package allowlist and changes neither executable nor packaged user files, so
  it does not invalidate the candidate baseline.

Phase 9.7 and the implementation plan are complete.  The remaining sequence
is operational: push this acceptance commit, merge `cli` to `main`, create
`cli-v0.1.0` at the merged stable source, push the tag, and confirm the stable
release workflow succeeds.
