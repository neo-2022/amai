use super::*;

fn select_scope_events(
    events: &[TokenBudgetEvent],
    profile: &ResolvedProfile,
    scope_code: &str,
    now_epoch_ms: i64,
) -> Result<(String, Vec<TokenBudgetEvent>)> {
    match scope_code {
        "current_session" => Ok((
            "текущая сессия".to_string(),
            current_session_events(
                events,
                profile.session_gap_minutes.saturating_mul(60_000) as i64,
            ),
        )),
        "rolling_window" => {
            let hours = profile
                .rolling_window_hours
                .ok_or_else(|| anyhow!("selected budget profile has no rolling window"))?;
            let lower_bound = now_epoch_ms.saturating_sub((hours as i64).saturating_mul(3_600_000));
            Ok((
                format!("окно {}", profile.display_name),
                events
                    .iter()
                    .filter(|event| event.created_at_epoch_ms >= lower_bound)
                    .cloned()
                    .collect::<Vec<_>>(),
            ))
        }
        "lifetime" => Ok(("всё время записи".to_string(), events.to_vec())),
        _ => bail!("unknown scope for token export surface: {scope_code}"),
    }
}

fn build_contractual_sources_value(
    report: &Value,
    repo_root: &Path,
    scope_code: &str,
    scope_label: &str,
) -> Value {
    let external_truth_sources = if report["token_budget_report"]["external_truth_sources"]
        .is_null()
    {
        report["token_budget_report"]["reconciliation_contract"]["external_truth_sources"].clone()
    } else {
        report["token_budget_report"]["external_truth_sources"].clone()
    };
    let statement_export_preview =
        report["token_budget_report"]["statement_export_previews"][scope_code].clone();
    let mut customer_contractual_boundary =
        statement_export_preview["customer_contractual_boundary"].clone();
    customer_contractual_boundary["surface_kind"] =
        json!("customer_contractual_sources_report_only");
    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "external_truth_sources": external_truth_sources,
        "external_truth_manifest": report["token_budget_report"]["external_truth_manifest"].clone(),
        "rate_card": report["token_budget_report"]["rate_card"].clone(),
        "infra_cost_profile": report["token_budget_report"]["infra_cost_profile"].clone(),
        "reconciliation_contract": report["token_budget_report"]["reconciliation_contract"].clone(),
        "provider_usage_binding": report["token_budget_report"]["reconciliation_contract"]["external_truth_bindings"]["provider_usage_export"].clone(),
        "provider_invoice_binding": report["token_budget_report"]["reconciliation_contract"]["external_truth_bindings"]["provider_invoice_export"].clone(),
        "statement_preview": report["token_budget_report"]["statement_previews"][scope_code].clone(),
        "reconciliation_preview": report["token_budget_report"]["reconciliation_previews"][scope_code].clone(),
        "margin_scope": report["token_budget_report"]["margin_view"][scope_code].clone(),
        "statement_export_preview": statement_export_preview,
        "settlement_report_preview": report["token_budget_report"]["settlement_report_previews"][scope_code].clone(),
        "settlement_activation_governance": report["token_budget_report"]["statement_export_previews"][scope_code]["settlement_activation_governance"].clone(),
        "adjustment_activation_governance": report["token_budget_report"]["statement_export_previews"][scope_code]["adjustment_activation_governance"].clone(),
        "transactional_statuses": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["transactional_statuses"].clone(),
        "customer_contractual_boundary": customer_contractual_boundary,
        "suggested_repo_local_paths": {
            "provider_usage_export": external_truth::provider_usage_default_path(repo_root).display().to_string(),
            "provider_invoice_export": external_truth::provider_invoice_default_path(repo_root).display().to_string(),
            "provider_rate_card": external_truth::provider_rate_card_default_path(repo_root).display().to_string(),
            "infra_cost_profile": external_truth::infra_cost_profile_default_path(repo_root).display().to_string(),
        },
        "note": "Этот inspect-layer нужен затем, чтобы provider truth sources, rate card, reconciliation и margin были видны как отдельный contractual contour, а не прятались внутри большого token report."
    })
}

