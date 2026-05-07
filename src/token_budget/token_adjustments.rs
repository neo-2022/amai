use crate::config::{self, AppConfig};
use crate::postgres;
use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct AdjustmentRegistryFile {
    #[serde(default)]
    adjustments: Vec<AdjustmentRegistryEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct AdjustmentRegistryEntry {
    adjustment_id: String,
    scope_code: String,
    kind: String,
    status: String,
    reason_code: String,
    created_at_epoch_ms: i64,
    #[serde(default)]
    tokens_delta: Option<i64>,
    #[serde(default)]
    amount_delta: Option<f64>,
    #[serde(default)]
    currency_profile: Option<String>,
    #[serde(default)]
    related_statement_id: Option<String>,
}

pub(super) fn build_adjustment_request_schema_json(
    contract: &super::TokenBudgetContractConfig,
) -> Value {
    json!({
        "schema_version": contract.adjustment_request_schema_version.clone(),
        "required_fields": [
            "adjustment_id",
            "scope_code",
            "kind",
            "status",
            "reason_code",
            "created_at_epoch_ms"
        ],
        "allowed_kinds": [
            "credit_note",
            "adjustment_entry",
            "dispute_hold"
        ],
        "allowed_statuses": [
            "requested",
            "pending_review",
            "approved_but_unapplied",
            "applied_report_only",
            "disputed",
            "rejected"
        ],
        "retroactive_rewrite_policy": "forbidden_use_adjustment_entries",
        "note": "Adjustment request schema существует затем, чтобы corrections/disputes materialize-ились отдельными entries, а не тихой перезаписью старого statement."
    })
}

fn adjustment_entry_json(entry: &AdjustmentRegistryEntry) -> Value {
    json!({
        "adjustment_id": entry.adjustment_id,
        "scope_code": entry.scope_code,
        "kind": entry.kind,
        "status": entry.status,
        "reason_code": entry.reason_code,
        "created_at_epoch_ms": entry.created_at_epoch_ms,
        "tokens_delta": entry.tokens_delta,
        "amount_delta": entry.amount_delta,
        "currency_profile": entry.currency_profile,
        "related_statement_id": entry.related_statement_id,
    })
}

fn adjustment_status_matches(status: Option<&str>, expected: &[&str]) -> bool {
    let Some(status) = status else {
        return false;
    };
    expected.iter().any(|candidate| *candidate == status)
}

fn sum_adjustment_tokens(entries: &[Value], statuses: &[&str]) -> i64 {
    entries
        .iter()
        .filter(|entry| adjustment_status_matches(entry["status"].as_str(), statuses))
        .map(|entry| entry["tokens_delta"].as_i64().unwrap_or(0))
        .sum()
}

fn sum_adjustment_amount(entries: &[Value], statuses: &[&str]) -> f64 {
    entries
        .iter()
        .filter(|entry| adjustment_status_matches(entry["status"].as_str(), statuses))
        .map(|entry| entry["amount_delta"].as_f64().unwrap_or(0.0))
        .sum()
}

pub(super) fn load_adjustment_registry_from_source(
    source: &Value,
    contract: &super::TokenBudgetContractConfig,
) -> Value {
    let mut base = json!({
        "schema_version": contract.adjustment_registry_version.clone(),
        "request_schema_version": contract.adjustment_request_schema_version.clone(),
        "source": source.clone(),
        "source_bytes": Value::Null,
        "source_sha256": Value::Null,
        "source_last_modified_epoch_ms": Value::Null,
        "status": source["status"].clone(),
        "entries_count": 0,
        "pending_entries_count": 0,
        "applied_entries_count": 0,
        "disputed_entries_count": 0,
        "registry_hash": Value::Null,
        "scopes": {
            "current_session": {
                "entries_count": 0,
                "pending_entries_count": 0,
                "applied_entries_count": 0,
                "disputed_entries_count": 0,
                "scope_hash": Value::Null,
            },
            "rolling_window": {
                "entries_count": 0,
                "pending_entries_count": 0,
                "applied_entries_count": 0,
                "disputed_entries_count": 0,
                "scope_hash": Value::Null,
            },
            "lifetime": {
                "entries_count": 0,
                "pending_entries_count": 0,
                "applied_entries_count": 0,
                "disputed_entries_count": 0,
                "scope_hash": Value::Null,
            }
        },
        "note": "Adjustment registry пока optional: без него report-only tokenonomics не переписывает прошлые периоды и не притворяется credit workflow."
    });

    let source_status = source["status"].as_str().unwrap_or("unknown");
    if !matches!(
        source_status,
        "configured_existing_path" | "default_existing_path"
    ) {
        return base;
    }

    let Some(path) = source["resolved_path"].as_str() else {
        return base;
    };

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            base["status"] = Value::String("read_error".to_string());
            base["source"]["binding_status"] = Value::String("read_error".to_string());
            base["read_error"] = Value::String(error.to_string());
            return base;
        }
    };
    super::attach_source_file_evidence(&mut base, Path::new(path), &content);
    let registry = match serde_json::from_str::<AdjustmentRegistryFile>(&content) {
        Ok(registry) => registry,
        Err(error) => {
            base["status"] = Value::String("parse_error".to_string());
            base["source"]["binding_status"] = Value::String("parse_error".to_string());
            base["parse_error"] = Value::String(error.to_string());
            return base;
        }
    };

    let entries = registry
        .adjustments
        .iter()
        .map(adjustment_entry_json)
        .collect::<Vec<_>>();

    let mut scope_map = serde_json::Map::new();
    for scope_code in ["current_session", "rolling_window", "lifetime"] {
        let scope_entries = registry
            .adjustments
            .iter()
            .filter(|entry| entry.scope_code == scope_code)
            .map(adjustment_entry_json)
            .collect::<Vec<_>>();
        let pending_entries_count = scope_entries
            .iter()
            .filter(|entry| {
                adjustment_status_matches(
                    entry["status"].as_str(),
                    &["requested", "pending_review", "approved_but_unapplied"],
                )
            })
            .count();
        let applied_entries_count = scope_entries
            .iter()
            .filter(|entry| {
                adjustment_status_matches(entry["status"].as_str(), &["applied_report_only"])
            })
            .count();
        let disputed_entries_count = scope_entries
            .iter()
            .filter(|entry| adjustment_status_matches(entry["status"].as_str(), &["disputed"]))
            .count();
        scope_map.insert(
            scope_code.to_string(),
            json!({
                "entries_count": scope_entries.len(),
                "pending_entries_count": pending_entries_count,
                "applied_entries_count": applied_entries_count,
                "disputed_entries_count": disputed_entries_count,
                "pending_tokens_delta": sum_adjustment_tokens(
                    &scope_entries,
                    &["requested", "pending_review", "approved_but_unapplied"],
                ),
                "pending_amount_delta": sum_adjustment_amount(
                    &scope_entries,
                    &["requested", "pending_review", "approved_but_unapplied"],
                ),
                "applied_tokens_delta": sum_adjustment_tokens(
                    &scope_entries,
                    &["applied_report_only"],
                ),
                "applied_amount_delta": sum_adjustment_amount(
                    &scope_entries,
                    &["applied_report_only"],
                ),
                "disputed_tokens_delta": sum_adjustment_tokens(
                    &scope_entries,
                    &["disputed"],
                ),
                "disputed_amount_delta": sum_adjustment_amount(
                    &scope_entries,
                    &["disputed"],
                ),
                "scope_hash": super::hash_line_items(&scope_entries)
                  .unwrap_or_else(|_| "hash_error".to_string()),
            }),
        );
    }

    let pending_entries_count = entries
        .iter()
        .filter(|entry| {
            adjustment_status_matches(
                entry["status"].as_str(),
                &["requested", "pending_review", "approved_but_unapplied"],
            )
        })
        .count();
    let applied_entries_count = entries
        .iter()
        .filter(|entry| {
            adjustment_status_matches(entry["status"].as_str(), &["applied_report_only"])
        })
        .count();
    let disputed_entries_count = entries
        .iter()
        .filter(|entry| adjustment_status_matches(entry["status"].as_str(), &["disputed"]))
        .count();

    base["status"] = Value::String("loaded".to_string());
    base["source"]["binding_status"] = Value::String(if source_status == "default_existing_path" {
        "default_loaded".to_string()
    } else {
        "loaded".to_string()
    });
    base["entries_count"] = json!(entries.len());
    base["pending_entries_count"] = json!(pending_entries_count);
    base["applied_entries_count"] = json!(applied_entries_count);
    base["disputed_entries_count"] = json!(disputed_entries_count);
    base["pending_tokens_delta"] = json!(sum_adjustment_tokens(
        &entries,
        &["requested", "pending_review", "approved_but_unapplied"],
    ));
    base["pending_amount_delta"] = json!(sum_adjustment_amount(
        &entries,
        &["requested", "pending_review", "approved_but_unapplied"],
    ));
    base["applied_tokens_delta"] =
        json!(sum_adjustment_tokens(&entries, &["applied_report_only"],));
    base["applied_amount_delta"] =
        json!(sum_adjustment_amount(&entries, &["applied_report_only"],));
    base["disputed_tokens_delta"] = json!(sum_adjustment_tokens(&entries, &["disputed"]));
    base["disputed_amount_delta"] = json!(sum_adjustment_amount(&entries, &["disputed"]));
    base["registry_hash"] = Value::String(
        super::hash_line_items(&entries).unwrap_or_else(|_| "hash_error".to_string()),
    );
    base["scopes"] = Value::Object(scope_map);
    base
}

