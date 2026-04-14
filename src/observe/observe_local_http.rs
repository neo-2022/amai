use serde_json::Value;
use std::time::Duration;

use super::{
    compact_budget_snapshot_preview_payload,
    compact_cli_client_budget_gate_from_root_cause_payload,
    compact_thread_bound_client_budget_gate_payload_is_consistent,
    compact_thread_bound_client_budget_root_cause_payload_is_consistent,
    load_shared_budget_snapshot_preview, load_shared_compact_client_budget_gate,
    load_shared_compact_client_budget_surfaces,
};
use crate::config::discover_repo_root;

pub(super) async fn try_fetch_local_observe_gate_payload_via_http(
    explicit_thread_id: Option<&str>,
) -> Option<Value> {
    let thread_id = resolved_local_observe_thread_id(explicit_thread_id);
    let repo_root = discover_repo_root(None).ok()?;
    let now_epoch_ms = super::current_epoch_ms_u64();
    if let Some(cached) =
        load_shared_compact_client_budget_gate(&repo_root, now_epoch_ms, thread_id.as_deref())
    {
        if compact_thread_bound_client_budget_gate_payload_is_consistent(
            thread_id.as_deref(),
            &cached.gate,
        ) {
            return Some(cached.gate);
        }
    }
    let client = reqwest::Client::builder()
        .timeout(client_budget_local_observe_http_timeout(
            thread_id.is_some(),
        ))
        .build()
        .ok()?;
    let base_url = local_observe_http_base_url();
    let root_cause_request = client.get(format!("{base_url}/api/client-budget-root-cause"));
    let root_cause = (if let Some(thread_id) = thread_id.as_deref() {
        root_cause_request.query(&[("thread_id", thread_id)])
    } else {
        root_cause_request
    })
    .send()
    .await
    .ok()?
    .error_for_status()
    .ok()?
    .json::<Value>()
    .await
    .ok()?;
    if let Some(gate) = compact_cli_client_budget_gate_from_root_cause_payload(&root_cause)
        && compact_thread_bound_client_budget_gate_payload_is_consistent(
            thread_id.as_deref(),
            &gate,
        )
    {
        return Some(gate);
    }
    let gate_request = client.get(format!("{base_url}/api/client-budget-gate"));
    let response = (if let Some(thread_id) = thread_id.as_deref() {
        gate_request.query(&[("thread_id", thread_id)])
    } else {
        gate_request
    })
    .send()
    .await
    .ok()?;
    let gate = response
        .error_for_status()
        .ok()?
        .json::<Value>()
        .await
        .ok()?;
    compact_thread_bound_client_budget_gate_payload_is_consistent(thread_id.as_deref(), &gate)
        .then_some(gate)
}

pub(super) async fn try_fetch_local_observe_root_cause_payload_via_http(
    explicit_thread_id: Option<&str>,
) -> Option<Value> {
    let thread_id = resolved_local_observe_thread_id(explicit_thread_id);
    let repo_root = discover_repo_root(None).ok()?;
    let now_epoch_ms = super::current_epoch_ms_u64();
    if let Some(cached) =
        load_shared_compact_client_budget_surfaces(&repo_root, now_epoch_ms, thread_id.as_deref())
    {
        if compact_thread_bound_client_budget_root_cause_payload_is_consistent(
            thread_id.as_deref(),
            &cached.root_cause,
        ) {
            return Some(cached.root_cause);
        }
    }
    let client = reqwest::Client::builder()
        .timeout(client_budget_local_observe_http_timeout(
            thread_id.is_some(),
        ))
        .build()
        .ok()?;
    let base_url = local_observe_http_base_url();
    let request = client.get(format!("{base_url}/api/client-budget-root-cause"));
    let payload = (if let Some(thread_id) = thread_id.as_deref() {
        request.query(&[("thread_id", thread_id)])
    } else {
        request
    })
    .send()
    .await
    .ok()?
    .error_for_status()
    .ok()?
    .json::<Value>()
    .await
    .ok()?;
    compact_thread_bound_client_budget_root_cause_payload_is_consistent(
        thread_id.as_deref(),
        &payload,
    )
    .then_some(payload)
}

pub(super) async fn try_fetch_local_observe_budget_snapshot_preview_via_http() -> Option<Value> {
    let thread_id = local_observe_thread_id_from_env();
    let repo_root = discover_repo_root(None).ok()?;
    if let Some(snapshot) = load_shared_budget_snapshot_preview(&repo_root, thread_id.as_deref()) {
        return Some(compact_budget_snapshot_preview_payload(&snapshot));
    }
    let client = reqwest::Client::builder()
        .timeout(client_budget_local_observe_http_timeout(
            thread_id.is_some(),
        ))
        .build()
        .ok()?;
    let base_url = local_observe_http_base_url();
    let request = client.get(format!("{base_url}/api/client-budget-snapshot-preview"));
    (if let Some(thread_id) = thread_id.as_deref() {
        request.query(&[("thread_id", thread_id)])
    } else {
        request
    })
    .send()
    .await
    .ok()?
    .error_for_status()
    .ok()?
    .json::<Value>()
    .await
    .ok()
}

pub(super) fn local_observe_http_base_url() -> String {
    let observe_bind =
        std::env::var("AMI_OBSERVE_BIND").unwrap_or_else(|_| "0.0.0.0:9464".to_string());
    let (raw_host, raw_port) = observe_bind
        .rsplit_once(':')
        .map(|(host, port)| (host.trim(), port.trim()))
        .unwrap_or(("0.0.0.0", "9464"));
    let host = match raw_host {
        "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
        value if value.starts_with('[') && value.ends_with(']') && value.len() > 2 => {
            value[1..value.len() - 1].to_string()
        }
        value => value.to_string(),
    };
    let port = if raw_port.is_empty() {
        "9464"
    } else {
        raw_port
    };
    if host.contains(':') {
        format!("http://[{host}]:{port}")
    } else {
        format!("http://{host}:{port}")
    }
}

pub(super) fn local_observe_thread_id_from_env() -> Option<String> {
    std::env::var("CODEX_THREAD_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(super) fn resolved_local_observe_thread_id(explicit_thread_id: Option<&str>) -> Option<String> {
    explicit_thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(local_observe_thread_id_from_env)
}

pub(super) fn client_budget_local_observe_http_timeout(thread_bound: bool) -> Duration {
    let fallback_ms = if thread_bound { 7000 } else { 1500 };
    let override_ms = std::env::var("AMI_CLIENT_BUDGET_OBSERVE_HTTP_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(200, 20_000));
    Duration::from_millis(override_ms.unwrap_or(fallback_ms))
}
