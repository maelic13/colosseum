# Colosseum CLI

Colosseum CLI is an independent, headless tool for testing ordinary UCI chess
engines. It provides reproducible fixed matches, pair-based SPRT, SPSA tuning,
calibration, NPS and scaling measurements, position suites, tournaments,
opening-book utilities and statistics analysis.

Engines need only implement UCI. They do not need a Colosseum manifest,
custom build command or source-tree integration.

## Start here

Download the archive for your platform from the
[Colosseum releases page](https://github.com/maelic13/colosseum/releases),
extract it, and check the installation:

```text
colosseum-cli --version
colosseum-cli self-test
colosseum-cli engine check ./my-engine
```

On Windows, run `colosseum-cli.exe`. On Linux and macOS, run
`./colosseum-cli` if the current directory is not on your `PATH`.

Run a first book-free fixed match with:

```text
colosseum-cli match ./candidate ./baseline --games 100 --a-movetime-ms 100 --b-movetime-ms 100
```

Colosseum writes self-contained evidence under `./colosseum-runs/` by default.
Use `--dir <path>` on durable workflows when you want an explicit resumable
run directory.

## Documentation

- [Complete CLI guide](docs/cli/README.md)
- [Quickstart](docs/cli/quickstart.md)
- [Generated command reference](docs/cli/command-reference.md)
- [How to trust and report a result](docs/cli/trust-results.md)
- [Engine compatibility and failure behavior](docs/cli/compatibility.md)
- [CLI changelog](CHANGELOG-CLI.md)

Every release archive includes these exact version-matched documents for
offline use. The executable also provides command-specific help, for example
`colosseum-cli sprt --help`.

Report problems through the
[GitHub issue tracker](https://github.com/maelic13/colosseum/issues).
Colosseum is licensed under the [GNU GPL v3 or later](LICENSE).
