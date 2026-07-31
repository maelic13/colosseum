# Reproducible randomness

Commands that need randomness use one unsigned 64-bit master seed. Supply it as
`--seed`; when absent, Colosseum obtains eight bytes from the operating system,
interprets them as little-endian `u64`, exposes the value to command output, and
records it in the resolved configuration before that configuration is hashed.

Each consumer owns a named independent stream. RNG/stats version 1 derives its
32-byte key as:

```text
SHA-256("colosseum-rng-v1\0" || master-seed-u64-LE || stream-name-UTF-8)
```

Stable stream names are `opening-order`, `spsa-perturbations`,
`bootstrap-resampling`, `position-order`, and `warmup-scheduling`. Adding a new
name cannot consume from or shift any existing stream.

The key initializes ChaCha12 at block counter zero with a zero 64-bit stream id.
Words and output are little-endian. `next_u64` consumes eight consecutive bytes.
Bounded integers reject values below `(-upper mod upper)` and then use modulo;
shuffle is descending Fisher–Yates; Rademacher maps bounded values 0/1 to −1/+1;
bootstrap draws bounded population indices with replacement. These algorithms
are implemented explicitly rather than inherited from a dependency API.

For master seed `0x0123456789abcdef`, the `opening-order` key is
`cb674f96baa6f4b214f5ab3db33e23491fb686ba7be898e895068429152ff49d` and
its first 64 bytes are
`c2198a09daf1a2cd09160d69492bff7c848323cd90f7fa7a916110036907f4553565d341820734b4348b985eb06a2ec5e975f2d492da09119596fc7ef6da02ff`.
Repository tests also pin bounded, shuffle, Rademacher, bootstrap and every
built-in stream-name vector. Changing any part requires a `stats_version`
change; resumed runs retain their recorded version.
