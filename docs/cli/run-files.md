# Run files and overrides

A run file is optional TOML for keeping repeatable experiment settings beside a
project. CLI arguments can express the same ordinary workflows. Resolution is:

```text
built-in defaults < inherited run files < command-line overrides
```

A file may name one parent with `extend`. The path is relative to the declaring
file, inheritance is limited to 16 files, and canonical file identities prevent
cycles:

```toml
extend = "../shared.toml"
unset = ["/engine/options/EvalFile"]

[engine]
path = "bin/rarog"
arguments = ["--uci"]

[engine.options]
Hash = 256
```

Tables merge recursively. Scalars replace inherited scalars, and arrays replace
whole arrays rather than merging by index. `unset` contains RFC 6901 JSON
pointers and is applied to the resolved parent before the child is overlaid.
Invalid or missing targets are errors that name the pointer and declaring file.

Paths retain the origin that declared them: a run-file path is relative to that
file, while a CLI path is relative to the invocation directory. The final
command schema converts path fields and units to canonical values. Every run
writes exact canonical `resolved-config.json` bytes, their `config.sha256`, and
the audit-only `config-origins.json` map.
