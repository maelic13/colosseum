# Phase 9.7 final CLI release acceptance

Date: 2026-08-07

Colosseum CLI 0.1.0 candidate `823b398` is accepted for merge and stable
publication. This record does not create a tag or GitHub Release; those remain
maintainer-owned remote operations after this acceptance commit is pushed and
merged.

## Final candidate

| Field | Accepted value |
|---|---|
| Source commit | `823b398a273ae5631c24e32b4bbfec5b3b35749f` |
| Candidate workflow | `31213773139` |
| Downloaded aggregate SHA-256 | `84fb68c207bff63ccd1eb172061b6b079c1b782dc7c5092015a996b7787a908e` |
| Product / version | `colosseum-cli` / `0.1.0` |

The aggregate contained exactly `CANDIDATE.json`, `SHA256SUMS` and the four
archives below. Its candidate identity names the accepted source and workflow,
all four downloaded archives match `SHA256SUMS`, and the workflow staged and
smoked each exact archive independently.

| Platform archive | SHA-256 |
|---|---|
| `colosseum-cli-0.1.0-windows-x86_64.zip` | `af578287d966ab412d17438fa8fe5dff1c9fca6b3cb37f198cf1efd9480d15bc` |
| `colosseum-cli-0.1.0-windows-arm64.zip` | `452c81295ef7eaa7c035bdc1dd0b22d0ad9087d159cd9b1677c933737eae9656` |
| `colosseum-cli-0.1.0-linux-x86_64.tar.gz` | `ee83987d74b517c4214f90a54f326eab3e819a00040e6caed34594b85d1181fe` |
| `colosseum-cli-0.1.0-macos-aarch64.tar.gz` | `53d67a81868293eb66616de168c0d8728e9012fd4da8784230871e3a3eb086ae` |

## Documentation package acceptance

Every package has exactly the expected top-level binary, `README.md`,
`CHANGELOG-CLI.md`, `LICENSE` and `docs/`. Each contains the same 24-file
offline CLI guide. The concise CLI-only README and product changelog are
identical across all four platforms:

| Packaged document | SHA-256 |
|---|---|
| `README.md` | `2c0755e6ead806666bd6ac17b95cb9d53bb7d23fb76b01dd0edefa8b3a3b2e57` |
| `CHANGELOG-CLI.md` | `94643777c6878c5d25d05a40197bbe087a0e33bb614ece0577ac6f2a08960b08` |

Exact-archive smoke checks that every packaged local Markdown link resolves.
The downloaded Windows x86-64 archive also passed a fresh local
`--version`/`--help`/`self-test`/deterministic-JSON smoke, and the checker was
confirmed to reject an injected missing-file link.

## Executable continuity and validation gates

The Linux x86-64 executable SHA-256 remains byte-for-byte identical to the
previous engine-tested candidate:
`aee38483368fdf60f14eace766011716a18dd7d9333390186c9eef019472b543`.
Its Rarog and Basilisk gate evidence therefore carries forward unchanged.

The Windows x86-64 executable rebuilt to SHA-256
`03f3cd60df832866be20488ba85ab6584334868aaae07417628aa57b4bafe65d`.
Although the source delta contains no CLI source, manifest, lockfile or
dependency change, both short Windows gates were repeated with that exact
extracted executable. Rarog source commit
`8f35647630eb57839f0435e94b3c58858bd91711` was built from a clean archive;
Basilisk remained at commit
`3cbf90ba5f7b6c7aab2c25e8066ba1417b273447`.
Their Windows executable SHA-256 values were respectively
`cef8f2338d67f8482da8a3117f56f9dee85febaf1baff0071284a61e87c980fa`
and `4f04eb60896b84cae9aa218b948a449a0b585797ef15e47b1b1a54eb37c87122`.

Every gate uses the same engine on both arms, depth 1, four pairs, seed 123,
placement off and deliberately early draw adjudication. Exit code 4 is the
required capped-inconclusive outcome, not a failure.

| Gate | Result SHA-256 | Accepted projection |
|---|---|---|
| Windows / Rarog, repeated | `4d543838abac61a920f8c1dadc2171c866ec49f97d5d50c43f332179e150c672` | 8 draws, 4 pairs, `[0,0,4,0,0]`, adjudicated draws, zero faults, exit 4 |
| Windows / Basilisk, repeated | `d3169846f91eb6cf3a60ba18447a152a0f46f797beba92ab799d7f7354f92aab` | same |
| Linux / Rarog, retained | `6b5930014d7e0c618ceb7ed170e2beaab22808afe5ab6bb8ba7ceaa390dd10bf` | same |
| Linux / Basilisk, retained | `1562555c26fd279f9b9c7aed24d73b5535e7ba39b1e5ef99ed0c9d3c0dabd947` | same |

This agrees exactly with Phase 8.1 on game and pair counts, colour reversal,
W/D/L, termination, faults and pentanomial projection.

## Version and publication contract

- `cargo run --locked -p colosseum-release -- cli-v0.1.0` accepts the stable
  product tag, package version, product-owned changelog and artifact stem.
- `CHANGELOG-CLI.md` has an independently owned dated `0.1.0` section and the
  generated release notes link the release to its version-matched guide.
- Generated product release notes have SHA-256
  `fda8180657b7e01da97b437f23bcebc861d0a65758142ebd0e90739b364e2f87`.
- The tag workflow will rebuild, exact-archive smoke and publish the four CLI
  archives plus checksums only after the accepted source is on `main`.
- This acceptance-only documentation is outside the CLI package allowlist and
  changes neither executable nor packaged user files, so it does not invalidate
  the candidate baseline.

Candidate `f0e3185` remains superseded because its archive omitted the CLI
changelog and used a combined README with broken packaged links. Candidate
`823b398` closes that defect and is the sole accepted 0.1.0 baseline.

Phase 9.7 and the implementation plan are complete. The remaining sequence is
operational: push this acceptance commit, merge `cli` to `main`, create
`cli-v0.1.0` at the merged stable source, push the tag, and confirm the stable
release workflow succeeds.
