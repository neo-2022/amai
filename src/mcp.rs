use crate::cli::{
    ContextPackArgs, ContinuityStartupArgs, McpConfigArgs, VerifyMcpArgs, VerifyMcpScope,
    VerifyMemoryMatrixArgs, VerifyTokenBenchmarkArgs,
};
use crate::{
    benchmark_matrix, compatibility, config, continuity, memory_task_matrix, observe, postgres,
    profiles, retrieval, token_budget, verify, working_state,
};
use anyhow::{Context, Result, anyhow};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command as ProcessCommand};
use tokio::time::{Duration, timeout};
use uuid::Uuid;

use crate::config::AppConfig;

pub(crate) const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
pub(crate) const SERVER_NAME: &str = "Art-memory-agent-index";

type McpToolResult<T> = std::result::Result<T, McpError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct McpErrorSpec {
    jsonrpc_code: i64,
    message: &'static str,
    amai_error_code: &'static str,
    amai_error_class: &'static str,
    retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct McpError {
    spec: McpErrorSpec,
    detail: String,
}

impl McpError {
    fn parse(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32700,
                message: "invalid JSON-RPC payload",
                amai_error_code: "invalid_json_rpc_payload",
                amai_error_class: "protocol_parse",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    fn invalid_request(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32600,
                message: "invalid request",
                amai_error_code: "invalid_request",
                amai_error_class: "protocol_request",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    fn invalid_params(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32602,
                message: "invalid params",
                amai_error_code: "invalid_params",
                amai_error_class: "client_input",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    fn method_not_found(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32601,
                message: "method not found",
                amai_error_code: "method_not_found",
                amai_error_class: "protocol_dispatch",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    fn prompt_not_found(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32601,
                message: "prompt not found",
                amai_error_code: "prompt_not_found",
                amai_error_class: "prompt_dispatch",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    fn tool_not_found(detail: impl Into<String>) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32601,
                message: "tool not found",
                amai_error_code: "tool_not_found",
                amai_error_class: "tool_dispatch",
                retryable: false,
            },
            detail: detail.into(),
        }
    }

    fn tool_runtime(error: anyhow::Error) -> Self {
        Self {
            spec: McpErrorSpec {
                jsonrpc_code: -32000,
                message: "tool execution failed",
                amai_error_code: "tool_execution_failed",
                amai_error_class: "tool_runtime",
                retryable: false,
            },
            detail: format!("{error:#}"),
        }
    }
}

fn mcp_error_taxonomy_payload(error: &McpError) -> Value {
    json!({
        "amai_error_code": error.spec.amai_error_code,
        "amai_error_class": error.spec.amai_error_class,
        "retryable": error.spec.retryable,
        "detail": error.detail,
    })
}

fn mcp_jsonrpc_error_response(id: Value, error: &McpError) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": error.spec.jsonrpc_code,
            "message": error.spec.message,
            "data": mcp_error_taxonomy_payload(error),
        }
    })
}

fn mcp_tool_error_result(error: &McpError) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": format!("tool failed: {}", error.detail)
        }],
        "isError": true,
        "structuredContent": {
            "error_taxonomy": mcp_error_taxonomy_payload(error),
        }
    })
}

pub async fn serve(cfg: &AppConfig) -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();
    let mut writer = stdout;

    while let Some(line) = lines
        .next_line()
        .await
        .context("failed to read MCP input line")?
    {
        if line.trim().is_empty() {
            continue;
        }

        let incoming: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                let response =
                    mcp_jsonrpc_error_response(Value::Null, &McpError::parse(error.to_string()));
                write_message(&mut writer, &response).await?;
                continue;
            }
        };

        if incoming.get("id").is_none() {
            continue;
        }

        let response = match handle_request(cfg, incoming).await {
            Ok(response) => response,
            Err(error) => mcp_jsonrpc_error_response(
                Value::Null,
                &McpError {
                    spec: McpErrorSpec {
                        jsonrpc_code: -32000,
                        message: "MCP request handler failed",
                        amai_error_code: "request_handler_failed",
                        amai_error_class: "server_runtime",
                        retryable: false,
                    },
                    detail: error.to_string(),
                },
            ),
        };
        write_message(&mut writer, &response).await?;
    }

    Ok(())
}

pub fn write_client_config(args: &McpConfigArgs) -> Result<()> {
    let rendered = render_client_config(args)?;
    if let Some(output) = &args.output {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let shape = config_shape_for_client(&args.client)?;
        let final_content = merge_existing_config(shape, args, &rendered, output)?;
        std::fs::write(output, final_content.as_bytes())
            .with_context(|| format!("failed to write {}", output.display()))?;
        println!("written: {}", output.display());
    } else {
        println!("{rendered}");
    }
    Ok(())
}

pub fn client_config_contains_server(args: &McpConfigArgs) -> Result<bool> {
    let output = args.output.as_ref().ok_or_else(|| {
        anyhow!("client config inspection requires --output or resolved install path")
    })?;
    if !output.exists() {
        return Ok(false);
    }

    let existing = fs::read_to_string(output)
        .with_context(|| format!("failed to read {}", output.display()))?;
    let server_name = args.server_name.trim();
    let shape = config_shape_for_client(&args.client)?;

    match shape {
        ConfigShape::GenericJson => Ok(!existing.trim().is_empty()),
        ConfigShape::VscodeJson => json_server_exists(&existing, "servers", server_name),
        ConfigShape::McpServersJson => json_server_exists(&existing, "mcpServers", server_name),
        ConfigShape::CodexToml => toml_server_exists(&existing, server_name),
    }
}

pub struct RemoveConfigResult {
    pub removed: bool,
    pub purged_file: bool,
}

pub fn remove_client_config(
    args: &McpConfigArgs,
    purge_empty_file: bool,
) -> Result<RemoveConfigResult> {
    let output = args.output.as_ref().ok_or_else(|| {
        anyhow!("client config removal requires --output or resolved install path")
    })?;
    if !output.exists() {
        return Ok(RemoveConfigResult {
            removed: false,
            purged_file: false,
        });
    }

    let shape = config_shape_for_client(&args.client)?;
    let existing = fs::read_to_string(output)
        .with_context(|| format!("failed to read {}", output.display()))?;

    let (updated, removed, is_empty) = match shape {
        ConfigShape::GenericJson => ("".to_string(), true, true),
        ConfigShape::VscodeJson => {
            remove_json_server(&existing, "servers", args.server_name.trim())?
        }
        ConfigShape::McpServersJson => {
            remove_json_server(&existing, "mcpServers", args.server_name.trim())?
        }
        ConfigShape::CodexToml => remove_toml_server(&existing, args.server_name.trim())?,
    };

    if !removed {
        return Ok(RemoveConfigResult {
            removed: false,
            purged_file: false,
        });
    }

    if purge_empty_file && is_empty {
        fs::remove_file(output)
            .with_context(|| format!("failed to remove {}", output.display()))?;
        return Ok(RemoveConfigResult {
            removed: true,
            purged_file: true,
        });
    }

    fs::write(output, updated.as_bytes())
        .with_context(|| format!("failed to write {}", output.display()))?;
    Ok(RemoveConfigResult {
        removed: true,
        purged_file: false,
    })
}

