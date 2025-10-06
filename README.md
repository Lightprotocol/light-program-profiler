# light-program-profiler


Profiler macros with custom profiler syscalls.

solana crate versions:
- `2.3`
- compatible with litesvm `0.7`

```
[patch.crates-io]
# Profiling logs and state is handled here
solana-program-runtime = { git = "https://github.com/Lightprotocol/agave", rev = "be34dc76559e14921a2c8610f2fa2402bf0684bb" }
# Profiling syscalls are defined here
solana-bpf-loader-program = { git = "https://github.com/Lightprotocol/agave", rev = "be34dc76559e14921a2c8610f2fa2402bf0684bb" }
```