pub(crate) async fn print_report(db: &Client, args: &ObserveTokenReportArgs) -> Result<()> {
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

pub(crate) async fn print_client_limit_hourly_burn(
    db: &Client,
    args: &ObserveClientLimitHourlyBurnArgs,
) -> Result<()> {
    let burn = collect_client_limit_hourly_burn_surface(
        db,
        args.window_minutes,
        args.max_live_age_seconds,
        args.min_history_span_minutes,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&burn)?);
    Ok(())
}

pub(crate) async fn print_client_limit_trend_analysis(
    db: &Client,
    args: &ObserveClientLimitTrendAnalysisArgs,
) -> Result<()> {
    let analysis = collect_exact_client_limit_trend_analysis(
        db,
        args.window_minutes,
        args.max_live_age_seconds,
        args.lookback_minutes,
        args.persist_snapshot,
    )
    .await?;
    println!("{}", serde_json::to_string_pretty(&analysis)?);
    Ok(())
}

pub(crate) async fn print_evidence_pack(
    db: &Client,
    args: &ObserveTokenEvidencePackArgs,
) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let include_verify = args
        .include_verify_events
        .unwrap_or(config.measurement.include_verify_events_by_default);
    let profile = resolve_profile(&config, args.budget_profile.as_deref(), &repo_root)?;
    let report = collect_report(
        &repo_root,
        db,
        args.budget_profile.as_deref(),
        include_verify,
        None,
    )
    .await?;
    let mut events = load_events(db, include_verify, None).await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let events = reconcile_followup_recovery(&events, profile.session_gap_minutes as i64 * 60_000);
    let now_epoch_ms = current_epoch_ms()?;
    let scope_code = args.scope.as_str();
    let (scope_label, scoped_events) =
        select_scope_events(&events, &profile, scope_code, now_epoch_ms)?;

    let pack = build_contractual_evidence_pack(
        &report,
        scope_code,
        &scope_label,
        &scoped_events,
        &config.contract,
        &profile,
        include_verify,
        now_epoch_ms,
    )?;
    let rendered = serde_json::to_string_pretty(&pack)?;
    if let Some(path) = &args.output {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(path, rendered).with_context(|| format!("failed to write {}", path.display()))?;
        println!("{}", path.display());
    } else {
        println!("{}", rendered);
    }
    Ok(())
}

pub(crate) async fn print_contractual_sources(
    db: &Client,
    args: &ObserveTokenContractualSourcesArgs,
) -> Result<()> {
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
    let scope_code = args.scope.as_str();
    let scope_label =
        report["token_budget_report"]["statement_previews"][scope_code]["scope_label"]
            .as_str()
            .ok_or_else(|| {
                anyhow!("unknown or unavailable scope for token contractual sources: {scope_code}")
            })?
            .to_string();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "token_contractual_sources": build_contractual_sources_value(
                &report,
                &repo_root,
                scope_code,
                &scope_label,
            )
        }))?
    );
    Ok(())
}