pub async fn run_smoke_proof(cfg: &AppConfig, args: &VerifyMcpArgs) -> Result<()> {
    compatibility::assert_supported(cfg).await?;
    let proof_context_source_kind = if args.context.token_source_kind.trim().is_empty()
        || args.context.token_source_kind == "live_context_pack"
    {
        "proof_mcp_context_pack".to_string()
    } else {
        args.context.token_source_kind.clone()
    };

    for client in [
        "generic",
        "vscode",
        "cursor",
        "claude-desktop",
        "claude-code",
        "codex",
    ] {
        let config = render_client_config(&McpConfigArgs {
            client: client.to_string(),
            server_name: "amai".to_string(),
            launcher_platform: "auto".to_string(),
            ssh_destination: None,
            remote_repo_root: None,
            command: Some("/tmp/run_mcp_stdio.sh".to_string()),
            cwd: Some(PathBuf::from("/tmp/amai")),
            output: None,
        })?;
        match client {
            "codex" => {
                let _: toml::Value =
                    toml::from_str(&config).context("generated codex config is not valid TOML")?;
            }
            _ => {
                let _: Value = serde_json::from_str(&config)
                    .context("generated client config is not valid JSON")?;
            }
        }
    }

    let mut session = spawn_proof_session(cfg).await?;
    let startup_contract = &session.protocol_manifest["startup_contracts"]["project_chat_startup"];
    if startup_contract["tool"].as_str() != Some("amai_continuity_startup") {
        return Err(anyhow!(
            "MCP startup contract does not point to amai_continuity_startup"
        ));
    }
    if startup_contract["must_call_before_substantive_work"].as_bool() != Some(true) {
        return Err(anyhow!(
            "MCP startup contract does not require continuity startup before substantive work"
        ));
    }
    if startup_contract["default_namespace"].as_str() != Some("continuity") {
        return Err(anyhow!(
            "MCP startup contract lost default continuity namespace"
        ));
    }
    if startup_contract["artifact_enforcement"]["workspace_contract_relative_path"].as_str()
        != Some(".amai/onboarding/project-chat-startup-contract.json")
    {
        return Err(anyhow!(
            "MCP startup contract lost workspace startup artifact path"
        ));
    }
    if startup_contract["artifact_enforcement"]["workspace_contract_required_before_tool_call"]
        .as_bool()
        != Some(true)
    {
        return Err(anyhow!(
            "MCP startup contract does not require workspace startup artifact before tool call"
        ));
    }
    if startup_contract["artifact_enforcement"]["missing_or_unreadable_fail_closed"].as_bool()
        != Some(true)
    {
        return Err(anyhow!(
            "MCP startup contract does not fail closed when startup artifact is missing or unreadable"
        ));
    }
    if startup_contract["artifact_enforcement"]["sha256_mismatch_fail_closed"].as_bool()
        != Some(true)
    {
        return Err(anyhow!(
            "MCP startup contract does not fail closed on startup artifact sha256 mismatch"
        ));
    }
    if startup_contract["runtime_state_artifact"]["gate_semantics_consistent_field"].as_str()
        != Some("gate_semantics_consistent")
    {
        return Err(anyhow!(
            "MCP startup contract lost runtime_state_artifact.gate_semantics_consistent_field"
        ));
    }
    if startup_contract["runtime_state_artifact"]["gate_semantics_consistent_true_required"]
        .as_bool()
        != Some(true)
    {
        return Err(anyhow!(
            "MCP startup contract does not require gate_semantics_consistent = true"
        ));
    }
    let required_summary_fields = startup_contract["required_summary_fields"]
        .as_array()
        .ok_or_else(|| anyhow!("MCP startup contract is missing required_summary_fields"))?;
    if !required_summary_fields
        .iter()
        .any(|field| field.as_str() == Some("execctl_resume_state"))
    {
        return Err(anyhow!(
            "MCP startup contract is missing execctl_resume_state from required summary fields"
        ));
    }
    if !required_summary_fields
        .iter()
        .any(|field| field.as_str() == Some("startup_next_action"))
    {
        return Err(anyhow!(
            "MCP startup contract is missing startup_next_action from required summary fields"
        ));
    }
    if !required_summary_fields
        .iter()
        .any(|field| field.as_str() == Some("execctl_active_lease"))
    {
        return Err(anyhow!(
            "MCP startup contract is missing execctl_active_lease from required summary fields"
        ));
    }
    if !required_summary_fields
        .iter()
        .any(|field| field.as_str() == Some("execctl_active_lease_summary"))
    {
        return Err(anyhow!(
            "MCP startup contract is missing execctl_active_lease_summary from required summary fields"
        ));
    }
    if startup_contract["resume_enforcement"]["active_lease_field"].as_str()
        != Some("execctl_active_lease")
    {
        return Err(anyhow!(
            "MCP startup contract resume_enforcement lost execctl_active_lease field"
        ));
    }
    if startup_contract["resume_enforcement"]["active_lease_owner_state_field"].as_str()
        != Some("lease_owner_state")
    {
        return Err(anyhow!(
            "MCP startup contract resume_enforcement lost lease_owner_state field"
        ));
    }
    if startup_contract["resume_enforcement"]["previous_session_owner_value"].as_str()
        != Some("previous_session_owner")
    {
        return Err(anyhow!(
            "MCP startup contract resume_enforcement lost previous_session_owner value"
        ));
    }
    if startup_contract["resume_enforcement"]["previous_session_owner_must_follow_startup_next_action"]
        .as_bool()
        != Some(true)
    {
        return Err(anyhow!(
            "MCP startup contract resume_enforcement does not require previous_session_owner to follow startup_next_action"
        ));
    }
    if startup_contract["startup_execution_gate_enforcement"]["gate_field"].as_str()
        != Some("startup_execution_gate")
    {
        return Err(anyhow!(
            "MCP startup contract lost startup_execution_gate_enforcement.gate_field"
        ));
    }
    if startup_contract["startup_execution_gate_enforcement"]["must_follow_field"].as_str()
        != Some("must_follow_startup_next_action")
    {
        return Err(anyhow!(
            "MCP startup contract lost startup_execution_gate_enforcement.must_follow_field"
        ));
    }
    if startup_contract["startup_execution_gate_enforcement"]["unrelated_work_allowed_field"]
        .as_str()
        != Some("unrelated_work_allowed")
    {
        return Err(anyhow!(
            "MCP startup contract lost startup_execution_gate_enforcement.unrelated_work_allowed_field"
        ));
    }
    if startup_contract["startup_execution_gate_enforcement"]
        ["must_read_prompt_text_before_reply_field"]
        .as_str()
        != Some("must_read_prompt_text_before_reply")
    {
        return Err(anyhow!(
            "MCP startup contract lost startup_execution_gate_enforcement.must_read_prompt_text_before_reply_field"
        ));
    }
    if startup_contract["startup_execution_gate_enforcement"]["required_action_kind_field"].as_str()
        != Some("required_action_kind_when_resume_required")
    {
        return Err(anyhow!(
            "MCP startup contract lost startup_execution_gate_enforcement.required_action_kind_field"
        ));
    }
    if startup_contract["startup_execution_gate_enforcement"]["no_silent_drop_field"].as_str()
        != Some("no_silent_drop")
    {
        return Err(anyhow!(
            "MCP startup contract lost startup_execution_gate_enforcement.no_silent_drop_field"
        ));
    }
    if startup_contract["startup_execution_gate_enforcement"]
        ["blocking_true_requires_must_follow"]
        .as_bool()
        != Some(true)
        || startup_contract["startup_execution_gate_enforcement"]
            ["blocking_true_blocks_unrelated_work"]
            .as_bool()
            != Some(true)
        || startup_contract["startup_execution_gate_enforcement"]
        ["must_follow_true_blocks_unrelated_work"]
        .as_bool()
        != Some(true)
        || startup_contract["startup_execution_gate_enforcement"]
            ["unrelated_work_allowed_false_blocks_unrelated_work"]
            .as_bool()
            != Some(true)
        || startup_contract["startup_execution_gate_enforcement"]
            ["must_read_prompt_text_true_requires_prompt_before_reply"]
            .as_bool()
            != Some(true)
        || startup_contract["startup_execution_gate_enforcement"]["no_silent_drop_must_be_true"]
            .as_bool()
            != Some(true)
        || startup_contract["startup_execution_gate_enforcement"]
            ["required_action_kind_resume_required_value"]
            .as_str()
            != Some("resume_required_return_task")
    {
        return Err(anyhow!(
            "MCP startup contract lost startup_execution_gate_enforcement semantics"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["guard_command"].as_str()
        != Some("observe client-budget-gate")
    {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement.guard_command"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["guard_summary_field"].as_str()
        != Some("client_budget_reply_gate")
    {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement.guard_summary_field"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["reply_execution_gate_field"].as_str()
        != Some("reply_execution_gate")
        || startup_contract["live_client_budget_enforcement"]["reply_execution_gate_version"]
            .as_str()
            != Some("client-reply-budget-gate-v1")
    {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement reply_execution_gate mapping"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["reply_budget_mode_field"].as_str()
        != Some("reply_budget_mode")
        || startup_contract["live_client_budget_enforcement"]["reply_budget_contract_field"]
            .as_str()
            != Some("reply_budget_contract")
        || startup_contract["live_client_budget_enforcement"]["compact_reply_mode_value"].as_str()
            != Some(working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL)
        || startup_contract["live_client_budget_enforcement"]["compact_reply_contract_version"]
            .as_str()
            != Some(working_state::CLIENT_REPLY_BUDGET_CONTRACT_VERSION)
    {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement compact reply mapping"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["guard_enforcement_flag"].as_str()
        != Some("--enforce-reply-gate")
        || startup_contract["live_client_budget_enforcement"]["guard_enforcement_exit_on_blocking"]
            .as_bool()
            != Some(true)
    {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement hard exit gate semantics"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["compact_diagnostics_command"].as_str()
        != Some("observe client-budget-root-cause")
        || startup_contract["live_client_budget_enforcement"]
            ["must_prefer_compact_diagnostics_over_full_snapshot"]
            .as_bool()
            != Some(true)
    {
        return Err(anyhow!(
            "MCP startup contract lost compact diagnostics semantics for live client-budget enforcement"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]
        ["must_check_before_each_substantive_reply"]
        .as_bool()
        != Some(true)
    {
        return Err(anyhow!(
            "MCP startup contract does not require live client-budget guard before each substantive reply"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["max_guard_age_seconds"].as_u64()
        != Some(10)
    {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement.max_guard_age_seconds = 10"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["stale_guard_requires_refresh"].as_bool()
        != Some(true)
    {
        return Err(anyhow!(
            "MCP startup contract does not require stale live client-budget guard refresh"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["rotate_now_field"].as_str()
        != Some("should_rotate_chat_now")
        || startup_contract["live_client_budget_enforcement"]["rotate_soon_field"].as_str()
            != Some("should_rotate_chat_soon")
        || startup_contract["live_client_budget_enforcement"]["status_label_field"].as_str()
            != Some("status_label")
    {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement rotate field mapping"
        ));
    }
    let rotate_status_labels = startup_contract["live_client_budget_enforcement"]
        ["rotate_status_labels"]
        .as_array()
        .ok_or_else(|| {
            anyhow!("MCP startup contract is missing live_client_budget_enforcement.rotate_status_labels")
        })?;
    if !rotate_status_labels
        .iter()
        .any(|value| value.as_str() == Some("новый чат нужен сейчас"))
        || rotate_status_labels
            .iter()
            .any(|value| value.as_str() == Some("новый чат рекомендован"))
    {
        return Err(anyhow!(
            "MCP startup contract lost hard rotate status labels or still treats advisory rotate labels as hard-stop"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["save_handoff_before_rotate"]
        .as_bool()
        != Some(true)
        || startup_contract["live_client_budget_enforcement"]
            ["fresh_chat_requires_continuity_startup"]
            .as_bool()
            != Some(true)
        || startup_contract["live_client_budget_enforcement"]
            ["full_scale_client_truth_required"]
            .as_bool()
            != Some(true)
    {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement truth semantics"
        ));
    }
    if startup_contract["live_client_budget_enforcement"]["blocking_reply_contract_field"]
        .as_str()
        != Some("blocking_reply_contract")
        || startup_contract["live_client_budget_enforcement"]["blocking_reply_contract_version"]
            .as_str()
            != Some(working_state::CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION)
        || startup_contract["live_client_budget_enforcement"]["blocking_reply_response_kind"]
            .as_str()
            != Some(working_state::CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND)
        || startup_contract["live_client_budget_enforcement"]["blocking_reply_max_sentences"]
            .as_u64()
            != Some(working_state::CLIENT_BUDGET_BLOCKING_REPLY_MAX_SENTENCES)
        || startup_contract["live_client_budget_enforcement"]
            ["blocking_reply_must_avoid_substantive_work"]
            .as_bool()
            != Some(true)
        || startup_contract["live_client_budget_enforcement"]
            ["blocking_reply_must_use_action_bundle_operator_flow"]
            .as_bool()
            != Some(true)
        || startup_contract["live_client_budget_enforcement"]["blocking_reply_template"]
            .as_str()
            != Some(working_state::CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE)
    {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement blocked reply contract"
        ));
    }
    let blocking_action_kinds =
        startup_contract["live_client_budget_enforcement"]["blocking_action_kinds"]
            .as_array()
            .ok_or_else(|| {
                anyhow!(
                    "MCP startup contract lost live_client_budget_enforcement.blocking_action_kinds"
                )
            })?;
    if !blocking_action_kinds
        .iter()
        .any(|value| value.as_str() == Some("rotate_chat_for_client_budget"))
        || !blocking_action_kinds
            .iter()
            .any(|value| value.as_str() == Some("wait_for_global_client_budget_recovery"))
    {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement blocking action kinds"
        ));
    }
    let allowed_response_kinds = startup_contract["live_client_budget_enforcement"]
        ["blocking_reply_allowed_response_kinds"]
        .as_array()
        .ok_or_else(|| {
            anyhow!(
                "MCP startup contract lost live_client_budget_enforcement.blocking_reply_allowed_response_kinds"
            )
        })?;
    if !allowed_response_kinds.iter().any(|value| {
        value.as_str() == Some(working_state::CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_RESPONSE_KIND)
    }) || !allowed_response_kinds.iter().any(|value| {
        value.as_str() == Some(working_state::CLIENT_BUDGET_WAIT_BLOCKING_REPLY_RESPONSE_KIND)
    }) {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement allowed blocked-reply kinds"
        ));
    }
    let allowed_templates = startup_contract["live_client_budget_enforcement"]
        ["blocking_reply_allowed_templates"]
        .as_array()
        .ok_or_else(|| {
            anyhow!(
                "MCP startup contract lost live_client_budget_enforcement.blocking_reply_allowed_templates"
            )
        })?;
    if !allowed_templates.iter().any(|value| {
        value.as_str() == Some(working_state::CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_TEMPLATE)
    }) || !allowed_templates.iter().any(|value| {
        value.as_str() == Some(working_state::CLIENT_BUDGET_WAIT_BLOCKING_REPLY_TEMPLATE)
    }) {
        return Err(anyhow!(
            "MCP startup contract lost live_client_budget_enforcement allowed blocked-reply templates"
        ));
    }

    let tools = session.request("tools/list", json!({})).await?;
    let tool_names = tools["tools"]
        .as_array()
        .ok_or_else(|| anyhow!("tools/list returned invalid tools array"))?
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(ToOwned::to_owned))
        .collect::<BTreeSet<_>>();
    let expected_tools = BTreeSet::from([
        "amai_benchmark_coverage".to_string(),
        "amai_continuity_startup".to_string(),
        "amai_list_projects".to_string(),
        "amai_list_namespaces".to_string(),
        "amai_observe_whole_cycle".to_string(),
        "amai_observe_whole_cycle_turn".to_string(),
        "amai_stack_preflight".to_string(),
        "amai_context_pack".to_string(),
        "amai_token_benchmark".to_string(),
        "amai_token_report".to_string(),
        "amai_memory_matrix".to_string(),
        "amai_observe_snapshot".to_string(),
        "amai_warm_cache".to_string(),
    ]);
    if tool_names != expected_tools {
        return Err(anyhow!(
            "unexpected MCP tool set: expected {:?}, got {:?}",
            expected_tools,
            tool_names
        ));
    }

    let prompts = session.request("prompts/list", json!({})).await?;
    let prompt_names = prompts["prompts"]
        .as_array()
        .ok_or_else(|| anyhow!("prompts/list returned invalid prompts array"))?
        .iter()
        .filter_map(|prompt| prompt["name"].as_str().map(ToOwned::to_owned))
        .collect::<BTreeSet<_>>();
    let expected_prompts = BTreeSet::from([
        "amai-continuity-startup".to_string(),
        "amai-onboarding".to_string(),
        "amai-context-pack".to_string(),
    ]);
    if prompt_names != expected_prompts {
        return Err(anyhow!(
            "unexpected MCP prompt set: expected {:?}, got {:?}",
            expected_prompts,
            prompt_names
        ));
    }

    let onboarding = session
        .request("prompts/get", json!({ "name": "amai-onboarding" }))
        .await?;
    let onboarding_text = onboarding["messages"]
        .as_array()
        .and_then(|messages| messages.first())
        .and_then(|message| message["content"]["text"].as_str())
        .unwrap_or_default();
    if !onboarding_text.contains("local_strict") {
        return Err(anyhow!(
            "onboarding prompt does not teach project isolation clearly"
        ));
    }
    if !onboarding_text.contains("amai_continuity_startup") {
        return Err(anyhow!(
            "onboarding prompt does not teach canonical continuity startup tool clearly"
        ));
    }

    let list_projects = session
        .tool_call("amai_list_projects", json!({}))
        .await
        .context("MCP amai_list_projects failed")?;
    let projects = list_projects["projects"]
        .as_array()
        .ok_or_else(|| anyhow!("amai_list_projects returned invalid project array"))?;
    if !projects
        .iter()
        .any(|project| project["code"].as_str() == Some(args.context.project.as_str()))
    {
        return Err(anyhow!(
            "MCP list_projects did not return {}",
            args.context.project
        ));
    }

    let list_namespaces = session
        .tool_call(
            "amai_list_namespaces",
            json!({ "project": args.context.project }),
        )
        .await
        .context("MCP amai_list_namespaces failed")?;
    let namespaces = list_namespaces["namespaces"]
        .as_array()
        .ok_or_else(|| anyhow!("amai_list_namespaces returned invalid namespace array"))?;
    if !namespaces
        .iter()
        .any(|namespace| namespace["code"].as_str() == Some(args.context.namespace.as_str()))
    {
        return Err(anyhow!(
            "MCP list_namespaces did not return {}",
            args.context.namespace
        ));
    }

    let continuity_prompt = session
        .request(
            "prompts/get",
            json!({
                "name": "amai-continuity-startup",
                "arguments": {
                    "project": args.context.project,
                    "namespace": "continuity"
                }
            }),
        )
        .await?;
    let continuity_prompt_text = continuity_prompt["messages"]
        .as_array()
        .and_then(|messages| messages.first())
        .and_then(|message| message["content"]["text"].as_str())
        .unwrap_or_default();
    if !continuity_prompt_text.contains("amai_continuity_startup") {
        return Err(anyhow!(
            "continuity-startup prompt does not point to amai_continuity_startup"
        ));
    }

    let continuity_startup = session
        .tool_call(
            "amai_continuity_startup",
            json!({
                "project": args.context.project,
                "namespace": "continuity",
            }),
        )
        .await
        .context("MCP amai_continuity_startup failed")?;
    if continuity_startup["continuity_startup_summary"]["project_code"].as_str()
        != Some(args.context.project.as_str())
    {
        return Err(anyhow!(
            "MCP continuity startup lost primary project {}",
            args.context.project
        ));
    }
    if continuity_startup["continuity_startup_summary"]["prompt_text_present"].as_bool()
        != Some(true)
    {
        return Err(anyhow!(
            "MCP continuity startup did not materialize chat_start_restore.prompt_text"
        ));
    }
    if continuity_startup["continuity_startup_summary"]["project_task_tree_summary"]
        .as_str()
        .is_none()
    {
        return Err(anyhow!(
            "MCP continuity startup did not surface project_task_tree_summary"
        ));
    }
    if !continuity_startup["continuity_startup_summary"]["project_task_tree"].is_object() {
        return Err(anyhow!(
            "MCP continuity startup did not surface project_task_tree"
        ));
    }
    if continuity_startup["continuity_startup_summary"]["project_task_ledger_summary"]
        .as_str()
        .is_none()
    {
        return Err(anyhow!(
            "MCP continuity startup did not surface project_task_ledger_summary"
        ));
    }
    if !continuity_startup["continuity_startup_summary"]["project_task_ledger"].is_object() {
        return Err(anyhow!(
            "MCP continuity startup did not surface project_task_ledger"
        ));
    }
    if !continuity_startup["continuity_startup_summary"]["startup_next_action"].is_object() {
        return Err(anyhow!(
            "MCP continuity startup did not surface startup_next_action"
        ));
    }
    if !continuity_startup["continuity_startup_summary"]["startup_execution_gate"].is_object() {
        return Err(anyhow!(
            "MCP continuity startup did not surface startup_execution_gate"
        ));
    }
    if continuity_startup["continuity_startup_summary"]
        .get("required_return_task")
        .is_none()
    {
        return Err(anyhow!(
            "MCP continuity startup did not surface required_return_task"
        ));
    }
    if continuity_startup["continuity_startup_summary"]["execctl_active_lease_summary"]
        .as_str()
        .is_none()
    {
        return Err(anyhow!(
            "MCP continuity startup did not surface execctl_active_lease_summary"
        ));
    }
    if !continuity_startup["continuity_startup_summary"]["execctl_active_lease"].is_object() {
        return Err(anyhow!(
            "MCP continuity startup did not surface execctl_active_lease"
        ));
    }

    let preflight = session
        .tool_call("amai_stack_preflight", json!({ "profile": "default" }))
        .await
        .context("MCP amai_stack_preflight failed")?;
    if preflight["preflight_summary"]["profile_code"].as_str() != Some("default") {
        return Err(anyhow!(
            "MCP stack_preflight did not keep requested profile=default"
        ));
    }

    let benchmark_coverage = session
        .tool_call("amai_benchmark_coverage", json!({}))
        .await
        .context("MCP amai_benchmark_coverage failed")?;
    if benchmark_coverage["benchmark_coverage_summary"]["total_benchmarks"]
        .as_u64()
        .unwrap_or_default()
        == 0
    {
        return Err(anyhow!(
            "MCP benchmark coverage returned zero benchmark entries"
        ));
    }

    let context_pack = session
        .tool_call(
            "amai_context_pack",
            json!({
                "project": args.context.project,
                "namespace": args.context.namespace,
                "query": args.context.query,
                "retrieval_mode": args.context.retrieval_mode,
                "disable_cache": false,
                "limit_documents": args.context.limit_documents,
                "limit_symbols": args.context.limit_symbols,
                "limit_chunks": args.context.limit_chunks,
                "limit_semantic_chunks": args.context.limit_semantic_chunks,
                "token_source_kind": proof_context_source_kind,
                "persist": true,
            }),
        )
        .await
        .context("MCP amai_context_pack failed")?;
    if !context_pack_contains_primary_project(&context_pack, &args.context.project) {
        return Err(anyhow!(
            "MCP context pack lost primary project {}",
            args.context.project
        ));
    }
    let context_pack_id = context_pack["stats"]["context_pack_id"]
        .as_str()
        .ok_or_else(|| anyhow!("MCP context pack returned invalid stats.context_pack_id"))?;
    let proof_thread_id = format!("proof-mcp-thread-{}", Uuid::new_v4().simple());
    let proof_turn_id = format!("proof-mcp-turn-attach-{}", Uuid::new_v4().simple());
    let turn_attach = session
        .tool_call(
            "amai_observe_whole_cycle_turn",
            json!({
                "thread_id": proof_thread_id,
                "turn_id": proof_turn_id,
                "context_pack_ids": [context_pack_id],
                "assistant_generation_tokens": 41,
            }),
        )
        .await
        .context("MCP amai_observe_whole_cycle_turn failed")?;
    if turn_attach["assistant_generation_turn_observed_attach"]["assistant_generation_tokens"]
        .as_u64()
        != Some(41)
    {
        return Err(anyhow!(
            "MCP whole-cycle turn observe did not attach assistant_generation_tokens=41"
        ));
    }
    let whole_cycle_attach = session
        .tool_call(
            "amai_observe_whole_cycle",
            json!({
                "context_pack_id": context_pack_id,
                "assistant_generation_tokens": 31,
            }),
        )
        .await
        .context("MCP amai_observe_whole_cycle failed")?;
    if whole_cycle_attach["whole_cycle_observed_attach"]["whole_cycle_observed"]["assistant_generation_tokens"]
        .as_u64()
        != Some(31)
    {
        return Err(anyhow!(
            "MCP whole-cycle observe did not attach assistant_generation_tokens=31"
        ));
    }

    let token_benchmark = session
        .tool_call(
            "amai_token_benchmark",
            json!({
                "project": args.context.project,
                "namespace": args.context.namespace,
                "query": args.context.query,
                "retrieval_mode": args.context.retrieval_mode,
                "limit_documents": args.context.limit_documents,
                "limit_symbols": args.context.limit_symbols,
                "limit_chunks": args.context.limit_chunks,
                "limit_semantic_chunks": args.context.limit_semantic_chunks,
                "token_source_kind": proof_context_source_kind,
                "tokenizer": args.tokenizer,
                "naive_limit_files": args.naive_limit_files,
                "naive_max_bytes_per_file": args.naive_max_bytes_per_file,
            }),
        )
        .await
        .context("MCP amai_token_benchmark failed")?;
    let savings = &token_benchmark["token_benchmark"]["savings"];
    let savings_factor = savings["savings_factor"]
        .as_f64()
        .ok_or_else(|| anyhow!("MCP token benchmark returned invalid savings_factor"))?;
    let savings_percent = savings["savings_percent"]
        .as_f64()
        .ok_or_else(|| anyhow!("MCP token benchmark returned invalid savings_percent"))?;
    if savings_factor < args.min_savings_factor || savings_percent < args.min_savings_percent {
        return Err(anyhow!(
            "MCP token benchmark below target: factor={savings_factor:.3}, percent={savings_percent:.3}"
        ));
    }

    let snapshot = session
        .tool_call("amai_observe_snapshot", json!({}))
        .await
        .context("MCP amai_observe_snapshot failed")?;
    let critical = snapshot["snapshot"]["sla"]["summary"]["critical"]
        .as_u64()
        .unwrap_or_default();
    let unknown = snapshot["snapshot"]["sla"]["summary"]["unknown"]
        .as_u64()
        .unwrap_or_default();
    if unknown != 0 {
        return Err(anyhow!(
            "MCP observe snapshot is not green: critical={critical}, unknown={unknown}"
        ));
    }
    if critical != 0
        && !snapshot_has_only_ignored_critical_metrics(
            &snapshot["snapshot"]["sla"]["checks"],
            &["observability.benchmark_contamination"],
        )
    {
        return Err(anyhow!(
            "MCP observe snapshot is not green: critical={critical}, unknown={unknown}"
        ));
    }

    let token_report = session
        .tool_call(
            "amai_token_report",
            json!({
                "budget_profile": "codex_5h",
                "include_verify_events": true,
            }),
        )
        .await
        .context("MCP amai_token_report failed")?;
    let session_events = token_report["token_budget_report"]["current_session"]["events_total"]
        .as_u64()
        .ok_or_else(|| anyhow!("MCP token report returned invalid current_session.events_total"))?;
    if session_events == 0 {
        return Err(anyhow!(
            "MCP token report returned zero current session events"
        ));
    }
    let observed_assistant_generation_tokens = token_report["token_budget_report"]["current_session"]
        ["observed_assistant_generation_tokens"]
        .as_u64()
        .unwrap_or_default();
    if observed_assistant_generation_tokens == 0 {
        return Err(anyhow!(
            "MCP token report did not materialize observed assistant generation tokens"
        ));
    }

    let mut memory_matrix_tasks_failed = Value::Null;
    if verify_mcp_scope_requires_memory_matrix(args.proof_scope) {
        let memory_matrix = session
            .tool_call(
                "amai_memory_matrix",
                json!({
                    "matrix": "letta_memory_local",
                    "project_prefix": "memory_eval"
                }),
            )
            .await
            .context("MCP amai_memory_matrix failed")?;
        if memory_matrix["memory_matrix_summary"]["matrix"].as_str() != Some("letta_memory_local") {
            return Err(anyhow!(
                "MCP memory matrix did not keep requested matrix=letta_memory_local"
            ));
        }
        if memory_matrix["memory_matrix_summary"]["tasks_failed"]
            .as_u64()
            .unwrap_or_default()
            != 0
        {
            return Err(anyhow!("MCP memory matrix returned task failures"));
        }
        memory_matrix_tasks_failed = memory_matrix["memory_matrix_summary"]["tasks_failed"].clone();
    }

    if verify_mcp_scope_requires_warm_cache(args.proof_scope) {
        let warm = session
            .tool_call(
                "amai_warm_cache",
                json!({
                    "projects": [args.context.project.clone()],
                    "namespace": args.context.namespace,
                    "query": args.context.query,
                    "retrieval_mode": args.context.retrieval_mode,
                    "limit_documents": args.context.limit_documents,
                    "limit_symbols": args.context.limit_symbols,
                    "limit_chunks": args.context.limit_chunks,
                    "limit_semantic_chunks": args.context.limit_semantic_chunks,
                }),
            )
            .await
            .context("MCP amai_warm_cache failed")?;
        let warmed = warm["warmup_cache"]["warmed"]
            .as_array()
            .ok_or_else(|| anyhow!("MCP warm cache returned invalid warmed array"))?;
        if warmed.is_empty() {
            return Err(anyhow!("MCP warm cache returned no warmed entries"));
        }
    }

    let result = json!({
        "mcp_verification": {
            "protocol_version": MCP_PROTOCOL_VERSION,
            "proof_scope": verify_mcp_scope_label(args.proof_scope),
            "tools": tool_names,
            "prompts": prompt_names,
            "benchmark_coverage_total": benchmark_coverage["benchmark_coverage_summary"]["total_benchmarks"].clone(),
            "token_savings_factor": savings_factor,
            "token_savings_percent": savings_percent,
            "token_report_session_events": session_events,
            "memory_matrix_tasks_failed": memory_matrix_tasks_failed,
            "critical": critical,
            "unknown": unknown,
        }
    });
    println!("{}", serde_json::to_string_pretty(&result)?);

    session.shutdown().await?;
    Ok(())
}

pub(crate) struct McpProofSession {
    child: Child,
    stdin: ChildStdin,
    stdout: tokio::io::Lines<BufReader<ChildStdout>>,
    next_id: u64,
    protocol_manifest: Value,
}

fn inject_proof_tool_arguments(name: &str, arguments: Value) -> Value {
    let mut object = match arguments {
        Value::Object(map) => map,
        other => return other,
    };
    match name {
        "amai_context_pack" | "amai_token_benchmark" => {
            object
                .entry("token_source_kind".to_string())
                .or_insert_with(|| Value::String("proof_mcp_context_pack".to_string()));
        }
        "amai_continuity_startup" => {
            object
                .entry("token_source_kind".to_string())
                .or_insert_with(|| Value::String("proof_mcp_continuity_startup".to_string()));
        }
        _ => {}
    }
    Value::Object(object)
}

impl McpProofSession {
    pub(crate) async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        write_message(&mut self.stdin, &request).await?;
        let line = timeout(Duration::from_secs(30), self.stdout.next_line())
            .await
            .context("timed out waiting for MCP response")?
            .context("failed to read MCP response line")?
            .ok_or_else(|| anyhow!("MCP server closed stdout unexpectedly"))?;
        let response: Value =
            serde_json::from_str(&line).context("failed to decode MCP response JSON")?;
        if response["id"] != json!(id) {
            return Err(anyhow!(
                "MCP response id mismatch: expected {id}, got {}",
                response["id"]
            ));
        }
        if response.get("error").is_some() {
            return Err(anyhow!(
                "MCP request {method} failed: {}",
                response["error"]
            ));
        }
        Ok(response["result"].clone())
    }

    pub(crate) async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        write_message(&mut self.stdin, &notification).await
    }

    pub(crate) async fn tool_call_raw(&mut self, name: &str, arguments: Value) -> Result<Value> {
        self.request(
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments,
            }),
        )
        .await
    }

    pub(crate) async fn tool_call(&mut self, name: &str, arguments: Value) -> Result<Value> {
        let result = self
            .tool_call_raw(name, inject_proof_tool_arguments(name, arguments))
            .await?;
        if result["isError"].as_bool().unwrap_or(false) {
            return Err(anyhow!(
                "MCP tool {} returned isError=true: {}",
                name,
                result["content"]
            ));
        }
        Ok(result["structuredContent"].clone())
    }

    pub(crate) async fn shutdown(mut self) -> Result<()> {
        self.child
            .kill()
            .await
            .context("failed to terminate MCP proof server")?;
        Ok(())
    }
}

pub(crate) async fn spawn_proof_session(cfg: &AppConfig) -> Result<McpProofSession> {
    compatibility::assert_supported(cfg).await?;

    let exe = std::env::current_exe().context("failed to resolve current amai executable")?;
    let mut child = ProcessCommand::new(&exe)
        .arg("mcp")
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("failed to spawn MCP server from {}", exe.display()))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("failed to capture MCP server stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("failed to capture MCP server stdout"))?;

    let mut session = McpProofSession {
        child,
        stdin,
        stdout: BufReader::new(stdout).lines(),
        next_id: 1,
        protocol_manifest: Value::Null,
    };

    let init = session
        .request(
            "initialize",
            json!({
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {
                    "name": "amai-proof",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        )
        .await?;
    let server_info = &init["serverInfo"];
    if server_info["name"].as_str() != Some(SERVER_NAME) {
        return Err(anyhow!(
            "unexpected MCP server name: {:?}",
            server_info["name"]
        ));
    }
    let protocol_manifest = init["amai_protocol_manifest"].clone();
    session
        .notify("notifications/initialized", json!({}))
        .await
        .context("failed to send MCP initialized notification")?;
    session.protocol_manifest = protocol_manifest;
    Ok(session)
}

async fn handle_request(cfg: &AppConfig, incoming: Value) -> Result<Value> {
    let id = incoming["id"].clone();
    let method = match incoming["method"].as_str() {
        Some(method) => method,
        None => {
            return Ok(mcp_jsonrpc_error_response(
                id,
                &McpError::invalid_request("JSON-RPC request is missing method"),
            ));
        }
    };
    let params = incoming.get("params").cloned().unwrap_or_else(|| json!({}));
    let response = match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": MCP_PROTOCOL_VERSION,
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": env!("CARGO_PKG_VERSION"),
                    "title": "Art-memory-agent-index (Amai)",
                },
                "capabilities": {
                    "tools": { "listChanged": false },
                    "prompts": { "listChanged": false },
                },
                "instructions": server_instructions(),
                "amai_protocol_manifest": protocol_manifest(),
            }
        }),
        "ping" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {}
        }),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": tool_definitions()
            }
        }),
        "prompts/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "prompts": prompt_definitions()
            }
        }),
        "prompts/get" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": match prompt_result(params) {
                Ok(result) => result,
                Err(error) => return Ok(mcp_jsonrpc_error_response(id, &error)),
            },
        }),
        "tools/call" => {
            let request: ToolCallRequest = match serde_json::from_value(params) {
                Ok(request) => request,
                Err(error) => {
                    return Ok(mcp_jsonrpc_error_response(
                        id,
                        &McpError::invalid_params(format!(
                            "failed to decode tool call request: {error}"
                        )),
                    ));
                }
            };
            let result = match handle_tool_call(cfg, request).await {
                Ok(result) => result,
                Err(error) => mcp_tool_error_result(&error),
            };
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            })
        }
        other => mcp_jsonrpc_error_response(id, &McpError::method_not_found(other)),
    };
    Ok(response)
}

async fn handle_tool_call(cfg: &AppConfig, request: ToolCallRequest) -> McpToolResult<Value> {
    match request.name.as_str() {
        "amai_list_projects" => tool_list_projects(cfg)
            .await
            .map_err(McpError::tool_runtime),
        "amai_list_namespaces" => {
            let args: ListNamespacesArgs = parse_arguments(request.arguments)?;
            tool_list_namespaces(cfg, args)
                .await
                .map_err(McpError::tool_runtime)
        }
        "amai_stack_preflight" => {
            let args: StackPreflightToolArgs = parse_arguments(request.arguments)?;
            tool_stack_preflight(args)
                .await
                .map_err(McpError::tool_runtime)
        }
        "amai_benchmark_coverage" => tool_benchmark_coverage()
            .await
            .map_err(McpError::tool_runtime),
        "amai_continuity_startup" => {
            let args: ContinuityStartupToolArgs = parse_arguments(request.arguments)?;
            tool_continuity_startup(cfg, args)
                .await
                .map_err(McpError::tool_runtime)
        }
        "amai_context_pack" => {
            let args: ContextPackToolArgs = parse_arguments(request.arguments)?;
            tool_context_pack(cfg, args)
                .await
                .map_err(McpError::tool_runtime)
        }
        "amai_observe_whole_cycle" => {
            let args: ObserveWholeCycleToolArgs = parse_arguments(request.arguments)?;
            tool_observe_whole_cycle(cfg, args)
                .await
                .map_err(McpError::tool_runtime)
        }
        "amai_observe_whole_cycle_turn" => {
            let args: ObserveWholeCycleTurnToolArgs = parse_arguments(request.arguments)?;
            tool_observe_whole_cycle_turn(cfg, args)
                .await
                .map_err(McpError::tool_runtime)
        }
        "amai_token_benchmark" => {
            let args: TokenBenchmarkToolArgs = parse_arguments(request.arguments)?;
            tool_token_benchmark(cfg, args)
                .await
                .map_err(McpError::tool_runtime)
        }
        "amai_token_report" => {
            let args: TokenReportToolArgs = parse_arguments(request.arguments)?;
            tool_token_report(cfg, args)
                .await
                .map_err(McpError::tool_runtime)
        }
        "amai_memory_matrix" => {
            let args: MemoryMatrixToolArgs = parse_arguments(request.arguments)?;
            tool_memory_matrix(cfg, args)
                .await
                .map_err(McpError::tool_runtime)
        }
        "amai_observe_snapshot" => tool_observe_snapshot(cfg)
            .await
            .map_err(McpError::tool_runtime),
        "amai_warm_cache" => {
            let args: WarmCacheToolArgs = parse_arguments(request.arguments)?;
            tool_warm_cache(cfg, args)
                .await
                .map_err(McpError::tool_runtime)
        }
        other => Err(McpError::tool_not_found(other)),
    }
}

