use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct BenchmarkMatrixFile {
    source: BenchmarkSource,
    families: BTreeMap<String, BenchmarkFamily>,
    benchmarks: BTreeMap<String, BenchmarkEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct BenchmarkSource {
    display_name: String,
    url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct BenchmarkFamily {
    order: u32,
    display_name: String,
    why_it_matters: String,
}

#[derive(Debug, Clone, Deserialize)]
struct BenchmarkEntry {
    order: u32,
    display_name: String,
    family: String,
    coverage_level: CoverageLevel,
    summary: String,
    reference_url: String,
    #[serde(default)]
    aliases: Vec<String>,
    why_relevant: Vec<String>,
    amai_focus: Vec<String>,
    current_coverage: Vec<String>,
    current_gaps: Vec<String>,
    proof_commands: Vec<String>,
    next_step: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum CoverageLevel {
    Materialized,
    Partial,
    Mapped,
    NextPriority,
    Future,
}

pub fn print_matrix(repo_root: &Path) -> Result<()> {
    let matrix = load_matrix(repo_root)?;
    println!("Amai benchmark matrix");
    println!();
    println!(
        "Источник внешнего benchmark-ландшафта: {}",
        matrix.source.display_name
    );
    println!("{}", matrix.source.url);
    println!();
    println!(
        "Это каноническая карта benchmark-семейств и конкретных эталонов, на которые Amai должен ориентироваться."
    );
    println!(
        "Матрица нужна не для коллекции ссылок, а для того, чтобы видно было: что уже покрыто, что следующая приоритетная цель и где ещё есть реальные пробелы."
    );
    println!();

    for (family_code, family) in ordered_families(&matrix) {
        let entries = family_benchmarks(&matrix, family_code);
        println!("{} ({})", family.display_name, family_code);
        println!("{}", family.why_it_matters);
        println!("Эталонов в этом семействе: {}", entries.len());
        for (benchmark_code, benchmark) in entries {
            println!(
                "- {} ({}) — {}",
                benchmark.display_name,
                benchmark_code,
                coverage_title(benchmark.coverage_level)
            );
            println!("  {}", benchmark.summary);
        }
        println!();
    }
    Ok(())
}

pub fn print_benchmark_explainer(repo_root: &Path, benchmark_query: &str) -> Result<()> {
    let matrix = load_matrix(repo_root)?;
    let (benchmark_code, benchmark) = resolve_benchmark(&matrix, benchmark_query)
        .ok_or_else(|| anyhow!("unknown benchmark: {benchmark_query}"))?;
    let family = matrix
        .families
        .get(&benchmark.family)
        .ok_or_else(|| anyhow!("benchmark family missing in matrix: {}", benchmark.family))?;

    println!("Amai benchmark explainer");
    println!();
    println!("Benchmark: {} ({})", benchmark.display_name, benchmark_code);
    println!("Семейство: {} ({})", family.display_name, benchmark.family);
    println!(
        "Статус для Amai: {}",
        coverage_title(benchmark.coverage_level)
    );
    println!("Ссылка: {}", benchmark.reference_url);
    println!();
    println!("Что это за benchmark:");
    println!("{}", benchmark.summary);
    println!();
    println!("Почему он важен для Amai:");
    for item in &benchmark.why_relevant {
        println!("- {}", item);
    }
    println!();
    println!("Какие контуры Amai он должен проверять:");
    for item in &benchmark.amai_focus {
        println!("- {}", item);
    }
    println!();
    println!("Что у нас уже есть:");
    for item in &benchmark.current_coverage {
        println!("- {}", item);
    }
    if !benchmark.current_gaps.is_empty() {
        println!();
        println!("Чего ещё не хватает:");
        for item in &benchmark.current_gaps {
            println!("- {}", item);
        }
    }
    if !benchmark.proof_commands.is_empty() {
        println!();
        println!("Что уже можно прогонять локально:");
        for item in &benchmark.proof_commands {
            println!("- {}", item);
        }
    }
    println!();
    println!("Следующий шаг:");
    println!("- {}", benchmark.next_step);
    Ok(())
}

pub fn print_coverage(repo_root: &Path) -> Result<()> {
    let matrix = load_matrix(repo_root)?;
    println!("Amai benchmark coverage");
    println!();
    println!(
        "Это сводка не по красивым словам, а по тем benchmark-семействам, где у Amai уже есть materialized задел и где ещё остаётся долг."
    );
    println!();

    let totals = coverage_counts(matrix.benchmarks.values().map(|entry| entry.coverage_level));
    println!("Общая картина:");
    print_counts(&totals);
    println!();

    for (family_code, family) in ordered_families(&matrix) {
        println!("{} ({})", family.display_name, family_code);
        let entries = family_benchmarks(&matrix, family_code);
        let counts = coverage_counts(entries.iter().map(|(_, entry)| entry.coverage_level));
        print_counts(&counts);
        let next_priority = entries
            .iter()
            .filter(|(_, entry)| entry.coverage_level == CoverageLevel::NextPriority)
            .map(|(code, entry)| format!("{} ({})", entry.display_name, code))
            .collect::<Vec<_>>();
        if !next_priority.is_empty() {
            println!("Следующие приоритеты:");
            for item in next_priority {
                println!("- {}", item);
            }
        }
        println!();
    }
    Ok(())
}

pub fn coverage_json(repo_root: &Path) -> Result<Value> {
    let matrix = load_matrix(repo_root)?;
    let totals = coverage_counts(matrix.benchmarks.values().map(|entry| entry.coverage_level));
    let families = ordered_families(&matrix)
        .into_iter()
        .map(|(family_code, family)| {
            let entries = family_benchmarks(&matrix, family_code);
            let counts = coverage_counts(entries.iter().map(|(_, entry)| entry.coverage_level));
            let next_priorities = entries
                .iter()
                .filter(|(_, entry)| entry.coverage_level == CoverageLevel::NextPriority)
                .map(|(code, entry)| format!("{} ({})", entry.display_name, code))
                .collect::<Vec<_>>();
            json!({
                "family_code": family_code,
                "display_name": family.display_name,
                "why_it_matters": family.why_it_matters,
                "coverage_counts": coverage_counts_json(&counts),
                "next_priorities": next_priorities,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "source": {
            "display_name": matrix.source.display_name,
            "url": matrix.source.url,
        },
        "coverage_counts": coverage_counts_json(&totals),
        "families": families,
    }))
}

fn print_counts(counts: &CoverageCounts) {
    println!("- Всего benchmark-эталонов в матрице: {}", counts.total);
    println!("- Уже materialized напрямую: {}", counts.materialized);
    println!(
        "- Частично покрыто текущими proof/harness слоями: {}",
        counts.partial
    );
    println!(
        "- Уже mapped в канонический план и Rust-first contours: {}",
        counts.mapped
    );
    println!(
        "- Следующий обязательный приоритет: {}",
        counts.next_priority
    );
    println!("- Пока только будущий слой: {}", counts.future);
}

fn ordered_families(matrix: &BenchmarkMatrixFile) -> Vec<(&String, &BenchmarkFamily)> {
    let mut entries = matrix.families.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(code, family)| (family.order, *code));
    entries
}

fn family_benchmarks<'a>(
    matrix: &'a BenchmarkMatrixFile,
    family_code: &str,
) -> Vec<(&'a String, &'a BenchmarkEntry)> {
    let mut entries = matrix
        .benchmarks
        .iter()
        .filter(|(_, benchmark)| benchmark.family == family_code)
        .collect::<Vec<_>>();
    entries.sort_by_key(|(code, benchmark)| (benchmark.order, *code));
    entries
}

#[derive(Default)]
struct CoverageCounts {
    total: usize,
    materialized: usize,
    partial: usize,
    mapped: usize,
    next_priority: usize,
    future: usize,
}

fn coverage_counts(levels: impl Iterator<Item = CoverageLevel>) -> CoverageCounts {
    let mut counts = CoverageCounts::default();
    for level in levels {
        counts.total += 1;
        match level {
            CoverageLevel::Materialized => counts.materialized += 1,
            CoverageLevel::Partial => counts.partial += 1,
            CoverageLevel::Mapped => counts.mapped += 1,
            CoverageLevel::NextPriority => counts.next_priority += 1,
            CoverageLevel::Future => counts.future += 1,
        }
    }
    counts
}

fn coverage_counts_json(counts: &CoverageCounts) -> Value {
    json!({
        "total": counts.total,
        "materialized": counts.materialized,
        "partial": counts.partial,
        "mapped": counts.mapped,
        "next_priority": counts.next_priority,
        "future": counts.future,
    })
}

fn coverage_title(level: CoverageLevel) -> &'static str {
    match level {
        CoverageLevel::Materialized => "уже materialized",
        CoverageLevel::Partial => "частично покрыто",
        CoverageLevel::Mapped => "mapped в канонический план",
        CoverageLevel::NextPriority => "следующий обязательный приоритет",
        CoverageLevel::Future => "следующий слой, но ещё не в ближайшем приоритете",
    }
}

fn resolve_benchmark<'a>(
    matrix: &'a BenchmarkMatrixFile,
    benchmark_query: &str,
) -> Option<(&'a str, &'a BenchmarkEntry)> {
    if let Some(entry) = matrix.benchmarks.get_key_value(benchmark_query) {
        return Some((entry.0.as_str(), entry.1));
    }

    let query = normalize_key(benchmark_query);
    matrix
        .benchmarks
        .iter()
        .find(|(code, benchmark)| {
            normalize_key(code) == query
                || normalize_key(&benchmark.display_name) == query
                || benchmark
                    .aliases
                    .iter()
                    .any(|alias| normalize_key(alias) == query)
        })
        .map(|(code, benchmark)| (code.as_str(), benchmark))
}

