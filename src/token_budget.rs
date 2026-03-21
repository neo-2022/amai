use crate::cli::{ContextPackArgs, ObserveTokenReportArgs};
use crate::config::{self, AppConfig};
use crate::language;
use crate::postgres::{self, ObservabilitySnapshotRecord};
use crate::retrieval;
use anyhow::{Context, Result, anyhow, bail};
use ignore::WalkBuilder;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
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
    session_id: String,
    rolling_window_profile: String,
    timestamp_utc: i64,
    snapshot_kind: String,
    source_kind: String,
    traffic_class: String,
    project: String,
    namespace: String,
    query: String,
    query_hash: String,
    query_type: String,
    target_kind: String,
    baseline_hit_target: bool,
    amai_hit_target: bool,
    cold_warm_state: String,
    baseline_strategy: String,
    retrieval_mode: Option<String>,
    tokenizer: String,
    latency_ms: f64,
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
    quality_tier: String,
    head_hit_target: bool,
    needed_followup: bool,
    followup_count: u64,
    followup_of_event_id: Option<String>,
    resolved_by_event_id: Option<String>,
    fallback_triggered: bool,
    fallback_count: u64,
    document_hits: u64,
    symbol_hits_count: u64,
    file_hits: u64,
    sources_count: u64,
    chunks_count: u64,
    pack_token_count: u64,
    deduped_token_count: u64,
}

#[derive(Debug, Clone)]
struct QualityVerdict {
    target_kind: &'static str,
    baseline_hit_target: bool,
    amai_hit_target: bool,
    quality_ok: bool,
    quality_score: f64,
    quality_method: &'static str,
    quality_tier: &'static str,
    head_hit_target: bool,
    needed_followup: bool,
    followup_count: u64,
}

#[derive(Debug, Clone, Copy)]
struct FollowupEventKey<'a> {
    query: &'a str,
    query_hash: &'a str,
    query_type: &'a str,
    target_kind: &'a str,
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

pub async fn repair_legacy_token_events(
    db: &Client,
    apply: bool,
    limit: Option<i64>,
) -> Result<()> {
    let rows =
        postgres::list_observability_snapshots_by_kinds(db, &["token_budget_event"], limit).await?;
    let mut scanned = 0_u64;
    let mut changed = 0_u64;

    for row in rows {
        scanned += 1;
        if let Some(payload) = repair_legacy_token_event_payload(&row.payload) {
            changed += 1;
            if apply {
                postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &payload)
                    .await?;
            }
        }
    }

    println!(
        "token ledger repair :: scanned={} changed={} mode={}",
        scanned,
        changed,
        if apply { "apply" } else { "dry_run" }
    );
    Ok(())
}

pub async fn reverify_legacy_live_events(
    cfg: &AppConfig,
    db: &mut Client,
    apply: bool,
    limit: Option<i64>,
) -> Result<()> {
    let rows =
        postgres::list_observability_snapshots_by_kinds(db, &["token_budget_event"], limit).await?;
    let repo_root = config::discover_repo_root(None)?;
    let measurement = load_config(&repo_root)?.measurement;
    let mut scanned = 0_u64;
    let mut eligible = 0_u64;
    let mut reverified = 0_u64;
    let mut quality_ok = 0_u64;
    let mut skipped = 0_u64;
    let mut failed = 0_u64;

    for row in rows {
        scanned += 1;
        if !needs_live_reverification(&row.payload) {
            skipped += 1;
            continue;
        }
        eligible += 1;

        match reverify_live_event_payload(cfg, db, &measurement, &row).await {
            Ok(Some(payload)) => {
                let node = &payload["token_budget_event"];
                if node["quality"]["quality_ok"].as_bool().unwrap_or(false) {
                    quality_ok += 1;
                }
                reverified += 1;
                if apply {
                    postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &payload)
                        .await?;
                }
            }
            Ok(None) => {
                skipped += 1;
            }
            Err(error) => {
                failed += 1;
                eprintln!(
                    "token ledger reverify failed: snapshot={} :: {}",
                    row.snapshot_id, error
                );
            }
        }
    }

    println!(
        "token ledger reverify :: scanned={} eligible={} reverified={} quality_ok={} skipped={} failed={} mode={}",
        scanned,
        eligible,
        reverified,
        quality_ok,
        skipped,
        failed,
        if apply { "apply" } else { "dry_run" }
    );
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
    let profile = resolve_profile(&config, None, &repo_root)?;
    let mut event = build_event_payload(
        payload,
        &config.measurement,
        "live_context_pack",
        "context_pack_token_budget",
    )?;
    enrich_live_event_payload(db, &mut event, &profile).await?;
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
                "quality_tier": "benchmark",
                "head_hit_target": true,
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
    let events = reconcile_followup_recovery(&events, profile.session_gap_minutes as i64 * 60_000);

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
    let query_slices = query_slice_breakdown(&events, &config.measurement);
    let temperature_slices = temperature_slice_breakdown(&events, &config.measurement);
    let current_session_summary =
        summarize_events(&session_events, now_epoch_ms, &config.measurement);
    let rolling_window_summary = if profile.rolling_window_hours.is_some() {
        summarize_events(&rolling_window_events, now_epoch_ms, &config.measurement)
    } else {
        json!(null)
    };
    let lifetime_summary = summarize_events(&events, now_epoch_ms, &config.measurement);
    let headline_summary = if profile.rolling_window_hours.is_some() {
        build_product_headline(
            &rolling_window_summary,
            &format!("окно {}", profile.display_name),
        )
    } else {
        build_product_headline(&lifetime_summary, "всё время записи")
    };

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
            "headline": headline_summary,
            "latest_event": latest_event,
            "current_session": current_session_summary,
            "rolling_window": rolling_window_summary,
            "lifetime": lifetime_summary,
            "source_breakdown": source_breakdown,
            "query_slices": query_slices,
            "temperature_slices": temperature_slices,
        }
    }))
}