async fn tool_list_projects(cfg: &AppConfig) -> Result<Value> {
    let db = postgres::connect_admin(cfg).await?;
    let projects = postgres::list_projects(&db).await?;
    let project_summary = summarize_codes(
        &projects
            .iter()
            .map(|project| project.code.as_str())
            .collect::<Vec<_>>(),
    );
    let structured = json!({
        "projects_summary": {
            "codes": projects.iter().map(|project| project.code.clone()).collect::<Vec<_>>(),
            "compact_codes": project_summary,
        },
        "projects": projects.iter().map(|project| {
            json!({
                "project_id": project.project_id,
                "code": project.code,
                "display_name": project.display_name,
                "repo_root": project.repo_root,
                "updated_at": project.updated_at,
            })
        }).collect::<Vec<_>>()
    });
    Ok(tool_result(
        format!(
            "registered projects: {} [{}]",
            structured["projects"].as_array().map_or(0, Vec::len),
            structured["projects_summary"]["compact_codes"]
                .as_str()
                .unwrap_or("none")
        ),
        structured,
    ))
}

async fn tool_list_namespaces(cfg: &AppConfig, args: ListNamespacesArgs) -> Result<Value> {
    let db = postgres::connect_admin(cfg).await?;
    let project = postgres::get_project_by_code(&db, &args.project).await?;
    let namespaces = postgres::list_namespaces_for_project(&db, project.project_id).await?;
    let namespace_summary = summarize_namespace_modes(
        &namespaces
            .iter()
            .map(|namespace| (namespace.code.as_str(), namespace.retrieval_mode.as_str()))
            .collect::<Vec<_>>(),
    );
    let structured = json!({
        "project": {
            "code": project.code,
            "display_name": project.display_name,
            "repo_root": project.repo_root,
        },
        "namespaces_summary": {
            "compact_codes": namespace_summary,
        },
        "namespaces": namespaces.iter().map(|namespace| {
            json!({
                "namespace_id": namespace.namespace_id,
                "code": namespace.code,
                "display_name": namespace.display_name,
                "retrieval_mode": namespace.retrieval_mode,
            })
        }).collect::<Vec<_>>()
    });
    Ok(tool_result(
        format!(
            "namespaces for {}: {} [{}]",
            args.project,
            structured["namespaces"].as_array().map_or(0, Vec::len),
            structured["namespaces_summary"]["compact_codes"]
                .as_str()
                .unwrap_or("none")
        ),
        structured,
    ))
}

async fn tool_stack_preflight(args: StackPreflightToolArgs) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)?;
    let report = profiles::preflight_report(&repo_root, &args.profile)?;
    let preflight = profiles::report_json(&report);
    let summary = stack_preflight_summary(&preflight);
    Ok(tool_result(
        format!(
            "stack preflight :: profile={} verdict={} cpu={} memory={:.2}GiB disk={:.2}GiB peak_benchmarks={} monitoring_default={} remote_recommended={}",
            summary.profile_code,
            summary.verdict,
            summary.host_logical_cpus,
            summary.host_total_memory_gib,
            summary.host_available_disk_gib,
            summary.supports_peak_benchmarks,
            summary.start_monitoring_by_default,
            summary.remote_mode_recommended,
        ),
        json!({
            "preflight_report": preflight,
            "preflight_summary": {
                "profile_code": summary.profile_code,
                "profile_display_name": summary.profile_display_name,
                "verdict": summary.verdict,
                "host_logical_cpus": summary.host_logical_cpus,
                "host_total_memory_gib": summary.host_total_memory_gib,
                "host_available_disk_gib": summary.host_available_disk_gib,
                "supports_peak_benchmarks": summary.supports_peak_benchmarks,
                "start_monitoring_by_default": summary.start_monitoring_by_default,
                "remote_mode_recommended": summary.remote_mode_recommended,
                "unmet_minimums_count": summary.unmet_minimums_count,
                "unmet_recommendations_count": summary.unmet_recommendations_count,
            }
        }),
    ))
}

async fn tool_benchmark_coverage() -> Result<Value> {
    let repo_root = config::discover_repo_root(None)?;
    let payload = benchmark_matrix::coverage_json(&repo_root)?;
    let summary = benchmark_coverage_summary(&payload);
    Ok(tool_result(
        format!(
            "benchmark coverage :: total={} materialized={} partial={} mapped={} next_priority={} future={} next={}",
            summary.total_benchmarks,
            summary.materialized,
            summary.partial,
            summary.mapped,
            summary.next_priority,
            summary.future,
            summary.next_priorities_summary,
        ),
        json!({
            "benchmark_coverage": payload,
            "benchmark_coverage_summary": {
                "source_display_name": summary.source_display_name,
                "total_benchmarks": summary.total_benchmarks,
                "materialized": summary.materialized,
                "partial": summary.partial,
                "mapped": summary.mapped,
                "next_priority": summary.next_priority,
                "future": summary.future,
                "next_priorities_summary": summary.next_priorities_summary,
            }
        }),
    ))
}

async fn tool_continuity_startup(
    cfg: &AppConfig,
    args: ContinuityStartupToolArgs,
) -> Result<Value> {
    let payload = continuity::startup_payload(cfg, &args.to_cli_args()).await?;
    let summary = continuity_startup_summary(&payload);
    let summary_json = continuity_startup_summary_json(&payload);
    Ok(tool_result(
        format!(
            "continuity startup :: {}::{} headline={} next_step={} execctl={} pending_return={}",
            summary.project_code,
            summary.namespace_code,
            summary.headline,
            summary.next_step,
            summary.execctl_resume_state,
            summary
                .execctl_resume_contract_summary
                .as_deref()
                .unwrap_or_else(|| summary.pending_return_summary.as_deref().unwrap_or("none")),
        ),
        json!({
            "continuity_startup": payload["continuity_startup"].clone(),
            "chat_start_restore": payload["chat_start_restore"].clone(),
            "working_state_restore": payload["working_state_restore"].clone(),
            "retrieval_science": payload["retrieval_science"].clone(),
            "degradation_policy": payload["degradation_policy"].clone(),
            "continuity_startup_summary": summary_json
        }),
    ))
}

fn summarize_codes(codes: &[&str]) -> String {
    summarize_with_limit(
        &codes
            .iter()
            .map(|code| (*code).to_string())
            .collect::<Vec<_>>(),
    )
}

fn summarize_namespace_modes(items: &[(&str, &str)]) -> String {
    summarize_with_limit(
        &items
            .iter()
            .map(|(code, mode)| format!("{code}={mode}"))
            .collect::<Vec<_>>(),
    )
}

fn summarize_with_limit(items: &[String]) -> String {
    if items.is_empty() {
        return "none".to_string();
    }
    let preview = items.iter().take(3).cloned().collect::<Vec<_>>();
    if items.len() > preview.len() {
        format!(
            "{} +{} more",
            preview.join(", "),
            items.len() - preview.len()
        )
    } else {
        preview.join(", ")
    }
}

fn summarize_verdict_counts(value: &Value) -> String {
    let Some(object) = value.as_object() else {
        return "none".to_string();
    };
    let parts = object
        .iter()
        .filter_map(|(key, count)| {
            let count = count.as_u64()?;
            Some(format!("{key}={count}"))
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join(", ")
    }
}

async fn tool_context_pack(cfg: &AppConfig, args: ContextPackToolArgs) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let mut db = postgres::connect_admin(cfg).await?;
    let context = args.to_context_args();
    let result =
        retrieval::execute_context_pack_capture(cfg, &mut db, &context, args.persist).await?;
    let model_visible_payload = retrieval::model_visible_context_pack_payload(&result.payload);
    let context_summary = context_pack_summary(&result.payload);
    let summary_block = json!({
        "included_reasons_summary": context_summary.included_reasons_summary.clone(),
        "excluded_reasons_summary": context_summary.excluded_reasons_summary.clone(),
    });
    let stats_block = context_pack_tool_stats_block(&result.stats);
    let structured = json!({
        "context_pack": model_visible_payload,
        "context_pack_summary": summary_block.clone(),
        "stats": stats_block.clone(),
    });
    let summary = context_pack_tool_summary(&result.stats);
    token_budget::observe_context_pack_tool_overhead(
        &mut db,
        &result.stats.context_pack_id.to_string(),
        &summary,
        &structured,
    )
    .await?;
    Ok(tool_result(summary, structured))
}

fn context_pack_tool_stats_block(stats: &retrieval::ContextPackStats) -> Value {
    let mut block = json!({
        "context_pack_id": stats.context_pack_id,
        "retrieval_counts": {
            "exact_documents": stats.exact_documents,
            "symbol_hits": stats.symbol_hits,
            "lexical_chunks": stats.lexical_chunks,
            "semantic_chunks": stats.semantic_chunks,
        }
    });
    if stats.cache_hit {
        block["cache_hit"] = Value::Bool(true);
    }
    block
}

fn context_pack_tool_summary(stats: &retrieval::ContextPackStats) -> String {
    let mut summary = format!(
        "ctx d={} s={} l={} m={}",
        stats.exact_documents, stats.symbol_hits, stats.lexical_chunks, stats.semantic_chunks
    );
    if stats.cache_hit {
        summary.push_str(" c=1");
    }
    summary
}

async fn tool_observe_whole_cycle(
    cfg: &AppConfig,
    args: ObserveWholeCycleToolArgs,
) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let db = postgres::connect_admin(cfg).await?;
    let structured = token_budget::attach_whole_cycle_observed_to_context_pack(
        &db,
        &args.context_pack_id,
        args.client_prompt_tokens,
        args.assistant_generation_tokens,
        args.tool_overhead_tokens,
        args.continuity_restore_tokens,
    )
    .await?;
    let attach = &structured["whole_cycle_observed_attach"];
    let updated = attach["updated_fields"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "none".to_string());
    let retained = attach["retained_fields"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "none".to_string());
    Ok(tool_result(
        format!(
            "whole-cycle observed attached for {} :: updated={} retained={}",
            args.context_pack_id, updated, retained
        ),
        structured,
    ))
}

async fn tool_observe_whole_cycle_turn(
    cfg: &AppConfig,
    args: ObserveWholeCycleTurnToolArgs,
) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let db = postgres::connect_admin(cfg).await?;
    let context_pack_ids = args
        .context_pack_ids
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    let structured = token_budget::attach_whole_cycle_observed_to_turn_group_with_thread_hint(
        &db,
        args.thread_id.as_deref(),
        &args.turn_id,
        &context_pack_ids,
        args.assistant_generation_tokens,
    )
    .await?;
    let attach = &structured["assistant_generation_turn_observed_attach"];
    Ok(tool_result(
        format!(
            "turn-scoped assistant generation attached for {} :: context_packs={} inferred_thread={}",
            attach["turn_id"].as_str().unwrap_or("unknown"),
            attach["context_pack_ids"]
                .as_array()
                .map_or(0, |items| items.len()),
            attach["thread_id_inferred"].as_bool().unwrap_or(false)
        ),
        structured,
    ))
}

async fn tool_token_benchmark(cfg: &AppConfig, args: TokenBenchmarkToolArgs) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let mut db = postgres::connect_admin(cfg).await?;
    let payload = verify::collect_token_benchmark(cfg, &mut db, &args.to_verify_args()).await?;
    let benchmark_summary = token_benchmark_summary(&payload);
    let summary = format!(
        "token benchmark :: saved_tokens={} savings_factor={:.3} savings_percent={:.3} naive_tokens={} context_tokens={} files_considered={}",
        benchmark_summary.saved_tokens,
        benchmark_summary.savings_factor,
        benchmark_summary.savings_percent,
        benchmark_summary.naive_tokens,
        benchmark_summary.context_tokens,
        benchmark_summary.files_considered,
    );
    Ok(tool_result(
        summary,
        json!({
            "token_benchmark": payload["token_benchmark"].clone(),
            "token_benchmark_summary": {
                "saved_tokens": benchmark_summary.saved_tokens,
                "savings_factor": benchmark_summary.savings_factor,
                "savings_percent": benchmark_summary.savings_percent,
                "naive_tokens": benchmark_summary.naive_tokens,
                "context_tokens": benchmark_summary.context_tokens,
                "files_considered": benchmark_summary.files_considered,
            }
        }),
    ))
}

async fn tool_token_report(cfg: &AppConfig, args: TokenReportToolArgs) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let db = postgres::connect_admin(cfg).await?;
    let payload = token_budget::collect_default_report_with_overrides(
        &db,
        args.budget_profile.as_deref(),
        args.include_verify_events,
    )
    .await?;
    let token_summary = token_report_summary(&payload);
    let summary = format!(
        "token report :: metric={} scope={} status={} value_percent={:.3} saved_tokens={} counted={}/{} agent_cycle_scope={} agent_cycle_verified_percent={:.3} contractual_scope={} contractual_state={} coverage={} freshness={} lag={} reconciliation={} margin={} blockers={} note={}",
        token_summary.metric_code,
        token_summary.scope_label,
        token_summary.status,
        token_summary.value_percent,
        token_summary.saved_tokens,
        token_summary.counted_events,
        token_summary.events_count,
        token_summary.agent_cycle_scope_label,
        token_summary.agent_cycle_verified_saved_percent,
        token_summary.contractual_scope_label,
        token_summary.contractual_state,
        token_summary.contractual_coverage_state,
        token_summary.contractual_freshness_state,
        token_summary.contractual_lag_state,
        token_summary.contractual_reconciliation_state,
        token_summary.contractual_margin_state,
        token_summary.contractual_blockers_summary,
        token_summary.note,
    );
    Ok(tool_result(
        summary,
        json!({
            "token_budget_report": payload["token_budget_report"].clone(),
            "token_report_summary": {
                "metric_code": token_summary.metric_code,
                "scope_label": token_summary.scope_label,
                "status": token_summary.status,
                "value_percent": token_summary.value_percent,
                "saved_tokens": token_summary.saved_tokens,
                "events_count": token_summary.events_count,
                "counted_events": token_summary.counted_events,
                "agent_cycle_scope_label": token_summary.agent_cycle_scope_label,
                "agent_cycle_status": token_summary.agent_cycle_status,
                "agent_cycle_verified_saved_percent": token_summary.agent_cycle_verified_saved_percent,
                "agent_cycle_verified_saved_tokens": token_summary.agent_cycle_verified_saved_tokens,
                "agent_cycle_note": token_summary.agent_cycle_note,
                "contractual_scope_label": token_summary.contractual_scope_label,
                "contractual_state": token_summary.contractual_state,
                "contractual_coverage_state": token_summary.contractual_coverage_state,
                "contractual_metering_ingest_state": token_summary.contractual_metering_ingest_state,
                "contractual_lag_state": token_summary.contractual_lag_state,
                "contractual_freshness_state": token_summary.contractual_freshness_state,
                "contractual_reconciliation_state": token_summary.contractual_reconciliation_state,
                "contractual_margin_state": token_summary.contractual_margin_state,
                "contractual_blockers_summary": token_summary.contractual_blockers_summary,
                "contractual_statement_summary": if payload["token_budget_report"]["contractual_statement_summaries"]["rolling_window"].is_null() {
                    payload["token_budget_report"]["contractual_statement_summaries"]["lifetime"].clone()
                } else {
                    payload["token_budget_report"]["contractual_statement_summaries"]["rolling_window"].clone()
                },
                "statement_export_preview": if payload["token_budget_report"]["statement_export_previews"]["rolling_window"].is_null() {
                    payload["token_budget_report"]["statement_export_previews"]["lifetime"].clone()
                } else {
                    payload["token_budget_report"]["statement_export_previews"]["rolling_window"].clone()
                },
                "note": token_summary.note,
            }
        }),
    ))
}

async fn tool_memory_matrix(cfg: &AppConfig, args: MemoryMatrixToolArgs) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let payload = memory_task_matrix::collect_matrix(cfg, &args.to_verify_args()).await?;
    let matrix_summary = memory_matrix_summary(&payload);
    let summary = format!(
        "memory matrix :: matrix={} tasks={}/{} failed={} success_rate={:.3} mean_score={:.3} p95_ms={:.3} gate_failures={} verdicts={}",
        matrix_summary.matrix,
        matrix_summary.tasks_passed,
        matrix_summary.tasks_total,
        matrix_summary.tasks_failed,
        matrix_summary.success_rate,
        matrix_summary.mean_score,
        matrix_summary.p95_ms,
        matrix_summary.gate_failures_count,
        matrix_summary.compact_verdict_counts,
    );
    Ok(tool_result(
        summary,
        json!({
            "memory_task_matrix": payload["memory_task_matrix"].clone(),
            "memory_matrix_summary": {
                "matrix": matrix_summary.matrix,
                "display_name": matrix_summary.display_name,
                "tasks_total": matrix_summary.tasks_total,
                "tasks_passed": matrix_summary.tasks_passed,
                "tasks_failed": matrix_summary.tasks_failed,
                "success_rate": matrix_summary.success_rate,
                "mean_score": matrix_summary.mean_score,
                "p95_ms": matrix_summary.p95_ms,
                "gate_failures_count": matrix_summary.gate_failures_count,
                "compact_verdict_counts": matrix_summary.compact_verdict_counts,
            }
        }),
    ))
}

