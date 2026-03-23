use crate::config::AppConfig;
use crate::edge_cache;
use crate::language::detect;
use crate::postgres::{
    self, ChunkRecord, DocumentRecord, NamespaceRecord, ProjectRecord, SymbolRecord,
};
use crate::qdrant::{self, VectorPoint};
use crate::syntax;
use anyhow::{Context, Result, anyhow};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use ignore::WalkBuilder;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio_postgres::Client;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct IndexingStats {
    pub files_indexed: usize,
    pub ast_eligible_files: usize,
    pub files_with_ast: usize,
    pub files_with_lexical_fallback: usize,
    pub files_without_ast_support: usize,
    pub symbols_written: usize,
    pub chunks_written: usize,
    pub vector_points_written: usize,
    pub total_bytes: i64,
    pub elapsed_ms: u128,
    pub files_per_min: f64,
    pub parser_coverage_ratio: f64,
    pub language_breakdown: Value,
}

#[derive(Debug, Clone)]
struct AnalyzedFile {
    absolute_path: PathBuf,
    relative_path: String,
    language: Option<String>,
    source_kind: String,
    ast_eligible: bool,
    file_sha256: String,
    line_count: i32,
    byte_count: i64,
    content: String,
    analysis_mode: String,
    metrics: Value,
    structure: Value,
    imports: Value,
    exports: Value,
    diagnostics: Value,
    metadata: Value,
    symbols: Vec<SymbolRecord>,
    chunk_blueprints: Vec<ChunkBlueprint>,
}

#[derive(Debug, Clone)]
struct ChunkBlueprint {
    chunk_index: i32,
    total_chunks: i32,
    start_line: i32,
    end_line: i32,
    start_byte: i32,
    end_byte: i32,
    content: String,
    metadata: Value,
}

type TreeSitterAnalysis = (
    Value,
    Value,
    Value,
    Value,
    Vec<SymbolRecord>,
    Vec<ChunkBlueprint>,
    Value,
    String,
);

