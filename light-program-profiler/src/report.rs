use std::collections::{BTreeMap, HashMap};
use std::fs::OpenOptions;
use std::io::Write;

/// A single profiled function's result.
#[derive(Debug, Clone)]
pub struct ProfileEntry {
    pub func_name: String,
    pub file_location: String,
    pub total_cu: u64,
    pub net_cu: u64,
}

/// A custom table rendered inside an instruction's section, after the CU
/// table. The title renders as bold text (not a heading), so it produces no
/// anchor or table-of-contents entry.
#[derive(Debug, Clone)]
pub struct SectionTable {
    pub title: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// Configuration for markdown report generation.
pub struct ReadmeConfig {
    pub title: String,
    pub description: String,
    /// Base URL for GitHub source links (e.g. "https://github.com/org/repo/blob/main/").
    /// Empty string disables source links.
    pub github_base_url: String,
    pub output_path: String,
    /// Optional command shown in the report for regeneration (e.g. "just bench").
    pub regenerate_command: Option<String>,
    /// Override display names for instruction categories. Falls back to snake_case -> Title Case.
    pub display_name_overrides: HashMap<String, String>,
    /// Fixed column width for function names in tables. None = auto-size to longest name.
    pub column_width: Option<usize>,
    /// Show one function's CU in the table of contents (e.g. Some("process_instruction")).
    /// None = no CU column in TOC.
    pub toc_summary_function: Option<String>,
}

impl Default for ReadmeConfig {
    fn default() -> Self {
        Self {
            title: String::new(),
            description: String::new(),
            github_base_url: String::new(),
            output_path: "CU_BENCHMARK.md".into(),
            regenerate_command: None,
            display_name_overrides: HashMap::new(),
            column_width: None,
            toc_summary_function: None,
        }
    }
}

/// Collects per-instruction profiling results and generates a markdown report.
pub struct CuBenchmark {
    config: ReadmeConfig,
    results: BTreeMap<String, Vec<ProfileEntry>>,
    tables: BTreeMap<String, Vec<SectionTable>>,
}

impl CuBenchmark {
    pub fn new(config: ReadmeConfig) -> Self {
        Self {
            config,
            results: BTreeMap::new(),
            tables: BTreeMap::new(),
        }
    }

    /// Parse profiling entries from program log lines and add them under `name`.
    pub fn add_from_logs(&mut self, name: &str, logs: &[String]) {
        let entries = parse_profiling_logs(logs);
        self.results
            .entry(name.to_string())
            .or_default()
            .extend(entries);
    }

    /// Add pre-built profiling entries under `name`.
    pub fn add_from_entries(&mut self, name: &str, entries: Vec<ProfileEntry>) {
        self.results
            .entry(name.to_string())
            .or_default()
            .extend(entries);
    }

    /// Append a custom table to `name`'s section, rendered after the CU table.
    /// Tables render in insertion order; a `name` without CU entries still gets
    /// its own section.
    pub fn add_table(&mut self, name: &str, table: SectionTable) {
        self.tables.entry(name.to_string()).or_default().push(table);
    }

    /// Write the markdown report to `config.output_path`.
    pub fn generate(&self) -> std::io::Result<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.config.output_path)?;

        // Header
        if !self.config.title.is_empty() {
            writeln!(f, "# {}\n", self.config.title)?;
        }
        if !self.config.description.is_empty() {
            writeln!(f, "{}\n", self.config.description)?;
        }
        if let Some(cmd) = &self.config.regenerate_command {
            writeln!(f, "Regenerate with `{}`.\n", cmd)?;
        }

        // Definitions
        writeln!(f, "## Definitions\n")?;
        writeln!(
            f,
            "- **Total CU**: Compute units consumed by the function including all children"
        )?;
        writeln!(
            f,
            "- **Net CU**: Compute units consumed by the function itself (excluding children)\n"
        )?;

        // Table of contents
        writeln!(f, "## Table of Contents\n")?;
        self.write_toc(&mut f)?;

        // Compute column width
        let func_width = self.config.column_width.unwrap_or_else(|| {
            self.results
                .values()
                .flat_map(|entries| entries.iter().map(|e| e.func_name.len() + 2))
                .max()
                .unwrap_or(8)
                .max(8)
        });

