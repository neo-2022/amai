use crate::bootstrap;
use crate::cli::VerifyMcpMatrixArgs;
use crate::config::AppConfig;
use crate::mcp;
use crate::postgres;
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::process::Command as ProcessCommand;

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
    max_failures: usize,
    max_p95_ms: Option<f64>,
    task_codes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MatrixTask {
    order: u32,
    class: MatrixTaskClass,
    display_name: String,
    kind: MatrixTaskKind,
    project: Option<String>,
    related_project: Option<String>,
    namespace: Option<String>,
    query: Option<String>,
    retrieval_mode: Option<String>,
    budget_profile: Option<String>,
    agent_scope: Option<String>,
    seed_agent_scope: Option<String>,
    expected_error_contains: Option<String>,
    #[serde(default)]
    bootstrap_lines: Vec<String>,
    seed_headline: Option<String>,
    seed_next_step: Option<String>,
    #[serde(default)]
    seed_details_lines: Vec<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
enum MatrixTaskClass {
    HappyPath,
    Hostile,
    Isolation,
}

impl MatrixTaskClass {
    fn as_str(self) -> &'static str {
        match self {
            MatrixTaskClass::HappyPath => "happy_path",
            MatrixTaskClass::Hostile => "hostile",
            MatrixTaskClass::Isolation => "isolation",
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MatrixTaskKind {
    ToolCatalog,
    ProjectListing,
    NamespaceListing,
    ContextLocalStrictIsolation,
    ContextRelatedScopeRouting,
    ObserveSnapshotGreen,
    TokenReportLive,
    WarmCache,
    UnknownToolFailClosed,
    UnknownProjectFailClosed,
    UnknownNamespaceFailClosed,
    ContinuityRestoreSuccess,
    ContinuityRestoreFailClosed,
}

pub async fn run_matrix(cfg: &AppConfig, args: &VerifyMcpMatrixArgs) -> Result<()> {
    bootstrap::bootstrap_stack(cfg).await?;
    let registry = load_registry()?;
    let matrix = registry
        .matrices
        .get(&args.matrix)
        .ok_or_else(|| anyhow!("unknown MCP task matrix: {}", args.matrix))?;

    let mut ordered_tasks = matrix
        .task_codes
        .iter()
        .map(|code| {
            registry
                .tasks
                .get(code)
                .cloned()
                .map(|task| (code.clone(), task))
                .ok_or_else(|| anyhow!("task {code} missing in MCP task matrix registry"))
        })
        .collect::<Result<Vec<_>>>()?;
    ordered_tasks.sort_by_key(|(code, task)| (task.order, code.clone()));

    let mut session = mcp::spawn_proof_session(cfg).await?;
    let run_result = run_matrix_inner(
        cfg,
        args,
        &registry.source,
        matrix,
        &ordered_tasks,
        &mut session,
    )
    .await;
    let shutdown_result = session.shutdown().await;

    run_result?;
    shutdown_result?;
    Ok(())
}

async fn run_matrix_inner(
    cfg: &AppConfig,
    args: &VerifyMcpMatrixArgs,
    source: &MatrixSource,
    matrix: &MatrixEntry,
    ordered_tasks: &[(String, MatrixTask)],
    session: &mut mcp::McpProofSession,
) -> Result<()> {
    let db = postgres::connect_admin(cfg).await?;
    let mut task_results = Vec::with_capacity(ordered_tasks.len());

    for (task_code, task) in ordered_tasks {
        task_results.push(run_task(args, task_code, task, session).await?);
    }

    let latencies = task_results
        .iter()
        .map(|task| task.latency_ms)
        .collect::<Vec<_>>();
    let tasks_total = task_results.len();
    let tasks_passed = task_results.iter().filter(|task| task.success).count();
    let tasks_failed = tasks_total.saturating_sub(tasks_passed);
    let success_rate = if tasks_total == 0 {
        0.0
    } else {
        tasks_passed as f64 / tasks_total as f64
    };
    let p50_ms = percentile_f64(&latencies, 50);
    let p95_ms = percentile_f64(&latencies, 95);
    let max_ms = latencies
        .iter()
        .copied()
        .max_by(f64::total_cmp)
        .unwrap_or_default();
    let mean_ms = mean_f64(&latencies);

    let mut class_breakdown = BTreeMap::<String, Value>::new();
    for class in [
        MatrixTaskClass::HappyPath,
        MatrixTaskClass::Hostile,
        MatrixTaskClass::Isolation,
    ] {
        let class_tasks = task_results
            .iter()
            .filter(|task| task.class == class)
            .collect::<Vec<_>>();
        if class_tasks.is_empty() {
            continue;
        }
        let class_total = class_tasks.len();
        let class_passed = class_tasks.iter().filter(|task| task.success).count();
        class_breakdown.insert(
            class.as_str().to_string(),
            json!({
                "tasks_total": class_total,
                "tasks_passed": class_passed,
                "tasks_failed": class_total.saturating_sub(class_passed),
                "success_rate": if class_total == 0 { 0.0 } else { class_passed as f64 / class_total as f64 },
            }),
        );
    }

    if success_rate < args.min_success_rate.unwrap_or(matrix.min_success_rate) {
        return Err(anyhow!(
            "MCP task matrix success_rate={success_rate:.3} below required {:.3}",
            args.min_success_rate.unwrap_or(matrix.min_success_rate)
        ));
    }

    if tasks_failed > matrix.max_failures {
        return Err(anyhow!(
            "MCP task matrix tasks_failed={} exceeds allowed {}",
            tasks_failed,
            matrix.max_failures
        ));
    }

    if let Some(limit) = args.max_p95_ms.or(matrix.max_p95_ms)
        && p95_ms > limit
    {
        return Err(anyhow!(
            "MCP task matrix p95_ms={p95_ms:.3} exceeds allowed {limit:.3}"
        ));
    }

    let payload = json!({
        "mcp_task_matrix": {
            "matrix": args.matrix,
            "display_name": matrix.display_name,
            "summary": matrix.summary,
            "source": {
                "display_name": source.display_name,
                "summary": source.summary,
                "reference_urls": source.reference_urls,
            },
            "tasks_total": tasks_total,
            "tasks_passed": tasks_passed,
            "tasks_failed": tasks_failed,
            "success_rate": success_rate,
            "mean_ms": mean_ms,
            "p50_ms": p50_ms,
            "p95_ms": p95_ms,
            "max_ms": max_ms,
            "class_breakdown": class_breakdown,
            "tasks": task_results.iter().map(TaskResult::as_json).collect::<Vec<_>>(),
        }
    });

    let _ = postgres::insert_observability_snapshot(&db, "mcp_task_matrix", &payload).await?;
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

async fn run_task(
    args: &VerifyMcpMatrixArgs,
    task_code: &str,
    task: &MatrixTask,
    session: &mut mcp::McpProofSession,
) -> Result<TaskResult> {
    let started = Instant::now();
    let details = match task.kind {
        MatrixTaskKind::ToolCatalog => run_tool_catalog_task(session).await?,
        MatrixTaskKind::ProjectListing => run_project_listing_task(args, task, session).await?,
        MatrixTaskKind::NamespaceListing => run_namespace_listing_task(args, task, session).await?,
        MatrixTaskKind::ContextLocalStrictIsolation => {
            run_context_local_strict_isolation_task(args, task, session).await?
        }
        MatrixTaskKind::ContextRelatedScopeRouting => {
            run_context_related_scope_routing_task(args, task, session).await?
        }
        MatrixTaskKind::ObserveSnapshotGreen => run_observe_snapshot_green_task(session).await?,
        MatrixTaskKind::TokenReportLive => run_token_report_live_task(args, task, session).await?,
        MatrixTaskKind::WarmCache => run_warm_cache_task(args, task, session).await?,
        MatrixTaskKind::UnknownToolFailClosed => {
            run_unknown_tool_fail_closed_task(task, session).await?
        }
        MatrixTaskKind::UnknownProjectFailClosed => {
            run_unknown_project_fail_closed_task(args, task, session).await?
        }
        MatrixTaskKind::UnknownNamespaceFailClosed => {
            run_unknown_namespace_fail_closed_task(args, task, session).await?
        }
        MatrixTaskKind::ContinuityRestoreSuccess => run_continuity_restore_task(task, true).await?,
        MatrixTaskKind::ContinuityRestoreFailClosed => {
            run_continuity_restore_task(task, false).await?
        }
    };

    Ok(TaskResult {
        code: task_code.to_string(),
        class: task.class,
        display_name: task.display_name.clone(),
        kind: format!("{:?}", task.kind),
        success: true,
        latency_ms: started.elapsed().as_secs_f64() * 1000.0,
        details,
    })
}

async fn run_tool_catalog_task(session: &mut mcp::McpProofSession) -> Result<Value> {
    let tools = session.request("tools/list", json!({})).await?;
    let tool_names = tools["tools"]
        .as_array()
        .ok_or_else(|| anyhow!("tools/list returned invalid tools array"))?
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(ToOwned::to_owned))
        .collect::<BTreeSet<_>>();
    let expected = BTreeSet::from([
        "amai_context_pack".to_string(),
        "amai_list_namespaces".to_string(),
        "amai_list_projects".to_string(),
        "amai_observe_snapshot".to_string(),
        "amai_token_benchmark".to_string(),
        "amai_token_report".to_string(),
        "amai_warm_cache".to_string(),
    ]);
    if tool_names != expected {
        return Err(anyhow!(
            "unexpected tool catalog: expected {:?}, got {:?}",
            expected,
            tool_names
        ));
    }
    Ok(json!({ "tools": tool_names }))
}

async fn run_project_listing_task(
    args: &VerifyMcpMatrixArgs,
    task: &MatrixTask,
    session: &mut mcp::McpProofSession,
) -> Result<Value> {
    let project = task.project.as_deref().unwrap_or(&args.project);
    let related_project = task
        .related_project
        .as_deref()
        .unwrap_or(&args.related_project);
    let response = session.tool_call("amai_list_projects", json!({})).await?;
    let projects = response["projects"]
        .as_array()
        .ok_or_else(|| anyhow!("amai_list_projects returned invalid project array"))?;
    let found = projects
        .iter()
        .filter_map(|item| item["code"].as_str())
        .collect::<BTreeSet<_>>();
    if !found.contains(project) || !found.contains(related_project) {
        return Err(anyhow!(
            "list_projects missing required projects: expected {project} and {related_project}, got {:?}",
            found
        ));
    }
    Ok(json!({ "projects": found }))
}

async fn run_namespace_listing_task(
    args: &VerifyMcpMatrixArgs,
    task: &MatrixTask,
    session: &mut mcp::McpProofSession,
) -> Result<Value> {
    let project = task.project.as_deref().unwrap_or(&args.project);
    let namespace = task.namespace.as_deref().unwrap_or(&args.namespace);
    let response = session
        .tool_call("amai_list_namespaces", json!({ "project": project }))
        .await?;
    let namespaces = response["namespaces"]
        .as_array()
        .ok_or_else(|| anyhow!("amai_list_namespaces returned invalid namespace array"))?;
    let found = namespaces
        .iter()
        .filter_map(|item| item["code"].as_str())
        .collect::<BTreeSet<_>>();
    if !found.contains(namespace) {
        return Err(anyhow!(
            "list_namespaces missing {} for {}",
            namespace,
            project
        ));
    }
    Ok(json!({ "project": project, "namespaces": found }))
}

async fn run_context_local_strict_isolation_task(
    args: &VerifyMcpMatrixArgs,
    task: &MatrixTask,
    session: &mut mcp::McpProofSession,
) -> Result<Value> {
    let project = task.project.as_deref().unwrap_or(&args.project);
    let related_project = task
        .related_project
        .as_deref()
        .unwrap_or(&args.related_project);
    let namespace = task.namespace.as_deref().unwrap_or(&args.namespace);
    let query = task.query.as_deref().unwrap_or("shared_runtime_marker");
    let response = session
        .tool_call(
            "amai_context_pack",
            json!({
                "project": project,
                "namespace": namespace,
                "query": query,
                "retrieval_mode": task.retrieval_mode.as_deref().unwrap_or("local_strict"),
                "disable_cache": false,
                "limit_documents": 8,
                "limit_symbols": 8,
                "limit_chunks": 8,
                "limit_semantic_chunks": 8,
                "persist": false,
            }),
        )
        .await?;

    let visible = collect_visible_projects(&response["context_pack"]["visible_projects"])?;
    if !visible.contains(project) {
        return Err(anyhow!(
            "local_strict context pack lost primary project {project}"
        ));
    }
    if visible.contains(related_project) {
        return Err(anyhow!(
            "local_strict context pack leaked related project {related_project}"
        ));
    }
    Ok(json!({
        "project": project,
        "namespace": namespace,
        "query": query,
        "visible_projects": visible,
    }))
}

async fn run_context_related_scope_routing_task(
    args: &VerifyMcpMatrixArgs,
    task: &MatrixTask,
    session: &mut mcp::McpProofSession,
) -> Result<Value> {
    let project = task.project.as_deref().unwrap_or(&args.project);
    let related_project = task
        .related_project
        .as_deref()
        .unwrap_or(&args.related_project);
    let namespace = task.namespace.as_deref().unwrap_or(&args.namespace);
    let query = task.query.as_deref().unwrap_or("shared_runtime_marker");
    let response = session
        .tool_call(
            "amai_context_pack",
            json!({
                "project": project,
                "namespace": namespace,
                "query": query,
                "retrieval_mode": task.retrieval_mode.as_deref().unwrap_or("local_plus_related"),
                "disable_cache": false,
                "limit_documents": 8,
                "limit_symbols": 8,
                "limit_chunks": 8,
                "limit_semantic_chunks": 8,
                "persist": false,
            }),
        )
        .await?;

    let visible = collect_visible_projects(&response["context_pack"]["visible_projects"])?;
    if !visible.contains(project) || !visible.contains(related_project) {
        return Err(anyhow!(
            "local_plus_related context pack did not route across expected projects: {:?}",
            visible
        ));
    }
    Ok(json!({
        "project": project,
        "related_project": related_project,
        "namespace": namespace,
        "query": query,
        "visible_projects": visible,
    }))
}

async fn run_observe_snapshot_green_task(session: &mut mcp::McpProofSession) -> Result<Value> {
    let response = session
        .tool_call("amai_observe_snapshot", json!({}))
        .await?;
    let summary = &response["snapshot"]["sla"]["summary"];
    let critical = summary["critical"].as_u64().unwrap_or_default();
    let unknown = summary["unknown"].as_u64().unwrap_or_default();
    if critical != 0 || unknown != 0 {
        return Err(anyhow!(
            "observe snapshot not green: critical={critical}, unknown={unknown}"
        ));
    }
    Ok(json!({
        "pass": summary["pass"].as_u64().unwrap_or_default(),
        "alert": summary["alert"].as_u64().unwrap_or_default(),
        "critical": critical,
        "unknown": unknown,
    }))
}

async fn run_token_report_live_task(
    args: &VerifyMcpMatrixArgs,
    task: &MatrixTask,
    session: &mut mcp::McpProofSession,
) -> Result<Value> {
    let budget_profile = task
        .budget_profile
        .as_deref()
        .unwrap_or(&args.budget_profile);
    let response = session
        .tool_call(
            "amai_token_report",
            json!({
                "budget_profile": budget_profile,
                "include_verify_events": false,
            }),
        )
        .await?;
    let headline = &response["token_budget_report"]["headline"];
    let events_total = response["token_budget_report"]["current_session"]["events_total"]
        .as_u64()
        .ok_or_else(|| anyhow!("token_report current_session.events_total is missing"))?;
    if headline["metric_code"].as_str() != Some("verified_effective_savings_pct") {
        return Err(anyhow!(
            "unexpected token headline metric: {:?}",
            headline["metric_code"]
        ));
    }
    if events_total == 0 {
        return Err(anyhow!("token report returned zero current session events"));
    }
    Ok(json!({
        "budget_profile": budget_profile,
        "metric_code": headline["metric_code"],
        "events_total": events_total,
        "value_percent": headline["value_percent"],
    }))
}

async fn run_warm_cache_task(
    args: &VerifyMcpMatrixArgs,
    task: &MatrixTask,
    session: &mut mcp::McpProofSession,
) -> Result<Value> {
    let project = task.project.as_deref().unwrap_or(&args.project);
    let namespace = task.namespace.as_deref().unwrap_or(&args.namespace);
    let query = task.query.as_deref().unwrap_or("shared_runtime_marker");
    let response = session
        .tool_call(
            "amai_warm_cache",
            json!({
                "projects": [project],
                "namespace": namespace,
                "query": query,
                "retrieval_mode": task.retrieval_mode.as_deref().unwrap_or("local_plus_related"),
                "limit_documents": 8,
                "limit_symbols": 8,
                "limit_chunks": 8,
                "limit_semantic_chunks": 8,
            }),
        )
        .await?;
    let warmed = response["warmup_cache"]["warmed"]
        .as_array()
        .ok_or_else(|| anyhow!("warm_cache returned invalid warmed array"))?;
    if warmed.is_empty() {
        return Err(anyhow!("warm_cache returned no warmed entries"));
    }
    Ok(json!({
        "project": project,
        "namespace": namespace,
        "query": query,
        "warmed_count": warmed.len(),
    }))
}

async fn run_unknown_tool_fail_closed_task(
    task: &MatrixTask,
    session: &mut mcp::McpProofSession,
) -> Result<Value> {
    let expected = task
        .expected_error_contains
        .as_deref()
        .unwrap_or("unknown MCP tool");
    let response = session
        .tool_call_raw("amai_unknown_tool", json!({}))
        .await?;
    assert_tool_error_contains(&response, expected)
}

async fn run_unknown_project_fail_closed_task(
    args: &VerifyMcpMatrixArgs,
    task: &MatrixTask,
    session: &mut mcp::McpProofSession,
) -> Result<Value> {
    let namespace = task.namespace.as_deref().unwrap_or(&args.namespace);
    let expected = task.expected_error_contains.as_deref().unwrap_or("project");
    let response = session
        .tool_call_raw(
            "amai_context_pack",
            json!({
                "project": task.project.as_deref().unwrap_or("unknown_project"),
                "namespace": namespace,
                "query": task.query.as_deref().unwrap_or("shared_runtime_marker"),
                "retrieval_mode": task.retrieval_mode.as_deref().unwrap_or("local_strict"),
                "disable_cache": false,
                "limit_documents": 8,
                "limit_symbols": 8,
                "limit_chunks": 8,
                "limit_semantic_chunks": 8,
                "persist": false,
            }),
        )
        .await?;
    assert_tool_error_contains(&response, expected)
}

async fn run_unknown_namespace_fail_closed_task(
    args: &VerifyMcpMatrixArgs,
    task: &MatrixTask,
    session: &mut mcp::McpProofSession,
) -> Result<Value> {
    let project = task.project.as_deref().unwrap_or(&args.project);
    let expected = task
        .expected_error_contains
        .as_deref()
        .unwrap_or("namespace");
    let response = session
        .tool_call_raw(
            "amai_context_pack",
            json!({
                "project": project,
                "namespace": task.namespace.as_deref().unwrap_or("unknown_namespace"),
                "query": task.query.as_deref().unwrap_or("shared_runtime_marker"),
                "retrieval_mode": task.retrieval_mode.as_deref().unwrap_or("local_strict"),
                "disable_cache": false,
                "limit_documents": 8,
                "limit_symbols": 8,
                "limit_chunks": 8,
                "limit_semantic_chunks": 8,
                "persist": false,
            }),
        )
        .await?;
    assert_tool_error_contains(&response, expected)
}

async fn run_continuity_restore_task(task: &MatrixTask, expect_success: bool) -> Result<Value> {
    let project = task
        .project
        .as_deref()
        .ok_or_else(|| anyhow!("continuity restore task requires project"))?;
    let namespace = task
        .namespace
        .as_deref()
        .ok_or_else(|| anyhow!("continuity restore task requires namespace"))?;
    ensure_continuity_seed(task, project, namespace).await?;
    let exe = std::env::current_exe().context("failed to resolve current amai executable")?;
    let mut command = ProcessCommand::new(exe);
    command
        .arg("continuity")
        .arg("restore")
        .arg("--project")
        .arg(project)
        .arg("--namespace")
        .arg(namespace);
    if let Some(agent_scope) = &task.agent_scope {
        command.env("AMAI_AGENT_SCOPE", agent_scope);
    }
    let output = command
        .output()
        .await
        .context("failed to run continuity restore task")?;
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if expect_success {
        if !output.status.success() {
            return Err(anyhow!(
                "continuity restore unexpectedly failed: {}",
                stderr.trim()
            ));
        }
        let parsed: Value = serde_json::from_str(&stdout)
            .context("continuity restore success output is not valid JSON")?;
        if parsed.get("chat_start_restore").is_none()
            || parsed.get("working_state_restore").is_none()
        {
            return Err(anyhow!(
                "continuity restore success output missing restore sections"
            ));
        }
        return Ok(json!({
            "project": project,
            "namespace": namespace,
            "agent_scope": task.agent_scope,
            "status": "success",
        }));
    }

    let expected = task
        .expected_error_contains
        .as_deref()
        .unwrap_or("no working-state restore bundle found");
    if output.status.success() {
        return Err(anyhow!(
            "continuity restore unexpectedly succeeded for isolated scope"
        ));
    }
    if !stderr.contains(expected) {
        return Err(anyhow!(
            "continuity restore failure did not contain expected text: {}",
            expected
        ));
    }
    Ok(json!({
        "project": project,
        "namespace": namespace,
        "agent_scope": task.agent_scope,
        "status": "fail_closed",
        "stderr": stderr.trim(),
    }))
}

async fn ensure_continuity_seed(task: &MatrixTask, project: &str, namespace: &str) -> Result<()> {
    let repo_root = std::env::temp_dir()
        .join("amai-mcp-matrix-projects")
        .join(project);
    fs::create_dir_all(&repo_root)
        .with_context(|| format!("failed to create {}", repo_root.display()))?;
    let bootstrap_path = std::env::temp_dir().join(format!(
        "amai-mcp-matrix-{}.md",
        uuid::Uuid::new_v4().simple()
    ));
    let bootstrap_lines = if task.bootstrap_lines.is_empty() {
        vec![
            "# Synthetic continuity bootstrap".to_string(),
            "This continuity namespace exists only for the MCP matrix restore contour.".to_string(),
        ]
    } else {
        task.bootstrap_lines.clone()
    };
    fs::write(&bootstrap_path, format!("{}\n", bootstrap_lines.join("\n")))
        .with_context(|| format!("failed to write {}", bootstrap_path.display()))?;
    let display_name = project.replace('_', " ");
    let output = ProcessCommand::new(std::env::current_exe()?)
        .arg("continuity")
        .arg("import")
        .arg("--project")
        .arg(project)
        .arg("--display-name")
        .arg(&display_name)
        .arg("--repo-root")
        .arg(&repo_root)
        .arg("--namespace")
        .arg(namespace)
        .arg("--bootstrap-file")
        .arg(&bootstrap_path)
        .arg("--transcript-limit")
        .arg("0")
        .output()
        .await
        .context("failed to run continuity import seed for MCP matrix")?;
    let cleanup_result = fs::remove_file(&bootstrap_path)
        .with_context(|| format!("failed to remove {}", bootstrap_path.display()));
    if !output.status.success() {
        return Err(anyhow!(
            "continuity seed import failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    cleanup_result?;
    let handoff_path = std::env::temp_dir().join(format!(
        "amai-mcp-matrix-handoff-{}.md",
        uuid::Uuid::new_v4().simple()
    ));
    let handoff_lines = if task.seed_details_lines.is_empty() {
        vec!["- Synthetic owner-scope fact for the MCP continuity matrix.".to_string()]
    } else {
        task.seed_details_lines.clone()
    };
    fs::write(&handoff_path, format!("{}\n", handoff_lines.join("\n")))
        .with_context(|| format!("failed to write {}", handoff_path.display()))?;
    let mut handoff = ProcessCommand::new(std::env::current_exe()?);
    handoff
        .arg("continuity")
        .arg("handoff")
        .arg("--project")
        .arg(project)
        .arg("--namespace")
        .arg(namespace)
        .arg("--headline")
        .arg(
            task.seed_headline
                .as_deref()
                .unwrap_or("Synthetic continuity seed"),
        )
        .arg("--next-step")
        .arg(
            task.seed_next_step
                .as_deref()
                .unwrap_or("This scope exists only for measured MCP continuity restore."),
        )
        .arg("--details-file")
        .arg(&handoff_path);
    if let Some(seed_scope) = task
        .seed_agent_scope
        .as_deref()
        .or(task.agent_scope.as_deref())
    {
        handoff.env("AMAI_AGENT_SCOPE", seed_scope);
    }
    let handoff_output = handoff
        .output()
        .await
        .context("failed to run continuity handoff seed for MCP matrix")?;
    let handoff_cleanup = fs::remove_file(&handoff_path)
        .with_context(|| format!("failed to remove {}", handoff_path.display()));
    if !handoff_output.status.success() {
        return Err(anyhow!(
            "continuity seed handoff failed: {}",
            String::from_utf8_lossy(&handoff_output.stderr).trim()
        ));
    }
    handoff_cleanup?;
    Ok(())
}

fn assert_tool_error_contains(response: &Value, expected: &str) -> Result<Value> {
    if !response["isError"].as_bool().unwrap_or(false) {
        return Err(anyhow!("MCP tool call unexpectedly succeeded"));
    }
    let text = response["content"]
        .as_array()
        .and_then(|items| items.first())
        .and_then(|item| item["text"].as_str())
        .unwrap_or_default();
    if !text.contains(expected) {
        return Err(anyhow!(
            "tool error did not contain expected text {}: {}",
            expected,
            text
        ));
    }
    Ok(json!({
        "status": "fail_closed",
        "message": text,
    }))
}

fn collect_visible_projects(value: &Value) -> Result<BTreeSet<String>> {
    let items = value
        .as_array()
        .ok_or_else(|| anyhow!("visible_projects is not an array"))?;
    Ok(items
        .iter()
        .filter_map(|item| item["project_code"].as_str().map(ToOwned::to_owned))
        .collect())
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

fn load_registry() -> Result<MatrixRegistry> {
    let path = registry_path();
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read MCP task matrix {}", path.display()))?;
    toml::from_str(&content).context("failed to parse MCP task matrix")
}

fn registry_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("config/mcp_task_matrix.toml")
}

struct TaskResult {
    code: String,
    class: MatrixTaskClass,
    display_name: String,
    kind: String,
    success: bool,
    latency_ms: f64,
    details: Value,
}

impl TaskResult {
    fn as_json(&self) -> Value {
        json!({
            "code": self.code,
            "class": self.class.as_str(),
            "display_name": self.display_name,
            "kind": self.kind,
            "success": self.success,
            "latency_ms": self.latency_ms,
            "details": self.details,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MatrixTaskClass, load_registry};

    #[test]
    fn registry_loads_with_required_matrices() {
        let registry = load_registry().expect("registry should load");
        assert!(registry.matrices.contains_key("live_mcpbench_local"));
        assert!(registry.matrices.contains_key("mcp_universe_local"));
        assert!(registry.tasks.contains_key("tool_catalog"));
    }

    #[test]
    fn registry_contains_isolation_tasks() {
        let registry = load_registry().expect("registry should load");
        let isolation_tasks = registry
            .tasks
            .values()
            .filter(|task| task.class == MatrixTaskClass::Isolation)
            .count();
        assert!(isolation_tasks >= 2);
    }
}
