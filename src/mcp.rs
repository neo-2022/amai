use crate::cli::{ContextPackArgs, McpConfigArgs, VerifyMcpArgs, VerifyTokenBenchmarkArgs};
use crate::{compatibility, config, observe, postgres, retrieval, token_budget, verify};
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

use crate::config::AppConfig;

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";
const SERVER_NAME: &str = "Art-memory-agent-index";

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
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {
                        "code": -32700,
                        "message": "invalid JSON-RPC payload",
                        "data": error.to_string(),
                    }
                });
                write_message(&mut writer, &response).await?;
                continue;
            }
        };

        if incoming.get("id").is_none() {
            continue;
        }

        let response = match handle_request(cfg, incoming).await {
            Ok(response) => response,
            Err(error) => json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": {
                    "code": -32000,
                    "message": "MCP request handler failed",
                    "data": error.to_string(),
                }
            }),
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
    session
        .notify("notifications/initialized", json!({}))
        .await
        .context("failed to send MCP initialized notification")?;

    let tools = session.request("tools/list", json!({})).await?;
    let tool_names = tools["tools"]
        .as_array()
        .ok_or_else(|| anyhow!("tools/list returned invalid tools array"))?
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(ToOwned::to_owned))
        .collect::<BTreeSet<_>>();
    let expected_tools = BTreeSet::from([
        "amai_list_projects".to_string(),
        "amai_list_namespaces".to_string(),
        "amai_context_pack".to_string(),
        "amai_token_benchmark".to_string(),
        "amai_token_report".to_string(),
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
                "persist": true,
            }),
        )
        .await
        .context("MCP amai_context_pack failed")?;
    let visible_projects = context_pack["context_pack"]["visible_projects"]
        .as_array()
        .ok_or_else(|| anyhow!("MCP context pack returned invalid visible_projects array"))?;
    if !visible_projects
        .iter()
        .any(|item| item["project_code"].as_str() == Some(args.context.project.as_str()))
    {
        return Err(anyhow!(
            "MCP context pack lost primary project {}",
            args.context.project
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
    if critical != 0 || unknown != 0 {
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

    let result = json!({
        "mcp_verification": {
            "protocol_version": MCP_PROTOCOL_VERSION,
            "tools": tool_names,
            "prompts": prompt_names,
            "token_savings_factor": savings_factor,
            "token_savings_percent": savings_percent,
            "token_report_session_events": session_events,
            "critical": critical,
            "unknown": unknown,
        }
    });
    println!("{}", serde_json::to_string_pretty(&result)?);

    session.shutdown().await?;
    Ok(())
}

struct McpProofSession {
    child: Child,
    stdin: ChildStdin,
    stdout: tokio::io::Lines<BufReader<ChildStdout>>,
    next_id: u64,
}

impl McpProofSession {
    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
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

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        write_message(&mut self.stdin, &notification).await
    }

    async fn tool_call(&mut self, name: &str, arguments: Value) -> Result<Value> {
        let result = self
            .request(
                "tools/call",
                json!({
                    "name": name,
                    "arguments": arguments,
                }),
            )
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

    async fn shutdown(mut self) -> Result<()> {
        self.child
            .kill()
            .await
            .context("failed to terminate MCP proof server")?;
        Ok(())
    }
}

async fn handle_request(cfg: &AppConfig, incoming: Value) -> Result<Value> {
    let id = incoming["id"].clone();
    let method = incoming["method"]
        .as_str()
        .ok_or_else(|| anyhow!("JSON-RPC request is missing method"))?;
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
            "result": prompt_result(params)?,
        }),
        "tools/call" => {
            let request: ToolCallRequest =
                serde_json::from_value(params).context("failed to decode tool call request")?;
            let result = match handle_tool_call(cfg, request).await {
                Ok(result) => result,
                Err(error) => json!({
                    "content": [{
                        "type": "text",
                        "text": format!("tool failed: {error:#}")
                    }],
                    "isError": true
                }),
            };
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            })
        }
        other => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": -32601,
                "message": "method not found",
                "data": other,
            }
        }),
    };
    Ok(response)
}

