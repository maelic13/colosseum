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
| `--cores LIST` | Allocated logical CPUs, such as `0,2-4,7`; Windows processor groups use `GROUP:CPU`, such as `0:0-3,1:0-3` |

UCI option names are preserved exactly and validated against the schema the
engine advertises during its normal handshake. An unqualified CPU belongs to
group zero. Duplicate names and duplicate, malformed or descending CPU
allocations are errors. No engine manifest, compiler metadata,
build command or custom engine feature is read or required.

## Inspection and compliance

`colosseum-cli engine inspect <EXECUTABLE>` performs the normal `uci`/`isready`
handshake and prints the engine-reported name, author and complete advertised
option schema.

`colosseum-cli engine check <EXECUTABLE>` reports PASS, FAIL or SKIP separately
for handshake, readiness, requested-value/schema validation, option acceptance,
a bounded legal search, `stop`, `ucinewgame` and shutdown. “Option acceptance”
means only that `setoption` caused no protocol/process failure and a subsequent
`isready` completed. UCI defines no option read-back, so Colosseum does not call
this a round trip or claim that the engine used the value internally. The
command exits nonzero if any requirement does not pass.
