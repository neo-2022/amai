use super::*;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct RateCardFile {
    pub(super) schema_version: String,
    pub(super) rate_card_version: String,
    pub(super) currency_profile: String,
    pub(super) provider: String,
    pub(super) default_input_cost_per_1k_tokens: f64,
    pub(super) default_output_cost_per_1k_tokens: f64,
    #[serde(default)]
    pub(super) effective_from_epoch_ms: Option<i64>,
    #[serde(default)]
    pub(super) effective_to_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct ProviderUsageExportFile {
    pub(super) schema_version: String,
    pub(super) provider: String,
    #[serde(default)]
    pub(super) currency_profile: Option<String>,
    #[serde(default)]
    pub(super) scopes: Vec<ProviderUsageScopeEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ProviderUsageScopeEntry {
    pub(super) scope_code: String,
    #[serde(default)]
    pub(super) input_tokens: Option<u64>,
    #[serde(default)]
    pub(super) output_tokens: Option<u64>,
    #[serde(default)]
    pub(super) total_tokens: Option<u64>,
    #[serde(default)]
    pub(super) provider_cost_amount: Option<f64>,
    #[serde(default)]
    pub(super) currency_profile: Option<String>,
    #[serde(default)]
    pub(super) period_start_epoch_ms: Option<i64>,
    #[serde(default)]
    pub(super) period_end_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct ProviderInvoiceExportFile {
    pub(super) schema_version: String,
    pub(super) provider: String,
    #[serde(default)]
    pub(super) currency_profile: Option<String>,
    #[serde(default)]
    pub(super) scopes: Vec<ProviderInvoiceScopeEntry>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ProviderInvoiceScopeEntry {
    pub(super) scope_code: String,
    pub(super) invoice_amount: f64,
    #[serde(default)]
    pub(super) currency_profile: Option<String>,
    #[serde(default)]
    pub(super) invoice_id: Option<String>,
    #[serde(default)]
    pub(super) period_start_epoch_ms: Option<i64>,
    #[serde(default)]
    pub(super) period_end_epoch_ms: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct InfraCostProfileFile {
    pub(super) schema_version: String,
    pub(super) infra_cost_profile_version: String,
    pub(super) currency_profile: String,
    #[serde(default)]
    pub(super) provider: Option<String>,
    pub(super) cost_per_1k_internal_billed_tokens: f64,
    #[serde(default)]
    pub(super) cost_per_live_event: f64,
    #[serde(default)]
    pub(super) fixed_scope_cost_amount: f64,
    #[serde(default)]
    pub(super) effective_from_epoch_ms: Option<i64>,
    #[serde(default)]
    pub(super) effective_to_epoch_ms: Option<i64>,
}

pub(super) fn build_external_truth_sources_json(repo_root: &Path) -> Value {
    json!({
        "provider_usage_export": configured_provider_usage_source(repo_root),
        "provider_invoice_export": configured_provider_invoice_source(repo_root),
        "provider_rate_card": configured_provider_rate_card_source(repo_root),
        "infra_cost_profile": configured_infra_cost_profile_source(repo_root),
    })
}

pub(super) fn external_truth_source_roles(code: &str) -> Value {
    match code {
        "provider_usage_export" => json!({
            "required_for_usage_truth": true,
            "required_for_cost_truth": true,
            "required_for_invoice_evidence": false,
            "required_for_margin_truth": true,
        }),
        "provider_rate_card" => json!({
            "required_for_usage_truth": false,
            "required_for_cost_truth": true,
            "required_for_invoice_evidence": false,
            "required_for_margin_truth": true,
        }),
        "provider_invoice_export" => json!({
            "required_for_usage_truth": false,
            "required_for_cost_truth": false,
            "required_for_invoice_evidence": true,
            "required_for_margin_truth": false,
        }),
        "infra_cost_profile" => json!({
            "required_for_usage_truth": false,
            "required_for_cost_truth": false,
            "required_for_invoice_evidence": false,
            "required_for_margin_truth": true,
        }),
        _ => json!({
            "required_for_usage_truth": false,
            "required_for_cost_truth": false,
            "required_for_invoice_evidence": false,
            "required_for_margin_truth": false,
        }),
    }
}

fn configured_external_truth_source(
    repo_root: &Path,
    env_var: &str,
    code: &str,
    label: &str,
    required_for_reconciliation: bool,
) -> Value {
    let configured_value = std::env::var(env_var)
        .ok()
        .filter(|value| !value.trim().is_empty());
    let resolved_path = configured_value.as_ref().map(|raw| {
        let candidate = PathBuf::from(raw);
        if candidate.is_absolute() {
            candidate
        } else {
            repo_root.join(candidate)
        }
    });
    let path_exists = resolved_path
        .as_ref()
        .map(|path| path.exists())
        .unwrap_or(false);
    let status = match (configured_value.as_ref(), path_exists) {
        (None, _) => "not_configured",
        (Some(_), false) => "configured_path_missing",
        (Some(_), true) => "configured_existing_path",
    };
    let binding_status = match status {
        "not_configured" => "not_configured",
        "configured_path_missing" => "configured_path_missing",
        "configured_existing_path" => "configured_but_unbound",
        _ => "unknown",
    };
    json!({
        "code": code,
        "label": label,
        "env_var": env_var,
        "required_for_reconciliation": required_for_reconciliation,
        "truth_roles": external_truth_source_roles(code),
        "configured_value": configured_value,
        "resolved_path": resolved_path.map(|path| path.display().to_string()),
        "status": status,
        "binding_status": binding_status,
        "note": "Источник может уже существовать как файл, но пока Amai не привязывает его автоматически к canonical reconciliation ledger."
    })
}

pub(super) fn configured_defaultable_external_truth_source(
    repo_root: &Path,
    env_var: &str,
    code: &str,
    label: &str,
    required_for_reconciliation: bool,
    default_relative_path: &str,
    note: &str,
) -> Value {
    let source = configured_external_truth_source(
        repo_root,
        env_var,
        code,
        label,
        required_for_reconciliation,
    );
    if source["status"].as_str() != Some("not_configured") {
        return source;
    }
    let default_path = repo_root.join(default_relative_path);
    let default_exists = default_path.exists();
    json!({
        "code": code,
        "label": label,
        "env_var": env_var,
        "required_for_reconciliation": required_for_reconciliation,
        "truth_roles": external_truth_source_roles(code),
        "configured_value": Value::Null,
        "resolved_path": default_path.display().to_string(),
        "status": if default_exists {
            "default_existing_path"
        } else {
            "default_path_missing"
        },
        "binding_status": if default_exists {
            "default_but_unbound"
        } else {
            "not_configured"
        },
        "note": note
    })
}

pub(super) fn provider_usage_default_path(repo_root: &Path) -> PathBuf {
    repo_root.join("state/provider_usage_export.json")
}

pub(super) fn provider_invoice_default_path(repo_root: &Path) -> PathBuf {
    repo_root.join("state/provider_invoice_export.json")
}

pub(super) fn provider_rate_card_default_path(repo_root: &Path) -> PathBuf {
    repo_root.join("state/provider_rate_card.json")
}

pub(super) fn infra_cost_profile_default_path(repo_root: &Path) -> PathBuf {
    repo_root.join("state/infra_cost_profile.json")
}

pub(super) fn configured_provider_usage_source(repo_root: &Path) -> Value {
    configured_defaultable_external_truth_source(
        repo_root,
        "AMAI_PROVIDER_USAGE_EXPORT_PATH",
        "provider_usage_export",
        "Выгрузка usage/tokens от внешнего model provider",
        true,
        "state/provider_usage_export.json",
        "Если env-binding не задан, provider usage export может жить в repo-local state/provider_usage_export.json как report-only reconciliation source.",
    )
}

pub(super) fn configured_provider_invoice_source(repo_root: &Path) -> Value {
    configured_defaultable_external_truth_source(
        repo_root,
        "AMAI_PROVIDER_INVOICE_EXPORT_PATH",
        "provider_invoice_export",
        "Invoice/export от внешнего provider",
        false,
        "state/provider_invoice_export.json",
        "Если env-binding не задан, provider invoice export может жить в repo-local state/provider_invoice_export.json как optional settlement-side evidence source.",
    )
}

pub(super) fn configured_provider_rate_card_source(repo_root: &Path) -> Value {
    configured_defaultable_external_truth_source(
        repo_root,
        "AMAI_PROVIDER_RATE_CARD_PATH",
        "provider_rate_card",
        "Versioned rate-card для денежной конверcии tokenonomics",
        true,
        "state/provider_rate_card.json",
        "Если env-binding не задан, provider rate card может жить в repo-local state/provider_rate_card.json как versioned money conversion source.",
    )
}

pub(super) fn configured_infra_cost_profile_source(repo_root: &Path) -> Value {
    configured_defaultable_external_truth_source(
        repo_root,
        "AMAI_INFRA_COST_PROFILE_PATH",
        "infra_cost_profile",
        "Профиль собственных infra costs для Amai",
        false,
        "state/infra_cost_profile.json",
        "Если env-binding не задан, infra cost profile может жить в repo-local state/infra_cost_profile.json как report-only margin source.",
    )
}

pub(super) fn parse_rate_card_file(raw: &str) -> Result<RateCardFile> {
    serde_json::from_str::<RateCardFile>(raw)
        .or_else(|_| toml::from_str::<RateCardFile>(raw).map_err(anyhow::Error::from))
        .context("failed to parse rate-card file as JSON or TOML")
}

fn parse_provider_usage_export_file(raw: &str) -> Result<ProviderUsageExportFile> {
    serde_json::from_str::<ProviderUsageExportFile>(raw)
        .or_else(|_| toml::from_str::<ProviderUsageExportFile>(raw).map_err(anyhow::Error::from))
        .context("failed to parse provider usage export as JSON or TOML")
}

fn parse_provider_invoice_export_file(raw: &str) -> Result<ProviderInvoiceExportFile> {
    serde_json::from_str::<ProviderInvoiceExportFile>(raw)
        .or_else(|_| toml::from_str::<ProviderInvoiceExportFile>(raw).map_err(anyhow::Error::from))
        .context("failed to parse provider invoice export as JSON or TOML")
}

pub(super) fn parse_infra_cost_profile_file(raw: &str) -> Result<InfraCostProfileFile> {
    serde_json::from_str::<InfraCostProfileFile>(raw)
        .or_else(|_| toml::from_str::<InfraCostProfileFile>(raw).map_err(anyhow::Error::from))
        .context("failed to parse infra cost profile as JSON or TOML")
}

pub(super) fn bind_rate_card_json_from_source(
    source: &Value,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let mut base = json!({
        "binding_model_version": contract.rate_card_binding_model_version.clone(),
        "configured_contract_version": contract.rate_card_version.clone(),
        "configured_currency_profile": contract.currency_profile.clone(),
        "source": source.clone(),
        "source_bytes": Value::Null,
        "source_sha256": Value::Null,
        "source_last_modified_epoch_ms": Value::Null,
        "money_conversion_enabled": false,
        "status": source["status"].clone(),
        "bound_rate_card_version": Value::Null,
        "bound_currency_profile": Value::Null,
        "provider": Value::Null,
        "default_input_cost_per_1k_tokens": Value::Null,
        "default_output_cost_per_1k_tokens": Value::Null,
        "effective_from_epoch_ms": Value::Null,
        "effective_to_epoch_ms": Value::Null,
        "temporal_scope_state": "source_period_unspecified",
        "note": "Денежная конверсия включается только после честного bind на versioned rate-card file."
    });

    if !matches!(
        source["status"].as_str(),
        Some("configured_existing_path" | "default_existing_path")
    ) {
        return base;
    }
    let Some(path) = source["resolved_path"].as_str() else {
        return base;
    };

    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            base["status"] = Value::String("read_error".to_string());
            base["source"]["binding_status"] = Value::String("read_error".to_string());
            base["read_error"] = Value::String(error.to_string());
            return base;
        }
    };
    attach_source_file_evidence(&mut base, Path::new(path), &raw);
    let rate_card = match parse_rate_card_file(&raw) {
        Ok(rate_card) => rate_card,
        Err(error) => {
            base["status"] = Value::String("parse_error".to_string());
            base["source"]["binding_status"] = Value::String("parse_error".to_string());
            base["parse_error"] = Value::String(error.to_string());
            return base;
        }
    };

    let money_conversion_enabled = rate_card.default_input_cost_per_1k_tokens > 0.0
        && rate_card.default_output_cost_per_1k_tokens > 0.0;
    base["status"] = Value::String(if money_conversion_enabled {
        "priced_bound".to_string()
    } else {
        "bound_but_unpriced".to_string()
    });
    base["source"]["binding_status"] = base["status"].clone();
    base["money_conversion_enabled"] = Value::Bool(money_conversion_enabled);
    base["schema_version"] = Value::String(rate_card.schema_version);
    base["bound_rate_card_version"] = Value::String(rate_card.rate_card_version);
    base["bound_currency_profile"] = Value::String(rate_card.currency_profile);
    base["provider"] = Value::String(rate_card.provider);
    base["default_input_cost_per_1k_tokens"] =
        Value::from(rate_card.default_input_cost_per_1k_tokens);
    base["default_output_cost_per_1k_tokens"] =
        Value::from(rate_card.default_output_cost_per_1k_tokens);
    base["effective_from_epoch_ms"] = match rate_card.effective_from_epoch_ms {
        Some(value) => json!(value),
        None => Value::Null,
    };
    base["effective_to_epoch_ms"] = match rate_card.effective_to_epoch_ms {
        Some(value) => json!(value),
        None => Value::Null,
    };
    base["temporal_scope_state"] = Value::String(
        source_temporal_scope_state(
            rate_card.effective_from_epoch_ms,
            rate_card.effective_to_epoch_ms,
        )
        .to_string(),
    );
    base
}

pub(super) fn build_rate_card_json(
    repo_root: &Path,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let source = configured_provider_rate_card_source(repo_root);
    bind_rate_card_json_from_source(&source, contract)
}

pub(super) fn bind_infra_cost_profile_json_from_source(
    source: &Value,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let mut base = json!({
        "binding_model_version": contract.infra_cost_binding_model_version.clone(),
        "configured_contract_version": contract.infra_cost_profile_version.clone(),
        "source": source.clone(),
        "source_bytes": Value::Null,
        "source_sha256": Value::Null,
        "source_last_modified_epoch_ms": Value::Null,
        "status": source["status"].clone(),
        "schema_version": Value::Null,
        "bound_profile_version": Value::Null,
        "bound_currency_profile": Value::Null,
        "provider": Value::Null,
        "cost_per_1k_internal_billed_tokens": Value::Null,
        "cost_per_live_event": Value::Null,
        "fixed_scope_cost_amount": Value::Null,
        "effective_from_epoch_ms": Value::Null,
        "effective_to_epoch_ms": Value::Null,
        "temporal_scope_state": "source_period_unspecified",
        "money_margin_enabled": false,
        "note": "Infra cost profile начинает влиять на margin preview только после честного bind на versioned machine-readable profile."
    });

    if !matches!(
        source["status"].as_str(),
        Some("configured_existing_path" | "default_existing_path")
    ) {
        return base;
    }
    let Some(path) = source["resolved_path"].as_str() else {
        return base;
    };

    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            base["status"] = Value::String("read_error".to_string());
            base["source"]["binding_status"] = Value::String("read_error".to_string());
            base["read_error"] = Value::String(error.to_string());
            return base;
        }
    };
    attach_source_file_evidence(&mut base, Path::new(path), &raw);
    let profile = match parse_infra_cost_profile_file(&raw) {
        Ok(profile) => profile,
        Err(error) => {
            base["status"] = Value::String("parse_error".to_string());
            base["source"]["binding_status"] = Value::String("parse_error".to_string());
            base["parse_error"] = Value::String(error.to_string());
            return base;
        }
    };

    let money_margin_enabled = profile.cost_per_1k_internal_billed_tokens > 0.0
        || profile.cost_per_live_event > 0.0
        || profile.fixed_scope_cost_amount > 0.0;
    base["status"] = Value::String(if money_margin_enabled {
        "priced_bound".to_string()
    } else {
        "bound_but_unpriced".to_string()
    });
    base["source"]["binding_status"] = base["status"].clone();
    base["schema_version"] = Value::String(profile.schema_version);
    base["bound_profile_version"] = Value::String(profile.infra_cost_profile_version);
    base["bound_currency_profile"] = Value::String(profile.currency_profile);
    base["provider"] = match profile.provider {
        Some(provider) => Value::String(provider),
        None => Value::Null,
    };
    base["cost_per_1k_internal_billed_tokens"] =
        Value::from(profile.cost_per_1k_internal_billed_tokens);
    base["cost_per_live_event"] = Value::from(profile.cost_per_live_event);
    base["fixed_scope_cost_amount"] = Value::from(profile.fixed_scope_cost_amount);
    base["effective_from_epoch_ms"] = match profile.effective_from_epoch_ms {
        Some(value) => json!(value),
        None => Value::Null,
    };
    base["effective_to_epoch_ms"] = match profile.effective_to_epoch_ms {
        Some(value) => json!(value),
        None => Value::Null,
    };
    base["temporal_scope_state"] = Value::String(
        source_temporal_scope_state(
            profile.effective_from_epoch_ms,
            profile.effective_to_epoch_ms,
        )
        .to_string(),
    );
    base["money_margin_enabled"] = Value::Bool(money_margin_enabled);
    base
}

pub(super) fn build_infra_cost_profile_json(
    repo_root: &Path,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let source = configured_infra_cost_profile_source(repo_root);
    bind_infra_cost_profile_json_from_source(&source, contract)
}

fn provider_usage_total_tokens(entry: &ProviderUsageScopeEntry) -> Option<u64> {
    entry
        .total_tokens
        .or_else(|| match (entry.input_tokens, entry.output_tokens) {
            (Some(input), Some(output)) => Some(input.saturating_add(output)),
            (Some(input), None) => Some(input),
            (None, Some(output)) => Some(output),
            (None, None) => None,
        })
}

fn provider_usage_cost_amount(entry: &ProviderUsageScopeEntry, rate_card: &Value) -> Option<f64> {
    if let Some(amount) = entry.provider_cost_amount {
        return Some(amount);
    }
    let input_rate = rate_card["default_input_cost_per_1k_tokens"].as_f64()?;
    let output_rate = rate_card["default_output_cost_per_1k_tokens"].as_f64()?;
    let input_tokens = entry.input_tokens?;
    let output_tokens = entry.output_tokens?;
    Some(
        (input_tokens as f64 / 1000.0) * input_rate + (output_tokens as f64 / 1000.0) * output_rate,
    )
}

pub(super) fn load_provider_usage_binding_from_source(source: &Value, rate_card: &Value) -> Value {
    let mut base = json!({
        "status": source["status"].clone(),
        "source": source.clone(),
        "source_bytes": Value::Null,
        "source_sha256": Value::Null,
        "source_last_modified_epoch_ms": Value::Null,
        "schema_version": Value::Null,
        "provider": Value::Null,
        "bound_currency_profile": Value::Null,
        "scope_count": 0,
        "scopes": {},
        "cost_binding_status": if rate_card["money_conversion_enabled"].as_bool() == Some(true) {
            "awaiting_usage_export"
        } else {
            "unpriced_rate_card"
        },
        "note": "Provider usage binding должен показывать реальные billed tokens по scope, а не подменять их lower-bound savings."
    });

    if !matches!(
        source["status"].as_str(),
        Some("configured_existing_path" | "default_existing_path")
    ) {
        return base;
    }
    let Some(path) = source["resolved_path"].as_str() else {
        return base;
    };

    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            base["status"] = Value::String("read_error".to_string());
            base["source"]["binding_status"] = Value::String("read_error".to_string());
            base["read_error"] = Value::String(error.to_string());
            return base;
        }
    };
    attach_source_file_evidence(&mut base, Path::new(path), &raw);
    let export = match parse_provider_usage_export_file(&raw) {
        Ok(export) => export,
        Err(error) => {
            base["status"] = Value::String("parse_error".to_string());
            base["source"]["binding_status"] = Value::String("parse_error".to_string());
            base["parse_error"] = Value::String(error.to_string());
            return base;
        }
    };

    let mut scope_map = serde_json::Map::new();
    let mut has_any_cost = false;
    for entry in &export.scopes {
        let total_tokens = provider_usage_total_tokens(entry);
        let cost_amount = provider_usage_cost_amount(entry, rate_card);
        if cost_amount.is_some() {
            has_any_cost = true;
        }
        scope_map.insert(
            entry.scope_code.clone(),
            json!({
                "input_tokens": entry.input_tokens,
                "output_tokens": entry.output_tokens,
                "total_tokens": total_tokens,
                "provider_cost_amount": cost_amount,
                "period_start_epoch_ms": entry.period_start_epoch_ms,
                "period_end_epoch_ms": entry.period_end_epoch_ms,
                "temporal_scope_state": source_temporal_scope_state(entry.period_start_epoch_ms, entry.period_end_epoch_ms),
                "currency_profile": entry
                    .currency_profile
                    .clone()
                    .or_else(|| export.currency_profile.clone()),
            }),
        );
    }

    base["schema_version"] = Value::String(export.schema_version);
    base["provider"] = Value::String(export.provider);
    base["bound_currency_profile"] = match export.currency_profile {
        Some(currency) => Value::String(currency),
        None => Value::Null,
    };
    base["scope_count"] = json!(scope_map.len());
    base["scopes"] = Value::Object(scope_map);
    base["status"] = Value::String(if has_any_cost {
        "usage_and_cost_bound".to_string()
    } else {
        "usage_bound".to_string()
    });
    base["source"]["binding_status"] = base["status"].clone();
    base["cost_binding_status"] = Value::String(if has_any_cost {
        "cost_bound".to_string()
    } else if rate_card["money_conversion_enabled"].as_bool() == Some(true) {
        "usage_bound_cost_unavailable".to_string()
    } else {
        "unpriced_rate_card".to_string()
    });
    base
}