async fn handle_tool_call(cfg: &AppConfig, request: ToolCallRequest) -> Result<Value> {
    match request.name.as_str() {
        "amai_list_projects" => tool_list_projects(cfg).await,
        "amai_list_namespaces" => {
            let args: ListNamespacesArgs = parse_arguments(request.arguments)?;
            tool_list_namespaces(cfg, args).await
        }
        "amai_context_pack" => {
            let args: ContextPackToolArgs = parse_arguments(request.arguments)?;
            tool_context_pack(cfg, args).await
        }
        "amai_token_benchmark" => {
            let args: TokenBenchmarkToolArgs = parse_arguments(request.arguments)?;
            tool_token_benchmark(cfg, args).await
        }
        "amai_token_report" => {
            let args: TokenReportToolArgs = parse_arguments(request.arguments)?;
            tool_token_report(cfg, args).await
        }
        "amai_observe_snapshot" => tool_observe_snapshot(cfg).await,
        "amai_warm_cache" => {
            let args: WarmCacheToolArgs = parse_arguments(request.arguments)?;
            tool_warm_cache(cfg, args).await
        }
        other => Err(anyhow!("unknown MCP tool: {other}")),
    }
}

async fn tool_list_projects(cfg: &AppConfig) -> Result<Value> {
    let db = postgres::connect_admin(cfg).await?;
    let projects = postgres::list_projects(&db).await?;
    let structured = json!({
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
            "registered projects: {}",
            structured["projects"].as_array().map_or(0, Vec::len)
        ),
        structured,
    ))
}

async fn tool_list_namespaces(cfg: &AppConfig, args: ListNamespacesArgs) -> Result<Value> {
    let db = postgres::connect_admin(cfg).await?;
    let project = postgres::get_project_by_code(&db, &args.project).await?;
    let namespaces = postgres::list_namespaces_for_project(&db, project.project_id).await?;
    let structured = json!({
        "project": {
            "code": project.code,
            "display_name": project.display_name,
            "repo_root": project.repo_root,
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
            "namespaces for {}: {}",
            args.project,
            structured["namespaces"].as_array().map_or(0, Vec::len)
        ),
        structured,
    ))
}

async fn tool_context_pack(cfg: &AppConfig, args: ContextPackToolArgs) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let mut db = postgres::connect_admin(cfg).await?;
    let context = args.to_context_args();
    let result =
        retrieval::execute_context_pack_capture(cfg, &mut db, &context, args.persist).await?;
    let structured = json!({
        "context_pack": result.payload,
        "stats": {
            "context_pack_id": result.stats.context_pack_id,
            "exact_documents": result.stats.exact_documents,
            "symbol_hits": result.stats.symbol_hits,
            "lexical_chunks": result.stats.lexical_chunks,
            "semantic_chunks": result.stats.semantic_chunks,
            "cache_hit": result.stats.cache_hit,
            "scope_signature": result.stats.scope_signature,
            "timings_ms": {
                "resolve_scope_ms": result.stats.timings.resolve_scope_ms,
                "cache_lookup_ms": result.stats.timings.cache_lookup_ms,
                "exact_lookup_ms": result.stats.timings.exact_lookup_ms,
                "symbol_lookup_ms": result.stats.timings.symbol_lookup_ms,
                "lexical_lookup_ms": result.stats.timings.lexical_lookup_ms,
                "query_embed_ms": result.stats.timings.query_embed_ms,
                "semantic_search_ms": result.stats.timings.semantic_search_ms,
                "semantic_hydrate_ms": result.stats.timings.semantic_hydrate_ms,
                "serialize_ms": result.stats.timings.serialize_ms,
                "persist_ms": result.stats.timings.persist_ms,
            }
        }
    });
    let summary = format!(
        "context pack built for {}:{} :: docs={} symbols={} lexical={} semantic={} cache_hit={}",
        context.project,
        context.namespace,
        result.stats.exact_documents,
        result.stats.symbol_hits,
        result.stats.lexical_chunks,
        result.stats.semantic_chunks,
        result.stats.cache_hit,
    );
    Ok(tool_result(summary, structured))
}

