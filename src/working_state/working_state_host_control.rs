use crate::codex_threads;
use serde_json::{Value, json};

use super::current_thread_id;
use super::working_state_shell_commands::{
    can_use_workspace_continuity_defaults, shell_join_command,
};
use super::{
    HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID,
    HOST_CURRENT_THREAD_COMPACT_WINDOW_HOST_SURFACE_KIND, HOST_CURRENT_THREAD_COMPACT_WINDOW_KIND,
    HOST_CURRENT_THREAD_COMPACT_WINDOW_ROUTE_PREFIX, HOST_CURRENT_THREAD_CONTROL_COMMAND_ID,
    HOST_CURRENT_THREAD_CONTROL_EXTERNAL_LAUNCH_KIND, HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED,
    HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED, HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED,
    HOST_CURRENT_THREAD_CONTROL_HOST_SURFACE_KIND, HOST_CURRENT_THREAD_CONTROL_KIND,
    HOST_CURRENT_THREAD_CONTROL_OBSERVE_API_LAUNCH_PATH, HOST_CURRENT_THREAD_CONTROL_ROUTE_PREFIX,
    HOST_CURRENT_THREAD_CONTROL_URI_AUTHORITY, HOST_CURRENT_THREAD_CONTROL_URI_SCHEME,
    HostContextCompactionStage,
};

pub(crate) fn normalize_host_current_thread_control_command_id(
    value: Option<&str>,
) -> &'static str {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some(HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID) => {
            HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID
        }
        _ => HOST_CURRENT_THREAD_CONTROL_COMMAND_ID,
    }
}

pub(super) fn host_current_thread_control_kind_for_command_id(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => HOST_CURRENT_THREAD_COMPACT_WINDOW_KIND,
        _ => HOST_CURRENT_THREAD_CONTROL_KIND,
    }
}

fn host_current_thread_control_host_surface_kind_for_command_id(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            HOST_CURRENT_THREAD_COMPACT_WINDOW_HOST_SURFACE_KIND
        }
        _ => HOST_CURRENT_THREAD_CONTROL_HOST_SURFACE_KIND,
    }
}

fn host_current_thread_control_route_prefix_for_command_id(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            HOST_CURRENT_THREAD_COMPACT_WINDOW_ROUTE_PREFIX
        }
        _ => HOST_CURRENT_THREAD_CONTROL_ROUTE_PREFIX,
    }
}

fn host_current_thread_control_button_label(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => "Open compact window",
        _ => "Open thread overlay",
    }
}

fn host_current_thread_control_summary(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Открыть current thread на compact-window route внутри Codex host-клиента."
        }
        _ => "Открыть overlay текущего thread внутри Codex host-клиента.",
    }
}

fn host_current_thread_control_note(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Это same-thread compact-window surface внутри Codex host-клиента. В electron этот route относится к popout/compact-window contour; в VS Code onUri path может открыть тот же compact-window renderer route внутри клиента, а не гарантированный отдельный window. Surface полезен для проверки, режет ли compact-window host overhead лучше overlay, но он всё ещё не равен clean-surface rebase."
        }
        _ => {
            "Это current-thread control surface внутри Codex host-клиента. Он полезен для same-thread operator control, но не равен clean-surface rebase. Public VS Code command palette path пока не доказан, однако при наличии thread_id materialized URI launch и server-side xdg-open path через Amai observe host."
        }
    }
}

fn host_current_thread_control_intro_message(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Открыть same-thread compact-window route текущего giant thread и проверить, снижает ли он host-side overhead лучше overlay."
        }
        _ => "Открыть same-thread overlay текущего giant thread через host surface.",
    }
}

fn host_current_thread_control_requested_message(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Запрошено открытие same-thread compact window текущего giant thread."
        }
        _ => "Запрошено открытие same-thread overlay текущего giant thread.",
    }
}

fn host_current_thread_control_feedback_ack_intro(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "После попытки запуска подтверди, открылся ли compact window. Это попадёт в Amai continuity."
        }
        _ => "После попытки запуска подтверди, открылся ли overlay. Это попадёт в Amai continuity.",
    }
}