pub(crate) async fn print_statement_export_bundle(
    db: &Client,
    args: &ObserveTokenStatementExportArgs,
) -> Result<()> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let include_verify = args
        .include_verify_events
        .unwrap_or(config.measurement.include_verify_events_by_default);
    let profile = resolve_profile(&config, args.budget_profile.as_deref(), &repo_root)?;
    let report = collect_report(
        &repo_root,
        db,
        args.budget_profile.as_deref(),
        include_verify,
        None,
    )
    .await?;
    let mut events = load_events(db, include_verify, None).await?;
    events.sort_by_key(|event| event.created_at_epoch_ms);
    let events = reconcile_followup_recovery(&events, profile.session_gap_minutes as i64 * 60_000);
    let now_epoch_ms = current_epoch_ms()?;
    let scope_code = args.scope.as_str();
    let (scope_label, scoped_events) =
        select_scope_events(&events, &profile, scope_code, now_epoch_ms)?;
    let evidence_pack = build_contractual_evidence_pack(
        &report,
        scope_code,
        &scope_label,
        &scoped_events,
        &config.contract,
        &profile,
        include_verify,
        now_epoch_ms,
    )?;
    let contractual_sources =
        build_contractual_sources_value(&report, &repo_root, scope_code, &scope_label);
    let statement_export_preview =
        report["token_budget_report"]["statement_export_previews"][scope_code].clone();
    if statement_export_preview.is_null() {
        bail!("unknown or unavailable scope for token statement export: {scope_code}");
    }
    let mut customer_contractual_boundary =
        statement_export_preview["customer_contractual_boundary"].clone();
    customer_contractual_boundary["surface_kind"] = json!("customer_review_bundle_report_only");
    let settlement_report_preview =
        settlement_report_preview_from_export(&config.contract, &statement_export_preview);
    let bundle = json!({
        "token_statement_export_bundle": {
            "bundle_version": "token-statement-export-bundle-v3",
            "generated_at_epoch_ms": now_epoch_ms,
            "scope_code": scope_code,
            "scope_label": scope_label,
            "report_only": true,
            "statement_preview_id": statement_export_preview["statement_preview_id"].clone(),
            "files": {
                "manifest": "manifest.json",
                "settlement_report_preview": "settlement_report_preview.json",
                "statement_export_preview": "statement_export_preview.json",
                "contractual_evidence_pack": "contractual_evidence_pack.json",
                "token_contractual_sources": "token_contractual_sources.json",
            },
        "settlement_report_preview": settlement_report_preview,
        "statement_export_preview": statement_export_preview.clone(),
        "contractual_evidence_pack": evidence_pack["contractual_evidence_pack"].clone(),
        "token_contractual_sources": contractual_sources,
        "customer_contractual_boundary": customer_contractual_boundary.clone(),
        "settlement_activation_governance": statement_export_preview["settlement_activation_governance"].clone(),
        "adjustment_activation_governance": statement_export_preview["adjustment_activation_governance"].clone(),
        "surface_kind": "customer_review_bundle_report_only",
        "self_serve_state": "self_serve_ready_report_only",
        "invoice_grade": false,
        "operational_telemetry_included": false,
        "redaction_policy": "raw_query_text_removed_keep_query_hash_and_token_state",
        "note": "Этот bundle собирает customer-facing statement preview, evidence pack и contractual sources в один report-only export surface. Он пригоден для review/audit, но не для invoice."
        }
    });
    if let Some(output_dir) = &args.output_dir {
        fs::create_dir_all(output_dir)
            .with_context(|| format!("failed to create {}", output_dir.display()))?;
        let root = &bundle["token_statement_export_bundle"];
        let manifest = json!({
            "bundle_version": root["bundle_version"].clone(),
            "generated_at_epoch_ms": root["generated_at_epoch_ms"].clone(),
            "scope_code": root["scope_code"].clone(),
            "scope_label": root["scope_label"].clone(),
            "report_only": root["report_only"].clone(),
            "surface_kind": root["surface_kind"].clone(),
            "self_serve_state": root["self_serve_state"].clone(),
            "invoice_grade": root["invoice_grade"].clone(),
            "operational_telemetry_included": root["operational_telemetry_included"].clone(),
            "customer_contractual_boundary": root["customer_contractual_boundary"].clone(),
            "settlement_activation_governance": root["settlement_activation_governance"].clone(),
            "adjustment_activation_governance": root["adjustment_activation_governance"].clone(),
            "redaction_policy": root["redaction_policy"].clone(),
            "statement_preview_id": root["statement_preview_id"].clone(),
            "files": root["files"].clone(),
            "note": root["note"].clone(),
        });
        let files = [
            ("manifest.json", manifest),
            (
                "settlement_report_preview.json",
                root["settlement_report_preview"].clone(),
            ),
            (
                "statement_export_preview.json",
                root["statement_export_preview"].clone(),
            ),
            (
                "contractual_evidence_pack.json",
                root["contractual_evidence_pack"].clone(),
            ),
            (
                "token_contractual_sources.json",
                root["token_contractual_sources"].clone(),
            ),
        ];
        for (name, payload) in files {
            let path = output_dir.join(name);
            fs::write(&path, serde_json::to_string_pretty(&payload)?)
                .with_context(|| format!("failed to write {}", path.display()))?;
        }
        println!("{}", output_dir.display());
    } else {
        println!("{}", serde_json::to_string_pretty(&bundle)?);
    }
    Ok(())
}

