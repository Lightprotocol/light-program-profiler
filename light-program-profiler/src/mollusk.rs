use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs::OpenOptions;
use std::io::Write;

use solana_program_runtime::{
    invoke_context::InvokeContext,
    solana_sbpf::{
        declare_builtin_function,
        memory_region::{AccessType, MemoryMapping},
        vm::ContextObject,
    },
};

// ---------------------------------------------------------------------------
// ProfilingState (from agave/program-runtime/src/profiling_state.rs)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ActiveEntry {
    pub id: String,
    pub start_cu: u64,
    pub start_sequence: usize,
    pub start_heap: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct HeapMetrics {
    pub start_heap: u64,
    pub end_heap: u64,
    pub total_heap: u64,
    pub net_heap: u64,
    pub remaining_heap: u64,
}

#[derive(Debug, Clone)]
pub struct CompletedEntry {
    pub id: String,
    pub start_cu: u64,
    pub end_cu: u64,
    pub start_sequence: usize,
    pub end_sequence: usize,
    pub total_cu: u64,
    pub net_cu: u64,
    pub remaining_cu: u64,
    pub heap: Option<HeapMetrics>,
}

#[derive(Debug, Default)]
pub struct ProfilingState {
    active_stack: Vec<ActiveEntry>,
    completed: Vec<CompletedEntry>,
    next_sequence: usize,
}

impl ProfilingState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self, id: String, current_cu: u64, heap_value: u64, with_heap: bool) {
        let entry = ActiveEntry {
            id,
            start_cu: current_cu,
            start_sequence: self.next_sequence,
            start_heap: if with_heap { Some(heap_value) } else { None },
        };
        self.active_stack.push(entry);
        self.next_sequence += 1;
    }

    pub fn end(
        &mut self,
        id: &str,
        current_cu: u64,
        heap_value: u64,
        with_heap: bool,
    ) -> Result<(), String> {
        let pos = self
            .active_stack
            .iter()
            .rposition(|entry| entry.id == id)
            .ok_or_else(|| format!("No active profiling section found for ID: {}", id))?;

        let active_entry = self.active_stack.remove(pos);
        let total_cu = active_entry.start_cu.saturating_sub(current_cu);

        let heap = if let Some(start_heap_value) = active_entry.start_heap {
            if with_heap {
                let total_heap = heap_value.saturating_sub(start_heap_value);
                let remaining_heap = 32_000u64.saturating_sub(start_heap_value);
                Some(HeapMetrics {
                    start_heap: start_heap_value,
                    end_heap: heap_value,
                    total_heap,
                    net_heap: 0,
                    remaining_heap,
                })
            } else {
                None
            }
        } else {
            None
        };

        let completed_entry = CompletedEntry {
            id: active_entry.id,
            start_cu: active_entry.start_cu,
            end_cu: current_cu,
            start_sequence: active_entry.start_sequence,
            end_sequence: self.next_sequence,
            total_cu,
            net_cu: 0,
            remaining_cu: active_entry.start_cu,
            heap,
        };

        self.completed.push(completed_entry);
        self.next_sequence += 1;
        Ok(())
    }

    pub fn post_process(&mut self) {
        for i in 0..self.completed.len() {
            let mut children_cu = 0;
            let mut children_heap = 0;
            let entry = &self.completed[i];

            for other in &self.completed {
                if other.start_sequence > entry.start_sequence
                    && other.end_sequence < entry.end_sequence
                {
                    children_cu += other.total_cu;
                    if let Some(ref other_heap) = other.heap {
                        children_heap += other_heap.total_heap;
                    }
                }
            }

            self.completed[i].net_cu = entry.total_cu.saturating_sub(children_cu);
            if let Some(ref mut heap) = self.completed[i].heap {
                heap.net_heap = heap.total_heap.saturating_sub(children_heap);
            }
        }
    }

    pub fn get_completed(&self) -> &[CompletedEntry] {
        &self.completed
    }

    pub fn clear(&mut self) {
        self.active_stack.clear();
        self.completed.clear();
        self.next_sequence = 0;
    }

    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }
}

// ---------------------------------------------------------------------------
// Thread-local profiling state
// ---------------------------------------------------------------------------

thread_local! {
    pub static PROFILING_STATE: RefCell<ProfilingState> = RefCell::new(ProfilingState::new());
}

// ---------------------------------------------------------------------------
// Memory translation (translate_slice is not public in solana-program-runtime 2.x)
// ---------------------------------------------------------------------------

fn translate_slice<'a>(
    memory_mapping: &'a MemoryMapping,
    vm_addr: u64,
    len: u64,
) -> Result<&'a [u8], Box<dyn std::error::Error>> {
    let host_addr: u64 = Result::from(memory_mapping.map(AccessType::Load, vm_addr, len))?;
    Ok(unsafe { std::slice::from_raw_parts(host_addr as *const u8, len as usize) })
}