async fn tool_token_benchmark(cfg: &AppConfig, args: TokenBenchmarkToolArgs) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let mut db = postgres::connect_admin(cfg).await?;
    let payload = verify::collect_token_benchmark(cfg, &mut db, &args.to_verify_args()).await?;
    let savings = &payload["token_benchmark"]["savings"];
    let summary = format!(
        "token benchmark :: saved_tokens={} savings_factor={:.3} savings_percent={:.3}",
        savings["saved_tokens"].as_u64().unwrap_or_default(),
        savings["savings_factor"].as_f64().unwrap_or_default(),
        savings["savings_percent"].as_f64().unwrap_or_default(),
    );
    Ok(tool_result(summary, payload))
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
    let headline = &payload["token_budget_report"]["headline"];
    let summary = format!(
        "token report :: metric={} value_percent={:.3} saved_tokens={} events={}",
        headline["metric_code"].as_str().unwrap_or("unknown"),
        headline["value_percent"].as_f64().unwrap_or_default(),
        headline["saved_tokens"].as_i64().unwrap_or_default(),
        headline["events_count"].as_u64().unwrap_or_default(),
    );
    Ok(tool_result(summary, payload))
}

async fn tool_observe_snapshot(cfg: &AppConfig) -> Result<Value> {
    compatibility::assert_supported(cfg).await?;
    let snapshot = observe::collect_snapshot_preview(cfg).await?;
    let summary = &snapshot["sla"]["summary"];
    let text = format!(
        "observe snapshot :: pass={} alert={} critical={} unknown={}",
        summary["pass"].as_u64().unwrap_or_default(),
        summary["alert"].as_u64().unwrap_or_default(),
        summary["critical"].as_u64().unwrap_or_default(),
        summary["unknown"].as_u64().unwrap_or_default(),
    );
    Ok(tool_result(text, json!({ "snapshot": snapshot })))
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
    let structured = json!({
        "warmup_cache": {
            "projects": args.projects,
            "namespace": args.namespace,
            "query": args.query,
            "retrieval_mode": args.retrieval_mode,
            "warmed": warmed,
        }
    });
    Ok(tool_result(
        format!(
            "warmup finished for {} project(s)",
            structured["warmup_cache"]["warmed"]
                .as_array()
                .map_or(0, Vec::len)
        ),
        structured,
    ))
}

fn server_instructions() -> String {
    [
        "Amai is a project-scoped continuity and retrieval server for AI agents.",
        "Default law: keep projects isolated and prefer local_strict unless a related-project policy is explicitly required.",
        "Use amai_list_projects first when you do not know what is registered.",
        "Use amai_list_namespaces before querying an unfamiliar project.",
        "Use amai_context_pack for retrieval instead of asking for whole repositories.",
        "Use amai_token_benchmark when you need a measured token-economy comparison.",
        "Use amai_token_report when you need cumulative token savings for the current session, budget window, or lifetime.",
        "Use amai_observe_snapshot when you need live stack health and SLA evidence.",
    ]
    .join(" ")
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

fn prompt_result(params: Value) -> Result<Value> {
    let name = params["name"]
        .as_str()
        .ok_or_else(|| anyhow!("prompts/get requires a prompt name"))?;
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
        other => return Err(anyhow!("unknown MCP prompt: {other}")),
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

fn parse_arguments<T>(value: Option<Value>) -> Result<T>
where
    T: DeserializeOwned + Default,
{
    match value {
        Some(value) => serde_json::from_value(value).context("failed to decode tool arguments"),
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

fn required_prompt_arg(arguments: &serde_json::Map<String, Value>, key: &str) -> Result<String> {
    arguments
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .ok_or_else(|| anyhow!("prompt argument is required: {key}"))
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
    #[serde(default = "default_true")]
    persist: bool,
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

fn default_true() -> bool {
    true
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
    use super::{McpConfigArgs, render_client_config};
    use std::path::PathBuf;

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
}