pub(crate) async fn repair_legacy_token_events(
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

#[derive(Debug, Clone)]
pub(crate) struct TokenLedgerRepairRequest {
    pub limit: Option<i64>,
    pub project: Option<String>,
    pub project_prefix: Option<String>,
    pub namespace: Option<String>,
    pub source_kind: Option<String>,
    pub correlation_id: Option<String>,
    pub rewrite_source_kind: Option<String>,
    pub repair_reason: Option<String>,
}

impl TokenLedgerRepairRequest {
    fn has_selector(&self) -> bool {
        self.project.is_some()
            || self.project_prefix.is_some()
            || self.namespace.is_some()
            || self.source_kind.is_some()
            || self.correlation_id.is_some()
    }
}

pub(crate) async fn repair_token_ledger_events(
    db: &Client,
    apply: bool,
    request: TokenLedgerRepairRequest,
) -> Result<()> {
    if request.rewrite_source_kind.is_none() && request.has_selector() {
        bail!(
            "repair-token-ledger selectors require --rewrite-source-kind; otherwise use plain legacy repair without selectors"
        );
    }

    if let Some(rewrite_source_kind) = request.rewrite_source_kind.as_deref() {
        let rows = postgres::list_observability_snapshots_by_kinds(
            db,
            &["token_budget_event"],
            request.limit,
        )
        .await?;
        let mut scanned = 0_u64;
        let mut matched = 0_u64;
        let mut changed = 0_u64;
        let repair_reason = request
            .repair_reason
            .as_deref()
            .unwrap_or("operator_source_kind_rewrite");

        for row in rows {
            scanned += 1;
            let Some(event) = parse_snapshot_event(&row)? else {
                continue;
            };
            if !matches_token_ledger_repair_selector(&event, &request) {
                continue;
            }
            matched += 1;
            if let Some(payload) =
                rewrite_token_ledger_source_kind_payload(&row, rewrite_source_kind, repair_reason)?
            {
                changed += 1;
                if apply {
                    postgres::update_observability_snapshot_payload(db, &row.snapshot_id, &payload)
                        .await?;
                }
            }
        }

        println!(
            "token ledger repair :: scanned={} matched={} changed={} rewrite_source_kind={} mode={}",
            scanned,
            matched,
            changed,
            rewrite_source_kind,
            if apply { "apply" } else { "dry_run" }
        );
        if apply && changed > 0 {
            let repo_root = config::discover_repo_root(None)?;
            let recorded_at_epoch_ms = current_epoch_ms()?;
            bump_dashboard_token_events_invalidation(&repo_root, recorded_at_epoch_ms)?;
            bump_dashboard_live_turn_retrieval_invalidation(&repo_root, recorded_at_epoch_ms)?;
        }
        return Ok(());
    }

    repair_legacy_token_events(db, apply, request.limit).await
}

pub(crate) async fn reverify_legacy_live_events(
    cfg: &AppConfig,
    db: &mut Client,
    apply: bool,
    limit: Option<i64>,
) -> Result<()> {
    let rows =
        postgres::list_observability_snapshots_by_kinds(db, &["token_budget_event"], limit).await?;
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    let measurement = config.measurement.clone();
    let contract = config.contract.clone();
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

        match reverify_live_event_payload(cfg, db, &measurement, &contract, &row).await {
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