fn normalize_key(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut last_was_separator = true;

    for ch in value.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
            last_was_separator = false;
        } else if !last_was_separator {
            normalized.push('_');
            last_was_separator = true;
        }
    }

    while normalized.ends_with('_') {
        normalized.pop();
    }
    normalized
}

fn load_matrix(repo_root: &Path) -> Result<BenchmarkMatrixFile> {
    let path = matrix_path(repo_root);
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read benchmark matrix {}", path.display()))?;
    toml::from_str(&content).context("failed to parse benchmark matrix")
}

fn matrix_path(repo_root: &Path) -> PathBuf {
    repo_root.join("config/benchmark_matrix.toml")
}

#[cfg(test)]
mod tests {
    use super::{
        BenchmarkEntry, BenchmarkFamily, BenchmarkMatrixFile, BenchmarkSource, CoverageLevel,
        coverage_counts, coverage_counts_json, normalize_key, ordered_families, resolve_benchmark,
    };
    use std::collections::BTreeMap;

    #[test]
    fn coverage_counts_tracks_each_bucket() {
        let counts = coverage_counts(
            [
                CoverageLevel::Materialized,
                CoverageLevel::Partial,
                CoverageLevel::Mapped,
                CoverageLevel::NextPriority,
                CoverageLevel::Future,
                CoverageLevel::Mapped,
            ]
            .into_iter(),
        );
        assert_eq!(counts.total, 6);
        assert_eq!(counts.materialized, 1);
        assert_eq!(counts.partial, 1);
        assert_eq!(counts.mapped, 2);
        assert_eq!(counts.next_priority, 1);
        assert_eq!(counts.future, 1);
    }

