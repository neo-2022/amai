use crate::{
    compatibility, config, config::AppConfig, continuity, nats, onboarding, postgres, qdrant, s3,
};
use anyhow::{Context, Result};
use reqwest::StatusCode;
use std::time::Duration;

pub async fn print_status(cfg: &AppConfig) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let db = postgres::connect_admin(cfg).await?;
    let (projects, namespaces, documents) = postgres::status_counts(&db).await?;
    let app_db = postgres::connect_app(cfg).await?;
    let (app_projects, app_namespaces, app_documents) = postgres::status_counts(&app_db).await?;

    let qdrant_client = qdrant::connect(cfg)?;
    let code_exists = qdrant_client
        .collection_exists(&cfg.qdrant_collection_code)
        .await?;
    let memory_exists = qdrant_client
        .collection_exists(&cfg.qdrant_collection_memory)
        .await?;

    let s3_client = s3::connect(cfg).await?;
    let buckets = s3::status_bucket_names(&s3_client).await?;

    let nats_client = nats::connect(cfg).await?;
    let streams = nats::status_stream_names(nats_client).await?;

    let nats_http = http_client()?.get(&cfg.nats_http_url).send().await?;
    let nats_http_ok = nats_http.status() == StatusCode::OK;

    println!("stack: {}", cfg.stack_name);
    println!(
        "postgres: ok (admin_projects={projects}, admin_namespaces={namespaces}, admin_documents={documents}, app_projects={app_projects}, app_namespaces={app_namespaces}, app_documents={app_documents})"
    );
    println!(
        "qdrant: ok (code_collection={}, memory_collection={})",
        code_exists, memory_exists
    );
    println!("s3: ok (buckets={})", buckets.join(", "));
    println!(
        "nats: ok (http={}, streams={})",
        nats_http_ok,
        streams.join(", ")
    );
    println!("memory_embed_model: {}", cfg.memory_embed_model);
    println!("edge_cache: {}", cfg.edge_cache_path.display());
    let compatibility = compatibility::check(cfg).await?;
    println!(
        "compatibility: {} (profile={}, postgres={}, qdrant={}, nats={}, s3={})",
        if compatibility.compatible() {
            "ok"
        } else {
            "FAIL"
        },
        compatibility.profile,
        compatibility.postgres.raw_version,
        compatibility.qdrant.raw_version,
        compatibility.nats.raw_version,
        compatibility.s3.raw_version
    );
    match onboarding::inspect_startup_artifacts(&repo_root) {
        Ok(Some(audit)) => {
            println!(
                "startup_artifacts: {} (instruction_present={}, instruction_sha_match={}, instruction_requires_pre_tool_read={}, instruction_missing_fail_closed={}, instruction_sha_mismatch_fail_closed={}, instruction_has_startup_next_action={}, instruction_has_required_return_task={}, instruction_has_resume_required_action_kind={}, instruction_has_previous_session_owner_follow={}, instruction_has_no_silent_drop={}, instruction_has_runtime_state_artifact={}, instruction_has_startup_execution_gate={}, instruction_has_startup_state_fallback_cli={}, contract_present={}, contract_sha_match={}, install_state_sha_match={}, contract_fail_closed={}, contract_has_startup_next_action_field={}, contract_has_required_return_task_field={}, contract_has_resume_required_action_kind={}, contract_has_previous_session_owner_follow={}, contract_has_no_silent_drop={}, contract_has_runtime_state_artifact={}, contract_has_startup_execution_gate={}, contract_has_startup_state_fallback_cli={}, instruction_path={}, contract_path={})",
                audit.status,
                audit.startup_instruction_exists,
                audit
                    .startup_instruction_contains_expected_sha
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_required_before_tool_call
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_missing_fail_closed
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_sha_mismatch_fail_closed
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_startup_next_action
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_required_return_task
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_resume_required_action_kind
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_previous_session_owner_follow
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_no_silent_drop
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_runtime_state_artifact
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_startup_execution_gate
                    .unwrap_or(false),
                audit
                    .startup_instruction_contains_startup_state_fallback_cli
                    .unwrap_or(false),
                audit.startup_contract_exists,
                audit
                    .startup_contract_sha_matches_current_contract
                    .unwrap_or(false),
                audit
                    .install_state_sha_matches_current_contract
                    .unwrap_or(false),
                audit.startup_contract_enforces_fail_closed.unwrap_or(false),
                audit
                    .startup_contract_contains_startup_next_action_field
                    .unwrap_or(false),
                audit
                    .startup_contract_contains_required_return_task_field
                    .unwrap_or(false),
                audit
                    .startup_contract_contains_resume_required_action_kind
                    .unwrap_or(false),
                audit
                    .startup_contract_contains_previous_session_owner_follow
                    .unwrap_or(false),
                audit
                    .startup_contract_contains_no_silent_drop
                    .unwrap_or(false),
                audit
                    .startup_contract_contains_runtime_state_artifact
                    .unwrap_or(false),
                audit
                    .startup_contract_contains_startup_execution_gate
                    .unwrap_or(false),
                audit
                    .startup_contract_contains_startup_state_fallback_cli
                    .unwrap_or(false),
                audit
                    .startup_instruction_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "n/a".to_string()),
                audit
                    .startup_contract_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "n/a".to_string())
            );
            if audit.status != "ok" {
                println!(
                    "startup_artifacts_repair: rerun ./scripts/onboard_local.sh --client {} --yes",
                    audit.client_key
                );
            }
        }
        Ok(None) => {
            println!("startup_artifacts: no_install_state");
            println!(
                "startup_artifacts_repair: run ./scripts/onboard_local.sh --client <client> --yes to materialize a startup artifact"
            );
        }
        Err(error) => println!("startup_artifacts: error ({error:#})"),
    }
    match continuity::inspect_startup_runtime_state(&repo_root) {
        Ok(audit) => {
            println!(
                "startup_runtime_state: {} (artifact_present={}, contract_sha_match={}, source_summary_field_match={}, prompt_text_present={}, startup_next_action_present={}, startup_execution_gate_present={}, required_return_task_field_present={}, execctl_active_lease_field_present={}, project_task_tree_field_present={}, project_task_ledger_field_present={}, resume_state={}, action_kind={}, lease_owner_state={}, must_follow_startup_next_action={}, unrelated_work_allowed={}, path={})",
                audit.status,
                audit.artifact_exists,
                audit
                    .startup_contract_sha_matches_current_contract
                    .unwrap_or(false),
                audit.source_summary_field_matches.unwrap_or(false),
                audit.prompt_text_present.unwrap_or(false),
                audit.startup_next_action_present.unwrap_or(false),
                audit.startup_execution_gate_present.unwrap_or(false),
                audit.required_return_task_field_present.unwrap_or(false),
                audit.execctl_active_lease_field_present.unwrap_or(false),
                audit.project_task_tree_field_present.unwrap_or(false),
                audit.project_task_ledger_field_present.unwrap_or(false),
                audit.resume_state.as_deref().unwrap_or("n/a"),
                audit.action_kind.as_deref().unwrap_or("n/a"),
                audit.lease_owner_state.as_deref().unwrap_or("n/a"),
                audit.must_follow_startup_next_action.unwrap_or(false),
                audit.unrelated_work_allowed.unwrap_or(false),
                audit.output_path.display(),
            );
            if audit.status != "ok" {
                println!(
                    "startup_runtime_state_repair: rerun cargo run -- continuity startup --repo-root {} --namespace continuity --json >/dev/null",
                    repo_root.display()
                );
            }
        }
        Err(error) => println!("startup_runtime_state: error ({error:#})"),
    }
    Ok(())
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .context("failed to build status HTTP client")
}
