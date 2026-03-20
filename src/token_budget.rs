use crate::cli::ObserveTokenReportArgs;
use crate::config;
use crate::language;
use crate::postgres::{self, ObservabilitySnapshotRecord};
use anyhow::{Context, Result, anyhow, bail};
use ignore::WalkBuilder;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tiktoken_rs::{CoreBPE, cl100k_base, o200k_base};
use tokio_postgres::Client;
use uuid::Uuid;

const CONFIG_RELATIVE_PATH: &str = "config/token_budget_profiles.toml";

#[derive(Debug, Clone, Deserialize)]
struct TokenBudgetConfigFile {
    default_profile: String,
    measurement: MeasurementConfig,
    #[serde(default)]
    profiles: BTreeMap<String, TokenBudgetProfile>,
    #[serde(default)]
    client_budget_overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct MeasurementConfig {
    tokenizer: String,
    naive_limit_files: usize,
    naive_max_bytes_per_file: usize,
    #[serde(default)]
    include_verify_events_by_default: bool,
    #[serde(default = "default_preliminary_min_events")]
    preliminary_min_events: u64,
    #[serde(default = "default_preliminary_min_baseline_tokens")]
    preliminary_min_baseline_tokens: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct TokenBudgetProfile {
    display_name: String,
    description: String,
    session_gap_minutes: u64,
    rolling_window_hours: Option<u64>,
}

#[derive(Debug, Clone)]
struct ResolvedProfile {
    code: String,
    display_name: String,
    description: String,
    session_gap_minutes: u64,
    rolling_window_hours: Option<u64>,
}

#[derive(Debug, Clone)]
struct TokenBudgetEvent {
    created_at_epoch_ms: i64,
    event_id: String,
    timestamp_utc: i64,
    snapshot_kind: String,
    source_kind: String,
    traffic_class: String,
    project: String,
    namespace: String,
    query: String,
    query_hash: String,
    query_type: String,
    cold_warm_state: String,
    baseline_strategy: String,
    retrieval_mode: Option<String>,
    tokenizer: String,
    saved_tokens: u64,
    naive_tokens: u64,
    context_tokens: u64,
    recovery_tokens: u64,
    effective_saved_tokens: i64,
    savings_factor: f64,
    savings_percent: f64,
    effective_savings_percent: f64,
    quality_ok: bool,
    quality_score: f64,
    quality_method: String,
    fallback_triggered: bool,
    fallback_count: u64,
    sources_count: u64,
    chunks_count: u64,
}

#[derive(Debug)]
struct NaiveScopeFile {
    project_code: String,
    relative_path: String,
    original_bytes: usize,
    bytes_used: usize,
    truncated: bool,
    content: String,
}

#[derive(Debug)]
struct NaiveScope {
    files: Vec<Value>,
    rendered_files: Vec<NaiveScopeFile>,
}

fn default_preliminary_min_events() -> u64 {
    50
}

fn default_preliminary_min_baseline_tokens() -> u64 {
    100_000
}

pub async fn print_report(db: &Client, args: &ObserveTokenReportArgs) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let include_verify = args
        .include_verify_events
        .unwrap_or(config.measurement.include_verify_events_by_default);
    let report = collect_report(
        &repo_root,
        db,
        args.budget_profile.as_deref(),
        include_verify,
        None,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

pub async fn collect_default_report(db: &Client) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    collect_report(
        &repo_root,
        db,
        None,
        config.measurement.include_verify_events_by_default,
        None,
    )
    .await
}

pub async fn collect_default_report_with_overrides(
    db: &Client,
    requested_profile: Option<&str>,
    include_verify_events: Option<bool>,
) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    collect_report(
        &repo_root,
        db,
        requested_profile,
        include_verify_events.unwrap_or(config.measurement.include_verify_events_by_default),
        None,
    )
    .await
}

pub async fn record_live_context_pack_event(db: &Client, payload: &Value) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let event = build_event_payload(
        payload,
        &config.measurement,
        "live_context_pack",
        "context_pack_token_budget",
    )?;
    let _ = postgres::insert_observability_snapshot(db, "token_budget_event", &event).await?;
    Ok(())
}