pub async fn index_project(
    cfg: &AppConfig,
    db: &mut Client,
    args: &crate::cli::IndexProjectArgs,
) -> Result<IndexingStats> {
    let started = Instant::now();
    let project = postgres::get_project_by_code(db, &args.code).await?;
    let namespace = postgres::ensure_namespace(
        db,
        project.project_id,
        &args.namespace,
        Some(&args.namespace),
        &cfg.default_retrieval_mode,
    )
    .await?;
    postgres::delete_namespace_documents(db, namespace.namespace_id).await?;

    let git_commit_sha = resolve_git_commit(&project.repo_root).await.ok();
    let files = collect_files(&args.path, args.limit_files, args.paths_file.as_deref())?;
    let qdrant_client = if args.skip_embeddings {
        None
    } else {
        Some(qdrant::connect(cfg)?)
    };
    let mut embedder = if args.skip_embeddings {
        None
    } else {
        Some(build_code_embedder(cfg)?)
    };

    let edge_cache_path = edge_cache::ensure(&cfg.edge_cache_path)?;
    let mut files_indexed = 0usize;
    let mut ast_eligible_files = 0usize;
    let mut files_with_ast = 0usize;
    let mut files_with_lexical_fallback = 0usize;
    let mut files_without_ast_support = 0usize;
    let mut symbols_written = 0usize;
    let mut chunks_written = 0usize;
    let mut vector_points_written = 0usize;
    let mut total_bytes = 0_i64;
    let mut language_breakdown = BTreeMap::<String, usize>::new();

    for file in files {
        let analyzed = analyze_file(cfg, &project, &file, git_commit_sha.as_deref())?;
        let document_id = Uuid::new_v4();
        files_indexed += 1;
        total_bytes += analyzed.byte_count;
        *language_breakdown
            .entry(
                analyzed
                    .language
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string()),
            )
            .or_default() += 1;
        if analyzed.ast_eligible {
            ast_eligible_files += 1;
            if analyzed.analysis_mode == "ast" {
                files_with_ast += 1;
            } else {
                files_with_lexical_fallback += 1;
            }
        } else {
            files_without_ast_support += 1;
        }

        let chunk_records = if let (Some(qdrant_client), Some(embedder)) =
            (qdrant_client.as_ref(), embedder.as_mut())
        {
            let points = embed_chunks(cfg, document_id, &project, &namespace, &analyzed, embedder)?;
            vector_points_written += points.len();
            qdrant::replace_document_points(
                qdrant_client,
                &cfg.qdrant_alias_code,
                document_id,
                &points,
            )
            .await?;
            to_chunk_records(&cfg.qdrant_alias_code, points, &analyzed.chunk_blueprints)
        } else {
            to_chunk_records_without_vectors(&analyzed.chunk_blueprints)
        };
        symbols_written += analyzed.symbols.len();
        chunks_written += chunk_records.len();

        let document_record = DocumentRecord {
            project_id: project.project_id,
            namespace_id: namespace.namespace_id,
            repo_root: project.repo_root.clone(),
            absolute_path: analyzed.absolute_path.display().to_string(),
            relative_path: analyzed.relative_path.clone(),
            language: analyzed.language.clone(),
            source_kind: analyzed.source_kind.clone(),
            git_commit_sha: git_commit_sha.clone(),
            file_sha256: analyzed.file_sha256.clone(),
            line_count: analyzed.line_count,
            byte_count: analyzed.byte_count,
            content: analyzed.content.clone(),
            metrics: analyzed.metrics.clone(),
            structure: analyzed.structure.clone(),
            imports: analyzed.imports.clone(),
            exports: analyzed.exports.clone(),
            diagnostics: analyzed.diagnostics.clone(),
            metadata: analyzed.metadata.clone(),
        };

        postgres::replace_document_index(db, &document_record, &analyzed.symbols, &chunk_records)
            .await?;

        edge_cache::upsert_document(
            &edge_cache_path,
            &format!(
                "{}::{}::{}",
                project.code, namespace.code, analyzed.relative_path
            ),
            &project.code,
            &namespace.code,
            &analyzed.relative_path,
            &analyzed.content,
        )?;
        tracing::info!(path = %analyzed.relative_path, "indexed file");
    }

    postgres::touch_project_updated_at(db, project.project_id).await?;

    let elapsed_ms = started.elapsed().as_millis();
    let files_per_min = if elapsed_ms == 0 {
        files_indexed as f64 * 60_000.0
    } else {
        files_indexed as f64 * 60_000.0 / elapsed_ms as f64
    };
    let parser_coverage_ratio =
        compute_parser_coverage_ratio(files_with_ast, files_with_lexical_fallback);

    Ok(IndexingStats {
        files_indexed,
        ast_eligible_files,
        files_with_ast,
        files_with_lexical_fallback,
        files_without_ast_support,
        symbols_written,
        chunks_written,
        vector_points_written,
        total_bytes,
        elapsed_ms,
        files_per_min,
        parser_coverage_ratio,
        language_breakdown: json!(language_breakdown),
    })
}

fn build_code_embedder(cfg: &AppConfig) -> Result<TextEmbedding> {
    let model = match cfg.code_embed_model.as_str() {
        "jina_base_code" => EmbeddingModel::JinaEmbeddingsV2BaseCode,
        "multilingual_e5_small" => EmbeddingModel::MultilingualE5Small,
        "multilingual_e5_base" => EmbeddingModel::MultilingualE5Base,
        other => return Err(anyhow!("unsupported code embedding model: {other}")),
    };
    TextEmbedding::try_new(InitOptions::new(model).with_show_download_progress(false))
}

fn collect_files(
    root: &Path,
    limit: Option<usize>,
    paths_file: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    if let Some(paths_file) = paths_file {
        return collect_explicit_files(root, limit, paths_file);
    }
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(true)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true);
    let files = builder
        .build()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .map(|entry| entry.into_path())
        .filter(|path| should_index_path(root, path))
        .filter(|path| detect(path).is_some())
        .take(limit.unwrap_or(usize::MAX))
        .collect::<Vec<_>>();
    Ok(files)
}

fn collect_explicit_files(
    root: &Path,
    limit: Option<usize>,
    paths_file: &Path,
) -> Result<Vec<PathBuf>> {
    let content = fs::read_to_string(paths_file)
        .with_context(|| format!("failed to read paths file {}", paths_file.display()))?;
    let mut unique_paths = BTreeSet::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        unique_paths.insert(line.to_string());
    }

    let mut files = Vec::with_capacity(unique_paths.len());
    for relative_path in unique_paths {
        let candidate = root.join(&relative_path);
        if !candidate.is_file() {
            return Err(anyhow!(
                "paths file {} references missing file {}",
                paths_file.display(),
                candidate.display()
            ));
        }
        if !should_index_path(root, &candidate) {
            return Err(anyhow!(
                "paths file {} references non-indexable path {}",
                paths_file.display(),
                candidate.display()
            ));
        }
        if detect(&candidate).is_none() {
            return Err(anyhow!(
                "paths file {} references unsupported file {}",
                paths_file.display(),
                candidate.display()
            ));
        }
        files.push(candidate);
    }

    if let Some(limit) = limit {
        files.truncate(limit);
    }
    Ok(files)
}

