use crate::bootstrap;
use crate::cli::VerifyMemoryMatrixArgs;
use crate::config::AppConfig;
use crate::eval_verdict::{self, EvalPattern, EvalSignals};
use crate::postgres;
use crate::retrieval_science;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::process::Command as ProcessCommand;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct MatrixRegistry {
    source: MatrixSource,
    matrices: BTreeMap<String, MatrixEntry>,
    tasks: BTreeMap<String, MatrixTask>,
}

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
    let db = postgres::connect_admin(cfg).await?;
    let mut task_results = Vec::with_capacity(ordered_tasks.len());
    for (task_code, task) in ordered_tasks {
        task_results.push(run_task(cfg, &db, args, task_code, task, temp_root).await?);
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

    let payload = json!({
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
    db: &tokio_postgres::Client,
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
    db: &tokio_postgres::Client,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    ensure_namespace(db, cfg, project, display_name, &task.namespace, temp_root).await?;
    import_bootstrap(
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    run_handoff_cli(
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
    let restore = run_restore_json(project, &task.namespace, task.agent_scope.as_deref()).await?;
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
    db: &tokio_postgres::Client,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    ensure_namespace(db, cfg, project, display_name, &task.namespace, temp_root).await?;
    import_bootstrap(
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    run_handoff_cli(
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
    let restore = run_restore_json(project, &task.namespace, task.agent_scope.as_deref()).await?;
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
    db: &tokio_postgres::Client,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    ensure_namespace(db, cfg, project, display_name, &task.namespace, temp_root).await?;
    import_bootstrap(
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    run_handoff_cli(
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
    run_handoff_cli(
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
    let restore = run_restore_json(project, &task.namespace, task.agent_scope.as_deref()).await?;
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
    db: &tokio_postgres::Client,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    ensure_namespace(db, cfg, project, display_name, &task.namespace, temp_root).await?;
    import_bootstrap(
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    let owner_scope = task.agent_scope.as_deref().unwrap_or("shared");
    let isolated_scope = task
        .isolated_agent_scope
        .as_deref()
        .ok_or_else(|| anyhow!("core scope isolation task requires isolated_agent_scope"))?;
    run_handoff_cli(
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
    let owner_restore = run_restore_json(project, &task.namespace, Some(owner_scope)).await?;
    let owner_ok = value_contains(
        &owner_restore["working_state_restore"]["materialized_notes"],
        task.expected_answer.as_deref().unwrap_or_default(),
    );
    let isolated = run_restore_error(project, &task.namespace, Some(isolated_scope)).await?;
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
    _cfg: &AppConfig,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    import_bootstrap(
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    let response = run_context_pack_json(
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
    _cfg: &AppConfig,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    import_bootstrap(
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    let response = run_context_pack_json(
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
    _cfg: &AppConfig,
    task: &MatrixTask,
    project: &str,
    display_name: &str,
    temp_root: &Path,
) -> Result<Value> {
    import_bootstrap(
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    import_bootstrap(
        project,
        display_name,
        &task.namespace,
        &task.updated_bootstrap_lines,
        temp_root,
    )
    .await?;
    let response = run_context_pack_json(
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
    _cfg: &AppConfig,
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
        project,
        display_name,
        &task.namespace,
        &task.bootstrap_lines,
        temp_root,
    )
    .await?;
    import_bootstrap(
        &related_project_code,
        &related_display_name,
        related_namespace,
        &task.related_bootstrap_lines,
        temp_root,
    )
    .await?;
    let response = run_context_pack_json(
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
    project: &str,
    display_name: &str,
    namespace: &str,
    lines: &[String],
    temp_root: &Path,
) -> Result<()> {
    let path = write_temp_markdown("memory-bootstrap", lines)?;
    let repo_root = ensure_project_repo_root(temp_root, project)?;
    let output = ProcessCommand::new(std::env::current_exe()?)
        .arg("continuity")
        .arg("import")
        .arg("--project")
        .arg(project)
        .arg("--display-name")
        .arg(display_name)
        .arg("--repo-root")
        .arg(&repo_root)
        .arg("--namespace")
        .arg(namespace)
        .arg("--bootstrap-file")
        .arg(&path)
        .arg("--transcript-limit")
        .arg("0")
        .output()
        .await
        .context("failed to run continuity import command")?;
    let cleanup_result =
        fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()));
    if !output.status.success() {
        return Err(anyhow!(
            "continuity import failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    cleanup_result?;
    Ok(())
}

fn ensure_project_repo_root(temp_root: &Path, project: &str) -> Result<PathBuf> {
    let repo_root = temp_root.join("projects").join(project);
    fs::create_dir_all(&repo_root)
        .with_context(|| format!("failed to create {}", repo_root.display()))?;
    Ok(repo_root)
}

async fn run_handoff_cli(
    project: &str,
    namespace: &str,
    headline: &str,
    next_step: &str,
    details_lines: &[String],
    agent_scope: Option<&str>,
    temp_root: &Path,
) -> Result<()> {
    let details_path = temp_root.join(format!("handoff-{}.md", Uuid::new_v4().simple()));
    if !details_lines.is_empty() {
        fs::write(&details_path, render_lines(details_lines))
            .with_context(|| format!("failed to write {}", details_path.display()))?;
    }
    let mut command = ProcessCommand::new(std::env::current_exe()?);
    command
        .arg("continuity")
        .arg("handoff")
        .arg("--project")
        .arg(project)
        .arg("--namespace")
        .arg(namespace)
        .arg("--headline")
        .arg(headline)
        .arg("--next-step")
        .arg(next_step);
    if !details_lines.is_empty() {
        command.arg("--details-file").arg(&details_path);
    }
    if let Some(agent_scope) = agent_scope {
        command.env("AMAI_AGENT_SCOPE", agent_scope);
    }
    let output = command
        .output()
        .await
        .context("failed to run continuity handoff command")?;
    if details_path.exists() {
        let _ = fs::remove_file(&details_path);
    }
    if !output.status.success() {
        return Err(anyhow!(
            "continuity handoff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

async fn run_restore_json(
    project: &str,
    namespace: &str,
    agent_scope: Option<&str>,
) -> Result<Value> {
    let mut command = ProcessCommand::new(std::env::current_exe()?);
    command
        .arg("continuity")
        .arg("restore")
        .arg("--project")
        .arg(project)
        .arg("--namespace")
        .arg(namespace);
    if let Some(agent_scope) = agent_scope {
        command.env("AMAI_AGENT_SCOPE", agent_scope);
    }
    let output = command
        .output()
        .await
        .context("failed to run continuity restore command")?;
    if !output.status.success() {
        return Err(anyhow!(
            "continuity restore failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).context("continuity restore did not return valid JSON")
}

async fn run_restore_error(
    project: &str,
    namespace: &str,
    agent_scope: Option<&str>,
) -> Result<String> {
    let mut command = ProcessCommand::new(std::env::current_exe()?);
    command
        .arg("continuity")
        .arg("restore")
        .arg("--project")
        .arg(project)
        .arg("--namespace")
        .arg(namespace);
    if let Some(agent_scope) = agent_scope {
        command.env("AMAI_AGENT_SCOPE", agent_scope);
    }
    let output = command
        .output()
        .await
        .context("failed to run continuity restore command")?;
    if output.status.success() {
        return Err(anyhow!("continuity restore unexpectedly succeeded"));
    }
    Ok(String::from_utf8_lossy(&output.stderr).to_string())
}

async fn run_context_pack_json(project: &str, namespace: &str, question: &str) -> Result<Value> {
    let output = ProcessCommand::new(std::env::current_exe()?)
        .arg("context")
        .arg("pack")
        .arg("--project")
        .arg(project)
        .arg("--namespace")
        .arg(namespace)
        .arg("--query")
        .arg(question)
        .arg("--retrieval-mode")
        .arg("local_strict")
        .arg("--token-source-kind")
        .arg("verify_memory_matrix_context_pack")
        .arg("--disable-cache")
        .output()
        .await
        .context("failed to run context pack command")?;
    if !output.status.success() {
        return Err(anyhow!(
            "context pack failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).context("context pack did not return valid JSON")
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
}