async fn tool_observe_snapshot(cfg: &AppConfig) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let snapshot = observe::collect_snapshot_preview(cfg).await?;
    let summary = observe_snapshot_summary(&snapshot);
    let mut text = format!(
        "observe snapshot :: pass={} alert={} critical={} unknown={}",
        summary.pass, summary.alert, summary.critical, summary.unknown,
    );
    if let Some(profile) = &summary.compatibility_profile {
        let state = if summary.compatibility_compatible == Some(true) {
            "ok"
        } else {
            "drift"
        };
        text.push_str(&format!(" compatibility={profile}:{state}"));
    }
    if let Some(status) = &summary.continuity_status {
        text.push_str(&format!(
            " continuity={}/{}:{}",
            summary.continuity_verified_probes, summary.continuity_total_probes, status
        ));
    }
    if let Some(value) = &summary.included_reasons_summary {
        text.push_str(&format!(" included={value}"));
    }
    if let Some(value) = &summary.excluded_reasons_summary {
        text.push_str(&format!(" excluded={value}"));
    }
    Ok(tool_result(
        text,
        json!({
            "snapshot": snapshot,
            "observe_snapshot_summary": {
                "continuity_status": summary.continuity_status,
                "continuity_verified_probes": summary.continuity_verified_probes,
                "continuity_total_probes": summary.continuity_total_probes,
                "compatibility_profile": summary.compatibility_profile,
                "compatibility_compatible": summary.compatibility_compatible,
                "included_reasons_summary": summary.included_reasons_summary,
                "excluded_reasons_summary": summary.excluded_reasons_summary,
            }
        }),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObserveSnapshotSummary {
    pass: u64,
    alert: u64,
    critical: u64,
    unknown: u64,
    continuity_status: Option<String>,
    continuity_verified_probes: u64,
    continuity_total_probes: u64,
    compatibility_profile: Option<String>,
    compatibility_compatible: Option<bool>,
    included_reasons_summary: Option<String>,
    excluded_reasons_summary: Option<String>,
}

fn observe_snapshot_summary(snapshot: &Value) -> ObserveSnapshotSummary {
    let sla = &snapshot["sla"]["summary"];
    ObserveSnapshotSummary {
        pass: sla["pass"].as_u64().unwrap_or_default(),
        alert: sla["alert"].as_u64().unwrap_or_default(),
        critical: sla["critical"].as_u64().unwrap_or_default(),
        unknown: sla["unknown"].as_u64().unwrap_or_default(),
        continuity_status: snapshot["continuity_correctness_model"]["summary"]["status"]
            .as_str()
            .map(ToOwned::to_owned),
        continuity_verified_probes:
            snapshot["continuity_correctness_model"]["summary"]["verified_probes"]
                .as_u64()
                .unwrap_or_default(),
        continuity_total_probes: snapshot["continuity_correctness_model"]["summary"]["probe_count"]
            .as_u64()
            .unwrap_or_default(),
        compatibility_profile: snapshot["compatibility"]["profile"]
            .as_str()
            .map(ToOwned::to_owned),
        compatibility_compatible: snapshot["compatibility"]["compatible"].as_bool(),
        included_reasons_summary: observe_snapshot_reason_summary(
            snapshot,
            "included_reasons_summary",
            "included",
        ),
        excluded_reasons_summary: observe_snapshot_reason_summary(
            snapshot,
            "excluded_reasons_summary",
            "not_included",
        ),
    }
}

fn observe_snapshot_reason_summary(
    snapshot: &Value,
    summary_key: &str,
    trace_key: &str,
) -> Option<String> {
    let restore = &snapshot["latest_working_state_restore"]["working_state_restore"];
    if let Some(value) = restore[summary_key]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }
    let items = restore["latest_decision_trace"][trace_key].as_array()?;
    let parts = items
        .iter()
        .take(3)
        .filter_map(|item| {
            let reason = item["reason"].as_str()?.trim();
            if reason.is_empty() {
                return None;
            }
            let strategy = item["strategy"].as_str().unwrap_or("unknown");
            let count = item["count"].as_u64();
            Some(match count {
                Some(value) if value > 0 => format!("{strategy} ({value}) — {reason}"),
                _ => format!("{strategy} — {reason}"),
            })
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" • "))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct TokenReportSummary {
    metric_code: String,
    scope_label: String,
    status: String,
    value_percent: f64,
    saved_tokens: i64,
    events_count: u64,
    counted_events: u64,
    agent_cycle_scope_label: String,
    agent_cycle_status: String,
    agent_cycle_verified_saved_percent: f64,
    agent_cycle_verified_saved_tokens: i64,
    agent_cycle_note: String,
    contractual_scope_label: String,
    contractual_state: String,
    contractual_coverage_state: String,
    contractual_metering_ingest_state: String,
    contractual_lag_state: String,
    contractual_freshness_state: String,
    contractual_reconciliation_state: String,
    contractual_margin_state: String,
    contractual_blockers_summary: String,
    note: String,
}

#[derive(Debug, Clone, PartialEq)]
struct MemoryMatrixSummary {
    matrix: String,
    display_name: String,
    tasks_total: u64,
    tasks_passed: u64,
    tasks_failed: u64,
    success_rate: f64,
    mean_score: f64,
    p95_ms: f64,
    gate_failures_count: u64,
    compact_verdict_counts: String,
}

#[derive(Debug, Clone, PartialEq)]
struct BenchmarkCoverageSummary {
    source_display_name: String,
    total_benchmarks: u64,
    materialized: u64,
    partial: u64,
    mapped: u64,
    next_priority: u64,
    future: u64,
    next_priorities_summary: String,
}

fn benchmark_coverage_summary(payload: &Value) -> BenchmarkCoverageSummary {
    let counts = &payload["coverage_counts"];
    let next_priorities = payload["families"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|family| family["next_priorities"].as_array().into_iter().flatten())
        .filter_map(|item| item.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    BenchmarkCoverageSummary {
        source_display_name: payload["source"]["display_name"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        total_benchmarks: counts["total"].as_u64().unwrap_or_default(),
        materialized: counts["materialized"].as_u64().unwrap_or_default(),
        partial: counts["partial"].as_u64().unwrap_or_default(),
        mapped: counts["mapped"].as_u64().unwrap_or_default(),
        next_priority: counts["next_priority"].as_u64().unwrap_or_default(),
        future: counts["future"].as_u64().unwrap_or_default(),
        next_priorities_summary: summarize_with_limit(&next_priorities),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContinuityStartupSummary {
    project_code: String,
    namespace_code: String,
    headline: String,
    next_step: String,
    restore_confidence: String,
    thread_count: u64,
    prompt_text_present: bool,
    execctl_resume_state: String,
    pending_return_summary: Option<String>,
    execctl_resume_contract_summary: Option<String>,
    execctl_resume_obligation: Value,
    startup_execution_gate: Value,
    startup_next_action: Value,
    startup_next_action_summary: Option<String>,
    execctl_active_lease: Value,
    execctl_active_lease_summary: Option<String>,
    required_return_task: Value,
    project_task_tree: Value,
    project_task_tree_summary: Option<String>,
    project_task_ledger: Value,
    project_task_ledger_summary: Option<String>,
    included_reasons_summary: Option<String>,
    excluded_reasons_summary: Option<String>,
}

fn fallback_startup_execution_gate(payload: &Value) -> Value {
    let contract = project_chat_startup_contract();
    let resume_enforcement = &contract["resume_enforcement"];
    let action_kind = payload["chat_start_restore"]["startup_next_action"]["action_kind"]
        .as_str()
        .unwrap_or("continue_active_workline");
    let lease_owner_state =
        payload["chat_start_restore"]["execctl_active_lease"]["lease_owner_state"].as_str();
    let previous_session_owner_value = resume_enforcement["previous_session_owner_value"]
        .as_str()
        .unwrap_or("previous_session_owner");
    let must_resume_before_unrelated =
        resume_enforcement["must_resume_required_return_task_before_unrelated_work"]
            .as_bool()
            .unwrap_or(false);
    let required_action_kind = resume_enforcement["required_action_kind_when_resume_required"]
        .as_str()
        .unwrap_or("resume_required_return_task");
    let must_follow = (must_resume_before_unrelated && action_kind == required_action_kind)
        || lease_owner_state == Some(previous_session_owner_value);

    json!({
        "gate_version": "startup-execution-gate-v1",
        "action_kind": action_kind,
        "blocking": payload["chat_start_restore"]["startup_next_action"]["blocking"]
            .as_bool()
            .unwrap_or(false),
        "resume_state": payload["chat_start_restore"]["execctl_resume_state"]
            .as_str()
            .unwrap_or("clear"),
        "required_return_task_present": payload["chat_start_restore"]["required_return_task"].is_object(),
        "required_return_task_headline": payload["chat_start_restore"]["required_return_task"]["headline"]
            .as_str(),
        "required_return_task_next_step": payload["chat_start_restore"]["required_return_task"]["next_step"]
            .as_str(),
        "lease_owner_state": lease_owner_state,
        "must_follow_startup_next_action": must_follow,
        "unrelated_work_allowed": !must_follow,
        "must_read_prompt_text_before_reply": payload["chat_start_restore"]["prompt_text"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        "required_action_kind_when_resume_required": required_action_kind,
        "no_silent_drop": resume_enforcement["no_silent_drop"]
            .as_bool()
            .unwrap_or(false),
    })
}

fn continuity_startup_summary(payload: &Value) -> ContinuityStartupSummary {
    ContinuityStartupSummary {
        project_code: payload["continuity_startup"]["project"]["code"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        namespace_code: payload["continuity_startup"]["namespace"]["code"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        headline: payload["chat_start_restore"]["headline"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        next_step: payload["chat_start_restore"]["next_step"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        restore_confidence: payload["chat_start_restore"]["restore_confidence"]
            .as_str()
            .unwrap_or("preliminary")
            .to_string(),
        thread_count: payload["chat_start_restore"]["thread_count"]
            .as_u64()
            .unwrap_or_default(),
        prompt_text_present: payload["chat_start_restore"]["prompt_text"]
            .as_str()
            .is_some_and(|value| !value.trim().is_empty()),
        execctl_resume_state: payload["chat_start_restore"]["execctl_resume_state"]
            .as_str()
            .unwrap_or("clear")
            .to_string(),
        pending_return_summary: payload["chat_start_restore"]["pending_return_summary"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        execctl_resume_contract_summary:
            payload["chat_start_restore"]["execctl_resume_contract_summary"]
                .as_str()
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned),
        execctl_resume_obligation: if payload["chat_start_restore"]["execctl_resume_obligation"]
            .is_object()
        {
            payload["chat_start_restore"]["execctl_resume_obligation"].clone()
        } else {
            json!({
                "resume_state": payload["chat_start_restore"]["execctl_resume_state"]
                    .as_str()
                    .unwrap_or("clear"),
                "no_silent_drop": true,
                "pending_return_count": 0,
                "active_task_headline": Value::Null,
                "required_return_headline": Value::Null,
                "required_return_next_step": Value::Null,
            })
        },
        startup_execution_gate: if payload["startup_execution_gate"].is_object() {
            payload["startup_execution_gate"].clone()
        } else {
            fallback_startup_execution_gate(payload)
        },
        startup_next_action: if payload["chat_start_restore"]["startup_next_action"].is_object() {
            payload["chat_start_restore"]["startup_next_action"].clone()
        } else {
            json!({
                "action_version": "startup-next-action-v1",
                "action_kind": "continue_active_workline",
                "blocking": false,
                "reason": "active_workline_restored",
                "resume_state": payload["chat_start_restore"]["execctl_resume_state"]
                    .as_str()
                    .unwrap_or("clear"),
                "no_silent_drop": true,
                "headline": payload["chat_start_restore"]["headline"].as_str().unwrap_or(""),
                "next_step": payload["chat_start_restore"]["next_step"].as_str().unwrap_or(""),
            })
        },
        startup_next_action_summary: payload["chat_start_restore"]["startup_next_action_summary"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        execctl_active_lease: if payload["chat_start_restore"]["execctl_active_lease"].is_object() {
            payload["chat_start_restore"]["execctl_active_lease"].clone()
        } else {
            Value::Null
        },
        execctl_active_lease_summary: payload["chat_start_restore"]["execctl_active_lease_summary"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        required_return_task: if payload["chat_start_restore"]["required_return_task"].is_object() {
            payload["chat_start_restore"]["required_return_task"].clone()
        } else {
            Value::Null
        },
        project_task_tree: if payload["chat_start_restore"]["project_task_tree"].is_object() {
            payload["chat_start_restore"]["project_task_tree"].clone()
        } else {
            Value::Null
        },
        project_task_tree_summary: payload["chat_start_restore"]["project_task_tree_summary"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        project_task_ledger: if payload["chat_start_restore"]["project_task_ledger"].is_object() {
            payload["chat_start_restore"]["project_task_ledger"].clone()
        } else {
            Value::Null
        },
        project_task_ledger_summary: payload["chat_start_restore"]["project_task_ledger_summary"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        included_reasons_summary: payload["chat_start_restore"]["included_reasons_summary"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
        excluded_reasons_summary: payload["chat_start_restore"]["excluded_reasons_summary"]
            .as_str()
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    }
}

pub(crate) fn continuity_startup_summary_json(payload: &Value) -> Value {
    let summary = continuity_startup_summary(payload);
    json!({
        "project_code": summary.project_code,
        "namespace_code": summary.namespace_code,
        "headline": summary.headline,
        "next_step": summary.next_step,
        "restore_confidence": summary.restore_confidence,
        "thread_count": summary.thread_count,
        "prompt_text_present": summary.prompt_text_present,
        "execctl_resume_state": summary.execctl_resume_state,
        "pending_return_summary": summary.pending_return_summary,
        "execctl_resume_contract_summary": summary.execctl_resume_contract_summary,
        "execctl_resume_obligation": summary.execctl_resume_obligation,
        "startup_execution_gate": summary.startup_execution_gate,
        "startup_next_action": summary.startup_next_action,
        "startup_next_action_summary": summary.startup_next_action_summary,
        "execctl_active_lease": summary.execctl_active_lease,
        "execctl_active_lease_summary": summary.execctl_active_lease_summary,
        "required_return_task": summary.required_return_task,
        "project_task_tree": summary.project_task_tree,
        "project_task_tree_summary": summary.project_task_tree_summary,
        "project_task_ledger": summary.project_task_ledger,
        "project_task_ledger_summary": summary.project_task_ledger_summary,
        "included_reasons_summary": summary.included_reasons_summary,
        "excluded_reasons_summary": summary.excluded_reasons_summary,
    })
}

fn memory_matrix_summary(payload: &Value) -> MemoryMatrixSummary {
    let matrix = &payload["memory_task_matrix"];
    MemoryMatrixSummary {
        matrix: matrix["matrix"].as_str().unwrap_or("").to_string(),
        display_name: matrix["display_name"].as_str().unwrap_or("").to_string(),
        tasks_total: matrix["tasks_total"].as_u64().unwrap_or_default(),
        tasks_passed: matrix["tasks_passed"].as_u64().unwrap_or_default(),
        tasks_failed: matrix["tasks_failed"].as_u64().unwrap_or_default(),
        success_rate: matrix["success_rate"].as_f64().unwrap_or_default(),
        mean_score: matrix["mean_score"].as_f64().unwrap_or_default(),
        p95_ms: matrix["p95_ms"].as_f64().unwrap_or_default(),
        gate_failures_count: matrix["gate_failures"]
            .as_array()
            .map_or(0, |items| items.len() as u64),
        compact_verdict_counts: summarize_verdict_counts(
            &matrix["canonical_eval"]["verdict_counts"],
        ),
    }
}

fn token_report_summary(payload: &Value) -> TokenReportSummary {
    let headline = &payload["token_budget_report"]["headline"];
    let report = &payload["token_budget_report"];
    let agent_cycle_scope = if report["rolling_window"].is_null() {
        &report["agent_cycle_economics"]["lifetime"]
    } else {
        &report["agent_cycle_economics"]["rolling_window"]
    };
    let contractual_scope = if report["contractual_statement_summaries"]["rolling_window"].is_null()
    {
        &report["contractual_statement_summaries"]["lifetime"]
    } else {
        &report["contractual_statement_summaries"]["rolling_window"]
    };
    let blockers = contractual_scope["blocking_reasons"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .take(4)
                .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    TokenReportSummary {
        metric_code: headline["metric_code"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        scope_label: headline["scope_label"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        status: headline["status"].as_str().unwrap_or("unknown").to_string(),
        value_percent: headline["value_percent"].as_f64().unwrap_or_default(),
        saved_tokens: headline["saved_tokens"].as_i64().unwrap_or_default(),
        events_count: headline["events_count"].as_u64().unwrap_or_default(),
        counted_events: headline["counted_events"].as_u64().unwrap_or_default(),
        agent_cycle_scope_label: agent_cycle_scope["scope_label"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        agent_cycle_status: report["agent_cycle_economics"]["status"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        agent_cycle_verified_saved_percent: agent_cycle_scope["verified_measured_saved_pct"]
            .as_f64()
            .unwrap_or_default(),
        agent_cycle_verified_saved_tokens: agent_cycle_scope["verified_measured_saved_tokens"]
            .as_i64()
            .unwrap_or_default(),
        agent_cycle_note: report["agent_cycle_economics"]["contract"]["note"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        contractual_scope_label: contractual_scope["scope_label"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        contractual_state: contractual_scope["contractual_state"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        contractual_coverage_state: contractual_scope["coverage_state"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        contractual_metering_ingest_state: contractual_scope["metering_ingest_state"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        contractual_lag_state: contractual_scope["contractual_lag_state"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        contractual_freshness_state: contractual_scope["contractual_freshness_state"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        contractual_reconciliation_state: contractual_scope["reconciliation_state"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        contractual_margin_state: contractual_scope["margin_state"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        contractual_blockers_summary: blockers,
        note: headline["note"].as_str().unwrap_or("").to_string(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextPackSummary {
    included_reasons_summary: Option<String>,
    excluded_reasons_summary: Option<String>,
}

fn context_pack_summary(payload: &Value) -> ContextPackSummary {
    ContextPackSummary {
        included_reasons_summary: decision_trace_summary(&payload["decision_trace"], "included"),
        excluded_reasons_summary: decision_trace_summary(
            &payload["decision_trace"],
            "not_included",
        ),
    }
}

fn context_pack_contains_primary_project(context_pack: &Value, project_code: &str) -> bool {
    context_pack["context_pack"]["visible_projects"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .any(|item| item["project_code"].as_str() == Some(project_code))
        })
        .unwrap_or_else(|| {
            context_pack["context_pack"]["project"]["code"].as_str() == Some(project_code)
        })
}

fn verify_mcp_scope_requires_memory_matrix(scope: VerifyMcpScope) -> bool {
    matches!(scope, VerifyMcpScope::Full)
}

fn verify_mcp_scope_requires_warm_cache(scope: VerifyMcpScope) -> bool {
    matches!(scope, VerifyMcpScope::Full)
}

fn verify_mcp_scope_label(scope: VerifyMcpScope) -> &'static str {
    match scope {
        VerifyMcpScope::Full => "full",
        VerifyMcpScope::TokenLedger => "token-ledger",
    }
}

fn snapshot_has_only_ignored_critical_metrics(checks: &Value, ignored_metrics: &[&str]) -> bool {
    let checks = match checks.as_array() {
        Some(items) => items,
        None => return false,
    };
    let mut saw_critical = false;
    for check in checks {
        if check["status"].as_str() != Some("critical") {
            continue;
        }
        saw_critical = true;
        let metric = check["metric"].as_str().unwrap_or_default();
        if !ignored_metrics.iter().any(|ignored| *ignored == metric) {
            return false;
        }
    }
    saw_critical
}

fn decision_trace_summary(trace: &Value, key: &str) -> Option<String> {
    let items = trace[key].as_array()?;
    let parts = items
        .iter()
        .take(3)
        .filter_map(|item| {
            let reason = item["reason"].as_str()?.trim();
            if reason.is_empty() {
                return None;
            }
            let strategy = item["strategy"].as_str().unwrap_or("unknown");
            let count = item["count"].as_u64();
            Some(match count {
                Some(value) if value > 0 => format!("{strategy} ({value}) — {reason}"),
                _ => format!("{strategy} — {reason}"),
            })
        })
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" • "))
    }
}

#[derive(Debug, Clone, PartialEq)]
struct TokenBenchmarkSummary {
    saved_tokens: u64,
    savings_factor: f64,
    savings_percent: f64,
    naive_tokens: u64,
    context_tokens: u64,
    files_considered: u64,
}

fn token_benchmark_summary(payload: &Value) -> TokenBenchmarkSummary {
    let benchmark = &payload["token_benchmark"];
    TokenBenchmarkSummary {
        saved_tokens: benchmark["savings"]["saved_tokens"]
            .as_u64()
            .unwrap_or_default(),
        savings_factor: benchmark["savings"]["savings_factor"]
            .as_f64()
            .unwrap_or_default(),
        savings_percent: benchmark["savings"]["savings_percent"]
            .as_f64()
            .unwrap_or_default(),
        naive_tokens: benchmark["naive_scope"]["tokens"]
            .as_u64()
            .unwrap_or_default(),
        context_tokens: benchmark["context_pack_render"]["tokens"]
            .as_u64()
            .unwrap_or_default(),
        files_considered: benchmark["naive_scope"]["files_considered"]
            .as_u64()
            .unwrap_or_default(),
    }
}

async fn tool_warm_cache(cfg: &AppConfig, args: WarmCacheToolArgs) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let mut db = postgres::connect_admin(cfg).await?;
    let mut warmed = Vec::with_capacity(args.projects.len());
    for project in &args.projects {
        let context = ContextPackArgs {
            project: project.clone(),
            namespace: args.namespace.clone(),
            query: args.query.clone(),
            retrieval_mode: args.retrieval_mode.clone(),
            disable_cache: false,
            limit_documents: args.limit_documents,
            limit_symbols: args.limit_symbols,
            limit_chunks: args.limit_chunks,
            limit_semantic_chunks: args.limit_semantic_chunks,
            token_source_kind: "proof_warmup_context_pack".to_string(),
            client_prompt_tokens: None,
            assistant_generation_tokens: None,
            tool_overhead_tokens: None,
            continuity_restore_tokens: None,
        };
        let stats = retrieval::execute_context_pack(cfg, &mut db, &context, true).await?;
        warmed.push(json!({
            "project": project,
            "namespace": context.namespace,
            "query": context.query,
            "cache_hit": stats.cache_hit,
            "exact_documents": stats.exact_documents,
            "symbol_hits": stats.symbol_hits,
            "lexical_chunks": stats.lexical_chunks,
            "semantic_chunks": stats.semantic_chunks,
            "scope_signature": stats.scope_signature,
        }));
    }
    let warm_summary = warm_cache_summary(&warmed, &args.projects);
    let structured = json!({
        "warmup_cache": {
            "projects": args.projects,
            "namespace": args.namespace,
            "query": args.query,
            "retrieval_mode": args.retrieval_mode,
            "warmed": warmed,
        },
        "warm_cache_summary": {
            "project_count": warm_summary.project_count,
            "compact_projects": warm_summary.compact_projects,
            "cache_hits": warm_summary.cache_hits,
            "exact_documents": warm_summary.exact_documents,
            "symbol_hits": warm_summary.symbol_hits,
            "lexical_chunks": warm_summary.lexical_chunks,
            "semantic_chunks": warm_summary.semantic_chunks,
        },
    });
    Ok(tool_result(
        format!(
            "warmup finished for {} project(s) [{}] cache_hits={}/{} exact={} symbol={} lexical={} semantic={}",
            warm_summary.project_count,
            warm_summary.compact_projects,
            warm_summary.cache_hits,
            warm_summary.project_count,
            warm_summary.exact_documents,
            warm_summary.symbol_hits,
            warm_summary.lexical_chunks,
            warm_summary.semantic_chunks,
        ),
        structured,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WarmCacheSummary {
    project_count: usize,
    compact_projects: String,
    cache_hits: u64,
    exact_documents: u64,
    symbol_hits: u64,
    lexical_chunks: u64,
    semantic_chunks: u64,
}

fn warm_cache_summary(warmed: &[Value], projects: &[String]) -> WarmCacheSummary {
    WarmCacheSummary {
        project_count: warmed.len(),
        compact_projects: summarize_with_limit(projects),
        cache_hits: warmed
            .iter()
            .filter(|entry| entry["cache_hit"].as_bool().unwrap_or(false))
            .count() as u64,
        exact_documents: warmed
            .iter()
            .map(|entry| entry["exact_documents"].as_u64().unwrap_or_default())
            .sum(),
        symbol_hits: warmed
            .iter()
            .map(|entry| entry["symbol_hits"].as_u64().unwrap_or_default())
            .sum(),
        lexical_chunks: warmed
            .iter()
            .map(|entry| entry["lexical_chunks"].as_u64().unwrap_or_default())
            .sum(),
        semantic_chunks: warmed
            .iter()
            .map(|entry| entry["semantic_chunks"].as_u64().unwrap_or_default())
            .sum(),
    }
}

#[derive(Debug, Clone, PartialEq)]
struct StackPreflightSummary {
    profile_code: String,
    profile_display_name: String,
    verdict: String,
    host_logical_cpus: u64,
    host_total_memory_gib: f64,
    host_available_disk_gib: f64,
    supports_peak_benchmarks: bool,
    start_monitoring_by_default: bool,
    remote_mode_recommended: bool,
    unmet_minimums_count: u64,
    unmet_recommendations_count: u64,
}

fn stack_preflight_summary(payload: &Value) -> StackPreflightSummary {
    StackPreflightSummary {
        profile_code: payload["profile_code"].as_str().unwrap_or("").to_string(),
        profile_display_name: payload["profile"]["display_name"]
            .as_str()
            .unwrap_or("")
            .to_string(),
        verdict: payload["verdict"].as_str().unwrap_or("unknown").to_string(),
        host_logical_cpus: payload["host"]["logical_cpus"].as_u64().unwrap_or_default(),
        host_total_memory_gib: payload["host"]["total_memory_gib"]
            .as_f64()
            .unwrap_or_default(),
        host_available_disk_gib: payload["host"]["available_disk_gib"]
            .as_f64()
            .unwrap_or_default(),
        supports_peak_benchmarks: payload["profile"]["supports_peak_benchmarks"]
            .as_bool()
            .unwrap_or(false),
        start_monitoring_by_default: payload["profile"]["start_monitoring_by_default"]
            .as_bool()
            .unwrap_or(false),
        remote_mode_recommended: payload["profile"]["remote_mode_recommended"]
            .as_bool()
            .unwrap_or(false),
        unmet_minimums_count: payload["unmet_minimums"]
            .as_array()
            .map_or(0, |items| items.len() as u64),
        unmet_recommendations_count: payload["unmet_recommendations"]
            .as_array()
            .map_or(0, |items| items.len() as u64),
    }
}

fn server_instructions() -> String {
    [
        "Amai is a project-scoped continuity and retrieval server for AI agents.",
        "Default law: keep projects isolated and prefer local_strict unless a related-project policy is explicitly required.",
        "Use amai_list_projects first when you do not know what is registered.",
        "Use amai_list_namespaces before querying an unfamiliar project.",
        "Use amai_continuity_startup at the beginning of a new chat or when resuming a project, before substantive work.",
        "Use amai_stack_preflight when you need to know what this machine can honestly support.",
        "Use amai_context_pack for retrieval instead of asking for whole repositories.",
        "Use amai_observe_whole_cycle when the client only learns assistant output tokens after the context-pack tool call and needs to attach real whole-cycle evidence back to the same event.",
        "Use amai_observe_whole_cycle_turn when the client learns assistant-generation tokens for one logical turn across multiple context packs and needs a turn-scoped attach without per-event duplication.",
        "Use amai_token_benchmark when you need a measured token-economy comparison.",
        "Use amai_token_report when you need cumulative token savings for the current session, budget window, or lifetime.",
        "Use amai_benchmark_coverage when you need the external benchmark and eval coverage map for Amai.",
        "Use amai_memory_matrix when you need the measured product-eval verdict for memory usefulness and isolation.",
        "Use amai_observe_snapshot when you need live stack health and SLA evidence.",
    ]
    .join(" ")
}

pub fn project_chat_startup_contract() -> Value {
    protocol_manifest()["startup_contracts"]["project_chat_startup"].clone()
}

fn protocol_manifest() -> Value {
    json!({
        "version": "mcp-contract-v2",
        "default_scope_rule": "project_scoped_fail_closed",
        "default_retrieval_mode": "local_strict",
        "startup_contracts": {
            "project_chat_startup": {
                "contract_version": "continuity-startup-contract-v13",
                "tool": "amai_continuity_startup",
                "prompt": "amai-continuity-startup",
                "purpose": "project-scoped continuity restore plus live client-budget discipline before each substantive reply in a new, resumed, or ongoing chat",
                "must_call_before_substantive_work": true,
                "must_call_before_tools": [
                    "amai_context_pack",
                    "amai_token_benchmark",
                    "amai_token_report",
                    "amai_observe_snapshot",
                    "amai_memory_matrix",
                    "amai_warm_cache"
                ],
                "project_binding_rule": "registered_project_fail_closed",
                "default_namespace": "continuity",
                "artifact_enforcement": {
                    "workspace_contract_relative_path": ".amai/onboarding/project-chat-startup-contract.json",
                    "workspace_contract_required_before_tool_call": true,
                    "workspace_contract_source_of_truth": true,
                    "workspace_contract_sha256_field": "startup_contract_sha256",
                    "missing_or_unreadable_fail_closed": true,
                    "sha256_mismatch_fail_closed": true
                },
                "runtime_state_artifact": {
                    "workspace_runtime_state_relative_path": ".amai/continuity/project-chat-startup-state.json",
                    "workspace_runtime_state_artifact_version": "workspace-startup-runtime-state-v4",
                    "written_by_tool": "amai_continuity_startup",
                    "source_summary_field": "continuity_startup_summary",
                    "contains_prompt_text": true,
                    "startup_execution_gate_field": "startup_execution_gate",
                    "startup_execution_gate_version": "startup-execution-gate-v1",
                    "gate_semantics_consistent_field": "gate_semantics_consistent",
                    "gate_semantics_consistent_true_required": true,
                    "inspection_fallback_cli": {
                        "command": "continuity startup-state",
                        "requires_repo_root_argument": true,
                        "json_required": true,
                        "returns_startup_execution_gate": true
                    }
                },
                "startup_execution_gate_enforcement": {
                    "gate_field": "startup_execution_gate",
                    "action_kind_field": "action_kind",
                    "blocking_field": "blocking",
                    "resume_state_field": "resume_state",
                    "required_return_task_present_field": "required_return_task_present",
                    "required_return_task_headline_field": "required_return_task_headline",
                    "required_return_task_next_step_field": "required_return_task_next_step",
                    "lease_owner_state_field": "lease_owner_state",
                    "must_follow_field": "must_follow_startup_next_action",
                    "unrelated_work_allowed_field": "unrelated_work_allowed",
                    "must_read_prompt_text_before_reply_field": "must_read_prompt_text_before_reply",
                    "required_action_kind_field": "required_action_kind_when_resume_required",
                    "no_silent_drop_field": "no_silent_drop",
                    "blocking_true_requires_must_follow": true,
                    "blocking_true_blocks_unrelated_work": true,
                    "must_follow_true_blocks_unrelated_work": true,
                    "unrelated_work_allowed_false_blocks_unrelated_work": true,
                    "must_read_prompt_text_true_requires_prompt_before_reply": true,
                    "required_action_kind_resume_required_value": "resume_required_return_task",
                    "no_silent_drop_must_be_true": true
                },
                "live_client_budget_enforcement": {
                    "guard_command": "observe client-budget-gate",
                    "guard_summary_field": "client_budget_reply_gate",
                    "reply_execution_gate_field": "reply_execution_gate",
                    "reply_execution_gate_version": "client-reply-budget-gate-v1",
                    "reply_budget_mode_field": "reply_budget_mode",
                    "reply_budget_contract_field": "reply_budget_contract",
                    "compact_reply_mode_value": working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL,
                    "compact_reply_contract_version": working_state::CLIENT_REPLY_BUDGET_CONTRACT_VERSION,
                    "compact_diagnostics_command": "observe client-budget-root-cause",
                    "must_prefer_compact_diagnostics_over_full_snapshot": true,
                    "guard_enforcement_flag": "--enforce-reply-gate",
                    "guard_enforcement_exit_on_blocking": true,
                    "must_check_before_each_substantive_reply": true,
                    "max_guard_age_seconds": 10,
                    "stale_guard_requires_refresh": true,
                    "rotate_now_field": "should_rotate_chat_now",
                    "rotate_soon_field": "should_rotate_chat_soon",
                    "status_label_field": "status_label",
                    "rotate_status_labels": [
                        "новый чат нужен сейчас"
                    ],
                    "save_handoff_before_rotate": true,
                    "fresh_chat_requires_continuity_startup": true,
                    "full_scale_client_truth_required": true,
                    "blocking_action_kinds": [
                        "rotate_chat_for_client_budget",
                        "wait_for_global_client_budget_recovery"
                    ],
                    "blocking_reply_contract_field": "blocking_reply_contract",
                    "blocking_reply_contract_version": working_state::CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION,
                    "blocking_reply_response_kind": working_state::CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND,
                    "blocking_reply_allowed_response_kinds": [
                        working_state::CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_RESPONSE_KIND,
                        working_state::CLIENT_BUDGET_WAIT_BLOCKING_REPLY_RESPONSE_KIND
                    ],
                    "blocking_reply_max_sentences": working_state::CLIENT_BUDGET_BLOCKING_REPLY_MAX_SENTENCES,
                    "blocking_reply_must_avoid_substantive_work": true,
                    "blocking_reply_must_use_action_bundle_operator_flow": true,
                    "blocking_reply_template": working_state::CLIENT_BUDGET_BLOCKING_REPLY_TEMPLATE,
                    "blocking_reply_allowed_templates": [
                        working_state::CLIENT_BUDGET_ROTATE_BLOCKING_REPLY_TEMPLATE,
                        working_state::CLIENT_BUDGET_WAIT_BLOCKING_REPLY_TEMPLATE
                    ]
                },
                "required_arguments": ["project"],
                "optional_arguments": ["repo_root", "namespace", "token_source_kind"],
                "summary_field": "continuity_startup_summary",
                "required_summary_fields": [
                    "project_code",
                    "namespace_code",
                    "headline",
                    "next_step",
                    "restore_confidence",
                    "thread_count",
                    "prompt_text_present",
                    "execctl_resume_state",
                    "execctl_resume_contract_summary",
                    "execctl_resume_obligation",
                    "startup_execution_gate",
                    "startup_next_action",
                    "startup_next_action_summary",
                    "execctl_active_lease",
                    "execctl_active_lease_summary",
                    "required_return_task",
                    "project_task_tree",
                    "project_task_tree_summary",
                    "project_task_ledger",
                    "project_task_ledger_summary"
                ],
                "restored_obligations": [
                    "active_workline",
                    "chat_start_restore_prompt_text",
                    "execctl_resume_state",
                    "pending_return_summary",
                    "execctl_resume_contract_summary",
                    "execctl_resume_obligation",
                    "startup_execution_gate",
                    "startup_next_action",
                    "execctl_active_lease_summary",
                    "required_return_task",
                    "project_task_tree",
                    "project_task_tree_summary",
                    "project_task_ledger",
                    "project_task_ledger_summary"
                ],
                "resume_enforcement": {
                    "contract_field": "execctl_resume_contract_summary",
                    "resume_state_field": "execctl_resume_state",
                    "obligation_field": "execctl_resume_obligation",
                    "startup_next_action_field": "startup_next_action",
                    "active_lease_field": "execctl_active_lease",
                    "active_lease_owner_state_field": "lease_owner_state",
                    "previous_session_owner_value": "previous_session_owner",
                    "must_resume_required_return_task_before_unrelated_work": true,
                    "previous_session_owner_must_follow_startup_next_action": true,
                    "required_action_kind_when_resume_required": "resume_required_return_task",
                    "default_action_kind_when_clear": "continue_active_workline",
                    "no_silent_drop": true
                },
                "fail_closed_conditions": [
                    "project_unregistered",
                    "repo_root_binding_ambiguous",
                    "continuity_restore_unavailable"
                ]
            }
        },
        "tool_contracts": {
            "amai_list_projects": {
                "summary_field": "projects_summary",
                "short_summary_contract": "registered project count plus compact code preview",
            },
            "amai_list_namespaces": {
                "summary_field": "namespaces_summary",
                "short_summary_contract": "namespace count plus compact namespace=mode preview",
            },
            "amai_stack_preflight": {
                "summary_field": "preflight_summary",
                "short_summary_contract": "host suitability verdict plus machine guarantees for a named deployment profile",
            },
            "amai_benchmark_coverage": {
                "summary_field": "benchmark_coverage_summary",
                "short_summary_contract": "external benchmark coverage totals plus the next benchmark priorities for Amai",
            },
            "amai_continuity_startup": {
                "summary_field": "continuity_startup_summary",
                "short_summary_contract": "project-scoped startup restore summary with headline, next step, prompt availability and execctl return obligations",
            },
            "amai_context_pack": {
                "summary_field": "context_pack_summary",
                "short_summary_contract": "layer totals plus included/excluded retrieval reasons",
            },
            "amai_observe_whole_cycle": {
                "summary_field": "whole_cycle_observed_attach",
                "short_summary_contract": "post-call attachment result for whole-cycle observed tokens on an existing context-pack event",
            },
            "amai_observe_whole_cycle_turn": {
                "summary_field": "assistant_generation_turn_observed_attach",
                "short_summary_contract": "turn-scoped attachment result for assistant-generation tokens that belong to one logical reply across multiple context-pack events",
            },
            "amai_token_benchmark": {
                "summary_field": "token_benchmark_summary",
                "short_summary_contract": "naive-vs-context token comparison with savings totals",
            },
            "amai_token_report": {
                "summary_field": "token_report_summary",
                "short_summary_contract": "scope, status, counted-events semantics and saved tokens",
            },
            "amai_memory_matrix": {
                "summary_field": "memory_matrix_summary",
                "short_summary_contract": "measured memory usefulness/isolation pass rate, score, latency and verdict totals",
            },
            "amai_observe_snapshot": {
                "summary_field": "observe_snapshot_summary",
                "short_summary_contract": "live SLA totals plus latest included/excluded retrieval reasons",
            },
            "amai_warm_cache": {
                "summary_field": "warm_cache_summary",
                "short_summary_contract": "project preview plus cache-hit and retrieval-layer totals",
            },
        },
        "prompt_contracts": {
            "amai-onboarding": {
                "purpose": "safe onboarding without mixing projects",
            },
            "amai-continuity-startup": {
                "purpose": "project-scoped startup guidance for continuity restore and live client-budget discipline before each substantive reply",
            },
            "amai-context-pack": {
                "purpose": "project-scoped retrieval guidance for context-pack requests",
            },
        },
        "error_contracts": {
            "invalid_json_rpc_payload": {
                "carrier": "jsonrpc_error",
                "jsonrpc_code": -32700,
                "error_class": "protocol_parse",
            },
            "invalid_request": {
                "carrier": "jsonrpc_error",
                "jsonrpc_code": -32600,
                "error_class": "protocol_request",
            },
            "method_not_found": {
                "carrier": "jsonrpc_error",
                "jsonrpc_code": -32601,
                "error_class": "protocol_dispatch",
            },
            "prompt_not_found": {
                "carrier": "jsonrpc_error",
                "jsonrpc_code": -32601,
                "error_class": "prompt_dispatch",
            },
            "invalid_params": {
                "carrier": "jsonrpc_error_or_tool_is_error",
                "jsonrpc_code": -32602,
                "error_class": "client_input",
            },
            "tool_not_found": {
                "carrier": "tool_is_error",
                "jsonrpc_code": -32601,
                "error_class": "tool_dispatch",
            },
            "tool_execution_failed": {
                "carrier": "tool_is_error",
                "jsonrpc_code": -32000,
                "error_class": "tool_runtime",
            },
        },
        "safety_laws": [
            "project isolation comes before retrieval breadth",
            "lexical and exact evidence outrank semantic guesses",
            "semantic retrieval must stay inside project scope unless policy explicitly allows more",
            "empty fail-closed retrieval is better than noisy cross-project context",
        ],
    })
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "amai_list_projects",
            "description": "List registered projects with their repo roots.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "annotations": {
                "title": "List Projects",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_list_namespaces",
            "description": "List namespaces already registered for a project.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "project": {
                        "type": "string",
                        "description": "Registered project code."
                    }
                },
                "required": ["project"],
                "additionalProperties": false
            },
            "annotations": {
                "title": "List Namespaces",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_stack_preflight",
            "description": "Measure what this machine can honestly support for a named deployment profile.",
            "inputSchema": stack_preflight_input_schema(),
            "annotations": {
                "title": "Stack Preflight",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_benchmark_coverage",
            "description": "Read the machine-readable benchmark and eval coverage map for Amai.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "annotations": {
                "title": "Benchmark Coverage",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_continuity_startup",
            "description": "Build a project-scoped continuity startup/restore pack for a new chat or resumed workline.",
            "inputSchema": continuity_startup_input_schema(),
            "annotations": {
                "title": "Continuity Startup",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_context_pack",
            "description": "Build a provenance-rich context pack for a project-scoped query.",
            "inputSchema": context_pack_input_schema(true),
            "annotations": {
                "title": "Build Context Pack",
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_observe_whole_cycle",
            "description": "Attach post-call whole-cycle observed tokens such as assistant generation back to an existing context-pack event.",
            "inputSchema": observe_whole_cycle_input_schema(),
            "annotations": {
                "title": "Attach Whole-Cycle Observed Tokens",
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_observe_whole_cycle_turn",
            "description": "Attach assistant-generation tokens once for a whole turn-group that spans one or more context-pack events.",
            "inputSchema": observe_whole_cycle_turn_input_schema(),
            "annotations": {
                "title": "Attach Turn-Scoped Assistant Generation",
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_token_benchmark",
            "description": "Measure naive-scope vs compact context-pack token usage on the same query.",
            "inputSchema": token_benchmark_input_schema(),
            "annotations": {
                "title": "Measure Token Savings",
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_token_report",
            "description": "Report cumulative token savings for the current session, budget window, and lifetime.",
            "inputSchema": token_report_input_schema(),
            "annotations": {
                "title": "Token Report",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_memory_matrix",
            "description": "Run the canonical measured memory matrix for Amai and return product-eval verdicts.",
            "inputSchema": memory_matrix_input_schema(),
            "annotations": {
                "title": "Memory Matrix",
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_observe_snapshot",
            "description": "Read a live observability snapshot with current SLA summary.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            },
            "annotations": {
                "title": "Observe Snapshot",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }),
        json!({
            "name": "amai_warm_cache",
            "description": "Warm cache entries for one or more registered projects.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "projects": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Registered project codes to warm."
                    },
                    "namespace": {
                        "type": "string",
                        "default": "default"
                    },
                    "query": {
                        "type": "string",
                        "default": "README"
                    },
                    "retrieval_mode": {
                        "type": ["string", "null"],
                        "enum": ["local_strict", "local_plus_related", "explicit_foreign", "audit_global", null]
                    },
                    "limit_documents": {
                        "type": "integer",
                        "minimum": 1,
                        "default": 4
                    },
                    "limit_symbols": {
                        "type": "integer",
                        "minimum": 1,
                        "default": 4
                    },
                    "limit_chunks": {
                        "type": "integer",
                        "minimum": 1,
                        "default": 4
                    },
                    "limit_semantic_chunks": {
                        "type": "integer",
                        "minimum": 1,
                        "default": 4
                    }
                },
                "required": ["projects"],
                "additionalProperties": false
            },
            "annotations": {
                "title": "Warm Cache",
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": false
            }
        }),
    ]
}

fn prompt_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "amai-onboarding",
            "description": "Explain how to use Amai safely without mixing projects.",
            "arguments": []
        }),
        json!({
            "name": "amai-continuity-startup",
            "description": "Guide the model to request a project-scoped continuity startup pack before substantive work.",
            "arguments": [
                {
                    "name": "project",
                    "description": "Registered project code to resume.",
                    "required": true
                },
                {
                    "name": "namespace",
                    "description": "Optional continuity namespace code. Defaults to continuity.",
                    "required": false
                }
            ]
        }),
        json!({
            "name": "amai-context-pack",
            "description": "Guide the model to request a project-scoped context pack from Amai.",
            "arguments": [
                {
                    "name": "project",
                    "description": "Registered project code to inspect.",
                    "required": true
                },
                {
                    "name": "query",
                    "description": "What the model wants to find inside the project.",
                    "required": true
                },
                {
                    "name": "namespace",
                    "description": "Optional namespace code. Defaults to default.",
                    "required": false
                },
                {
                    "name": "retrieval_mode",
                    "description": "Optional retrieval mode override.",
                    "required": false
                }
            ]
        }),
    ]
}

fn prompt_result(params: Value) -> McpToolResult<Value> {
    let name = params["name"]
        .as_str()
        .ok_or_else(|| McpError::invalid_params("prompts/get requires a prompt name"))?;
    let arguments = params["arguments"].as_object().cloned().unwrap_or_default();
    let result = match name {
        "amai-onboarding" => json!({
            "description": "How to use Amai without mixing project context.",
            "messages": [{
                "role": "assistant",
                "content": {
                    "type": "text",
                    "text": server_instructions()
                }
            }]
        }),
        "amai-continuity-startup" => {
            let project = required_prompt_arg(&arguments, "project")?;
            let namespace = arguments
                .get("namespace")
                .and_then(Value::as_str)
                .unwrap_or("continuity");
            json!({
                "description": "Prompt for calling Amai continuity-startup correctly.",
                "messages": [{
                    "role": "assistant",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "Before substantive work in a new or resumed chat, call amai_continuity_startup for project {project} in namespace {namespace}. Use it to recover the current active line, the next required step, the chat-start restore prompt_text, any pending_return_queue obligations, execctl_resume_contract_summary, execctl_resume_obligation, startup_execution_gate, startup_next_action, execctl_active_lease, and execctl_active_lease_summary. Treat startup_execution_gate as the immediate return-enforcement object. Require gate_semantics_consistent = true before trusting that gate or executing startup_next_action. If startup_next_action.action_kind is resume_required_return_task, execute that required return before unrelated work and do not silently switch away. If execctl_active_lease.lease_owner_state is previous_session_owner, do not silently seize the workline; follow startup_next_action first."
                        )
                    }
                }]
            })
        }
        "amai-context-pack" => {
            let project = required_prompt_arg(&arguments, "project")?;
            let query = required_prompt_arg(&arguments, "query")?;
            let namespace = arguments
                .get("namespace")
                .and_then(Value::as_str)
                .unwrap_or("default");
            let retrieval_mode = arguments
                .get("retrieval_mode")
                .and_then(Value::as_str)
                .unwrap_or("local_strict");
            json!({
                "description": "Prompt for calling Amai context-pack retrieval correctly.",
                "messages": [{
                    "role": "assistant",
                    "content": {
                        "type": "text",
                        "text": format!(
                            "Use Amai to collect a project-scoped context pack. Project: {project}. Namespace: {namespace}. Query: {query}. Retrieval mode: {retrieval_mode}. Prefer local_strict unless the task explicitly requires a related-project lookup."
                        )
                    }
                }]
            })
        }
        other => return Err(McpError::prompt_not_found(other)),
    };
    Ok(result)
}

fn context_pack_input_schema(include_persist: bool) -> Value {
    let mut properties = serde_json::Map::from_iter([
        (
            "project".to_string(),
            json!({
                "type": "string",
                "description": "Registered project code."
            }),
        ),
        (
            "namespace".to_string(),
            json!({
                "type": "string",
                "default": "default"
            }),
        ),
        (
            "query".to_string(),
            json!({
                "type": "string",
                "description": "Question or lookup string for retrieval."
            }),
        ),
        (
            "retrieval_mode".to_string(),
            json!({
                "type": ["string", "null"],
                "enum": ["local_strict", "local_plus_related", "explicit_foreign", "audit_global", null]
            }),
        ),
        (
            "disable_cache".to_string(),
            json!({
                "type": "boolean",
                "default": false
            }),
        ),
        (
            "limit_documents".to_string(),
            json!({
                "type": "integer",
                "minimum": 1,
                "default": 5
            }),
        ),
        (
            "limit_symbols".to_string(),
            json!({
                "type": "integer",
                "minimum": 1,
                "default": 8
            }),
        ),
        (
            "limit_chunks".to_string(),
            json!({
                "type": "integer",
                "minimum": 1,
                "default": 8
            }),
        ),
        (
            "limit_semantic_chunks".to_string(),
            json!({
                "type": "integer",
                "minimum": 1,
                "default": 8
            }),
        ),
        (
            "token_source_kind".to_string(),
            json!({
                "type": "string",
                "default": "live_context_pack",
                "description": "Token ledger source kind for this context-pack call. Use proof_/verify_ prefixes for engineering calls."
            }),
        ),
        (
            "client_prompt_tokens".to_string(),
            json!({
                "type": ["integer", "null"],
                "minimum": 0,
                "description": "Optional upstream-observed client prompt tokens in the same meter the client/provider reports."
            }),
        ),
        (
            "assistant_generation_tokens".to_string(),
            json!({
                "type": ["integer", "null"],
                "minimum": 0,
                "description": "Optional upstream-observed assistant generation tokens for the same context-pack event."
            }),
        ),
        (
            "tool_overhead_tokens".to_string(),
            json!({
                "type": ["integer", "null"],
                "minimum": 0,
                "description": "Optional upstream-observed non-retrieval tool overhead tokens for the same context-pack event."
            }),
        ),
        (
            "continuity_restore_tokens".to_string(),
            json!({
                "type": ["integer", "null"],
                "minimum": 0,
                "description": "Optional upstream-observed continuity-restore tokens outside retrieval for the same context-pack event."
            }),
        ),
    ]);
    if include_persist {
        properties.insert(
            "persist".to_string(),
            json!({
                "type": "boolean",
                "default": true
            }),
        );
    }
    Value::Object(serde_json::Map::from_iter([
        ("type".to_string(), Value::String("object".to_string())),
        ("properties".to_string(), Value::Object(properties)),
        ("required".to_string(), json!(["project", "query"])),
        ("additionalProperties".to_string(), Value::Bool(false)),
    ]))
}

fn continuity_startup_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "project": {
                "type": ["string", "null"],
                "description": "Registered project code to resume."
            },
            "repo_root": {
                "type": ["string", "null"],
                "description": "Optional repo root path used to resolve the registered project binding."
            },
            "namespace": {
                "type": "string",
                "default": "continuity",
                "description": "Continuity namespace code."
            },
            "token_source_kind": {
                "type": "string",
                "default": "live_continuity_startup",
                "description": "Token ledger source kind for continuity-startup observed whole-cycle events. Use proof_/verify_ prefixes for engineering calls."
            }
        },
        "additionalProperties": false
    })
}

fn token_benchmark_input_schema() -> Value {
    let mut schema = context_pack_input_schema(false);
    let properties = schema
        .get_mut("properties")
        .and_then(Value::as_object_mut)
        .expect("context pack schema always has properties");
    properties.insert(
        "tokenizer".to_string(),
        json!({
            "type": "string",
            "default": "o200k_base",
            "enum": ["o200k_base", "cl100k_base"]
        }),
    );
    properties.insert(
        "naive_limit_files".to_string(),
        json!({
            "type": "integer",
            "minimum": 1,
            "default": 20
        }),
    );
    properties.insert(
        "naive_max_bytes_per_file".to_string(),
        json!({
            "type": "integer",
            "minimum": 1,
            "default": 32768
        }),
    );
    schema
}

fn token_report_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "budget_profile": {
                "type": "string",
                "description": "Optional token budget profile code such as codex_5h."
            },
            "include_verify_events": {
                "type": "boolean",
                "description": "Whether verification and benchmark events should be included with live token usage.",
                "default": false
            }
        },
        "additionalProperties": false
    })
}

fn observe_whole_cycle_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "context_pack_id": {
                "type": "string",
                "description": "Context-pack id returned by amai_context_pack for the event that should receive whole-cycle observed tokens."
            },
            "client_prompt_tokens": {
                "type": ["integer", "null"],
                "minimum": 0,
                "description": "Optional observed client prompt tokens in the same meter the client/provider reports."
            },
            "assistant_generation_tokens": {
                "type": ["integer", "null"],
                "minimum": 0,
                "description": "Optional observed assistant generation tokens learned after the client finished its answer."
            },
            "tool_overhead_tokens": {
                "type": ["integer", "null"],
                "minimum": 0,
                "description": "Optional observed non-retrieval tool overhead tokens for the same context-pack event."
            },
            "continuity_restore_tokens": {
                "type": ["integer", "null"],
                "minimum": 0,
                "description": "Optional observed continuity-restore tokens outside retrieval for the same context-pack event."
            }
        },
        "required": ["context_pack_id"],
        "additionalProperties": false
    })
}

fn observe_whole_cycle_turn_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "thread_id": {
                "type": ["string", "null"],
                "description": "Optional thread id for the turn-group. If omitted, Amai infers it from working_state metadata for the provided context packs and fails closed on ambiguity."
            },
            "turn_id": {
                "type": "string",
                "description": "Logical turn id whose assistant-generation tokens should be counted once for the whole turn-group."
            },
            "context_pack_ids": {
                "type": "array",
                "description": "One or more context-pack ids that belong to the same logical turn-group.",
                "items": {
                    "type": "string"
                },
                "minItems": 1
            },
            "assistant_generation_tokens": {
                "type": "integer",
                "minimum": 1,
                "description": "Observed assistant-generation tokens for the whole turn-group. These tokens are counted once, not once per context pack."
            }
        },
        "required": ["turn_id", "context_pack_ids", "assistant_generation_tokens"],
        "additionalProperties": false
    })
}

fn stack_preflight_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "profile": {
                "type": "string",
                "default": "default",
                "description": "Deployment profile code from config/deployment_profiles.toml."
            }
        },
        "additionalProperties": false
    })
}

fn memory_matrix_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "matrix": {
                "type": "string",
                "default": "letta_memory_local",
                "description": "Measured memory matrix code from config/memory_task_matrix.toml."
            },
            "project_prefix": {
                "type": "string",
                "default": "memory_eval",
                "description": "Project-code prefix used for synthetic evaluation projects."
            },
            "min_success_rate": {
                "type": ["number", "null"],
                "minimum": 0.0,
                "maximum": 1.0
            },
            "min_mean_score": {
                "type": ["number", "null"],
                "minimum": 0.0,
                "maximum": 1.0
            },
            "max_p95_ms": {
                "type": ["number", "null"],
                "minimum": 0.0
            }
        },
        "additionalProperties": false
    })
}

fn tool_result(text: String, structured_content: Value) -> Value {
    json!({
        "content": [{
            "type": "text",
            "text": text
        }],
        "structuredContent": structured_content
    })
}

async fn write_message<W: AsyncWriteExt + Unpin>(writer: &mut W, message: &Value) -> Result<()> {
    let data = serde_json::to_vec(message).context("failed to serialize JSON-RPC message")?;
    writer
        .write_all(&data)
        .await
        .context("failed to write JSON-RPC message")?;
    writer
        .write_all(b"\n")
        .await
        .context("failed to write JSON-RPC newline")?;
    writer
        .flush()
        .await
        .context("failed to flush JSON-RPC writer")
}

fn parse_arguments<T>(value: Option<Value>) -> McpToolResult<T>
where
    T: DeserializeOwned + Default,
{
    match value {
        Some(value) => serde_json::from_value(value).map_err(|error| {
            McpError::invalid_params(format!("failed to decode tool arguments: {error}"))
        }),
        None => Ok(T::default()),
    }
}

fn render_client_config(args: &McpConfigArgs) -> Result<String> {
    let client = args.client.trim().to_ascii_lowercase();
    let repo_root = args.cwd.clone().unwrap_or(discover_repo_root()?);
    let cwd = repo_root.display().to_string();
    let launcher = resolve_launcher(
        &repo_root,
        &args.launcher_platform,
        args.command.as_deref(),
        args.ssh_destination.as_deref(),
        args.remote_repo_root.as_deref(),
    )?;
    let server_name = args.server_name.trim();
    if server_name.is_empty() {
        return Err(anyhow!("MCP server name must not be empty"));
    }

    match config_shape_for_client(&client)? {
        ConfigShape::GenericJson => serde_json::to_string_pretty(&json!({
            "name": server_name,
            "transport": "stdio",
            "command": launcher.command,
            "args": launcher.args,
            "cwd": cwd
        }))
        .context("failed to render generic MCP config"),
        ConfigShape::VscodeJson => serde_json::to_string_pretty(&json!({
            "servers": {
                server_name: {
                    "type": "stdio",
                    "command": launcher.command,
                    "args": launcher.args,
                    "cwd": cwd
                }
            }
        }))
        .context("failed to render VS Code MCP config"),
        ConfigShape::McpServersJson => serde_json::to_string_pretty(&json!({
            "mcpServers": {
                server_name: {
                    "command": launcher.command,
                    "args": launcher.args,
                    "cwd": cwd
                }
            }
        }))
        .context("failed to render MCP config"),
        ConfigShape::CodexToml => Ok(format!(
            "[mcp_servers.{server_name}]\ncommand = {:?}\nargs = {}\n",
            launcher.command,
            format_toml_string_array(&launcher.args)
        )),
    }
}

#[derive(Clone, Copy)]
enum ConfigShape {
    GenericJson,
    VscodeJson,
    McpServersJson,
    CodexToml,
}

fn config_shape_for_client(client: &str) -> Result<ConfigShape> {
    match client.trim().to_ascii_lowercase().as_str() {
        "generic" => Ok(ConfigShape::GenericJson),
        "vscode" => Ok(ConfigShape::VscodeJson),
        "cursor" | "claude-desktop" | "claude-code" => Ok(ConfigShape::McpServersJson),
        "codex" => Ok(ConfigShape::CodexToml),
        other => Err(anyhow!(
            "unsupported MCP client config target: {other}; use generic|vscode|cursor|claude-desktop|claude-code|codex"
        )),
    }
}

#[derive(Clone)]
struct LauncherCommand {
    command: String,
    args: Vec<String>,
}

fn resolve_launcher(
    repo_root: &Path,
    launcher_platform: &str,
    explicit_command: Option<&str>,
    ssh_destination: Option<&str>,
    remote_repo_root: Option<&Path>,
) -> Result<LauncherCommand> {
    if let Some(command) = explicit_command {
        return Ok(LauncherCommand {
            command: command.to_string(),
            args: Vec::new(),
        });
    }

    if let Some(destination) = ssh_destination {
        let remote_repo_root = remote_repo_root.ok_or_else(|| {
            anyhow!("--remote-repo-root is required when --ssh-destination is used")
        })?;
        let remote_command = format!(
            "cd {} && ./scripts/run_mcp_stdio.sh",
            shell_escape_single_quotes(&remote_repo_root.display().to_string())
        );
        return Ok(LauncherCommand {
            command: "ssh".to_string(),
            args: vec![destination.to_string(), remote_command],
        });
    }

    let normalized = normalize_launcher_platform(launcher_platform)?;
    match normalized.as_str() {
        "linux" | "macos" => Ok(LauncherCommand {
            command: repo_root
                .join("scripts/run_mcp_stdio.sh")
                .display()
                .to_string(),
            args: Vec::new(),
        }),
        "windows-cmd" => Ok(LauncherCommand {
            command: windows_path(&repo_root.join("scripts/run_mcp_stdio.cmd")),
            args: Vec::new(),
        }),
        "windows-powershell" => Ok(LauncherCommand {
            command: "powershell.exe".to_string(),
            args: vec![
                "-ExecutionPolicy".to_string(),
                "Bypass".to_string(),
                "-File".to_string(),
                windows_path(&repo_root.join("scripts/run_mcp_stdio.ps1")),
            ],
        }),
        other => Err(anyhow!("unsupported launcher platform: {other}")),
    }
}

fn normalize_launcher_platform(input: &str) -> Result<String> {
    let platform = input.trim().to_ascii_lowercase();
    if platform == "auto" {
        if cfg!(target_os = "windows") {
            return Ok("windows-powershell".to_string());
        }
        if cfg!(target_os = "macos") {
            return Ok("macos".to_string());
        }
        return Ok("linux".to_string());
    }

    match platform.as_str() {
        "linux" | "macos" | "windows-cmd" | "windows-powershell" => Ok(platform),
        other => Err(anyhow!(
            "unsupported launcher platform: {other}; use auto|linux|macos|windows-cmd|windows-powershell"
        )),
    }
}

fn windows_path(path: &std::path::Path) -> String {
    path.display().to_string().replace('/', "\\")
}

fn format_toml_string_array(items: &[String]) -> String {
    let rendered = items
        .iter()
        .map(|item| format!("{item:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{rendered}]")
}

fn shell_escape_single_quotes(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('\'');
    for ch in value.chars() {
        if ch == '\'' {
            escaped.push_str("'\"'\"'");
        } else {
            escaped.push(ch);
        }
    }
    escaped.push('\'');
    escaped
}

fn merge_existing_config(
    shape: ConfigShape,
    args: &McpConfigArgs,
    rendered: &str,
    output: &PathBuf,
) -> Result<String> {
    if !output.is_file() {
        return Ok(rendered.to_string());
    }

    let existing = fs::read_to_string(output)
        .with_context(|| format!("failed to read {}", output.display()))?;
    match shape {
        ConfigShape::GenericJson => Ok(rendered.to_string()),
        ConfigShape::VscodeJson => {
            merge_json_server(&existing, rendered, "servers", args.server_name.trim())
        }
        ConfigShape::McpServersJson => {
            merge_json_server(&existing, rendered, "mcpServers", args.server_name.trim())
        }
        ConfigShape::CodexToml => merge_toml_server(&existing, rendered, args.server_name.trim()),
    }
}

fn merge_json_server(
    existing: &str,
    rendered: &str,
    top_level_key: &str,
    server_name: &str,
) -> Result<String> {
    let mut existing_json: Value =
        serde_json::from_str(existing).context("failed to parse existing MCP JSON config")?;
    let rendered_json: Value =
        serde_json::from_str(rendered).context("failed to parse rendered MCP JSON config")?;

    let new_server = rendered_json
        .get(top_level_key)
        .and_then(Value::as_object)
        .and_then(|servers| servers.get(server_name))
        .cloned()
        .ok_or_else(|| anyhow!("rendered MCP config is missing server {server_name}"))?;

    let root = existing_json
        .as_object_mut()
        .ok_or_else(|| anyhow!("existing MCP JSON config is not an object"))?;
    let server_map = root
        .entry(top_level_key.to_string())
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| anyhow!("existing MCP JSON server map {top_level_key} is not an object"))?;
    server_map.insert(server_name.to_string(), new_server);
    serde_json::to_string_pretty(&existing_json)
        .context("failed to serialize merged MCP JSON config")
}

fn json_server_exists(existing: &str, top_level_key: &str, server_name: &str) -> Result<bool> {
    let existing_json: Value =
        serde_json::from_str(existing).context("failed to parse existing MCP JSON config")?;
    Ok(existing_json
        .get(top_level_key)
        .and_then(Value::as_object)
        .map(|servers| servers.contains_key(server_name))
        .unwrap_or(false))
}

fn remove_json_server(
    existing: &str,
    top_level_key: &str,
    server_name: &str,
) -> Result<(String, bool, bool)> {
    let mut existing_json: Value =
        serde_json::from_str(existing).context("failed to parse existing MCP JSON config")?;
    let root = existing_json
        .as_object_mut()
        .ok_or_else(|| anyhow!("existing MCP JSON config is not an object"))?;
    let Some(server_map_value) = root.get_mut(top_level_key) else {
        return Ok((existing.to_string(), false, false));
    };
    let server_map = server_map_value
        .as_object_mut()
        .ok_or_else(|| anyhow!("existing MCP JSON server map {top_level_key} is not an object"))?;
    let removed = server_map.remove(server_name).is_some();
    let server_map_empty = server_map.is_empty();
    if server_map_empty {
        root.remove(top_level_key);
    }
    let is_empty = root.is_empty() || root.values().all(Value::is_null);
    Ok((
        serde_json::to_string_pretty(&existing_json)
            .context("failed to serialize pruned MCP JSON config")?,
        removed,
        is_empty || existing_json == json!({}),
    ))
}

fn merge_toml_server(existing: &str, rendered: &str, server_name: &str) -> Result<String> {
    let mut existing_value: toml::Value =
        toml::from_str(existing).context("failed to parse existing Codex TOML config")?;
    let rendered_value: toml::Value =
        toml::from_str(rendered).context("failed to parse rendered Codex TOML config")?;

    let new_server = rendered_value
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .and_then(|table| table.get(server_name))
        .cloned()
        .ok_or_else(|| anyhow!("rendered Codex config is missing server {server_name}"))?;

    let root = existing_value
        .as_table_mut()
        .ok_or_else(|| anyhow!("existing Codex config is not a TOML table"))?;
    let server_map = root
        .entry("mcp_servers".to_string())
        .or_insert_with(|| toml::Value::Table(Default::default()))
        .as_table_mut()
        .ok_or_else(|| anyhow!("existing mcp_servers entry is not a TOML table"))?;
    server_map.insert(server_name.to_string(), new_server);
    toml::to_string_pretty(&existing_value).context("failed to serialize merged Codex TOML config")
}

fn toml_server_exists(existing: &str, server_name: &str) -> Result<bool> {
    let existing_value: toml::Value =
        toml::from_str(existing).context("failed to parse existing Codex TOML config")?;
    Ok(existing_value
        .get("mcp_servers")
        .and_then(toml::Value::as_table)
        .map(|table| table.contains_key(server_name))
        .unwrap_or(false))
}

fn remove_toml_server(existing: &str, server_name: &str) -> Result<(String, bool, bool)> {
    let mut existing_value: toml::Value =
        toml::from_str(existing).context("failed to parse existing Codex TOML config")?;
    let root = existing_value
        .as_table_mut()
        .ok_or_else(|| anyhow!("existing Codex config is not a TOML table"))?;
    let Some(mcp_servers_value) = root.get_mut("mcp_servers") else {
        return Ok((existing.to_string(), false, false));
    };
    let mcp_servers = mcp_servers_value
        .as_table_mut()
        .ok_or_else(|| anyhow!("existing mcp_servers entry is not a TOML table"))?;
    let removed = mcp_servers.remove(server_name).is_some();
    if mcp_servers.is_empty() {
        root.remove("mcp_servers");
    }
    let is_empty = root.is_empty();
    Ok((
        toml::to_string_pretty(&existing_value)
            .context("failed to serialize pruned Codex TOML config")?,
        removed,
        is_empty,
    ))
}

fn required_prompt_arg(
    arguments: &serde_json::Map<String, Value>,
    key: &str,
) -> McpToolResult<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| McpError::invalid_params(format!("prompt argument is required: {key}")))
}