pub(super) fn build_adjustment_registry_json(
    repo_root: &Path,
    contract: &super::TokenBudgetContractConfig,
) -> Value {
    let source = configured_adjustment_registry_source(repo_root, contract);
    load_adjustment_registry_from_source(&source, contract)
}

fn adjustment_registry_default_path(repo_root: &Path) -> PathBuf {
    repo_root.join("state/token_adjustment_registry.json")
}

fn configured_adjustment_registry_source(
    repo_root: &Path,
    contract: &super::TokenBudgetContractConfig,
) -> Value {
    let source = super::external_truth::configured_defaultable_external_truth_source(
        repo_root,
        "AMAI_TOKEN_ADJUSTMENT_REGISTRY_PATH",
        "token_adjustment_registry",
        "Report-only registry для correction/credit/dispute entries",
        false,
        "state/token_adjustment_registry.json",
        "Если env-binding не задан, token adjustment registry может жить в repo-local state/token_adjustment_registry.json как operator-safe report-only ledger.",
    );
    if source["status"].as_str() == Some("not_configured") {
        return source;
    }
    let mut enriched = source;
    enriched["schema_version"] = Value::String(contract.adjustment_registry_version.clone());
    enriched
}

fn validate_adjustment_scope(scope: &str) -> Result<()> {
    if matches!(scope, "current_session" | "rolling_window" | "lifetime") {
        Ok(())
    } else {
        bail!(
            "unsupported adjustment scope {} (expected current_session, rolling_window or lifetime)",
            scope
        )
    }
}

