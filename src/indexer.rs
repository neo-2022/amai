use crate::config::AppConfig;
use crate::edge_cache;
use crate::language::detect;
use crate::postgres::{
    self, ChunkRecord, DocumentRecord, NamespaceRecord, ProjectRecord,
    ReplaceDocumentIndexErrorPhase, SymbolRecord,
};
use crate::qdrant::{self, VectorPoint};
use crate::syntax;
use anyhow::{Context, Error, Result, anyhow};
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
    Value,
    Vec<SymbolRecord>,
    Vec<ChunkBlueprint>,
    Value,
    String,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QdrantPostgresFailureMode {
    BeforeQdrantUpdate,
    ExistingDocumentCommitOutcomeUnknown,
    ExistingDocumentCompensated,
    ExistingDocumentInconsistentState,
    NewDocumentCommitOutcomeUnknown,
    NewDocumentCompensated,
    NewDocumentInconsistentState,
}

impl QdrantPostgresFailureMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::BeforeQdrantUpdate => "before_qdrant_update",
            Self::ExistingDocumentCommitOutcomeUnknown => {
                "existing_document_commit_outcome_unknown"
            }
            Self::ExistingDocumentCompensated => "existing_document_compensated",
            Self::ExistingDocumentInconsistentState => "existing_document_inconsistent_state",
            Self::NewDocumentCommitOutcomeUnknown => "new_document_commit_outcome_unknown",
            Self::NewDocumentCompensated => "new_document_compensated",
            Self::NewDocumentInconsistentState => "new_document_inconsistent_state",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QdrantPostgresConsistencyState {
    PostgresFailureBeforeQdrantUpdate,
    CrossStoreConsistencyRestoredByCompensation,
    CrossStoreConsistencyUnknownCommitOutcome,
    CrossStoreInconsistentAfterCompensationFailure,
}

impl QdrantPostgresConsistencyState {
    fn as_str(self) -> &'static str {
        match self {
            Self::PostgresFailureBeforeQdrantUpdate => "postgres_failure_before_qdrant_update",
            Self::CrossStoreConsistencyRestoredByCompensation => {
                "cross_store_consistency_restored_by_compensation"
            }
            Self::CrossStoreConsistencyUnknownCommitOutcome => {
                "cross_store_consistency_unknown_commit_outcome"
            }
            Self::CrossStoreInconsistentAfterCompensationFailure => {
                "cross_store_inconsistent_after_compensation_failure"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QdrantPostgresRequiredAction {
    RetryOrInvestigatePostgresBeforeRetryingQdrantMutation,
    NoFurtherCrossStoreRecoveryRequired,
    ManualCrossStoreInvestigationRequired,
}

impl QdrantPostgresRequiredAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::RetryOrInvestigatePostgresBeforeRetryingQdrantMutation => {
                "retry_or_investigate_postgres_before_retrying_qdrant_mutation"
            }
            Self::NoFurtherCrossStoreRecoveryRequired => "no_further_cross_store_recovery_required",
            Self::ManualCrossStoreInvestigationRequired => {
                "manual_cross_store_investigation_required"
            }
        }
    }
}

struct QdrantPostgresFailureVerdict {
    mode: QdrantPostgresFailureMode,
    failure_phase: ReplaceDocumentIndexErrorPhase,
    failure_sqlstate: Option<String>,
    compensation_attempted: bool,
    compensation_succeeded: Option<bool>,
    consistency_state: QdrantPostgresConsistencyState,
    required_action: QdrantPostgresRequiredAction,
    remediation_bundle_path: Option<PathBuf>,
    error: Error,
}

impl QdrantPostgresFailureVerdict {
    fn compensation_succeeded_surface(&self) -> &'static str {
        match self.compensation_succeeded {
            Some(true) => "true",
            Some(false) => "false",
            None => "none",
        }
    }
}

fn render_qdrant_postgres_failure_verdict_log(
    relative_path: &str,
    document_id: Uuid,
    verdict: &QdrantPostgresFailureVerdict,
) -> String {
    format!(
        "relative_path={} document_id={} failure_mode={} failure_phase={} failure_sqlstate={} compensation_attempted={} compensation_succeeded={} consistency_state={} required_action={} remediation_bundle_path={}",
        relative_path,
        document_id,
        verdict.mode.as_str(),
        verdict.failure_phase.as_str(),
        verdict.failure_sqlstate.as_deref().unwrap_or("none"),
        verdict.compensation_attempted,
        verdict.compensation_succeeded_surface(),
        verdict.consistency_state.as_str(),
        verdict.required_action.as_str(),
        verdict
            .remediation_bundle_path
            .as_deref()
            .and_then(|path| path.to_str())
            .unwrap_or("none"),
    )
}

fn emit_qdrant_postgres_failure_verdict_log(
    relative_path: &str,
    document_id: Uuid,
    verdict: &QdrantPostgresFailureVerdict,
) {
    postgres::observability_profile_log(
        "index_project.qdrant_postgres_failure_verdict",
        0,
        &render_qdrant_postgres_failure_verdict_log(relative_path, document_id, verdict),
    );
}

pub(crate) const QDRANT_POSTGRES_REMEDIATION_BUNDLE_ARTIFACT_VERSION: &str =
    "qdrant_postgres_remediation_bundle_v1";

pub(crate) fn qdrant_postgres_remediation_bundle_dir(repo_root: &Path) -> PathBuf {
    if let Some(path) = std::env::var_os("AMAI_QDRANT_POSTGRES_REMEDIATION_DIR") {
        PathBuf::from(path)
    } else {
        repo_root
            .join("state")
            .join("incidents")
            .join("qdrant-postgres-remediation")
    }
}

fn sanitize_relative_path_for_filename(relative_path: &str) -> String {
    relative_path
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '_' | '-' => ch,
            _ => '_',
        })
        .collect()
}

fn write_qdrant_postgres_remediation_bundle(
    repo_root: &Path,
    relative_path: &str,
    document_id: Uuid,
    had_existing_document: bool,
    verdict: &QdrantPostgresFailureVerdict,
) -> Result<PathBuf> {
    let bundle_dir = qdrant_postgres_remediation_bundle_dir(repo_root);
    fs::create_dir_all(&bundle_dir)
        .with_context(|| format!("failed to create {}", bundle_dir.display()))?;
    let created_at_epoch_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_millis();
    let bundle_id = Uuid::new_v4();
    let bundle_path = bundle_dir.join(format!(
        "{}__{}__{}.json",
        created_at_epoch_ms,
        sanitize_relative_path_for_filename(relative_path),
        document_id
    ));
    let temp_bundle_path = bundle_dir.join(format!(
        ".{}.tmp-{}",
        bundle_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("remediation-bundle"),
        Uuid::new_v4()
    ));
    let payload = json!({
        "artifact_version": QDRANT_POSTGRES_REMEDIATION_BUNDLE_ARTIFACT_VERSION,
        "bundle_id": bundle_id,
        "created_at_epoch_ms": created_at_epoch_ms,
        "workspace_repo_root": repo_root,
        "incident_kind": "qdrant_postgres_cross_store_manual_recovery",
        "relative_path": relative_path,
        "document_id": document_id,
        "had_existing_document": had_existing_document,
        "failure_mode": verdict.mode.as_str(),
        "failure_phase": verdict.failure_phase.as_str(),
        "failure_sqlstate": verdict.failure_sqlstate,
        "compensation_attempted": verdict.compensation_attempted,
        "compensation_succeeded": verdict.compensation_succeeded,
        "consistency_state": verdict.consistency_state.as_str(),
        "required_action": verdict.required_action.as_str(),
        "operator_summary": "Cross-store state requires explicit operator investigation before any retry or cleanup.",
        "operator_checklist": [
            "Inspect this document_id in PostgreSQL documents/chunks/symbols and compare it to Qdrant points for the same document_id.",
            "Confirm whether the latest authoritative content should come from the pre-failure Postgres state or the already-mutated Qdrant state.",
            "Reconcile the losing side explicitly; do not rerun generic indexing until the cross-store state is understood.",
            "Capture the final reconciliation decision in continuity or incident notes before closing the bundle."
        ],
        "observability_stage": "index_project.qdrant_postgres_failure_verdict"
    });
    fs::write(
        &temp_bundle_path,
        serde_json::to_string_pretty(&payload).expect("serialize remediation bundle"),
    )
    .with_context(|| format!("failed to write {}", temp_bundle_path.display()))?;
    if let Err(error) = fs::rename(&temp_bundle_path, &bundle_path) {
        let _ = fs::remove_file(&temp_bundle_path);
        return Err(error).with_context(|| {
            format!(
                "failed to atomically publish remediation bundle {} -> {}",
                temp_bundle_path.display(),
                bundle_path.display()
            )
        });
    }
    postgres::observability_profile_log(
        "index_project.qdrant_postgres_manual_recovery_bundle",
        0,
        &format!(
            "relative_path={} document_id={} bundle_id={} bundle_path={}",
            relative_path,
            document_id,
            bundle_id,
            bundle_path.display()
        ),
    );
    Ok(bundle_path)
}