fn host_current_thread_control_external_summary(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Попробовать открыть current thread на compact-window route через VS Code URI handler."
        }
        _ => "Попробовать открыть current thread overlay через VS Code URI handler.",
    }
}

fn host_current_thread_control_external_note(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Этот path опирается на openai.chatgpt onUri -> navigateToRoute(path) и webview route /hotkey-window/thread/:conversationId. В electron это compact-window/popout contour; в VS Code расширении route может открыться как compact-window renderer category без гарантии отдельного окна. Локальный Amai observe host умеет запускать xdg-open для этого URI, но end-to-end эффект всё ещё подтверждается отдельным feedback."
        }
        _ => {
            "Этот path опирается на openai.chatgpt onUri -> navigateToRoute(path) и webview route /thread-overlay/:conversationId. Он truthfully best-effort: route и handler materialized; локальный Amai observe host теперь умеет запускать xdg-open для этого URI, но end-to-end open всё ещё подтверждается отдельным feedback."
        }
    }
}

fn host_current_thread_control_observe_api_launch_summary(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            "Запустить current thread compact-window route через локальный Amai observe host."
        }
        _ => "Запустить current thread overlay через локальный Amai observe host.",
    }
}

fn host_current_thread_control_subject(command_id: &str) -> &'static str {
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => "same-thread compact window",
        _ => "same-thread overlay",
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_host_current_thread_control_surface() -> Value {
    build_host_current_thread_control_surface_for_stage(HostContextCompactionStage::Inactive)
}

fn preferred_host_current_thread_control_command_id_for_stage(
    stage: HostContextCompactionStage,
) -> &'static str {
    if stage.preserve_active() {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID
    } else {
        HOST_CURRENT_THREAD_CONTROL_COMMAND_ID
    }
}

fn alternate_host_current_thread_control_command_id_for_stage(
    stage: HostContextCompactionStage,
) -> &'static str {
    if preferred_host_current_thread_control_command_id_for_stage(stage)
        == HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID
    {
        HOST_CURRENT_THREAD_CONTROL_COMMAND_ID
    } else {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID
    }
}

fn host_current_thread_control_selection_reason(
    stage: HostContextCompactionStage,
    primary_command_id: &str,
) -> &'static str {
    if stage.preserve_active() {
        if normalize_host_current_thread_control_command_id(Some(primary_command_id))
            == HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID
        {
            "protect_recent_host_compaction_gain"
        } else if stage.critical_regrowth_active() {
            "critical_regrowth_try_overlay"
        } else {
            "compact_window_failed_try_overlay"
        }
    } else {
        "default_overlay_first"
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_host_current_thread_control_surface_for_stage(
    stage: HostContextCompactionStage,
) -> Value {
    let current_thread = current_thread_id();
    build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
        current_thread.as_deref(),
        stage,
        None,
    )
}

#[allow(dead_code)]
pub(crate) fn build_host_current_thread_control_surface_for_stage_and_primary_command(
    stage: HostContextCompactionStage,
    primary_command_id: Option<&str>,
) -> Value {
    let current_thread = current_thread_id();
    build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
        current_thread.as_deref(),
        stage,
        primary_command_id,
    )
}

fn build_host_current_thread_control_route_path_for_command(
    thread_id: &str,
    command_id: &str,
) -> String {
    format!(
        "{}/{}",
        host_current_thread_control_route_prefix_for_command_id(command_id),
        thread_id
    )
}

fn build_host_current_thread_control_uri(route_path: &str) -> String {
    format!(
        "{HOST_CURRENT_THREAD_CONTROL_URI_SCHEME}://{HOST_CURRENT_THREAD_CONTROL_URI_AUTHORITY}{route_path}"
    )
}

fn build_host_current_thread_control_launch_command(uri: &str) -> Option<String> {
    if cfg!(target_os = "linux") {
        Some(shell_join_command(&["xdg-open", uri]))
    } else {
        None
    }
}