        // Sections
        for (i, name) in self.section_names().into_iter().enumerate() {
            let display_name = self.display_name(name);
            writeln!(f, "## {}. {}\n", i + 1, display_name)?;

            let entries = self.results.get(name).map(Vec::as_slice).unwrap_or(&[]);
            let tables = self.tables.get(name).map(Vec::as_slice).unwrap_or(&[]);

            if entries.is_empty() && tables.is_empty() {
                writeln!(f, "_No profiling data collected._\n")?;
                continue;
            }

            if !entries.is_empty() {
                writeln!(
                    f,
                    "| {:<fw$} | {:>10} | {:>10} |",
                    "Function",
                    "Total CU",
                    "Net CU",
                    fw = func_width
                )?;
                writeln!(
                    f,
                    "| {:-<fw$} | {:-<10} | {:-<10} |",
                    "",
                    "",
                    "",
                    fw = func_width
                )?;

                for entry in entries {
                    let func_display = if !self.config.github_base_url.is_empty()
                        && !entry.file_location.is_empty()
                    {
                        make_github_link(
                            &entry.func_name,
                            &entry.file_location,
                            &self.config.github_base_url,
                        )
                    } else {
                        format!("`{}`", entry.func_name)
                    };

                    writeln!(
                        f,
                        "| {:<fw$} | {:>10} | {:>10} |",
                        func_display,
                        format_thousands(entry.total_cu),
                        format_thousands(entry.net_cu),
                        fw = func_width,
                    )?;
                }
                writeln!(f)?;
            }

            for table in tables {
                write_section_table(&mut f, table)?;
            }
        }
        Ok(())
    }

    /// All section names, sorted: instructions with CU entries, custom tables,
    /// or both.
    fn section_names(&self) -> Vec<&String> {
        let mut names: Vec<&String> = self.results.keys().chain(self.tables.keys()).collect();
        names.sort();
        names.dedup();
        names
    }

    fn write_toc(&self, f: &mut impl Write) -> std::io::Result<()> {
        let has_cu_column = self.config.toc_summary_function.is_some();

        let toc_entries: Vec<(usize, String, Option<String>)> = self
            .section_names()
            .into_iter()
            .enumerate()
            .map(|(i, name)| {
                let anchor = name.to_lowercase().replace(' ', "-").replace('_', "-");
                let display = self.display_name(name);
                let link = format!("[{}](#{})", display, anchor);
                let cu = self.config.toc_summary_function.as_ref().and_then(|func| {
                    self.results.get(name).and_then(|entries| {
                        entries
                            .iter()
                            .find(|e| e.func_name == *func)
                            .map(|e| format_thousands(e.total_cu))
                    })
                });
                (i + 1, link, cu)
            })
            .collect();

        let max_link = toc_entries
            .iter()
            .map(|(_, l, _)| l.len())
            .max()
            .unwrap_or(20);

        if has_cu_column {
            let max_cu = toc_entries
                .iter()
                .filter_map(|(_, _, c)| c.as_ref().map(|s| s.len()))
                .max()
                .unwrap_or(6)
                .max(6);

            writeln!(
                f,
                "| {:<3} | {:<wl$} | {:>wc$} |",
                "#",
                "Instruction",
                "CU",
                wl = max_link,
                wc = max_cu
            )?;
            writeln!(
                f,
                "| {:-<3} | {:-<wl$} | {:-<wc$} |",
                "",
                "",
                "",
                wl = max_link,
                wc = max_cu
            )?;
            for (num, link, cu) in &toc_entries {
                let cu_str = cu.as_deref().unwrap_or("-");
                writeln!(
                    f,
                    "| {:<3} | {:<wl$} | {:>wc$} |",
                    num,
                    link,
                    cu_str,
                    wl = max_link,
                    wc = max_cu
                )?;
            }
        } else {
            for (num, link, _) in &toc_entries {
                writeln!(f, "{}. {}", num, link)?;
            }
        }
        writeln!(f)?;
        Ok(())
    }

    fn display_name(&self, name: &str) -> String {
        if let Some(override_name) = self.config.display_name_overrides.get(name) {
            return override_name.clone();
        }
        snake_to_title_case(name)
    }
}