fn validate_adjustment_kind(kind: &str) -> Result<()> {
    if matches!(kind, "credit_note" | "adjustment_entry" | "dispute_hold") {
        Ok(())
    } else {
        bail!(
            "unsupported adjustment kind {} (expected credit_note, adjustment_entry or dispute_hold)",
            kind
        )
    }
}

fn validate_adjustment_status(status: &str) -> Result<()> {
    if matches!(
        status,
        "requested"
            | "pending_review"
            | "approved_but_unapplied"
            | "applied_report_only"
            | "disputed"
            | "rejected"
    ) {
        Ok(())
    } else {
        bail!(
            "unsupported adjustment status {} (expected requested, pending_review, approved_but_unapplied, applied_report_only, disputed or rejected)",
            status
        )
    }
}

fn adjustment_registry_write_path(repo_root: &Path) -> PathBuf {
    std::env::var("AMAI_TOKEN_ADJUSTMENT_REGISTRY_PATH")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                repo_root.join(path)
            }
        })
        .unwrap_or_else(|| adjustment_registry_default_path(repo_root))
}

fn load_adjustment_registry_file_for_write(path: &Path) -> Result<AdjustmentRegistryFile> {
    if !path.exists() {
        return Ok(AdjustmentRegistryFile::default());
    }
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read adjustment registry {}", path.display()))?;
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse adjustment registry {}", path.display()))
}