    #[test]
    fn coverage_counts_json_renders_machine_readable_totals() {
        let counts = coverage_counts(
            [
                CoverageLevel::Materialized,
                CoverageLevel::Partial,
                CoverageLevel::Mapped,
                CoverageLevel::Future,
            ]
            .into_iter(),
        );
        let json = coverage_counts_json(&counts);
        assert_eq!(json["total"].as_u64(), Some(4));
        assert_eq!(json["materialized"].as_u64(), Some(1));
        assert_eq!(json["partial"].as_u64(), Some(1));
        assert_eq!(json["mapped"].as_u64(), Some(1));
        assert_eq!(json["future"].as_u64(), Some(1));
    }

    #[test]
    fn normalize_key_cleans_human_input() {
        assert_eq!(normalize_key("SWE-bench Verified"), "swe_bench_verified");
        assert_eq!(normalize_key("LiveMCPBench"), "livemcpbench");
        assert_eq!(normalize_key("previous:2"), "previous_2");
    }

    #[test]
    fn resolve_benchmark_accepts_aliases_and_display_names() {
        let matrix = sample_matrix();
        let (code, _) = resolve_benchmark(&matrix, "SWE-bench Verified").expect("display name");
        assert_eq!(code, "swe_bench_verified");

        let (code, _) = resolve_benchmark(&matrix, "live-mcpbench").expect("normalized alias");
        assert_eq!(code, "live_mcpbench");
    }

    #[test]
    fn families_follow_explicit_order() {
        let matrix = sample_matrix();
        let ordered = ordered_families(&matrix);
        assert_eq!(ordered[0].0.as_str(), "function_calling_tool_use");
        assert_eq!(ordered[1].0.as_str(), "coding_software_engineering");
    }

    fn sample_matrix() -> BenchmarkMatrixFile {
        let mut families = BTreeMap::new();
        families.insert(
            "coding_software_engineering".to_owned(),
            BenchmarkFamily {
                order: 2,
                display_name: "Coding".to_owned(),
                why_it_matters: "Coding family".to_owned(),
            },
        );
        families.insert(
            "function_calling_tool_use".to_owned(),
            BenchmarkFamily {
                order: 1,
                display_name: "Tool Use".to_owned(),
                why_it_matters: "Tool use family".to_owned(),
            },
        );

        let mut benchmarks = BTreeMap::new();
        benchmarks.insert(
            "live_mcpbench".to_owned(),
            BenchmarkEntry {
                order: 1,
                display_name: "LiveMCPBench".to_owned(),
                family: "function_calling_tool_use".to_owned(),
                coverage_level: CoverageLevel::NextPriority,
                summary: "MCP benchmark".to_owned(),
                reference_url: "https://example.com".to_owned(),
                aliases: vec!["live-mcpbench".to_owned()],
                why_relevant: vec![],
                amai_focus: vec![],
                current_coverage: vec![],
                current_gaps: vec![],
                proof_commands: vec![],
                next_step: "next".to_owned(),
            },
        );
        benchmarks.insert(
            "swe_bench_verified".to_owned(),
            BenchmarkEntry {
                order: 2,
                display_name: "SWE-bench Verified".to_owned(),
                family: "coding_software_engineering".to_owned(),
                coverage_level: CoverageLevel::Mapped,
                summary: "Coding benchmark".to_owned(),
                reference_url: "https://example.com".to_owned(),
                aliases: vec!["swebench-verified".to_owned()],
                why_relevant: vec![],
                amai_focus: vec![],
                current_coverage: vec![],
                current_gaps: vec![],
                proof_commands: vec![],
                next_step: "next".to_owned(),
            },
        );

        BenchmarkMatrixFile {
            source: BenchmarkSource {
                display_name: "Compendium".to_owned(),
                url: "https://example.com".to_owned(),
            },
            families,
            benchmarks,
        }
    }
}
