# Executable self-test

Run `colosseum-cli self-test` after installing or unpacking the CLI. The command
uses a hidden deterministic UCI stub compiled into that exact executable; it
does not rely on Stockfish, a network connection, the GUI, or a developer
checkout.

The checks cover:

- UCI handshake, readiness, options, bounded search, stop, new game and quit;
- concurrent stdout/stderr draining, finite diagnostic tails and rejection of
  protocol lines larger than 64 KiB;
- bounded shutdown escalation and process-tree reaping (Windows Job Object or
  Unix process group), including a descendant and an engine that ignores quit;
- propagation of a required persistence write failure;
- a deterministic four-ply exchange between two independently launched stubs.

`self-test --json` follows the common single-document stdout contract. A failed
check gives the command a nonzero exit status.