pub(super) fn load_provider_invoice_binding_from_source(source: &Value) -> Value {
    let mut base = json!({
        "status": source["status"].clone(),
        "source": source.clone(),
        "source_bytes": Value::Null,
        "source_sha256": Value::Null,
        "source_last_modified_epoch_ms": Value::Null,
        "schema_version": Value::Null,
        "provider": Value::Null,
        "bound_currency_profile": Value::Null,
        "scope_count": 0,
        "scopes": {},
        "note": "Provider invoice binding остаётся optional: он не подменяет usage truth, а только даёт отдельный settlement-side evidence слой."
    });

    if !matches!(
        source["status"].as_str(),
        Some("configured_existing_path" | "default_existing_path")
    ) {
        return base;
    }
    let Some(path) = source["resolved_path"].as_str() else {
        return base;
    };

    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) => {
            base["status"] = Value::String("read_error".to_string());
            base["source"]["binding_status"] = Value::String("read_error".to_string());
            base["read_error"] = Value::String(error.to_string());
            return base;
        }
    };
    attach_source_file_evidence(&mut base, Path::new(path), &raw);
    let export = match parse_provider_invoice_export_file(&raw) {
        Ok(export) => export,
        Err(error) => {
            base["status"] = Value::String("parse_error".to_string());
            base["source"]["binding_status"] = Value::String("parse_error".to_string());
            base["parse_error"] = Value::String(error.to_string());
            return base;
        }
    };

    let mut scope_map = serde_json::Map::new();
    for entry in &export.scopes {
        scope_map.insert(
            entry.scope_code.clone(),
            json!({
                "invoice_amount": entry.invoice_amount,
                "period_start_epoch_ms": entry.period_start_epoch_ms,
                "period_end_epoch_ms": entry.period_end_epoch_ms,
                "temporal_scope_state": source_temporal_scope_state(entry.period_start_epoch_ms, entry.period_end_epoch_ms),
                "currency_profile": entry
                    .currency_profile
                    .clone()
                    .or_else(|| export.currency_profile.clone()),
                "invoice_id": entry.invoice_id,
            }),
        );
    }

    base["schema_version"] = Value::String(export.schema_version);
    base["provider"] = Value::String(export.provider);
    base["bound_currency_profile"] = match export.currency_profile {
        Some(currency) => Value::String(currency),
        None => Value::Null,
    };
    base["scope_count"] = json!(scope_map.len());
    base["scopes"] = Value::Object(scope_map);
    base["status"] = Value::String("invoice_bound".to_string());
    base["source"]["binding_status"] = Value::String("invoice_bound".to_string());
    base
}
