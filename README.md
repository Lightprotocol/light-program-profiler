# light-program-profiler

Profiler macros with custom syscalls for measuring CU and heap consumption in Solana programs.

## Usage

### Solana program

Annotate functions with `#[profile]`:

```rust
use light_program_profiler::profile;

#[profile]
pub fn my_function() -> u64 {
    let a: u64 = 1000;
    let b: u64 = 2000;
    a.wrapping_add(b)
}
```

```toml
[dependencies]
light-program-profiler = { git = "https://github.com/Lightprotocol/light-program-profiler", features = ["profile-program", "inline"] }
```

### Test harness

#### Option 1: Mollusk (preferred)

```toml
[dev-dependencies]
light-program-profiler = { git = "https://github.com/Lightprotocol/light-program-profiler", features = ["mollusk"] }
mollusk-svm = "0.3.0"
```

```rust
use light_program_profiler::mollusk::{
    register_profiling_syscalls, take_profiling_results,
    extract_category_and_file, write_categorized_readme, ReadmeConfig,
};
use mollusk_svm::Mollusk;

let mut mollusk = Mollusk::default();
register_profiling_syscalls(&mut mollusk);
mollusk.add_program(&program_id, "my_program", &mollusk_svm::program::loader_keys::LOADER_V3);

mollusk.process_instruction(&instruction, &accounts);

for (func_name, cu_consumed, file_location) in take_profiling_results() {
    println!("{}: {} CU at {}", func_name, cu_consumed, file_location);
}
```

`ReadmeConfig` and `write_categorized_readme` generate benchmark README files from collected results. See the doc example on `ReadmeConfig` for the full workflow.

#### Option 2: LiteSVM with agave fork

Patch `solana-program-runtime` and `solana-bpf-loader-program` to use the Lightprotocol agave fork, which has the profiling syscalls built into the runtime:

```toml
[dev-dependencies]
litesvm = "0.7"

[patch.crates-io]
solana-program-runtime = { git = "https://github.com/Lightprotocol/agave", rev = "be34dc76559e14921a2c8610f2fa2402bf0684bb" }
solana-bpf-loader-program = { git = "https://github.com/Lightprotocol/agave", rev = "be34dc76559e14921a2c8610f2fa2402bf0684bb" }
```

Profiling results are written to transaction logs and must be parsed by the caller.

## Features

- `profile-program` -- enables `#[profile]` macro instrumentation
- `profile-heap` -- includes heap usage tracking
- `inline` -- adds `#[inline(always)]` to profiled functions
- `mollusk` -- Mollusk test harness integration (syscall registration, README generation)

## Solana versions

- Solana program: `2.2+`
- Mollusk feature: `solana-program-runtime 2.3+`, `mollusk-svm 0.3.0`
- Agave fork: `2.3`