fn should_index_path(root: &Path, path: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    !relative.components().any(|component| {
        let segment = component.as_os_str().to_string_lossy();
        matches!(
            segment.as_ref(),
            ".git"
                | "target"
                | "state"
                | "tmp"
                | ".fastembed_cache"
                | ".venv"
                | "node_modules"
                | ".next"
                | "dist"
                | "build"
                | "coverage"
                | "venv"
                | "site-packages"
                | "__pycache__"
        )
    })
}

fn compute_parser_coverage_ratio(files_with_ast: usize, files_with_lexical_fallback: usize) -> f64 {
    let ast_eligible_files = files_with_ast + files_with_lexical_fallback;
    if ast_eligible_files == 0 {
        return 1.0;
    }
    files_with_ast as f64 / ast_eligible_files as f64
}

fn analyze_file(
    cfg: &AppConfig,
    project: &ProjectRecord,
    absolute_path: &Path,
    git_commit_sha: Option<&str>,
) -> Result<AnalyzedFile> {
    let relative_path = absolute_path
        .strip_prefix(&project.repo_root)
        .unwrap_or(absolute_path)
        .display()
        .to_string();
    let bytes = fs::read(absolute_path)
        .with_context(|| format!("failed to read {}", absolute_path.display()))?;
    let content = String::from_utf8_lossy(&bytes).into_owned();
    let descriptor = detect(absolute_path).ok_or_else(|| anyhow!("unsupported file"))?;
    let file_sha256 = hex_sha256(&bytes);
    let line_count = content.lines().count() as i32;
    let byte_count = bytes.len() as i64;

    let ast_eligible = descriptor
        .parser_language
        .map(syntax::supports)
        .unwrap_or(false);
    let (
        structure,
        imports,
        exports,
        diagnostics,
        symbols,
        chunk_blueprints,
        metrics,
        analysis_mode,
    ) = if let Some(language) = descriptor.parser_language {
        if ast_eligible {
            match parse_with_tree_sitter(cfg, language, &content) {
                Ok(parsed) => parsed,
                Err(error) => {
                    tracing::warn!(
                        path = %absolute_path.display(),
                        language,
                        error = %error,
                        "tree-sitter analysis unavailable, falling back to lexical-only analysis"
                    );
                    fallback_analysis(cfg, &content)
                }
            }
        } else {
            fallback_analysis(cfg, &content)
        }
    } else {
        fallback_analysis(cfg, &content)
    };

    Ok(AnalyzedFile {
        absolute_path: absolute_path.to_path_buf(),
        relative_path,
        language: descriptor.parser_language.map(ToOwned::to_owned),
        source_kind: descriptor.source_kind.to_string(),
        ast_eligible,
        file_sha256,
        line_count,
        byte_count,
        content,
        analysis_mode: analysis_mode.clone(),
        metrics,
        structure,
        imports,
        exports,
        diagnostics,
        metadata: json!({
            "git_commit_sha": git_commit_sha,
            "source_kind": descriptor.source_kind,
            "parser_language": descriptor.parser_language,
            "ast_eligible": ast_eligible,
            "analysis_mode": analysis_mode
        }),
        symbols,
        chunk_blueprints,
    })
}

fn parse_with_tree_sitter(
    cfg: &AppConfig,
    language: &str,
    content: &str,
) -> Result<TreeSitterAnalysis> {
    let analysis = syntax::analyze(cfg, language, content)?;
    let chunk_blueprints = if analysis.chunks.is_empty() {
        fallback_chunks(cfg, content)
    } else {
        analysis
            .chunks
            .into_iter()
            .map(|chunk| ChunkBlueprint {
                chunk_index: chunk.chunk_index,
                total_chunks: chunk.total_chunks,
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                start_byte: chunk.start_byte,
                end_byte: chunk.end_byte,
                content: chunk.content,
                metadata: chunk.metadata,
            })
            .collect::<Vec<_>>()
    };

    Ok((
        analysis.structure,
        analysis.imports,
        analysis.exports,
        analysis.diagnostics,
        analysis.symbols,
        chunk_blueprints,
        analysis.metrics,
        "ast".to_string(),
    ))
}