fn attach_context_to_error(error: &mut Error, context: String) {
    let previous = std::mem::replace(error, anyhow!("qdrant/postgres remediation placeholder"));
    *error = previous.context(context);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QdrantCompensationPolicy {
    AttemptCompensation,
    SkipCompensationCommitOutcomeUnknown,
}

struct IndexProjectFileOutcome {
    files_indexed_delta: usize,
    ast_eligible_files_delta: usize,
    files_with_ast_delta: usize,
    files_with_lexical_fallback_delta: usize,
    files_without_ast_support_delta: usize,
    symbols_written_delta: usize,
    chunks_written_delta: usize,
    vector_points_written_delta: usize,
    total_bytes_delta: i64,
    language_key: String,
}

struct IndexProjectLockedFileCtx {
    project: ProjectRecord,
    namespace: NamespaceRecord,
    analyzed: AnalyzedFile,
    git_commit_sha: Option<String>,
    qdrant_client: Option<qdrant_client::Qdrant>,
    embedded_vectors: Option<Vec<Vec<f32>>>,
    edge_cache_path: PathBuf,
    qdrant_alias_code: String,
    skip_edge_cache_writes: bool,
}

pub async fn index_project(
    cfg: &AppConfig,
    db: &mut Client,
    args: &crate::cli::IndexProjectArgs,
) -> Result<IndexingStats> {
    let started = Instant::now();
    let index_root = canonicalize_existing_dir(&args.path)?;
    let project = postgres::get_project_by_code(db, &args.code).await?;
    let namespace = postgres::ensure_namespace(
        db,
        project.project_id,
        &args.namespace,
        Some(&args.namespace),
        &cfg.default_retrieval_mode,
    )
    .await?;
    let git_commit_sha = resolve_git_commit(&project.repo_root).await.ok();
    let files = collect_files(&index_root, args.limit_files, args.paths_file.as_deref())?;
    if args.preserve_namespace_documents {
        if files.is_empty() {
            postgres::delete_namespace_documents(db, namespace.namespace_id).await?;
        } else {
            let keep_paths = files
                .iter()
                .map(|path| {
                    path.strip_prefix(&project.repo_root)
                        .unwrap_or(path)
                        .display()
                        .to_string()
                })
                .collect::<Vec<_>>();
            postgres::delete_namespace_documents_except_paths(
                db,
                namespace.namespace_id,
                &keep_paths,
            )
            .await?;
        }
    } else {
        postgres::delete_namespace_documents(db, namespace.namespace_id).await?;
    }
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
    let skip_edge_cache_writes = should_skip_edge_cache_documents(args, &files);
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
        let _relative_path = file
            .strip_prefix(&project.repo_root)
            .unwrap_or(&file)
            .display()
            .to_string();
        let analyze_started = Instant::now();
        let analyzed = analyze_file(cfg, &project, &file, git_commit_sha.as_deref())?;
        postgres::observability_profile_log(
            "index_project.analyze_file",
            analyze_started.elapsed().as_millis(),
            &format!(
                "relative_path={} source_kind={} analysis_mode={} byte_count={} symbols={} chunks={}",
                analyzed.relative_path,
                analyzed.source_kind,
                analyzed.analysis_mode,
                analyzed.byte_count,
                analyzed.symbols.len(),
                analyzed.chunk_blueprints.len()
            ),
        );
        let advisory_lock_key =
            document_index_advisory_lock_key(namespace.namespace_id, &analyzed.relative_path);
        let advisory_lock_started = Instant::now();
        let qdrant_client_owned = qdrant_client.clone();
        let embedded_vectors = if let Some(embedder) = embedder.as_mut() {
            Some(embed_chunk_vectors(cfg, &analyzed, embedder)?)
        } else {
            None
        };
        let advisory_lock_relative_path = analyzed.relative_path.clone();
        let project_for_lock = project.clone();
        let namespace_for_lock = namespace.clone();
        let analyzed_for_lock = analyzed.clone();
        let git_commit_sha_for_lock = git_commit_sha.clone();
        let edge_cache_path_for_lock = edge_cache_path.clone();
        let qdrant_alias_code_for_lock = cfg.qdrant_alias_code.clone();
        let file_outcome = postgres::with_postgres_advisory_lock_mut(
            db,
            advisory_lock_key,
            format!(
                "failed to acquire document advisory lock for {}",
                analyzed.relative_path
            ),
            format!(
                "failed to release document advisory lock for {}",
                analyzed.relative_path
            ),
            move |db| {
                postgres::observability_profile_log(
                    "index_project.document_update_lock",
                    advisory_lock_started.elapsed().as_millis(),
                    &format!(
                        "relative_path={} advisory_lock_key={}",
                        advisory_lock_relative_path, advisory_lock_key
                    ),
                );
                Box::pin(index_project_file_under_lock(
                    db,
                    IndexProjectLockedFileCtx {
                        project: project_for_lock.clone(),
                        namespace: namespace_for_lock.clone(),
                        analyzed: analyzed_for_lock.clone(),
                        git_commit_sha: git_commit_sha_for_lock.clone(),
                        qdrant_client: qdrant_client_owned.clone(),
                        embedded_vectors: embedded_vectors.clone(),
                        edge_cache_path: edge_cache_path_for_lock.clone(),
                        qdrant_alias_code: qdrant_alias_code_for_lock.clone(),
                        skip_edge_cache_writes,
                    },
                ))
            },
        )
        .await?;
        files_indexed += file_outcome.files_indexed_delta;
        ast_eligible_files += file_outcome.ast_eligible_files_delta;
        files_with_ast += file_outcome.files_with_ast_delta;
        files_with_lexical_fallback += file_outcome.files_with_lexical_fallback_delta;
        files_without_ast_support += file_outcome.files_without_ast_support_delta;
        symbols_written += file_outcome.symbols_written_delta;
        chunks_written += file_outcome.chunks_written_delta;
        vector_points_written += file_outcome.vector_points_written_delta;
        total_bytes += file_outcome.total_bytes_delta;
        *language_breakdown
            .entry(file_outcome.language_key)
            .or_default() += 1;
    }

    let touch_project_started = Instant::now();
    postgres::touch_project_updated_at(db, project.project_id).await?;
    postgres::observability_profile_log(
        "index_project.touch_project_updated_at",
        touch_project_started.elapsed().as_millis(),
        &format!(
            "project_code={} namespace_code={}",
            project.code, namespace.code
        ),
    );

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

async fn index_project_file_under_lock(
    db: &mut Client,
    ctx: IndexProjectLockedFileCtx,
) -> Result<IndexProjectFileOutcome> {
    let existing_document_id = postgres::get_document_id_for_namespace_relative_path(
        db,
        ctx.namespace.namespace_id,
        &ctx.analyzed.relative_path,
    )
    .await?;
    let had_existing_document = existing_document_id.is_some();
    let document_id = existing_document_id.unwrap_or_else(Uuid::new_v4);

    let mut qdrant_points_replaced = false;
    let mut prior_qdrant_points = None;
    let mut vector_points_written_delta = 0usize;
    let chunk_records = if let (Some(qdrant_client), Some(vectors)) =
        (ctx.qdrant_client.as_ref(), ctx.embedded_vectors)
    {
        let points = vector_points_from_embeddings(
            document_id,
            &ctx.project,
            &ctx.namespace,
            &ctx.analyzed,
            vectors,
        );
        vector_points_written_delta = points.len();
        prior_qdrant_points = Some(
            qdrant::replace_document_points_with_prior_snapshot(
                qdrant_client,
                &ctx.qdrant_alias_code,
                document_id,
                &points,
            )
            .await?,
        );
        qdrant_points_replaced = true;
        to_chunk_records(
            &ctx.qdrant_alias_code,
            points,
            &ctx.analyzed.chunk_blueprints,
        )
    } else {
        to_chunk_records_without_vectors(&ctx.analyzed.chunk_blueprints)
    };

    let document_record = DocumentRecord {
        project_id: ctx.project.project_id,
        namespace_id: ctx.namespace.namespace_id,
        repo_root: ctx.project.repo_root.clone(),
        absolute_path: ctx.analyzed.absolute_path.display().to_string(),
        relative_path: ctx.analyzed.relative_path.clone(),
        language: ctx.analyzed.language.clone(),
        source_kind: ctx.analyzed.source_kind.clone(),
        git_commit_sha: ctx.git_commit_sha.clone(),
        file_sha256: ctx.analyzed.file_sha256.clone(),
        line_count: ctx.analyzed.line_count,
        byte_count: ctx.analyzed.byte_count,
        content: ctx.analyzed.content.clone(),
        metrics: ctx.analyzed.metrics.clone(),
        structure: ctx.analyzed.structure.clone(),
        imports: ctx.analyzed.imports.clone(),
        exports: ctx.analyzed.exports.clone(),
        diagnostics: ctx.analyzed.diagnostics.clone(),
        metadata: ctx.analyzed.metadata.clone(),
    };

    let replace_document_index_started = Instant::now();
    let replace_result = postgres::replace_document_index_with_document_id_detailed(
        db,
        &document_record,
        &ctx.analyzed.symbols,
        &chunk_records,
        document_id,
    )
    .await;
    if let Err(error) = replace_result {
        if qdrant_points_replaced {
            if let Some(qdrant_client) = ctx.qdrant_client.as_ref() {
                return Err(handle_replace_document_index_failure_after_qdrant_update(
                    Path::new(&ctx.project.repo_root),
                    &ctx.analyzed.relative_path,
                    document_id,
                    had_existing_document,
                    error,
                    || async {
                        if let Some(forced_error) =
                            forced_qdrant_compensation_failure_for_tests(had_existing_document)
                        {
                            return Err(forced_error);
                        }
                        if had_existing_document {
                            qdrant::replace_document_points(
                                qdrant_client,
                                &ctx.qdrant_alias_code,
                                document_id,
                                prior_qdrant_points.as_deref().unwrap_or(&[]),
                            )
                            .await
                        } else {
                            qdrant::clear_document_points(
                                qdrant_client,
                                &ctx.qdrant_alias_code,
                                document_id,
                            )
                            .await
                        }
                    },
                )
                .await);
            }
        }
        let verdict = finalize_postgres_failure_without_qdrant_update(
            &ctx.analyzed.relative_path,
            document_id,
            error.phase,
            error.sqlstate_code.clone(),
            error.error,
        );
        emit_qdrant_postgres_failure_verdict_log(
            &ctx.analyzed.relative_path,
            document_id,
            &verdict,
        );
        return Err(verdict.error);
    }

    postgres::observability_profile_log(
        "index_project.replace_document_index",
        replace_document_index_started.elapsed().as_millis(),
        &format!(
            "relative_path={} symbols={} chunks={} byte_count={}",
            ctx.analyzed.relative_path,
            ctx.analyzed.symbols.len(),
            chunk_records.len(),
            ctx.analyzed.byte_count
        ),
    );

    let edge_cache_started = Instant::now();
    if ctx.skip_edge_cache_writes {
        postgres::observability_profile_log(
            "index_project.edge_cache_upsert_skipped",
            0,
            &format!("relative_path={}", ctx.analyzed.relative_path),
        );
    } else {
        edge_cache::upsert_document(
            &ctx.edge_cache_path,
            &format!(
                "{}::{}::{}",
                ctx.project.code, ctx.namespace.code, ctx.analyzed.relative_path
            ),
            &ctx.project.code,
            &ctx.namespace.code,
            &ctx.analyzed.relative_path,
            &ctx.analyzed.content,
        )?;
        postgres::observability_profile_log(
            "index_project.edge_cache_upsert",
            edge_cache_started.elapsed().as_millis(),
            &format!(
                "relative_path={} content_bytes={}",
                ctx.analyzed.relative_path,
                ctx.analyzed.content.len()
            ),
        );
    }
    tracing::info!(path = %ctx.analyzed.relative_path, "indexed file");

    Ok(IndexProjectFileOutcome {
        files_indexed_delta: 1,
        ast_eligible_files_delta: usize::from(ctx.analyzed.ast_eligible),
        files_with_ast_delta: usize::from(
            ctx.analyzed.ast_eligible && ctx.analyzed.analysis_mode == "ast",
        ),
        files_with_lexical_fallback_delta: usize::from(
            ctx.analyzed.ast_eligible && ctx.analyzed.analysis_mode != "ast",
        ),
        files_without_ast_support_delta: usize::from(!ctx.analyzed.ast_eligible),
        symbols_written_delta: ctx.analyzed.symbols.len(),
        chunks_written_delta: chunk_records.len(),
        vector_points_written_delta,
        total_bytes_delta: ctx.analyzed.byte_count,
        language_key: ctx
            .analyzed
            .language
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
    })
}

fn canonicalize_existing_dir(path: &Path) -> Result<PathBuf> {
    let canonical = path
        .canonicalize()
        .with_context(|| format!("failed to canonicalize index root {}", path.display()))?;
    if !canonical.is_dir() {
        return Err(anyhow!(
            "index root is not a directory after canonicalization: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

fn should_skip_edge_cache_documents(
    args: &crate::cli::IndexProjectArgs,
    files: &[PathBuf],
) -> bool {
    args.skip_embeddings
        && args.preserve_namespace_documents
        && !files.is_empty()
        && files.iter().all(|file| {
            file.extension()
                .and_then(|value| value.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
        })
}

fn document_index_advisory_lock_key(namespace_id: Uuid, relative_path: &str) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(namespace_id.as_bytes());
    hasher.update(relative_path.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

fn finalize_postgres_failure_without_qdrant_update(
    relative_path: &str,
    document_id: Uuid,
    failure_phase: ReplaceDocumentIndexErrorPhase,
    failure_sqlstate: Option<String>,
    postgres_error: Error,
) -> QdrantPostgresFailureVerdict {
    QdrantPostgresFailureVerdict {
        mode: QdrantPostgresFailureMode::BeforeQdrantUpdate,
        failure_phase,
        failure_sqlstate,
        compensation_attempted: false,
        compensation_succeeded: None,
        consistency_state: QdrantPostgresConsistencyState::PostgresFailureBeforeQdrantUpdate,
        required_action:
            QdrantPostgresRequiredAction::RetryOrInvestigatePostgresBeforeRetryingQdrantMutation,
        remediation_bundle_path: None,
        error: postgres_error.context(format!(
            "postgres index failure before any qdrant update for {} document_id={}",
            relative_path, document_id
        )),
    }
}

fn qdrant_compensation_policy_for_replace_document_index_phase(
    phase: ReplaceDocumentIndexErrorPhase,
) -> QdrantCompensationPolicy {
    match phase {
        ReplaceDocumentIndexErrorPhase::BeforeCommit => {
            QdrantCompensationPolicy::AttemptCompensation
        }
        ReplaceDocumentIndexErrorPhase::CommitRolledBackAtCommit => {
            QdrantCompensationPolicy::AttemptCompensation
        }
        ReplaceDocumentIndexErrorPhase::CommitOutcomeUnknown => {
            QdrantCompensationPolicy::SkipCompensationCommitOutcomeUnknown
        }
    }
}

async fn handle_replace_document_index_failure_after_qdrant_update<F, Fut>(
    repo_root: &Path,
    relative_path: &str,
    document_id: Uuid,
    had_existing_document: bool,
    error: postgres::ReplaceDocumentIndexError,
    compensation: F,
) -> Error
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut verdict = match qdrant_compensation_policy_for_replace_document_index_phase(error.phase)
    {
        QdrantCompensationPolicy::AttemptCompensation => {
            let compensation_started = Instant::now();
            let compensation_result = compensation().await;
            postgres::observability_profile_log(
                "index_project.qdrant_compensation_after_postgres_failure",
                compensation_started.elapsed().as_millis(),
                &format!(
                    "relative_path={} document_id={} had_existing_document={} compensation_ok={}",
                    relative_path,
                    document_id,
                    had_existing_document,
                    compensation_result.is_ok()
                ),
            );
            if had_existing_document {
                finalize_existing_document_postgres_failure_after_qdrant_update(
                    relative_path,
                    document_id,
                    error.phase,
                    error.sqlstate_code.clone(),
                    error.error,
                    compensation_result,
                )
            } else {
                finalize_new_document_postgres_failure_after_qdrant_update(
                    relative_path,
                    document_id,
                    error.phase,
                    error.sqlstate_code.clone(),
                    error.error,
                    compensation_result,
                )
            }
        }
        QdrantCompensationPolicy::SkipCompensationCommitOutcomeUnknown => {
            if had_existing_document {
                finalize_existing_document_commit_outcome_unknown_after_qdrant_update(
                    relative_path,
                    document_id,
                    error.sqlstate_code.clone(),
                    error.error,
                )
            } else {
                finalize_new_document_commit_outcome_unknown_after_qdrant_update(
                    relative_path,
                    document_id,
                    error.sqlstate_code.clone(),
                    error.error,
                )
            }
        }
    };
    if verdict.required_action
        == QdrantPostgresRequiredAction::ManualCrossStoreInvestigationRequired
    {
        match write_qdrant_postgres_remediation_bundle(
            repo_root,
            relative_path,
            document_id,
            had_existing_document,
            &verdict,
        ) {
            Ok(bundle_path) => {
                verdict.remediation_bundle_path = Some(bundle_path.clone());
                attach_context_to_error(
                    &mut verdict.error,
                    format!("remediation_bundle_path={}", bundle_path.display()),
                );
            }
            Err(bundle_write_error) => {
                attach_context_to_error(
                    &mut verdict.error,
                    format!("remediation_bundle_write_failed={bundle_write_error:#}"),
                );
            }
        }
    }
    emit_qdrant_postgres_failure_verdict_log(relative_path, document_id, &verdict);
    verdict.error
}

#[cfg(not(test))]
fn forced_qdrant_compensation_failure_for_tests(_had_existing_document: bool) -> Option<Error> {
    None
}

#[cfg(test)]
fn forced_qdrant_compensation_failure_for_tests(had_existing_document: bool) -> Option<Error> {
    let raw = std::env::var("AMAI_TEST_FORCE_QDRANT_COMPENSATION_FAILURE").ok()?;
    let mode = raw.trim();
    let applies = match mode {
        "" => false,
        "always" => true,
        "existing_document" => had_existing_document,
        "new_document" => !had_existing_document,
        _ => false,
    };
    applies.then(|| {
        anyhow!(
            "forced qdrant compensation failure for tests scope={} mode={}",
            if had_existing_document {
                "existing_document"
            } else {
                "new_document"
            },
            mode
        )
    })
}

fn finalize_new_document_postgres_failure_after_qdrant_update(
    relative_path: &str,
    document_id: Uuid,
    failure_phase: ReplaceDocumentIndexErrorPhase,
    failure_sqlstate: Option<String>,
    postgres_error: Error,
    compensation_result: Result<()>,
) -> QdrantPostgresFailureVerdict {
    match compensation_result {
        Ok(()) => QdrantPostgresFailureVerdict {
            mode: QdrantPostgresFailureMode::NewDocumentCompensated,
            failure_phase,
            failure_sqlstate,
            compensation_attempted: true,
            compensation_succeeded: Some(true),
            consistency_state:
                QdrantPostgresConsistencyState::CrossStoreConsistencyRestoredByCompensation,
            required_action: QdrantPostgresRequiredAction::NoFurtherCrossStoreRecoveryRequired,
            remediation_bundle_path: None,
            error: postgres_error.context(format!(
                "postgres index failure after qdrant update was compensated for {} document_id={}",
                relative_path, document_id
            )),
        },
        Err(compensation_error) => build_qdrant_postgres_inconsistent_state_error(
            relative_path,
            document_id,
            failure_phase,
            failure_sqlstate,
            postgres_error,
            compensation_error,
        ),
    }
}

fn finalize_new_document_commit_outcome_unknown_after_qdrant_update(
    relative_path: &str,
    document_id: Uuid,
    failure_sqlstate: Option<String>,
    postgres_error: Error,
) -> QdrantPostgresFailureVerdict {
    QdrantPostgresFailureVerdict {
        mode: QdrantPostgresFailureMode::NewDocumentCommitOutcomeUnknown,
        failure_phase: ReplaceDocumentIndexErrorPhase::CommitOutcomeUnknown,
        failure_sqlstate,
        compensation_attempted: false,
        compensation_succeeded: None,
        consistency_state: QdrantPostgresConsistencyState::CrossStoreConsistencyUnknownCommitOutcome,
        required_action: QdrantPostgresRequiredAction::ManualCrossStoreInvestigationRequired,
        remediation_bundle_path: None,
        error: postgres_error.context(format!(
            "ambiguous postgres commit outcome after qdrant update for new document {}; qdrant compensation intentionally skipped to avoid cross-store corruption document_id={}",
            relative_path, document_id
        )),
    }
}

fn finalize_existing_document_postgres_failure_after_qdrant_update(
    relative_path: &str,
    document_id: Uuid,
    failure_phase: ReplaceDocumentIndexErrorPhase,
    failure_sqlstate: Option<String>,
    postgres_error: Error,
    compensation_result: Result<()>,
) -> QdrantPostgresFailureVerdict {
    match compensation_result {
        Ok(()) => QdrantPostgresFailureVerdict {
            mode: QdrantPostgresFailureMode::ExistingDocumentCompensated,
            failure_phase,
            failure_sqlstate,
            compensation_attempted: true,
            compensation_succeeded: Some(true),
            consistency_state:
                QdrantPostgresConsistencyState::CrossStoreConsistencyRestoredByCompensation,
            required_action: QdrantPostgresRequiredAction::NoFurtherCrossStoreRecoveryRequired,
            remediation_bundle_path: None,
            error: postgres_error.context(format!(
                "postgres index failure after qdrant update was compensated by restoring prior qdrant points for existing document {} document_id={}",
                relative_path, document_id
            )),
        },
        Err(compensation_error) => build_existing_document_qdrant_postgres_inconsistent_state_error(
            relative_path,
            document_id,
            failure_phase,
            failure_sqlstate,
            postgres_error,
            compensation_error,
        ),
    }
}

fn finalize_existing_document_commit_outcome_unknown_after_qdrant_update(
    relative_path: &str,
    document_id: Uuid,
    failure_sqlstate: Option<String>,
    postgres_error: Error,
) -> QdrantPostgresFailureVerdict {
    QdrantPostgresFailureVerdict {
        mode: QdrantPostgresFailureMode::ExistingDocumentCommitOutcomeUnknown,
        failure_phase: ReplaceDocumentIndexErrorPhase::CommitOutcomeUnknown,
        failure_sqlstate,
        compensation_attempted: false,
        compensation_succeeded: None,
        consistency_state: QdrantPostgresConsistencyState::CrossStoreConsistencyUnknownCommitOutcome,
        required_action: QdrantPostgresRequiredAction::ManualCrossStoreInvestigationRequired,
        remediation_bundle_path: None,
        error: postgres_error.context(format!(
            "ambiguous postgres commit outcome after qdrant update for existing document {}; qdrant compensation intentionally skipped to avoid cross-store corruption document_id={}",
            relative_path, document_id
        )),
    }
}

fn build_existing_document_qdrant_postgres_inconsistent_state_error(
    relative_path: &str,
    document_id: Uuid,
    failure_phase: ReplaceDocumentIndexErrorPhase,
    failure_sqlstate: Option<String>,
    postgres_error: Error,
    compensation_error: Error,
) -> QdrantPostgresFailureVerdict {
    QdrantPostgresFailureVerdict {
        mode: QdrantPostgresFailureMode::ExistingDocumentInconsistentState,
        failure_phase,
        failure_sqlstate,
        compensation_attempted: true,
        compensation_succeeded: Some(false),
        consistency_state:
            QdrantPostgresConsistencyState::CrossStoreInconsistentAfterCompensationFailure,
        required_action: QdrantPostgresRequiredAction::ManualCrossStoreInvestigationRequired,
        remediation_bundle_path: None,
        error: anyhow!(
            "inconsistent qdrant/postgres state for existing document after qdrant update and postgres index failure for {} document_id={}: postgres_error={:#}; qdrant_restore_error={:#}",
            relative_path,
            document_id,
            postgres_error,
            compensation_error
        ),
    }
}

fn build_qdrant_postgres_inconsistent_state_error(
    relative_path: &str,
    document_id: Uuid,
    failure_phase: ReplaceDocumentIndexErrorPhase,
    failure_sqlstate: Option<String>,
    postgres_error: Error,
    compensation_error: Error,
) -> QdrantPostgresFailureVerdict {
    QdrantPostgresFailureVerdict {
        mode: QdrantPostgresFailureMode::NewDocumentInconsistentState,
        failure_phase,
        failure_sqlstate,
        compensation_attempted: true,
        compensation_succeeded: Some(false),
        consistency_state:
            QdrantPostgresConsistencyState::CrossStoreInconsistentAfterCompensationFailure,
        required_action: QdrantPostgresRequiredAction::ManualCrossStoreInvestigationRequired,
        remediation_bundle_path: None,
        error: anyhow!(
            "inconsistent qdrant/postgres state after qdrant update and postgres index failure for {} document_id={}: postgres_error={:#}; qdrant_compensation_error={:#}",
            relative_path,
            document_id,
            postgres_error,
            compensation_error
        ),
    }
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

pub fn collect_files(
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
        call_references,
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
            "analysis_mode": analysis_mode,
            "call_references": call_references
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
        analysis.call_references,
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

fn embed_chunk_vectors(
    cfg: &AppConfig,
    analyzed: &AnalyzedFile,
    embedder: &mut TextEmbedding,
) -> Result<Vec<Vec<f32>>> {
    let texts = analyzed
        .chunk_blueprints
        .iter()
        .map(|chunk| chunk.content.clone())
        .collect::<Vec<_>>();
    let vectors = embedder.embed(&texts, Some(16))?;
    for vector in &vectors {
        if vector.len() as u64 != cfg.qdrant_code_dim {
            return Err(anyhow!(
                "embedding size mismatch: expected {}, got {}",
                cfg.qdrant_code_dim,
                vector.len()
            ));
        }
    }
    Ok(vectors)
}

fn vector_points_from_embeddings(
    document_id: Uuid,
    project: &ProjectRecord,
    namespace: &NamespaceRecord,
    analyzed: &AnalyzedFile,
    vectors: Vec<Vec<f32>>,
) -> Vec<VectorPoint> {
    analyzed
        .chunk_blueprints
        .iter()
        .zip(vectors.into_iter())
        .map(|(chunk, vector)| VectorPoint {
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
        .collect()
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
    use super::{
        AnalyzedFile, ChunkBlueprint, IndexProjectLockedFileCtx, QdrantCompensationPolicy,
        QdrantPostgresConsistencyState, QdrantPostgresFailureMode, QdrantPostgresRequiredAction,
        build_qdrant_postgres_inconsistent_state_error, canonicalize_existing_dir,
        collect_explicit_files, compute_parser_coverage_ratio, document_index_advisory_lock_key,
        emit_qdrant_postgres_failure_verdict_log,
        finalize_existing_document_commit_outcome_unknown_after_qdrant_update,
        finalize_existing_document_postgres_failure_after_qdrant_update,
        finalize_new_document_commit_outcome_unknown_after_qdrant_update,
        finalize_new_document_postgres_failure_after_qdrant_update,
        finalize_postgres_failure_without_qdrant_update,
        handle_replace_document_index_failure_after_qdrant_update, hex_sha256,
        index_project_file_under_lock, qdrant_compensation_policy_for_replace_document_index_phase,
        render_qdrant_postgres_failure_verdict_log, should_index_path,
        should_skip_edge_cache_documents, to_chunk_records, vector_points_from_embeddings,
    };
    use crate::cli::IndexProjectArgs;
    use crate::config::AppConfig;
    use crate::postgres::{
        DocumentRecord, NamespaceRecord, ProjectRecord, ReplaceDocumentIndexError,
        ReplaceDocumentIndexErrorPhase, connect_admin, ensure_namespace,
        get_document_id_for_namespace_relative_path, get_project_by_code,
        replace_document_index_with_document_id, take_observability_profile_test_logs,
    };
    use crate::qdrant;
    use anyhow::anyhow;
    use serde_json::{Value, json};
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::runtime::Runtime;
    use uuid::Uuid;

    static INDEXER_RUNTIME_FAULT_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn load_runtime_test_env() {
        if let Ok(env_text) =
            std::fs::read_to_string(".env").or_else(|_| std::fs::read_to_string(".env.example"))
        {
            for line in env_text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = trimmed.split_once('=') {
                    unsafe {
                        std::env::set_var(key.trim(), value.trim_matches('\"'));
                    }
                }
            }
        }
    }

    struct ScopedEnvVar {
        key: &'static str,
        previous: Option<String>,
    }

    impl ScopedEnvVar {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                unsafe {
                    std::env::set_var(self.key, previous);
                }
            } else {
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

    fn runtime_test_vectors(chunk_count: usize, dim: u64, seed: f32) -> Vec<Vec<f32>> {
        (0..chunk_count)
            .map(|index| vec![seed + index as f32; dim as usize])
            .collect()
    }

    fn build_runtime_test_analyzed_file(
        relative_path: &str,
        absolute_path: &Path,
        content: &str,
    ) -> AnalyzedFile {
        let line_count = content.lines().count() as i32;
        let byte_count = content.len() as i64;
        let chunk = ChunkBlueprint {
            chunk_index: 0,
            total_chunks: 1,
            start_line: 0,
            end_line: line_count,
            start_byte: 0,
            end_byte: content.len() as i32,
            content: content.to_string(),
            metadata: json!({
                "language": "rust",
                "node_types": [],
                "context_path": [],
                "symbols_defined": [],
                "has_error_nodes": false
            }),
        };
        AnalyzedFile {
            absolute_path: absolute_path.to_path_buf(),
            relative_path: relative_path.to_string(),
            language: Some("rust".to_string()),
            source_kind: "git".to_string(),
            ast_eligible: false,
            file_sha256: hex_sha256(content.as_bytes()),
            line_count,
            byte_count,
            content: content.to_string(),
            analysis_mode: "lexical_fallback".to_string(),
            metrics: json!({"total_lines": line_count, "total_bytes": byte_count}),
            structure: json!({}),
            imports: json!([]),
            exports: json!([]),
            diagnostics: json!([]),
            metadata: json!({"runtime_fault_test": true}),
            symbols: Vec::new(),
            chunk_blueprints: vec![chunk],
        }
    }

    fn build_runtime_test_document_record(
        project: &ProjectRecord,
        namespace: &NamespaceRecord,
        analyzed: &AnalyzedFile,
        git_commit_sha: Option<String>,
    ) -> DocumentRecord {
        DocumentRecord {
            project_id: project.project_id,
            namespace_id: namespace.namespace_id,
            repo_root: project.repo_root.clone(),
            absolute_path: analyzed.absolute_path.display().to_string(),
            relative_path: analyzed.relative_path.clone(),
            language: analyzed.language.clone(),
            source_kind: analyzed.source_kind.clone(),
            git_commit_sha,
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
        }
    }

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
    fn canonicalize_existing_dir_resolves_relative_segments() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("amai-indexer-canonical-root-{unique}"));
        let nested = root.join("nested");
        fs::create_dir_all(&nested).expect("create nested");
        let raw = nested.join("..").join("nested").join(".");

        let canonical = canonicalize_existing_dir(&raw).expect("canonical root");
        assert_eq!(canonical, nested);

        fs::remove_dir_all(&root).expect("cleanup");
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

    #[tokio::test]
    async fn runtime_forced_existing_document_compensation_failure_flows_through_index_project_file_under_lock()
     {
        let _test_lock = INDEXER_RUNTIME_FAULT_TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("runtime fault lock");
        load_runtime_test_env();

        let mut cfg = AppConfig::from_env().expect("config");
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let temp_root = std::env::temp_dir().join(format!("amai-indexer-runtime-fault-{suffix}"));
        let source_dir = temp_root.join("src");
        fs::create_dir_all(&source_dir).expect("create source dir");
        let edge_cache_path = temp_root.join("edge-cache");
        fs::create_dir_all(&edge_cache_path).expect("create edge cache");
        cfg.edge_cache_path = edge_cache_path.clone();
        let remediation_dir = temp_root.join("remediation-bundles");

        let absolute_path = source_dir.join("lib.rs");
        let relative_path = "src/lib.rs";
        fs::write(
            &absolute_path,
            "pub fn forced_runtime_fault() -> usize { 1 }\n",
        )
        .expect("write source file");

        let mut client = connect_admin(&cfg).await.expect("postgres");
        let qdrant_client = qdrant::connect(&cfg).expect("qdrant");
        qdrant::bootstrap_collections(&qdrant_client, &cfg)
            .await
            .expect("bootstrap qdrant");

        let project = get_project_by_code(&client, "project_alpha")
            .await
            .expect("project_alpha");
        let namespace_code = format!("idx_runtime_fault_{suffix}");
        let namespace = ensure_namespace(
            &client,
            project.project_id,
            &namespace_code,
            Some(&namespace_code),
            "local_strict",
        )
        .await
        .expect("namespace");

        let existing_document_id = Uuid::new_v4();
        let seeded = build_runtime_test_analyzed_file(
            relative_path,
            &absolute_path,
            "pub fn seeded_version() -> usize { 1 }\n",
        );
        let seeded_vectors =
            runtime_test_vectors(seeded.chunk_blueprints.len(), cfg.qdrant_code_dim, 1.0);
        let seeded_points = vector_points_from_embeddings(
            existing_document_id,
            &project,
            &namespace,
            &seeded,
            seeded_vectors,
        );
        qdrant::replace_document_points(
            &qdrant_client,
            &cfg.qdrant_alias_code,
            existing_document_id,
            &seeded_points,
        )
        .await
        .expect("seed qdrant points");
        let seeded_chunks = to_chunk_records(
            &cfg.qdrant_alias_code,
            seeded_points.clone(),
            &seeded.chunk_blueprints,
        );
        replace_document_index_with_document_id(
            &mut client,
            &build_runtime_test_document_record(
                &project,
                &namespace,
                &seeded,
                Some(format!("seed-{suffix}")),
            ),
            &seeded.symbols,
            &seeded_chunks,
            existing_document_id,
        )
        .await
        .expect("seed postgres document");

        let updated = build_runtime_test_analyzed_file(
            relative_path,
            &absolute_path,
            "pub fn updated_version() -> usize { 2 }\n",
        );
        let updated_vectors =
            runtime_test_vectors(updated.chunk_blueprints.len(), cfg.qdrant_code_dim, 9.0);
        let _clear_logs = take_observability_profile_test_logs();
        let _replace_failure = ScopedEnvVar::set(
            "AMAI_TEST_FORCE_REPLACE_DOCUMENT_INDEX_FAILURE",
            "before_commit:23514",
        );
        let _compensation_failure = ScopedEnvVar::set(
            "AMAI_TEST_FORCE_QDRANT_COMPENSATION_FAILURE",
            "existing_document",
        );
        let remediation_dir_value = remediation_dir.display().to_string();
        let _remediation_dir = ScopedEnvVar::set(
            "AMAI_QDRANT_POSTGRES_REMEDIATION_DIR",
            &remediation_dir_value,
        );

        let error = match index_project_file_under_lock(
            &mut client,
            IndexProjectLockedFileCtx {
                project: project.clone(),
                namespace: namespace.clone(),
                analyzed: updated,
                git_commit_sha: Some(format!("update-{suffix}")),
                qdrant_client: Some(qdrant_client.clone()),
                embedded_vectors: Some(updated_vectors.clone()),
                edge_cache_path: edge_cache_path.clone(),
                qdrant_alias_code: cfg.qdrant_alias_code.clone(),
                skip_edge_cache_writes: false,
            },
        )
        .await
        {
            Ok(_) => panic!("expected forced compensation failure"),
            Err(error) => error,
        };
        let rendered = format!("{error:#}");
        assert!(rendered.contains("inconsistent qdrant/postgres state for existing document"));
        assert!(rendered.contains("forced replace_document_index failure for tests"));
        assert!(rendered.contains("sqlstate=23514"));
        assert!(rendered.contains("forced qdrant compensation failure for tests"));
        assert!(rendered.contains("remediation_bundle_path="));

        let persisted_document_id = get_document_id_for_namespace_relative_path(
            &client,
            namespace.namespace_id,
            relative_path,
        )
        .await
        .expect("load persisted id")
        .expect("existing document id");
        assert_eq!(persisted_document_id, existing_document_id);

        let qdrant_points = qdrant::snapshot_document_points_for_tests(
            &qdrant_client,
            &cfg.qdrant_alias_code,
            existing_document_id,
        )
        .await
        .expect("snapshot qdrant points");
        assert_eq!(qdrant_points.len(), 1);
        assert_eq!(
            qdrant_points[0].payload["relative_path"],
            json!(relative_path)
        );
        assert_eq!(
            qdrant_points[0].payload["document_id"],
            json!(existing_document_id)
        );

        let logs = take_observability_profile_test_logs().join("\n");
        assert!(logs.contains("stage=index_project.qdrant_compensation_after_postgres_failure"));
        assert!(logs.contains("compensation_ok=false"));
        assert!(logs.contains("stage=index_project.qdrant_postgres_manual_recovery_bundle"));
        assert!(logs.contains("stage=index_project.qdrant_postgres_failure_verdict"));
        assert!(logs.contains("failure_mode=existing_document_inconsistent_state"));
        assert!(
            logs.contains("consistency_state=cross_store_inconsistent_after_compensation_failure")
        );
        assert!(logs.contains("required_action=manual_cross_store_investigation_required"));
        assert!(logs.contains("remediation_bundle_path="));

        let bundle_paths = fs::read_dir(&remediation_dir)
            .expect("read remediation dir")
            .map(|entry| entry.expect("dir entry").path())
            .collect::<Vec<_>>();
        assert_eq!(bundle_paths.len(), 1);
        let bundle: Value = serde_json::from_str(
            &fs::read_to_string(&bundle_paths[0]).expect("read remediation bundle"),
        )
        .expect("parse remediation bundle");
        assert_eq!(
            bundle["artifact_version"],
            json!("qdrant_postgres_remediation_bundle_v1")
        );
        assert_eq!(
            bundle["failure_mode"],
            json!("existing_document_inconsistent_state")
        );
        assert_eq!(
            bundle["required_action"],
            json!("manual_cross_store_investigation_required")
        );
        assert_eq!(bundle["relative_path"], json!(relative_path));
        assert_eq!(bundle["document_id"], json!(existing_document_id));

        qdrant::clear_document_points(&qdrant_client, &cfg.qdrant_alias_code, existing_document_id)
            .await
            .expect("cleanup qdrant points");
        fs::remove_dir_all(&temp_root).expect("cleanup temp root");
    }

    #[test]
    fn synthetic_markdown_runtime_skips_edge_cache_writes() {
        let args = IndexProjectArgs {
            code: "benchrt_demo".to_string(),
            path: PathBuf::from("/tmp/demo"),
            namespace: "ns".to_string(),
            limit_files: None,
            paths_file: Some(PathBuf::from("/tmp/demo/paths.txt")),
            skip_embeddings: true,
            preserve_namespace_documents: true,
        };
        let files = vec![
            PathBuf::from("/tmp/demo/001_window.md"),
            PathBuf::from("/tmp/demo/002_window.md"),
        ];
        assert!(should_skip_edge_cache_documents(&args, &files));
    }

    #[test]
    fn non_markdown_or_non_runtime_paths_keep_edge_cache_writes() {
        let args = IndexProjectArgs {
            code: "benchrt_demo".to_string(),
            path: PathBuf::from("/tmp/demo"),
            namespace: "ns".to_string(),
            limit_files: None,
            paths_file: Some(PathBuf::from("/tmp/demo/paths.txt")),
            skip_embeddings: true,
            preserve_namespace_documents: true,
        };
        assert!(!should_skip_edge_cache_documents(
            &args,
            &[PathBuf::from("/tmp/demo/src/lib.rs")]
        ));
        let args_without_preserve = IndexProjectArgs {
            code: "benchrt_demo".to_string(),
            path: PathBuf::from("/tmp/demo"),
            namespace: "ns".to_string(),
            limit_files: None,
            paths_file: Some(PathBuf::from("/tmp/demo/paths.txt")),
            skip_embeddings: true,
            preserve_namespace_documents: false,
        };
        assert!(!should_skip_edge_cache_documents(
            &args_without_preserve,
            &[PathBuf::from("/tmp/demo/001_window.md")]
        ));
    }

    #[test]
    fn inconsistent_state_error_mentions_both_failures() {
        let verdict = build_qdrant_postgres_inconsistent_state_error(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            None,
            anyhow!("postgres write failed"),
            anyhow!("qdrant cleanup failed"),
        );
        assert_eq!(
            verdict.mode,
            QdrantPostgresFailureMode::NewDocumentInconsistentState
        );
        assert!(verdict.compensation_attempted);
        assert_eq!(verdict.compensation_succeeded, Some(false));
        let rendered = format!("{:#}", verdict.error);
        assert!(rendered.contains("inconsistent qdrant/postgres state"));
        assert!(rendered.contains("after qdrant update and postgres index failure"));
        assert!(rendered.contains("postgres write failed"));
        assert!(rendered.contains("qdrant cleanup failed"));
        assert!(rendered.contains("document_id=00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn finalize_postgres_failure_marks_compensated_qdrant_update() {
        let verdict = finalize_new_document_postgres_failure_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            None,
            anyhow!("postgres write failed"),
            Ok(()),
        );
        assert_eq!(
            verdict.mode,
            QdrantPostgresFailureMode::NewDocumentCompensated
        );
        assert!(verdict.compensation_attempted);
        assert_eq!(verdict.compensation_succeeded, Some(true));
        let rendered = format!("{:#}", verdict.error);
        assert!(rendered.contains("postgres index failure after qdrant update was compensated"));
        assert!(rendered.contains("postgres write failed"));
        assert!(rendered.contains("document_id=00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn finalize_postgres_failure_marks_inconsistent_state_when_compensation_fails() {
        let verdict = finalize_new_document_postgres_failure_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            None,
            anyhow!("postgres write failed"),
            Err(anyhow!("qdrant cleanup failed")),
        );
        assert_eq!(
            verdict.mode,
            QdrantPostgresFailureMode::NewDocumentInconsistentState
        );
        assert_eq!(
            verdict.failure_phase,
            ReplaceDocumentIndexErrorPhase::BeforeCommit
        );
        assert!(verdict.compensation_attempted);
        assert_eq!(verdict.compensation_succeeded, Some(false));
        let rendered = format!("{:#}", verdict.error);
        assert!(rendered.contains("inconsistent qdrant/postgres state"));
        assert!(rendered.contains("postgres write failed"));
        assert!(rendered.contains("qdrant cleanup failed"));
    }

    #[test]
    fn new_document_commit_outcome_unknown_skips_compensation() {
        let verdict = finalize_new_document_commit_outcome_unknown_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            Some("40003".to_string()),
            anyhow!("postgres commit outcome unknown"),
        );
        assert_eq!(
            verdict.mode,
            QdrantPostgresFailureMode::NewDocumentCommitOutcomeUnknown
        );
        assert!(!verdict.compensation_attempted);
        assert_eq!(verdict.compensation_succeeded, None);
        assert_eq!(
            verdict.consistency_state,
            QdrantPostgresConsistencyState::CrossStoreConsistencyUnknownCommitOutcome
        );
        assert_eq!(
            verdict.required_action,
            QdrantPostgresRequiredAction::ManualCrossStoreInvestigationRequired
        );
        let rendered = format!("{:#}", verdict.error);
        assert!(rendered.contains("ambiguous postgres commit outcome after qdrant update"));
        assert!(rendered.contains("compensation intentionally skipped"));
        assert!(rendered.contains("postgres commit outcome unknown"));
    }

    #[test]
    fn before_commit_phase_requires_qdrant_compensation_attempt() {
        assert_eq!(
            qdrant_compensation_policy_for_replace_document_index_phase(
                ReplaceDocumentIndexErrorPhase::BeforeCommit
            ),
            QdrantCompensationPolicy::AttemptCompensation
        );
    }

    #[test]
    fn commit_rolled_back_at_commit_phase_requires_qdrant_compensation_attempt() {
        assert_eq!(
            qdrant_compensation_policy_for_replace_document_index_phase(
                ReplaceDocumentIndexErrorPhase::CommitRolledBackAtCommit
            ),
            QdrantCompensationPolicy::AttemptCompensation
        );
    }

    #[test]
    fn commit_outcome_unknown_phase_skips_qdrant_compensation_attempt() {
        assert_eq!(
            qdrant_compensation_policy_for_replace_document_index_phase(
                ReplaceDocumentIndexErrorPhase::CommitOutcomeUnknown
            ),
            QdrantCompensationPolicy::SkipCompensationCommitOutcomeUnknown
        );
    }

    #[test]
    fn compensation_succeeded_surface_is_tristate() {
        let before = finalize_postgres_failure_without_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            None,
            anyhow!("postgres write failed"),
        );
        assert_eq!(before.compensation_succeeded_surface(), "none");

        let compensated = finalize_new_document_postgres_failure_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            None,
            anyhow!("postgres write failed"),
            Ok(()),
        );
        assert_eq!(compensated.compensation_succeeded_surface(), "true");

        let inconsistent = finalize_new_document_postgres_failure_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            None,
            anyhow!("postgres write failed"),
            Err(anyhow!("qdrant cleanup failed")),
        );
        assert_eq!(inconsistent.compensation_succeeded_surface(), "false");
    }

    #[test]
    fn qdrant_postgres_failure_verdict_log_uses_shared_tristate_contract() {
        let before = finalize_postgres_failure_without_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            Some("23514".to_string()),
            anyhow!("postgres write failed"),
        );
        let rendered = render_qdrant_postgres_failure_verdict_log(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            &before,
        );
        assert!(rendered.contains("failure_mode=before_qdrant_update"));
        assert!(rendered.contains("failure_phase=before_commit"));
        assert!(rendered.contains("failure_sqlstate=23514"));
        assert!(rendered.contains("compensation_attempted=false"));
        assert!(rendered.contains("compensation_succeeded=none"));
        assert!(rendered.contains("consistency_state=postgres_failure_before_qdrant_update"));
        assert!(rendered.contains(
            "required_action=retry_or_investigate_postgres_before_retrying_qdrant_mutation"
        ));

        let compensated = finalize_new_document_postgres_failure_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::CommitRolledBackAtCommit,
            Some("40P01".to_string()),
            anyhow!("postgres write failed"),
            Ok(()),
        );
        let rendered = render_qdrant_postgres_failure_verdict_log(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            &compensated,
        );
        assert!(rendered.contains("failure_mode=new_document_compensated"));
        assert!(rendered.contains("failure_phase=commit_rolled_back_at_commit"));
        assert!(rendered.contains("failure_sqlstate=40P01"));
        assert!(rendered.contains("compensation_attempted=true"));
        assert!(rendered.contains("compensation_succeeded=true"));
        assert!(
            rendered.contains("consistency_state=cross_store_consistency_restored_by_compensation")
        );
        assert!(rendered.contains("required_action=no_further_cross_store_recovery_required"));

        let inconsistent = finalize_new_document_postgres_failure_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            None,
            anyhow!("postgres write failed"),
            Err(anyhow!("qdrant cleanup failed")),
        );
        let rendered = render_qdrant_postgres_failure_verdict_log(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            &inconsistent,
        );
        assert!(rendered.contains("failure_mode=new_document_inconsistent_state"));
        assert!(rendered.contains("failure_phase=before_commit"));
        assert!(rendered.contains("failure_sqlstate=none"));
        assert!(rendered.contains("compensation_attempted=true"));
        assert!(rendered.contains("compensation_succeeded=false"));
        assert!(
            rendered
                .contains("consistency_state=cross_store_inconsistent_after_compensation_failure")
        );
        assert!(rendered.contains("required_action=manual_cross_store_investigation_required"));

        let unknown = finalize_new_document_commit_outcome_unknown_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            Some("40003".to_string()),
            anyhow!("postgres commit outcome unknown"),
        );
        let rendered = render_qdrant_postgres_failure_verdict_log(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            &unknown,
        );
        assert!(rendered.contains("failure_mode=new_document_commit_outcome_unknown"));
        assert!(rendered.contains("failure_phase=commit_outcome_unknown"));
        assert!(rendered.contains("failure_sqlstate=40003"));
        assert!(rendered.contains("compensation_attempted=false"));
        assert!(rendered.contains("compensation_succeeded=none"));
        assert!(
            rendered.contains("consistency_state=cross_store_consistency_unknown_commit_outcome")
        );
        assert!(rendered.contains("required_action=manual_cross_store_investigation_required"));
    }

    #[test]
    fn existing_document_postgres_failure_after_qdrant_update_is_compensated_when_restore_succeeds()
    {
        let verdict = finalize_existing_document_postgres_failure_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::CommitRolledBackAtCommit,
            Some("40001".to_string()),
            anyhow!("postgres write failed"),
            Ok(()),
        );
        assert_eq!(
            verdict.mode,
            QdrantPostgresFailureMode::ExistingDocumentCompensated
        );
        assert_eq!(
            verdict.failure_phase,
            ReplaceDocumentIndexErrorPhase::CommitRolledBackAtCommit
        );
        assert!(verdict.compensation_attempted);
        assert_eq!(verdict.compensation_succeeded, Some(true));
        let rendered = format!("{:#}", verdict.error);
        assert!(rendered.contains(
            "postgres index failure after qdrant update was compensated by restoring prior qdrant points for existing document"
        ));
        assert!(rendered.contains("postgres write failed"));
        assert!(!rendered.contains("inconsistent qdrant/postgres state"));
    }

    #[test]
    fn existing_document_postgres_failure_after_qdrant_update_is_inconsistent_when_restore_fails() {
        let verdict = finalize_existing_document_postgres_failure_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            None,
            anyhow!("postgres write failed"),
            Err(anyhow!("qdrant restore failed")),
        );
        assert_eq!(
            verdict.mode,
            QdrantPostgresFailureMode::ExistingDocumentInconsistentState
        );
        assert_eq!(
            verdict.failure_phase,
            ReplaceDocumentIndexErrorPhase::BeforeCommit
        );
        assert!(verdict.compensation_attempted);
        assert_eq!(verdict.compensation_succeeded, Some(false));
        let rendered = format!("{:#}", verdict.error);
        assert!(rendered.contains("inconsistent qdrant/postgres state for existing document"));
        assert!(rendered.contains("postgres write failed"));
        assert!(rendered.contains("qdrant restore failed"));
        assert!(!rendered.contains("was compensated"));
    }

    #[test]
    fn existing_document_commit_outcome_unknown_skips_compensation() {
        let verdict = finalize_existing_document_commit_outcome_unknown_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            Some("40003".to_string()),
            anyhow!("postgres commit outcome unknown"),
        );
        assert_eq!(
            verdict.mode,
            QdrantPostgresFailureMode::ExistingDocumentCommitOutcomeUnknown
        );
        assert!(!verdict.compensation_attempted);
        assert_eq!(verdict.compensation_succeeded, None);
        assert_eq!(
            verdict.consistency_state,
            QdrantPostgresConsistencyState::CrossStoreConsistencyUnknownCommitOutcome
        );
        assert_eq!(
            verdict.required_action,
            QdrantPostgresRequiredAction::ManualCrossStoreInvestigationRequired
        );
        let rendered = format!("{:#}", verdict.error);
        assert!(rendered.contains("ambiguous postgres commit outcome after qdrant update"));
        assert!(rendered.contains("compensation intentionally skipped"));
        assert!(rendered.contains("postgres commit outcome unknown"));
    }

    #[test]
    fn qdrant_postgres_failure_verdict_recovery_contract_is_explicit_across_branch_classes() {
        let before = finalize_postgres_failure_without_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            Some("23514".to_string()),
            anyhow!("postgres write failed"),
        );
        assert_eq!(
            before.consistency_state,
            QdrantPostgresConsistencyState::PostgresFailureBeforeQdrantUpdate
        );
        assert_eq!(
            before.required_action,
            QdrantPostgresRequiredAction::RetryOrInvestigatePostgresBeforeRetryingQdrantMutation
        );

        let compensated = finalize_new_document_postgres_failure_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::CommitRolledBackAtCommit,
            Some("40P01".to_string()),
            anyhow!("postgres write failed"),
            Ok(()),
        );
        assert_eq!(
            compensated.consistency_state,
            QdrantPostgresConsistencyState::CrossStoreConsistencyRestoredByCompensation
        );
        assert_eq!(
            compensated.required_action,
            QdrantPostgresRequiredAction::NoFurtherCrossStoreRecoveryRequired
        );

        let unknown = finalize_new_document_commit_outcome_unknown_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            Some("40003".to_string()),
            anyhow!("postgres commit outcome unknown"),
        );
        assert_eq!(
            unknown.consistency_state,
            QdrantPostgresConsistencyState::CrossStoreConsistencyUnknownCommitOutcome
        );
        assert_eq!(
            unknown.required_action,
            QdrantPostgresRequiredAction::ManualCrossStoreInvestigationRequired
        );

        let inconsistent = finalize_new_document_postgres_failure_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            None,
            anyhow!("postgres write failed"),
            Err(anyhow!("qdrant cleanup failed")),
        );
        assert_eq!(
            inconsistent.consistency_state,
            QdrantPostgresConsistencyState::CrossStoreInconsistentAfterCompensationFailure
        );
        assert_eq!(
            inconsistent.required_action,
            QdrantPostgresRequiredAction::ManualCrossStoreInvestigationRequired
        );
    }

    #[test]
    fn live_failure_emitter_surfaces_recovery_contract_in_observability_log() {
        let _ = take_observability_profile_test_logs();
        let verdict = finalize_new_document_commit_outcome_unknown_after_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            Some("40003".to_string()),
            anyhow!("postgres commit outcome unknown"),
        );
        emit_qdrant_postgres_failure_verdict_log(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            &verdict,
        );
        let logs = take_observability_profile_test_logs();
        let joined = logs.join("\n");
        assert!(joined.contains("stage=index_project.qdrant_postgres_failure_verdict"));
        assert!(joined.contains("failure_mode=new_document_commit_outcome_unknown"));
        assert!(joined.contains("failure_phase=commit_outcome_unknown"));
        assert!(joined.contains("failure_sqlstate=40003"));
        assert!(
            joined.contains("consistency_state=cross_store_consistency_unknown_commit_outcome")
        );
        assert!(joined.contains("required_action=manual_cross_store_investigation_required"));
    }

    #[test]
    fn forced_post_qdrant_failure_branch_skips_compensation_for_commit_outcome_unknown() {
        let _ = take_observability_profile_test_logs();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let remediation_dir =
            std::env::temp_dir().join(format!("amai-indexer-remediation-unknown-{unique}"));
        let remediation_dir_value = remediation_dir.display().to_string();
        let _remediation_dir = ScopedEnvVar::set(
            "AMAI_QDRANT_POSTGRES_REMEDIATION_DIR",
            &remediation_dir_value,
        );
        let compensation_called = AtomicBool::new(false);
        let error = ReplaceDocumentIndexError {
            phase: ReplaceDocumentIndexErrorPhase::CommitOutcomeUnknown,
            sqlstate_code: Some("40003".to_string()),
            error: anyhow!("postgres commit outcome unknown"),
        };
        let returned = Runtime::new().expect("runtime").block_on(
            handle_replace_document_index_failure_after_qdrant_update(
                Path::new("."),
                "fixtures/runtime/001_window.md",
                Uuid::nil(),
                false,
                error,
                || async {
                    compensation_called.store(true, AtomicOrdering::SeqCst);
                    Ok(())
                },
            ),
        );
        assert!(!compensation_called.load(AtomicOrdering::SeqCst));
        let rendered = format!("{:#}", returned);
        assert!(rendered.contains("ambiguous postgres commit outcome after qdrant update"));
        assert!(rendered.contains("remediation_bundle_path="));
        let joined = take_observability_profile_test_logs().join("\n");
        assert!(joined.contains("stage=index_project.qdrant_postgres_manual_recovery_bundle"));
        assert!(joined.contains("stage=index_project.qdrant_postgres_failure_verdict"));
        assert!(joined.contains("failure_mode=new_document_commit_outcome_unknown"));
        assert!(joined.contains("remediation_bundle_path="));
        assert!(!joined.contains("stage=index_project.qdrant_compensation_after_postgres_failure"));
        let bundle_paths = fs::read_dir(&remediation_dir)
            .expect("read remediation dir")
            .map(|entry| entry.expect("dir entry").path())
            .collect::<Vec<_>>();
        assert_eq!(bundle_paths.len(), 1);
        fs::remove_dir_all(&remediation_dir).expect("cleanup remediation dir");
    }

    #[test]
    fn forced_post_qdrant_failure_branch_attempts_compensation_and_emits_both_logs() {
        let _ = take_observability_profile_test_logs();
        let compensation_called = AtomicBool::new(false);
        let error = ReplaceDocumentIndexError {
            phase: ReplaceDocumentIndexErrorPhase::BeforeCommit,
            sqlstate_code: Some("23514".to_string()),
            error: anyhow!("postgres write failed"),
        };
        let returned = Runtime::new().expect("runtime").block_on(
            handle_replace_document_index_failure_after_qdrant_update(
                Path::new("."),
                "fixtures/runtime/001_window.md",
                Uuid::nil(),
                false,
                error,
                || async {
                    compensation_called.store(true, AtomicOrdering::SeqCst);
                    Ok(())
                },
            ),
        );
        assert!(compensation_called.load(AtomicOrdering::SeqCst));
        let rendered = format!("{:#}", returned);
        assert!(rendered.contains("postgres index failure after qdrant update was compensated"));
        let joined = take_observability_profile_test_logs().join("\n");
        assert!(joined.contains("stage=index_project.qdrant_compensation_after_postgres_failure"));
        assert!(joined.contains("compensation_ok=true"));
        assert!(joined.contains("stage=index_project.qdrant_postgres_failure_verdict"));
        assert!(
            joined.contains("consistency_state=cross_store_consistency_restored_by_compensation")
        );
    }

    #[test]
    fn forced_existing_document_post_qdrant_failure_branch_skips_compensation_for_commit_outcome_unknown()
     {
        let _ = take_observability_profile_test_logs();
        let compensation_called = AtomicBool::new(false);
        let error = ReplaceDocumentIndexError {
            phase: ReplaceDocumentIndexErrorPhase::CommitOutcomeUnknown,
            sqlstate_code: Some("40003".to_string()),
            error: anyhow!("postgres commit outcome unknown"),
        };
        let returned = Runtime::new().expect("runtime").block_on(
            handle_replace_document_index_failure_after_qdrant_update(
                Path::new("."),
                "fixtures/runtime/001_window.md",
                Uuid::nil(),
                true,
                error,
                || async {
                    compensation_called.store(true, AtomicOrdering::SeqCst);
                    Ok(())
                },
            ),
        );
        assert!(!compensation_called.load(AtomicOrdering::SeqCst));
        let rendered = format!("{:#}", returned);
        assert!(rendered.contains(
            "ambiguous postgres commit outcome after qdrant update for existing document"
        ));
        let joined = take_observability_profile_test_logs().join("\n");
        assert!(joined.contains("stage=index_project.qdrant_postgres_failure_verdict"));
        assert!(joined.contains("failure_mode=existing_document_commit_outcome_unknown"));
        assert!(!joined.contains("stage=index_project.qdrant_compensation_after_postgres_failure"));
    }

    #[test]
    fn forced_existing_document_post_qdrant_failure_branch_attempts_restore_and_emits_both_logs() {
        let _ = take_observability_profile_test_logs();
        let compensation_called = AtomicBool::new(false);
        let error = ReplaceDocumentIndexError {
            phase: ReplaceDocumentIndexErrorPhase::CommitRolledBackAtCommit,
            sqlstate_code: Some("40P01".to_string()),
            error: anyhow!("postgres write failed"),
        };
        let returned = Runtime::new().expect("runtime").block_on(
            handle_replace_document_index_failure_after_qdrant_update(
                Path::new("."),
                "fixtures/runtime/001_window.md",
                Uuid::nil(),
                true,
                error,
                || async {
                    compensation_called.store(true, AtomicOrdering::SeqCst);
                    Ok(())
                },
            ),
        );
        assert!(compensation_called.load(AtomicOrdering::SeqCst));
        let rendered = format!("{:#}", returned);
        assert!(rendered.contains(
            "postgres index failure after qdrant update was compensated by restoring prior qdrant points for existing document"
        ));
        let joined = take_observability_profile_test_logs().join("\n");
        assert!(joined.contains("stage=index_project.qdrant_compensation_after_postgres_failure"));
        assert!(joined.contains("compensation_ok=true"));
        assert!(joined.contains("stage=index_project.qdrant_postgres_failure_verdict"));
        assert!(joined.contains("failure_mode=existing_document_compensated"));
        assert!(
            joined.contains("consistency_state=cross_store_consistency_restored_by_compensation")
        );
    }

    #[test]
    fn manual_recovery_bundle_write_failure_is_surfaced_in_error() {
        let _ = take_observability_profile_test_logs();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let temp_root =
            std::env::temp_dir().join(format!("amai-indexer-remediation-write-failure-{unique}"));
        fs::create_dir_all(&temp_root).expect("create temp root");
        let blocking_file = temp_root.join("not-a-directory");
        fs::write(&blocking_file, "x").expect("create blocking file");
        let remediation_dir_value = blocking_file.display().to_string();
        let _remediation_dir = ScopedEnvVar::set(
            "AMAI_QDRANT_POSTGRES_REMEDIATION_DIR",
            &remediation_dir_value,
        );
        let error = ReplaceDocumentIndexError {
            phase: ReplaceDocumentIndexErrorPhase::CommitOutcomeUnknown,
            sqlstate_code: Some("40003".to_string()),
            error: anyhow!("postgres commit outcome unknown"),
        };
        let returned = Runtime::new().expect("runtime").block_on(
            handle_replace_document_index_failure_after_qdrant_update(
                Path::new("."),
                "fixtures/runtime/001_window.md",
                Uuid::nil(),
                false,
                error,
                || async { Ok(()) },
            ),
        );
        let rendered = format!("{returned:#}");
        assert!(rendered.contains("remediation_bundle_write_failed="));
        assert!(!rendered.contains("remediation_bundle_path="));
        let joined = take_observability_profile_test_logs().join("\n");
        assert!(joined.contains("stage=index_project.qdrant_postgres_failure_verdict"));
        assert!(!joined.contains("stage=index_project.qdrant_postgres_manual_recovery_bundle"));
        fs::remove_dir_all(&temp_root).expect("cleanup temp root");
    }

    #[test]
    fn postgres_failure_without_qdrant_update_is_not_labeled_compensated() {
        let verdict = finalize_postgres_failure_without_qdrant_update(
            "fixtures/runtime/001_window.md",
            Uuid::nil(),
            ReplaceDocumentIndexErrorPhase::BeforeCommit,
            None,
            anyhow!("postgres write failed"),
        );
        assert_eq!(verdict.mode, QdrantPostgresFailureMode::BeforeQdrantUpdate);
        assert_eq!(
            verdict.failure_phase,
            ReplaceDocumentIndexErrorPhase::BeforeCommit
        );
        assert!(!verdict.compensation_attempted);
        assert_eq!(verdict.compensation_succeeded, None);
        let rendered = format!("{:#}", verdict.error);
        assert!(rendered.contains("postgres index failure before any qdrant update"));
        assert!(rendered.contains("postgres write failed"));
        assert!(!rendered.contains("was compensated"));
        assert!(!rendered.contains("inconsistent qdrant/postgres state"));
    }

    #[test]
    fn document_index_advisory_lock_key_is_stable_for_same_scope() {
        let namespace_id = Uuid::nil();
        let key1 = document_index_advisory_lock_key(namespace_id, "src/lib.rs");
        let key2 = document_index_advisory_lock_key(namespace_id, "src/lib.rs");
        assert_eq!(key1, key2);
    }

    #[test]
    fn document_index_advisory_lock_key_changes_with_scope() {
        let namespace_id = Uuid::nil();
        let key_a = document_index_advisory_lock_key(namespace_id, "src/lib.rs");
        let key_b = document_index_advisory_lock_key(namespace_id, "src/main.rs");
        let key_c = document_index_advisory_lock_key(Uuid::from_u128(1), "src/lib.rs");
        assert_ne!(key_a, key_b);
        assert_ne!(key_a, key_c);
    }
}