fn write_adjustment_registry_file(path: &Path, registry: &AdjustmentRegistryFile) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for adjustment registry {}",
                path.display()
            )
        })?;
    }
    let content =
        serde_json::to_string_pretty(registry).context("failed to encode adjustment registry")?;
    fs::write(path, content)
        .with_context(|| format!("failed to write adjustment registry {}", path.display()))
}

async fn resolve_statement_preview_id_for_scope(
    repo_root: &Path,
    config: &super::TokenBudgetConfigFile,
    budget_profile: Option<&str>,
    include_verify_events: Option<bool>,
    scope: &str,
) -> Result<String> {
    let cfg = AppConfig::from_env()?;
    let db = postgres::connect_admin(&cfg).await?;
    let report = super::collect_report(
        repo_root,
        &db,
        budget_profile,
        include_verify_events.unwrap_or(config.measurement.include_verify_events_by_default),
        None,
    )
    .await?;
    let statement_preview_id =
        report["token_budget_report"]["statement_export_previews"][scope]["statement_preview_id"]
            .as_str()
            .ok_or_else(|| anyhow!("statement preview id unavailable for scope {scope}"))?;
    Ok(statement_preview_id.to_string())
}

pub async fn print_adjustment_registry(
    args: &crate::cli::ObserveTokenAdjustmentRegistryArgs,
) -> Result<()> {
    if let Some(scope) = args.scope.as_deref() {
        validate_adjustment_scope(scope)?;
    }
    let repo_root = config::discover_repo_root(None)?;
    let config = super::load_config(&repo_root)?;
    let registry = build_adjustment_registry_json(&repo_root, &config.contract);
    let payload = if let Some(scope) = args.scope.as_deref() {
        json!({
            "token_adjustment_registry": registry,
            "scope_code": scope,
            "scope_summary": registry["scopes"][scope].clone(),
            "adjustment_request_schema": build_adjustment_request_schema_json(&config.contract),
        })
    } else {
        json!({
            "token_adjustment_registry": registry,
            "adjustment_request_schema": build_adjustment_request_schema_json(&config.contract),
        })
    };
    println!("{}", serde_json::to_string_pretty(&payload)?);
    Ok(())
}

pub async fn add_adjustment_entry(args: &crate::cli::ObserveTokenAdjustmentAddArgs) -> Result<()> {
    validate_adjustment_scope(&args.scope)?;
    validate_adjustment_kind(&args.kind)?;
    validate_adjustment_status(&args.status)?;
    let repo_root = config::discover_repo_root(None)?;
    let config = super::load_config(&repo_root)?;
    let related_statement_id = match (
        args.related_statement_id.as_ref(),
        args.resolve_related_statement_id,
    ) {
        (Some(explicit), _) => Some(explicit.clone()),
        (None, true) => Some(
            resolve_statement_preview_id_for_scope(
                &repo_root,
                &config,
                args.budget_profile.as_deref(),
                args.include_verify_events,
                &args.scope,
            )
            .await?,
        ),
        (None, false) => None,
    };
    let path = adjustment_registry_write_path(&repo_root);
    let mut registry = load_adjustment_registry_file_for_write(&path)?;
    let entry = AdjustmentRegistryEntry {
        adjustment_id: args
            .adjustment_id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        scope_code: args.scope.clone(),
        kind: args.kind.clone(),
        status: args.status.clone(),
        reason_code: args.reason_code.clone(),
        created_at_epoch_ms: super::current_epoch_ms()?,
        tokens_delta: args.tokens_delta,
        amount_delta: args.amount_delta,
        currency_profile: args.currency_profile.clone(),
        related_statement_id: related_statement_id.clone(),
    };
    registry.adjustments.push(entry.clone());
    write_adjustment_registry_file(&path, &registry)?;
    let registry_preview = build_adjustment_registry_json(&repo_root, &config.contract);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "token_adjustment_add": {
                "registry_path": path.display().to_string(),
                "entry": adjustment_entry_json(&entry),
                "scope_summary": registry_preview["scopes"][args.scope.as_str()].clone(),
                "registry_status": registry_preview["status"].clone(),
                "resolved_related_statement_id": related_statement_id,
                "request_schema_version": config.contract.adjustment_request_schema_version,
                "note": "Adjustment entry materialized отдельно от token events: historical usage не переписывается, а correction/dispute живёт как отдельный registry layer."
            }
        }))?
    );
    Ok(())
}