pub async fn record_verify_benchmark_event(db: &Client, benchmark_payload: &Value) -> Result<()> {
    let benchmark = benchmark_payload
        .get("token_benchmark")
        .cloned()
        .ok_or_else(|| anyhow!("token benchmark payload missing token_benchmark root"))?;
    let event = json!({
        "token_budget_event": {
            "event_id": Uuid::new_v4(),
            "timestamp_utc": current_epoch_ms()?,
            "source_kind": "verify_token_benchmark",
            "traffic_class": "verify",
            "payload_origin": "verify_token_benchmark",
            "project": benchmark["project"].clone(),
            "namespace": benchmark["namespace"].clone(),
            "query": benchmark["query"].clone(),
            "query_hash": hex_sha256(benchmark["query"].as_str().unwrap_or_default().as_bytes()),
            "query_type": "unknown",
            "cold_warm_state": "benchmark",
            "baseline_strategy": "naive_top_files",
            "retrieval_mode": benchmark["retrieval_mode"].clone(),
            "tokenizer": benchmark["tokenizer"].clone(),
            "naive_limit_files": benchmark["naive_limit_files"].clone(),
            "naive_max_bytes_per_file": benchmark["naive_max_bytes_per_file"].clone(),
            "visible_projects": benchmark["visible_projects"].clone(),
            "naive_scope": benchmark["naive_scope"].clone(),
            "context_pack_render": benchmark["context_pack_render"].clone(),
            "recovery": {
                "recovery_tokens": 0,
                "fallback_triggered": false,
                "fallback_count": 0,
            },
            "quality": {
                "quality_ok": true,
                "quality_score": 1.0,
                "quality_method": "benchmark_assumption",
            },
            "shape": {
                "sources_count": 0,
                "chunks_count": 0,
            },
            "savings": benchmark["savings"].clone()
        }
    });
    let _ = postgres::insert_observability_snapshot(db, "token_budget_event", &event).await?;
    Ok(())
}

