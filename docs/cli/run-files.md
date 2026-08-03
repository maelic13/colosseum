# Run-file reference

A run file is optional TOML that expands into the same public arguments accepted
by `colosseum-cli`. It is useful for committed project policy; no engine needs a
manifest or Colosseum-specific file.

This complete file runs a fixed match:

```toml
command = ["match"]
positionals = ["./bin/candidate", "./bin/baseline"]

[options]
games = 100
a-movetime-ms = 100
b-movetime-ms = 100
concurrency = 4
placement = "auto"
seed = 42
```

Invoke it and optionally replace an option on the command line:

```text
colosseum-cli --run-file ./testing/gate.toml
colosseum-cli --run-file ./testing/gate.toml --games 200
```

The schema has three user keys:

| Key | Type | Meaning |
|---|---|---|
| `command` | non-empty string array | Public command path, for example `["spsa", "plan"]` or `["tournament", "run"]` |
| `positionals` | string array | Positional values in command-reference order |
| `options` | table | Long option names without leading `--`; a scalar emits once, an array repeats the option, `true` emits a flag and `false` omits it |

Option names and values are validated by the real Clap parser after expansion,
so the [generated command reference](command-reference.md) is the authoritative
schema. A file may contain only `[options]`; supply the command and positionals
normally to share conditions across workflows:

```toml
# testing/fast.toml
[options]
concurrency = 4
placement = "auto"
book = "books/openings.epd"
book-order = "random"
seed = 42
```

```text
colosseum-cli --run-file ./testing/fast.toml match ./candidate ./baseline --games 100
```

## Inheritance and precedence

A file may name one parent with `extend`. Resolution is:

```text
built-in defaults < inherited run files < command-line options
```

```toml
extend = "common.toml"
unset = ["/options/book"]

[options]
games = 1000
```

`extend` is relative to the declaring file. Parent files resolve first,
canonical identities detect cycles, and chains are limited to 16 files. Tables
merge recursively; child scalars replace parent scalars and arrays replace
whole arrays. `unset` contains strict RFC 6901 JSON pointers applied before the
child overlay. `extend` and `unset` never reach Clap.

An explicit CLI long option replaces the same run-file option, including an
array-valued repeated option. Use `--unset-run-option book` to remove an
inherited option without supplying a replacement. Invalid or absent unset
targets are errors rather than silent typos.

When both the file and command line contain a command, the explicit command and
its positionals are used; the file contributes only options. This makes a
shared `[options]` file safe and lets engine paths remain ordinary CLI inputs.

## Paths

Engine, book, tune, run-directory, baseline and other documented filesystem
paths declared in a run file are resolved relative to the file that declared
that exact value, including inherited parents. Explicit CLI paths remain
relative to the invocation directory. The resulting absolute values pass
through the ordinary parser and produce the same canonical run identity as an
equivalent all-CLI invocation.

Use `--dry-run --json` to inspect the fully normalized experiment and exact
process invocations before any engine starts:

```text
colosseum-cli --run-file ./testing/gate.toml --dry-run --json
```
