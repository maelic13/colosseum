# Engine controls

Colosseum CLI consumes ordinary UCI executables. An executable path is the only
required engine input. Commands that launch an engine share these optional
controls:

| Control | Meaning |
|---|---|
| `--label NAME` | Display label only; never replaces UCI identity |
| `--engine-arg VALUE` | Process argument; repeat it and use `--engine-arg=--flag` for values beginning with `-` |
| `--cwd PATH` | Engine process working directory |
| `--env KEY=VALUE` | Environment override; repeat for multiple variables |
| `--option NAME=VALUE` | Arbitrary UCI option value; repeat for multiple options |
| `--button NAME` | Trigger a UCI button option |
| `--cores LIST` | Allocated logical CPUs, such as `0,2-4,7` |

UCI option names are preserved exactly and validated against the schema the
engine advertises during its normal handshake. Duplicate names and duplicate or
descending CPU allocations are errors. No engine manifest, compiler metadata,
build command or custom engine feature is read or required.