// ---------------------------------------------------------------------------
// Custom syscalls
// ---------------------------------------------------------------------------

declare_builtin_function!(
    SyscallLogComputeUnitsStart,
    fn rust(
        invoke_context: &mut InvokeContext,
        id_addr: u64,
        id_len: u64,
        heap_value: u64,
        with_heap: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let current_cu = invoke_context.get_remaining();
        let buf = translate_slice(memory_mapping, id_addr, id_len)?;
        let id = std::str::from_utf8(buf)?.to_string();
        PROFILING_STATE.with(|state| {
            state
                .borrow_mut()
                .start(id, current_cu, heap_value, with_heap != 0);
        });
        Ok(0)
    }
);

declare_builtin_function!(
    SyscallLogComputeUnitsEnd,
    fn rust(
        invoke_context: &mut InvokeContext,
        id_addr: u64,
        id_len: u64,
        heap_value: u64,
        with_heap: u64,
        _arg5: u64,
        memory_mapping: &mut MemoryMapping,
    ) -> Result<u64, Box<dyn std::error::Error>> {
        let current_cu = invoke_context.get_remaining();
        let buf = translate_slice(memory_mapping, id_addr, id_len)?;
        let id = std::str::from_utf8(buf)?;
        PROFILING_STATE.with(|state| {
            let _ = state
                .borrow_mut()
                .end(id, current_cu, heap_value, with_heap != 0);
        });
        Ok(0)
    }
);

// ---------------------------------------------------------------------------
// Registration helper
// ---------------------------------------------------------------------------

/// Register profiling syscalls on a Mollusk instance.
/// Must be called BEFORE `add_program`.
pub fn register_profiling_syscalls(mollusk: &mut mollusk_svm::Mollusk) {
    mollusk
        .program_cache
        .program_runtime_environment
        .register_function(
            "sol_log_compute_units_start",
            SyscallLogComputeUnitsStart::vm,
        )
        .unwrap();
    mollusk
        .program_cache
        .program_runtime_environment
        .register_function("sol_log_compute_units_end", SyscallLogComputeUnitsEnd::vm)
        .unwrap();
}

// ---------------------------------------------------------------------------
// Profiling result extraction
// ---------------------------------------------------------------------------

/// Take profiling results, post-process, and clear state for next instruction.
/// Returns Vec of (func_name, cu_consumed, file_location).
pub fn take_profiling_results() -> Vec<(String, u64, String)> {
    PROFILING_STATE.with(|state| {
        let mut state = state.borrow_mut();
        if state.completed_count() == 0 {
            return vec![];
        }
        state.post_process();
        let results = state
            .get_completed()
            .iter()
            .map(|entry| {
                // PROFILE_ID format: "func_name\n        src/path/file.rs:line        "
                let (func_name, file_location) = if let Some(pos) = entry.id.find('\n') {
                    (
                        entry.id[..pos].to_string(),
                        entry.id[pos + 1..].trim().to_string(),
                    )
                } else {
                    (entry.id.clone(), String::new())
                };
                (func_name, entry.total_cu, file_location)
            })
            .collect();
        state.clear();
        results
    })
}

// ---------------------------------------------------------------------------
// README generation
// ---------------------------------------------------------------------------

/// Configuration for README benchmark report generation.
///
/// # Example
///
/// ```rust,no_run
/// use std::collections::HashMap;
/// use light_program_profiler::mollusk::{
///     register_profiling_syscalls, take_profiling_results,
///     extract_category_and_file, write_categorized_readme,
///     BenchmarkEntry, BenchmarkResults, ReadmeConfig,
/// };
/// use mollusk_svm::Mollusk;
/// use solana_pubkey::Pubkey;
///
/// let program_id = Pubkey::new_unique();
/// let mut mollusk = Mollusk::default();
/// register_profiling_syscalls(&mut mollusk);
/// mollusk.add_program(
///     &program_id,
///     "my_program",
///     &mollusk_svm::program::loader_keys::LOADER_V3,
/// );
///
/// let mut results = BenchmarkResults::new();
///
/// let instruction = solana_instruction::Instruction::new_with_bytes(
///     program_id, &[0, 0], vec![],
/// );
/// mollusk.process_instruction(&instruction, &[]);
///
/// for (func_name, cu_consumed, file_location) in take_profiling_results() {
///     let (category, filename) = extract_category_and_file(&file_location);
///     results
///         .entry(category)
///         .or_default()
///         .entry(filename)
///         .or_default()
///         .push(BenchmarkEntry {
///             func_name,
///             cu_value: cu_consumed.to_string(),
///             file_location,
///         });
/// }
///
/// let config = ReadmeConfig {
///     title: "My Program Benchmarks".to_string(),
///     description: "CU benchmarks for my Solana program.".to_string(),
///     github_base_url: "https://github.com/myorg/myrepo/blob/main/".to_string(),
///     output_path: "README.md".to_string(),
///     display_name_overrides: HashMap::new(),
/// };
/// write_categorized_readme(&config, results);
/// ```
pub struct ReadmeConfig {
    pub title: String,
    pub description: String,
    /// Base URL for GitHub source links, e.g. "https://github.com/org/repo/blob/main/"
    pub github_base_url: String,
    pub output_path: String,
    /// Optional overrides for folder_name -> display_name.
    /// Falls back to snake_case -> Title Case conversion.
    pub display_name_overrides: HashMap<String, String>,
}