fn fallback_analysis(
    cfg: &AppConfig,
    content: &str,
) -> (
    Value,
    Value,
    Value,
    Value,
    Vec<SymbolRecord>,
    Vec<ChunkBlueprint>,
    Value,
    String,
) {
    let total_lines = content.lines().count();
    let blank_lines = content
        .lines()
        .filter(|line| line.trim().is_empty())
        .count();
    let metrics = json!({
        "total_lines": total_lines,
        "code_lines": total_lines.saturating_sub(blank_lines),
        "comment_lines": 0,
        "blank_lines": blank_lines,
        "total_bytes": content.len(),
        "node_count": 0,
        "error_count": 0,
        "max_depth": 0
    });
    (
        json!([]),
        json!([]),
        json!([]),
        json!([]),
        Vec::new(),
        fallback_chunks(cfg, content),
        metrics,
        "lexical_fallback".to_string(),
    )
}

fn fallback_chunks(cfg: &AppConfig, content: &str) -> Vec<ChunkBlueprint> {
    let lines = content.lines().map(ToOwned::to_owned).collect::<Vec<_>>();
    if lines.is_empty() {
        return Vec::new();
    }
    let mut start = 0usize;
    let mut chunks = Vec::new();
    let step = cfg
        .fallback_chunk_lines
        .saturating_sub(cfg.fallback_chunk_overlap_lines)
        .max(1);
    while start < lines.len() {
        let end = (start + cfg.fallback_chunk_lines).min(lines.len());
        let chunk_content = lines[start..end].join("\n");
        let start_byte = lines[..start]
            .iter()
            .map(|line| line.len() + 1)
            .sum::<usize>();
        let end_byte = start_byte + chunk_content.len();
        chunks.push(ChunkBlueprint {
            chunk_index: chunks.len() as i32,
            total_chunks: 0,
            start_line: start as i32,
            end_line: end as i32,
            start_byte: start_byte as i32,
            end_byte: end_byte as i32,
            content: chunk_content,
            metadata: json!({
                "language": Value::Null,
                "node_types": [],
                "context_path": [],
                "symbols_defined": [],
                "has_error_nodes": false
            }),
        });
        if end == lines.len() {
            break;
        }
        start += step;
    }
    let total_chunks = chunks.len() as i32;
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, mut chunk)| {
            chunk.chunk_index = index as i32;
            chunk.total_chunks = total_chunks;
            chunk
        })
        .collect()
}

