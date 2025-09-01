# light-program-profiler


Profiler macros with custom profiler syscalls.

solana crate versions:
- `2.2.1`
- compatible with litesvm `0.6,1`

```
[patch.crates-io]
# Profiling logs and state is handled here
solana-program-runtime = { git = "https://github.com/Lightprotocol/agave", rev = "580e29f03e4176a4a5525abc188a948c6595c47f" }
# Profiling syscalls are defined here
solana-bpf-loader-program = { git = "https://github.com/Lightprotocol/agave", rev = "580e29f03e4176a4a5525abc188a948c6595c47f" }
# Patch solana-program-memory to use older version where is_nonoverlapping is public
solana-program-memory = { git = "https://github.com/anza-xyz/solana-sdk", rev = "1c1d667f161666f12f5a43ebef8eda9470a8c6ee" }
```