/// Extract category and file from a profiling file location string.
/// Format: "src/folder/file.rs:line" -> ("folder", "file_stem")
/// Special case: "src/lib.rs:line" -> ("baseline", "lib")
pub fn extract_category_and_file(file_location: &str) -> (String, String) {
    if file_location == "src/lib.rs" || file_location.starts_with("src/lib.rs:") {
        return ("baseline".to_string(), "lib".to_string());
    }

    if let Some(without_src) = file_location.strip_prefix("src/") {
        let path_parts: Vec<&str> = without_src.split('/').collect();

        if path_parts.len() >= 2 {
            let folder_name = path_parts[0];
            let file_part = path_parts[1];
            let file_stem = file_part
                .split(':')
                .next()
                .unwrap_or(file_part)
                .trim_end_matches(".rs");
            return (folder_name.to_string(), file_stem.to_string());
        } else if !path_parts.is_empty() {
            let folder_name = path_parts[0];
            let clean_folder = folder_name.split('.').next().unwrap_or(folder_name);
            let clean_folder = clean_folder.split(':').next().unwrap_or(clean_folder);
            return (clean_folder.to_string(), "unknown".to_string());
        }
    }

    ("other".to_string(), "unknown".to_string())
}

/// Convert snake_case string to Title Case.
pub fn snake_to_title_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

fn get_display_name(folder_name: &str, overrides: &HashMap<String, String>) -> String {
    if let Some(name) = overrides.get(folder_name) {
        return name.clone();
    }
    snake_to_title_case(folder_name)
}

fn add_indentation(text: &str, level: usize) -> String {
    let indent = "  ".repeat(level);
    format!("{}{}", indent, text)
}

/// A single benchmark entry: one profiled function's result.
#[derive(Debug, Clone)]
pub struct BenchmarkEntry {
    pub func_name: String,
    pub cu_value: String,
    pub file_location: String,
}

/// Benchmark results organized as category -> file_stem -> entries.
pub type BenchmarkResults = BTreeMap<String, BTreeMap<String, Vec<BenchmarkEntry>>>;

