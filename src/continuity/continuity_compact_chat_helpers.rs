use super::{
    CLIENT_BUDGET_COMPACT_CHAT_COMMAND, COMPACT_CHAT_PROMPT_ARTIFACT_RELATIVE_PATH, shell_quote,
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

pub(super) fn client_budget_compact_chat_notice_message() -> String {
    format!(
        "Подготовлен compact restore для huge-chat rebase. Exact chat-команда: `{}`.",
        CLIENT_BUDGET_COMPACT_CHAT_COMMAND
    )
}

pub(super) fn client_budget_compact_chat_launch_notice_message() -> String {
    "Clean chat surface запрошен автоматически; fresh CHAT_START_RESTORE уже передан в новый чат."
        .to_string()
}

pub(super) fn compact_chat_manual_fallback_note() -> String {
    "Чтобы реально уменьшить burn giant thread/context, host/client должен продолжить работу на чистом context surface и использовать prompt_text как единственный startup prompt, если automatic launch bridge недоступен. Closest same-thread host control surface тоже известен, но он не равен clean-surface rebase. Для ручного открытия используйте команды Codex: New Thread in Codex Sidebar (chatgpt.newChat) или Codex: New Codex Agent (chatgpt.newCodexPanel).".to_string()
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

pub(super) fn build_vscode_code_chat_launch_command(
    repo_root: &Path,
    prompt_path: &Path,
    new_window: bool,
) -> Option<String> {
    let code_binary = resolve_vscode_code_cli_command()?;
    Some(build_vscode_code_chat_launch_command_with_binary(
        &code_binary,
        repo_root,
        prompt_path,
        new_window,
    ))
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
    payload["continuity_compact_chat"]["operator_notice"]["required_host_action"] = Value::Null;
    payload["continuity_compact_chat"]["operator_notice"]["note"] = if fallback_used {
        json!(
            "Clean context surface запрошен через fallback reuse-window. Проверь, что это действительно новый чистый чат; если нет, открой новый чат вручную и используй prompt_text."
        )
    } else {
        json!(
            "Clean context surface уже запрошен и fresh prompt_text передан через VS Code `code chat`; отдельный ручной injection шаг больше не требуется."
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
        json!("Automatic clean chat launch не сработал; manual fallback остаётся каноническим.");
    payload["continuity_compact_chat"]["operator_notice"]["required_host_action"] =
        json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable");
    payload["continuity_compact_chat"]["operator_notice"]["note"] = json!(
        "Amai попыталась открыть clean context surface автоматически, но host launch bridge не смог довести запуск до конца. Используйте prompt_text вручную и не считайте auto-launch выполненным."
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
    payload["continuity_compact_chat"]["operator_notice"]["message_text"] =
        json!("Automatic clean chat launch отключён политикой; используйте prompt_text вручную.");
    payload["continuity_compact_chat"]["operator_notice"]["required_host_action"] =
        json!("open_clean_chat_surface_and_inject_prompt_text_if_launch_bridge_unavailable");
    payload["continuity_compact_chat"]["operator_notice"]["note"] = json!(
        "Система может только рекомендовать новый clean-surface чат, но не открывать его автоматически."
    );
}

pub(crate) async fn maybe_launch_compact_chat_host(
    payload: &mut Value,
    _launch_requested: bool,
    _implicit_launch_allowed: bool,
) -> Result<bool> {
    apply_compact_chat_host_launch_disabled_by_policy(payload);
    Ok(false)
}