async fn collect_report(
    repo_root: &Path,
    db: &Client,
    requested_profile: Option<&str>,
    include_verify_events: bool,
    limit: Option<i64>,
) -> Result<Value> {
    let config = load_config(repo_root)?;
    let profile = resolve_profile(&config, requested_profile, repo_root)?;
    let mut events = load_events(db, include_verify_events, limit).await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);

    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let session_gap_ms = profile.session_gap_minutes.saturating_mul(60_000) as i64;
    let session_events = current_session_events(&events, session_gap_ms);
    let rolling_window_events = profile
        .rolling_window_hours
        .map(|hours| {
            let lower_bound = now_epoch_ms.saturating_sub((hours as i64).saturating_mul(3_600_000));
            events
                .iter()
                .filter(|event| event.created_at_epoch_ms >= lower_bound)
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let latest_event = events
        .last()
        .map(event_to_json)
        .unwrap_or_else(|| json!(null));
    let source_breakdown = source_breakdown(&events, &config.measurement);

    Ok(json!({
        "token_budget_report": {
            "profile": {
                "code": profile.code,
                "display_name": profile.display_name,
                "description": profile.description,
                "session_gap_minutes": profile.session_gap_minutes,
                "rolling_window_hours": profile.rolling_window_hours,
                "preliminary_min_events": config.measurement.preliminary_min_events,
                "preliminary_min_baseline_tokens": config.measurement.preliminary_min_baseline_tokens,
            },
            "filters": {
                "include_verify_events": include_verify_events,
            },
            "latest_event": latest_event,
            "current_session": summarize_events(&session_events, now_epoch_ms, &config.measurement),
            "rolling_window": if profile.rolling_window_hours.is_some() {
                summarize_events(&rolling_window_events, now_epoch_ms, &config.measurement)
            } else {
                json!(null)
            },
            "lifetime": summarize_events(&events, now_epoch_ms, &config.measurement),
            "source_breakdown": source_breakdown,
        }
    }))
}

fn load_config(repo_root: &Path) -> Result<TokenBudgetConfigFile> {
    let path = repo_root.join(CONFIG_RELATIVE_PATH);
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn resolve_profile(
    config: &TokenBudgetConfigFile,
    requested_profile: Option<&str>,
    repo_root: &Path,
) -> Result<ResolvedProfile> {
    let install_state_path = repo_root.join("state/install_state.json");
    let install_state_client = fs::read_to_string(&install_state_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|value| value["client_key"].as_str().map(ToOwned::to_owned));
    let profile_code = if let Some(requested) = requested_profile {
        requested.to_string()
    } else if let Ok(from_env) = std::env::var("AMAI_TOKEN_BUDGET_PROFILE") {
        from_env
    } else if let Some(client_key) = install_state_client {
        config
            .client_budget_overrides
            .get(&client_key)
            .cloned()
            .unwrap_or_else(|| config.default_profile.clone())
    } else {
        config.default_profile.clone()
    };
    let profile = config
        .profiles
        .get(&profile_code)
        .ok_or_else(|| anyhow!("unknown token budget profile: {profile_code}"))?;
    Ok(ResolvedProfile {
        code: profile_code,
        display_name: profile.display_name.clone(),
        description: profile.description.clone(),
        session_gap_minutes: profile.session_gap_minutes,
        rolling_window_hours: profile.rolling_window_hours,
    })
}

async fn load_events(
    db: &Client,
    include_verify_events: bool,
    limit: Option<i64>,
) -> Result<Vec<TokenBudgetEvent>> {
    let rows = postgres::list_observability_snapshots_by_kinds(
        db,
        &["token_budget_event", "token_benchmark"],
        limit,
    )
    .await?;
    let mut events = Vec::new();
    for row in rows {
        if let Some(event) = parse_snapshot_event(&row)? {
            if !include_verify_events && event.source_kind.starts_with("verify_") {
                continue;
            }
            events.push(event);
        }
    }
    Ok(events)
}

fn parse_snapshot_event(row: &ObservabilitySnapshotRecord) -> Result<Option<TokenBudgetEvent>> {
    let (node, fallback_source_kind) = match row.snapshot_kind.as_str() {
        "token_budget_event" => (&row.payload["token_budget_event"], None),
        "token_benchmark" => (
            &row.payload["token_benchmark"],
            Some("verify_token_benchmark_legacy"),
        ),
        _ => return Ok(None),
    };
    if !node.is_object() {
        return Ok(None);
    }
    let source_kind = node["source_kind"]
        .as_str()
        .or(fallback_source_kind)
        .unwrap_or("unknown")
        .to_string();
    let traffic_class = node["traffic_class"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_traffic_class(&source_kind));
    let project = node["project"].as_str().unwrap_or_default().to_string();
    let namespace = node["namespace"].as_str().unwrap_or_default().to_string();
    let query = node["query"].as_str().unwrap_or_default().to_string();
    let query_hash = node["query_hash"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| hex_sha256(query.as_bytes()));
    let query_type = node["query_type"].as_str().unwrap_or("unknown").to_string();
    let cold_warm_state = node["cold_warm_state"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let baseline_strategy = node["baseline_strategy"]
        .as_str()
        .unwrap_or("naive_top_files")
        .to_string();
    let retrieval_mode = node["retrieval_mode"].as_str().map(ToOwned::to_owned);
    let tokenizer = node["tokenizer"].as_str().unwrap_or_default().to_string();
    let event_id = node["event_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{}-{}", row.snapshot_kind, row.created_at_epoch_ms));
    let timestamp_utc = node["timestamp_utc"]
        .as_i64()
        .unwrap_or(row.created_at_epoch_ms);
    let saved_tokens = node["savings"]["saved_tokens"].as_u64().unwrap_or(0);
    let naive_tokens = node["naive_scope"]["tokens"].as_u64().unwrap_or(0);
    let context_tokens = node["context_pack_render"]["tokens"].as_u64().unwrap_or(0);
    let recovery_tokens = node["recovery"]["recovery_tokens"].as_u64().unwrap_or(0);
    let effective_saved_tokens = node["savings"]["effective_saved_tokens"]
        .as_i64()
        .unwrap_or_else(|| naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64));
    let savings_factor = node["savings"]["savings_factor"].as_f64().unwrap_or(0.0);
    let savings_percent = node["savings"]["savings_percent"].as_f64().unwrap_or(0.0);
    let effective_savings_percent = node["savings"]["effective_savings_percent"]
        .as_f64()
        .unwrap_or_else(|| percent_from_signed(effective_saved_tokens, naive_tokens));
    let quality_ok = node["quality"]["quality_ok"].as_bool().unwrap_or(false);
    let quality_score = node["quality"]["quality_score"]
        .as_f64()
        .unwrap_or(if quality_ok { 1.0 } else { 0.0 });
    let quality_method = node["quality"]["quality_method"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let fallback_triggered = node["recovery"]["fallback_triggered"]
        .as_bool()
        .unwrap_or(false);
    let fallback_count = node["recovery"]["fallback_count"].as_u64().unwrap_or(0);
    let sources_count = node["shape"]["sources_count"].as_u64().unwrap_or(0);
    let chunks_count = node["shape"]["chunks_count"].as_u64().unwrap_or(0);

    Ok(Some(TokenBudgetEvent {
        created_at_epoch_ms: row.created_at_epoch_ms,
        event_id,
        timestamp_utc,
        snapshot_kind: row.snapshot_kind.clone(),
        source_kind,
        traffic_class,
        project,
        namespace,
        query,
        query_hash,
        query_type,
        cold_warm_state,
        baseline_strategy,
        retrieval_mode,
        tokenizer,
        saved_tokens,
        naive_tokens,
        context_tokens,
        recovery_tokens,
        effective_saved_tokens,
        savings_factor,
        savings_percent,
        effective_savings_percent,
        quality_ok,
        quality_score,
        quality_method,
        fallback_triggered,
        fallback_count,
        sources_count,
        chunks_count,
    }))
}

fn current_session_events(
    events: &[TokenBudgetEvent],
    session_gap_ms: i64,
) -> Vec<TokenBudgetEvent> {
    let Some(latest) = events.last() else {
        return Vec::new();
    };
    let mut session = vec![latest.clone()];
    let mut newer_ts = latest.created_at_epoch_ms;
    for event in events.iter().rev().skip(1) {
        if newer_ts.saturating_sub(event.created_at_epoch_ms) > session_gap_ms {
            break;
        }
        session.push(event.clone());
        newer_ts = event.created_at_epoch_ms;
    }
    session.reverse();
    session
}

fn summarize_events(
    events: &[TokenBudgetEvent],
    now_epoch_ms: i64,
    measurement: &MeasurementConfig,
) -> Value {
    if events.is_empty() {
        return json!({
            "events_total": 0,
            "events_count": 0,
            "counted_events": 0,
            "preliminary": true,
            "total_saved_tokens": 0,
            "total_effective_saved_tokens": 0,
            "verified_effective_saved_tokens": 0,
            "total_naive_tokens": 0,
            "total_context_tokens": 0,
            "total_recovery_tokens": 0,
            "gross_savings_pct": 0.0,
            "effective_savings_pct": 0.0,
            "verified_effective_savings_pct": 0.0,
            "savings_percent": 0.0,
            "savings_factor": 0.0,
            "avg_saved_tokens_per_event": 0.0,
            "quality_ok_rate": 0.0,
            "fallback_rate": 0.0,
            "started_at_epoch_ms": Value::Null,
            "ended_at_epoch_ms": Value::Null,
            "age_ms_since_latest": Value::Null,
        });
    }

    let total_saved_tokens = events.iter().map(|event| event.saved_tokens).sum::<u64>();
    let total_naive_tokens = events.iter().map(|event| event.naive_tokens).sum::<u64>();
    let total_context_tokens = events.iter().map(|event| event.context_tokens).sum::<u64>();
    let total_recovery_tokens = events
        .iter()
        .map(|event| event.recovery_tokens)
        .sum::<u64>();
    let total_effective_saved_tokens = events
        .iter()
        .map(|event| event.effective_saved_tokens)
        .sum::<i64>();
    let verified_events = events
        .iter()
        .filter(|event| event.traffic_class == "live" && event.quality_ok)
        .collect::<Vec<_>>();
    let verified_effective_saved_tokens = verified_events
        .iter()
        .map(|event| event.effective_saved_tokens)
        .sum::<i64>();
    let verified_baseline_tokens = verified_events
        .iter()
        .map(|event| event.naive_tokens)
        .sum::<u64>();
    let gross_savings_pct = if total_naive_tokens == 0 {
        0.0
    } else {
        total_saved_tokens as f64 * 100.0 / total_naive_tokens as f64
    };
    let effective_savings_pct =
        percent_from_signed(total_effective_saved_tokens, total_naive_tokens);
    let verified_effective_savings_pct =
        percent_from_signed(verified_effective_saved_tokens, verified_baseline_tokens);
    let savings_factor = if total_context_tokens == 0 {
        total_naive_tokens as f64
    } else {
        total_naive_tokens as f64 / total_context_tokens as f64
    };
    let avg_saved_tokens_per_event = total_saved_tokens as f64 / events.len() as f64;
    let quality_ok_events = events.iter().filter(|event| event.quality_ok).count() as f64;
    let fallback_events = events
        .iter()
        .filter(|event| event.fallback_triggered)
        .count() as f64;
    let quality_ok_rate = quality_ok_events * 100.0 / events.len() as f64;
    let fallback_rate = fallback_events * 100.0 / events.len() as f64;
    let started_at_epoch_ms = events
        .first()
        .map(|event| event.created_at_epoch_ms)
        .unwrap_or_default();
    let ended_at_epoch_ms = events
        .last()
        .map(|event| event.created_at_epoch_ms)
        .unwrap_or_default();

    let preliminary = events.len() < measurement.preliminary_min_events as usize
        || total_naive_tokens < measurement.preliminary_min_baseline_tokens;

    json!({
        "events_total": events.len(),
        "events_count": events.len(),
        "counted_events": verified_events.len(),
        "preliminary": preliminary,
        "total_saved_tokens": total_saved_tokens,
        "total_effective_saved_tokens": total_effective_saved_tokens,
        "verified_effective_saved_tokens": verified_effective_saved_tokens,
        "total_naive_tokens": total_naive_tokens,
        "total_context_tokens": total_context_tokens,
        "total_recovery_tokens": total_recovery_tokens,
        "gross_savings_pct": gross_savings_pct,
        "effective_savings_pct": effective_savings_pct,
        "verified_effective_savings_pct": verified_effective_savings_pct,
        "savings_percent": gross_savings_pct,
        "savings_factor": savings_factor,
        "avg_saved_tokens_per_event": avg_saved_tokens_per_event,
        "quality_ok_rate": quality_ok_rate,
        "fallback_rate": fallback_rate,
        "started_at_epoch_ms": started_at_epoch_ms,
        "ended_at_epoch_ms": ended_at_epoch_ms,
        "age_ms_since_latest": now_epoch_ms.saturating_sub(ended_at_epoch_ms),
    })
}

fn source_breakdown(events: &[TokenBudgetEvent], measurement: &MeasurementConfig) -> Value {
    let mut grouped = BTreeMap::<String, Vec<TokenBudgetEvent>>::new();
    for event in events {
        grouped
            .entry(event.source_kind.clone())
            .or_default()
            .push(event.clone());
    }
    Value::Array(
        grouped
            .into_iter()
            .map(|(source_kind, items)| {
                json!({
                    "source_kind": source_kind,
                    "summary": summarize_events(
                        &items,
                        items.last()
                            .map(|item| item.created_at_epoch_ms)
                            .unwrap_or_default(),
                        measurement,
                    ),
                })
            })
            .collect(),
    )
}

fn event_to_json(event: &TokenBudgetEvent) -> Value {
    json!({
        "created_at_epoch_ms": event.created_at_epoch_ms,
        "event_id": event.event_id,
        "timestamp_utc": event.timestamp_utc,
        "snapshot_kind": event.snapshot_kind,
        "source_kind": event.source_kind,
        "traffic_class": event.traffic_class,
        "project": event.project,
        "namespace": event.namespace,
        "query": event.query,
        "query_hash": event.query_hash,
        "query_type": event.query_type,
        "cold_warm_state": event.cold_warm_state,
        "baseline_strategy": event.baseline_strategy,
        "retrieval_mode": event.retrieval_mode,
        "tokenizer": event.tokenizer,
        "saved_tokens": event.saved_tokens,
        "naive_tokens": event.naive_tokens,
        "context_tokens": event.context_tokens,
        "recovery_tokens": event.recovery_tokens,
        "effective_saved_tokens": event.effective_saved_tokens,
        "savings_factor": event.savings_factor,
        "savings_percent": event.savings_percent,
        "effective_savings_percent": event.effective_savings_percent,
        "quality_ok": event.quality_ok,
        "quality_score": event.quality_score,
        "quality_method": event.quality_method,
        "fallback_triggered": event.fallback_triggered,
        "fallback_count": event.fallback_count,
        "sources_count": event.sources_count,
        "chunks_count": event.chunks_count,
    })
}

fn build_event_payload(
    payload: &Value,
    measurement: &MeasurementConfig,
    source_kind: &str,
    payload_origin: &str,
) -> Result<Value> {
    let tokenizer = build_tokenizer(&measurement.tokenizer)?;
    let naive_scope = collect_naive_scope(
        payload,
        measurement.naive_limit_files,
        measurement.naive_max_bytes_per_file,
    )?;
    let naive_prompt = render_naive_scope_prompt(payload, &naive_scope);
    let context_prompt = render_context_pack_prompt(payload);
    let naive_tokens = tokenizer.encode_with_special_tokens(&naive_prompt).len();
    let context_tokens = tokenizer.encode_with_special_tokens(&context_prompt).len();
    let saved_tokens = naive_tokens.saturating_sub(context_tokens);
    let recovery_tokens = 0_u64;
    let effective_saved_tokens =
        naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64);
    let savings_factor = if context_tokens == 0 {
        naive_tokens as f64
    } else {
        naive_tokens as f64 / context_tokens as f64
    };
    let savings_percent = if naive_tokens == 0 {
        0.0
    } else {
        saved_tokens as f64 * 100.0 / naive_tokens as f64
    };
    let effective_savings_percent =
        percent_from_signed(effective_saved_tokens, naive_tokens as u64);
    let (quality_ok, quality_score, quality_method) = derive_quality_verdict(payload);
    let fallback_count = count_lexical_fallback_chunks(payload) as u64;
    let fallback_triggered = fallback_count > 0;
    let sources_count = count_sources(payload) as u64;
    let chunks_count = count_chunks(payload) as u64;
    let event_id = payload["context_pack_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let timestamp_utc = current_epoch_ms()?;

    Ok(json!({
        "token_budget_event": {
            "event_id": event_id,
            "timestamp_utc": timestamp_utc,
            "source_kind": source_kind,
            "traffic_class": "live",
            "payload_origin": payload_origin,
            "project": payload["project"]["code"].clone(),
            "namespace": payload["namespace"]["code"].clone(),
            "query": payload["query"].clone(),
            "query_hash": hex_sha256(payload["query"].as_str().unwrap_or_default().as_bytes()),
            "query_type": "unknown",
            "cold_warm_state": if payload["retrieval_runtime"]["cache_hit"].as_bool().unwrap_or(false) {
                "warm"
            } else {
                "cold"
            },
            "baseline_strategy": "naive_top_files",
            "retrieval_mode": payload["effective_retrieval_mode"].clone(),
            "tokenizer": measurement.tokenizer,
            "naive_limit_files": measurement.naive_limit_files,
            "naive_max_bytes_per_file": measurement.naive_max_bytes_per_file,
            "visible_projects": payload["visible_projects"].clone(),
            "naive_scope": {
                "files_considered": naive_scope.files.len(),
                "files": naive_scope.files,
                "rendered_bytes": naive_prompt.len(),
                "tokens": naive_tokens,
            },
            "context_pack_render": {
                "rendered_bytes": context_prompt.len(),
                "tokens": context_tokens,
            },
            "recovery": {
                "recovery_tokens": recovery_tokens,
                "fallback_triggered": fallback_triggered,
                "fallback_count": fallback_count,
            },
            "quality": {
                "quality_ok": quality_ok,
                "quality_score": quality_score,
                "quality_method": quality_method,
            },
            "shape": {
                "sources_count": sources_count,
                "chunks_count": chunks_count,
            },
            "savings": {
                "saved_tokens": saved_tokens,
                "effective_saved_tokens": effective_saved_tokens,
                "savings_factor": savings_factor,
                "savings_percent": savings_percent,
                "effective_savings_percent": effective_savings_percent,
            }
        }
    }))
}

fn derive_traffic_class(source_kind: &str) -> String {
    if source_kind.starts_with("live_") {
        "live".to_string()
    } else if source_kind.starts_with("verify_") {
        "verify".to_string()
    } else if source_kind.starts_with("proof_") {
        "proof".to_string()
    } else if source_kind.starts_with("benchmark_") {
        "benchmark".to_string()
    } else {
        "unknown".to_string()
    }
}

fn derive_quality_verdict(payload: &Value) -> (bool, f64, &'static str) {
    let exact_hits = payload["retrieval"]["exact_documents"]
        .as_array()
        .map_or(0, Vec::len);
    let symbol_hits = payload["retrieval"]["symbol_hits"]
        .as_array()
        .map_or(0, Vec::len);
    let lexical_hits = payload["retrieval"]["lexical_chunks"]
        .as_array()
        .map_or(0, Vec::len);
    let semantic_hits = payload["retrieval"]["semantic_chunks"]
        .as_array()
        .map_or(0, Vec::len);
    let semantic_guard_abstained = payload["quality"]["semantic_guard"]["abstained"]
        .as_bool()
        .unwrap_or(false);
    let total_hits = exact_hits + symbol_hits + lexical_hits + semantic_hits;
    let quality_ok = total_hits > 0 && !semantic_guard_abstained;
    let quality_score = if quality_ok { 1.0 } else { 0.0 };
    (quality_ok, quality_score, "retrieval_parity")
}

fn count_lexical_fallback_chunks(payload: &Value) -> usize {
    payload["retrieval"]["semantic_chunks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|chunk| chunk["retrieval_strategy"].as_str() == Some("lexical_fallback"))
        .count()
}

fn count_sources(payload: &Value) -> usize {
    let retrieval = &payload["retrieval"];
    retrieval["exact_documents"].as_array().map_or(0, Vec::len)
        + retrieval["symbol_hits"].as_array().map_or(0, Vec::len)
        + retrieval["lexical_chunks"].as_array().map_or(0, Vec::len)
        + retrieval["semantic_chunks"].as_array().map_or(0, Vec::len)
}

fn count_chunks(payload: &Value) -> usize {
    let retrieval = &payload["retrieval"];
    retrieval["lexical_chunks"].as_array().map_or(0, Vec::len)
        + retrieval["semantic_chunks"].as_array().map_or(0, Vec::len)
}

fn current_epoch_ms() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64)
}

fn percent_from_signed(saved_tokens: i64, baseline_tokens: u64) -> f64 {
    if baseline_tokens == 0 {
        0.0
    } else {
        saved_tokens as f64 * 100.0 / baseline_tokens as f64
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn collect_naive_scope(
    payload: &Value,
    limit_files: usize,
    max_bytes_per_file: usize,
) -> Result<NaiveScope> {
    let mut files = Vec::new();
    for project in payload["visible_projects"].as_array().into_iter().flatten() {
        let Some(project_code) = project["project_code"].as_str() else {
            continue;
        };
        let Some(repo_root) = project["repo_root"].as_str() else {
            continue;
        };
        for path in collect_scope_files(Path::new(repo_root), limit_files)? {
            let relative_path = path
                .strip_prefix(repo_root)
                .unwrap_or(path.as_path())
                .display()
                .to_string();
            let bytes = fs::read(&path)
                .with_context(|| format!("failed to read naive scope file {}", path.display()))?;
            let original_bytes = bytes.len();
            let bytes_used = original_bytes.min(max_bytes_per_file);
            let content = safe_lossy_prefix(&bytes, bytes_used);
            files.push(NaiveScopeFile {
                project_code: project_code.to_string(),
                relative_path,
                original_bytes,
                bytes_used: content.len(),
                truncated: original_bytes > content.len(),
                content,
            });
        }
    }

    files.sort_by(|left, right| {
        left.project_code
            .cmp(&right.project_code)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    if limit_files > 0 {
        files.truncate(limit_files);
    }

    let metadata = files
        .iter()
        .map(|file| {
            json!({
                "project_code": file.project_code,
                "relative_path": file.relative_path,
                "original_bytes": file.original_bytes,
                "bytes_used": file.bytes_used,
                "truncated": file.truncated,
            })
        })
        .collect();

    Ok(NaiveScope {
        files: metadata,
        rendered_files: files,
    })
}

fn collect_scope_files(root: &Path, limit_files: usize) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        bail!("visible project root does not exist: {}", root.display());
    }
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(true)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true);
    let mut files = builder
        .build()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .map(|entry| entry.into_path())
        .filter(|path| language::detect(path).is_some())
        .collect::<Vec<_>>();
    files.sort();
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn safe_lossy_prefix(bytes: &[u8], max_bytes: usize) -> String {
    let slice = &bytes[..bytes.len().min(max_bytes)];
    String::from_utf8_lossy(slice).into_owned()
}

fn render_naive_scope_prompt(payload: &Value, scope: &NaiveScope) -> String {
    let mut prompt = String::new();
    prompt.push_str("NAIVE_SCOPE\n");
    prompt.push_str(
        "This bundle represents the visible project scope without retrieval reduction.\n",
    );
    prompt.push_str("Query: ");
    prompt.push_str(payload["query"].as_str().unwrap_or_default());
    prompt.push_str("\nVisible projects:\n");
    for project in payload["visible_projects"].as_array().into_iter().flatten() {
        prompt.push_str("- ");
        prompt.push_str(project["project_code"].as_str().unwrap_or_default());
        prompt.push_str(" :: ");
        prompt.push_str(project["repo_root"].as_str().unwrap_or_default());
        prompt.push('\n');
    }
    prompt.push('\n');
    for file in &scope.rendered_files {
        prompt.push_str("## PROJECT ");
        prompt.push_str(&file.project_code);
        prompt.push('\n');
        prompt.push_str("### FILE ");
        prompt.push_str(&file.relative_path);
        prompt.push('\n');
        prompt.push_str(&file.content);
        prompt.push_str("\n\n");
    }
    prompt
}

fn render_context_pack_prompt(payload: &Value) -> String {
    let mut excerpt_paths = HashSet::new();
    let mut exact_lines = Vec::new();
    let mut symbol_lines = Vec::new();
    let mut seen_symbols = HashSet::new();
    for item in payload["retrieval"]["symbol_hits"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let line = format!(
            "[{}] {} :: {} :: {}",
            item["provenance"]["source_project"]
                .as_str()
                .unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
            item["name"].as_str().unwrap_or_default(),
            item["kind"].as_str().unwrap_or_default(),
        );
        if seen_symbols.insert(line.clone()) {
            symbol_lines.push(line);
        }
    }

    let mut excerpt_lines = Vec::new();
    let mut seen_excerpts = HashSet::new();
    for section in ["lexical_chunks", "semantic_chunks"] {
        for item in payload["retrieval"][section]
            .as_array()
            .into_iter()
            .flatten()
        {
            let line = format!(
                "[{}] {} :: {}",
                item["provenance"]["source_project"]
                    .as_str()
                    .or_else(|| item["project_code"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default(),
                item["content"].as_str().unwrap_or_default(),
            );
            if seen_excerpts.insert(line.clone()) {
                excerpt_lines.push(line);
            }
            excerpt_paths.insert(format!(
                "{}::{}",
                item["provenance"]["source_project"]
                    .as_str()
                    .or_else(|| item["project_code"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default()
            ));
        }
    }

    let mut seen_exact = HashSet::new();
    for item in payload["retrieval"]["exact_documents"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let key = format!(
            "{}::{}",
            item["project_code"].as_str().unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default()
        );
        if excerpt_paths.contains(&key) {
            continue;
        }
        let line = format!(
            "[{}] {} {}",
            item["project_code"].as_str().unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
            item["snippet"].as_str().unwrap_or_default(),
        );
        if seen_exact.insert(line.clone()) {
            exact_lines.push(line);
        }
    }

    let mut prompt = String::new();
    prompt.push_str("Q:");
    prompt.push_str(payload["query"].as_str().unwrap_or_default());
    prompt.push('\n');
    prompt.push_str("M:");
    prompt.push_str(
        payload["effective_retrieval_mode"]
            .as_str()
            .unwrap_or_default(),
    );
    prompt.push('\n');
    prompt.push_str("P\n");
    for project in payload["visible_projects"].as_array().into_iter().flatten() {
        prompt.push('[');
        prompt.push_str(project["project_code"].as_str().unwrap_or_default());
        prompt.push_str("] ");
        prompt.push_str(project["repo_root"].as_str().unwrap_or_default());
        prompt.push('\n');
    }
    prompt.push('\n');
    push_compact_lines(&mut prompt, "D", &exact_lines);
    push_compact_lines(&mut prompt, "S", &symbol_lines);
    push_compact_lines(&mut prompt, "E", &excerpt_lines);
    prompt
}

fn push_compact_lines(prompt: &mut String, title: &str, lines: &[String]) {
    prompt.push_str(title);
    prompt.push('\n');
    for line in lines {
        prompt.push_str(line);
        prompt.push('\n');
    }
    prompt.push('\n');
}

fn build_tokenizer(name: &str) -> Result<CoreBPE> {
    match name {
        "o200k_base" => o200k_base().context("failed to initialize o200k_base tokenizer"),
        "cl100k_base" => cl100k_base().context("failed to initialize cl100k_base tokenizer"),
        other => Err(anyhow!("unsupported tokenizer: {other}")),
    }
}
