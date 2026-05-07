use super::{
    CLIENT_BUDGET_COMPACT_CHAT_COMMAND, COMPACT_CHAT_PROMPT_ARTIFACT_RELATIVE_PATH, shell_quote,
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command as ProcessCommand;

pub(super) const COMPACT_CHAT_AUTO_LAUNCH_ENV: &str = "AMAI_COMPACT_CHAT_AUTO_LAUNCH";
const COMPACT_CHAT_BRIDGE_RESULT_RELATIVE_PATH: &str =
    ".amai/continuity/compact-chat-launch-result.json";
const COMPACT_CHAT_BRIDGE_LIVE_STATE_RELATIVE_PATH: &str =
    ".amai/onboarding/vscode-public-bridge-live-state.json";
const VSCODE_AMAI_BRIDGE_URI_AUTHORITY: &str = "amai.amai-vscode-bridge";

pub(super) fn client_budget_compact_chat_notice_message() -> String {
    format!(
        "Подготовлен compact restore для переноса рабочей линии на новую чистую поверхность. Exact chat-команда: `{}`.",
        CLIENT_BUDGET_COMPACT_CHAT_COMMAND
    )
}

pub(super) fn client_budget_compact_chat_launch_notice_message() -> String {
    "Automatic clean-surface launch запрошен; проверь, что новая рабочая поверхность действительно открылась и получила startup restore, иначе используй prompt_text вручную.".to_string()
}

pub(super) fn compact_chat_manual_fallback_steps(client_surface: &Value) -> Vec<String> {
    let client_key = client_surface["client_key"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let display_name = client_surface["display_name"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Unknown client");
    let mut steps = Vec::new();
    if client_key == "codex" {
        steps.push("Codex: New Thread in Codex Sidebar (chatgpt.newChat)".to_string());
        steps.push("URI: command:chatgpt.newChat".to_string());
        steps.push("Codex: New Codex Agent (chatgpt.newCodexPanel)".to_string());
        steps.push("URI: command:chatgpt.newCodexPanel".to_string());
    } else {
        steps.push(format!(
            "Открой новую чистую рабочую поверхность в {display_name} и используй prompt_text как единственный startup prompt."
        ));
    }
    if let Some(path) = client_surface["startup_instruction_path"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let mode = client_surface["startup_instruction_mode"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unknown");
        steps.push(format!("Startup surface: {path} ({mode})"));
    }
    if let Some(summary) = client_surface["delivery_surface_assist_summary"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            client_surface["fresh_chat_assist_summary"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
    {
        steps.push(summary.to_string());
    }
    steps
}

pub(super) fn compact_chat_manual_fallback_note(client_surface: &Value) -> String {
    let display_name = client_surface["display_name"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Unknown client");
    let mut note = format!(
        "Чтобы реально уменьшить burn giant thread/context, host/client должен продолжить работу на новой чистой рабочей поверхности в {display_name} и использовать prompt_text как единственный startup prompt, если automatic launch bridge недоступен. Closest same-thread host control surface тоже известен, но он не равен переносу на отдельную clean surface."
    );
    if let Some(summary) = client_surface["delivery_surface_assist_summary"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            client_surface["fresh_chat_assist_summary"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
    {
        note.push(' ');
        note.push_str(summary);
    }
    note
}

pub(super) fn compact_chat_runtime_artifact_note() -> String {
    "Live continuity restore не ответил вовремя, поэтому использован последний startup runtime artifact. Данные могут быть устаревшими; если видите рассогласование, запустите полноценный continuity startup и повторите compact-chat."
        .to_string()
}

pub(super) fn compact_chat_prompt_artifact_path(repo_root: &Path) -> PathBuf {
    repo_root.join(COMPACT_CHAT_PROMPT_ARTIFACT_RELATIVE_PATH)
}

pub(super) fn write_compact_chat_prompt_artifact(
    repo_root: &Path,
    prompt_text: &str,
) -> Result<PathBuf> {
    let artifact_path = compact_chat_prompt_artifact_path(repo_root);
    if let Some(parent) = artifact_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&artifact_path, prompt_text)
        .with_context(|| format!("failed to write {}", artifact_path.display()))?;
    Ok(artifact_path)
}

fn resolve_vscode_code_cli_command() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    let candidates: &[&str] = if cfg!(windows) {
        &["code.cmd", "code.exe", "code"]
    } else {
        &["code"]
    };
    for dir in std::env::split_paths(&path) {
        for candidate in candidates {
            let candidate_path = dir.join(candidate);
            if candidate_path.is_file() {
                return Some((*candidate).to_string());
            }
        }
    }
    None
}

fn resolve_linux_uri_open_command() -> Option<String> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("xdg-open");
        if candidate.is_file() {
            return Some("xdg-open".to_string());
        }
    }
    None
}

fn detect_installed_vscode_amai_bridge() -> bool {
    let home = match std::env::var_os("HOME") {
        Some(value) => PathBuf::from(value),
        None => return false,
    };
    let extensions_root = home.join(".vscode/extensions");
    let Ok(entries) = fs::read_dir(extensions_root) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with("amai.amai-vscode-bridge-"))
    })
}

pub(super) fn workspace_bound_vscode_chat_profile_name(repo_root: &Path) -> String {
    let workspace_label = repo_root
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("workspace");
    let sanitized_label = workspace_label
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => ch,
            _ => '-',
        })
        .collect::<String>();
    let repo_root_display = repo_root.display().to_string();
    let path_fingerprint = Sha256::digest(repo_root_display.as_bytes());
    let fingerprint = format!("{:x}", path_fingerprint);
    format!(
        "Amai-{}-{}",
        sanitized_label,
        &fingerprint[..12.min(fingerprint.len())]
    )
}