/// Write a categorized README.md from benchmark results.
pub fn write_categorized_readme(config: &ReadmeConfig, mut results: BenchmarkResults) {
    let mut readme = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&config.output_path)
        .expect("Failed to create output file");

    writeln!(readme, "# {}\n", config.title).unwrap();
    writeln!(readme, "{}\n", config.description).unwrap();

    // Table of contents
    writeln!(readme, "## Table of Contents\n").unwrap();

    let mut section_number = 1;
    let mut baseline_number = 0;
    if results.contains_key("baseline") {
        writeln!(
            readme,
            "**[{}. Baseline](#{}-baseline)**\n",
            section_number, section_number
        )
        .unwrap();

        if let Some(files_map) = results.get("baseline") {
            let mut file_number = 1;
            for file_stem in files_map.keys() {
                let file_display_name = snake_to_title_case(file_stem);
                let anchor = format!(
                    "{}{}-{}",
                    section_number,
                    file_number,
                    file_display_name.to_lowercase().replace(' ', "-")
                );
                writeln!(
                    readme,
                    "  - [{}.{} {}](#{})",
                    section_number, file_number, file_display_name, anchor
                )
                .unwrap();
                file_number += 1;
            }
        }
        writeln!(readme).unwrap();
        baseline_number = section_number;
        section_number += 1;
    }

    let mut category_numbers = BTreeMap::new();
    for category in results.keys() {
        if category != "baseline" {
            let display_name = get_display_name(category, &config.display_name_overrides);
            let anchor = format!("{}-{}", section_number, category.replace('_', "-"));
            writeln!(
                readme,
                "**[{}. {}](#{})**\n",
                section_number, display_name, anchor
            )
            .unwrap();

            if let Some(files_map) = results.get(category) {
                let mut file_number = 1;
                for file_stem in files_map.keys() {
                    let file_display_name = snake_to_title_case(file_stem);
                    let anchor = format!(
                        "{}{}-{}",
                        section_number,
                        file_number,
                        file_display_name.to_lowercase().replace(' ', "-")
                    );
                    writeln!(
                        readme,
                        "  - [{}.{} {}](#{})",
                        section_number, file_number, file_display_name, anchor
                    )
                    .unwrap();
                    file_number += 1;
                }
            }
            writeln!(readme).unwrap();
            category_numbers.insert(category.clone(), section_number);
            section_number += 1;
        }
    }

    writeln!(readme).unwrap();

    // Definitions
    writeln!(readme, "## Definitions\n").unwrap();
    writeln!(
        readme,
        "- **CU Consumed**: Total compute units consumed by the profiled function"
    )
    .unwrap();
    writeln!(
        readme,
        "- **CU Adjusted**: Actual operation cost with baseline profiling overhead subtracted (CU Consumed - Baseline CU)"
    )
    .unwrap();
    writeln!(
        readme,
        "- **Baseline CU**: CU consumed by an empty profiled function (`#[profile]` macro overhead)\n"
    )
    .unwrap();

    // Get baseline CU
    let mut baseline_cu: u64 = 0;
    if let Some(baseline_files) = results.get("baseline") {
        if let Some(first_file_results) = baseline_files.values().next() {
            if let Some(entry) = first_file_results.first() {
                baseline_cu = entry.cu_value.parse::<u64>().unwrap_or(0);
            }
        }
    }

    let table_header = add_indentation("| Function                                                                                                                                                                                                                | CU Consumed | CU Adjusted |", 1);
    let table_separator = add_indentation("|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-------------|-------------|", 1);

    // Write baseline category first
    if let Some(baseline_files) = results.remove("baseline") {
        writeln!(readme, "## {}. Baseline\n", baseline_number).unwrap();

        let mut file_number = 1;
        for (file_stem, file_results) in baseline_files {
            let file_display_name = snake_to_title_case(&file_stem);
            let indented_header = add_indentation(
                &format!(
                    "### {}.{} {}",
                    baseline_number, file_number, file_display_name
                ),
                1,
            );
            writeln!(readme, "{}\n", indented_header).unwrap();
            writeln!(readme, "{}", table_header).unwrap();
            writeln!(readme, "{}", table_separator).unwrap();

            for entry in file_results {
                let github_link = make_github_link(
                    &entry.func_name,
                    &entry.file_location,
                    &config.github_base_url,
                );
                let table_row = add_indentation(
                    &format!(
                        "| {:<180} | {:<11} | {:<11} |",
                        github_link, entry.cu_value, "N/A"
                    ),
                    1,
                );
                writeln!(readme, "{}", table_row).unwrap();
            }
            writeln!(readme).unwrap();
            file_number += 1;
        }
    }

    // Write remaining categories
    for (category, files_map) in results {
        let display_name = get_display_name(&category, &config.display_name_overrides);
        let number = category_numbers.get(&category).unwrap_or(&0);

        writeln!(readme, "## {}. {}\n", number, display_name).unwrap();

        let mut file_number = 1;
        for (file_stem, file_results) in files_map {
            let file_display_name = snake_to_title_case(&file_stem);
            let indented_header = add_indentation(
                &format!("### {}.{} {}", number, file_number, file_display_name),
                1,
            );
            writeln!(readme, "{}\n", indented_header).unwrap();
            writeln!(readme, "{}", table_header).unwrap();
            writeln!(readme, "{}", table_separator).unwrap();

            for entry in file_results {
                let github_link = make_github_link(
                    &entry.func_name,
                    &entry.file_location,
                    &config.github_base_url,
                );
                let cu_consumed = entry.cu_value.parse::<u64>().unwrap_or(0);
                let cu_adjusted = if cu_consumed >= baseline_cu {
                    (cu_consumed - baseline_cu).to_string()
                } else {
                    "0".to_string()
                };
                let table_row = add_indentation(
                    &format!(
                        "| {:<180} | {:<11} | {:<11} |",
                        github_link, entry.cu_value, cu_adjusted
                    ),
                    1,
                );
                writeln!(readme, "{}", table_row).unwrap();
            }
            writeln!(readme).unwrap();
            file_number += 1;
        }
    }
}

fn make_github_link(func_name: &str, file_location: &str, github_base_url: &str) -> String {
    if file_location.is_empty() {
        return func_name.to_string();
    }
    let parts: Vec<&str> = file_location.split(':').collect();
    if parts.len() >= 2 {
        let file_path = parts[0];
        let line_num = parts[1].trim().parse::<usize>().unwrap_or(0) + 1;
        format!(
            "[{}]({}{}#L{})",
            func_name, github_base_url, file_path, line_num
        )
    } else {
        func_name.to_string()
    }
}