/// Render a custom section table: bold title, header row, separator, then one
/// line per row. Each column is sized to its widest cell (header included);
/// data cells are right-aligned since they are typically numbers or durations.
fn write_section_table(f: &mut impl Write, table: &SectionTable) -> std::io::Result<()> {
    if !table.title.is_empty() {
        writeln!(f, "**{}**", table.title)?;
    }

    let widths: Vec<usize> = table
        .headers
        .iter()
        .enumerate()
        .map(|(i, header)| {
            table
                .rows
                .iter()
                .filter_map(|row| row.get(i))
                .map(String::len)
                .chain(std::iter::once(header.len()))
                .max()
                .unwrap_or(0)
        })
        .collect();

    let header_line: Vec<String> = table
        .headers
        .iter()
        .zip(&widths)
        .map(|(header, w)| format!("{:<w$}", header, w = w))
        .collect();
    writeln!(f, "| {} |", header_line.join(" | "))?;

    let separator: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    writeln!(f, "| {} |", separator.join(" | "))?;

    for row in &table.rows {
        let cells: Vec<String> = widths
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let cell = row.get(i).map(String::as_str).unwrap_or("");
                format!("{:>w$}", cell, w = w)
            })
            .collect();
        writeln!(f, "| {} |", cells.join(" | "))?;
    }
    writeln!(f)?;
    Ok(())
}

/// Parse profiling entries from program log lines.
///
/// Expects the log format emitted by the patched agave profiling runtime
/// (Lightprotocol/agave profiling-runtime branches). Each profiled function
/// produces a log entry starting with "Program log: #" containing the function
/// name, source location, and CU consumption.
pub fn parse_profiling_logs(logs: &[String]) -> Vec<ProfileEntry> {
    let mut entries = Vec::new();

    for log in logs {
        if !log.starts_with("Program log: #") {
            continue;
        }

        let content = log.strip_prefix("Program log: ").unwrap_or(log);
        let lines: Vec<&str> = content.lines().collect();

        if lines.len() < 2 {
            continue;
        }

        // First line: "# <seq>    func_name"
        let first_line = lines[0].trim();
        let func_name = if let Some(pos) = first_line.find("    ") {
            first_line[pos..].trim().to_string()
        } else {
            continue;
        };

        // The PROFILE_ID may embed file location after a newline
        let (func_name, embedded_location) = if let Some(nl_pos) = func_name.find('\n') {
            (
                func_name[..nl_pos].to_string(),
                func_name[nl_pos + 1..].trim().to_string(),
            )
        } else {
            (func_name, String::new())
        };

        let file_location = if !embedded_location.is_empty() {
            embedded_location
        } else if lines.len() > 1 {
            let loc = lines[1].trim().to_string();
            if let Some(pos) = loc.find("src/") {
                loc[pos..].to_string()
            } else {
                loc
            }
        } else {
            String::new()
        };

        let mut total_cu = 0u64;
        let mut net_cu = 0u64;

        for line in &lines {
            if line.contains("consumed") && line.contains("net") {
                if let Some(consumed_pos) = line.find("consumed") {
                    let parts: Vec<&str> = line[consumed_pos + 8..].split_whitespace().collect();
                    if !parts.is_empty() {
                        total_cu = parts[0].parse().unwrap_or(0);
                    }
                }
                if let Some(net_pos) = line.find("(net") {
                    let after = line[net_pos + 4..].trim_start();
                    if let Some(end) = after.find(')') {
                        net_cu = after[..end].trim().parse().unwrap_or(0);
                    }
                }
                break;
            }
        }

        if total_cu > 0 || net_cu > 0 {
            entries.push(ProfileEntry {
                func_name,
                file_location,
                total_cu,
                net_cu,
            });
        }
    }

    entries
}

fn format_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(c);
    }
    result
}