async fn enrich_live_event_payload(
    db: &Client,
    payload: &mut Value,
    profile: &ResolvedProfile,
) -> Result<()> {
    let node = payload["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("token budget payload missing token_budget_event object"))?;
    let timestamp_utc = node
        .get("timestamp_utc")
        .and_then(Value::as_i64)
        .unwrap_or_else(|| current_epoch_ms().unwrap_or_default());
    let current_event_id = node
        .get("event_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let project = node
        .get("project")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let namespace = node
        .get("namespace")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query = node
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query_hash = node
        .get("query_hash")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let query_type = node
        .get("query_type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let target_kind = node
        .get("target_kind")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let session_gap_ms = profile.session_gap_minutes as i64 * 60_000;
    let mut events = load_events(db, false, Some(64)).await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let session_id = resolve_session_id(&events, timestamp_utc, session_gap_ms);
    node.insert("session_id".to_string(), Value::String(session_id));
    node.insert(
        "rolling_window_profile".to_string(),
        Value::String(profile.code.clone()),
    );
    node.insert(
        "budget_profile".to_string(),
        Value::String(profile.code.clone()),
    );

    let current_key = FollowupEventKey {
        query: &query,
        query_hash: &query_hash,
        query_type: &query_type,
        target_kind: &target_kind,
    };

    let candidate_rows =
        postgres::list_observability_snapshots_by_kinds(db, &["token_budget_event"], Some(64))
            .await?;
    let mut candidates = candidate_rows
        .into_iter()
        .filter_map(|row| {
            parse_snapshot_event(&row)
                .ok()
                .flatten()
                .filter(|event| event.traffic_class == "live")
                .map(|event| (row, event))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(_, event)| event.created_at_epoch_ms);

    if let Some((row, previous)) = candidates.into_iter().rev().find(|(_, previous)| {
        previous.traffic_class == "live"
            && previous.needed_followup
            && previous.resolved_by_event_id.is_none()
            && previous.project == project
            && previous.namespace == namespace
            && timestamp_utc.saturating_sub(previous.created_at_epoch_ms) <= session_gap_ms
            && followup_queries_related(followup_event_key(previous), current_key)
    }) {
        let previous_cost = previous
            .context_tokens
            .saturating_add(previous.recovery_tokens);
        set_recovery_penalty(
            payload,
            previous_cost,
            previous.followup_count.saturating_add(1),
        )?;
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
        let target_kind_owned = payload["token_budget_event"]["target_kind"]
            .as_str()
            .unwrap_or("file")
            .to_string();
        let node = payload["token_budget_event"]
            .as_object_mut()
            .ok_or_else(|| anyhow!("token budget payload missing token_budget_event object"))?;
        let followup = ensure_nested_object(node, "followup")?;
        followup.insert(
            "followup_of_event_id".to_string(),
            Value::String(previous.event_id.clone()),
        );
        let quality = ensure_nested_object(node, "quality")?;
        let quality_ok = quality
            .get("quality_ok")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let head_hit_target = quality
            .get("head_hit_target")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let answer_like_proxy = answer_like_from_counts(
            &target_kind_owned,
            head_hit_target,
            exact_hits,
            symbol_hits,
            lexical_hits,
            semantic_hits,
        );
        quality.insert(
            "quality_method".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "hybrid_answer_success".to_string()
                } else {
                    "hybrid_task_success".to_string()
                }
            } else {
                "hybrid_followup_pending".to_string()
            }),
        );
        quality.insert(
            "quality_tier".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "answer_success_recovered".to_string()
                } else {
                    "task_success_recovered".to_string()
                }
            } else {
                "partial".to_string()
            }),
        );

        let mut previous_payload = row.payload.clone();
        let previous_node = previous_payload["token_budget_event"]
            .as_object_mut()
            .ok_or_else(|| anyhow!("previous token budget payload missing token_budget_event"))?;
        let previous_followup = ensure_nested_object(previous_node, "followup")?;
        previous_followup.insert(
            "resolved_by_event_id".to_string(),
            Value::String(current_event_id),
        );
        previous_followup.insert("recovery_resolved".to_string(), Value::Bool(true));
        previous_followup.insert(
            "recovery_resolved_at_utc".to_string(),
            Value::from(timestamp_utc),
        );
        postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &previous_payload)
            .await?;
    }

    Ok(())
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
            if !include_traffic_class_in_report(&event.traffic_class, include_verify_events) {
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
    let project = node["project"]
        .as_str()
        .or_else(|| node["project_code"].as_str())
        .unwrap_or_default()
        .to_string();
    let namespace = node["namespace"]
        .as_str()
        .or_else(|| node["namespace_code"].as_str())
        .unwrap_or_default()
        .to_string();
    let query = node["query"].as_str().unwrap_or_default().to_string();
    let query_hash = node["query_hash"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| hex_sha256(query.as_bytes()));
    let query_type = node["query_type"]
        .as_str()
        .filter(|value| !value.is_empty() && *value != "unknown")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_query_type(&query).to_string());
    let target_kind = node["target_kind"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let baseline_hit_target = node["baseline_hit_target"].as_bool().unwrap_or(false);
    let amai_hit_target = node["amai_hit_target"].as_bool().unwrap_or(false);
    let cold_warm_state = node["cold_warm_state"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let baseline_strategy = node["baseline_strategy"]
        .as_str()
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_baseline_strategy(&query_type).to_string());
    let retrieval_mode = node["retrieval_mode"].as_str().map(ToOwned::to_owned);
    let tokenizer = node["tokenizer"].as_str().unwrap_or_default().to_string();
    let latency_ms = node["latency_ms"].as_f64().unwrap_or(0.0);
    let event_id = node["event_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("{}-{}", row.snapshot_kind, row.created_at_epoch_ms));
    let session_id = node["session_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_default();
    let rolling_window_profile = node["rolling_window_profile"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_default();
    let timestamp_utc = node["timestamp_utc"]
        .as_i64()
        .unwrap_or(row.created_at_epoch_ms);
    let saved_tokens = node["savings"]["saved_tokens"].as_u64().unwrap_or(0);
    let naive_tokens = node["naive_scope"]["tokens"]
        .as_u64()
        .or_else(|| node["baseline_tokens"].as_u64())
        .unwrap_or(0);
    let context_tokens = node["context_pack_render"]["tokens"]
        .as_u64()
        .or_else(|| node["delivered_tokens"].as_u64())
        .unwrap_or(0);
    let recovery_tokens = node["recovery"]["recovery_tokens"].as_u64().unwrap_or(0);
    let effective_saved_tokens = node["savings"]["effective_saved_tokens"]
        .as_i64()
        .unwrap_or_else(|| naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64));
    let savings_factor = node["savings"]["savings_factor"].as_f64().unwrap_or(0.0);
    let savings_percent = node["savings"]["savings_percent"]
        .as_f64()
        .or_else(|| node["gross_savings_pct"].as_f64())
        .unwrap_or(0.0);
    let effective_savings_percent = node["savings"]["effective_savings_percent"]
        .as_f64()
        .unwrap_or_else(|| percent_from_signed(effective_saved_tokens, naive_tokens));
    let quality_ok = node["quality"]["quality_ok"].as_bool().unwrap_or(false);
    let quality_score = node["quality"]["quality_score"]
        .as_f64()
        .unwrap_or(if quality_ok { 1.0 } else { 0.0 });
    let quality_method = node["quality"]["quality_method"]
        .as_str()
        .unwrap_or(if node["quality"].is_object() {
            "unknown"
        } else {
            "legacy_unverified"
        })
        .to_string();
    let quality_tier = node["quality"]["quality_tier"]
        .as_str()
        .unwrap_or("unknown")
        .to_string();
    let head_hit_target = node["quality"]["head_hit_target"]
        .as_bool()
        .unwrap_or(false);
    let needed_followup = node["followup"]["needed_followup"]
        .as_bool()
        .unwrap_or(!quality_ok);
    let followup_count = node["followup"]["followup_count"].as_u64().unwrap_or(0);
    let followup_of_event_id = node["followup"]["followup_of_event_id"]
        .as_str()
        .map(ToOwned::to_owned);
    let resolved_by_event_id = node["followup"]["resolved_by_event_id"]
        .as_str()
        .map(ToOwned::to_owned);
    let fallback_triggered = node["recovery"]["fallback_triggered"]
        .as_bool()
        .unwrap_or(false);
    let fallback_count = node["recovery"]["fallback_count"].as_u64().unwrap_or(0);
    let document_hits = node["shape"]["document_hits"].as_u64().unwrap_or(0);
    let symbol_hits_count = node["shape"]["symbol_hits"].as_u64().unwrap_or(0);
    let file_hits = node["shape"]["file_hits"].as_u64().unwrap_or(0);
    let sources_count = node["shape"]["sources_count"].as_u64().unwrap_or(0);
    let chunks_count = node["shape"]["chunks_count"].as_u64().unwrap_or(0);
    let pack_token_count = node["shape"]["pack_token_count"]
        .as_u64()
        .unwrap_or(context_tokens);
    let deduped_token_count = node["shape"]["deduped_token_count"]
        .as_u64()
        .unwrap_or(context_tokens);

    Ok(Some(TokenBudgetEvent {
        created_at_epoch_ms: row.created_at_epoch_ms,
        event_id,
        session_id,
        rolling_window_profile,
        timestamp_utc,
        snapshot_kind: row.snapshot_kind.clone(),
        source_kind,
        traffic_class,
        project,
        namespace,
        query,
        query_hash,
        query_type,
        target_kind,
        baseline_hit_target,
        amai_hit_target,
        cold_warm_state,
        baseline_strategy,
        retrieval_mode,
        tokenizer,
        latency_ms,
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
        quality_tier,
        head_hit_target,
        needed_followup,
        followup_count,
        followup_of_event_id,
        resolved_by_event_id,
        fallback_triggered,
        fallback_count,
        document_hits,
        symbol_hits_count,
        file_hits,
        sources_count,
        chunks_count,
        pack_token_count,
        deduped_token_count,
    }))
}

fn needs_live_reverification(payload: &Value) -> bool {
    let node = &payload["token_budget_event"];
    if !node.is_object() {
        return false;
    }
    let source_kind = node["source_kind"].as_str().unwrap_or_default();
    let traffic_class = node["traffic_class"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_traffic_class(source_kind));
    if traffic_class != "live" {
        return false;
    }
    let quality_method = node["quality"]["quality_method"]
        .as_str()
        .unwrap_or_default();
    let quality_ok = node["quality"]["quality_ok"].as_bool().unwrap_or(false);
    let needs_shape_upgrade = node["target_kind"]
        .as_str()
        .map(|value| value.is_empty() || value == "unknown")
        .unwrap_or(true)
        || node.get("latency_ms").is_none()
        || node["quality"].get("quality_tier").is_none()
        || node["quality"].get("head_hit_target").is_none()
        || node["shape"].get("pack_token_count").is_none()
        || node["shape"].get("deduped_token_count").is_none()
        || node["followup"].is_null()
        || node["shape"].get("file_hits").is_none();
    quality_method == "legacy_unverified"
        || (quality_method.is_empty() && !quality_ok)
        || needs_shape_upgrade
}

async fn reverify_live_event_payload(
    cfg: &AppConfig,
    db: &mut Client,
    measurement: &MeasurementConfig,
    row: &ObservabilitySnapshotRecord,
) -> Result<Option<Value>> {
    let node = &row.payload["token_budget_event"];
    if !node.is_object() {
        return Ok(None);
    }

    let project = node["project"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("token event missing project"))?;
    let namespace = node["namespace"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("token event missing namespace"))?;
    let query = node["query"]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("token event missing query"))?;

    let args = ContextPackArgs {
        project: project.to_string(),
        namespace: namespace.to_string(),
        query: query.to_string(),
        retrieval_mode: node["retrieval_mode"].as_str().map(ToOwned::to_owned),
        disable_cache: false,
        limit_documents: 5,
        limit_symbols: 8,
        limit_chunks: 8,
        limit_semantic_chunks: 8,
    };

    let result =
        retrieval::execute_context_pack_capture_with_options(cfg, db, &args, false, false).await?;
    let source_kind = node["source_kind"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("live_context_pack");
    let mut rebuilt = build_event_payload(
        &result.payload,
        measurement,
        source_kind,
        "reverified_live_context_pack",
    )?;
    apply_reverification_metadata(&mut rebuilt, node, row.created_at_epoch_ms)?;
    Ok(Some(rebuilt))
}

fn apply_reverification_metadata(
    rebuilt_payload: &mut Value,
    original_node: &Value,
    fallback_timestamp_utc: i64,
) -> Result<()> {
    let target_kind_owned = rebuilt_payload["token_budget_event"]["target_kind"]
        .as_str()
        .unwrap_or("file")
        .to_string();
    let exact_hits = rebuilt_payload["retrieval"]["exact_documents"]
        .as_array()
        .map_or(0, Vec::len);
    let symbol_hits = rebuilt_payload["retrieval"]["symbol_hits"]
        .as_array()
        .map_or(0, Vec::len);
    let lexical_hits = rebuilt_payload["retrieval"]["lexical_chunks"]
        .as_array()
        .map_or(0, Vec::len);
    let semantic_hits = rebuilt_payload["retrieval"]["semantic_chunks"]
        .as_array()
        .map_or(0, Vec::len);
    let rebuilt_node = rebuilt_payload["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("rebuilt token event payload missing token_budget_event object"))?;

    let event_id = original_node["event_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let timestamp_utc = original_node["timestamp_utc"]
        .as_i64()
        .unwrap_or(fallback_timestamp_utc);
    let source_kind = original_node["source_kind"]
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or("live_context_pack");
    let quality_ok = rebuilt_node
        .get("quality")
        .and_then(|value| value.get("quality_ok"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let reverified_at_utc = current_epoch_ms()?;

    rebuilt_node.insert("event_id".to_string(), Value::String(event_id));
    rebuilt_node.insert("timestamp_utc".to_string(), Value::from(timestamp_utc));
    rebuilt_node.insert(
        "source_kind".to_string(),
        Value::String(source_kind.to_string()),
    );
    rebuilt_node.insert(
        "traffic_class".to_string(),
        Value::String(derive_traffic_class(source_kind)),
    );
    rebuilt_node.insert(
        "payload_origin".to_string(),
        Value::String("reverified_live_context_pack".to_string()),
    );
    if let Some(quality) = rebuilt_node
        .get_mut("quality")
        .and_then(Value::as_object_mut)
    {
        let head_hit_target = quality
            .get("head_hit_target")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let answer_like_proxy = answer_like_from_counts(
            &target_kind_owned,
            head_hit_target,
            exact_hits,
            symbol_hits,
            lexical_hits,
            semantic_hits,
        );
        quality.insert(
            "quality_method".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "reverified_answer_proxy".to_string()
                } else if head_hit_target {
                    "reverified_task_proxy".to_string()
                } else {
                    "reverified_retrieval_parity".to_string()
                }
            } else {
                "reverified_retrieval_miss".to_string()
            }),
        );
        quality.insert(
            "quality_tier".to_string(),
            Value::String(if quality_ok {
                if answer_like_proxy {
                    "answer_proxy".to_string()
                } else if head_hit_target {
                    "task_proxy".to_string()
                } else {
                    "retrieval".to_string()
                }
            } else {
                "partial".to_string()
            }),
        );
        quality.insert(
            "reverified_at_utc".to_string(),
            Value::from(reverified_at_utc),
        );
    }
    rebuilt_node.insert(
        "reverification".to_string(),
        json!({
            "reverified_at_utc": reverified_at_utc,
            "previous_quality_method": original_node["quality"]["quality_method"]
                .as_str()
                .unwrap_or("missing"),
            "previous_quality_ok": original_node["quality"]["quality_ok"]
                .as_bool()
                .unwrap_or(false),
        }),
    );
    Ok(())
}

fn repair_legacy_token_event_payload(payload: &Value) -> Option<Value> {
    let mut updated = payload.clone();
    let node = updated.get_mut("token_budget_event")?;
    let object = node.as_object_mut()?;
    let source_kind = object
        .get("source_kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let query = object
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let query_type = object
        .get("query_type")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && *value != "unknown")
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| derive_query_type(query).to_string());
    let mut changed = false;

    if !object.contains_key("traffic_class") {
        object.insert(
            "traffic_class".to_string(),
            Value::String(derive_traffic_class(source_kind)),
        );
        changed = true;
    }
    if !object.contains_key("query_type") {
        object.insert("query_type".to_string(), Value::String(query_type.clone()));
        changed = true;
    }
    if !object.contains_key("baseline_strategy") {
        object.insert(
            "baseline_strategy".to_string(),
            Value::String(derive_baseline_strategy(&query_type).to_string()),
        );
        changed = true;
    }
    if !object.contains_key("recovery") {
        object.insert(
            "recovery".to_string(),
            json!({
                "recovery_tokens": 0,
                "fallback_triggered": false,
                "fallback_count": 0
            }),
        );
        changed = true;
    }
    if !object.contains_key("shape") {
        object.insert(
            "shape".to_string(),
            json!({
                "sources_count": 0,
                "chunks_count": 0
            }),
        );
        changed = true;
    }
    if !object.contains_key("quality") {
        object.insert(
            "quality".to_string(),
            json!({
                "quality_ok": false,
                "quality_score": 0.0,
                "quality_method": "legacy_unverified",
                "quality_tier": "unverified",
                "head_hit_target": false
            }),
        );
        changed = true;
    }
    let naive_tokens = object
        .get("naive_scope")
        .and_then(|value| value.get("tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let context_tokens = object
        .get("context_pack_render")
        .and_then(|value| value.get("tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let recovery_tokens = object
        .get("recovery")
        .and_then(|value| value.get("recovery_tokens"))
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if let Some(savings) = object.get_mut("savings").and_then(Value::as_object_mut) {
        if !savings.contains_key("effective_saved_tokens") {
            savings.insert(
                "effective_saved_tokens".to_string(),
                Value::from(naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64)),
            );
            changed = true;
        }
        if !savings.contains_key("effective_savings_percent") {
            let effective_saved_tokens = savings
                .get("effective_saved_tokens")
                .and_then(Value::as_i64)
                .unwrap_or(naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64));
            savings.insert(
                "effective_savings_percent".to_string(),
                Value::from(percent_from_signed(effective_saved_tokens, naive_tokens)),
            );
            changed = true;
        }
    }

    changed.then_some(updated)
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

fn resolve_session_id(events: &[TokenBudgetEvent], current_ts: i64, session_gap_ms: i64) -> String {
    events
        .iter()
        .rev()
        .find(|event| {
            event.traffic_class == "live"
                && current_ts.saturating_sub(event.created_at_epoch_ms) <= session_gap_ms
        })
        .map(|event| {
            if event.session_id.is_empty() {
                event.event_id.clone()
            } else {
                event.session_id.clone()
            }
        })
        .unwrap_or_else(|| Uuid::new_v4().to_string())
}

fn set_recovery_penalty(
    payload: &mut Value,
    recovery_tokens: u64,
    followup_count: u64,
) -> Result<()> {
    let node = payload["token_budget_event"]
        .as_object_mut()
        .ok_or_else(|| anyhow!("token budget payload missing token_budget_event object"))?;
    let recovery = ensure_nested_object(node, "recovery")?;
    recovery.insert("recovery_tokens".to_string(), Value::from(recovery_tokens));
    let followup = ensure_nested_object(node, "followup")?;
    followup.insert("followup_count".to_string(), Value::from(followup_count));

    let context_tokens = node["context_pack_render"]["tokens"].as_u64().unwrap_or(0);
    let naive_tokens = node["naive_scope"]["tokens"].as_u64().unwrap_or(0);
    let effective_saved_tokens =
        naive_tokens as i64 - (context_tokens as i64 + recovery_tokens as i64);
    let effective_savings_percent = percent_from_signed(effective_saved_tokens, naive_tokens);
    let savings = ensure_nested_object(node, "savings")?;
    savings.insert(
        "effective_saved_tokens".to_string(),
        Value::from(effective_saved_tokens),
    );
    savings.insert(
        "effective_savings_percent".to_string(),
        Value::from(effective_savings_percent),
    );
    Ok(())
}

fn ensure_nested_object<'a>(
    parent: &'a mut serde_json::Map<String, Value>,
    key: &str,
) -> Result<&'a mut serde_json::Map<String, Value>> {
    if !parent.get(key).is_some_and(Value::is_object) {
        parent.insert(key.to_string(), json!({}));
    }
    parent
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .ok_or_else(|| anyhow!("payload field {key} is not an object"))
}

fn reconcile_followup_recovery(
    events: &[TokenBudgetEvent],
    session_gap_ms: i64,
) -> Vec<TokenBudgetEvent> {
    let mut reconciled = events.to_vec();
    for current_index in 1..reconciled.len() {
        if reconciled[current_index].traffic_class != "live"
            || reconciled[current_index].followup_of_event_id.is_some()
        {
            continue;
        }
        let current_ts = reconciled[current_index].created_at_epoch_ms;
        let current_project = reconciled[current_index].project.clone();
        let current_namespace = reconciled[current_index].namespace.clone();
        let current_key = followup_event_key(&reconciled[current_index]);

        for previous_index in (0..current_index).rev() {
            if reconciled[previous_index].traffic_class != "live"
                || !reconciled[previous_index].needed_followup
                || reconciled[previous_index].resolved_by_event_id.is_some()
            {
                continue;
            }
            if current_ts.saturating_sub(reconciled[previous_index].created_at_epoch_ms)
                > session_gap_ms
            {
                break;
            }
            if reconciled[previous_index].project != current_project
                || reconciled[previous_index].namespace != current_namespace
            {
                continue;
            }
            if !followup_queries_related(
                followup_event_key(&reconciled[previous_index]),
                current_key,
            ) {
                continue;
            }
            let recovery_tokens = reconciled[current_index].recovery_tokens.saturating_add(
                reconciled[previous_index]
                    .context_tokens
                    .saturating_add(reconciled[previous_index].recovery_tokens),
            );
            reconciled[current_index].recovery_tokens = recovery_tokens;
            reconciled[current_index].followup_count =
                reconciled[previous_index].followup_count.saturating_add(1);
            reconciled[current_index].followup_of_event_id =
                Some(reconciled[previous_index].event_id.clone());
            reconciled[current_index].effective_saved_tokens = reconciled[current_index]
                .naive_tokens as i64
                - (reconciled[current_index].context_tokens as i64 + recovery_tokens as i64);
            reconciled[current_index].effective_savings_percent = percent_from_signed(
                reconciled[current_index].effective_saved_tokens,
                reconciled[current_index].naive_tokens,
            );
            reconciled[previous_index].resolved_by_event_id =
                Some(reconciled[current_index].event_id.clone());
            break;
        }
    }
    reconciled
}

fn followup_event_key(event: &TokenBudgetEvent) -> FollowupEventKey<'_> {
    FollowupEventKey {
        query: &event.query,
        query_hash: &event.query_hash,
        query_type: &event.query_type,
        target_kind: &event.target_kind,
    }
}

fn followup_queries_related(current: FollowupEventKey<'_>, follower: FollowupEventKey<'_>) -> bool {
    if !current.query_hash.is_empty() && current.query_hash == follower.query_hash {
        return true;
    }
    if current.query_type != follower.query_type {
        return false;
    }
    if current.target_kind != follower.target_kind {
        return false;
    }
    if normalized_query(current.query) == normalized_query(follower.query) {
        return true;
    }
    query_terms_overlap_count(current.query, follower.query) >= 2
}

fn query_terms_overlap_count(left: &str, right: &str) -> usize {
    let left_terms = extract_query_terms(left);
    if left_terms.is_empty() {
        return 0;
    }
    let right_terms = extract_query_terms(right);
    if right_terms.is_empty() {
        return 0;
    }
    let right_set = right_terms.into_iter().collect::<HashSet<_>>();
    left_terms
        .into_iter()
        .filter(|term| right_set.contains(term))
        .count()
}

fn normalized_query(query: &str) -> String {
    extract_query_terms(query).join(" ")
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
            "task_success_like_counted_events": 0,
            "answer_like_counted_events": 0,
            "legacy_unverified_events": 0,
            "preliminary": true,
            "baseline_tokens": 0,
            "delivered_tokens": 0,
            "recovery_tokens": 0,
            "effective_saved_tokens": 0,
            "total_saved_tokens": 0,
            "total_effective_saved_tokens": 0,
            "verified_effective_saved_tokens": 0,
            "verified_task_like_saved_tokens": 0,
            "verified_answer_like_saved_tokens": 0,
            "total_naive_tokens": 0,
            "total_context_tokens": 0,
            "total_recovery_tokens": 0,
            "gross_savings_pct": 0.0,
            "effective_savings_pct": 0.0,
            "verified_effective_savings_pct": 0.0,
            "verified_task_like_savings_pct": 0.0,
            "verified_answer_like_savings_pct": 0.0,
            "savings_percent": 0.0,
            "savings_factor": 0.0,
            "avg_saved_tokens_per_event": 0.0,
            "quality_ok_rate": 0.0,
            "task_success_like_rate": 0.0,
            "answer_like_rate": 0.0,
            "fallback_rate": 0.0,
            "median_recovery_tokens": 0.0,
            "p95_latency_ms": 0.0,
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
    let task_like_events = verified_events
        .iter()
        .copied()
        .filter(|event| {
            matches!(
                event.quality_tier.as_str(),
                "task_proxy"
                    | "task_success_recovered"
                    | "answer_proxy"
                    | "answer_success_recovered"
            )
        })
        .collect::<Vec<_>>();
    let answer_like_events = verified_events
        .iter()
        .copied()
        .filter(|event| is_answer_like_event(event))
        .collect::<Vec<_>>();
    let verified_task_like_saved_tokens = task_like_events
        .iter()
        .map(|event| event.effective_saved_tokens)
        .sum::<i64>();
    let verified_task_like_baseline_tokens = task_like_events
        .iter()
        .map(|event| event.naive_tokens)
        .sum::<u64>();
    let verified_answer_like_saved_tokens = answer_like_events
        .iter()
        .map(|event| event.effective_saved_tokens)
        .sum::<i64>();
    let verified_answer_like_baseline_tokens = answer_like_events
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
    let verified_task_like_savings_pct = percent_from_signed(
        verified_task_like_saved_tokens,
        verified_task_like_baseline_tokens,
    );
    let verified_answer_like_savings_pct = percent_from_signed(
        verified_answer_like_saved_tokens,
        verified_answer_like_baseline_tokens,
    );
    let savings_factor = if total_context_tokens == 0 {
        total_naive_tokens as f64
    } else {
        total_naive_tokens as f64 / total_context_tokens as f64
    };
    let avg_saved_tokens_per_event = total_saved_tokens as f64 / events.len() as f64;
    let quality_ok_events = events.iter().filter(|event| event.quality_ok).count() as f64;
    let task_success_like_events = events
        .iter()
        .filter(|event| {
            matches!(
                event.quality_tier.as_str(),
                "task_proxy"
                    | "task_success_recovered"
                    | "answer_proxy"
                    | "answer_success_recovered"
            )
        })
        .count() as f64;
    let answer_like_events_rate = events
        .iter()
        .filter(|event| is_answer_like_event(event))
        .count() as f64;
    let legacy_unverified_events = events
        .iter()
        .filter(|event| event.quality_method == "legacy_unverified")
        .count();
    let fallback_events = events
        .iter()
        .filter(|event| event.fallback_triggered)
        .count() as f64;
    let mut recovery_values = events
        .iter()
        .map(|event| event.recovery_tokens as f64)
        .collect::<Vec<_>>();
    recovery_values
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let median_recovery_tokens = percentile_from_sorted(&recovery_values, 0.5);
    let mut latency_values = events
        .iter()
        .map(|event| event.latency_ms)
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    latency_values
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let p95_latency_ms = percentile_from_sorted(&latency_values, 0.95);
    let quality_ok_rate = quality_ok_events * 100.0 / events.len() as f64;
    let task_success_like_rate = task_success_like_events * 100.0 / events.len() as f64;
    let answer_like_rate = answer_like_events_rate * 100.0 / events.len() as f64;
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
        && total_naive_tokens < measurement.preliminary_min_baseline_tokens;

    json!({
        "events_total": events.len(),
        "events_count": events.len(),
        "counted_events": verified_events.len(),
        "task_success_like_counted_events": task_like_events.len(),
        "answer_like_counted_events": answer_like_events.len(),
        "legacy_unverified_events": legacy_unverified_events,
        "preliminary": preliminary,
        "baseline_tokens": total_naive_tokens,
        "delivered_tokens": total_context_tokens,
        "recovery_tokens": total_recovery_tokens,
        "effective_saved_tokens": total_effective_saved_tokens,
        "total_saved_tokens": total_saved_tokens,
        "total_effective_saved_tokens": total_effective_saved_tokens,
        "verified_effective_saved_tokens": verified_effective_saved_tokens,
        "verified_task_like_saved_tokens": verified_task_like_saved_tokens,
        "verified_answer_like_saved_tokens": verified_answer_like_saved_tokens,
        "total_naive_tokens": total_naive_tokens,
        "total_context_tokens": total_context_tokens,
        "total_recovery_tokens": total_recovery_tokens,
        "gross_savings_pct": gross_savings_pct,
        "effective_savings_pct": effective_savings_pct,
        "verified_effective_savings_pct": verified_effective_savings_pct,
        "verified_task_like_savings_pct": verified_task_like_savings_pct,
        "verified_answer_like_savings_pct": verified_answer_like_savings_pct,
        "savings_percent": gross_savings_pct,
        "savings_factor": savings_factor,
        "avg_saved_tokens_per_event": avg_saved_tokens_per_event,
        "quality_ok_rate": quality_ok_rate,
        "task_success_like_rate": task_success_like_rate,
        "answer_like_rate": answer_like_rate,
        "fallback_rate": fallback_rate,
        "median_recovery_tokens": median_recovery_tokens,
        "p95_latency_ms": p95_latency_ms,
        "started_at_epoch_ms": started_at_epoch_ms,
        "ended_at_epoch_ms": ended_at_epoch_ms,
        "age_ms_since_latest": now_epoch_ms.saturating_sub(ended_at_epoch_ms),
    })
}

fn build_product_headline(summary: &Value, scope_label: &str) -> Value {
    let events_total = summary["events_total"].as_u64().unwrap_or(0);
    let counted_events = summary["counted_events"].as_u64().unwrap_or(0);
    let legacy_unverified_events = summary["legacy_unverified_events"].as_u64().unwrap_or(0);
    let preliminary = summary["preliminary"].as_bool().unwrap_or(true);
    let verified_percent = summary["verified_effective_savings_pct"]
        .as_f64()
        .unwrap_or(0.0);
    let effective_percent = summary["effective_savings_pct"].as_f64().unwrap_or(0.0);
    let verified_saved_tokens = summary["verified_effective_saved_tokens"]
        .as_i64()
        .unwrap_or(0);
    let effective_saved_tokens = summary["total_effective_saved_tokens"]
        .as_i64()
        .unwrap_or(0);
    let quality_ok_rate = summary["quality_ok_rate"].as_f64().unwrap_or(0.0);
    let fallback_rate = summary["fallback_rate"].as_f64().unwrap_or(0.0);

    if counted_events > 0 {
        json!({
            "metric_code": "verified_effective_savings_pct",
            "title": "Проверенная реальная экономия",
            "scope_label": scope_label,
            "status": if preliminary { "alert" } else { "pass" },
            "preliminary": preliminary,
            "value_percent": verified_percent,
            "saved_tokens": verified_saved_tokens,
            "events_count": events_total,
            "counted_events": counted_events,
            "quality_ok_rate": quality_ok_rate,
            "fallback_rate": fallback_rate,
            "note": if preliminary {
                "Это уже quality-gated метрика, но выборка пока ещё маленькая."
            } else {
                "Это главный честный KPI: live-only, quality-gated и с учётом recovery."
            },
        })
    } else if events_total > 0 {
        json!({
            "metric_code": "effective_savings_pct_preliminary",
            "title": "Реальная экономия пока предварительно",
            "scope_label": scope_label,
            "status": "alert",
            "preliminary": true,
            "value_percent": effective_percent,
            "saved_tokens": effective_saved_tokens,
            "events_count": events_total,
            "counted_events": counted_events,
            "quality_ok_rate": quality_ok_rate,
            "fallback_rate": fallback_rate,
            "note": if legacy_unverified_events > 0 {
                "Проверенная выборка ещё не набрана: часть исторических live-событий была записана старым форматом без quality-блока, поэтому пока показывается общая реальная экономия."
            } else {
                "Проверенная выборка ещё не набрана, поэтому временно показывается общая реальная экономия по live-событиям."
            },
        })
    } else {
        json!({
            "metric_code": "no_live_events",
            "title": "Реальная экономия пока не накоплена",
            "scope_label": scope_label,
            "status": "unknown",
            "preliminary": true,
            "value_percent": 0.0,
            "saved_tokens": 0,
            "events_count": 0,
            "counted_events": 0,
            "quality_ok_rate": 0.0,
            "fallback_rate": 0.0,
            "note": "Amai ещё не накопил live-события для этой метрики.",
        })
    }
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

fn query_slice_breakdown(events: &[TokenBudgetEvent], measurement: &MeasurementConfig) -> Value {
    let mut grouped = BTreeMap::<String, Vec<TokenBudgetEvent>>::new();
    for event in events {
        grouped
            .entry(event.query_type.clone())
            .or_default()
            .push(event.clone());
    }
    Value::Array(
        grouped
            .into_iter()
            .map(|(query_type, items)| {
                let summary = summarize_events(
                    &items,
                    items
                        .last()
                        .map(|item| item.created_at_epoch_ms)
                        .unwrap_or_default(),
                    measurement,
                );
                json!({
                    "query_type": query_type,
                    "events_count": summary["events_count"],
                    "counted_events": summary["counted_events"],
                    "task_success_like_counted_events": summary["task_success_like_counted_events"],
                    "answer_like_counted_events": summary["answer_like_counted_events"],
                    "verified_effective_savings_pct": summary["verified_effective_savings_pct"],
                    "verified_task_like_savings_pct": summary["verified_task_like_savings_pct"],
                    "verified_answer_like_savings_pct": summary["verified_answer_like_savings_pct"],
                    "quality_ok_rate": summary["quality_ok_rate"],
                    "task_success_like_rate": summary["task_success_like_rate"],
                    "answer_like_rate": summary["answer_like_rate"],
                    "fallback_rate": summary["fallback_rate"],
                    "p95_latency_ms": summary["p95_latency_ms"],
                })
            })
            .collect(),
    )
}

fn temperature_slice_breakdown(
    events: &[TokenBudgetEvent],
    measurement: &MeasurementConfig,
) -> Value {
    let mut grouped = BTreeMap::<String, Vec<TokenBudgetEvent>>::new();
    for event in events {
        grouped
            .entry(event.cold_warm_state.clone())
            .or_default()
            .push(event.clone());
    }
    Value::Array(
        grouped
            .into_iter()
            .map(|(state, items)| {
                let summary = summarize_events(
                    &items,
                    items
                        .last()
                        .map(|item| item.created_at_epoch_ms)
                        .unwrap_or_default(),
                    measurement,
                );
                json!({
                    "state": state,
                    "events_count": summary["events_count"],
                    "counted_events": summary["counted_events"],
                    "verified_effective_savings_pct": summary["verified_effective_savings_pct"],
                    "median_recovery_tokens": summary["median_recovery_tokens"],
                    "p95_latency_ms": summary["p95_latency_ms"],
                })
            })
            .collect(),
    )
}

fn percentile_from_sorted(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let percentile = percentile.clamp(0.0, 1.0);
    let index = ((values.len() - 1) as f64 * percentile).ceil() as usize;
    values[index.min(values.len() - 1)]
}

fn event_to_json(event: &TokenBudgetEvent) -> Value {
    let mut object = serde_json::Map::new();
    object.insert(
        "created_at_epoch_ms".to_string(),
        Value::from(event.created_at_epoch_ms),
    );
    object.insert(
        "event_id".to_string(),
        Value::String(event.event_id.clone()),
    );
    object.insert(
        "session_id".to_string(),
        Value::String(event.session_id.clone()),
    );
    object.insert(
        "rolling_window_profile".to_string(),
        Value::String(event.rolling_window_profile.clone()),
    );
    object.insert(
        "timestamp_utc".to_string(),
        Value::from(event.timestamp_utc),
    );
    object.insert(
        "snapshot_kind".to_string(),
        Value::String(event.snapshot_kind.clone()),
    );
    object.insert(
        "source_kind".to_string(),
        Value::String(event.source_kind.clone()),
    );
    object.insert(
        "traffic_class".to_string(),
        Value::String(event.traffic_class.clone()),
    );
    object.insert("project".to_string(), Value::String(event.project.clone()));
    object.insert(
        "project_code".to_string(),
        Value::String(event.project.clone()),
    );
    object.insert(
        "namespace".to_string(),
        Value::String(event.namespace.clone()),
    );
    object.insert(
        "namespace_code".to_string(),
        Value::String(event.namespace.clone()),
    );
    object.insert("query".to_string(), Value::String(event.query.clone()));
    object.insert(
        "query_hash".to_string(),
        Value::String(event.query_hash.clone()),
    );
    object.insert(
        "query_type".to_string(),
        Value::String(event.query_type.clone()),
    );
    object.insert(
        "target_kind".to_string(),
        Value::String(event.target_kind.clone()),
    );
    object.insert(
        "baseline_hit_target".to_string(),
        Value::Bool(event.baseline_hit_target),
    );
    object.insert(
        "amai_hit_target".to_string(),
        Value::Bool(event.amai_hit_target),
    );
    object.insert(
        "cold_warm_state".to_string(),
        Value::String(event.cold_warm_state.clone()),
    );
    object.insert(
        "baseline_strategy".to_string(),
        Value::String(event.baseline_strategy.clone()),
    );
    object.insert(
        "retrieval_mode".to_string(),
        event
            .retrieval_mode
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "tokenizer".to_string(),
        Value::String(event.tokenizer.clone()),
    );
    object.insert("latency_ms".to_string(), Value::from(event.latency_ms));
    object.insert("saved_tokens".to_string(), Value::from(event.saved_tokens));
    object.insert("naive_tokens".to_string(), Value::from(event.naive_tokens));
    object.insert(
        "baseline_tokens".to_string(),
        Value::from(event.naive_tokens),
    );
    object.insert(
        "context_tokens".to_string(),
        Value::from(event.context_tokens),
    );
    object.insert(
        "delivered_tokens".to_string(),
        Value::from(event.context_tokens),
    );
    object.insert(
        "recovery_tokens".to_string(),
        Value::from(event.recovery_tokens),
    );
    object.insert(
        "effective_saved_tokens".to_string(),
        Value::from(event.effective_saved_tokens),
    );
    object.insert(
        "savings_factor".to_string(),
        Value::from(event.savings_factor),
    );
    object.insert(
        "savings_percent".to_string(),
        Value::from(event.savings_percent),
    );
    object.insert(
        "gross_savings_pct".to_string(),
        Value::from(event.savings_percent),
    );
    object.insert(
        "effective_savings_percent".to_string(),
        Value::from(event.effective_savings_percent),
    );
    object.insert("quality_ok".to_string(), Value::Bool(event.quality_ok));
    object.insert(
        "quality_score".to_string(),
        Value::from(event.quality_score),
    );
    object.insert(
        "answer_like_proxy".to_string(),
        Value::Bool(is_answer_like_event(event)),
    );
    object.insert(
        "quality_method".to_string(),
        Value::String(event.quality_method.clone()),
    );
    object.insert(
        "quality_tier".to_string(),
        Value::String(event.quality_tier.clone()),
    );
    object.insert(
        "head_hit_target".to_string(),
        Value::Bool(event.head_hit_target),
    );
    object.insert(
        "needed_followup".to_string(),
        Value::Bool(event.needed_followup),
    );
    object.insert(
        "followup_count".to_string(),
        Value::from(event.followup_count),
    );
    object.insert(
        "followup_of_event_id".to_string(),
        event
            .followup_of_event_id
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "resolved_by_event_id".to_string(),
        event
            .resolved_by_event_id
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "fallback_triggered".to_string(),
        Value::Bool(event.fallback_triggered),
    );
    object.insert(
        "fallback_count".to_string(),
        Value::from(event.fallback_count),
    );
    object.insert(
        "document_hits".to_string(),
        Value::from(event.document_hits),
    );
    object.insert(
        "symbol_hits_count".to_string(),
        Value::from(event.symbol_hits_count),
    );
    object.insert("file_hits".to_string(), Value::from(event.file_hits));
    object.insert(
        "sources_count".to_string(),
        Value::from(event.sources_count),
    );
    object.insert("chunks_count".to_string(), Value::from(event.chunks_count));
    object.insert(
        "pack_token_count".to_string(),
        Value::from(event.pack_token_count),
    );
    object.insert(
        "deduped_token_count".to_string(),
        Value::from(event.deduped_token_count),
    );
    Value::Object(object)
}

fn build_event_payload(
    payload: &Value,
    measurement: &MeasurementConfig,
    source_kind: &str,
    payload_origin: &str,
) -> Result<Value> {
    let tokenizer = build_tokenizer(&measurement.tokenizer)?;
    let query = payload["query"].as_str().unwrap_or_default();
    let query_type = derive_query_type(query);
    let baseline_strategy = derive_baseline_strategy(query_type);
    let naive_scope = collect_naive_scope(
        payload,
        measurement.naive_limit_files,
        measurement.naive_max_bytes_per_file,
        baseline_strategy,
        query,
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
    let quality = derive_quality_verdict(payload, query_type, &naive_scope);
    let fallback_count = count_lexical_fallback_chunks(payload) as u64;
    let fallback_triggered = fallback_count > 0;
    let document_hits = payload["retrieval"]["exact_documents"]
        .as_array()
        .map_or(0, Vec::len) as u64;
    let symbol_hits = payload["retrieval"]["symbol_hits"]
        .as_array()
        .map_or(0, Vec::len) as u64;
    let file_hits = unique_file_hit_count(payload) as u64;
    let sources_count = count_sources(payload) as u64;
    let chunks_count = count_chunks(payload) as u64;
    let traffic_class = derive_traffic_class(source_kind);
    let event_id = payload["context_pack_id"]
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let timestamp_utc = current_epoch_ms()?;
    let latency_ms = total_latency_ms(payload);

    Ok(json!({
        "token_budget_event": {
            "event_id": event_id,
            "timestamp_utc": timestamp_utc,
            "source_kind": source_kind,
            "traffic_class": traffic_class,
            "payload_origin": payload_origin,
            "project": payload["project"]["code"].clone(),
            "project_code": payload["project"]["code"].clone(),
            "namespace": payload["namespace"]["code"].clone(),
            "namespace_code": payload["namespace"]["code"].clone(),
            "query": payload["query"].clone(),
            "query_hash": hex_sha256(query.as_bytes()),
            "query_type": query_type,
            "target_kind": quality.target_kind,
            "baseline_hit_target": quality.baseline_hit_target,
            "amai_hit_target": quality.amai_hit_target,
            "cold_warm_state": if payload["retrieval_runtime"]["cache_hit"].as_bool().unwrap_or(false) {
                "warm"
            } else {
                "cold"
            },
            "baseline_strategy": baseline_strategy,
            "retrieval_mode": payload["effective_retrieval_mode"].clone(),
            "tokenizer": measurement.tokenizer,
            "latency_ms": latency_ms,
            "baseline_tokens": naive_tokens,
            "delivered_tokens": context_tokens,
            "gross_savings_pct": savings_percent,
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
                "quality_ok": quality.quality_ok,
                "quality_score": quality.quality_score,
                "quality_method": quality.quality_method,
                "quality_tier": quality.quality_tier,
                "head_hit_target": quality.head_hit_target,
            },
            "followup": {
                "needed_followup": quality.needed_followup,
                "followup_count": quality.followup_count,
                "followup_of_event_id": Value::Null,
                "resolved_by_event_id": Value::Null,
            },
            "shape": {
                "document_hits": document_hits,
                "symbol_hits": symbol_hits,
                "file_hits": file_hits,
                "sources_count": sources_count,
                "chunks_count": chunks_count,
                "pack_token_count": context_tokens,
                "deduped_token_count": context_tokens,
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

fn include_traffic_class_in_report(traffic_class: &str, include_verify_events: bool) -> bool {
    include_verify_events || traffic_class == "live"
}

fn derive_baseline_strategy(query_type: &str) -> &'static str {
    match query_type {
        "onboarding_query" => "legacy_pre_amai",
        "config_lookup" | "symbol_lookup" | "code_lookup" => "ide_search_top_files",
        "docs_lookup" | "cross_file_trace" => "grep_top_files",
        "architecture_question" | "bugfix_context" => "semantic_top_k",
        _ => "naive_top_files",
    }
}

fn derive_query_type(query: &str) -> &'static str {
    let lowered = query.to_lowercase();

    if [
        "onboarding",
        "getting started",
        "setup",
        "install",
        "как подключ",
        "как установить",
        "как запустить",
        "как начать",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "onboarding_query"
    } else if [
        "config",
        "конфиг",
        "настрой",
        ".env",
        "yaml",
        "toml",
        "json",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "config_lookup"
    } else if [
        "bug",
        "fix",
        "ошиб",
        "не работает",
        "падает",
        "сломал",
        "почин",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "bugfix_context"
    } else if ["архитект", "architecture", "контур", "как устроен", "зачем"]
        .iter()
        .any(|needle| lowered.contains(needle))
    {
        "architecture_question"
    } else if [
        "trace",
        "call stack",
        "flow",
        "цепоч",
        "где вызыва",
        "откуда приходит",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "cross_file_trace"
    } else if [
        "symbol",
        "struct",
        "enum",
        "trait",
        "type",
        "тип",
        "функц",
        "method",
        "класс",
    ]
    .iter()
    .any(|needle| lowered.contains(needle))
    {
        "symbol_lookup"
    } else if ["docs", "readme", "guide", "док", "документац"]
        .iter()
        .any(|needle| lowered.contains(needle))
    {
        "docs_lookup"
    } else {
        "code_lookup"
    }
}

fn derive_quality_verdict(
    payload: &Value,
    query_type: &str,
    naive_scope: &NaiveScope,
) -> QualityVerdict {
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
    let query_terms = extract_query_terms(payload["query"].as_str().unwrap_or_default());
    let target_kind = match query_type {
        "onboarding_query" | "docs_lookup" => "document",
        "config_lookup" | "code_lookup" => "file",
        "symbol_lookup" => "symbol",
        "cross_file_trace" => "cross_file_trace",
        "architecture_question" | "bugfix_context" => "evidence_bundle",
        _ => "file",
    };
    let baseline_hit_target = !naive_scope.files.is_empty();
    let amai_hit_target = match target_kind {
        "document" => exact_hits > 0 || lexical_hits > 0,
        "file" => exact_hits > 0 || lexical_hits > 0 || symbol_hits > 0,
        "symbol" => symbol_hits > 0,
        "cross_file_trace" => {
            (symbol_hits > 0 && lexical_hits > 0)
                || (symbol_hits + lexical_hits + semantic_hits >= 2)
        }
        "evidence_bundle" => total_hits >= 2,
        _ => total_hits > 0,
    };
    let head_hit_target = top_hit_matches_task(payload, target_kind, &query_terms);
    let quality_ok = baseline_hit_target && amai_hit_target && !semantic_guard_abstained;
    let task_success_proxy = quality_ok
        && match target_kind {
            "document" | "file" | "symbol" => head_hit_target,
            "cross_file_trace" => head_hit_target && total_hits >= 2,
            "evidence_bundle" => head_hit_target && total_hits >= 3,
            _ => head_hit_target,
        };
    let answer_like_proxy = answer_like_from_counts(
        target_kind,
        head_hit_target,
        exact_hits,
        symbol_hits,
        lexical_hits,
        semantic_hits,
    ) && task_success_proxy;
    let quality_score = match target_kind {
        "cross_file_trace" => {
            if answer_like_proxy {
                1.0
            } else if task_success_proxy {
                0.92
            } else if quality_ok {
                0.85
            } else if total_hits > 0 && !semantic_guard_abstained {
                0.5
            } else {
                0.0
            }
        }
        "evidence_bundle" => {
            if answer_like_proxy {
                1.0
            } else if task_success_proxy {
                0.94
            } else if quality_ok {
                0.9
            } else if total_hits > 0 && !semantic_guard_abstained {
                0.6
            } else {
                0.0
            }
        }
        _ => {
            if answer_like_proxy {
                1.0
            } else if task_success_proxy {
                0.9
            } else if quality_ok {
                0.8
            } else if total_hits > 0 && !semantic_guard_abstained {
                0.4
            } else {
                0.0
            }
        }
    };
    let (quality_method, quality_tier) = if answer_like_proxy {
        ("hybrid_answer_proxy", "answer_proxy")
    } else if task_success_proxy {
        ("hybrid_task_proxy", "task_proxy")
    } else if quality_ok {
        ("hybrid_retrieval_parity", "retrieval")
    } else if total_hits > 0 && !semantic_guard_abstained {
        ("hybrid_partial_retrieval", "partial")
    } else {
        ("hybrid_retrieval_parity", "retrieval")
    };
    QualityVerdict {
        target_kind,
        baseline_hit_target,
        amai_hit_target,
        quality_ok,
        quality_score,
        quality_method,
        quality_tier,
        head_hit_target,
        needed_followup: !quality_ok,
        followup_count: 0,
    }
}

fn answer_like_from_counts(
    target_kind: &str,
    head_hit_target: bool,
    exact_hits: usize,
    symbol_hits: usize,
    lexical_hits: usize,
    semantic_hits: usize,
) -> bool {
    if !head_hit_target {
        return false;
    }
    let total_hits = exact_hits + symbol_hits + lexical_hits + semantic_hits;
    let nonzero_sections = [exact_hits, symbol_hits, lexical_hits, semantic_hits]
        .into_iter()
        .filter(|count| *count > 0)
        .count();
    match target_kind {
        "document" => exact_hits > 0,
        "file" => exact_hits > 0 || lexical_hits > 0,
        "symbol" => symbol_hits > 0,
        "cross_file_trace" => symbol_hits > 0 && lexical_hits > 0 && total_hits >= 3,
        "evidence_bundle" => total_hits >= 4 && nonzero_sections >= 2,
        _ => total_hits > 0,
    }
}

fn is_answer_like_event(event: &TokenBudgetEvent) -> bool {
    if !event.quality_ok {
        return false;
    }
    if matches!(
        event.quality_tier.as_str(),
        "answer_proxy" | "answer_success_recovered"
    ) {
        return true;
    }
    match event.target_kind.as_str() {
        "document" => event.head_hit_target && event.document_hits > 0,
        "file" => event.head_hit_target && event.file_hits > 0,
        "symbol" => event.head_hit_target && event.symbol_hits_count > 0,
        "cross_file_trace" => {
            event.head_hit_target && event.symbol_hits_count > 0 && event.chunks_count >= 2
        }
        "evidence_bundle" => {
            event.head_hit_target && event.sources_count >= 2 && event.chunks_count >= 3
        }
        _ => event.head_hit_target && event.sources_count > 0,
    }
}

fn top_hit_matches_task(payload: &Value, target_kind: &str, query_terms: &[String]) -> bool {
    let items = top_retrieval_items(payload, 3);
    items
        .into_iter()
        .any(|item| retrieval_item_matches_task(item, target_kind, query_terms))
}

fn top_retrieval_items(payload: &Value, limit: usize) -> Vec<&Value> {
    let retrieval = &payload["retrieval"];
    let mut items = Vec::new();
    for section in [
        "exact_documents",
        "symbol_hits",
        "lexical_chunks",
        "semantic_chunks",
    ] {
        for item in retrieval[section].as_array().into_iter().flatten() {
            items.push(item);
            if items.len() >= limit {
                return items;
            }
        }
    }
    items
}

fn retrieval_item_matches_task(item: &Value, target_kind: &str, query_terms: &[String]) -> bool {
    let kind_matches = match target_kind {
        "document" => {
            item.get("snippet").is_some()
                || item.get("content").is_some()
                || ledger_item_relative_path(item).is_some_and(is_document_like_path)
        }
        "file" => ledger_item_relative_path(item).is_some(),
        "symbol" => item["name"].as_str().is_some(),
        "cross_file_trace" => {
            ledger_item_relative_path(item).is_some() || item["name"].as_str().is_some()
        }
        "evidence_bundle" => {
            ledger_item_relative_path(item).is_some() || item["content"].as_str().is_some()
        }
        _ => true,
    };
    kind_matches && retrieval_item_matches_query(item, query_terms)
}

fn retrieval_item_matches_query(item: &Value, query_terms: &[String]) -> bool {
    if query_terms.is_empty() {
        return false;
    }
    let mut haystacks = Vec::new();
    if let Some(value) = ledger_item_relative_path(item) {
        haystacks.push(value.to_lowercase());
    }
    if let Some(value) = item["name"].as_str() {
        haystacks.push(value.to_lowercase());
    }
    if let Some(value) = item["snippet"].as_str() {
        haystacks.push(value.to_lowercase());
    }
    if let Some(value) = item["content"].as_str() {
        haystacks.push(value.to_lowercase());
    }
    haystacks
        .into_iter()
        .any(|haystack| query_terms.iter().any(|term| haystack.contains(term)))
}

fn is_document_like_path(path: &str) -> bool {
    let lowered = path.to_lowercase();
    lowered.ends_with(".md")
        || lowered.ends_with(".txt")
        || lowered.contains("readme")
        || lowered.contains("docs/")
        || lowered.contains("guide")
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

fn unique_file_hit_count(payload: &Value) -> usize {
    let mut files = HashSet::new();
    for section in [
        "exact_documents",
        "symbol_hits",
        "lexical_chunks",
        "semantic_chunks",
    ] {
        for item in payload["retrieval"][section]
            .as_array()
            .into_iter()
            .flatten()
        {
            let project_code = item["project_code"]
                .as_str()
                .or_else(|| item["provenance"]["source_project"].as_str())
                .unwrap_or_default();
            let relative_path = item["relative_path"]
                .as_str()
                .or_else(|| item["provenance"]["path"].as_str())
                .unwrap_or_default();
            if !project_code.is_empty() || !relative_path.is_empty() {
                files.insert(format!("{project_code}::{relative_path}"));
            }
        }
    }
    files.len()
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

fn total_latency_ms(payload: &Value) -> f64 {
    let runtime = &payload["retrieval_runtime"];
    if let Some(value) = runtime["total_ms"].as_f64() {
        return value;
    }
    [
        "resolve_scope_ms",
        "cache_lookup_ms",
        "exact_lookup_ms",
        "symbol_lookup_ms",
        "lexical_lookup_ms",
        "query_embed_ms",
        "semantic_search_ms",
        "semantic_hydrate_ms",
        "serialize_ms",
        "persist_ms",
    ]
    .iter()
    .map(|key| runtime[*key].as_f64().unwrap_or(0.0))
    .sum()
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
    baseline_strategy: &str,
    query: &str,
) -> Result<NaiveScope> {
    let mut files = Vec::new();
    let strategy_files =
        collect_payload_scope_files_by_strategy(payload, baseline_strategy, limit_files)?;
    if !strategy_files.is_empty() {
        for (project_code, repo_root, path) in strategy_files {
            files.push(read_scope_file(
                &project_code,
                &repo_root,
                &path,
                max_bytes_per_file,
            )?);
        }
    } else {
        for project in payload["visible_projects"].as_array().into_iter().flatten() {
            let Some(project_code) = project["project_code"].as_str() else {
                continue;
            };
            let Some(repo_root) = project["repo_root"].as_str() else {
                continue;
            };
            for path in collect_scope_files_by_strategy(
                Path::new(repo_root),
                query,
                baseline_strategy,
                limit_files,
                max_bytes_per_file.min(16 * 1024),
            )? {
                files.push(read_scope_file(
                    project_code,
                    Path::new(repo_root),
                    &path,
                    max_bytes_per_file,
                )?);
            }
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

fn read_scope_file(
    project_code: &str,
    repo_root: &Path,
    path: &Path,
    max_bytes_per_file: usize,
) -> Result<NaiveScopeFile> {
    let relative_path = path
        .strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string();
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read naive scope file {}", path.display()))?;
    let original_bytes = bytes.len();
    let bytes_used = original_bytes.min(max_bytes_per_file);
    let content = safe_lossy_prefix(&bytes, bytes_used);
    Ok(NaiveScopeFile {
        project_code: project_code.to_string(),
        relative_path,
        original_bytes,
        bytes_used: content.len(),
        truncated: original_bytes > content.len(),
        content,
    })
}

fn collect_payload_scope_files_by_strategy(
    payload: &Value,
    baseline_strategy: &str,
    limit_files: usize,
) -> Result<Vec<(String, PathBuf, PathBuf)>> {
    let sections: &[&str] = match baseline_strategy {
        "ide_search_top_files" => &["exact_documents", "symbol_hits", "lexical_chunks"],
        "semantic_top_k" => &["semantic_chunks"],
        _ => return Ok(Vec::new()),
    };
    let repo_roots = payload["visible_projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|project| {
            Some((
                project["project_code"].as_str()?.to_string(),
                PathBuf::from(project["repo_root"].as_str()?),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    for section in sections {
        for item in payload["retrieval"][section]
            .as_array()
            .into_iter()
            .flatten()
        {
            let Some(project_code) = ledger_item_project_code(item) else {
                continue;
            };
            let Some(relative_path) = ledger_item_relative_path(item) else {
                continue;
            };
            let Some(repo_root) = repo_roots.get(project_code) else {
                continue;
            };
            let path = repo_root.join(relative_path);
            if !path.is_file() {
                continue;
            }
            if seen.insert(format!("{project_code}::{relative_path}")) {
                files.push((project_code.to_string(), repo_root.clone(), path));
            }
        }
    }
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn collect_scope_files_by_strategy(
    root: &Path,
    query: &str,
    baseline_strategy: &str,
    limit_files: usize,
    score_bytes_per_file: usize,
) -> Result<Vec<PathBuf>> {
    match baseline_strategy {
        "grep_top_files" => {
            collect_grep_scope_files(root, query, limit_files, score_bytes_per_file)
        }
        "legacy_pre_amai" => {
            collect_legacy_scope_files(root, query, limit_files, score_bytes_per_file)
        }
        _ => collect_scope_files(root, limit_files),
    }
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

fn collect_grep_scope_files(
    root: &Path,
    query: &str,
    limit_files: usize,
    score_bytes_per_file: usize,
) -> Result<Vec<PathBuf>> {
    let files = collect_scope_files(root, 0)?;
    let terms = extract_query_terms(query);
    if terms.is_empty() {
        return collect_scope_files(root, limit_files);
    }

    let mut scored = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .display()
            .to_string()
            .to_lowercase();
        let mut score = text_match_score(&relative, &terms) * 8;

        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read grep scope file {}", path.display()))?;
        let content = safe_lossy_prefix(&bytes, score_bytes_per_file).to_lowercase();
        score += text_match_score(&content, &terms);

        if score > 0 {
            scored.push((score, path));
        }
    }

    if scored.is_empty() {
        return collect_scope_files(root, limit_files);
    }

    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut files = scored.into_iter().map(|(_, path)| path).collect::<Vec<_>>();
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn collect_legacy_scope_files(
    root: &Path,
    query: &str,
    limit_files: usize,
    score_bytes_per_file: usize,
) -> Result<Vec<PathBuf>> {
    let files = collect_scope_files(root, 0)?;
    let terms = extract_query_terms(query);
    let mut scored = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .display()
            .to_string()
            .to_lowercase();
        let docs_bias = if relative.contains("readme")
            || relative.contains("docs/")
            || relative.contains("guide")
            || relative.contains("install")
            || relative.contains("setup")
        {
            12
        } else {
            0
        };
        let mut score = docs_bias + text_match_score(&relative, &terms) * 6;
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read legacy scope file {}", path.display()))?;
        let content = safe_lossy_prefix(&bytes, score_bytes_per_file).to_lowercase();
        score += text_match_score(&content, &terms);
        if score > 0 {
            scored.push((score, path));
        }
    }
    if scored.is_empty() {
        return collect_scope_files(root, limit_files);
    }
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut files = scored.into_iter().map(|(_, path)| path).collect::<Vec<_>>();
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn ledger_item_project_code(item: &Value) -> Option<&str> {
    item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
}

fn ledger_item_relative_path(item: &Value) -> Option<&str> {
    item["relative_path"]
        .as_str()
        .or_else(|| item["provenance"]["path"].as_str())
}

fn extract_query_terms(query: &str) -> Vec<String> {
    let mut terms = query
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '.')
        .filter(|term| term.len() >= 3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

fn text_match_score(haystack: &str, terms: &[String]) -> usize {
    terms
        .iter()
        .map(|term| haystack.match_indices(term).count())
        .sum()
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

#[cfg(test)]
mod tests {
    use super::{
        MeasurementConfig, NaiveScope, TokenBudgetEvent, apply_reverification_metadata,
        build_product_headline, derive_baseline_strategy, derive_quality_verdict,
        derive_query_type, derive_traffic_class, event_to_json, followup_queries_related,
        include_traffic_class_in_report, needs_live_reverification, parse_snapshot_event,
        reconcile_followup_recovery, repair_legacy_token_event_payload, summarize_events,
    };
    use crate::postgres::ObservabilitySnapshotRecord;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn traffic_class_comes_from_source_kind_prefix() {
        assert_eq!(derive_traffic_class("live_context_pack"), "live");
        assert_eq!(derive_traffic_class("verify_token_benchmark"), "verify");
        assert_eq!(derive_traffic_class("proof_hostile"), "proof");
        assert_eq!(derive_traffic_class("benchmark_hot_path"), "benchmark");
        assert_eq!(derive_traffic_class("custom_unknown"), "unknown");
    }

    #[test]
    fn default_product_report_is_live_only() {
        assert!(include_traffic_class_in_report("live", false));
        assert!(!include_traffic_class_in_report("verify", false));
        assert!(!include_traffic_class_in_report("proof", false));
        assert!(!include_traffic_class_in_report("benchmark", false));
        assert!(include_traffic_class_in_report("verify", true));
        assert!(include_traffic_class_in_report("proof", true));
        assert!(include_traffic_class_in_report("benchmark", true));
    }

    #[test]
    fn baseline_strategy_matches_realistic_non_amai_workflows() {
        assert_eq!(
            derive_baseline_strategy("onboarding_query"),
            "legacy_pre_amai"
        );
        assert_eq!(
            derive_baseline_strategy("config_lookup"),
            "ide_search_top_files"
        );
        assert_eq!(
            derive_baseline_strategy("code_lookup"),
            "ide_search_top_files"
        );
        assert_eq!(
            derive_baseline_strategy("symbol_lookup"),
            "ide_search_top_files"
        );
        assert_eq!(derive_baseline_strategy("docs_lookup"), "grep_top_files");
        assert_eq!(
            derive_baseline_strategy("cross_file_trace"),
            "grep_top_files"
        );
        assert_eq!(
            derive_baseline_strategy("architecture_question"),
            "semantic_top_k"
        );
        assert_eq!(derive_baseline_strategy("bugfix_context"), "semantic_top_k");
    }

    #[test]
    fn query_type_is_classified_for_common_human_queries() {
        assert_eq!(
            derive_query_type("Как установить Amai и подключить к VS Code?"),
            "onboarding_query"
        );
        assert_eq!(
            derive_query_type("Почему падает retrieval и как это починить?"),
            "bugfix_context"
        );
        assert_eq!(
            derive_query_type("Где вызывается эта функция и как идёт flow?"),
            "cross_file_trace"
        );
        assert_eq!(
            derive_query_type("Покажи config и .env для Amai"),
            "config_lookup"
        );
        assert_eq!(
            derive_query_type("Где лежит нужный файл для MCP integration?"),
            "code_lookup"
        );
    }

    #[test]
    fn quality_verdict_uses_target_kind_specific_rules() {
        let payload = json!({
            "retrieval": {
                "exact_documents": [],
                "symbol_hits": [{"name": "run"}],
                "lexical_chunks": [],
                "semantic_chunks": []
            },
            "quality": {
                "semantic_guard": {
                    "abstained": false
                }
            }
        });
        let verdict = derive_quality_verdict(
            &json!({
                "query": "run symbol",
                "retrieval": payload["retrieval"].clone(),
                "quality": payload["quality"].clone()
            }),
            "symbol_lookup",
            &NaiveScope {
                files: vec![json!({"relative_path": "src/main.rs"})],
                rendered_files: Vec::new(),
            },
        );
        assert_eq!(verdict.target_kind, "symbol");
        assert!(verdict.baseline_hit_target);
        assert!(verdict.amai_hit_target);
        assert!(verdict.quality_ok);
        assert_eq!(verdict.quality_method, "hybrid_answer_proxy");
        assert_eq!(verdict.quality_tier, "answer_proxy");
        assert!(verdict.head_hit_target);
    }

    #[test]
    fn event_json_exposes_canonical_token_ledger_aliases() {
        let event = TokenBudgetEvent {
            created_at_epoch_ms: 10,
            event_id: "event-1".to_string(),
            session_id: "session-1".to_string(),
            rolling_window_profile: "codex_5h".to_string(),
            timestamp_utc: 10,
            snapshot_kind: "token_budget_event".to_string(),
            source_kind: "live_context_pack".to_string(),
            traffic_class: "live".to_string(),
            project: "art".to_string(),
            namespace: "continuity".to_string(),
            query: "token report".to_string(),
            query_hash: "hash".to_string(),
            query_type: "architecture_question".to_string(),
            target_kind: "evidence_bundle".to_string(),
            baseline_hit_target: true,
            amai_hit_target: true,
            cold_warm_state: "warm".to_string(),
            baseline_strategy: "grep_top_files".to_string(),
            retrieval_mode: Some("local_strict".to_string()),
            tokenizer: "o200k_base".to_string(),
            latency_ms: 3.0,
            saved_tokens: 700,
            naive_tokens: 1000,
            context_tokens: 300,
            recovery_tokens: 20,
            effective_saved_tokens: 680,
            savings_factor: 3.33,
            savings_percent: 70.0,
            effective_savings_percent: 68.0,
            quality_ok: true,
            quality_score: 1.0,
            quality_method: "hybrid_task_success".to_string(),
            quality_tier: "task_success_recovered".to_string(),
            head_hit_target: true,
            needed_followup: false,
            followup_count: 1,
            followup_of_event_id: Some("event-0".to_string()),
            resolved_by_event_id: None,
            fallback_triggered: true,
            fallback_count: 1,
            document_hits: 1,
            symbol_hits_count: 0,
            file_hits: 1,
            sources_count: 2,
            chunks_count: 2,
            pack_token_count: 300,
            deduped_token_count: 300,
        };

        let payload = event_to_json(&event);
        assert_eq!(payload["project_code"], "art");
        assert_eq!(payload["namespace_code"], "continuity");
        assert_eq!(payload["baseline_tokens"], 1000);
        assert_eq!(payload["delivered_tokens"], 300);
        assert_eq!(payload["gross_savings_pct"], 70.0);
    }

    #[test]
    fn parse_snapshot_event_accepts_canonical_alias_fields() {
        let row = ObservabilitySnapshotRecord {
            snapshot_id: Uuid::new_v4(),
            snapshot_kind: "token_budget_event".to_string(),
            created_at_epoch_ms: 1234,
            payload: json!({
                "token_budget_event": {
                    "event_id": "event-1",
                    "source_kind": "live_context_pack",
                    "traffic_class": "live",
                    "project_code": "art",
                    "namespace_code": "continuity",
                    "query": "token report",
                    "query_hash": "hash",
                    "query_type": "code_lookup",
                    "target_kind": "file",
                    "baseline_hit_target": true,
                    "amai_hit_target": true,
                    "cold_warm_state": "warm",
                    "baseline_strategy": "grep_top_files",
                    "tokenizer": "o200k_base",
                    "latency_ms": 2.0,
                    "baseline_tokens": 1500,
                    "delivered_tokens": 400,
                    "gross_savings_pct": 73.3333333333,
                    "recovery": {
                        "recovery_tokens": 40,
                        "fallback_triggered": false,
                        "fallback_count": 0
                    },
                    "quality": {
                        "quality_ok": true,
                        "quality_score": 1.0,
                        "quality_method": "hybrid_task_success",
                        "quality_tier": "task_success_recovered",
                        "head_hit_target": true
                    },
                    "followup": {
                        "needed_followup": false,
                        "followup_count": 1,
                        "followup_of_event_id": "event-0",
                        "resolved_by_event_id": null
                    },
                    "shape": {
                        "document_hits": 1,
                        "symbol_hits": 0,
                        "file_hits": 1,
                        "sources_count": 2,
                        "chunks_count": 2,
                        "pack_token_count": 400,
                        "deduped_token_count": 400
                    },
                    "savings": {
                        "saved_tokens": 1100,
                        "effective_saved_tokens": 1060,
                        "savings_factor": 3.75,
                        "effective_savings_percent": 70.6666666667
                    }
                }
            }),
        };

        let parsed = parse_snapshot_event(&row)
            .expect("parse should succeed")
            .expect("event should exist");
        assert_eq!(parsed.project, "art");
        assert_eq!(parsed.namespace, "continuity");
        assert_eq!(parsed.naive_tokens, 1500);
        assert_eq!(parsed.context_tokens, 400);
        assert_eq!(parsed.savings_percent, 73.3333333333);
    }

    #[test]
    fn product_headline_prefers_verified_metric_when_available() {
        let headline = build_product_headline(
            &json!({
                "events_total": 12,
                "counted_events": 7,
                "preliminary": false,
                "verified_effective_savings_pct": 28.4,
                "effective_savings_pct": 31.2,
                "verified_effective_saved_tokens": 184220,
                "total_effective_saved_tokens": 200000,
                "quality_ok_rate": 96.1,
                "fallback_rate": 3.8
            }),
            "окно Codex 5 часов",
        );
        assert_eq!(headline["metric_code"], "verified_effective_savings_pct");
        assert_eq!(headline["value_percent"], 28.4);
        assert_eq!(headline["saved_tokens"], 184220);
        assert_eq!(headline["status"], "pass");
    }

    #[test]
    fn product_headline_falls_back_to_preliminary_effective_metric() {
        let headline = build_product_headline(
            &json!({
                "events_total": 10,
                "counted_events": 0,
                "legacy_unverified_events": 3,
                "preliminary": true,
                "verified_effective_savings_pct": 0.0,
                "effective_savings_pct": 44.0,
                "verified_effective_saved_tokens": 0,
                "total_effective_saved_tokens": 1200,
                "quality_ok_rate": 0.0,
                "fallback_rate": 0.0
            }),
            "окно Codex 5 часов",
        );
        assert_eq!(headline["metric_code"], "effective_savings_pct_preliminary");
        assert_eq!(headline["value_percent"], 44.0);
        assert_eq!(headline["saved_tokens"], 1200);
        assert_eq!(headline["status"], "alert");
        assert!(
            headline["note"]
                .as_str()
                .unwrap_or_default()
                .contains("старым форматом")
        );
    }

    #[test]
    fn legacy_token_event_repair_adds_missing_fields() {
        let repaired = repair_legacy_token_event_payload(&json!({
            "token_budget_event": {
                "query": "Как установить Amai и подключить к VS Code?",
                "source_kind": "live_context_pack",
                "naive_scope": { "tokens": 1000 },
                "context_pack_render": { "tokens": 200 },
                "savings": {
                    "saved_tokens": 800,
                    "savings_percent": 80.0,
                    "savings_factor": 5.0
                }
            }
        }))
        .expect("repair should produce patched payload");

        let event = &repaired["token_budget_event"];
        assert_eq!(event["traffic_class"], "live");
        assert_eq!(event["query_type"], "onboarding_query");
        assert_eq!(event["quality"]["quality_method"], "legacy_unverified");
        assert_eq!(event["savings"]["effective_saved_tokens"], 800);
        assert_eq!(event["savings"]["effective_savings_percent"], 80.0);
    }

    #[test]
    fn only_legacy_live_events_need_reverification() {
        assert!(needs_live_reverification(&json!({
            "token_budget_event": {
                "source_kind": "live_context_pack",
                "traffic_class": "live",
                "quality": {
                    "quality_ok": false,
                    "quality_method": "legacy_unverified"
                }
            }
        })));
        assert!(needs_live_reverification(&json!({
            "token_budget_event": {
                "source_kind": "live_context_pack",
                "traffic_class": "live",
                "target_kind": "unknown",
                "quality": {
                    "quality_ok": true,
                    "quality_method": "reverified_retrieval_parity"
                },
                "shape": {}
            }
        })));
        assert!(!needs_live_reverification(&json!({
            "token_budget_event": {
                "source_kind": "verify_token_benchmark",
                "traffic_class": "verify",
                "quality": {
                    "quality_ok": true,
                    "quality_method": "benchmark_assumption"
                }
            }
        })));
        assert!(!needs_live_reverification(&json!({
            "token_budget_event": {
                "source_kind": "live_context_pack",
                "traffic_class": "live",
                "target_kind": "file",
                "latency_ms": 12.0,
                "quality": {
                    "quality_ok": true,
                    "quality_method": "retrieval_parity",
                    "quality_tier": "retrieval",
                    "head_hit_target": true
                },
                "followup": {
                    "needed_followup": false,
                    "followup_count": 0
                },
                "shape": {
                    "file_hits": 1,
                    "pack_token_count": 100,
                    "deduped_token_count": 100
                }
            }
        })));
    }

    #[test]
    fn reverification_keeps_identity_and_marks_method() {
        let mut rebuilt = json!({
            "retrieval": {
                "exact_documents": [{}],
                "symbol_hits": [],
                "lexical_chunks": [],
                "semantic_chunks": []
            },
            "token_budget_event": {
                "target_kind": "file",
                "quality": {
                    "quality_ok": true,
                    "quality_score": 1.0,
                    "quality_method": "retrieval_parity",
                    "quality_tier": "retrieval",
                    "head_hit_target": true
                }
            }
        });
        apply_reverification_metadata(
            &mut rebuilt,
            &json!({
                "event_id": "existing-event",
                "timestamp_utc": 12345,
                "source_kind": "live_context_pack",
                "quality": {
                    "quality_ok": false,
                    "quality_method": "legacy_unverified"
                }
            }),
            99999,
        )
        .expect("reverification metadata should apply");

        let event = &rebuilt["token_budget_event"];
        assert_eq!(event["event_id"], "existing-event");
        assert_eq!(event["timestamp_utc"], 12345);
        assert_eq!(event["traffic_class"], "live");
        assert_eq!(
            event["quality"]["quality_method"],
            "reverified_answer_proxy"
        );
        assert_eq!(event["quality"]["quality_tier"], "answer_proxy");
        assert_eq!(
            event["reverification"]["previous_quality_method"],
            "legacy_unverified"
        );
    }

    #[test]
    fn preliminary_turns_off_when_token_volume_is_high_enough() {
        let measurement = MeasurementConfig {
            tokenizer: "o200k_base".to_string(),
            naive_limit_files: 5,
            naive_max_bytes_per_file: 16384,
            include_verify_events_by_default: false,
            preliminary_min_events: 50,
            preliminary_min_baseline_tokens: 100_000,
        };
        let summary = summarize_events(
            &[TokenBudgetEvent {
                created_at_epoch_ms: 10,
                event_id: "event-1".to_string(),
                session_id: "session-1".to_string(),
                rolling_window_profile: "codex_5h".to_string(),
                timestamp_utc: 10,
                snapshot_kind: "token_budget_event".to_string(),
                source_kind: "live_context_pack".to_string(),
                traffic_class: "live".to_string(),
                project: "art".to_string(),
                namespace: "continuity".to_string(),
                query: "explain token savings".to_string(),
                query_hash: "hash".to_string(),
                query_type: "architecture_question".to_string(),
                target_kind: "evidence_bundle".to_string(),
                baseline_hit_target: true,
                amai_hit_target: true,
                cold_warm_state: "warm".to_string(),
                baseline_strategy: "naive_top_files".to_string(),
                retrieval_mode: Some("local_strict".to_string()),
                tokenizer: "o200k_base".to_string(),
                latency_ms: 12.0,
                saved_tokens: 150_000,
                naive_tokens: 160_000,
                context_tokens: 10_000,
                recovery_tokens: 0,
                effective_saved_tokens: 150_000,
                savings_factor: 16.0,
                savings_percent: 93.75,
                effective_savings_percent: 93.75,
                quality_ok: true,
                quality_score: 1.0,
                quality_method: "retrieval_parity".to_string(),
                quality_tier: "retrieval".to_string(),
                head_hit_target: true,
                needed_followup: false,
                followup_count: 0,
                followup_of_event_id: None,
                resolved_by_event_id: None,
                fallback_triggered: false,
                fallback_count: 0,
                document_hits: 1,
                symbol_hits_count: 2,
                file_hits: 3,
                sources_count: 5,
                chunks_count: 4,
                pack_token_count: 10_000,
                deduped_token_count: 10_000,
            }],
            20,
            &measurement,
        );

        assert_eq!(summary["preliminary"], false);
        assert_eq!(summary["counted_events"], 1);
        assert_eq!(summary["answer_like_counted_events"], 1);
        assert_eq!(summary["verified_effective_savings_pct"], 93.75);
        assert_eq!(summary["verified_answer_like_savings_pct"], 93.75);
    }

    #[test]
    fn followup_recovery_is_attributed_to_successful_followup_event() {
        let reconciled = reconcile_followup_recovery(
            &[
                TokenBudgetEvent {
                    created_at_epoch_ms: 1000,
                    event_id: "event-1".to_string(),
                    session_id: "session-1".to_string(),
                    rolling_window_profile: "codex_5h".to_string(),
                    timestamp_utc: 1000,
                    snapshot_kind: "token_budget_event".to_string(),
                    source_kind: "live_context_pack".to_string(),
                    traffic_class: "live".to_string(),
                    project: "art".to_string(),
                    namespace: "continuity".to_string(),
                    query: "find dashboard token bug".to_string(),
                    query_hash: "hash-1".to_string(),
                    query_type: "code_lookup".to_string(),
                    target_kind: "file".to_string(),
                    baseline_hit_target: true,
                    amai_hit_target: false,
                    cold_warm_state: "cold".to_string(),
                    baseline_strategy: "naive_top_files".to_string(),
                    retrieval_mode: Some("local_strict".to_string()),
                    tokenizer: "o200k_base".to_string(),
                    latency_ms: 10.0,
                    saved_tokens: 900,
                    naive_tokens: 1000,
                    context_tokens: 100,
                    recovery_tokens: 0,
                    effective_saved_tokens: 900,
                    savings_factor: 10.0,
                    savings_percent: 90.0,
                    effective_savings_percent: 90.0,
                    quality_ok: false,
                    quality_score: 0.0,
                    quality_method: "hybrid_retrieval_parity".to_string(),
                    quality_tier: "retrieval".to_string(),
                    head_hit_target: false,
                    needed_followup: true,
                    followup_count: 0,
                    followup_of_event_id: None,
                    resolved_by_event_id: None,
                    fallback_triggered: false,
                    fallback_count: 0,
                    document_hits: 0,
                    symbol_hits_count: 0,
                    file_hits: 0,
                    sources_count: 0,
                    chunks_count: 0,
                    pack_token_count: 100,
                    deduped_token_count: 100,
                },
                TokenBudgetEvent {
                    created_at_epoch_ms: 2000,
                    event_id: "event-2".to_string(),
                    session_id: "session-1".to_string(),
                    rolling_window_profile: "codex_5h".to_string(),
                    timestamp_utc: 2000,
                    snapshot_kind: "token_budget_event".to_string(),
                    source_kind: "live_context_pack".to_string(),
                    traffic_class: "live".to_string(),
                    project: "art".to_string(),
                    namespace: "continuity".to_string(),
                    query: "dashboard token bug file".to_string(),
                    query_hash: "hash-2".to_string(),
                    query_type: "code_lookup".to_string(),
                    target_kind: "file".to_string(),
                    baseline_hit_target: true,
                    amai_hit_target: true,
                    cold_warm_state: "warm".to_string(),
                    baseline_strategy: "naive_top_files".to_string(),
                    retrieval_mode: Some("local_strict".to_string()),
                    tokenizer: "o200k_base".to_string(),
                    latency_ms: 4.0,
                    saved_tokens: 800,
                    naive_tokens: 1000,
                    context_tokens: 120,
                    recovery_tokens: 0,
                    effective_saved_tokens: 800,
                    savings_factor: 8.0,
                    savings_percent: 80.0,
                    effective_savings_percent: 80.0,
                    quality_ok: true,
                    quality_score: 1.0,
                    quality_method: "hybrid_retrieval_parity".to_string(),
                    quality_tier: "retrieval".to_string(),
                    head_hit_target: true,
                    needed_followup: false,
                    followup_count: 0,
                    followup_of_event_id: None,
                    resolved_by_event_id: None,
                    fallback_triggered: false,
                    fallback_count: 0,
                    document_hits: 1,
                    symbol_hits_count: 0,
                    file_hits: 1,
                    sources_count: 1,
                    chunks_count: 1,
                    pack_token_count: 120,
                    deduped_token_count: 120,
                },
            ],
            30 * 60_000,
        );

        assert_eq!(reconciled[0].recovery_tokens, 0);
        assert_eq!(
            reconciled[0].resolved_by_event_id.as_deref(),
            Some("event-2")
        );
        assert_eq!(reconciled[1].recovery_tokens, 100);
        assert_eq!(reconciled[1].followup_count, 1);
        assert_eq!(
            reconciled[1].followup_of_event_id.as_deref(),
            Some("event-1")
        );
        assert_eq!(reconciled[1].effective_saved_tokens, 780);
        assert_eq!(reconciled[1].effective_savings_percent, 78.0);
    }

    #[test]
    fn followup_query_matching_requires_same_shape_and_meaningful_overlap() {
        assert!(followup_queries_related(
            super::FollowupEventKey {
                query: "find dashboard token bug",
                query_hash: "",
                query_type: "code_lookup",
                target_kind: "file",
            },
            super::FollowupEventKey {
                query: "dashboard token bug file",
                query_hash: "",
                query_type: "code_lookup",
                target_kind: "file",
            },
        ));
        assert!(!followup_queries_related(
            super::FollowupEventKey {
                query: "find dashboard token bug",
                query_hash: "",
                query_type: "code_lookup",
                target_kind: "file",
            },
            super::FollowupEventKey {
                query: "dashboard config",
                query_hash: "",
                query_type: "config_lookup",
                target_kind: "file",
            },
        ));
        assert!(!followup_queries_related(
            super::FollowupEventKey {
                query: "find dashboard token bug",
                query_hash: "",
                query_type: "code_lookup",
                target_kind: "file",
            },
            super::FollowupEventKey {
                query: "dashboard token",
                query_hash: "",
                query_type: "code_lookup",
                target_kind: "symbol",
            },
        ));
    }
}