fn embed_chunks(
    cfg: &AppConfig,
    document_id: Uuid,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    analyzed: &AnalyzedFile,
    embedder: &mut TextEmbedding,
) -> Result<Vec<VectorPoint>> {
    let texts = analyzed
        .chunk_blueprints
        .iter()
        .map(|chunk| chunk.content.clone())
        .collect::<Vec<_>>();
    let vectors = embedder.embed(&texts, Some(16))?;
    let points = analyzed
        .chunk_blueprints
        .iter()
        .zip(vectors.into_iter())
        .map(|(chunk, vector)| {
            if vector.len() as u64 != cfg.qdrant_code_dim {
                return Err(anyhow!(
                    "embedding size mismatch: expected {}, got {}",
                    cfg.qdrant_code_dim,
                    vector.len()
                ));
            }
            Ok(VectorPoint {
                point_id: Uuid::new_v4(),
                vector,
                payload: json!({
                    "project_id": project.project_id,
                    "project_code": project.code,
                    "namespace_id": namespace.namespace_id,
                    "namespace_code": namespace.code,
                    "repo_root": project.repo_root,
                    "relative_path": analyzed.relative_path,
                    "absolute_path": analyzed.absolute_path.display().to_string(),
                    "language": analyzed.language,
                    "source_kind": analyzed.source_kind,
                    "document_id": document_id,
                    "chunk_index": chunk.chunk_index,
                    "total_chunks": chunk.total_chunks,
                    "start_line": chunk.start_line,
                    "end_line": chunk.end_line,
                    "trust_level": "local_repo"
                }),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(points)
}

fn to_chunk_records(
    collection_alias: &str,
    points: Vec<VectorPoint>,
    blueprints: &[ChunkBlueprint],
) -> Vec<ChunkRecord> {
    blueprints
        .iter()
        .zip(points)
        .map(|(chunk, point)| ChunkRecord {
            chunk_id: Uuid::new_v4(),
            qdrant_point_id: Some(point.point_id),
            qdrant_collection_alias: Some(collection_alias.to_string()),
            chunk_index: chunk.chunk_index,
            total_chunks: chunk.total_chunks,
            start_line: chunk.start_line,
            end_line: chunk.end_line,
            start_byte: chunk.start_byte,
            end_byte: chunk.end_byte,
            content: chunk.content.clone(),
            metadata: chunk.metadata.clone(),
        })
        .collect()
}

fn to_chunk_records_without_vectors(blueprints: &[ChunkBlueprint]) -> Vec<ChunkRecord> {
    blueprints
        .iter()
        .map(|chunk| ChunkRecord {
            chunk_id: Uuid::new_v4(),
            qdrant_point_id: None,
            qdrant_collection_alias: None,
            chunk_index: chunk.chunk_index,
            total_chunks: chunk.total_chunks,
            start_line: chunk.start_line,
            end_line: chunk.end_line,
            start_byte: chunk.start_byte,
            end_byte: chunk.end_byte,
            content: chunk.content.clone(),
            metadata: chunk.metadata.clone(),
        })
        .collect()
}

async fn resolve_git_commit(repo_root: &str) -> Result<String> {
    let output = tokio::process::Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .await?;
    if !output.status.success() {
        return Err(anyhow!("git rev-parse failed for {repo_root}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::{collect_explicit_files, compute_parser_coverage_ratio, should_index_path};
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn skips_runtime_and_generated_directories() {
        let root = Path::new("/repo");
        assert!(!should_index_path(
            root,
            Path::new("/repo/state/external-benchmarks/upstream/file.py")
        ));
        assert!(!should_index_path(
            root,
            Path::new("/repo/target/debug/app")
        ));
        assert!(!should_index_path(root, Path::new("/repo/tmp/cache.txt")));
        assert!(!should_index_path(
            root,
            Path::new("/repo/.venv/lib/python3.12/site-packages/pydantic/__init__.py")
        ));
        assert!(!should_index_path(
            root,
            Path::new("/repo/venv/lib/python3.12/site-packages/pydantic/__init__.py")
        ));
        assert!(!should_index_path(
            root,
            Path::new("/repo/src/__pycache__/module.cpython-312.pyc")
        ));
    }

    #[test]
    fn keeps_canonical_repo_sources() {
        let root = Path::new("/repo");
        assert!(should_index_path(root, Path::new("/repo/src/postgres.rs")));
        assert!(should_index_path(root, Path::new("/repo/README.md")));
        assert!(should_index_path(
            root,
            Path::new("/repo/fixtures/project_alpha/src/lib.rs")
        ));
    }

    #[test]
    fn parser_coverage_ignores_non_ast_capable_files() {
        assert_eq!(compute_parser_coverage_ratio(55, 0), 1.0);
    }

    #[test]
    fn parser_coverage_penalizes_real_ast_fallbacks() {
        assert!((compute_parser_coverage_ratio(3, 2) - 0.6).abs() < f64::EPSILON);
    }

    #[test]
    fn explicit_paths_file_is_deterministic_and_ignores_comments() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("amai-indexer-explicit-paths-{unique}"));
        fs::create_dir_all(root.join("src")).expect("create src");
        fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").expect("write lib");
        fs::write(root.join("Cargo.toml"), "[package]\nname = \"demo\"\n").expect("write cargo");
        let paths_file = root.join("paths.txt");
        fs::write(
            &paths_file,
            "# comment\nCargo.toml\nsrc/lib.rs\nCargo.toml\n\n",
        )
        .expect("write paths");

        let files = collect_explicit_files(&root, None, &paths_file).expect("collect");
        assert_eq!(
            files,
            vec![root.join("Cargo.toml"), root.join("src/lib.rs")]
        );

        fs::remove_dir_all(&root).expect("cleanup temp root");
    }

    #[test]
    fn explicit_paths_file_rejects_missing_entries() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("amai-indexer-explicit-missing-{unique}"));
        fs::create_dir_all(root.join("src")).expect("create src");
        fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n").expect("write lib");
        let paths_file = root.join("paths.txt");
        fs::write(&paths_file, "src/lib.rs\nmissing.rs\n").expect("write paths");

        let error = collect_explicit_files(&root, None, &paths_file).expect_err("must fail");
        assert!(error.to_string().contains("references missing file"));

        fs::remove_dir_all(&root).expect("cleanup temp root");
    }
}