fn snake_to_title_case(name: &str) -> String {
    name.split('_')
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

fn make_github_link(func_name: &str, file_location: &str, github_base_url: &str) -> String {
    let parts: Vec<&str> = file_location.split(':').collect();
    if parts.len() >= 2 {
        let file_path = parts[0];
        let line_num = parts[1].trim().parse::<usize>().unwrap_or(0) + 1;
        format!(
            "[`{}`]({}{}#L{})",
            func_name, github_base_url, file_path, line_num
        )
    } else {
        format!("`{}`", func_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_thousands() {
        assert_eq!(format_thousands(0), "0");
        assert_eq!(format_thousands(999), "999");
        assert_eq!(format_thousands(1000), "1,000");
        assert_eq!(format_thousands(94804), "94,804");
        assert_eq!(format_thousands(1_000_000), "1,000,000");
    }

    #[test]
    fn test_snake_to_title_case() {
        assert_eq!(snake_to_title_case("sync_transfer"), "Sync Transfer");
        assert_eq!(
            snake_to_title_case("create_associated_token_account"),
            "Create Associated Token Account"
        );
        assert_eq!(snake_to_title_case("close"), "Close");
    }

    #[test]
    fn test_parse_profiling_logs_empty() {
        let logs: Vec<String> = vec!["Program invoked".into(), "Program success".into()];
        let entries = parse_profiling_logs(&logs);
        assert!(entries.is_empty());
    }

    #[test]
    fn test_parse_profiling_logs_basic() {
        let logs = vec![
            "Program log: #  1    parse_transfer\n        src/sync/transfer/parser.rs:17        \nCU                                                  consumed   353 (net   353) of 200000 CU".to_string(),
        ];
        let entries = parse_profiling_logs(&logs);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].func_name, "parse_transfer");
        assert_eq!(entries[0].total_cu, 353);
        assert_eq!(entries[0].net_cu, 353);
    }

    #[test]
    fn test_write_section_table() {
        let table = SectionTable {
            title: "Transaction Size".into(),
            headers: vec![
                "ix data (B)".into(),
                "accounts".into(),
                "legacy tx (B)".into(),
            ],
            rows: vec![vec!["949".into(), "4".into(), "1227".into()]],
        };
        let mut buf = Vec::new();
        write_section_table(&mut buf, &table).unwrap();
        let rendered = String::from_utf8(buf).unwrap();
        assert_eq!(
            rendered,
            "**Transaction Size**\n\
             | ix data (B) | accounts | legacy tx (B) |\n\
             | ----------- | -------- | ------------- |\n\
             |         949 |        4 |          1227 |\n\n"
        );
    }

    #[test]
    fn test_write_section_table_cell_wider_than_header() {
        let table = SectionTable {
            title: "Proving Time".into(),
            headers: vec!["Total".into()],
            rows: vec![vec!["1,215 ms".into()], vec!["69 ms".into()]],
        };
        let mut buf = Vec::new();
        write_section_table(&mut buf, &table).unwrap();
        let rendered = String::from_utf8(buf).unwrap();
        assert_eq!(
            rendered,
            "**Proving Time**\n\
             | Total    |\n\
             | -------- |\n\
             | 1,215 ms |\n\
             |    69 ms |\n\n"
        );
    }

    #[test]
    fn test_add_table_creates_section_without_cu_entries() {
        let mut bench = CuBenchmark::new(ReadmeConfig::default());
        bench.add_from_entries(
            "fill",
            vec![ProfileEntry {
                func_name: "process_fill".into(),
                file_location: String::new(),
                total_cu: 100,
                net_cu: 100,
            }],
        );
        bench.add_table(
            "cancel",
            SectionTable {
                title: "Transaction Size".into(),
                headers: vec!["legacy tx (B)".into()],
                rows: vec![vec!["836".into()]],
            },
        );
        assert_eq!(bench.section_names(), ["cancel", "fill"]);
    }

    #[test]
    fn test_readme_config_default() {
        let config = ReadmeConfig::default();
        assert_eq!(config.output_path, "CU_BENCHMARK.md");
        assert!(config.regenerate_command.is_none());
        assert!(config.column_width.is_none());
        assert!(config.toc_summary_function.is_none());
    }
}
