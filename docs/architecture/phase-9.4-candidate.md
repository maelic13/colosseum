# Phase 9.4 unpublished CLI candidate evidence

Date: 2026-08-07

Phase 9.4 is accepted as the exact artifact baseline for Phases 9.5–9.7. This
candidate is a retained workflow artifact, not a tag, prerelease or stable
release.

## Identity

| Field | Value |
|---|---|
| Product | `colosseum-cli` |
| Version | `0.1.0` |
| Source commit | `22aefa8a4374405f7cedbcf2d1baf09066f9ebe7` |
| Candidate workflow run | `31199592962` (run number 6) |
| Required CI run | Run number 21, green on the same commit |
| Retained artifact | `colosseum-cli-candidate-22aefa8a4374405f7cedbcf2d1baf09066f9ebe7` |

The retained `CANDIDATE.json` contains the product, version, complete source
commit and workflow-run identity above.

## Exact archives

| Platform | Archive | SHA-256 |
|---|---|---|
| Windows x86-64 | `colosseum-cli-0.1.0-windows-x86_64.zip` | `822523da34c54fee8310d25d814ea8ed1afe1b477f84b14fff2e53a1ccccd512` |
| Windows ARM64 | `colosseum-cli-0.1.0-windows-arm64.zip` | `6a0302ea81e897a77fd4be57b71cc776292ebfd256e784a503d6f733a1b7371c` |
| Linux x86-64 | `colosseum-cli-0.1.0-linux-x86_64.tar.gz` | `d07a1fe1a52e58519f81e6ddaa1bf7442173e483711d07957e31fabb73350957` |
| macOS ARM64 | `colosseum-cli-0.1.0-macos-aarch64.tar.gz` | `4035c4e304657097cc633c92c4984f06d4208910a0846400dede6ad94e5dabbc` |

The downloaded aggregate contained exactly these four archives plus
`SHA256SUMS` and `CANDIDATE.json`. All four downloaded archive hashes matched
`SHA256SUMS` locally.

## Acceptance

- Required debug and release CI passed on Windows, Linux and macOS.
- Each platform job built only the headless CLI, staged the allowlisted files
  and smoked its exact final archive with `--version`, `--help`, `self-test`
  and a deterministic JSON workflow.
- The preparation job checked the CLI dependency boundary and generated
  documentation.
- The aggregation job verified the complete platform matrix and retained the
  unpublished candidate bundle without creating a GitHub Release.
- A second local smoke of the downloaded Windows x86-64 archive passed from an
  isolated directory.

Any later change that affects the CLI or its package invalidates this baseline
and requires a new candidate before final acceptance.