fn compact_chat_bridge_result_path(repo_root: &Path) -> PathBuf {
    repo_root.join(COMPACT_CHAT_BRIDGE_RESULT_RELATIVE_PATH)
}

fn compact_chat_bridge_live_state_path(repo_root: &Path) -> PathBuf {
    repo_root.join(COMPACT_CHAT_BRIDGE_LIVE_STATE_RELATIVE_PATH)
}

fn expected_vscode_public_bridge_version(repo_root: &Path) -> Option<String> {
    let package_json_path = repo_root.join("tools/vscode-amai-bridge/package.json");
    let contents = fs::read_to_string(package_json_path).ok()?;
    let value = serde_json::from_str::<Value>(&contents).ok()?;
    value["version"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn detect_verified_vscode_public_bridge(repo_root: &Path) -> bool {
    let Some(expected_version) = expected_vscode_public_bridge_version(repo_root) else {
        return false;
    };
    let live_state_path = compact_chat_bridge_live_state_path(repo_root);
    let Ok(contents) = fs::read_to_string(live_state_path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<Value>(&contents) else {
        return false;
    };
    value["status"] == json!("live_launch_verified")
        && value["public_bridge"]["authority"] == json!(VSCODE_AMAI_BRIDGE_URI_AUTHORITY)
        && value["public_bridge"]["version"] == json!(expected_version)
        && value["ui_cleanup"]["success"] == json!(true)
        && value["ui_cleanup"]["uri_cleanup_requested"] == json!(true)
        && value["ui_cleanup"]["matching_tabs_after"] == json!(0)
        && value["ui_cleanup"]["active_editor_matches_bridge_uri_after"] == json!(false)
}

fn url_encode_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn build_vscode_public_bridge_uri(
    repo_root: &Path,
    prompt_path: &Path,
    result_path: &Path,
    target: &str,
) -> String {
    format!(
        "vscode://{}/open-clean-chat?prompt_file={}&result_file={}&repo_root={}&target={}&auto_submit=1",
        VSCODE_AMAI_BRIDGE_URI_AUTHORITY,
        url_encode_component(&prompt_path.display().to_string()),
        url_encode_component(&result_path.display().to_string()),
        url_encode_component(&repo_root.display().to_string()),
        url_encode_component(target),
    )
}

fn build_vscode_public_bridge_launch_command(
    vscode_binary: Option<&str>,
    uri_open_command: Option<&str>,
    repo_root: &Path,
    prompt_path: &Path,
    result_path: &Path,
    target: &str,
) -> String {
    let uri = build_vscode_public_bridge_uri(repo_root, prompt_path, result_path, target);
    if let Some(vscode_binary) = vscode_binary
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return format!(
            "cd {} && {} --open-url {}",
            shell_quote(&repo_root.display().to_string()),
            shell_quote(vscode_binary),
            shell_quote(&uri),
        );
    }
    let uri_open_command = uri_open_command
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("xdg-open");
    format!(
        "cd {} && {} {}",
        shell_quote(&repo_root.display().to_string()),
        shell_quote(uri_open_command),
        shell_quote(&uri),
    )
}

pub(super) fn build_vscode_code_chat_launch_command_with_binary(
    code_binary: &str,
    repo_root: &Path,
    prompt_path: &Path,
    new_window: bool,
) -> String {
    let profile_name = workspace_bound_vscode_chat_profile_name(repo_root);
    let window_flag = if new_window {
        "--new-window"
    } else {
        "--reuse-window"
    };
    format!(
        "cd {} && {} chat --mode agent {} --profile {} - < {}",
        shell_quote(&repo_root.display().to_string()),
        shell_quote(code_binary),
        window_flag,
        shell_quote(&profile_name),
        shell_quote(&prompt_path.display().to_string())
    )
}

pub(super) fn build_compact_chat_clean_launch_surface_with_vscode_binary(
    client_surface: &Value,
    repo_root: &Path,
    prompt_path: Option<&Path>,
    vscode_binary: Option<&str>,
) -> Value {
    build_compact_chat_clean_launch_surface_with_vscode_contracts(
        client_surface,
        repo_root,
        prompt_path,
        vscode_binary,
        resolve_linux_uri_open_command().as_deref(),
        detect_installed_vscode_amai_bridge(),
        detect_verified_vscode_public_bridge(repo_root),
    )
}

pub(super) fn build_compact_chat_clean_launch_surface_with_vscode_contracts(
    client_surface: &Value,
    repo_root: &Path,
    prompt_path: Option<&Path>,
    vscode_binary: Option<&str>,
    uri_open_command: Option<&str>,
    public_bridge_installed: bool,
    public_bridge_live_verified: bool,
) -> Value {
    let client_key = client_surface["client_key"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let display_name = client_surface["display_name"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Unknown client");
    let Some(prompt_path) = prompt_path else {
        return json!({
            "client_key": client_key,
            "display_name": display_name,
            "status": "bridge_unavailable",
            "supported_auto_launch": false,
            "command_kind": Value::Null,
            "launch_clean_chat_command": Value::Null,
            "launch_clean_chat_fallback_command": Value::Null,
            "unavailable_reason": "prompt_artifact_unavailable",
            "proof_boundary": "command_contract_only",
            "ux_verdict": "not_seamless_until_live_client_proof",
            "bridge_live_verification_state": "missing",
            "bridge_live_verification_artifact": compact_chat_bridge_live_state_path(repo_root).display().to_string(),
        });
    };
    if client_key != "vscode" {
        return json!({
            "client_key": client_key,
            "display_name": display_name,
            "status": "manual_only",
            "supported_auto_launch": false,
            "command_kind": Value::Null,
            "launch_clean_chat_command": Value::Null,
            "launch_clean_chat_fallback_command": Value::Null,
            "unavailable_reason": "client_has_no_automatic_clean_chat_bridge",
            "proof_boundary": "manual_fallback_contract_only",
            "ux_verdict": "not_seamless_until_live_client_proof",
            "bridge_live_verification_state": "missing",
            "bridge_live_verification_artifact": compact_chat_bridge_live_state_path(repo_root).display().to_string(),
        });
    }
    let Some(vscode_binary) = vscode_binary
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        if let (Some(uri_open_command), true, true) = (
            uri_open_command
                .map(str::trim)
                .filter(|value| !value.is_empty()),
            public_bridge_installed,
            public_bridge_live_verified,
        ) {
            let result_path = compact_chat_bridge_result_path(repo_root);
            return json!({
                "client_key": client_key,
                "display_name": display_name,
                "status": "launch_command_available",
                "supported_auto_launch": true,
                "command_kind": "vscode_uri_amai_bridge",
                "launch_clean_chat_command": build_vscode_public_bridge_launch_command(
                    None,
                    Some(uri_open_command),
                    repo_root,
                    prompt_path,
                    &result_path,
                    "sidebar",
                ),
                "launch_clean_chat_fallback_command": Value::Null,
                "unavailable_reason": Value::Null,
                "proof_boundary": "public_bridge_live_contract_only",
                "ux_verdict": "not_seamless_until_live_client_proof",
                "bridge_result_file": result_path.display().to_string(),
                "bridge_live_verification_state": "verified",
                "bridge_live_verification_artifact": compact_chat_bridge_live_state_path(repo_root).display().to_string(),
            });
        }
        return json!({
            "client_key": client_key,
            "display_name": display_name,
            "status": "bridge_unavailable",
            "supported_auto_launch": true,
            "command_kind": "vscode_code_chat_cli",
            "launch_clean_chat_command": Value::Null,
            "launch_clean_chat_fallback_command": Value::Null,
            "unavailable_reason": if public_bridge_installed {
                json!("vscode_public_bridge_live_verification_missing")
            } else {
                json!("vscode_code_cli_unavailable")
            },
            "proof_boundary": "command_contract_only",
            "ux_verdict": "not_seamless_until_live_client_proof",
            "bridge_live_verification_state": if public_bridge_live_verified { json!("verified") } else { json!("missing") },
            "bridge_live_verification_artifact": compact_chat_bridge_live_state_path(repo_root).display().to_string(),
        });
    };

    if let (Some(uri_open_command), true, true) = (
        uri_open_command
            .map(str::trim)
            .filter(|value| !value.is_empty()),
        public_bridge_installed,
        public_bridge_live_verified,
    ) {
        let result_path = compact_chat_bridge_result_path(repo_root);
        return json!({
            "client_key": client_key,
            "display_name": display_name,
            "status": "launch_command_available",
            "supported_auto_launch": true,
            "command_kind": "vscode_uri_amai_bridge",
            "launch_clean_chat_command": build_vscode_public_bridge_launch_command(
                Some(vscode_binary),
                Some(uri_open_command),
                repo_root,
                prompt_path,
                &result_path,
                "sidebar",
            ),
            "launch_clean_chat_fallback_command": build_vscode_code_chat_launch_command_with_binary(
                vscode_binary,
                repo_root,
                prompt_path,
                false,
            ),
            "unavailable_reason": Value::Null,
            "proof_boundary": "public_bridge_live_contract_only",
            "ux_verdict": "not_seamless_until_live_client_proof",
            "bridge_result_file": result_path.display().to_string(),
            "bridge_live_verification_state": "verified",
            "bridge_live_verification_artifact": compact_chat_bridge_live_state_path(repo_root).display().to_string(),
        });
    }

    json!({
        "client_key": client_key,
        "display_name": display_name,
        "status": "launch_command_available",
        "supported_auto_launch": true,
        "command_kind": "vscode_code_chat_cli",
        "launch_clean_chat_command": build_vscode_code_chat_launch_command_with_binary(
            vscode_binary,
            repo_root,
            prompt_path,
            true,
        ),
        "launch_clean_chat_fallback_command": build_vscode_code_chat_launch_command_with_binary(
            vscode_binary,
            repo_root,
            prompt_path,
            false,
        ),
        "unavailable_reason": Value::Null,
        "proof_boundary": "command_contract_only",
        "ux_verdict": "not_seamless_until_live_client_proof",
        "bridge_live_verification_state": if public_bridge_live_verified { json!("verified") } else { json!("missing") },
        "bridge_live_verification_artifact": compact_chat_bridge_live_state_path(repo_root).display().to_string(),
    })
}

pub(super) fn build_compact_chat_clean_launch_surface(
    client_surface: &Value,
    repo_root: &Path,
    prompt_path: Option<&Path>,
) -> Value {
    build_compact_chat_clean_launch_surface_with_vscode_binary(
        client_surface,
        repo_root,
        prompt_path,
        resolve_vscode_code_cli_command().as_deref(),
    )
}

pub(super) fn apply_compact_chat_host_launch_completion(
    payload: &mut Value,
    mode: &str,
    fallback_used: bool,
) {
    payload["continuity_compact_chat"]["host_launch"] = json!({
        "attempted": true,
        "status": "requested",
        "mode": mode,
        "fallback_used": fallback_used,
        "launch_clean_chat_command": payload["continuity_compact_chat"]["operator_notice"]["launch_clean_chat_command"].clone(),
    });
    payload["continuity_compact_chat"]["operator_notice"]["message_text"] =
        json!(client_budget_compact_chat_launch_notice_message());
    payload["continuity_compact_chat"]["operator_notice"]["note"] = if fallback_used {
        json!(
            "Clean context surface запрошена через fallback reuse-window. Проверь, что это действительно новая чистая рабочая поверхность; если нет, открой её вручную и используй prompt_text."
        )
    } else {
        json!(
            "Clean context surface запрошена через VS Code `code chat`, но bounded proof пока подтверждает только exit-zero launch command. Проверь, что действительно открылась новая чистая рабочая поверхность и что startup restore дошёл туда; если нет, открой её вручную и используй prompt_text."
        )
    };
}

fn apply_compact_chat_host_launch_state(
    payload: &mut Value,
    status: &str,
    attempted: bool,
    mode: Option<&str>,
    reason: Option<&str>,
) {
    payload["continuity_compact_chat"]["host_launch"] = json!({
        "attempted": attempted,
        "status": status,
        "mode": mode,
        "reason": reason,
    });
}

pub(super) fn apply_compact_chat_host_launch_failed(
    payload: &mut Value,
    mode: &str,
    error: &anyhow::Error,
) {
    apply_compact_chat_host_launch_state(
        payload,
        "launch_failed",
        true,
        Some(mode),
        Some(&error.to_string()),
    );
    payload["continuity_compact_chat"]["operator_notice"]["message_text"] =
        json!("Automatic clean-surface launch не сработал; manual fallback остаётся каноническим.");
    payload["continuity_compact_chat"]["operator_notice"]["required_host_action"] =
        json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable");
    payload["continuity_compact_chat"]["operator_notice"]["note"] = json!(
        "Amai попыталась открыть новую clean work surface автоматически, но host launch bridge не смог довести запуск до конца. Используйте prompt_text вручную и не считайте auto-launch выполненным."
    );
}

pub(super) fn apply_compact_chat_host_launch_disabled_by_policy(payload: &mut Value) {
    apply_compact_chat_host_launch_state(
        payload,
        "disabled_by_policy",
        false,
        None,
        Some("auto_launch_disabled_by_policy"),
    );
    payload["continuity_compact_chat"]["operator_notice"]["message_text"] = json!(
        "Automatic clean-surface launch отключён политикой; используйте prompt_text вручную."
    );
    payload["continuity_compact_chat"]["operator_notice"]["required_host_action"] =
        json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable");
    payload["continuity_compact_chat"]["operator_notice"]["note"] = json!(
        "Система может только рекомендовать новую clean work surface, но не открывать её автоматически."
    );
}

fn apply_compact_chat_host_launch_available_not_requested(payload: &mut Value) {
    apply_compact_chat_host_launch_state(
        payload,
        "available_not_requested",
        false,
        None,
        Some("launch_not_requested"),
    );
    payload["continuity_compact_chat"]["operator_notice"]["message_text"] =
        json!("Automatic clean-surface launch не запрошен; manual fallback остаётся каноническим.");
    payload["continuity_compact_chat"]["operator_notice"]["required_host_action"] =
        json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable");
    payload["continuity_compact_chat"]["operator_notice"]["note"] = json!(
        "Host launch bridge может быть доступен, но этот вызов не просил открывать новую clean work surface автоматически."
    );
}

fn apply_compact_chat_host_launch_bridge_unavailable(payload: &mut Value, reason: &str) {
    apply_compact_chat_host_launch_state(payload, "bridge_unavailable", false, None, Some(reason));
    payload["continuity_compact_chat"]["operator_notice"]["message_text"] =
        json!("Automatic clean-surface launch недоступен; manual fallback остаётся каноническим.");
    payload["continuity_compact_chat"]["operator_notice"]["required_host_action"] =
        json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable");
    payload["continuity_compact_chat"]["operator_notice"]["note"] = json!(
        "Amai не нашла host launch command для новой clean work surface. Используйте prompt_text вручную."
    );
}

fn compact_chat_auto_launch_enabled() -> bool {
    std::env::var(COMPACT_CHAT_AUTO_LAUNCH_ENV)
        .ok()
        .map(|value| matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn compact_chat_launch_clean_chat_command(payload: &Value) -> Option<&str> {
    payload["continuity_compact_chat"]["operator_notice"]["launch_clean_chat_command"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn compact_chat_launch_unavailable_reason(payload: &Value) -> String {
    payload["continuity_compact_chat"]["operator_notice"]["clean_chat_launch"]["unavailable_reason"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("launch_command_unavailable")
        .to_string()
}

async fn execute_compact_chat_launch_command(command: &str) -> Result<()> {
    let mut process = if cfg!(windows) {
        let mut cmd = ProcessCommand::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    } else {
        let mut cmd = ProcessCommand::new("sh");
        cmd.arg("-lc").arg(command);
        cmd
    };
    let output = tokio::time::timeout(Duration::from_secs(10), process.output())
        .await
        .context("timed out waiting for clean-surface launch command")?
        .context("failed to run clean-surface launch command")?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let status = output
        .status
        .code()
        .map(|value| value.to_string())
        .unwrap_or_else(|| "terminated-by-signal".to_string());
    if stderr.is_empty() {
        anyhow::bail!("clean-surface launch command exited with status {status}");
    }
    anyhow::bail!("clean-surface launch command exited with status {status}: {stderr}");
}

pub(crate) async fn maybe_launch_compact_chat_host(
    payload: &mut Value,
    launch_requested: bool,
    implicit_launch_allowed: bool,
) -> Result<bool> {
    maybe_launch_compact_chat_host_with_auto_launch_policy(
        payload,
        launch_requested,
        implicit_launch_allowed,
        compact_chat_auto_launch_enabled(),
    )
    .await
}

pub(super) async fn maybe_launch_compact_chat_host_with_auto_launch_policy(
    payload: &mut Value,
    launch_requested: bool,
    implicit_launch_allowed: bool,
    auto_launch_enabled: bool,
) -> Result<bool> {
    if !launch_requested && !implicit_launch_allowed {
        apply_compact_chat_host_launch_available_not_requested(payload);
        return Ok(false);
    }

    let Some(command) = compact_chat_launch_clean_chat_command(payload).map(str::to_owned) else {
        let reason = compact_chat_launch_unavailable_reason(payload);
        apply_compact_chat_host_launch_bridge_unavailable(payload, &reason);
        return Ok(false);
    };

    if !auto_launch_enabled {
        apply_compact_chat_host_launch_disabled_by_policy(payload);
        return Ok(false);
    }

    match execute_compact_chat_launch_command(&command).await {
        Ok(()) => {
            apply_compact_chat_host_launch_completion(payload, "implicit_default", false);
            Ok(true)
        }
        Err(error) => {
            apply_compact_chat_host_launch_failed(payload, "implicit_default", &error);
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_verified_vscode_public_bridge_rejects_stale_or_invalid_marker() {
        let repo_root = std::env::temp_dir().join(format!(
            "amai-vscode-bridge-live-state-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("epoch")
                .as_nanos()
        ));
        let state_path = compact_chat_bridge_live_state_path(&repo_root);
        fs::create_dir_all(state_path.parent().expect("parent")).expect("mkdir");
        let package_json_path = repo_root.join("tools/vscode-amai-bridge/package.json");
        fs::create_dir_all(package_json_path.parent().expect("package parent")).expect("mkdir");
        fs::write(
            &package_json_path,
            r#"{
  "version": "0.0.2"
}"#,
        )
        .expect("write package json");

        fs::write(
            &state_path,
            r#"{
  "status": "launch_started",
  "public_bridge": { "authority": "amai.amai-vscode-bridge" }
}"#,
        )
        .expect("write stale state");
        assert!(!detect_verified_vscode_public_bridge(&repo_root));

        fs::write(
            &state_path,
            r#"{
  "status": "live_launch_verified",
  "public_bridge": { "authority": "wrong.authority" }
}"#,
        )
        .expect("write wrong authority state");
        assert!(!detect_verified_vscode_public_bridge(&repo_root));

        fs::write(
            &state_path,
            r#"{
  "status": "live_launch_verified",
  "public_bridge": { "authority": "amai.amai-vscode-bridge" }
}"#,
        )
        .expect("write verified state");
        assert!(!detect_verified_vscode_public_bridge(&repo_root));

        let expected_version =
            expected_vscode_public_bridge_version(&repo_root).expect("expected bridge version");

        fs::write(
            &state_path,
            r#"{
  "status": "live_launch_verified",
  "public_bridge": { "authority": "amai.amai-vscode-bridge" },
  "ui_cleanup": {
    "success": false,
    "uri_cleanup_requested": true,
    "matching_tabs_after": 1,
    "active_editor_matches_bridge_uri_after": true
  }
}"#,
        )
        .expect("write dirty ui cleanup state");
        assert!(!detect_verified_vscode_public_bridge(&repo_root));

        fs::write(
            &state_path,
            r#"{
  "status": "live_launch_verified",
  "public_bridge": { "authority": "amai.amai-vscode-bridge", "version": "0.0.0-stale" },
  "ui_cleanup": {
    "success": true,
    "uri_cleanup_requested": true,
    "matching_tabs_after": 0,
    "active_editor_matches_bridge_uri_after": false
  }
}"#,
        )
        .expect("write wrong version state");
        assert!(!detect_verified_vscode_public_bridge(&repo_root));

        fs::write(
            &state_path,
            &format!(
                r#"{{
  "status": "live_launch_verified",
  "public_bridge": {{ "authority": "amai.amai-vscode-bridge", "version": "{}" }},
  "ui_cleanup": {{
    "success": true,
    "uri_cleanup_requested": true,
    "matching_tabs_after": 0,
    "active_editor_matches_bridge_uri_after": false
  }}
}}"#,
                expected_version
            ),
        )
        .expect("write verified state");
        assert!(detect_verified_vscode_public_bridge(&repo_root));

        fs::remove_dir_all(&repo_root).expect("cleanup");
    }

    #[test]
    fn compact_chat_manual_fallback_steps_preserve_codex_specific_actions() {
        let steps = compact_chat_manual_fallback_steps(&json!({
            "client_key": "codex",
            "display_name": "Codex",
            "startup_instruction_path": "/repo/AGENTS.md",
            "startup_instruction_mode": "managed_append_block",
                "fresh_chat_assist_summary": "Для front-door новой чистой рабочей поверхности используй ./scripts/reconnect_local.sh --client codex или ./scripts/amai_exec.sh bootstrap reconnect --client codex --yes."
        }));

        assert!(steps.iter().any(|value| value.contains("chatgpt.newChat")));
        assert!(
            steps
                .iter()
                .any(|value| value.contains("chatgpt.newCodexPanel"))
        );
        assert!(steps.iter().any(|value| value.contains("/repo/AGENTS.md")));
        assert!(
            steps
                .iter()
                .any(|value| value.contains("bootstrap reconnect --client codex"))
        );
    }

    #[test]
    fn compact_chat_manual_fallback_steps_surface_generic_client_context() {
        let steps = compact_chat_manual_fallback_steps(&json!({
            "client_key": "generic",
            "display_name": "Generic",
            "startup_instruction_path": "/repo/tmp/onboarding/generic-amai-startup.md",
            "startup_instruction_mode": "manual_snippet_only",
                "fresh_chat_assist_summary": "Для front-door новой чистой рабочей поверхности используй ./scripts/reconnect_local.sh --client generic или ./scripts/amai_exec.sh bootstrap reconnect --client generic --yes."
        }));

        assert!(
            steps
                .iter()
                .any(|value| value.contains("новую чистую рабочую поверхность в Generic"))
        );
        assert!(
            steps
                .iter()
                .any(|value| value.contains("generic-amai-startup.md"))
        );
        assert!(
            steps
                .iter()
                .any(|value| value.contains("bootstrap reconnect --client generic"))
        );
    }

    #[test]
    fn compact_chat_manual_fallback_note_fail_closes_to_unknown_client() {
        let note = compact_chat_manual_fallback_note(&json!({}));
        assert!(note.contains("Unknown client"));
        assert!(note.contains("prompt_text"));
    }
}
