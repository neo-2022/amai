use crate::benchmark_measured_approval;
use crate::benchmark_promotion;
use crate::benchmark_statistics;
use crate::bootstrap;
use crate::cli::{
    ContextPackArgs, ContinuityImportArgs, ContinuityStartupArgs,
    DEFAULT_CLI_CONTINUITY_STARTUP_TOKEN_SOURCE_KIND, VerifyMemoryMatrixArgs,
};
use crate::config::AppConfig;
use crate::continuity;
use crate::eval_verdict::{self, EvalPattern, EvalSignals};
use crate::postgres;
use crate::retrieval;
use crate::retrieval_science;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct MatrixRegistry {
    source: MatrixSource,
    matrices: BTreeMap<String, MatrixEntry>,
    tasks: BTreeMap<String, MatrixTask>,
}

static AGENT_SCOPE_ENV_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Deserialize)]
struct MatrixSource {
    display_name: String,
    summary: String,
    reference_urls: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MatrixEntry {
    display_name: String,
    summary: String,
    min_success_rate: f64,
    min_mean_score: f64,
    max_failures: usize,
    max_p95_ms: Option<f64>,
    task_codes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MatrixTask {
    order: u32,
    class: MemoryTaskClass,
    layer: MemoryLayer,
    eval_pattern: EvalPattern,
    display_name: String,
    kind: MemoryTaskKind,
    project: String,
    project_display_name: Option<String>,
    namespace: String,
    related_project: Option<String>,
    related_project_display_name: Option<String>,
    related_namespace: Option<String>,
    question: Option<String>,
    headline: Option<String>,
    next_step: Option<String>,
    updated_headline: Option<String>,
    updated_next_step: Option<String>,
    expected_answer: Option<String>,
    unexpected_answer: Option<String>,
    expected_error_contains: Option<String>,
    expected_operation_count: usize,
    agent_scope: Option<String>,
    isolated_agent_scope: Option<String>,
    #[serde(default)]
    bootstrap_lines: Vec<String>,
    #[serde(default)]
    details_lines: Vec<String>,
    #[serde(default)]
    updated_bootstrap_lines: Vec<String>,
    #[serde(default)]
    updated_details_lines: Vec<String>,
    #[serde(default)]
    related_bootstrap_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum MemoryTaskClass {
    Read,
    Write,
    Update,
    Isolation,
}

impl MemoryTaskClass {
    fn as_str(self) -> &'static str {
        match self {
            MemoryTaskClass::Read => "read",
            MemoryTaskClass::Write => "write",
            MemoryTaskClass::Update => "update",
            MemoryTaskClass::Isolation => "isolation",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum MemoryLayer {
    Core,
    Archival,
}

impl MemoryLayer {
    fn as_str(self) -> &'static str {
        match self {
            MemoryLayer::Core => "core",
            MemoryLayer::Archival => "archival",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MemoryTaskKind {
    CoreRead,
    CoreWrite,
    CoreUpdate,
    CoreScopeIsolation,
    ArchivalRead,
    ArchivalWrite,
    ArchivalUpdate,
    ArchivalProjectIsolation,
}

pub async fn run_matrix(cfg: &AppConfig, args: &VerifyMemoryMatrixArgs) -> Result<()> {
    let payload = collect_matrix(cfg, args).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    let gate_failures = gate_failures_from_payload(&payload);
    if !gate_failures.is_empty() {
        return Err(anyhow!(
            "memory task matrix failed gates: {}",
            gate_failures.join("; ")
        ));
    }
    Ok(())
}

pub async fn collect_matrix(cfg: &AppConfig, args: &VerifyMemoryMatrixArgs) -> Result<Value> {
    bootstrap::bootstrap_stack(cfg).await?;
    let registry = load_registry()?;
    let matrix = registry
        .matrices
        .get(&args.matrix)
        .ok_or_else(|| anyhow!("unknown memory task matrix: {}", args.matrix))?;
    let mut ordered_tasks = matrix
        .task_codes
        .iter()
        .map(|code| {
            registry
                .tasks
                .get(code)
                .cloned()
                .map(|task| (code.clone(), task))
                .ok_or_else(|| anyhow!("task {code} missing in memory task matrix registry"))
        })
        .collect::<Result<Vec<_>>>()?;
    ordered_tasks.sort_by_key(|(code, task)| (task.order, code.clone()));

    let temp_root =
        std::env::temp_dir().join(format!("amai-memory-matrix-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&temp_root)
        .with_context(|| format!("failed to create {}", temp_root.display()))?;
    let run_result = run_matrix_inner(
        cfg,
        args,
        &registry.source,
        matrix,
        &ordered_tasks,
        &temp_root,
    )
    .await;
    let cleanup_result = fs::remove_dir_all(&temp_root)
        .with_context(|| format!("failed to remove {}", temp_root.display()));
    let payload = run_result?;
    cleanup_result?;
    Ok(payload)
}

async fn run_matrix_inner(
    cfg: &AppConfig,
    args: &VerifyMemoryMatrixArgs,
    source: &MatrixSource,
    matrix: &MatrixEntry,
    ordered_tasks: &[(String, MatrixTask)],
    temp_root: &Path,
) -> Result<Value> {
    let mut db = postgres::connect_admin(cfg).await?;
    let matrix_run_id = Uuid::new_v4();
    let captured_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    let mut task_results = Vec::with_capacity(ordered_tasks.len());
    for (task_code, task) in ordered_tasks {
        task_results.push(run_task(cfg, &mut db, args, task_code, task, temp_root).await?);
    }

    let latencies = task_results
        .iter()
        .map(|task| task.latency_ms)
        .collect::<Vec<_>>();
    let scores = task_results
        .iter()
        .map(|task| task.score)
        .collect::<Vec<_>>();
    let tasks_total = task_results.len();
    let tasks_passed = task_results.iter().filter(|task| task.success).count();
    let tasks_failed = tasks_total.saturating_sub(tasks_passed);
    let success_rate = if tasks_total == 0 {
        0.0
    } else {
        tasks_passed as f64 / tasks_total as f64
    };
    let mean_score = mean_f64(&scores);
    let p50_ms = percentile_f64(&latencies, 50);
    let p95_ms = percentile_f64(&latencies, 95);
    let max_ms = latencies
        .iter()
        .copied()
        .max_by(f64::total_cmp)
        .unwrap_or_default();

    let mut class_breakdown = BTreeMap::<String, Value>::new();
    for class in [
        MemoryTaskClass::Read,
        MemoryTaskClass::Write,
        MemoryTaskClass::Update,
        MemoryTaskClass::Isolation,
    ] {
        let class_tasks = task_results
            .iter()
            .filter(|task| task.class == class)
            .collect::<Vec<_>>();
        if class_tasks.is_empty() {
            continue;
        }
        class_breakdown.insert(class.as_str().to_string(), summarize_subset(&class_tasks));
    }

    let mut layer_breakdown = BTreeMap::<String, Value>::new();
    for layer in [MemoryLayer::Core, MemoryLayer::Archival] {
        let layer_tasks = task_results
            .iter()
            .filter(|task| task.layer == layer)
            .collect::<Vec<_>>();
        if layer_tasks.is_empty() {
            continue;
        }
        layer_breakdown.insert(layer.as_str().to_string(), summarize_subset(&layer_tasks));
    }

    let required_success_rate = args.min_success_rate.unwrap_or(matrix.min_success_rate);
    let required_mean_score = args.min_mean_score.unwrap_or(matrix.min_mean_score);
    let required_max_p95_ms = args.max_p95_ms.or(matrix.max_p95_ms);
    let mut gate_failures = Vec::new();
    if success_rate < required_success_rate {
        gate_failures.push(format!(
            "success_rate={success_rate:.3} below required {required_success_rate:.3}"
        ));
    }
    if mean_score < required_mean_score {
        gate_failures.push(format!(
            "mean_score={mean_score:.3} below required {required_mean_score:.3}"
        ));
    }
    if tasks_failed > matrix.max_failures {
        gate_failures.push(format!(
            "tasks_failed={} exceeds allowed {}",
            tasks_failed, matrix.max_failures
        ));
    }
    if let Some(limit) = required_max_p95_ms
        && p95_ms > limit
    {
        gate_failures.push(format!("p95_ms={p95_ms:.3} exceeds allowed {limit:.3}"));
    }

    let baseline_snapshot = postgres::list_observability_snapshots_by_kind_for_scope(
        &db,
        "memory_task_matrix",
        "memory_task_matrix",
        "amai",
        &args.matrix,
        Some(1),
    )
    .await?
    .into_iter()
    .next();

    let mut payload = json!({
        "_observability": {
            "source_event_id": matrix_run_id,
            "source_kind": "memory_task_matrix_run",
            "scope_project_code": "amai",
            "scope_namespace_code": args.matrix,
            "captured_at_epoch_ms": captured_at_epoch_ms,
        },
        "memory_task_matrix": {
            "matrix": args.matrix,
            "display_name": matrix.display_name,
            "summary": matrix.summary,
            "retrieval_science": retrieval_science::suite_metadata("memory_task_matrix")?,
            "source": {
                "display_name": source.display_name,
                "summary": source.summary,
                "reference_urls": source.reference_urls,
            },
            "tasks_total": tasks_total,
            "tasks_passed": tasks_passed,
            "tasks_failed": tasks_failed,
            "success_rate": success_rate,
            "mean_score": mean_score,
            "p50_ms": p50_ms,
            "p95_ms": p95_ms,
            "max_ms": max_ms,
            "gate_failures": gate_failures,
            "class_breakdown": class_breakdown,
            "layer_breakdown": layer_breakdown,
            "canonical_eval": eval_verdict::summarize_eval_layer(
                task_results.iter().map(|task| task.eval_verdict_class.as_str())
            )?,
            "tasks": task_results.iter().map(TaskResult::as_json).collect::<Vec<_>>(),
        }
    });
    let statistics_gate_failures = payload["memory_task_matrix"]["gate_failures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let statistics = benchmark_statistics::statistics_block_from_pair(
        "memory_task_matrix",
        &payload,
        baseline_snapshot.as_ref().map(|record| &record.payload),
        matrix_run_id,
        &statistics_gate_failures,
    );
    payload["memory_task_matrix"]["statistics"] = statistics;
    payload["memory_task_matrix"]["promotion_law"] =
        benchmark_promotion::promotion_law_block("memory_task_matrix", &payload);
    payload["memory_task_matrix"]["measured_approval"] =
        benchmark_measured_approval::measured_approval_block("memory_task_matrix", &payload);
    let _ = postgres::insert_observability_snapshot(&db, "memory_task_matrix", &payload).await?;
    Ok(payload)
}

fn gate_failures_from_payload(payload: &Value) -> Vec<String> {
    payload["memory_task_matrix"]["gate_failures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect()
}

async fn run_task(
    cfg: &AppConfig,
    db: &mut tokio_postgres::Client,
    args: &VerifyMemoryMatrixArgs,
    task_code: &str,
    task: &MatrixTask,
    temp_root: &Path,
) -> Result<TaskResult> {
    let started = Instant::now();
    let task_project = format!("{}_{}", args.project_prefix, task.project);
    let task_display_name = task
        .project_display_name
        .clone()
        .unwrap_or_else(|| titleize(&task_project));
    let details = match task.kind {
        MemoryTaskKind::CoreRead => {
            run_core_read_task(cfg, db, task, &task_project, &task_display_name, temp_root).await?
        }
        MemoryTaskKind::CoreWrite => {
            run_core_write_task(cfg, db, task, &task_project, &task_display_name, temp_root).await?
        }
        MemoryTaskKind::CoreUpdate => {
            run_core_update_task(cfg, db, task, &task_project, &task_display_name, temp_root)
                .await?
        }
        MemoryTaskKind::CoreScopeIsolation => {
            run_core_scope_isolation_task(
                cfg,
                db,
                task,
                &task_project,
                &task_display_name,
                temp_root,
            )
            .await?
        }
        MemoryTaskKind::ArchivalRead => {
            run_archival_read_task(cfg, task, &task_project, &task_display_name, temp_root).await?
        }
        MemoryTaskKind::ArchivalWrite => {
            run_archival_write_task(cfg, task, &task_project, &task_display_name, temp_root).await?
        }
        MemoryTaskKind::ArchivalUpdate => {
            run_archival_update_task(cfg, task, &task_project, &task_display_name, temp_root)
                .await?
        }
        MemoryTaskKind::ArchivalProjectIsolation => {
            run_archival_project_isolation_task(
                cfg,
                task,
                &task_project,
                &task_display_name,
                temp_root,
            )
            .await?
        }
    };
    let actual_operation_count = details["actual_operation_count"]
        .as_u64()
        .unwrap_or_default() as usize;
    let answer_ok = details["answer_ok"].as_bool().unwrap_or(false);
    let penalty_points =
        actual_operation_count.saturating_sub(task.expected_operation_count) as u64;
    let score = if answer_ok {
        (1.0 - 0.1 * penalty_points as f64).max(0.0)
    } else {
        0.0
    };
    let success = answer_ok && penalty_points == 0;
    let eval = eval_verdict::derive_eval_verdict(
        task.eval_pattern,
        &EvalSignals::from_details(&details, task.expected_answer.is_some()),
    )?;
    Ok(TaskResult {
        code: task_code.to_string(),
        class: task.class,
        layer: task.layer,
        display_name: task.display_name.clone(),
        kind: format!("{:?}", task.kind),
        success,
        score,
        latency_ms: started.elapsed().as_secs_f64() * 1000.0,
        expected_operation_count: task.expected_operation_count,
        actual_operation_count,
        penalty_points,
        eval_verdict_class: eval.class_key,
        eval_reason: eval.reason,
        details,
    })
}

async fn run_core_read_task(
    cfg: &AppConfig,
    db: &mut tokio_postgres::Client,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    ensure_namespace(db, cfg, project, display_name, &task.namespace, temp_root).await?;
    run_core_handoff_fast(
        db,
        project,
        &task.namespace,
        task.headline.as_deref().unwrap_or("Core memory read"),
        task.next_step
            .as_deref()
            .unwrap_or("Answer from core memory without archival lookup."),
        &task.details_lines,
        task.agent_scope.as_deref(),
        temp_root,
    )
    .await?;
    let restore =
        run_restore_json(db, project, &task.namespace, task.agent_scope.as_deref()).await?;
    let answer_ok = value_contains(
        &restore["working_state_restore"]["materialized_notes"],
        task.expected_answer.as_deref().unwrap_or_default(),
    );
    Ok(json!({
        "answer_ok": answer_ok,
        "expected_present": answer_ok,
        "unexpected_present": false,
        "recovered_state": true,
        "actual_operation_count": 3,
        "expected_answer": task.expected_answer,
        "materialized_notes": restore["working_state_restore"]["materialized_notes"].clone(),
        "restore_confidence": restore["working_state_restore"]["restore_confidence"].clone(),
    }))
}

async fn run_core_write_task(
    cfg: &AppConfig,
    db: &mut tokio_postgres::Client,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    ensure_namespace(db, cfg, project, display_name, &task.namespace, temp_root).await?;
    run_core_handoff_fast(
        db,
        project,
        &task.namespace,
        task.headline.as_deref().unwrap_or("Core memory write"),
        task.next_step
            .as_deref()
            .unwrap_or("Persist the new fact into core memory."),
        &task.details_lines,
        task.agent_scope.as_deref(),
        temp_root,
    )
    .await?;
    let restore =
        run_restore_json(db, project, &task.namespace, task.agent_scope.as_deref()).await?;
    let answer_ok = value_contains(
        &restore["chat_start_restore"]["prompt_text"],
        task.expected_answer.as_deref().unwrap_or_default(),
    );
    Ok(json!({
        "answer_ok": answer_ok,
        "expected_present": answer_ok,
        "unexpected_present": false,
        "recovered_state": true,
        "actual_operation_count": 3,
        "expected_answer": task.expected_answer,
        "prompt_text_excerpt": first_chars(restore["chat_start_restore"]["prompt_text"].as_str().unwrap_or_default(), 220),
    }))
}

async fn run_core_update_task(
    cfg: &AppConfig,
    db: &mut tokio_postgres::Client,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    ensure_namespace(db, cfg, project, display_name, &task.namespace, temp_root).await?;
    run_core_handoff_fast(
        db,
        project,
        &task.namespace,
        task.headline
            .as_deref()
            .unwrap_or("Core memory update before"),
        task.next_step
            .as_deref()
            .unwrap_or("This is the stale core-memory version."),
        &task.details_lines,
        task.agent_scope.as_deref(),
        temp_root,
    )
    .await?;
    run_core_handoff_fast(
        db,
        project,
        &task.namespace,
        task.updated_headline
            .as_deref()
            .unwrap_or("Core memory update after"),
        task.updated_next_step
            .as_deref()
            .unwrap_or("This is the current core-memory version."),
        &task.updated_details_lines,
        task.agent_scope.as_deref(),
        temp_root,
    )
    .await?;
    let restore =
        run_restore_json(db, project, &task.namespace, task.agent_scope.as_deref()).await?;
    let current_goal = restore["working_state_restore"]["current_goal"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let expected_present =
        current_goal.contains(task.expected_answer.as_deref().unwrap_or_default());
    let unexpected_present = task
        .unexpected_answer
        .as_deref()
        .is_some_and(|unexpected| current_goal.contains(unexpected));
    let answer_ok = expected_present && !unexpected_present;
    Ok(json!({
        "answer_ok": answer_ok,
        "expected_present": expected_present,
        "unexpected_present": unexpected_present,
        "recovered_state": true,
        "actual_operation_count": 4,
        "expected_answer": task.expected_answer,
        "unexpected_answer": task.unexpected_answer,
        "current_goal": current_goal,
    }))
}

async fn run_core_scope_isolation_task(
    cfg: &AppConfig,
    db: &mut tokio_postgres::Client,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    ensure_namespace(db, cfg, project, display_name, &task.namespace, temp_root).await?;
    let owner_scope = task.agent_scope.as_deref().unwrap_or("shared");
    let isolated_scope = task
        .isolated_agent_scope
        .as_deref()
        .ok_or_else(|| anyhow!("core scope isolation task requires isolated_agent_scope"))?;
    run_core_handoff_fast(
        db,
        project,
        &task.namespace,
        task.headline.as_deref().unwrap_or("Core scope isolation"),
        task.next_step
            .as_deref()
            .unwrap_or("Owner scope should recover this fact."),
        &task.details_lines,
        Some(owner_scope),
        temp_root,
    )
    .await?;
    let owner_restore = run_restore_json(db, project, &task.namespace, Some(owner_scope)).await?;
    let owner_ok = value_contains(
        &owner_restore["working_state_restore"]["materialized_notes"],
        task.expected_answer.as_deref().unwrap_or_default(),
    );
    let isolated = run_restore_error(db, project, &task.namespace, Some(isolated_scope)).await?;
    let expected_error = task
        .expected_error_contains
        .as_deref()
        .unwrap_or("no working-state restore bundle found");
    let fail_closed_ok = isolated.contains(expected_error);
    let answer_ok = owner_ok && fail_closed_ok;
    Ok(json!({
        "answer_ok": answer_ok,
        "expected_present": owner_ok,
        "unexpected_present": false,
        "boundary_clean": fail_closed_ok,
        "fail_closed_ok": fail_closed_ok,
        "actual_operation_count": 4,
        "owner_scope": owner_scope,
        "isolated_scope": isolated_scope,
        "owner_materialized_notes": owner_restore["working_state_restore"]["materialized_notes"].clone(),
        "isolated_error": isolated.trim(),
    }))
}

async fn run_archival_read_task(
    cfg: &AppConfig,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    import_bootstrap(
        cfg,
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    let response = run_context_pack_json(
        cfg,
        project,
        &task.namespace,
        task.question.as_deref().unwrap_or_default(),
    )
    .await?;
    let answer_ok = value_contains(
        &response,
        task.expected_answer.as_deref().unwrap_or_default(),
    );
    Ok(json!({
        "answer_ok": answer_ok,
        "expected_present": answer_ok,
        "unexpected_present": false,
        "recovered_state": false,
        "actual_operation_count": 2,
        "expected_answer": task.expected_answer,
        "visible_projects": response["visible_projects"].clone(),
    }))
}

async fn run_archival_write_task(
    cfg: &AppConfig,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    import_bootstrap(
        cfg,
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    let response = run_context_pack_json(
        cfg,
        project,
        &task.namespace,
        task.question.as_deref().unwrap_or_default(),
    )
    .await?;
    let answer_ok = value_contains(
        &response,
        task.expected_answer.as_deref().unwrap_or_default(),
    );
    Ok(json!({
        "answer_ok": answer_ok,
        "expected_present": answer_ok,
        "unexpected_present": false,
        "recovered_state": false,
        "actual_operation_count": 2,
        "expected_answer": task.expected_answer,
        "retrieval_runtime_ms": response["retrieval_runtime"]["total_ms"].clone(),
    }))
}

async fn run_archival_update_task(
    cfg: &AppConfig,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    import_bootstrap(
        cfg,
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    import_bootstrap(
        cfg,
        project,
        display_name,
        &task.namespace,
        &task.updated_bootstrap_lines,
        temp_root,
    )
    .await?;
    let response = run_context_pack_json(
        cfg,
        project,
        &task.namespace,
        task.question.as_deref().unwrap_or_default(),
    )
    .await?;
    let expected = task.expected_answer.as_deref().unwrap_or_default();
    let expected_present = value_contains(&response, expected);
    let unexpected_present = task
        .unexpected_answer
        .as_deref()
        .is_some_and(|unexpected| value_contains(&response, unexpected));
    let answer_ok = expected_present && !unexpected_present;
    Ok(json!({
        "answer_ok": answer_ok,
        "expected_present": expected_present,
        "unexpected_present": unexpected_present,
        "recovered_state": true,
        "actual_operation_count": 3,
        "expected_answer": task.expected_answer,
        "unexpected_answer": task.unexpected_answer,
    }))
}

async fn run_archival_project_isolation_task(
    cfg: &AppConfig,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    let related_project = task
        .related_project
        .as_deref()
        .ok_or_else(|| anyhow!("archival project isolation task requires related_project"))?;
    let related_display_name = task
        .related_project_display_name
        .clone()
        .unwrap_or_else(|| titleize(related_project));
    let related_namespace = task.related_namespace.as_deref().unwrap_or(&task.namespace);
    let related_project_code = format!("{}_{}", project, related_project);
    import_bootstrap(
        cfg,
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    import_bootstrap(
        cfg,
        &related_project_code,
        &related_display_name,
        related_namespace,
        &task.related_bootstrap_lines,
        temp_root,
    )
    .await?;
    let response = run_context_pack_json(
        cfg,
        &related_project_code,
        related_namespace,
        task.question.as_deref().unwrap_or_default(),
    )
    .await?;
    let leaked = task
        .unexpected_answer
        .as_deref()
        .is_some_and(|unexpected| value_contains(&response, unexpected));
    let visible_projects = collect_visible_projects(&response["visible_projects"]);
    let boundary_clean = !leaked && !visible_projects.iter().any(|item| item == project);
    let answer_ok = boundary_clean;
    Ok(json!({
        "answer_ok": answer_ok,
        "expected_present": false,
        "unexpected_present": leaked,
        "boundary_clean": boundary_clean,
        "fail_closed_ok": boundary_clean,
        "actual_operation_count": 3,
        "unexpected_answer": task.unexpected_answer,
        "visible_projects": visible_projects,
    }))
}

async fn ensure_namespace(
    db: &tokio_postgres::Client,
    cfg: &AppConfig,
    project: &str,
    display_name: &str,
    namespace: &str,
    temp_root: &Path,
) -> Result<()> {
    let repo_root = ensure_project_repo_root(temp_root, project)?;
    let project = postgres::upsert_project(
        db,
        project,
        display_name,
        &repo_root.display().to_string(),
        Some("main"),
        "default",
        "project_shared",
        &cfg.default_retrieval_mode,
    )
    .await?;
    let _ = postgres::ensure_namespace(
        db,
        project.project_id,
        namespace,
        Some("Memory Eval"),
        "local_strict",
    )
    .await?;
    Ok(())
}

async fn import_bootstrap(
    cfg: &AppConfig,
    project: &str,
    display_name: &str,
    namespace: &str,
    lines: &[String],
    temp_root: &Path,
) -> Result<()> {
    let path = write_temp_markdown("memory-bootstrap", lines)?;
    let repo_root = ensure_project_repo_root(temp_root, project)?;
    let args = ContinuityImportArgs {
        project: project.to_string(),
        display_name: display_name.to_string(),
        repo_root,
        namespace: namespace.to_string(),
        bootstrap_file: path.clone(),
        thread_index_file: None,
        active_workline_file: None,
        memory_dir: None,
        transcript_limit: Some(0),
    };
    let import_result = continuity::import_sources_payload(cfg, &args).await;
    let cleanup_result =
        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()));
    import_result?;
    cleanup_result?;
    Ok(())
}

fn ensure_project_repo_root(temp_root: &Path, project: &str) -> Result<PathBuf> {
    let repo_root = temp_root.join("projects").join(project);
    fs::create_dir_all(&repo_root)
        .with_context(|| format!("failed to create {}", repo_root.display()))?;
    Ok(repo_root)
}

async fn run_core_handoff_fast(
    db: &mut tokio_postgres::Client,
    project: &str,
    namespace: &str,
    headline: &str,
    next_step: &str,
    details_lines: &[String],
    agent_scope: Option<&str>,
    temp_root: &Path,
) -> Result<()> {
    let details = render_lines(details_lines);
    let project_record = postgres::get_project_by_code(db, project).await?;
    let namespace_record =
        postgres::get_namespace_by_code(db, project_record.project_id, namespace).await?;
    let local_path = ensure_project_repo_root(temp_root, project)?.join("live-handoff.md");
    let local_body = format!("# {headline}\n\n{next_step}\n\n{details}");
    fs::write(&local_path, local_body)
        .with_context(|| format!("failed to write {}", local_path.display()))?;
    let captured_at_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    let payload = json!({
        "continuity_handoff": {
            "project": {
                "code": project_record.code.clone(),
                "display_name": project_record.display_name.clone(),
                "repo_root": project_record.repo_root.clone(),
            },
            "namespace": {
                "code": namespace_record.code.clone(),
                "display_name": namespace_record.display_name.clone(),
            },
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "headline": headline,
            "next_step": next_step,
            "details": details,
            "resolve_current_goal": false,
            "resolved_pending_return_headlines": Vec::<String>::new(),
            "resolved_pending_return_task_ids": Vec::<String>::new(),
            "relative_path": ".amai-continuity/live-handoff/HANDOFF.md",
            "local_path": local_path.display().to_string(),
            "document_index_refresh_performed": false,
        }
    });
    let _ = postgres::insert_observability_snapshot(db, "continuity_handoff", &payload).await?;
    let event_agent_scope = agent_scope.unwrap_or("shared");
    let materialized_notes = details_lines
        .iter()
        .map(|line| line.trim().trim_start_matches('-').trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let working_state_payload = json!({
        "working_state_event": {
            "event_id": Uuid::new_v4().to_string(),
            "project": {
                "code": project_record.code,
                "display_name": project_record.display_name,
                "repo_root": project_record.repo_root,
            },
            "namespace": {
                "code": namespace_record.code,
                "display_name": namespace_record.display_name,
            },
            "recorded_at_epoch_ms": captured_at_epoch_ms,
            "event_kind": "continuity_handoff",
            "session_id": format!("memory-matrix::{project}::{namespace}::{event_agent_scope}"),
            "agent_scope": event_agent_scope,
            "thread_id": Value::Null,
            "source_kind": "continuity_handoff",
            "headline": headline,
            "next_step_hint": next_step,
            "summary": format!("{headline} -> {next_step}"),
            "active_files": Vec::<String>::new(),
            "recent_paths": Vec::<String>::new(),
            "visible_projects": vec![project.to_string()],
            "query": Value::Null,
            "query_type": Value::Null,
            "target_kind": "handoff",
            "current_hypothesis": Value::Null,
            "rejected_hypotheses": Vec::<String>::new(),
            "open_questions": Vec::<String>::new(),
            "materialized_notes": materialized_notes,
            "pending_return_queue": Value::Null,
            "client_budget_target_percent": Value::Null,
            "resolve_current_goal": false,
            "resolved_pending_return_headlines": Vec::<String>::new(),
            "resolved_pending_return_task_ids": Vec::<String>::new(),
            "last_command": "memory_task_matrix core handoff",
            "last_results_summary": format!("Synthetic core handoff for {project} :: {namespace}"),
            "local_path": local_path.display().to_string(),
        }
    });
    let _ =
        postgres::insert_observability_snapshot(db, "working_state_event", &working_state_payload)
            .await?;
    Ok(())
}

async fn run_restore_json(
    db: &tokio_postgres::Client,
    project: &str,
    namespace: &str,
    agent_scope: Option<&str>,
) -> Result<Value> {
    let args = synthetic_memory_matrix_restore_args(project, namespace);
    with_agent_scope_env(agent_scope, continuity::restore_payload_with_db(db, &args)).await
}

async fn run_restore_error(
    db: &tokio_postgres::Client,
    project: &str,
    namespace: &str,
    agent_scope: Option<&str>,
) -> Result<String> {
    let args = synthetic_memory_matrix_restore_args(project, namespace);
    let result =
        with_agent_scope_env(agent_scope, continuity::restore_payload_with_db(db, &args)).await;
    match result {
        Ok(_) => Err(anyhow!("continuity restore unexpectedly succeeded")),
        Err(error) => Ok(format!("{error:#}")),
    }
}

fn synthetic_memory_matrix_restore_args(project: &str, namespace: &str) -> ContinuityStartupArgs {
    ContinuityStartupArgs {
        project: Some(project.to_string()),
        repo_root: None,
        namespace: namespace.to_string(),
        json: false,
        runtime_state_json: false,
        token_source_kind: DEFAULT_CLI_CONTINUITY_STARTUP_TOKEN_SOURCE_KIND.to_string(),
        skip_live_client_budget_guard: true,
    }
}

async fn with_agent_scope_env<T, F>(agent_scope: Option<&str>, future: F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    let env_mutex = AGENT_SCOPE_ENV_MUTEX.get_or_init(|| Mutex::new(()));
    let _env_guard = env_mutex.lock().await;
    let previous = std::env::var_os("AMAI_AGENT_SCOPE");
    set_agent_scope_env(agent_scope);
    let result = future.await;
    restore_agent_scope_env(previous);
    result
}

fn set_agent_scope_env(agent_scope: Option<&str>) {
    match agent_scope {
        Some(agent_scope) => {
            // SAFETY: process environment mutation is serialized by AGENT_SCOPE_ENV_MUTEX,
            // and this benchmark helper restores the previous value before releasing the lock.
            unsafe { std::env::set_var("AMAI_AGENT_SCOPE", agent_scope) };
        }
        None => {
            // SAFETY: process environment mutation is serialized by AGENT_SCOPE_ENV_MUTEX,
            // and this benchmark helper restores the previous value before releasing the lock.
            unsafe { std::env::remove_var("AMAI_AGENT_SCOPE") };
        }
    }
}

fn restore_agent_scope_env(previous: Option<OsString>) {
    match previous {
        Some(previous) => {
            // SAFETY: process environment mutation is serialized by AGENT_SCOPE_ENV_MUTEX,
            // and this restores the exact pre-call value for AMAI_AGENT_SCOPE.
            unsafe { std::env::set_var("AMAI_AGENT_SCOPE", previous) };
        }
        None => {
            // SAFETY: process environment mutation is serialized by AGENT_SCOPE_ENV_MUTEX,
            // and this restores the exact pre-call absence for AMAI_AGENT_SCOPE.
            unsafe { std::env::remove_var("AMAI_AGENT_SCOPE") };
        }
    }
}

async fn run_context_pack_json(
    cfg: &AppConfig,
    project: &str,
    namespace: &str,
    question: &str,
) -> Result<Value> {
    let mut db = postgres::connect_admin(cfg).await?;
    let args = ContextPackArgs {
        project: project.to_string(),
        namespace: namespace.to_string(),
        query: question.to_string(),
        retrieval_mode: Some("local_strict".to_string()),
        disable_cache: true,
        limit_documents: 5,
        limit_symbols: 8,
        limit_chunks: 8,
        limit_semantic_chunks: 8,
        at_epoch_ms: None,
        token_source_kind: "verify_memory_matrix_context_pack".to_string(),
        client_prompt_tokens: None,
        assistant_generation_tokens: None,
        tool_overhead_tokens: None,
        continuity_restore_tokens: None,
    };
    let result = retrieval::execute_context_pack_capture(cfg, &mut db, &args, true).await?;
    Ok(result.payload)
}

fn summarize_subset(tasks: &[&TaskResult]) -> Value {
    let tasks_total = tasks.len();
    let tasks_passed = tasks.iter().filter(|task| task.success).count();
    let mean_score = if tasks_total == 0 {
        0.0
    } else {
        tasks.iter().map(|task| task.score).sum::<f64>() / tasks_total as f64
    };
    json!({
        "tasks_total": tasks_total,
        "tasks_passed": tasks_passed,
        "tasks_failed": tasks_total.saturating_sub(tasks_passed),
        "success_rate": if tasks_total == 0 { 0.0 } else { tasks_passed as f64 / tasks_total as f64 },
        "mean_score": mean_score,
    })
}

fn mean_f64(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    samples.iter().sum::<f64>() / samples.len() as f64
}

fn percentile_f64(samples: &[f64], percentile: usize) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let percentile = percentile.min(100);
    let rank = (percentile * sorted.len()).div_ceil(100);
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}

fn value_contains(value: &Value, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    serde_json::to_string(value)
        .map(|rendered| rendered.contains(needle))
        .unwrap_or(false)
}

fn collect_visible_projects(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["project_code"].as_str().map(ToOwned::to_owned))
        .collect()
}

fn write_temp_markdown(prefix: &str, lines: &[String]) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("{prefix}-{}.md", Uuid::new_v4().simple()));
    fs::write(&path, render_lines(lines))
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn render_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn first_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        format!("{}...", value.chars().take(max_chars).collect::<String>())
    }
}

fn titleize(value: &str) -> String {
    value.replace('_', " ")
}

fn load_registry() -> Result<MatrixRegistry> {
    let path = registry_path();
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read memory task matrix {}", path.display()))?;
    toml::from_str(&content).context("failed to parse memory task matrix")
}

fn registry_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("config/memory_task_matrix.toml")
}

struct TaskResult {
    code: String,
    class: MemoryTaskClass,
    layer: MemoryLayer,
    display_name: String,
    kind: String,
    success: bool,
    score: f64,
    latency_ms: f64,
    expected_operation_count: usize,
    actual_operation_count: usize,
    penalty_points: u64,
    eval_verdict_class: String,
    eval_reason: String,
    details: Value,
}

impl TaskResult {
    fn as_json(&self) -> Value {
        json!({
            "code": self.code,
            "class": self.class.as_str(),
            "layer": self.layer.as_str(),
            "display_name": self.display_name,
            "kind": self.kind,
            "success": self.success,
            "score": self.score,
            "latency_ms": self.latency_ms,
            "expected_operation_count": self.expected_operation_count,
            "actual_operation_count": self.actual_operation_count,
            "penalty_points": self.penalty_points,
            "eval_verdict_class": self.eval_verdict_class,
            "eval_reason": self.eval_reason,
            "details": self.details,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MatrixTask, MemoryLayer, MemoryTaskClass, gate_failures_from_payload, load_registry,
    };
    use crate::eval_verdict::EvalPattern;
    use crate::eval_verdict::{EvalSignals, derive_eval_verdict};
    use serde_json::json;

    #[test]
    fn registry_loads_with_required_matrix() {
        let registry = load_registry().expect("registry should load");
        assert!(registry.matrices.contains_key("letta_memory_local"));
        assert!(registry.tasks.contains_key("core_memory_read"));
        assert!(registry.tasks.contains_key("archival_project_isolation"));
    }

    #[test]
    fn registry_covers_core_and_archival_layers() {
        let registry = load_registry().expect("registry should load");
        let mut has_core = false;
        let mut has_archival = false;
        let mut has_isolation = false;
        for task in registry.tasks.values() {
            has_core |= task.layer == MemoryLayer::Core;
            has_archival |= task.layer == MemoryLayer::Archival;
            has_isolation |= task.class == MemoryTaskClass::Isolation;
        }
        assert!(has_core);
        assert!(has_archival);
        assert!(has_isolation);
    }

    fn fake_task(eval_pattern: EvalPattern) -> MatrixTask {
        MatrixTask {
            order: 1,
            class: MemoryTaskClass::Read,
            layer: MemoryLayer::Core,
            eval_pattern,
            display_name: "fake".to_string(),
            kind: super::MemoryTaskKind::CoreRead,
            project: "fake".to_string(),
            project_display_name: None,
            namespace: "fake".to_string(),
            related_project: None,
            related_project_display_name: None,
            related_namespace: None,
            question: None,
            headline: None,
            next_step: None,
            updated_headline: None,
            updated_next_step: None,
            expected_answer: Some("expected".to_string()),
            unexpected_answer: Some("stale".to_string()),
            expected_error_contains: None,
            expected_operation_count: 1,
            agent_scope: None,
            isolated_agent_scope: None,
            bootstrap_lines: Vec::new(),
            details_lines: Vec::new(),
            updated_bootstrap_lines: Vec::new(),
            updated_details_lines: Vec::new(),
            related_bootstrap_lines: Vec::new(),
        }
    }

    #[test]
    fn recovery_pattern_marks_stale_and_overincluded_distinctly() {
        let task = fake_task(EvalPattern::RecoveryTarget);
        let stale = derive_eval_verdict(
            task.eval_pattern,
            &EvalSignals::from_details(
                &json!({"expected_present": false, "unexpected_present": true}),
                task.expected_answer.is_some(),
            ),
        )
        .expect("verdict");
        assert_eq!(stale.class_key, "stale_target");

        let noisy = derive_eval_verdict(
            task.eval_pattern,
            &EvalSignals::from_details(
                &json!({"expected_present": true, "unexpected_present": true}),
                task.expected_answer.is_some(),
            ),
        )
        .expect("verdict");
        assert_eq!(noisy.class_key, "over_included");
    }

    #[test]
    fn isolation_pattern_marks_clean_boundary_as_correct_target() {
        let task = fake_task(EvalPattern::IsolationBoundary);
        let verdict = derive_eval_verdict(
            task.eval_pattern,
            &EvalSignals::from_details(
                &json!({
                    "expected_present": true,
                    "unexpected_present": false,
                    "boundary_clean": true,
                    "fail_closed_ok": true
                }),
                task.expected_answer.is_some(),
            ),
        )
        .expect("verdict");
        assert_eq!(verdict.class_key, "hit_correct_target");
    }

    #[test]
    fn gate_failures_are_collected_from_payload() {
        let payload = json!({
            "memory_task_matrix": {
                "gate_failures": [
                    "success_rate=0.5 below required 1.0",
                    "mean_score=0.8 below required 1.0"
                ]
            }
        });
        assert_eq!(
            gate_failures_from_payload(&payload),
            vec![
                "success_rate=0.5 below required 1.0".to_string(),
                "mean_score=0.8 below required 1.0".to_string()
            ]
        );
    }

    #[test]
    fn synthetic_memory_matrix_restore_args_skip_live_client_budget_guard() {
        let args = super::synthetic_memory_matrix_restore_args("proj", "ns");
        assert_eq!(args.project.as_deref(), Some("proj"));
        assert_eq!(args.namespace, "ns");
        assert!(args.skip_live_client_budget_guard);
        assert_eq!(
            args.token_source_kind,
            super::DEFAULT_CLI_CONTINUITY_STARTUP_TOKEN_SOURCE_KIND
        );
    }
}