pub(super) fn build_host_current_thread_control_observe_launch_command(
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    repo_root: Option<&str>,
    surface: &Value,
) -> Option<String> {
    let thread_id = surface["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let command_id = surface["command_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let mut base_args = vec!["amai", "observe", "ctl-launch", "--thread-id", thread_id];
    match normalize_host_current_thread_control_command_id(Some(command_id)) {
        HOST_CURRENT_THREAD_COMPACT_WINDOW_COMMAND_ID => {
            base_args.push("--compact-window");
        }
        HOST_CURRENT_THREAD_CONTROL_COMMAND_ID => {}
        _ => {
            base_args.push("--command-id");
            base_args.push(command_id);
        }
    }
    if can_use_workspace_continuity_defaults(namespace_code, repo_root) {
        return Some(shell_join_command(&base_args));
    }
    let project_code = project_code.filter(|value| !value.is_empty())?;
    let namespace_code = namespace_code.filter(|value| !value.is_empty())?;
    let repo_root = repo_root.filter(|value| !value.is_empty())?;
    base_args.extend_from_slice(&[
        "--project",
        project_code,
        "--namespace",
        namespace_code,
        "--repo-root",
        repo_root,
    ]);
    Some(shell_join_command(&base_args))
}

pub(crate) fn build_host_current_thread_control_surface_for_thread(
    thread_id: Option<&str>,
) -> Value {
    build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
        thread_id,
        HostContextCompactionStage::Inactive,
        None,
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn build_host_current_thread_control_surface_for_thread_and_stage(
    thread_id: Option<&str>,
    stage: HostContextCompactionStage,
) -> Value {
    build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
        thread_id, stage, None,
    )
}

pub(crate) fn build_host_current_thread_control_surface_for_thread_and_stage_with_primary_command(
    thread_id: Option<&str>,
    stage: HostContextCompactionStage,
    primary_command_id: Option<&str>,
) -> Value {
    let primary_command_id = primary_command_id
        .map(|value| normalize_host_current_thread_control_command_id(Some(value)))
        .unwrap_or_else(|| preferred_host_current_thread_control_command_id_for_stage(stage));
    let alternate_command_id = alternate_host_current_thread_control_command_id_for_stage(stage);
    let mut primary = build_host_current_thread_control_surface_for_thread_and_command(
        thread_id,
        Some(primary_command_id),
    );
    if let Some(root) = primary.as_object_mut() {
        root.insert(
            "host_context_compaction_stage".to_string(),
            json!(stage.as_str()),
        );
        root.insert(
            "selection_reason".to_string(),
            json!(host_current_thread_control_selection_reason(
                stage,
                primary_command_id,
            )),
        );
        root.insert(
            "alternate_controls".to_string(),
            Value::Array(vec![
                build_host_current_thread_control_surface_for_thread_and_command(
                    thread_id,
                    Some(if alternate_command_id == primary_command_id {
                        alternate_host_current_thread_control_command_id_for_stage(
                            HostContextCompactionStage::Inactive,
                        )
                    } else {
                        alternate_command_id
                    }),
                ),
            ]),
        );
    }
    primary
}

pub(crate) fn build_host_current_thread_control_surface_for_thread_and_command(
    thread_id: Option<&str>,
    command_id: Option<&str>,
) -> Value {
    let command_id = normalize_host_current_thread_control_command_id(command_id);
    let thread_id = thread_id.map(str::trim).filter(|value| !value.is_empty());
    let route_path = thread_id.map(|thread_id| {
        build_host_current_thread_control_route_path_for_command(thread_id, command_id)
    });
    let uri = route_path
        .as_deref()
        .map(build_host_current_thread_control_uri);
    let launch_command = uri
        .as_deref()
        .and_then(build_host_current_thread_control_launch_command);
    let observe_api_launch_available = launch_command.is_some();
    json!({
        "available": true,
        "control_kind": host_current_thread_control_kind_for_command_id(command_id),
        "host_surface_kind": host_current_thread_control_host_surface_kind_for_command_id(command_id),
        "command_id": command_id,
        "button_label": host_current_thread_control_button_label(command_id),
        "intro_message": host_current_thread_control_intro_message(command_id),
        "requested_message_text": host_current_thread_control_requested_message(command_id),
        "feedback_ack_intro": host_current_thread_control_feedback_ack_intro(command_id),
        "thread_id": thread_id,
        "same_thread_surface": true,
        "automation_ready": observe_api_launch_available,
        "requires_host_bridge": true,
        "snapshot_seeded_before_open": true,
        "resume_if_needed_before_snapshot": true,
        "external_uri_launch": {
            "available": uri.is_some(),
            "launch_surface_kind": HOST_CURRENT_THREAD_CONTROL_EXTERNAL_LAUNCH_KIND,
            "best_effort": true,
            "observe_api_launch_available": observe_api_launch_available,
            "observe_api_launch_path": if observe_api_launch_available {
                Some(HOST_CURRENT_THREAD_CONTROL_OBSERVE_API_LAUNCH_PATH)
            } else {
                None::<&str>
            },
            "observe_api_launch_summary": if observe_api_launch_available {
                Some(host_current_thread_control_observe_api_launch_summary(command_id))
            } else {
                None::<&str>
            },
            "verification_state": if uri.is_some() && observe_api_launch_available {
                "route_resolved_launch_command_available"
            } else if uri.is_some() {
                "route_resolved_not_executed"
            } else {
                "missing_thread_id"
            },
            "uri_scheme": HOST_CURRENT_THREAD_CONTROL_URI_SCHEME,
            "uri_authority": HOST_CURRENT_THREAD_CONTROL_URI_AUTHORITY,
            "route_path": route_path,
            "uri": uri,
            "platform_launch_command": launch_command,
            "summary": host_current_thread_control_external_summary(command_id),
            "note": host_current_thread_control_external_note(command_id)
        },
        "summary": host_current_thread_control_summary(command_id),
        "note": host_current_thread_control_note(command_id)
    })
}

pub(crate) fn compact_host_current_thread_control_surface_for_runtime(surface: &Value) -> Value {
    let Some(node) = surface.as_object() else {
        return Value::Null;
    };
    let surface_exhausted_after_verified_failure = node
        .get("surface_exhausted_after_verified_failure")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut compact = serde_json::Map::new();
    for field in [
        "available",
        "automation_ready",
        "button_label",
        "command_id",
        "control_kind",
        "thread_id",
        "host_context_compaction_stage",
        "feedback_pending",
        "measurement_pending",
        "retry_allowed",
        "retry_blocked_reason",
        "effect_verdict",
        "selection_reason",
        "availability_state",
        "surface_exhausted_after_verified_failure",
    ] {
        if let Some(value) = node.get(field).filter(|value| !value.is_null()) {
            compact.insert(field.to_string(), value.clone());
        }
    }
    if !surface_exhausted_after_verified_failure {
        if let Some(value) = node.get("effect_summary").filter(|value| !value.is_null()) {
            compact.insert("effect_summary".to_string(), value.clone());
        }
        if let Some(alternates) = node.get("alternate_controls").and_then(Value::as_array) {
            let alternate_controls = alternates
                .iter()
                .map(|alternate| {
                    json!({
                        "button_label": alternate["button_label"].clone(),
                        "command_id": alternate["command_id"].clone(),
                        "control_kind": alternate["control_kind"].clone(),
                    })
                })
                .collect::<Vec<_>>();
            compact.insert(
                "alternate_controls".to_string(),
                Value::Array(alternate_controls),
            );
        }
    }
    Value::Object(compact)
}

pub(crate) fn normalize_host_current_thread_control_feedback_kind(
    value: &str,
) -> Option<&'static str> {
    match value.trim() {
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED => {
            Some(HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED)
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED => {
            Some(HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED)
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED => {
            Some(HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED)
        }
        _ => None,
    }
}

pub(crate) fn host_current_thread_control_feedback_notice_text_for_command(
    feedback_kind: &str,
    command_id: Option<&str>,
) -> String {
    let subject = host_current_thread_control_subject(
        normalize_host_current_thread_control_command_id(command_id),
    );
    match feedback_kind {
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED => {
            format!("Попытка открыть {subject} зафиксирована. После запуска отметь результат.")
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED => {
            format!("Подтверждено: {subject} открылся.")
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED => {
            format!("Зафиксировано: {subject} не открылся.")
        }
        _ => format!("Feedback по {subject} зафиксирован."),
    }
}

pub(super) fn host_current_thread_control_feedback_summary(
    feedback_kind: &str,
    command_id: Option<&str>,
) -> String {
    let subject = host_current_thread_control_subject(
        normalize_host_current_thread_control_command_id(command_id),
    );
    match feedback_kind {
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_REQUESTED => {
            format!("Requested {subject} launch via host current-thread control.")
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_OPENED => {
            format!("Operator confirmed {subject} opened.")
        }
        HOST_CURRENT_THREAD_CONTROL_FEEDBACK_FAILED => {
            format!("Operator reported {subject} did not open.")
        }
        _ => "Recorded host current-thread control feedback.".to_string(),
    }
}

pub(super) fn build_host_current_thread_control_feedback_snapshot_for_thread(
    thread_id: Option<&str>,
) -> Value {
    let thread_id = thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    if thread_id.is_empty() {
        return Value::Null;
    }
    let client_live_meter =
        codex_threads::latest_rollout_client_meter_observation_for_thread(thread_id)
            .ok()
            .flatten();
    let host_context_compaction =
        codex_threads::latest_rollout_context_compaction_observation_for_thread(thread_id)
            .ok()
            .flatten();
    let host_context_compaction = if let Some(observation) = host_context_compaction {
        let current_turn_total_tokens = client_live_meter
            .as_ref()
            .map(|value| value.client_turn_total_tokens)
            .unwrap_or_default();
        let growth_since_compaction_tokens =
            current_turn_total_tokens.saturating_sub(observation.post_compaction_turn_total_tokens);
        let recovered_surface_tokens = observation
            .pre_compaction_turn_total_tokens
            .saturating_sub(observation.post_compaction_turn_total_tokens);
        let regrowth_of_recovered_surface_ratio = if recovered_surface_tokens > 0 {
            growth_since_compaction_tokens as f64 / recovered_surface_tokens as f64
        } else {
            0.0
        };
        json!({
            "compacted_at_epoch_ms": observation.compacted_at_epoch_ms,
            "pre_compaction_turn_total_tokens": observation.pre_compaction_turn_total_tokens,
            "post_compaction_turn_total_tokens": observation.post_compaction_turn_total_tokens,
            "post_compaction_turn_id": observation.post_compaction_turn_id,
            "compaction_count": observation.compaction_count,
            "growth_since_compaction_tokens": growth_since_compaction_tokens,
            "recovered_surface_tokens": recovered_surface_tokens,
            "regrowth_of_recovered_surface_ratio": regrowth_of_recovered_surface_ratio,
            "observation_source": observation.observation_source,
        })
    } else {
        Value::Null
    };
    let client_live_meter = if let Some(observation) = client_live_meter {
        let context_used_percent = if observation.latest_model_context_window == 0 {
            None
        } else {
            Some(
                observation.client_turn_total_tokens as f64 * 100.0
                    / observation.latest_model_context_window as f64,
            )
        };
        json!({
            "thread_id": observation.thread_id,
            "turn_id": observation.turn_id,
            "started_at_epoch_ms": observation.started_at_epoch_ms,
            "ended_at_epoch_ms": observation.ended_at_epoch_ms,
            "client_turn_total_tokens": observation.client_turn_total_tokens,
            "latest_model_context_window": observation.latest_model_context_window,
            "context_used_percent": context_used_percent,
            "primary_limit_used_percent": observation.latest_primary_limit_used_percent,
            "primary_limit_remaining_percent":
                100_u64.saturating_sub(observation.latest_primary_limit_used_percent),
            "secondary_limit_used_percent": observation.latest_secondary_limit_used_percent,
            "secondary_limit_remaining_percent":
                100_u64.saturating_sub(observation.latest_secondary_limit_used_percent),
            "observation_source": observation.observation_source,
        })
    } else {
        Value::Null
    };
    json!({
        "snapshot_version": "host-current-thread-control-effect-snapshot-v1",
        "thread_id": thread_id,
        "client_live_meter": client_live_meter,
        "host_context_compaction": host_context_compaction,
    })
}