fn discover_repo_root() -> Result<PathBuf> {
    config::discover_repo_root(None)
}

#[derive(Debug, Deserialize)]
struct ToolCallRequest {
    name: String,
    #[serde(default)]
    arguments: Option<Value>,
}

#[derive(Debug, Default, Deserialize)]
struct ListNamespacesArgs {
    project: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ContextPackToolArgs {
    #[serde(default)]
    project: String,
    #[serde(default = "default_namespace")]
    namespace: String,
    #[serde(default)]
    query: String,
    retrieval_mode: Option<String>,
    #[serde(default)]
    disable_cache: bool,
    #[serde(default = "default_limit_documents")]
    limit_documents: usize,
    #[serde(default = "default_limit_symbols")]
    limit_symbols: usize,
    #[serde(default = "default_limit_chunks")]
    limit_chunks: usize,
    #[serde(default = "default_limit_semantic_chunks")]
    limit_semantic_chunks: usize,
    #[serde(default = "default_context_pack_token_source_kind")]
    token_source_kind: String,
    #[serde(default)]
    client_prompt_tokens: Option<u64>,
    #[serde(default)]
    assistant_generation_tokens: Option<u64>,
    #[serde(default)]
    tool_overhead_tokens: Option<u64>,
    #[serde(default)]
    continuity_restore_tokens: Option<u64>,
    #[serde(default = "default_true")]
    persist: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ContinuityStartupToolArgs {
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    repo_root: Option<String>,
    #[serde(default = "default_continuity_namespace")]
    namespace: String,
    #[serde(default = "default_continuity_startup_token_source_kind")]
    token_source_kind: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ObserveWholeCycleToolArgs {
    #[serde(default)]
    context_pack_id: String,
    #[serde(default)]
    client_prompt_tokens: Option<u64>,
    #[serde(default)]
    assistant_generation_tokens: Option<u64>,
    #[serde(default)]
    tool_overhead_tokens: Option<u64>,
    #[serde(default)]
    continuity_restore_tokens: Option<u64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ObserveWholeCycleTurnToolArgs {
    #[serde(default)]
    thread_id: Option<String>,
    #[serde(default)]
    turn_id: String,
    #[serde(default)]
    context_pack_ids: Vec<String>,
    #[serde(default)]
    assistant_generation_tokens: u64,
}

impl ContextPackToolArgs {
    fn to_context_args(&self) -> ContextPackArgs {
        ContextPackArgs {
            project: self.project.clone(),
            namespace: self.namespace.clone(),
            query: self.query.clone(),
            retrieval_mode: self.retrieval_mode.clone(),
            disable_cache: self.disable_cache,
            limit_documents: self.limit_documents,
            limit_symbols: self.limit_symbols,
            limit_chunks: self.limit_chunks,
            limit_semantic_chunks: self.limit_semantic_chunks,
            token_source_kind: self.token_source_kind.clone(),
            client_prompt_tokens: self.client_prompt_tokens,
            assistant_generation_tokens: self.assistant_generation_tokens,
            tool_overhead_tokens: self.tool_overhead_tokens,
            continuity_restore_tokens: self.continuity_restore_tokens,
        }
    }
}

impl ContinuityStartupToolArgs {
    fn to_cli_args(&self) -> ContinuityStartupArgs {
        ContinuityStartupArgs {
            project: self.project.clone(),
            repo_root: self.repo_root.as_ref().map(PathBuf::from),
            namespace: self.namespace.clone(),
            json: true,
            token_source_kind: self.token_source_kind.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TokenBenchmarkToolArgs {
    #[serde(flatten)]
    context: ContextPackToolArgs,
    #[serde(default = "default_tokenizer")]
    tokenizer: String,
    #[serde(default = "default_naive_limit_files")]
    naive_limit_files: usize,
    #[serde(default = "default_naive_max_bytes_per_file")]
    naive_max_bytes_per_file: usize,
}

impl TokenBenchmarkToolArgs {
    fn to_verify_args(&self) -> VerifyTokenBenchmarkArgs {
        VerifyTokenBenchmarkArgs {
            context: self.context.to_context_args(),
            tokenizer: self.tokenizer.clone(),
            naive_limit_files: self.naive_limit_files,
            naive_max_bytes_per_file: self.naive_max_bytes_per_file,
            min_savings_factor: 0.0,
            min_savings_percent: 0.0,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct TokenReportToolArgs {
    budget_profile: Option<String>,
    include_verify_events: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct MemoryMatrixToolArgs {
    #[serde(default = "default_memory_matrix")]
    matrix: String,
    #[serde(default = "default_memory_project_prefix")]
    project_prefix: String,
    min_success_rate: Option<f64>,
    min_mean_score: Option<f64>,
    max_p95_ms: Option<f64>,
}

impl MemoryMatrixToolArgs {
    fn to_verify_args(&self) -> VerifyMemoryMatrixArgs {
        VerifyMemoryMatrixArgs {
            matrix: self.matrix.clone(),
            project_prefix: self.project_prefix.clone(),
            min_success_rate: self.min_success_rate,
            min_mean_score: self.min_mean_score,
            max_p95_ms: self.max_p95_ms,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct StackPreflightToolArgs {
    #[serde(default = "default_stack_profile")]
    profile: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct WarmCacheToolArgs {
    #[serde(default)]
    projects: Vec<String>,
    #[serde(default = "default_namespace")]
    namespace: String,
    #[serde(default = "default_warm_query")]
    query: String,
    retrieval_mode: Option<String>,
    #[serde(default = "default_warm_limit")]
    limit_documents: usize,
    #[serde(default = "default_warm_limit")]
    limit_symbols: usize,
    #[serde(default = "default_warm_limit")]
    limit_chunks: usize,
    #[serde(default = "default_warm_limit")]
    limit_semantic_chunks: usize,
}

fn default_namespace() -> String {
    "default".to_string()
}

fn default_continuity_namespace() -> String {
    "continuity".to_string()
}

fn default_limit_documents() -> usize {
    5
}

fn default_limit_symbols() -> usize {
    8
}

fn default_limit_chunks() -> usize {
    8
}

fn default_limit_semantic_chunks() -> usize {
    8
}

fn default_context_pack_token_source_kind() -> String {
    "live_context_pack".to_string()
}

fn default_continuity_startup_token_source_kind() -> String {
    "live_continuity_startup".to_string()
}

fn default_true() -> bool {
    true
}

fn default_stack_profile() -> String {
    "default".to_string()
}

fn default_memory_matrix() -> String {
    "letta_memory_local".to_string()
}

fn default_memory_project_prefix() -> String {
    "memory_eval".to_string()
}

fn default_tokenizer() -> String {
    "o200k_base".to_string()
}

fn default_naive_limit_files() -> usize {
    20
}

fn default_naive_max_bytes_per_file() -> usize {
    32768
}

fn default_warm_query() -> String {
    "README".to_string()
}

fn default_warm_limit() -> usize {
    4
}

#[cfg(test)]
mod tests {
    use super::{
        ContextPackToolArgs, ContinuityStartupToolArgs, McpConfigArgs, McpError,
        benchmark_coverage_summary, context_pack_contains_primary_project,
        context_pack_input_schema, context_pack_summary, context_pack_tool_stats_block,
        context_pack_tool_summary, continuity_startup_input_schema, continuity_startup_summary,
        inject_proof_tool_arguments, mcp_tool_error_result, memory_matrix_summary,
        observe_snapshot_summary, observe_whole_cycle_input_schema,
        observe_whole_cycle_turn_input_schema, prompt_result, protocol_manifest,
        render_client_config, snapshot_has_only_ignored_critical_metrics, stack_preflight_summary,
        summarize_codes, summarize_namespace_modes, token_benchmark_summary, token_report_summary,
        verify_mcp_scope_label, verify_mcp_scope_requires_memory_matrix,
        verify_mcp_scope_requires_warm_cache, warm_cache_summary,
    };
    use crate::cli::VerifyMcpScope;
    use crate::retrieval::{ContextPackStats, ContextPackTimings};
    use crate::working_state;
    use serde_json::json;
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn renders_vscode_config() {
        let config = render_client_config(&McpConfigArgs {
            client: "vscode".to_string(),
            server_name: "amai".to_string(),
            launcher_platform: "auto".to_string(),
            ssh_destination: None,
            remote_repo_root: None,
            command: Some("/tmp/run_mcp_stdio.sh".to_string()),
            cwd: Some(PathBuf::from("/tmp/amai")),
            output: None,
        })
        .expect("render should succeed");
        let json: serde_json::Value =
            serde_json::from_str(&config).expect("config must be valid JSON");
        assert_eq!(json["servers"]["amai"]["type"], "stdio");
        assert_eq!(json["servers"]["amai"]["command"], "/tmp/run_mcp_stdio.sh");
    }

    #[test]
    fn renders_codex_config() {
        let config = render_client_config(&McpConfigArgs {
            client: "codex".to_string(),
            server_name: "amai".to_string(),
            launcher_platform: "auto".to_string(),
            ssh_destination: None,
            remote_repo_root: None,
            command: Some("/tmp/run_mcp_stdio.sh".to_string()),
            cwd: Some(PathBuf::from("/tmp/amai")),
            output: None,
        })
        .expect("render should succeed");
        let value: toml::Value = toml::from_str(&config).expect("config must be valid TOML");
        assert_eq!(
            value["mcp_servers"]["amai"]["command"].as_str(),
            Some("/tmp/run_mcp_stdio.sh")
        );
    }

    #[test]
    fn renders_windows_powershell_config() {
        let config = render_client_config(&McpConfigArgs {
            client: "cursor".to_string(),
            server_name: "amai".to_string(),
            launcher_platform: "windows-powershell".to_string(),
            ssh_destination: None,
            remote_repo_root: None,
            command: None,
            cwd: Some(PathBuf::from("/tmp/amai")),
            output: None,
        })
        .expect("render should succeed");
        let json: serde_json::Value =
            serde_json::from_str(&config).expect("config must be valid JSON");
        assert_eq!(json["mcpServers"]["amai"]["command"], "powershell.exe");
        let args = json["mcpServers"]["amai"]["args"]
            .as_array()
            .expect("args must be array");
        assert!(
            args.iter()
                .any(|item| item.as_str() == Some("\\tmp\\amai\\scripts\\run_mcp_stdio.ps1"))
        );
    }

    #[test]
    fn renders_remote_ssh_config() {
        let config = render_client_config(&McpConfigArgs {
            client: "cursor".to_string(),
            server_name: "amai".to_string(),
            launcher_platform: "auto".to_string(),
            ssh_destination: Some("ops@example-host".to_string()),
            remote_repo_root: Some(PathBuf::from("/srv/amai")),
            command: None,
            cwd: Some(PathBuf::from("/tmp/amai")),
            output: None,
        })
        .expect("render should succeed");
        let json: serde_json::Value =
            serde_json::from_str(&config).expect("config must be valid JSON");
        assert_eq!(json["mcpServers"]["amai"]["command"], "ssh");
        let args = json["mcpServers"]["amai"]["args"]
            .as_array()
            .expect("args must be array");
        assert_eq!(args[0], "ops@example-host");
        assert_eq!(args[1], "cd '/srv/amai' && ./scripts/run_mcp_stdio.sh");
    }

    #[test]
    fn context_pack_schema_exposes_whole_cycle_observed_fields() {
        let schema = context_pack_input_schema(true);
        let properties = schema["properties"]
            .as_object()
            .expect("properties must be object");
        for field in [
            "client_prompt_tokens",
            "assistant_generation_tokens",
            "tool_overhead_tokens",
            "continuity_restore_tokens",
        ] {
            assert!(properties.contains_key(field), "missing field {field}");
        }
    }

    #[test]
    fn context_pack_tool_args_forward_whole_cycle_observed_fields() {
        let args = ContextPackToolArgs {
            project: "art".to_string(),
            namespace: "continuity".to_string(),
            query: "same meter".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            disable_cache: false,
            limit_documents: 5,
            limit_symbols: 8,
            limit_chunks: 8,
            limit_semantic_chunks: 8,
            token_source_kind: "proof_mcp_context_pack".to_string(),
            client_prompt_tokens: Some(42),
            assistant_generation_tokens: Some(24),
            tool_overhead_tokens: Some(7),
            continuity_restore_tokens: Some(3),
            persist: true,
        };

        let context = args.to_context_args();
        assert_eq!(context.token_source_kind, "proof_mcp_context_pack");
        assert_eq!(context.client_prompt_tokens, Some(42));
        assert_eq!(context.assistant_generation_tokens, Some(24));
        assert_eq!(context.tool_overhead_tokens, Some(7));
        assert_eq!(context.continuity_restore_tokens, Some(3));
    }

    #[test]
    fn continuity_startup_schema_exposes_project_or_repo_binding_fields() {
        let schema = continuity_startup_input_schema();
        let properties = schema["properties"]
            .as_object()
            .expect("properties must be object");
        assert!(properties.contains_key("project"));
        assert!(properties.contains_key("repo_root"));
        assert!(properties.contains_key("namespace"));
        assert!(properties.contains_key("token_source_kind"));
    }

    #[test]
    fn continuity_startup_tool_args_forward_cli_contract() {
        let args = ContinuityStartupToolArgs {
            project: Some("art".to_string()),
            repo_root: Some("/tmp/art".to_string()),
            namespace: "continuity".to_string(),
            token_source_kind: "proof_mcp_continuity_startup".to_string(),
        };
        let cli = args.to_cli_args();
        assert_eq!(cli.project.as_deref(), Some("art"));
        assert_eq!(
            cli.repo_root
                .as_ref()
                .map(|path| path.display().to_string()),
            Some("/tmp/art".to_string())
        );
        assert_eq!(cli.namespace, "continuity");
        assert!(cli.json);
        assert_eq!(cli.token_source_kind, "proof_mcp_continuity_startup");
    }

    #[test]
    fn proof_tool_call_injects_non_live_defaults_for_context_pack() {
        let injected = inject_proof_tool_arguments(
            "amai_context_pack",
            json!({
                "project": "art",
                "namespace": "continuity",
                "query": "token drift"
            }),
        );
        assert_eq!(
            injected["token_source_kind"].as_str(),
            Some("proof_mcp_context_pack")
        );
    }

    #[test]
    fn proof_tool_call_preserves_explicit_token_source_kind() {
        let injected = inject_proof_tool_arguments(
            "amai_context_pack",
            json!({
                "project": "art",
                "namespace": "continuity",
                "query": "token drift",
                "token_source_kind": "verify_context_pack"
            }),
        );
        assert_eq!(
            injected["token_source_kind"].as_str(),
            Some("verify_context_pack")
        );
    }

    #[test]
    fn proof_tool_call_injects_non_live_defaults_for_continuity_startup() {
        let injected = inject_proof_tool_arguments(
            "amai_continuity_startup",
            json!({
                "project": "art",
                "namespace": "continuity"
            }),
        );
        assert_eq!(
            injected["token_source_kind"].as_str(),
            Some("proof_mcp_continuity_startup")
        );
    }

    #[test]
    fn observe_whole_cycle_schema_requires_context_pack_id() {
        let schema = observe_whole_cycle_input_schema();
        let properties = schema["properties"]
            .as_object()
            .expect("properties must be object");
        assert!(properties.contains_key("context_pack_id"));
        assert!(properties.contains_key("assistant_generation_tokens"));
        assert_eq!(schema["required"], json!(["context_pack_id"]));
    }

    #[test]
    fn observe_whole_cycle_turn_schema_requires_turn_group_fields() {
        let schema = observe_whole_cycle_turn_input_schema();
        let properties = schema["properties"]
            .as_object()
            .expect("properties must be object");
        assert!(properties.contains_key("thread_id"));
        assert!(properties.contains_key("turn_id"));
        assert!(properties.contains_key("context_pack_ids"));
        assert!(properties.contains_key("assistant_generation_tokens"));
        assert_eq!(
            schema["required"],
            json!(["turn_id", "context_pack_ids", "assistant_generation_tokens"])
        );
    }

    #[test]
    fn observe_snapshot_summary_uses_reason_summaries_and_trace_fallback() {
        let snapshot = json!({
            "continuity_correctness_model": {
                "summary": {
                    "status": "pass",
                    "verified_probes": 9,
                    "probe_count": 9
                }
            },
            "compatibility": {
                "profile": "amai-1",
                "compatible": true
            },
            "sla": {
                "summary": {
                    "pass": 19,
                    "alert": 0,
                    "critical": 0,
                    "unknown": 0
                }
            },
            "latest_working_state_restore": {
                "working_state_restore": {
                    "included_reasons_summary": "exact_documents (1) — Exact layer matched the visible document.",
                    "latest_decision_trace": {
                        "included": [{
                            "strategy": "lexical_chunks",
                            "count": 2,
                            "reason": "fallback should not win"
                        }],
                        "not_included": [{
                            "strategy": "semantic_chunks",
                            "reason": "Semantic layer abstained."
                        }]
                    }
                }
            }
        });

        let summary = observe_snapshot_summary(&snapshot);
        assert_eq!(summary.pass, 19);
        assert_eq!(summary.alert, 0);
        assert_eq!(summary.critical, 0);
        assert_eq!(summary.unknown, 0);
        assert_eq!(summary.continuity_status.as_deref(), Some("pass"));
        assert_eq!(summary.continuity_verified_probes, 9);
        assert_eq!(summary.continuity_total_probes, 9);
        assert_eq!(summary.compatibility_profile.as_deref(), Some("amai-1"));
        assert_eq!(summary.compatibility_compatible, Some(true));
        assert_eq!(
            summary.included_reasons_summary.as_deref(),
            Some("exact_documents (1) — Exact layer matched the visible document.")
        );
        assert_eq!(
            summary.excluded_reasons_summary.as_deref(),
            Some("semantic_chunks — Semantic layer abstained.")
        );
    }

    #[test]
    fn token_report_summary_surfaces_scope_and_counting_semantics() {
        let payload = json!({
            "token_budget_report": {
                "headline": {
                    "metric_code": "verified_effective_savings_pct",
                    "scope_label": "окно Обычная рабочая машина",
                    "status": "pass",
                    "value_percent": 99.48,
                    "saved_tokens": 6923645,
                    "events_count": 120,
                    "counted_events": 75,
                    "note": "Это главный честный KPI: live-only, quality-gated и с учётом recovery."
                },
                "rolling_window": {
                    "events_total": 120
                },
                "agent_cycle_economics": {
                    "status": "partial_lower_bound",
                    "contract": {
                        "note": "Это честная нижняя граница полного агентного цикла."
                    },
                    "rolling_window": {
                        "scope_label": "окно Обычная рабочая машина",
                        "verified_measured_saved_pct": 96.11,
                        "verified_measured_saved_tokens": 6812345
                    }
                },
                "contractual_statement_summaries": {
                    "rolling_window": {
                        "scope_label": "окно Обычная рабочая машина",
                        "contractual_state": "report_only_preview_open",
                        "coverage_state": "partially_confirmed",
                        "metering_ingest_state": "soft_lag",
                        "contractual_lag_state": "awaiting_late_events",
                        "contractual_freshness_state": "provisional_open_window",
                        "reconciliation_state": "awaiting_provider_usage_source",
                        "margin_state": "awaiting_rate_card",
                        "blocking_reasons": [
                            "provider_usage_source_missing",
                            "provider_rate_card_unpriced"
                        ]
                    }
                }
            }
        });

        let summary = token_report_summary(&payload);
        assert_eq!(summary.metric_code, "verified_effective_savings_pct");
        assert_eq!(summary.scope_label, "окно Обычная рабочая машина");
        assert_eq!(summary.status, "pass");
        assert_eq!(summary.value_percent, 99.48);
        assert_eq!(summary.saved_tokens, 6923645);
        assert_eq!(summary.events_count, 120);
        assert_eq!(summary.counted_events, 75);
        assert_eq!(
            summary.agent_cycle_scope_label,
            "окно Обычная рабочая машина"
        );
        assert_eq!(summary.agent_cycle_status, "partial_lower_bound");
        assert_eq!(summary.agent_cycle_verified_saved_percent, 96.11);
        assert_eq!(summary.agent_cycle_verified_saved_tokens, 6812345);
        assert_eq!(
            summary.agent_cycle_note,
            "Это честная нижняя граница полного агентного цикла."
        );
        assert_eq!(
            summary.contractual_scope_label,
            "окно Обычная рабочая машина"
        );
        assert_eq!(summary.contractual_state, "report_only_preview_open");
        assert_eq!(summary.contractual_coverage_state, "partially_confirmed");
        assert_eq!(summary.contractual_metering_ingest_state, "soft_lag");
        assert_eq!(summary.contractual_lag_state, "awaiting_late_events");
        assert_eq!(
            summary.contractual_freshness_state,
            "provisional_open_window"
        );
        assert_eq!(
            summary.contractual_reconciliation_state,
            "awaiting_provider_usage_source"
        );
        assert_eq!(summary.contractual_margin_state, "awaiting_rate_card");
        assert_eq!(
            summary.contractual_blockers_summary,
            "provider_usage_source_missing, provider_rate_card_unpriced"
        );
        assert_eq!(
            summary.note,
            "Это главный честный KPI: live-only, quality-gated и с учётом recovery."
        );
    }

    #[test]
    fn memory_matrix_summary_surfaces_product_eval_contract() {
        let payload = json!({
            "memory_task_matrix": {
                "matrix": "letta_memory_local",
                "display_name": "Letta-style local memory matrix",
                "tasks_total": 8,
                "tasks_passed": 8,
                "tasks_failed": 0,
                "success_rate": 1.0,
                "mean_score": 1.0,
                "p95_ms": 418.778,
                "gate_failures": [],
                "canonical_eval": {
                    "verdict_counts": {
                        "hit_correct_target": 4,
                        "recovered_useful": 4
                    }
                }
            }
        });

        let summary = memory_matrix_summary(&payload);
        assert_eq!(summary.matrix, "letta_memory_local");
        assert_eq!(summary.display_name, "Letta-style local memory matrix");
        assert_eq!(summary.tasks_total, 8);
        assert_eq!(summary.tasks_passed, 8);
        assert_eq!(summary.tasks_failed, 0);
        assert_eq!(summary.success_rate, 1.0);
        assert_eq!(summary.mean_score, 1.0);
        assert_eq!(summary.p95_ms, 418.778);
        assert_eq!(summary.gate_failures_count, 0);
        assert_eq!(
            summary.compact_verdict_counts,
            "hit_correct_target=4, recovered_useful=4"
        );
    }

    #[test]
    fn benchmark_coverage_summary_surfaces_eval_taxonomy_totals() {
        let payload = json!({
            "source": {
                "display_name": "Benchmark Compendium"
            },
            "coverage_counts": {
                "total": 19,
                "materialized": 0,
                "partial": 2,
                "mapped": 12,
                "next_priority": 1,
                "future": 4
            },
            "families": [{
                "next_priorities": [
                    "SWE-bench Verified (swe_bench_verified)",
                    "τ-bench (tau_bench)"
                ]
            }]
        });

        let summary = benchmark_coverage_summary(&payload);
        assert_eq!(summary.source_display_name, "Benchmark Compendium");
        assert_eq!(summary.total_benchmarks, 19);
        assert_eq!(summary.materialized, 0);
        assert_eq!(summary.partial, 2);
        assert_eq!(summary.mapped, 12);
        assert_eq!(summary.next_priority, 1);
        assert_eq!(summary.future, 4);
        assert_eq!(
            summary.next_priorities_summary,
            "SWE-bench Verified (swe_bench_verified), τ-bench (tau_bench)"
        );
    }

    #[test]
    fn context_pack_summary_surfaces_included_and_excluded_reasons() {
        let payload = json!({
            "decision_trace": {
                "included": [{
                    "strategy": "exact_documents",
                    "count": 1,
                    "reason": "Нашлись точные document/path совпадения внутри видимого контура."
                }],
                "not_included": [{
                    "strategy": "semantic_chunks",
                    "reason": "Semantic layer не добавил новых фрагментов после scope и relevance проверки."
                }]
            }
        });

        let summary = context_pack_summary(&payload);
        assert_eq!(
            summary.included_reasons_summary.as_deref(),
            Some(
                "exact_documents (1) — Нашлись точные document/path совпадения внутри видимого контура."
            )
        );
        assert_eq!(
            summary.excluded_reasons_summary.as_deref(),
            Some(
                "semantic_chunks — Semantic layer не добавил новых фрагментов после scope и relevance проверки."
            )
        );
    }

    #[test]
    fn context_pack_contains_primary_project_accepts_cache_reuse_shape() {
        let cache_reuse_payload = json!({
            "context_pack": {
                "project": {
                    "code": "amai"
                },
                "cache_reuse_reference": {
                    "state": "same_thread_context_pack_replay"
                }
            }
        });
        let full_payload = json!({
            "context_pack": {
                "project": {
                    "code": "amai"
                },
                "visible_projects": [
                    { "project_code": "amai" }
                ]
            }
        });
        let wrong_payload = json!({
            "context_pack": {
                "project": {
                    "code": "other"
                }
            }
        });

        assert!(context_pack_contains_primary_project(
            &cache_reuse_payload,
            "amai"
        ));
        assert!(context_pack_contains_primary_project(&full_payload, "amai"));
        assert!(!context_pack_contains_primary_project(
            &wrong_payload,
            "amai"
        ));
    }

    #[test]
    fn snapshot_ignored_critical_filter_accepts_benchmark_contamination_only() {
        let checks = json!([
            {
                "metric": "observability.benchmark_contamination",
                "status": "critical"
            },
            {
                "metric": "postgres.connection_usage_ratio",
                "status": "pass"
            }
        ]);
        let mixed_checks = json!([
            {
                "metric": "observability.benchmark_contamination",
                "status": "critical"
            },
            {
                "metric": "postgres.connection_usage_ratio",
                "status": "critical"
            }
        ]);

        assert!(snapshot_has_only_ignored_critical_metrics(
            &checks,
            &["observability.benchmark_contamination"]
        ));
        assert!(!snapshot_has_only_ignored_critical_metrics(
            &mixed_checks,
            &["observability.benchmark_contamination"]
        ));
    }

    #[test]
    fn token_ledger_mcp_scope_skips_heavy_tail() {
        assert!(!verify_mcp_scope_requires_memory_matrix(
            VerifyMcpScope::TokenLedger
        ));
        assert!(!verify_mcp_scope_requires_warm_cache(
            VerifyMcpScope::TokenLedger
        ));
        assert_eq!(
            verify_mcp_scope_label(VerifyMcpScope::TokenLedger),
            "token-ledger"
        );
    }

    #[test]
    fn full_mcp_scope_keeps_heavy_tail() {
        assert!(verify_mcp_scope_requires_memory_matrix(
            VerifyMcpScope::Full
        ));
        assert!(verify_mcp_scope_requires_warm_cache(VerifyMcpScope::Full));
        assert_eq!(verify_mcp_scope_label(VerifyMcpScope::Full), "full");
    }

    #[test]
    fn context_pack_tool_payload_stays_compact_for_model_visible_output() {
        let stats = ContextPackStats {
            context_pack_id: Uuid::parse_str("12345678-1234-5678-9abc-123456789abc").expect("uuid"),
            exact_documents: 2,
            symbol_hits: 1,
            lexical_chunks: 3,
            semantic_chunks: 4,
            cache_hit: true,
            scope_signature: "scope-signature-heavy-value".to_string(),
            timings: ContextPackTimings {
                resolve_scope_ms: 11,
                exact_lookup_ms: 12,
                symbol_lookup_ms: 13,
                lexical_lookup_ms: 14,
                query_embed_ms: 15,
                semantic_search_ms: 16,
                semantic_hydrate_ms: 17,
                ranking_ms: 18,
                provenance_ms: 19,
                pack_assembly_ms: 20,
                cache_lookup_ms: 21,
                serialize_ms: 22,
                persist_ms: 23,
            },
        };

        let compact_stats = context_pack_tool_stats_block(&stats);
        let compact_summary = context_pack_tool_summary(&stats);
        let legacy_stats = json!({
            "context_pack_id": stats.context_pack_id,
            "exact_documents": stats.exact_documents,
            "symbol_hits": stats.symbol_hits,
            "lexical_chunks": stats.lexical_chunks,
            "semantic_chunks": stats.semantic_chunks,
            "cache_hit": stats.cache_hit,
            "scope_signature": stats.scope_signature,
            "timings_ms": {
                "resolve_scope_ms": stats.timings.resolve_scope_ms,
                "cache_lookup_ms": stats.timings.cache_lookup_ms,
                "exact_lookup_ms": stats.timings.exact_lookup_ms,
                "symbol_lookup_ms": stats.timings.symbol_lookup_ms,
                "lexical_lookup_ms": stats.timings.lexical_lookup_ms,
                "query_embed_ms": stats.timings.query_embed_ms,
                "semantic_search_ms": stats.timings.semantic_search_ms,
                "semantic_hydrate_ms": stats.timings.semantic_hydrate_ms,
                "serialize_ms": stats.timings.serialize_ms,
                "persist_ms": stats.timings.persist_ms,
            }
        });
        let legacy_summary = "context pack built for amai:continuity :: docs=2 symbols=1 lexical=3 semantic=4 cache_hit=true included=exact_documents (2) excluded=semantic_chunks";

        assert_eq!(
            compact_stats["retrieval_counts"],
            json!({
                "exact_documents": 2,
                "symbol_hits": 1,
                "lexical_chunks": 3,
                "semantic_chunks": 4,
            })
        );
        assert_eq!(compact_stats["cache_hit"], json!(true));
        assert_eq!(compact_summary, "ctx d=2 s=1 l=3 m=4 c=1");
        assert!(
            serde_json::to_string(&compact_stats)
                .expect("compact stats")
                .len()
                < serde_json::to_string(&legacy_stats)
                    .expect("legacy stats")
                    .len()
        );
        assert!(compact_summary.len() < legacy_summary.len());
    }

    #[test]
    fn token_benchmark_summary_surfaces_naive_vs_context_scope() {
        let payload = json!({
            "token_benchmark": {
                "naive_scope": {
                    "files_considered": 12,
                    "tokens": 4096
                },
                "context_pack_render": {
                    "tokens": 512
                },
                "savings": {
                    "saved_tokens": 3584,
                    "savings_factor": 8.0,
                    "savings_percent": 87.5
                }
            }
        });

        let summary = token_benchmark_summary(&payload);
        assert_eq!(summary.saved_tokens, 3584);
        assert_eq!(summary.savings_factor, 8.0);
        assert_eq!(summary.savings_percent, 87.5);
        assert_eq!(summary.naive_tokens, 4096);
        assert_eq!(summary.context_tokens, 512);
        assert_eq!(summary.files_considered, 12);
    }

    #[test]
    fn warm_cache_summary_surfaces_cache_and_layer_totals() {
        let warmed = vec![
            json!({
                "project": "art",
                "cache_hit": true,
                "exact_documents": 2,
                "symbol_hits": 1,
                "lexical_chunks": 3,
                "semantic_chunks": 0,
            }),
            json!({
                "project": "regart",
                "cache_hit": false,
                "exact_documents": 1,
                "symbol_hits": 0,
                "lexical_chunks": 2,
                "semantic_chunks": 4,
            }),
        ];
        let projects = vec!["art".to_string(), "regart".to_string()];

        let summary = warm_cache_summary(&warmed, &projects);
        assert_eq!(summary.project_count, 2);
        assert_eq!(summary.compact_projects, "art, regart");
        assert_eq!(summary.cache_hits, 1);
        assert_eq!(summary.exact_documents, 3);
        assert_eq!(summary.symbol_hits, 1);
        assert_eq!(summary.lexical_chunks, 5);
        assert_eq!(summary.semantic_chunks, 4);
    }

    #[test]
    fn stack_preflight_summary_surfaces_machine_guarantees() {
        let payload = json!({
            "profile_code": "default",
            "profile": {
                "display_name": "Workstation Full",
                "supports_peak_benchmarks": true,
                "start_monitoring_by_default": false,
                "remote_mode_recommended": false
            },
            "host": {
                "logical_cpus": 16,
                "total_memory_gib": 31.5,
                "available_disk_gib": 420.0
            },
            "verdict": "pass",
            "unmet_minimums": [],
            "unmet_recommendations": ["memory below recommendation"]
        });

        let summary = stack_preflight_summary(&payload);
        assert_eq!(summary.profile_code, "default");
        assert_eq!(summary.profile_display_name, "Workstation Full");
        assert_eq!(summary.verdict, "pass");
        assert_eq!(summary.host_logical_cpus, 16);
        assert_eq!(summary.host_total_memory_gib, 31.5);
        assert_eq!(summary.host_available_disk_gib, 420.0);
        assert!(summary.supports_peak_benchmarks);
        assert!(!summary.start_monitoring_by_default);
        assert!(!summary.remote_mode_recommended);
        assert_eq!(summary.unmet_minimums_count, 0);
        assert_eq!(summary.unmet_recommendations_count, 1);
    }

    #[test]
    fn protocol_manifest_lists_summary_contracts() {
        let manifest = protocol_manifest();
        assert_eq!(manifest["version"].as_str(), Some("mcp-contract-v2"));
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["tool"].as_str(),
            Some("amai_continuity_startup")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["prompt"].as_str(),
            Some("amai-continuity-startup")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["contract_version"].as_str(),
            Some("continuity-startup-contract-v13")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["must_call_before_substantive_work"].as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["project_binding_rule"].as_str(),
            Some("registered_project_fail_closed")
        );
        let startup_required_fields =
            manifest["startup_contracts"]["project_chat_startup"]["required_summary_fields"]
                .as_array()
                .expect("startup required summary fields");
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("execctl_resume_contract_summary"))
        );
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("execctl_resume_obligation"))
        );
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("startup_execution_gate"))
        );
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("startup_next_action"))
        );
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("execctl_active_lease"))
        );
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("execctl_active_lease_summary"))
        );
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("required_return_task"))
        );
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("project_task_tree"))
        );
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("project_task_tree_summary"))
        );
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("project_task_ledger"))
        );
        assert!(
            startup_required_fields
                .iter()
                .any(|field| field.as_str() == Some("project_task_ledger_summary"))
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["runtime_state_artifact"]
                ["workspace_runtime_state_artifact_version"]
                .as_str(),
            Some("workspace-startup-runtime-state-v4")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["runtime_state_artifact"]
                ["startup_execution_gate_field"]
                .as_str(),
            Some("startup_execution_gate")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["runtime_state_artifact"]
                ["startup_execution_gate_version"]
                .as_str(),
            Some("startup-execution-gate-v1")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["runtime_state_artifact"]
                ["gate_semantics_consistent_field"]
                .as_str(),
            Some("gate_semantics_consistent")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["runtime_state_artifact"]
                ["gate_semantics_consistent_true_required"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["runtime_state_artifact"]
                ["inspection_fallback_cli"]["command"]
                .as_str(),
            Some("continuity startup-state")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["runtime_state_artifact"]
                ["inspection_fallback_cli"]["returns_startup_execution_gate"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["artifact_enforcement"]
                ["workspace_contract_relative_path"]
                .as_str(),
            Some(".amai/onboarding/project-chat-startup-contract.json")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["artifact_enforcement"]
                ["workspace_contract_required_before_tool_call"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["artifact_enforcement"]
                ["missing_or_unreadable_fail_closed"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["artifact_enforcement"]
                ["sha256_mismatch_fail_closed"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["runtime_state_artifact"]
                ["workspace_runtime_state_relative_path"]
                .as_str(),
            Some(".amai/continuity/project-chat-startup-state.json")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["runtime_state_artifact"]
                ["written_by_tool"]
                .as_str(),
            Some("amai_continuity_startup")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["runtime_state_artifact"]
                ["source_summary_field"]
                .as_str(),
            Some("continuity_startup_summary")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]["gate_field"]
                .as_str(),
            Some("startup_execution_gate")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]["must_follow_field"]
                .as_str(),
            Some("must_follow_startup_next_action")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]["unrelated_work_allowed_field"]
                .as_str(),
            Some("unrelated_work_allowed")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]
                ["must_read_prompt_text_before_reply_field"]
                .as_str(),
            Some("must_read_prompt_text_before_reply")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]["required_action_kind_field"]
                .as_str(),
            Some("required_action_kind_when_resume_required")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]["no_silent_drop_field"]
                .as_str(),
            Some("no_silent_drop")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]
                ["blocking_true_requires_must_follow"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]
                ["blocking_true_blocks_unrelated_work"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]
                ["must_follow_true_blocks_unrelated_work"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]
                ["unrelated_work_allowed_false_blocks_unrelated_work"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]
                ["must_read_prompt_text_true_requires_prompt_before_reply"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]
                ["required_action_kind_resume_required_value"]
                .as_str(),
            Some("resume_required_return_task")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]
                ["startup_execution_gate_enforcement"]["no_silent_drop_must_be_true"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["resume_enforcement"]
                ["obligation_field"]
                .as_str(),
            Some("execctl_resume_obligation")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["resume_enforcement"]
                ["startup_next_action_field"]
                .as_str(),
            Some("startup_next_action")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["resume_enforcement"]
                ["active_lease_field"]
                .as_str(),
            Some("execctl_active_lease")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["resume_enforcement"]
                ["active_lease_owner_state_field"]
                .as_str(),
            Some("lease_owner_state")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["resume_enforcement"]
                ["previous_session_owner_value"]
                .as_str(),
            Some("previous_session_owner")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["resume_enforcement"]
                ["must_resume_required_return_task_before_unrelated_work"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["resume_enforcement"]
                ["previous_session_owner_must_follow_startup_next_action"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["resume_enforcement"]
                ["required_action_kind_when_resume_required"]
                .as_str(),
            Some("resume_required_return_task")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["resume_enforcement"]
                ["no_silent_drop"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["tool_contracts"]["amai_continuity_startup"]["summary_field"].as_str(),
            Some("continuity_startup_summary")
        );
        assert_eq!(
            manifest["tool_contracts"]["amai_context_pack"]["summary_field"].as_str(),
            Some("context_pack_summary")
        );
        assert_eq!(
            manifest["tool_contracts"]["amai_observe_whole_cycle"]["summary_field"].as_str(),
            Some("whole_cycle_observed_attach")
        );
        assert_eq!(
            manifest["tool_contracts"]["amai_observe_whole_cycle_turn"]["summary_field"].as_str(),
            Some("assistant_generation_turn_observed_attach")
        );
        assert_eq!(
            manifest["tool_contracts"]["amai_warm_cache"]["summary_field"].as_str(),
            Some("warm_cache_summary")
        );
        assert_eq!(
            manifest["tool_contracts"]["amai_stack_preflight"]["summary_field"].as_str(),
            Some("preflight_summary")
        );
        assert_eq!(
            manifest["tool_contracts"]["amai_benchmark_coverage"]["summary_field"].as_str(),
            Some("benchmark_coverage_summary")
        );
        assert_eq!(
            manifest["tool_contracts"]["amai_memory_matrix"]["summary_field"].as_str(),
            Some("memory_matrix_summary")
        );
        assert_eq!(
            manifest["prompt_contracts"]["amai-onboarding"]["purpose"].as_str(),
            Some("safe onboarding without mixing projects")
        );
        assert_eq!(
            manifest["prompt_contracts"]["amai-continuity-startup"]["purpose"].as_str(),
            Some(
                "project-scoped startup guidance for continuity restore and live client-budget discipline before each substantive reply"
            )
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["contract_version"].as_str(),
            Some("continuity-startup-contract-v13")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["guard_command"]
                .as_str(),
            Some("observe client-budget-gate")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["guard_summary_field"]
                .as_str(),
            Some("client_budget_reply_gate")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["reply_execution_gate_field"]
                .as_str(),
            Some("reply_execution_gate")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["reply_execution_gate_version"]
                .as_str(),
            Some("client-reply-budget-gate-v1")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["reply_budget_mode_field"]
                .as_str(),
            Some("reply_budget_mode")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["reply_budget_contract_field"]
                .as_str(),
            Some("reply_budget_contract")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["compact_reply_mode_value"]
                .as_str(),
            Some(working_state::CLIENT_REPLY_BUDGET_MODE_COMPACT_HIGH_SIGNAL)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["compact_reply_contract_version"]
                .as_str(),
            Some(working_state::CLIENT_REPLY_BUDGET_CONTRACT_VERSION)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["compact_diagnostics_command"]
                .as_str(),
            Some("observe client-budget-root-cause")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["must_prefer_compact_diagnostics_over_full_snapshot"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["guard_enforcement_flag"]
                .as_str(),
            Some("--enforce-reply-gate")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["guard_enforcement_exit_on_blocking"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["must_check_before_each_substantive_reply"]
                .as_bool(),
            Some(true)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["max_guard_age_seconds"]
                .as_u64(),
            Some(10)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["blocking_reply_contract_field"]
                .as_str(),
            Some("blocking_reply_contract")
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["blocking_reply_contract_version"]
                .as_str(),
            Some(working_state::CLIENT_BUDGET_BLOCKING_REPLY_CONTRACT_VERSION)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["blocking_reply_response_kind"]
                .as_str(),
            Some(working_state::CLIENT_BUDGET_BLOCKING_REPLY_RESPONSE_KIND)
        );
        assert_eq!(
            manifest["startup_contracts"]["project_chat_startup"]["live_client_budget_enforcement"]
                ["blocking_reply_max_sentences"]
                .as_u64(),
            Some(working_state::CLIENT_BUDGET_BLOCKING_REPLY_MAX_SENTENCES)
        );
        assert_eq!(
            manifest["error_contracts"]["tool_execution_failed"]["error_class"].as_str(),
            Some("tool_runtime")
        );
        assert_eq!(
            manifest["error_contracts"]["invalid_params"]["carrier"].as_str(),
            Some("jsonrpc_error_or_tool_is_error")
        );
        let safety_laws = manifest["safety_laws"]
            .as_array()
            .expect("safety laws array");
        assert!(!safety_laws.is_empty());
    }

    #[test]
    fn continuity_startup_prompt_points_to_canonical_tool() {
        let result = prompt_result(json!({
            "name": "amai-continuity-startup",
            "arguments": {
                "project": "art",
                "namespace": "continuity"
            }
        }))
        .expect("prompt result");
        let text = result["messages"][0]["content"]["text"]
            .as_str()
            .unwrap_or_default();
        assert!(text.contains("amai_continuity_startup"));
        assert!(text.contains("pending_return_queue"));
        assert!(text.contains("execctl_resume_contract_summary"));
        assert!(text.contains("execctl_resume_obligation"));
        assert!(text.contains("startup_execution_gate"));
        assert!(text.contains("gate_semantics_consistent"));
        assert!(text.contains("startup_next_action"));
        assert!(text.contains("execctl_active_lease"));
        assert!(text.contains("execctl_active_lease_summary"));
        assert!(text.contains("lease_owner_state"));
        assert!(text.contains("previous_session_owner"));
        assert!(text.contains("resume_required_return_task"));
    }

    #[test]
    fn continuity_startup_summary_surfaces_execctl_and_prompt_state() {
        let payload = json!({
            "continuity_startup": {
                "project": { "code": "art" },
                "namespace": { "code": "continuity" }
            },
            "chat_start_restore": {
                "headline": "ExecCtl pending return contour materialized in Amai",
                "next_step": "Continue runtime auto-start guarantees.",
                "restore_confidence": "high",
                "thread_count": 8,
                "prompt_text": "CHAT_START_RESTORE\nProject: Art",
                "execctl_resume_state": "pending_return_queue_present",
                "pending_return_summary": "Same-meter spend control -> Materialize live assistant generation source.",
                "execctl_resume_contract_summary": "return_required(1): Same-meter spend control -> Materialize live assistant generation source.",
                "execctl_resume_obligation": {
                    "resume_state": "return_required",
                    "no_silent_drop": true,
                    "pending_return_count": 1,
                    "active_task_headline": "Continue runtime auto-start guarantees.",
                    "required_return_headline": "Same-meter spend control",
                    "required_return_next_step": "Materialize live assistant generation source."
                },
                "startup_execution_gate": {
                    "gate_version": "startup-execution-gate-v1",
                    "action_kind": "resume_required_return_task",
                    "must_follow_startup_next_action": true,
                    "unrelated_work_allowed": false
                },
                "startup_next_action": {
                    "action_kind": "resume_required_return_task",
                    "blocking": true,
                    "headline": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                },
                "startup_next_action_summary": "resume_required_return_task: Same-meter spend control -> Materialize live assistant generation source.",
                "execctl_active_lease": {
                    "lease_owner_state": "previous_session_owner",
                    "headline": "Continue runtime auto-start guarantees.",
                    "next_step": "Re-enter the active workline.",
                    "storage_lane": "ami.execctl_task_leases"
                },
                "execctl_active_lease_summary": "previous_session_owner: Continue runtime auto-start guarantees. -> Re-enter the active workline.",
                "required_return_task": {
                    "headline": "Same-meter spend control",
                    "next_step": "Materialize live assistant generation source."
                },
                "project_task_tree": {
                    "open_tasks_count": 2,
                    "nodes": [
                        {"task_role": "active", "headline": "Continue runtime auto-start guarantees."},
                        {"task_role": "pending_return", "headline": "Same-meter spend control"}
                    ]
                },
                "project_task_tree_summary": "active: Continue runtime auto-start guarantees.; pending_return(1): Same-meter spend control -> Materialize live assistant generation source.",
                "project_task_ledger": {
                    "open_tasks_count": 2,
                    "historical_handoffs_count": 3,
                    "entries": [
                        {"task_role": "active", "headline": "Continue runtime auto-start guarantees."}
                    ]
                },
                "project_task_ledger_summary": "active: Continue runtime auto-start guarantees.; pending_return(1); historical_handoffs(3)",
                "included_reasons_summary": "exact_documents (1) — Exact layer matched.",
                "excluded_reasons_summary": "semantic_chunks — Semantic layer abstained."
            }
        });
        let summary = continuity_startup_summary(&payload);
        assert_eq!(summary.project_code, "art");
        assert_eq!(summary.namespace_code, "continuity");
        assert_eq!(summary.execctl_resume_state, "pending_return_queue_present");
        assert!(summary.prompt_text_present);
        assert_eq!(summary.thread_count, 8);
        assert!(
            summary
                .pending_return_summary
                .as_deref()
                .is_some_and(|value| value.contains("Same-meter spend control"))
        );
        assert!(
            summary
                .execctl_resume_contract_summary
                .as_deref()
                .is_some_and(|value| value.contains("return_required(1)"))
        );
        assert_eq!(
            summary.execctl_resume_obligation["resume_state"],
            json!("return_required")
        );
        assert_eq!(
            summary.execctl_resume_obligation["required_return_headline"],
            json!("Same-meter spend control")
        );
        assert_eq!(
            summary.startup_execution_gate["action_kind"],
            json!("resume_required_return_task")
        );
        assert_eq!(
            summary.startup_execution_gate["must_follow_startup_next_action"],
            json!(true)
        );
        assert_eq!(
            summary.startup_next_action["action_kind"],
            json!("resume_required_return_task")
        );
        assert!(
            summary
                .startup_next_action_summary
                .as_deref()
                .is_some_and(|value| value.contains("resume_required_return_task"))
        );
        assert_eq!(
            summary.execctl_active_lease["storage_lane"],
            json!("ami.execctl_task_leases")
        );
        assert!(
            summary
                .execctl_active_lease_summary
                .as_deref()
                .is_some_and(|value| value.contains("previous_session_owner"))
        );
        assert_eq!(
            summary.required_return_task["headline"],
            json!("Same-meter spend control")
        );
        assert_eq!(summary.project_task_tree["open_tasks_count"], json!(2));
        assert!(
            summary
                .project_task_tree_summary
                .as_deref()
                .is_some_and(|value| value.contains("pending_return(1)"))
        );
        assert_eq!(
            summary.project_task_ledger["historical_handoffs_count"],
            json!(3)
        );
        assert!(
            summary
                .project_task_ledger_summary
                .as_deref()
                .is_some_and(|value| value.contains("historical_handoffs(3)"))
        );
    }

    #[test]
    fn startup_contract_exposes_global_budget_wait_reply_contract() {
        let contract = super::project_chat_startup_contract();
        let enforcement = &contract["live_client_budget_enforcement"];
        let blocking_action_kinds = enforcement["blocking_action_kinds"]
            .as_array()
            .expect("blocking action kinds");
        assert!(
            blocking_action_kinds
                .iter()
                .any(|value| value.as_str() == Some("wait_for_global_client_budget_recovery"))
        );

        let allowed_response_kinds = enforcement["blocking_reply_allowed_response_kinds"]
            .as_array()
            .expect("allowed response kinds");
        assert!(allowed_response_kinds.iter().any(|value| {
            value.as_str() == Some(working_state::CLIENT_BUDGET_WAIT_BLOCKING_REPLY_RESPONSE_KIND)
        }));

        let allowed_templates = enforcement["blocking_reply_allowed_templates"]
            .as_array()
            .expect("allowed templates");
        assert!(allowed_templates.iter().any(|value| {
            value.as_str() == Some(working_state::CLIENT_BUDGET_WAIT_BLOCKING_REPLY_TEMPLATE)
        }));
    }

    #[test]
    fn tool_error_result_carries_machine_readable_taxonomy() {
        let result = mcp_tool_error_result(&McpError::tool_not_found("missing_tool"));
        assert_eq!(result["isError"].as_bool(), Some(true));
        assert_eq!(
            result["structuredContent"]["error_taxonomy"]["amai_error_code"].as_str(),
            Some("tool_not_found")
        );
        assert_eq!(
            result["structuredContent"]["error_taxonomy"]["amai_error_class"].as_str(),
            Some("tool_dispatch")
        );
    }

    #[test]
    fn summarize_codes_limits_preview() {
        assert_eq!(summarize_codes(&[]), "none");
        assert_eq!(summarize_codes(&["art", "amai"]), "art, amai");
        assert_eq!(
            summarize_codes(&["art", "amai", "alpha", "beta"]),
            "art, amai, alpha +1 more"
        );
    }

    #[test]
    fn summarize_namespace_modes_shows_mode_preview() {
        assert_eq!(summarize_namespace_modes(&[]), "none");
        assert_eq!(
            summarize_namespace_modes(&[
                ("continuity", "local_strict"),
                ("review", "local_plus_related")
            ]),
            "continuity=local_strict, review=local_plus_related"
        );
    }
}
