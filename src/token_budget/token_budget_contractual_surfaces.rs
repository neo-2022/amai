use super::*;

pub(super) fn report_contract_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "usage_event_schema_version": contract.usage_event_schema_version.clone(),
        "settlement_statement_version": contract.settlement_statement_version.clone(),
        "metering_event_schema_version": contract.metering_event_schema_version.clone(),
        "usage_lifecycle_model_version": contract.usage_lifecycle_model_version.clone(),
        "baseline_method_version": contract.baseline_method_version.clone(),
        "quality_method_version": contract.quality_method_version.clone(),
        "coverage_model_version": contract.coverage_model_version.clone(),
        "metering_freshness_model_version": contract.metering_freshness_model_version.clone(),
        "agent_cycle_model_version": contract.agent_cycle_model_version.clone(),
        "client_limit_meter_alignment_version": contract.client_limit_meter_alignment_version.clone(),
        "client_limit_baseline_equivalence_version": contract
            .client_limit_baseline_equivalence_version
            .clone(),
        "client_limit_strict_meter_slice_version": contract
            .client_limit_strict_meter_slice_version
            .clone(),
        "client_limit_explicit_boundary_surface_version": contract
            .client_limit_explicit_boundary_surface_version
            .clone(),
        "client_limit_continuity_boundary_rollup_version": contract
            .client_limit_continuity_boundary_rollup_version
            .clone(),
        "client_limit_pre_amai_baseline_source_version": contract
            .client_limit_pre_amai_baseline_source_version
            .clone(),
        "client_limit_frozen_gap_review_surface_version": contract
            .client_limit_frozen_gap_review_surface_version
            .clone(),
        "client_limit_reviewed_frozen_debt_export_surface_version": contract
            .client_limit_reviewed_frozen_debt_export_surface_version
            .clone(),
        "excluded_taxonomy_version": contract.excluded_taxonomy_version.clone(),
        "dedup_contract_version": contract.dedup_contract_version.clone(),
        "backfill_policy_version": contract.backfill_policy_version.clone(),
        "correction_policy_version": contract.correction_policy_version.clone(),
        "freeze_close_policy_version": contract.freeze_close_policy_version.clone(),
        "late_arrival_policy_version": contract.late_arrival_policy_version.clone(),
        "dispute_policy_version": contract.dispute_policy_version.clone(),
        "settlement_lifecycle_model_version": contract.settlement_lifecycle_model_version.clone(),
        "statement_period_governance_version": contract.statement_period_governance_version.clone(),
        "adjustment_preview_model_version": contract.adjustment_preview_model_version.clone(),
        "adjustment_request_schema_version": contract.adjustment_request_schema_version.clone(),
        "adjustment_registry_version": contract.adjustment_registry_version.clone(),
        "rate_card_binding_model_version": contract.rate_card_binding_model_version.clone(),
        "infra_cost_binding_model_version": contract.infra_cost_binding_model_version.clone(),
        "telemetry_surface_split_version": contract.telemetry_surface_split_version.clone(),
        "event_time_policy_version": contract.event_time_policy_version.clone(),
        "billing_policy_version": contract.billing_policy_version.clone(),
        "suitability_model_version": contract.suitability_model_version.clone(),
        "contractual_readiness_model_version": contract.contractual_readiness_model_version.clone(),
        "customer_contractual_boundary_version": contract.customer_contractual_boundary_version.clone(),
        "settlement_activation_governance_version": contract
            .settlement_activation_governance_version
            .clone(),
        "adjustment_activation_governance_version": contract
            .adjustment_activation_governance_version
            .clone(),
        "billing_mode": contract.billing_mode.clone(),
        "reconciliation_contract_version": contract.reconciliation_contract_version.clone(),
        "margin_model_version": contract.margin_model_version.clone(),
        "infra_cost_profile_version": contract.infra_cost_profile_version.clone(),
        "contractual_evidence_pack_version": contract.contractual_evidence_pack_version.clone(),
        "contractual_statement_export_version": contract.contractual_statement_export_version.clone(),
        "settlement_report_preview_version": contract.settlement_report_preview_version.clone(),
        "rate_card_version": contract.rate_card_version.clone(),
        "currency_profile": contract.currency_profile.clone(),
        "settlement_status": contract.settlement_status.clone(),
        "note": "Сейчас tokenonomics работает в report-only режиме: metering и lower-bound semantics уже materialized, но money-facing billable settlement ещё не включён."
    })
}

pub(super) fn token_contract_metadata_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "usage_event_schema_version": contract.usage_event_schema_version.clone(),
        "settlement_statement_version": contract.settlement_statement_version.clone(),
        "metering_event_schema_version": contract.metering_event_schema_version.clone(),
        "usage_lifecycle_model_version": contract.usage_lifecycle_model_version.clone(),
        "baseline_method_version": contract.baseline_method_version.clone(),
        "quality_method_version": contract.quality_method_version.clone(),
        "coverage_model_version": contract.coverage_model_version.clone(),
        "metering_freshness_model_version": contract.metering_freshness_model_version.clone(),
        "agent_cycle_model_version": contract.agent_cycle_model_version.clone(),
        "client_limit_meter_alignment_version": contract.client_limit_meter_alignment_version.clone(),
        "client_limit_baseline_equivalence_version": contract
            .client_limit_baseline_equivalence_version
            .clone(),
        "client_limit_strict_meter_slice_version": contract
            .client_limit_strict_meter_slice_version
            .clone(),
        "client_limit_explicit_boundary_surface_version": contract
            .client_limit_explicit_boundary_surface_version
            .clone(),
        "client_limit_pre_amai_baseline_source_version": contract
            .client_limit_pre_amai_baseline_source_version
            .clone(),
        "client_limit_frozen_gap_review_surface_version": contract
            .client_limit_frozen_gap_review_surface_version
            .clone(),
        "client_limit_reviewed_frozen_debt_export_surface_version": contract
            .client_limit_reviewed_frozen_debt_export_surface_version
            .clone(),
        "excluded_taxonomy_version": contract.excluded_taxonomy_version.clone(),
        "dedup_contract_version": contract.dedup_contract_version.clone(),
        "backfill_policy_version": contract.backfill_policy_version.clone(),
        "correction_policy_version": contract.correction_policy_version.clone(),
        "freeze_close_policy_version": contract.freeze_close_policy_version.clone(),
        "late_arrival_policy_version": contract.late_arrival_policy_version.clone(),
        "dispute_policy_version": contract.dispute_policy_version.clone(),
        "settlement_lifecycle_model_version": contract.settlement_lifecycle_model_version.clone(),
        "statement_period_governance_version": contract.statement_period_governance_version.clone(),
        "adjustment_preview_model_version": contract.adjustment_preview_model_version.clone(),
        "adjustment_request_schema_version": contract.adjustment_request_schema_version.clone(),
        "adjustment_registry_version": contract.adjustment_registry_version.clone(),
        "rate_card_binding_model_version": contract.rate_card_binding_model_version.clone(),
        "infra_cost_binding_model_version": contract.infra_cost_binding_model_version.clone(),
        "telemetry_surface_split_version": contract.telemetry_surface_split_version.clone(),
        "event_time_policy_version": contract.event_time_policy_version.clone(),
        "billing_policy_version": contract.billing_policy_version.clone(),
        "suitability_model_version": contract.suitability_model_version.clone(),
        "contractual_readiness_model_version": contract.contractual_readiness_model_version.clone(),
        "customer_contractual_boundary_version": contract.customer_contractual_boundary_version.clone(),
        "settlement_activation_governance_version": contract
            .settlement_activation_governance_version
            .clone(),
        "adjustment_activation_governance_version": contract
            .adjustment_activation_governance_version
            .clone(),
        "billing_mode": contract.billing_mode.clone(),
        "reconciliation_contract_version": contract.reconciliation_contract_version.clone(),
        "margin_model_version": contract.margin_model_version.clone(),
        "infra_cost_profile_version": contract.infra_cost_profile_version.clone(),
        "contractual_evidence_pack_version": contract.contractual_evidence_pack_version.clone(),
        "contractual_statement_export_version": contract.contractual_statement_export_version.clone(),
        "settlement_report_preview_version": contract.settlement_report_preview_version.clone(),
        "rate_card_version": contract.rate_card_version.clone(),
        "currency_profile": contract.currency_profile.clone(),
        "settlement_status": contract.settlement_status.clone(),
    })
}

pub(super) fn build_usage_event_schema_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "schema_version": contract.usage_event_schema_version.clone(),
        "identity": {
            "required_fields": [
                "event_id",
                "correlation_id",
                "source_kind",
                "traffic_class",
                "project_code",
                "namespace_code",
                "measurement_scope",
                "occurred_at_epoch_ms",
                "ingested_at_epoch_ms"
            ],
            "dedup_key_format": "source_kind:event_id",
            "event_identity_note": "Исторические события сохраняют записанные contract versions; новые report semantics не переписывают прошлую truth-схему."
        },
        "lifecycle": {
            "model_version": contract.usage_lifecycle_model_version.clone(),
            "statuses": [
                "verified_included",
                "excluded_quality_gate_failed",
                "excluded_awaiting_followup_reconciliation",
                "excluded_legacy_unverified",
                "excluded_non_live"
            ],
            "reporting_layers": [
                "measured_non_billable",
                "excluded"
            ]
        },
        "dedup": {
            "policy_version": contract.dedup_contract_version.clone(),
            "idempotency_scope": "source_kind + event_id",
            "retry_behavior": "same dedup key must resolve to the same usage event identity"
        },
        "time_policy": {
            "policy_version": contract.event_time_policy_version.clone(),
            "canonical_window_field": "occurred_at_epoch_ms",
            "ingest_field": "ingested_at_epoch_ms",
            "ordering_note": "Rollup-окна считаются по occurred_at_epoch_ms; ingest time хранится отдельно и не подменяет event time."
        },
        "backfill": {
            "policy_version": contract.backfill_policy_version.clone(),
            "status": "report_only_manual_repair_or_reverify",
            "note": "Backfill пока разрешён только через явные repair/reverify paths и не должен тихо переписывать старую event truth."
        },
        "corrections": {
            "policy_version": contract.correction_policy_version.clone(),
            "status": "mutable_snapshot_report_only",
            "note": "До settlement layer corrections остаются report-only snapshot updates, а не invoice-grade credit workflow."
        },
        "whole_cycle_observed": {
            "status": "optional_progressive_measurement",
            "component_fields": [
                "client_prompt_tokens",
                "assistant_generation_tokens",
                "tool_overhead_tokens",
                "continuity_restore_tokens"
            ],
            "note": "Observed whole-cycle fields можно materialize-ить постепенно: их наличие расширяет видимость клиентского spend meter, но не даёт права объявлять same-meter savings без baseline-equivalent semantics."
        }
    })
}

pub(super) fn build_metering_freshness_contract_json(
    contract: &TokenBudgetContractConfig,
    measurement: &MeasurementConfig,
) -> Value {
    json!({
        "model_version": contract.metering_freshness_model_version.clone(),
        "ingest_warning_seconds": measurement.metering_ingest_warning_seconds,
        "ingest_slo_seconds": measurement.metering_ingest_slo_seconds,
        "late_arrival_grace_minutes": measurement.late_arrival_grace_minutes,
        "ingest_states": [
            "empty",
            "within_slo",
            "soft_lag",
            "lagging"
        ],
        "contractual_lag_states": [
            "empty",
            "awaiting_late_events",
            "lag_window_elapsed"
        ],
        "contractual_freshness_states": [
            "empty",
            "provisional_open_window",
            "stable",
            "lagging_pipeline"
        ],
        "note": "Freshness и lag semantics разделены: ingest state показывает здоровье metering pipeline, а contractual lag state — можно ли уже считать окно стабилизированным без поздних событий."
    })
}

pub(super) fn combine_reason_arrays(values: &[&Value]) -> Value {
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    for value in values {
        let Some(array) = value.as_array() else {
            continue;
        };
        for item in array {
            let Some(reason) = item.as_str() else {
                continue;
            };
            if seen.insert(reason.to_string()) {
                items.push(Value::String(reason.to_string()));
            }
        }
    }
    Value::Array(items)
}

pub(super) fn event_ingest_lag_ms(event: &TokenBudgetEvent) -> u64 {
    event
        .ingested_at_epoch_ms
        .saturating_sub(event.occurred_at_epoch_ms)
        .max(0) as u64
}

pub(super) fn build_metering_freshness_summary(
    contract: &TokenBudgetContractConfig,
    measurement: &MeasurementConfig,
    now_epoch_ms: i64,
    events: &[TokenBudgetEvent],
) -> Value {
    if events.is_empty() {
        return json!({
            "model_version": contract.metering_freshness_model_version.clone(),
            "events_count": 0,
            "metering_ingest_state": "empty",
            "contractual_lag_state": "empty",
            "contractual_freshness_state": "empty",
            "can_treat_scope_as_stable": false,
            "late_arrival_grace_ms": measurement.late_arrival_grace_minutes.saturating_mul(60_000),
            "latest_event_occurred_at_epoch_ms": Value::Null,
            "latest_event_ingested_at_epoch_ms": Value::Null,
            "latest_event_age_ms": Value::Null,
            "latest_ingest_lag_ms": Value::Null,
            "p50_ingest_lag_ms": 0.0,
            "p95_ingest_lag_ms": 0.0,
            "max_ingest_lag_ms": 0.0,
            "negative_ingest_skew_events": 0,
            "blocking_reasons": ["no_measured_usage_events"],
        });
    }

    let latest_event = events
        .iter()
        .max_by_key(|event| (event.occurred_at_epoch_ms, event.ingested_at_epoch_ms))
        .expect("events is not empty");
    let late_arrival_grace_ms = measurement
        .late_arrival_grace_minutes
        .saturating_mul(60_000);
    let latest_event_age_ms = now_epoch_ms.saturating_sub(latest_event.occurred_at_epoch_ms);
    let latest_ingest_lag_ms = event_ingest_lag_ms(latest_event);
    let negative_ingest_skew_events = events
        .iter()
        .filter(|event| event.ingested_at_epoch_ms < event.occurred_at_epoch_ms)
        .count() as u64;
    let mut lag_values = events
        .iter()
        .map(|event| event_ingest_lag_ms(event) as f64)
        .collect::<Vec<_>>();
    lag_values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let max_ingest_lag_ms = lag_values.last().copied().unwrap_or_default();
    let warning_lag_ms = measurement
        .metering_ingest_warning_seconds
        .saturating_mul(1000) as f64;
    let slo_lag_ms = measurement.metering_ingest_slo_seconds.saturating_mul(1000) as f64;
    let metering_ingest_state = if max_ingest_lag_ms == 0.0 {
        "within_slo"
    } else if max_ingest_lag_ms <= warning_lag_ms {
        "within_slo"
    } else if max_ingest_lag_ms <= slo_lag_ms {
        "soft_lag"
    } else {
        "lagging"
    };
    let contractual_lag_state = if latest_event_age_ms < late_arrival_grace_ms as i64 {
        "awaiting_late_events"
    } else {
        "lag_window_elapsed"
    };
    let contractual_freshness_state = if metering_ingest_state == "lagging" {
        "lagging_pipeline"
    } else if contractual_lag_state == "awaiting_late_events" {
        "provisional_open_window"
    } else {
        "stable"
    };
    let mut blocking_reasons = Vec::new();
    if metering_ingest_state == "lagging" {
        blocking_reasons.push("metering_pipeline_lagging");
    }
    if contractual_lag_state == "awaiting_late_events" {
        blocking_reasons.push("late_arrival_window_open");
    }
    if negative_ingest_skew_events > 0 {
        blocking_reasons.push("negative_ingest_clock_skew_detected");
    }

    json!({
        "model_version": contract.metering_freshness_model_version.clone(),
        "events_count": events.len(),
        "metering_ingest_state": metering_ingest_state,
        "contractual_lag_state": contractual_lag_state,
        "contractual_freshness_state": contractual_freshness_state,
        "can_treat_scope_as_stable": contractual_freshness_state == "stable",
        "late_arrival_grace_ms": late_arrival_grace_ms,
        "latest_event_occurred_at_epoch_ms": latest_event.occurred_at_epoch_ms,
        "latest_event_ingested_at_epoch_ms": latest_event.ingested_at_epoch_ms,
        "latest_event_age_ms": latest_event_age_ms,
        "latest_ingest_lag_ms": latest_ingest_lag_ms,
        "p50_ingest_lag_ms": percentile_from_sorted(&lag_values, 0.50),
        "p95_ingest_lag_ms": percentile_from_sorted(&lag_values, 0.95),
        "max_ingest_lag_ms": max_ingest_lag_ms,
        "negative_ingest_skew_events": negative_ingest_skew_events,
        "blocking_reasons": blocking_reasons,
    })
}

pub(super) fn allowed_baseline_classes() -> [&'static str; 5] {
    [
        "naive_top_files",
        "grep_top_files",
        "ide_search_top_files",
        "semantic_top_k",
        "legacy_pre_amai",
    ]
}

pub(super) fn disallowed_baseline_classes() -> [&'static str; 2] {
    ["entire_repo", "all_docs"]
}

pub(super) fn build_baseline_contract_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "baseline_method_version": contract.baseline_method_version.clone(),
        "allowed_classes": allowed_baseline_classes(),
        "disallowed_classes": disallowed_baseline_classes(),
        "fairness_note": "Savings разрешено считать только против реалистичного baseline scope; раздутый entire_repo/all_docs baseline запрещён."
    })
}

pub(super) fn build_billing_policy_json(
    contract: &TokenBudgetContractConfig,
    measurement: &MeasurementConfig,
) -> Value {
    json!({
        "policy_version": contract.billing_policy_version.clone(),
        "mode": contract.billing_mode.clone(),
        "status": "report_only",
        "settlement_status": contract.settlement_status.clone(),
        "current_billable_state": "disabled_report_only",
        "savings_floor_term": "savings floor",
        "confirmed_lower_bound_term": "confirmed lower bound",
        "retrieval_savings_floor_term": "retrieval savings floor",
        "whole_cycle_term": "partial whole-agent-cycle lower bound",
        "quality_gate_required": true,
        "required_traffic_class": "live",
        "preliminary_thresholds": {
            "min_events": measurement.preliminary_min_events,
            "min_baseline_tokens": measurement.preliminary_min_baseline_tokens
        },
        "included_reporting_layers": [
            "measured_non_billable",
            "unmeasured"
        ],
        "excluded_from_future_billing": [
            "synthetic traffic",
            "unverified live events",
            "quality_gate_failed",
            "awaiting_followup_reconciliation"
        ],
        "truth_guardrail": {
            "retrieval_savings_floor": "real",
            "partial_whole_agent_cycle_lower_bound": "real",
            "full_session_economics": "not_fully_measured"
        },
        "note": "Billing semantics пока не активны: lower bound уже измеряется, но current policy остаётся report-only и не превращает savings в денежное начисление. confirmed lower bound пригоден для truthful KPI только вместе с coverage и completeness state."
    })
}

pub(super) fn build_suitability_contract_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "model_version": contract.suitability_model_version.clone(),
        "surfaces": [
            {
                "code": "operational_live",
                "meaning": "Инженерный live-contour для наблюдения за текущим потоком. Может показывать и положительный, и отрицательный результат без денежного смысла."
            },
            {
                "code": "product_kpi",
                "meaning": "Truthful product KPI по confirmed lower bound. Требует confirmed usage и обязан показываться вместе с coverage и completeness state."
            },
            {
                "code": "customer_review",
                "meaning": "Customer-facing review/report-only слой. Может быть пригоден даже в provisional состоянии, если это прямо показано."
            },
            {
                "code": "contractual_export",
                "meaning": "Export/evidence surface для review и audit. Это не invoice и не settlement."
            },
            {
                "code": "billing_amount",
                "meaning": "Будущий money-facing слой. До честного billable close и reconciliation обязан оставаться непригодным."
            },
            {
                "code": "compensation_pricing",
                "meaning": "Самый строгий слой для success-fee или pay-from-savings. Требует billable lower bound, money truth и final settlement semantics."
            }
        ],
        "required_companions": [
            "coverage",
            "completeness_state",
            "truth_guardrail"
        ],
        "truth_guardrail": {
            "retrieval_savings_floor": "real",
            "partial_whole_agent_cycle_lower_bound": "real",
            "full_session_economics": "not_fully_measured"
        },
        "note": "Suitability отвечает не на вопрос, хорошая ли цифра, а на вопрос, где её можно использовать без обмана. Отрицательная экономия тоже может быть truthful KPI, если scope и coverage показаны честно."
    })
}

pub(super) fn file_last_modified_epoch_ms(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let duration = modified.duration_since(UNIX_EPOCH).ok()?;
    Some(duration.as_millis() as i64)
}

pub(super) fn attach_source_file_evidence(base: &mut Value, path: &Path, raw: &str) {
    base["source_bytes"] = json!(raw.len() as u64);
    base["source_sha256"] = Value::String(hex_sha256(raw.as_bytes()));
    base["source_last_modified_epoch_ms"] = match file_last_modified_epoch_ms(path) {
        Some(value) => json!(value),
        None => Value::Null,
    };
}

pub(super) fn build_settlement_contract_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "statement_version": contract.settlement_statement_version.clone(),
        "freeze_close_policy_version": contract.freeze_close_policy_version.clone(),
        "late_arrival_policy_version": contract.late_arrival_policy_version.clone(),
        "correction_policy_version": contract.correction_policy_version.clone(),
        "dispute_policy_version": contract.dispute_policy_version.clone(),
        "settlement_lifecycle_model_version": contract.settlement_lifecycle_model_version.clone(),
        "statement_period_governance_version": contract.statement_period_governance_version.clone(),
        "adjustment_preview_model_version": contract.adjustment_preview_model_version.clone(),
        "settlement_report_preview_version": contract.settlement_report_preview_version.clone(),
        "telemetry_surface_split_version": contract.telemetry_surface_split_version.clone(),
        "settlement_status": contract.settlement_status.clone(),
        "current_materialized_boundary": "measured_report_only",
        "statement_lifecycle": [
            {
                "code": "live_measurement_open",
                "surface": "operational",
                "meaning": "Live token rollup ещё открыт и меняется по мере новых событий."
            },
            {
                "code": "report_only_preview_open",
                "surface": "contractual",
                "meaning": "Есть contractual preview, но billing и закрытие периода ещё не включены."
            },
            {
                "code": "report_only_preview_provisionally_stable",
                "surface": "contractual",
                "meaning": "Scope уже перестал плыть по late-arrival и ingest lag, но остаётся только report-only preview, а не закрытым statement."
            },
            {
                "code": "report_only_preview_provisional_hold",
                "surface": "contractual",
                "meaning": "Scope ещё нельзя даже provisionally считать устойчивым: есть lag, late-arrival окно, coverage gap или adjustment/dispute hold."
            },
            {
                "code": "close_blocked_report_only",
                "surface": "contractual",
                "meaning": "Период нельзя честно закрыть: settlement остаётся report-only."
            },
            {
                "code": "closed_with_adjustments_reserved",
                "surface": "future_reserved",
                "meaning": "Будущий invoice-grade слой должен использовать отдельные adjustment/credit semantics, а не тихую перезапись."
            }
        ],
        "materialized_settlement_stages": [
            {
                "code": "empty_report_only",
                "family": "empty",
                "surface": "contractual",
                "meaning": "Пока нет измеренных usage-событий даже для report-only statement preview."
            },
            {
                "code": "measured_open_report_only",
                "family": "measured_report_only",
                "surface": "contractual",
                "meaning": "Измеренный report-only scope уже есть, но он ещё не дотянулся даже до review-ready состояния."
            },
            {
                "code": "measured_review_ready_report_only",
                "family": "measured_report_only",
                "surface": "contractual",
                "meaning": "Measured lower bound уже provisionally stable и пригоден для review/export, но всё ещё не является billable amount."
            },
            {
                "code": "measured_adjusted_report_only",
                "family": "measured_report_only",
                "surface": "contractual",
                "meaning": "Measured lower bound уже содержит applied report-only adjustment entries."
            },
            {
                "code": "measured_pending_adjustment_report_only",
                "family": "measured_report_only",
                "surface": "contractual",
                "meaning": "Есть measured scope, но adjustment review ещё не завершён."
            },
            {
                "code": "measured_disputed_report_only",
                "family": "measured_report_only",
                "surface": "contractual",
                "meaning": "Measured scope существует, но по нему открыт dispute hold."
            }
        ],
        "future_reserved_settlement_stages": future_reserved_settlement_stages(),
        "transition_contract": {
            "current_materialized_boundary": "measured_report_only",
            "future_reserved_boundary": "billable_and_beyond_reserved",
            "note": "Текущий runtime materialize-ит только measured/report-only lifecycle. Billable, settled, invoiced, credited, disputed и closed остаются зарезервированными стадиями, а не активной денежной логикой."
        },
        "current_operational_state": "live_measurement_open",
        "current_contractual_state": "report_only_preview_open",
        "freeze_close_status": "provisional_report_only",
        "late_arrival_status": "deadline_from_latest_event_report_only",
        "note": "Settlement layer остаётся report-only preview: scope уже можно честно маркировать как provisionally stable или provisional hold, но это всё ещё не денежный close workflow и не invoice."
    })
}

pub(super) fn build_statement_period_json(
    scope_code: &str,
    scope_label: &str,
    now_epoch_ms: i64,
    events: &[TokenBudgetEvent],
    profile: &ResolvedProfile,
    contract: &TokenBudgetContractConfig,
    metering_freshness: &Value,
    provisional_close_candidate: bool,
    provisional_close_barriers: &[String],
) -> Value {
    let start_epoch_ms = match scope_code {
        "current_session" | "lifetime" => {
            events.iter().map(|event| event.occurred_at_epoch_ms).min()
        }
        "rolling_window" => profile
            .rolling_window_hours
            .map(|hours| now_epoch_ms - (hours as i64 * 60 * 60 * 1000)),
        _ => None,
    };
    let window_anchor = match scope_code {
        "current_session" => json!({
            "kind": "session_gap_minutes",
            "value": profile.session_gap_minutes,
        }),
        "rolling_window" => json!({
            "kind": "rolling_window_hours",
            "value": profile.rolling_window_hours,
        }),
        "lifetime" => json!({
            "kind": "first_recorded_event",
            "value": start_epoch_ms,
        }),
        _ => Value::Null,
    };
    let latest_event_epoch_ms = metering_freshness["latest_event_occurred_at_epoch_ms"].as_i64();
    let late_arrival_grace_ms = metering_freshness["late_arrival_grace_ms"].as_i64();
    let provisional_close_earliest_at_epoch_ms = latest_event_epoch_ms
        .zip(late_arrival_grace_ms)
        .map(|(latest, grace)| latest + grace);
    let window_state = if events.is_empty() {
        "empty_report_only"
    } else if metering_freshness["metering_ingest_state"].as_str() == Some("lagging") {
        "pipeline_lag_open_report_only"
    } else if metering_freshness["contractual_lag_state"].as_str() == Some("awaiting_late_events") {
        "open_late_arrival_window_report_only"
    } else if provisional_close_candidate {
        "provisionally_stable_report_only"
    } else {
        "open_review_hold_report_only"
    };
    let close_policy_state = if events.is_empty() {
        "provisional_close_not_applicable_empty"
    } else if provisional_close_candidate {
        "provisional_close_candidate_report_only"
    } else {
        "provisional_close_blocked_report_only"
    };
    let late_arrival_policy_state = if events.is_empty() {
        "no_events_report_only"
    } else if metering_freshness["contractual_lag_state"].as_str() == Some("awaiting_late_events") {
        "accepting_events_within_provisional_deadline"
    } else {
        "provisional_deadline_elapsed"
    };

    json!({
        "model_version": contract.statement_period_governance_version.clone(),
        "scope_code": scope_code,
        "scope_label": scope_label,
        "event_time_basis": "occurred_at_epoch_ms",
        "period_start_epoch_ms": start_epoch_ms,
        "period_end_epoch_ms": now_epoch_ms,
        "close_at_epoch_ms": Value::Null,
        "late_arrival_deadline_epoch_ms": provisional_close_earliest_at_epoch_ms,
        "provisional_close_earliest_at_epoch_ms": provisional_close_earliest_at_epoch_ms,
        "provisional_close_candidate": provisional_close_candidate,
        "provisional_close_barriers": provisional_close_barriers,
        "window_anchor": window_anchor,
        "window_state": window_state,
        "close_policy_state": close_policy_state,
        "late_arrival_policy_state": late_arrival_policy_state,
        "note": "Период по-прежнему report-only: close_at остаётся пустым до реального settlement workflow, но provisional deadline и provisional stability уже считаются по latest event и late-arrival policy."
    })
}

pub(super) fn build_adjustment_preview_json(
    scope_code: &str,
    contract: &TokenBudgetContractConfig,
    adjustment_registry: &Value,
) -> Value {
    let scope_summary = &adjustment_registry["scopes"][scope_code];
    let pending_entries = scope_summary["pending_entries_count"].as_u64().unwrap_or(0);
    let applied_entries = scope_summary["applied_entries_count"].as_u64().unwrap_or(0);
    let disputed_entries = scope_summary["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0);
    json!({
        "model_version": contract.adjustment_preview_model_version.clone(),
        "request_schema_version": contract.adjustment_request_schema_version.clone(),
        "registry_version": contract.adjustment_registry_version.clone(),
        "registry_status": adjustment_registry["status"].clone(),
        "status": match adjustment_registry["status"].as_str() {
            Some("loaded") => "loaded_report_only",
            Some(other) => other,
            None => "unknown",
        },
        "current_entries_count": scope_summary["entries_count"].clone(),
        "pending_entries_count": scope_summary["pending_entries_count"].clone(),
        "applied_entries_count": scope_summary["applied_entries_count"].clone(),
        "disputed_entries_count": scope_summary["disputed_entries_count"].clone(),
        "scope_hash": scope_summary["scope_hash"].clone(),
        "pending_tokens_delta": scope_summary["pending_tokens_delta"].clone(),
        "pending_amount_delta": scope_summary["pending_amount_delta"].clone(),
        "applied_tokens_delta": scope_summary["applied_tokens_delta"].clone(),
        "applied_amount_delta": scope_summary["applied_amount_delta"].clone(),
        "disputed_tokens_delta": scope_summary["disputed_tokens_delta"].clone(),
        "disputed_amount_delta": scope_summary["disputed_amount_delta"].clone(),
        "net_tokens_delta": scope_summary["applied_tokens_delta"].clone(),
        "net_amount_delta": scope_summary["applied_amount_delta"].clone(),
        "correction_action_state": if disputed_entries > 0 {
            "dispute_hold_open"
        } else if pending_entries > 0 {
            "pending_review"
        } else if applied_entries > 0 {
            "applied_report_only"
        } else {
            "no_adjustments"
        },
        "allowed_future_actions": [
            "credit_note",
            "adjustment_entry",
            "dispute_hold"
        ],
        "note": "Корректировки и credit semantics materialize-ятся отдельным registry слоем: report-only preview не переписывает прошлые statement задним числом."
    })
}

pub(super) fn binding_currency_profile(binding: &Value) -> Value {
    if !binding["bound_currency_profile"].is_null() {
        binding["bound_currency_profile"].clone()
    } else {
        binding["currency_profile"].clone()
    }
}

pub(super) fn binding_bound_version(binding: &Value) -> Value {
    for key in [
        "bound_rate_card_version",
        "bound_profile_version",
        "schema_version",
    ] {
        if !binding[key].is_null() {
            return binding[key].clone();
        }
    }
    Value::Null
}

pub(super) fn build_external_truth_manifest_entry(binding: &Value) -> Value {
    json!({
        "status": binding["status"].clone(),
        "binding_status": binding["source"]["binding_status"].clone(),
        "resolved_path": binding["source"]["resolved_path"].clone(),
        "source_bytes": binding["source_bytes"].clone(),
        "source_sha256": binding["source_sha256"].clone(),
        "source_last_modified_epoch_ms": binding["source_last_modified_epoch_ms"].clone(),
        "schema_version": binding["schema_version"].clone(),
        "bound_version": binding_bound_version(binding),
        "provider": binding["provider"].clone(),
        "currency_profile": binding_currency_profile(binding),
    })
}

pub(super) fn build_external_truth_manifest(
    contract: &TokenBudgetContractConfig,
    rate_card: &Value,
    infra_cost_profile: &Value,
    provider_usage_binding: &Value,
    provider_invoice_binding: &Value,
    adjustment_registry: &Value,
) -> Value {
    let entries = json!({
        "provider_usage_export": build_external_truth_manifest_entry(provider_usage_binding),
        "provider_invoice_export": build_external_truth_manifest_entry(provider_invoice_binding),
        "provider_rate_card": build_external_truth_manifest_entry(rate_card),
        "infra_cost_profile": build_external_truth_manifest_entry(infra_cost_profile),
        "token_adjustment_registry": build_external_truth_manifest_entry(adjustment_registry),
    });
    let manifest_hash = serde_json::to_vec(&entries)
        .map(|bytes| hex_sha256(&bytes))
        .unwrap_or_else(|_| "hash_error".to_string());
    json!({
        "reconciliation_contract_version": contract.reconciliation_contract_version.clone(),
        "statement_export_version": contract.contractual_statement_export_version.clone(),
        "evidence_pack_version": contract.contractual_evidence_pack_version.clone(),
        "entries": entries,
        "manifest_hash": manifest_hash,
        "note": "External truth manifest фиксирует fingerprint привязанных usage/invoice/rate-card/infra/adjustment sources. Это audit trail для contractual review, а не invoice-grade settlement."
    })
}

pub(super) fn build_telemetry_surfaces_json(contract: &TokenBudgetContractConfig) -> Value {
    json!({
        "model_version": contract.telemetry_surface_split_version.clone(),
        "operational_surface": {
            "code": "engineering_live_telemetry",
            "intended_consumers": [
                "dashboard",
                "observability",
                "engineers"
            ],
            "fields": [
                "headline",
                "current_session",
                "rolling_window",
                "lifetime",
                "source_breakdown",
                "query_slices",
                "baseline_strategy_slices",
                "temperature_slices"
            ],
            "not_for": [
                "invoice",
                "settlement",
                "customer_billing"
            ]
        },
        "contractual_surface": {
            "code": "report_only_tokenonomics_contract",
            "intended_consumers": [
                "customer_review",
                "audit",
                "finance_preparation"
            ],
            "fields": [
                "usage_event_schema",
                "metering_freshness_contract",
                "baseline_contract",
                "billing_policy",
                "rate_card",
                "settlement_contract",
                "metering_freshness",
                "statement_previews",
                "reconciliation_contract",
                "reconciliation_previews",
                "infra_cost_profile",
                "margin_contract",
                "margin_view",
                "adjustment_request_schema",
                "adjustment_registry",
                "statement_export_previews",
                "contractual_evidence_pack"
            ],
            "state": "report_only_preview",
            "not_for": [
                "live_latency_tuning",
                "hot_path_benchmarking"
            ]
        },
        "note": "Operational telemetry и contractual tokenonomics intentionally split: dashboard live rollups нельзя трактовать как invoice или закрытый statement."
    })
}

pub(super) fn source_codes_with_truth_role(external_sources: &Value, role_key: &str) -> Value {
    let mut codes = external_sources
        .as_object()
        .into_iter()
        .flat_map(|entries| entries.values())
        .filter(|source| source["truth_roles"][role_key].as_bool() == Some(true))
        .filter_map(|source| source["code"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    codes.sort();
    Value::Array(codes.into_iter().map(Value::String).collect())
}

pub(super) fn missing_source_codes_json(missing_codes: Vec<&'static str>) -> Value {
    let mut codes = missing_codes
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
    codes.sort();
    Value::Array(codes.into_iter().map(Value::String).collect())
}

pub(super) fn provider_usage_truth_bound(status: &str) -> bool {
    matches!(status, "usage_bound" | "usage_and_cost_bound")
}

pub(super) fn provider_usage_cost_truth_bound(status: &str) -> bool {
    status == "usage_and_cost_bound"
}

pub(super) fn rate_card_priced_bound(status: &str) -> bool {
    status == "priced_bound"
}

pub(super) fn infra_cost_profile_priced_bound(status: &str) -> bool {
    status == "priced_bound"
}

pub(super) fn provider_invoice_bound(status: &str) -> bool {
    status == "invoice_bound"
}

pub(super) fn internal_provider_cost_estimate_amount(
    internal_provider_billed_tokens: u64,
    rate_card: &Value,
) -> Option<f64> {
    let input_rate = rate_card["default_input_cost_per_1k_tokens"].as_f64()?;
    Some((internal_provider_billed_tokens as f64 / 1000.0) * input_rate)
}

pub(super) fn amount_delta(lhs: Option<f64>, rhs: Option<f64>) -> Value {
    match (lhs, rhs) {
        (Some(lhs), Some(rhs)) => json!(lhs - rhs),
        _ => Value::Null,
    }
}

pub(super) fn source_temporal_scope_state(
    start_epoch_ms: Option<i64>,
    end_epoch_ms: Option<i64>,
) -> &'static str {
    match (start_epoch_ms, end_epoch_ms) {
        (None, None) => "source_period_unspecified",
        (Some(_), Some(_)) => "source_period_bounded",
        _ => "source_period_partially_bound",
    }
}

pub(super) fn statement_period_bounds(statement_preview: &Value) -> (Option<i64>, Option<i64>) {
    (
        statement_preview["period"]["period_start_epoch_ms"].as_i64(),
        statement_preview["period"]["period_end_epoch_ms"].as_i64(),
    )
}

pub(super) fn scope_period_alignment_state(
    scope_start_epoch_ms: Option<i64>,
    scope_end_epoch_ms: Option<i64>,
    source_start_epoch_ms: Option<i64>,
    source_end_epoch_ms: Option<i64>,
) -> &'static str {
    match (scope_start_epoch_ms, scope_end_epoch_ms) {
        (Some(scope_start), Some(scope_end)) => {
            match (source_start_epoch_ms, source_end_epoch_ms) {
                (Some(source_start), Some(source_end))
                    if source_start <= scope_start && source_end >= scope_end =>
                {
                    "scope_period_aligned"
                }
                (Some(_), Some(_)) => "scope_period_mismatch",
                (None, None) => "source_period_unspecified",
                _ => "source_period_partially_bound",
            }
        }
        _ => "scope_period_unknown",
    }
}

pub(super) fn combined_temporal_truth_state(states: &[&str]) -> &'static str {
    if states.contains(&"scope_period_mismatch") {
        "scope_period_mismatch"
    } else if states.contains(&"source_period_partially_bound") {
        "source_period_partially_bound"
    } else if states.contains(&"source_period_unspecified") {
        "source_period_unspecified"
    } else if states.contains(&"scope_period_unknown") {
        "scope_period_unknown"
    } else if states.iter().all(|state| *state == "scope_period_aligned") {
        "scope_period_aligned"
    } else {
        "scope_period_unchecked"
    }
}

pub(super) fn bound_provider_name<'a>(
    binding: &'a Value,
    expected_statuses: &[&str],
) -> Option<&'a str> {
    let status = binding["status"].as_str()?;
    if !expected_statuses.contains(&status) {
        return None;
    }
    binding["provider"].as_str()
}

pub(super) fn provider_alignment_state(
    lhs_provider: Option<&str>,
    rhs_provider: Option<&str>,
) -> &'static str {
    match (lhs_provider, rhs_provider) {
        (Some(lhs), Some(rhs)) if lhs == rhs => "provider_identity_aligned",
        (Some(_), Some(_)) => "provider_identity_mismatch",
        _ => "provider_identity_unchecked",
    }
}

pub(super) fn combined_provider_identity_state(states: &[&str]) -> &'static str {
    if states.contains(&"provider_identity_mismatch") {
        "provider_identity_mismatch"
    } else if states
        .iter()
        .all(|state| *state == "provider_identity_aligned")
    {
        "provider_identity_aligned"
    } else {
        "provider_identity_unchecked"
    }
}

pub(super) fn base_reconciliation_blocking_reasons(
    statement_preview: &Value,
    rate_card: &Value,
    include_provider_usage_missing: bool,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if include_provider_usage_missing {
        reasons.push("provider_usage_source_missing");
    }
    if rate_card["money_conversion_enabled"].as_bool() != Some(true) {
        reasons.push("provider_rate_card_unpriced");
    }
    reasons.push("billing_policy_report_only");
    if statement_preview["billable_lower_bound_tokens"].is_null() {
        reasons.push("billable_lower_bound_not_materialized");
    }
    reasons
}

pub(super) fn usage_truth_completeness_state(provider_usage_status: &str) -> &'static str {
    if matches!(
        provider_usage_status,
        "not_configured" | "default_path_missing"
    ) {
        "awaiting_provider_usage_source"
    } else if matches!(
        provider_usage_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "provider_usage_source_error"
    } else if matches!(
        provider_usage_status,
        "usage_bound" | "usage_and_cost_bound"
    ) {
        "provider_usage_bound"
    } else {
        "provider_usage_not_yet_bound"
    }
}

pub(super) fn provider_cost_truth_completeness_state(
    provider_usage_status: &str,
    rate_card_status: &str,
) -> &'static str {
    if !matches!(
        provider_usage_status,
        "usage_bound" | "usage_and_cost_bound"
    ) {
        "no_external_cost_truth"
    } else if matches!(rate_card_status, "not_configured" | "default_path_missing") {
        "awaiting_rate_card_source"
    } else if matches!(
        rate_card_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "provider_rate_card_error"
    } else if provider_usage_status == "usage_and_cost_bound" {
        "provider_cost_bound"
    } else if rate_card_status == "bound_but_unpriced" {
        "rate_card_bound_unpriced"
    } else {
        "rate_card_bound_internal_estimate_only"
    }
}

pub(super) fn invoice_evidence_completeness_state(
    provider_usage_status: &str,
    provider_invoice_status: &str,
) -> &'static str {
    if !matches!(
        provider_usage_status,
        "usage_bound" | "usage_and_cost_bound"
    ) {
        "no_invoice_evidence_scope"
    } else if matches!(
        provider_invoice_status,
        "not_configured" | "default_path_missing"
    ) {
        "awaiting_provider_invoice_source"
    } else if matches!(
        provider_invoice_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "provider_invoice_source_error"
    } else if provider_invoice_status == "invoice_bound" {
        "provider_invoice_bound"
    } else {
        "provider_invoice_not_yet_bound"
    }
}

pub(super) fn money_truth_completeness_state(
    provider_cost_truth_state: &str,
    invoice_evidence_truth_state: &str,
) -> &'static str {
    match provider_cost_truth_state {
        "no_external_cost_truth" => "no_external_money_truth",
        "awaiting_rate_card_source" => "awaiting_rate_card_source",
        "provider_rate_card_error" => "provider_rate_card_error",
        "rate_card_bound_unpriced" => "rate_card_bound_unpriced",
        "provider_cost_bound" => match invoice_evidence_truth_state {
            "provider_invoice_source_error" => "provider_invoice_source_error",
            "provider_invoice_bound" => "provider_cost_and_invoice_bound",
            _ => "provider_cost_bound_without_invoice",
        },
        "rate_card_bound_internal_estimate_only" => match invoice_evidence_truth_state {
            "provider_invoice_source_error" => "provider_invoice_source_error",
            _ => "rate_card_bound_internal_estimate_only",
        },
        _ => "provider_cost_truth_not_yet_bound",
    }
}

pub(super) fn rate_card_truth_completeness_state(rate_card_status: &str) -> &'static str {
    if matches!(rate_card_status, "not_configured" | "default_path_missing") {
        "awaiting_rate_card_source"
    } else if matches!(
        rate_card_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "provider_rate_card_error"
    } else if rate_card_status == "priced_bound" {
        "rate_card_priced_bound"
    } else if rate_card_status == "bound_but_unpriced" {
        "rate_card_bound_unpriced"
    } else {
        "rate_card_not_yet_bound"
    }
}

pub(super) fn infra_cost_truth_completeness_state(infra_cost_status: &str) -> &'static str {
    if matches!(infra_cost_status, "not_configured" | "default_path_missing") {
        "awaiting_infra_cost_profile"
    } else if matches!(
        infra_cost_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "infra_cost_profile_error"
    } else if infra_cost_status == "priced_bound" {
        "infra_cost_profile_priced_bound"
    } else if infra_cost_status == "bound_but_unpriced" {
        "infra_cost_profile_bound_unpriced"
    } else {
        "infra_cost_profile_not_yet_bound"
    }
}

pub(super) fn pricing_truth_completeness_state(
    rate_card_truth_state: &str,
    infra_cost_truth_state: &str,
) -> &'static str {
    if matches!(rate_card_truth_state, "provider_rate_card_error")
        || matches!(infra_cost_truth_state, "infra_cost_profile_error")
    {
        "pricing_truth_source_error"
    } else if rate_card_truth_state == "rate_card_priced_bound"
        && infra_cost_truth_state == "infra_cost_profile_priced_bound"
    {
        "pricing_truth_ready"
    } else if matches!(
        rate_card_truth_state,
        "awaiting_rate_card_source" | "rate_card_not_yet_bound"
    ) && matches!(
        infra_cost_truth_state,
        "awaiting_infra_cost_profile" | "infra_cost_profile_not_yet_bound"
    ) {
        "awaiting_rate_card_and_infra_cost_profile"
    } else if matches!(
        rate_card_truth_state,
        "awaiting_rate_card_source" | "rate_card_not_yet_bound"
    ) {
        "awaiting_rate_card_source"
    } else if matches!(
        infra_cost_truth_state,
        "awaiting_infra_cost_profile" | "infra_cost_profile_not_yet_bound"
    ) {
        "awaiting_infra_cost_profile"
    } else if matches!(rate_card_truth_state, "rate_card_bound_unpriced")
        || matches!(infra_cost_truth_state, "infra_cost_profile_bound_unpriced")
    {
        "pricing_truth_bound_unpriced"
    } else {
        "pricing_truth_partially_bound"
    }
}

pub(super) fn customer_savings_money_truth_completeness_state(
    rate_card_truth_state: &str,
) -> &'static str {
    match rate_card_truth_state {
        "provider_rate_card_error" => "customer_savings_money_truth_source_error",
        "awaiting_rate_card_source" | "rate_card_not_yet_bound" => "awaiting_rate_card_source",
        "rate_card_bound_unpriced" => "rate_card_bound_unpriced",
        "rate_card_priced_bound" => "customer_savings_lower_bound_ready_report_only",
        _ => "customer_savings_money_truth_not_yet_bound",
    }
}

pub(super) fn amai_cost_truth_completeness_state(infra_cost_truth_state: &str) -> &'static str {
    match infra_cost_truth_state {
        "infra_cost_profile_error" => "amai_cost_truth_source_error",
        "awaiting_infra_cost_profile" | "infra_cost_profile_not_yet_bound" => {
            "awaiting_infra_cost_profile"
        }
        "infra_cost_profile_bound_unpriced" => "infra_cost_profile_bound_unpriced",
        "infra_cost_profile_priced_bound" => "amai_cost_preview_ready_report_only",
        _ => "amai_cost_truth_not_yet_bound",
    }
}

pub(super) fn margin_truth_completeness_state(
    customer_savings_truth_state: &str,
    amai_cost_truth_state: &str,
) -> &'static str {
    if matches!(
        customer_savings_truth_state,
        "customer_savings_money_truth_source_error"
    ) || matches!(amai_cost_truth_state, "amai_cost_truth_source_error")
    {
        "margin_truth_source_error"
    } else if customer_savings_truth_state == "customer_savings_lower_bound_ready_report_only"
        && amai_cost_truth_state == "amai_cost_preview_ready_report_only"
    {
        "margin_preview_amounts_ready_report_only"
    } else if matches!(
        customer_savings_truth_state,
        "awaiting_rate_card_source" | "customer_savings_money_truth_not_yet_bound"
    ) && matches!(
        amai_cost_truth_state,
        "awaiting_infra_cost_profile" | "amai_cost_truth_not_yet_bound"
    ) {
        "awaiting_rate_card_and_infra_cost_profile"
    } else if matches!(
        customer_savings_truth_state,
        "awaiting_rate_card_source" | "customer_savings_money_truth_not_yet_bound"
    ) {
        "awaiting_rate_card_source"
    } else if matches!(
        amai_cost_truth_state,
        "awaiting_infra_cost_profile" | "amai_cost_truth_not_yet_bound"
    ) {
        "awaiting_infra_cost_profile"
    } else if customer_savings_truth_state == "rate_card_bound_unpriced"
        || amai_cost_truth_state == "infra_cost_profile_bound_unpriced"
    {
        "margin_truth_bound_unpriced"
    } else {
        "margin_truth_partially_bound"
    }
}

pub(super) fn reconciliation_readiness_state(
    usage_truth_completeness_state: &str,
    provider_cost_truth_completeness_state: &str,
    invoice_evidence_completeness_state: &str,
) -> &'static str {
    match usage_truth_completeness_state {
        "awaiting_provider_usage_source" => "awaiting_provider_usage_source",
        "provider_usage_source_error" => "provider_usage_source_error",
        "provider_usage_not_yet_bound" => "provider_usage_not_yet_bound",
        _ => match provider_cost_truth_completeness_state {
            "awaiting_rate_card_source" => "usage_truth_bound_not_priced",
            "provider_rate_card_error" => "usage_truth_bound_rate_card_error",
            "provider_cost_bound" => match invoice_evidence_completeness_state {
                "provider_invoice_source_error" => "usage_cost_truth_ready_invoice_source_error",
                "provider_invoice_bound" => "usage_cost_and_invoice_truth_ready",
                _ => "usage_and_cost_truth_ready",
            },
            "rate_card_bound_unpriced" => "usage_truth_bound_unpriced",
            "rate_card_bound_internal_estimate_only" => "usage_truth_bound_internal_estimate_only",
            _ => "usage_truth_bound_not_priced",
        },
    }
}

pub(super) fn reconciliation_governance_blocking_reasons(
    provider_usage_status: &str,
    rate_card_status: &str,
    provider_invoice_status: &str,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if matches!(
        provider_usage_status,
        "not_configured" | "default_path_missing"
    ) {
        reasons.push("provider_usage_source_missing");
    } else if matches!(
        provider_usage_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        reasons.push("provider_usage_source_error");
    }
    if matches!(rate_card_status, "not_configured" | "default_path_missing") {
        reasons.push("provider_rate_card_unpriced");
    } else if matches!(
        rate_card_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        reasons.push("provider_rate_card_error");
    }
    if matches!(
        provider_invoice_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        reasons.push("provider_invoice_source_error");
    }
    reasons
}

pub(super) fn provider_usage_scope_alignment_state(
    statement_preview: &Value,
    provider_usage_binding: &Value,
    scope_code: &str,
) -> &'static str {
    let provider_usage_status = provider_usage_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    if !matches!(
        provider_usage_status,
        "usage_bound" | "usage_and_cost_bound"
    ) {
        return "provider_usage_not_bound";
    }
    let (scope_start, scope_end) = statement_period_bounds(statement_preview);
    let scope_entry = &provider_usage_binding["scopes"][scope_code];
    scope_period_alignment_state(
        scope_start,
        scope_end,
        scope_entry["period_start_epoch_ms"].as_i64(),
        scope_entry["period_end_epoch_ms"].as_i64(),
    )
}

pub(super) fn provider_invoice_scope_alignment_state(
    statement_preview: &Value,
    provider_invoice_binding: &Value,
    scope_code: &str,
) -> &'static str {
    let provider_invoice_status = provider_invoice_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    if provider_invoice_status != "invoice_bound" {
        return "invoice_not_bound";
    }
    let (scope_start, scope_end) = statement_period_bounds(statement_preview);
    let scope_entry = &provider_invoice_binding["scopes"][scope_code];
    scope_period_alignment_state(
        scope_start,
        scope_end,
        scope_entry["period_start_epoch_ms"].as_i64(),
        scope_entry["period_end_epoch_ms"].as_i64(),
    )
}

pub(super) fn rate_card_scope_alignment_state(
    statement_preview: &Value,
    rate_card: &Value,
) -> &'static str {
    let rate_card_status = rate_card["status"].as_str().unwrap_or("not_configured");
    if !matches!(rate_card_status, "priced_bound" | "bound_but_unpriced") {
        return "rate_card_not_bound";
    }
    let (scope_start, scope_end) = statement_period_bounds(statement_preview);
    scope_period_alignment_state(
        scope_start,
        scope_end,
        rate_card["effective_from_epoch_ms"].as_i64(),
        rate_card["effective_to_epoch_ms"].as_i64(),
    )
}

pub(super) fn infra_cost_scope_alignment_state(
    statement_preview: &Value,
    infra_cost_profile: &Value,
) -> &'static str {
    let infra_cost_status = infra_cost_profile["status"]
        .as_str()
        .unwrap_or("not_configured");
    if !matches!(infra_cost_status, "priced_bound" | "bound_but_unpriced") {
        return "infra_cost_profile_not_bound";
    }
    let (scope_start, scope_end) = statement_period_bounds(statement_preview);
    scope_period_alignment_state(
        scope_start,
        scope_end,
        infra_cost_profile["effective_from_epoch_ms"].as_i64(),
        infra_cost_profile["effective_to_epoch_ms"].as_i64(),
    )
}

pub(super) fn build_reconciliation_contract_json(
    contract: &TokenBudgetContractConfig,
    external_sources: &Value,
    provider_usage_binding: &Value,
    provider_invoice_binding: &Value,
    rate_card: &Value,
) -> Value {
    let provider_usage_status = provider_usage_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    let rate_card_status = rate_card["status"].as_str().unwrap_or("not_configured");
    let provider_invoice_status = provider_invoice_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    let provider_usage_missing = matches!(
        provider_usage_status,
        "not_configured" | "default_path_missing"
    );
    let rate_card_missing = matches!(rate_card_status, "not_configured" | "default_path_missing");
    let usage_truth_state = usage_truth_completeness_state(provider_usage_status);
    let rate_card_truth_state = rate_card_truth_completeness_state(rate_card_status);
    let provider_cost_truth_state =
        provider_cost_truth_completeness_state(provider_usage_status, rate_card_status);
    let invoice_evidence_truth_state =
        invoice_evidence_completeness_state(provider_usage_status, provider_invoice_status);
    let money_truth_state =
        money_truth_completeness_state(provider_cost_truth_state, invoice_evidence_truth_state);
    let reconciliation_readiness_state = reconciliation_readiness_state(
        usage_truth_state,
        provider_cost_truth_state,
        invoice_evidence_truth_state,
    );
    let governance_blocking_reasons = reconciliation_governance_blocking_reasons(
        provider_usage_status,
        rate_card_status,
        provider_invoice_status,
    );
    let usage_provider = bound_provider_name(
        provider_usage_binding,
        &["usage_bound", "usage_and_cost_bound"],
    );
    let rate_card_provider =
        bound_provider_name(rate_card, &["priced_bound", "bound_but_unpriced"]);
    let invoice_provider = bound_provider_name(provider_invoice_binding, &["invoice_bound"]);
    let rate_card_provider_alignment_state =
        provider_alignment_state(usage_provider, rate_card_provider);
    let invoice_provider_alignment_state =
        provider_alignment_state(usage_provider, invoice_provider);
    let provider_identity_state = combined_provider_identity_state(&[
        rate_card_provider_alignment_state,
        invoice_provider_alignment_state,
    ]);
    let required_sources_for_usage_truth =
        source_codes_with_truth_role(external_sources, "required_for_usage_truth");
    let required_sources_for_cost_truth =
        source_codes_with_truth_role(external_sources, "required_for_cost_truth");
    let optional_sources_for_invoice_evidence =
        source_codes_with_truth_role(external_sources, "required_for_invoice_evidence");
    let unready_required_sources_for_usage_truth =
        missing_source_codes_json(if provider_usage_truth_bound(provider_usage_status) {
            Vec::new()
        } else {
            vec!["provider_usage_export"]
        });
    let unready_required_sources_for_cost_truth = missing_source_codes_json({
        let mut missing = Vec::new();
        if !provider_usage_cost_truth_bound(provider_usage_status) {
            missing.push("provider_usage_export");
        }
        if !rate_card_priced_bound(rate_card_status) {
            missing.push("provider_rate_card");
        }
        missing
    });
    let unready_optional_sources_for_invoice_evidence =
        missing_source_codes_json(if provider_invoice_bound(provider_invoice_status) {
            Vec::new()
        } else {
            vec!["provider_invoice_export"]
        });
    let mut governance_blocking_reasons = governance_blocking_reasons;
    if provider_identity_state == "provider_identity_mismatch"
        && !governance_blocking_reasons.contains(&"provider_identity_mismatch")
    {
        governance_blocking_reasons.push("provider_identity_mismatch");
    }
    let ready_for_external_reconciliation = matches!(
        provider_usage_status,
        "usage_bound" | "usage_and_cost_bound"
    );
    let status = if ready_for_external_reconciliation {
        if provider_invoice_status == "invoice_bound" {
            "usage_and_invoice_bound_report_only"
        } else if provider_usage_status == "usage_and_cost_bound" {
            "usage_and_cost_bound_report_only"
        } else {
            "usage_bound_report_only"
        }
    } else if provider_usage_missing {
        "awaiting_provider_usage_source"
    } else if matches!(
        provider_usage_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        "provider_usage_source_error"
    } else if rate_card_missing {
        "awaiting_rate_card_source"
    } else {
        "configured_sources_not_yet_bound"
    };

    json!({
        "contract_version": contract.reconciliation_contract_version.clone(),
        "status": status,
        "ready_for_external_reconciliation": ready_for_external_reconciliation,
        "usage_truth_completeness_state": usage_truth_state,
        "rate_card_truth_completeness_state": rate_card_truth_state,
        "provider_cost_truth_completeness_state": provider_cost_truth_state,
        "invoice_evidence_completeness_state": invoice_evidence_truth_state,
        "money_truth_completeness_state": money_truth_state,
        "reconciliation_readiness_state": reconciliation_readiness_state,
        "governance_blocking_reasons": governance_blocking_reasons,
        "rate_card_provider_alignment_state": rate_card_provider_alignment_state,
        "invoice_provider_alignment_state": invoice_provider_alignment_state,
        "provider_identity_state": provider_identity_state,
        "internal_truth_layers": [
            "token_budget_event",
            "usage_event_schema",
            "statement_previews",
            "agent_cycle_economics"
        ],
        "canonical_internal_scope": "retrieval savings floor + partial whole-agent-cycle lower bound + internal delivered token accounting",
        "external_truth_sources": external_sources.clone(),
        "external_truth_bindings": {
            "provider_usage_export": provider_usage_binding.clone(),
            "provider_invoice_export": provider_invoice_binding.clone(),
            "provider_rate_card": rate_card.clone(),
        },
        "source_requirements": {
            "required_sources_for_usage_truth": required_sources_for_usage_truth,
            "required_sources_for_cost_truth": required_sources_for_cost_truth,
            "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence,
            "unready_required_sources_for_usage_truth": unready_required_sources_for_usage_truth,
            "unready_required_sources_for_cost_truth": unready_required_sources_for_cost_truth,
            "unready_optional_sources_for_invoice_evidence": unready_optional_sources_for_invoice_evidence,
        },
        "note": "Amai уже меряет внутренний lower bound честно, но external reconciliation должен сравнивать provider usage с внутренними delivered tokens, а не с saved tokens. Governance-layer отдельно показывает, дошли ли мы только до usage truth, до usage+cost truth или уже до invoice-side evidence. Это reconciliation contract, а не готовый settlement engine."
    })
}

pub(super) fn build_reconciliation_preview(
    scope_code: &str,
    scope_label: &str,
    statement_preview: &Value,
    contract: &TokenBudgetContractConfig,
    external_sources: &Value,
    provider_usage_binding: &Value,
    provider_invoice_binding: &Value,
    rate_card: &Value,
) -> Value {
    let provider_usage_status = provider_usage_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    let rate_card_status = rate_card["status"].as_str().unwrap_or("not_configured");
    let provider_invoice_status = provider_invoice_binding["status"]
        .as_str()
        .unwrap_or("not_configured");
    let provider_usage_missing = matches!(
        provider_usage_status,
        "not_configured" | "default_path_missing"
    );
    let rate_card_missing = matches!(rate_card_status, "not_configured" | "default_path_missing");
    let usage_truth_state = usage_truth_completeness_state(provider_usage_status);
    let rate_card_truth_state = rate_card_truth_completeness_state(rate_card_status);
    let provider_cost_truth_state =
        provider_cost_truth_completeness_state(provider_usage_status, rate_card_status);
    let invoice_evidence_truth_state =
        invoice_evidence_completeness_state(provider_usage_status, provider_invoice_status);
    let money_truth_state =
        money_truth_completeness_state(provider_cost_truth_state, invoice_evidence_truth_state);
    let readiness_state = reconciliation_readiness_state(
        usage_truth_state,
        provider_cost_truth_state,
        invoice_evidence_truth_state,
    );
    let governance_blocking_reasons = reconciliation_governance_blocking_reasons(
        provider_usage_status,
        rate_card_status,
        provider_invoice_status,
    );
    let provider_usage_alignment_state =
        provider_usage_scope_alignment_state(statement_preview, provider_usage_binding, scope_code);
    let provider_invoice_alignment_state = provider_invoice_scope_alignment_state(
        statement_preview,
        provider_invoice_binding,
        scope_code,
    );
    let rate_card_alignment_state = rate_card_scope_alignment_state(statement_preview, rate_card);
    let usage_provider = bound_provider_name(
        provider_usage_binding,
        &["usage_bound", "usage_and_cost_bound"],
    );
    let rate_card_provider =
        bound_provider_name(rate_card, &["priced_bound", "bound_but_unpriced"]);
    let invoice_provider = bound_provider_name(provider_invoice_binding, &["invoice_bound"]);
    let rate_card_provider_alignment_state =
        provider_alignment_state(usage_provider, rate_card_provider);
    let invoice_provider_alignment_state =
        provider_alignment_state(usage_provider, invoice_provider);
    let provider_identity_state = combined_provider_identity_state(&[
        rate_card_provider_alignment_state,
        invoice_provider_alignment_state,
    ]);
    let required_sources_for_usage_truth =
        source_codes_with_truth_role(external_sources, "required_for_usage_truth");
    let required_sources_for_cost_truth =
        source_codes_with_truth_role(external_sources, "required_for_cost_truth");
    let optional_sources_for_invoice_evidence =
        source_codes_with_truth_role(external_sources, "required_for_invoice_evidence");
    let unready_required_sources_for_usage_truth =
        missing_source_codes_json(if provider_usage_truth_bound(provider_usage_status) {
            Vec::new()
        } else {
            vec!["provider_usage_export"]
        });
    let unready_required_sources_for_cost_truth = missing_source_codes_json({
        let mut missing = Vec::new();
        if !provider_usage_cost_truth_bound(provider_usage_status) {
            missing.push("provider_usage_export");
        }
        if !rate_card_priced_bound(rate_card_status) {
            missing.push("provider_rate_card");
        }
        missing
    });
    let unready_optional_sources_for_invoice_evidence =
        missing_source_codes_json(if provider_invoice_bound(provider_invoice_status) {
            Vec::new()
        } else {
            vec!["provider_invoice_export"]
        });
    let mut temporal_states = vec![provider_usage_alignment_state, rate_card_alignment_state];
    if provider_invoice_status == "invoice_bound" {
        temporal_states.push(provider_invoice_alignment_state);
    }
    let temporal_truth_state = combined_temporal_truth_state(&temporal_states);
    let mut temporal_blocking_reasons = Vec::new();
    if provider_usage_alignment_state == "scope_period_mismatch" {
        temporal_blocking_reasons.push("provider_usage_scope_period_mismatch");
    }
    if provider_invoice_alignment_state == "scope_period_mismatch" {
        temporal_blocking_reasons.push("provider_invoice_scope_period_mismatch");
    }
    if rate_card_alignment_state == "scope_period_mismatch" {
        temporal_blocking_reasons.push("provider_rate_card_scope_period_mismatch");
    }
    if provider_identity_state == "provider_identity_mismatch" {
        temporal_blocking_reasons.push("provider_identity_mismatch");
    }
    if provider_usage_missing {
        let blocking_reasons =
            base_reconciliation_blocking_reasons(statement_preview, rate_card, true);
        return json!({
            "scope_code": scope_code,
            "scope_label": scope_label,
            "reconciliation_state": "awaiting_provider_usage_source",
            "usage_truth_completeness_state": usage_truth_state,
            "rate_card_truth_completeness_state": rate_card_truth_state,
            "provider_cost_truth_completeness_state": provider_cost_truth_state,
            "invoice_evidence_completeness_state": invoice_evidence_truth_state,
            "money_truth_completeness_state": money_truth_state,
            "reconciliation_readiness_state": readiness_state,
            "governance_blocking_reasons": governance_blocking_reasons,
            "usage_reconciliation_state": "awaiting_provider_usage_source",
            "invoice_reconciliation_state": if provider_invoice_status == "invoice_bound" {
                "invoice_bound_without_usage"
            } else {
                "invoice_not_bound"
            },
            "provider_usage_scope_alignment_state": provider_usage_alignment_state,
            "provider_invoice_scope_alignment_state": provider_invoice_alignment_state,
            "rate_card_scope_alignment_state": rate_card_alignment_state,
            "temporal_truth_state": temporal_truth_state,
            "rate_card_provider_alignment_state": rate_card_provider_alignment_state,
            "invoice_provider_alignment_state": invoice_provider_alignment_state,
            "provider_identity_state": provider_identity_state,
            "coverage": statement_preview["coverage"].clone(),
            "internal_provider_billed_tokens": statement_preview["internal_provider_billed_tokens"].clone(),
            "internal_provider_cost_estimate_amount": Value::Null,
            "internal_delivered_tokens": statement_preview["internal_delivered_tokens"].clone(),
            "internal_recovery_tokens": statement_preview["internal_recovery_tokens"].clone(),
            "internal_measured_non_billable_lower_bound_tokens": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
            "billable_lower_bound_tokens": statement_preview["billable_lower_bound_tokens"].clone(),
            "external_provider_usage_tokens": Value::Null,
            "external_provider_cost_amount": Value::Null,
            "external_invoice_amount": Value::Null,
            "drift_tokens": Value::Null,
            "drift_amount": Value::Null,
            "invoice_drift_amount": Value::Null,
            "currency_profile": contract.currency_profile.clone(),
            "external_truth_sources": external_sources.clone(),
            "external_truth_bindings": {
                "provider_usage_export": provider_usage_binding.clone(),
                "provider_invoice_export": provider_invoice_binding.clone(),
                "provider_rate_card": rate_card.clone(),
            },
            "required_sources_for_usage_truth": required_sources_for_usage_truth.clone(),
            "required_sources_for_cost_truth": required_sources_for_cost_truth.clone(),
            "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence.clone(),
            "unready_required_sources_for_usage_truth": unready_required_sources_for_usage_truth.clone(),
            "unready_required_sources_for_cost_truth": unready_required_sources_for_cost_truth.clone(),
            "unready_optional_sources_for_invoice_evidence": unready_optional_sources_for_invoice_evidence.clone(),
            "blocking_reasons": blocking_reasons,
            "note": "Этот preview честно показывает internal delivered tokens и retrieval lower bound по scope. Drift по токенам считается только между internal delivered usage и external provider usage, а не между provider usage и saved tokens."
        });
    } else if matches!(
        provider_usage_status,
        "configured_path_missing" | "read_error" | "parse_error"
    ) {
        let blocking_reasons =
            base_reconciliation_blocking_reasons(statement_preview, rate_card, true);
        return json!({
            "scope_code": scope_code,
            "scope_label": scope_label,
            "reconciliation_state": "provider_usage_source_error",
            "usage_truth_completeness_state": usage_truth_state,
            "rate_card_truth_completeness_state": rate_card_truth_state,
            "provider_cost_truth_completeness_state": provider_cost_truth_state,
            "invoice_evidence_completeness_state": invoice_evidence_truth_state,
            "money_truth_completeness_state": money_truth_state,
            "reconciliation_readiness_state": readiness_state,
            "governance_blocking_reasons": governance_blocking_reasons,
            "usage_reconciliation_state": "provider_usage_source_error",
            "invoice_reconciliation_state": if provider_invoice_status == "invoice_bound" {
                "invoice_bound_without_usage"
            } else {
                "invoice_not_bound"
            },
            "provider_usage_scope_alignment_state": provider_usage_alignment_state,
            "provider_invoice_scope_alignment_state": provider_invoice_alignment_state,
            "rate_card_scope_alignment_state": rate_card_alignment_state,
            "temporal_truth_state": temporal_truth_state,
            "rate_card_provider_alignment_state": rate_card_provider_alignment_state,
            "invoice_provider_alignment_state": invoice_provider_alignment_state,
            "provider_identity_state": provider_identity_state,
            "coverage": statement_preview["coverage"].clone(),
            "internal_provider_billed_tokens": statement_preview["internal_provider_billed_tokens"].clone(),
            "internal_provider_cost_estimate_amount": Value::Null,
            "internal_delivered_tokens": statement_preview["internal_delivered_tokens"].clone(),
            "internal_recovery_tokens": statement_preview["internal_recovery_tokens"].clone(),
            "internal_measured_non_billable_lower_bound_tokens": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
            "billable_lower_bound_tokens": statement_preview["billable_lower_bound_tokens"].clone(),
            "external_provider_usage_tokens": Value::Null,
            "external_provider_cost_amount": Value::Null,
            "external_invoice_amount": Value::Null,
            "drift_tokens": Value::Null,
            "drift_amount": Value::Null,
            "invoice_drift_amount": Value::Null,
            "currency_profile": contract.currency_profile.clone(),
            "external_truth_sources": external_sources.clone(),
            "external_truth_bindings": {
                "provider_usage_export": provider_usage_binding.clone(),
                "provider_invoice_export": provider_invoice_binding.clone(),
                "provider_rate_card": rate_card.clone(),
            },
            "required_sources_for_usage_truth": required_sources_for_usage_truth.clone(),
            "required_sources_for_cost_truth": required_sources_for_cost_truth.clone(),
            "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence.clone(),
            "unready_required_sources_for_usage_truth": unready_required_sources_for_usage_truth.clone(),
            "unready_required_sources_for_cost_truth": unready_required_sources_for_cost_truth.clone(),
            "unready_optional_sources_for_invoice_evidence": unready_optional_sources_for_invoice_evidence.clone(),
            "blocking_reasons": blocking_reasons,
            "note": "Этот preview честно показывает internal delivered tokens и retrieval lower bound по scope. Drift по токенам считается только между internal delivered usage и external provider usage, а не между provider usage и saved tokens."
        });
    } else if rate_card_missing {
        let mut blocking_reasons =
            base_reconciliation_blocking_reasons(statement_preview, rate_card, false);
        blocking_reasons.retain(|reason| *reason != "provider_rate_card_unpriced");
        blocking_reasons.insert(0, "provider_rate_card_unpriced");
        return json!({
            "scope_code": scope_code,
            "scope_label": scope_label,
            "reconciliation_state": "awaiting_rate_card_source",
            "usage_truth_completeness_state": usage_truth_state,
            "rate_card_truth_completeness_state": rate_card_truth_state,
            "provider_cost_truth_completeness_state": provider_cost_truth_state,
            "invoice_evidence_completeness_state": invoice_evidence_truth_state,
            "money_truth_completeness_state": money_truth_state,
            "reconciliation_readiness_state": readiness_state,
            "governance_blocking_reasons": governance_blocking_reasons,
            "usage_reconciliation_state": "external_usage_bound_report_only",
            "invoice_reconciliation_state": if provider_invoice_status == "invoice_bound" {
                "invoice_bound_report_only"
            } else {
                "invoice_not_bound"
            },
            "provider_usage_scope_alignment_state": provider_usage_alignment_state,
            "provider_invoice_scope_alignment_state": provider_invoice_alignment_state,
            "rate_card_scope_alignment_state": rate_card_alignment_state,
            "temporal_truth_state": temporal_truth_state,
            "rate_card_provider_alignment_state": rate_card_provider_alignment_state,
            "invoice_provider_alignment_state": invoice_provider_alignment_state,
            "provider_identity_state": provider_identity_state,
            "coverage": statement_preview["coverage"].clone(),
            "internal_provider_billed_tokens": statement_preview["internal_provider_billed_tokens"].clone(),
            "internal_provider_cost_estimate_amount": Value::Null,
            "internal_delivered_tokens": statement_preview["internal_delivered_tokens"].clone(),
            "internal_recovery_tokens": statement_preview["internal_recovery_tokens"].clone(),
            "internal_measured_non_billable_lower_bound_tokens": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
            "billable_lower_bound_tokens": statement_preview["billable_lower_bound_tokens"].clone(),
            "external_provider_usage_tokens": provider_usage_binding["scopes"][scope_code]["total_tokens"].clone(),
            "external_provider_cost_amount": provider_usage_binding["scopes"][scope_code]["provider_cost_amount"].clone(),
            "external_invoice_amount": provider_invoice_binding["scopes"][scope_code]["invoice_amount"].clone(),
            "drift_tokens": Value::Null,
            "drift_amount": Value::Null,
            "invoice_drift_amount": Value::Null,
            "currency_profile": contract.currency_profile.clone(),
            "external_truth_sources": external_sources.clone(),
            "external_truth_bindings": {
                "provider_usage_export": provider_usage_binding.clone(),
                "provider_invoice_export": provider_invoice_binding.clone(),
                "provider_rate_card": rate_card.clone(),
            },
            "required_sources_for_usage_truth": required_sources_for_usage_truth.clone(),
            "required_sources_for_cost_truth": required_sources_for_cost_truth.clone(),
            "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence.clone(),
            "unready_required_sources_for_usage_truth": unready_required_sources_for_usage_truth.clone(),
            "unready_required_sources_for_cost_truth": unready_required_sources_for_cost_truth.clone(),
            "unready_optional_sources_for_invoice_evidence": unready_optional_sources_for_invoice_evidence.clone(),
            "blocking_reasons": blocking_reasons,
            "note": "Этот preview честно показывает internal delivered tokens и retrieval lower bound по scope. Drift по токенам считается только между internal delivered usage и external provider usage, а не между provider usage и saved tokens."
        });
    };
    let mut blocking_reasons = Vec::new();
    blocking_reasons.push("billing_policy_report_only");
    if statement_preview["billable_lower_bound_tokens"].is_null() {
        blocking_reasons.push("billable_lower_bound_not_materialized");
    }
    if contract.billing_mode == "report_only" {
        blocking_reasons.push("billing_mode_report_only");
    }
    let usage_scope = &provider_usage_binding["scopes"][scope_code];
    let invoice_scope = &provider_invoice_binding["scopes"][scope_code];
    let internal_provider_billed_tokens = statement_preview["internal_provider_billed_tokens"]
        .as_u64()
        .unwrap_or(0);
    let internal_provider_cost_estimate =
        internal_provider_cost_estimate_amount(internal_provider_billed_tokens, rate_card);
    let external_provider_usage_tokens = usage_scope["total_tokens"].clone();
    let drift_tokens = match external_provider_usage_tokens.as_u64() {
        Some(external_tokens) => {
            json!(internal_provider_billed_tokens as i64 - external_tokens as i64)
        }
        None => Value::Null,
    };
    let external_provider_cost_amount = usage_scope["provider_cost_amount"].clone();
    let external_invoice_amount = invoice_scope["invoice_amount"].clone();
    let drift_amount = amount_delta(
        internal_provider_cost_estimate,
        external_provider_cost_amount.as_f64(),
    );
    let invoice_drift_amount = amount_delta(
        external_provider_cost_amount.as_f64(),
        external_invoice_amount.as_f64(),
    );
    let usage_reconciliation_state = match drift_tokens.as_i64() {
        Some(0) => "external_usage_aligned_report_only",
        Some(_) => {
            blocking_reasons.push("provider_usage_drift_detected");
            "external_usage_drift_report_only"
        }
        None => "external_usage_bound_report_only",
    };
    let invoice_reconciliation_state = if provider_invoice_status == "invoice_bound" {
        match invoice_drift_amount.as_f64() {
            Some(value) if value.abs() < 1e-9 => "invoice_aligned_report_only",
            Some(_) => {
                blocking_reasons.push("provider_invoice_drift_detected");
                "invoice_drift_report_only"
            }
            None => "invoice_bound_report_only",
        }
    } else {
        "invoice_not_bound"
    };
    let reconciliation_state = if usage_reconciliation_state == "external_usage_aligned_report_only"
    {
        if invoice_reconciliation_state == "invoice_aligned_report_only" {
            "external_usage_and_invoice_aligned_report_only"
        } else {
            "external_usage_aligned_report_only"
        }
    } else if usage_reconciliation_state == "external_usage_drift_report_only" {
        if invoice_reconciliation_state == "invoice_drift_report_only" {
            "external_usage_and_invoice_drift_report_only"
        } else {
            "external_usage_drift_report_only"
        }
    } else if provider_invoice_status == "invoice_bound" {
        "external_usage_and_invoice_bound_report_only"
    } else if provider_usage_status == "usage_and_cost_bound" {
        "external_usage_and_cost_bound_report_only"
    } else {
        "external_usage_bound_report_only"
    };
    for reason in temporal_blocking_reasons {
        if !blocking_reasons.contains(&reason) {
            blocking_reasons.push(reason);
        }
    }

    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "reconciliation_state": reconciliation_state,
        "usage_truth_completeness_state": usage_truth_state,
        "rate_card_truth_completeness_state": rate_card_truth_state,
        "provider_cost_truth_completeness_state": provider_cost_truth_state,
        "invoice_evidence_completeness_state": invoice_evidence_truth_state,
        "money_truth_completeness_state": money_truth_state,
        "reconciliation_readiness_state": readiness_state,
        "governance_blocking_reasons": governance_blocking_reasons,
        "usage_reconciliation_state": usage_reconciliation_state,
        "invoice_reconciliation_state": invoice_reconciliation_state,
        "provider_usage_scope_alignment_state": provider_usage_alignment_state,
        "provider_invoice_scope_alignment_state": provider_invoice_alignment_state,
        "rate_card_scope_alignment_state": rate_card_alignment_state,
        "temporal_truth_state": temporal_truth_state,
        "rate_card_provider_alignment_state": rate_card_provider_alignment_state,
        "invoice_provider_alignment_state": invoice_provider_alignment_state,
        "provider_identity_state": provider_identity_state,
        "coverage": statement_preview["coverage"].clone(),
        "internal_provider_billed_tokens": statement_preview["internal_provider_billed_tokens"].clone(),
        "internal_provider_cost_estimate_amount": internal_provider_cost_estimate,
        "internal_delivered_tokens": statement_preview["internal_delivered_tokens"].clone(),
        "internal_recovery_tokens": statement_preview["internal_recovery_tokens"].clone(),
        "internal_observed_whole_cycle_lower_bound_tokens": statement_preview["internal_observed_whole_cycle_lower_bound_tokens"].clone(),
        "verified_internal_observed_whole_cycle_lower_bound_tokens": statement_preview["verified_internal_observed_whole_cycle_lower_bound_tokens"].clone(),
        "internal_measured_non_billable_lower_bound_tokens": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
        "billable_lower_bound_tokens": statement_preview["billable_lower_bound_tokens"].clone(),
        "external_provider_usage_tokens": external_provider_usage_tokens,
        "external_provider_cost_amount": external_provider_cost_amount,
        "external_invoice_amount": external_invoice_amount,
        "drift_tokens": drift_tokens,
        "drift_amount": drift_amount,
        "invoice_drift_amount": invoice_drift_amount,
        "currency_profile": usage_scope["currency_profile"]
            .as_str()
            .or_else(|| invoice_scope["currency_profile"].as_str())
            .or_else(|| rate_card["bound_currency_profile"].as_str())
            .unwrap_or(&contract.currency_profile)
            .to_string(),
        "external_truth_sources": external_sources.clone(),
        "external_truth_bindings": {
            "provider_usage_export": provider_usage_binding.clone(),
            "provider_invoice_export": provider_invoice_binding.clone(),
            "provider_rate_card": rate_card.clone(),
        },
        "required_sources_for_usage_truth": required_sources_for_usage_truth,
        "required_sources_for_cost_truth": required_sources_for_cost_truth,
        "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence,
        "unready_required_sources_for_usage_truth": unready_required_sources_for_usage_truth,
        "unready_required_sources_for_cost_truth": unready_required_sources_for_cost_truth,
        "unready_optional_sources_for_invoice_evidence": unready_optional_sources_for_invoice_evidence,
        "blocking_reasons": blocking_reasons,
        "note": "Этот preview честно показывает internal delivered tokens и retrieval lower bound по scope. Drift по токенам считается только между internal delivered usage и external provider usage, а не между provider usage и saved tokens."
    })
}

pub(super) fn build_margin_contract_json(
    contract: &TokenBudgetContractConfig,
    external_sources: &Value,
    rate_card: &Value,
    infra_cost_profile: &Value,
    reconciliation_contract: &Value,
) -> Value {
    let rate_card_priced = rate_card["money_conversion_enabled"]
        .as_bool()
        .unwrap_or(false);
    let infra_cost_status = infra_cost_profile["status"]
        .as_str()
        .unwrap_or("not_configured");
    let rate_card_truth_state = rate_card_truth_completeness_state(
        rate_card["status"].as_str().unwrap_or("not_configured"),
    );
    let infra_cost_truth_state = infra_cost_truth_completeness_state(infra_cost_status);
    let pricing_truth_state =
        pricing_truth_completeness_state(rate_card_truth_state, infra_cost_truth_state);
    let customer_savings_truth_state =
        customer_savings_money_truth_completeness_state(rate_card_truth_state);
    let amai_cost_truth_state = amai_cost_truth_completeness_state(infra_cost_truth_state);
    let margin_truth_state =
        margin_truth_completeness_state(customer_savings_truth_state, amai_cost_truth_state);
    let provider_status = reconciliation_contract["status"]
        .as_str()
        .unwrap_or("awaiting_provider_usage_source");
    let usage_bound =
        provider_status.starts_with("usage_") || provider_status.starts_with("external_usage_");
    let provider_identity_state = reconciliation_contract["provider_identity_state"]
        .as_str()
        .unwrap_or("provider_identity_unchecked");
    let status = if !rate_card_priced {
        "awaiting_rate_card"
    } else if infra_cost_status != "priced_bound" {
        "awaiting_infra_cost_profile"
    } else if !usage_bound
        && reconciliation_contract["ready_for_external_reconciliation"].as_bool() != Some(true)
    {
        "awaiting_provider_reconciliation"
    } else if provider_identity_state == "provider_identity_mismatch" {
        "provider_identity_mismatch"
    } else {
        "priced_preview_report_only"
    };
    let required_sources_for_margin_truth =
        source_codes_with_truth_role(external_sources, "required_for_margin_truth");
    let optional_sources_for_invoice_evidence =
        source_codes_with_truth_role(external_sources, "required_for_invoice_evidence");
    let unready_required_sources_for_margin_truth = missing_source_codes_json({
        let mut missing = Vec::new();
        if !provider_usage_truth_bound(
            reconciliation_contract["external_truth_bindings"]["provider_usage_export"]["status"]
                .as_str()
                .unwrap_or("not_configured"),
        ) {
            missing.push("provider_usage_export");
        }
        if !rate_card_priced_bound(rate_card["status"].as_str().unwrap_or("not_configured")) {
            missing.push("provider_rate_card");
        }
        if !infra_cost_profile_priced_bound(infra_cost_status) {
            missing.push("infra_cost_profile");
        }
        missing
    });

    json!({
        "model_version": contract.margin_model_version.clone(),
        "infra_cost_profile_version": contract.infra_cost_profile_version.clone(),
        "infra_cost_binding_model_version": contract.infra_cost_binding_model_version.clone(),
        "rate_card_truth_completeness_state": rate_card_truth_state,
        "infra_cost_truth_completeness_state": infra_cost_truth_state,
        "pricing_truth_completeness_state": pricing_truth_state,
        "customer_savings_money_truth_completeness_state": customer_savings_truth_state,
        "amai_cost_truth_completeness_state": amai_cost_truth_state,
        "margin_truth_completeness_state": margin_truth_state,
        "margin_readiness_state": status,
        "rate_card_status": rate_card["status"].clone(),
        "rate_card_temporal_scope_state": rate_card["temporal_scope_state"].clone(),
        "infra_cost_temporal_scope_state": infra_cost_profile["temporal_scope_state"].clone(),
        "provider_identity_state": provider_identity_state,
        "status": status,
        "money_margin_enabled": status == "priced_preview_report_only",
        "infra_cost_profile": infra_cost_profile.clone(),
        "source_requirements": {
            "required_sources_for_margin_truth": required_sources_for_margin_truth,
            "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence,
            "unready_required_sources_for_margin_truth": unready_required_sources_for_margin_truth,
        },
        "note": "Margin layer требует одновременно priced rate card, provider usage binding и infra cost profile. Temporal scope pricing проверяется уже на уровне scope preview, чтобы rate card и infra profile не выглядели применимыми к периоду без отдельной проверки. Даже после этого слой остаётся report-only preview, а не invoice."
    })
}

pub(super) fn build_margin_scope(
    external_sources: &Value,
    scope_code: &str,
    scope_label: &str,
    statement_preview: &Value,
    reconciliation_preview: &Value,
    rate_card: &Value,
    infra_cost_profile: &Value,
) -> Value {
    let rate_card_priced = rate_card["money_conversion_enabled"]
        .as_bool()
        .unwrap_or(false);
    let infra_cost_status = infra_cost_profile["status"]
        .as_str()
        .unwrap_or("not_configured");
    let rate_card_truth_state = rate_card_truth_completeness_state(
        rate_card["status"].as_str().unwrap_or("not_configured"),
    );
    let infra_cost_truth_state = infra_cost_truth_completeness_state(infra_cost_status);
    let pricing_truth_state =
        pricing_truth_completeness_state(rate_card_truth_state, infra_cost_truth_state);
    let customer_savings_truth_state =
        customer_savings_money_truth_completeness_state(rate_card_truth_state);
    let amai_cost_truth_state = amai_cost_truth_completeness_state(infra_cost_truth_state);
    let margin_truth_state =
        margin_truth_completeness_state(customer_savings_truth_state, amai_cost_truth_state);
    let reconciliation_state = reconciliation_preview["reconciliation_state"]
        .as_str()
        .unwrap_or("awaiting_provider_usage_source");
    let usage_bound = reconciliation_state.starts_with("external_usage_");
    let usage_drifted = reconciliation_state.contains("_drift_");
    let currency_match = rate_card["bound_currency_profile"].as_str()
        == infra_cost_profile["bound_currency_profile"].as_str();
    let provider_usage_alignment_state =
        reconciliation_preview["provider_usage_scope_alignment_state"]
            .as_str()
            .unwrap_or("provider_usage_not_bound");
    let rate_card_alignment_state = reconciliation_preview["rate_card_scope_alignment_state"]
        .as_str()
        .unwrap_or("rate_card_not_bound");
    let provider_identity_state = reconciliation_preview["provider_identity_state"]
        .as_str()
        .unwrap_or("provider_identity_unchecked");
    let infra_cost_alignment_state =
        infra_cost_scope_alignment_state(statement_preview, infra_cost_profile);
    let temporal_truth_state = combined_temporal_truth_state(&[
        provider_usage_alignment_state,
        rate_card_alignment_state,
        infra_cost_alignment_state,
    ]);
    let margin_state = if !rate_card_priced {
        "awaiting_rate_card"
    } else if infra_cost_status != "priced_bound" {
        "awaiting_infra_cost_profile"
    } else if !usage_bound {
        "awaiting_provider_reconciliation"
    } else if provider_identity_state == "provider_identity_mismatch" {
        "provider_identity_mismatch"
    } else if temporal_truth_state == "scope_period_mismatch" {
        "pricing_period_mismatch"
    } else if !currency_match {
        "currency_profile_mismatch"
    } else if usage_drifted {
        "priced_preview_with_provider_drift"
    } else if matches!(
        temporal_truth_state,
        "source_period_unspecified" | "source_period_partially_bound" | "scope_period_unknown"
    ) {
        "priced_preview_temporal_unscoped_report_only"
    } else {
        "priced_preview_report_only"
    };
    let margin_confidence_state = match margin_state {
        "priced_preview_report_only" => "aligned_report_only",
        "priced_preview_with_provider_drift" => "provider_drift_detected",
        "pricing_period_mismatch" => "pricing_period_mismatch",
        "priced_preview_temporal_unscoped_report_only" => "period_unscoped_report_only",
        "currency_profile_mismatch" => "currency_profile_mismatch",
        "awaiting_provider_reconciliation" => "awaiting_provider_reconciliation",
        "awaiting_infra_cost_profile" => "awaiting_infra_cost_profile",
        "provider_identity_mismatch" => "provider_identity_mismatch",
        _ => "awaiting_rate_card",
    };
    let margin_readiness_state = match margin_state {
        "awaiting_rate_card" | "awaiting_infra_cost_profile" => "awaiting_pricing_truth",
        "awaiting_provider_reconciliation" => "awaiting_usage_truth",
        "provider_identity_mismatch" => "provider_identity_mismatch",
        "pricing_period_mismatch" => "pricing_period_mismatch",
        "currency_profile_mismatch" => "currency_profile_mismatch",
        "priced_preview_with_provider_drift" => "provider_drift_detected",
        "priced_preview_temporal_unscoped_report_only" => "temporal_truth_unscoped_report_only",
        "priced_preview_report_only" => "preview_ready_report_only",
        _ => "awaiting_pricing_truth",
    };
    let mut blocking_reasons = Vec::new();
    if !rate_card_priced {
        blocking_reasons.push("rate_card_unpriced");
    }
    if infra_cost_status != "priced_bound" {
        blocking_reasons.push("infra_cost_profile_missing");
    }
    if !usage_bound {
        blocking_reasons.push("provider_reconciliation_not_complete");
    }
    if provider_identity_state == "provider_identity_mismatch" {
        blocking_reasons.push("provider_identity_mismatch");
    }
    if provider_usage_alignment_state == "scope_period_mismatch" {
        blocking_reasons.push("provider_usage_scope_period_mismatch");
    }
    if rate_card_alignment_state == "scope_period_mismatch" {
        blocking_reasons.push("provider_rate_card_scope_period_mismatch");
    }
    if infra_cost_alignment_state == "scope_period_mismatch" {
        blocking_reasons.push("infra_cost_scope_period_mismatch");
    }
    if !currency_match && rate_card_priced && infra_cost_status == "priced_bound" {
        blocking_reasons.push("currency_profile_mismatch");
    }
    if usage_drifted {
        blocking_reasons.push("provider_usage_drift_detected");
    }
    let customer_saved_amount_lower_bound =
        statement_preview["measured_non_billable_lower_bound_tokens"]
            .as_i64()
            .and_then(|tokens| {
                rate_card["default_input_cost_per_1k_tokens"]
                    .as_f64()
                    .map(|rate| (tokens as f64 / 1000.0) * rate)
            });
    let amai_infra_cost_amount = if rate_card_priced && infra_cost_status == "priced_bound" {
        let per_1k = infra_cost_profile["cost_per_1k_internal_billed_tokens"]
            .as_f64()
            .unwrap_or(0.0);
        let per_event = infra_cost_profile["cost_per_live_event"]
            .as_f64()
            .unwrap_or(0.0);
        let fixed_scope = infra_cost_profile["fixed_scope_cost_amount"]
            .as_f64()
            .unwrap_or(0.0);
        let internal_provider_billed_tokens = statement_preview["internal_provider_billed_tokens"]
            .as_u64()
            .unwrap_or(0);
        let included_events = statement_preview["coverage"]["included_events"]
            .as_u64()
            .unwrap_or(0);
        Some(
            (internal_provider_billed_tokens as f64 / 1000.0) * per_1k
                + (included_events as f64) * per_event
                + fixed_scope,
        )
    } else {
        None
    };
    let margin_amount = match (customer_saved_amount_lower_bound, amai_infra_cost_amount) {
        (Some(saved), Some(cost)) if currency_match => Some(saved - cost),
        _ => None,
    };
    let savings_to_cost_ratio = match (customer_saved_amount_lower_bound, amai_infra_cost_amount) {
        (Some(saved), Some(cost)) if currency_match && cost > 0.0 => Some(saved / cost),
        _ => None,
    };
    let currency_profile = if currency_match {
        rate_card["bound_currency_profile"]
            .as_str()
            .unwrap_or("unpriced")
            .to_string()
    } else {
        "mismatch".to_string()
    };
    let required_sources_for_margin_truth =
        source_codes_with_truth_role(external_sources, "required_for_margin_truth");
    let optional_sources_for_invoice_evidence =
        source_codes_with_truth_role(external_sources, "required_for_invoice_evidence");
    let unready_required_sources_for_margin_truth = missing_source_codes_json({
        let mut missing = Vec::new();
        if !provider_usage_truth_bound(
            reconciliation_preview["external_truth_bindings"]["provider_usage_export"]["status"]
                .as_str()
                .unwrap_or("not_configured"),
        ) {
            missing.push("provider_usage_export");
        }
        if !rate_card_priced_bound(rate_card["status"].as_str().unwrap_or("not_configured")) {
            missing.push("provider_rate_card");
        }
        if !infra_cost_profile_priced_bound(infra_cost_status) {
            missing.push("infra_cost_profile");
        }
        missing
    });

    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "margin_state": margin_state,
        "margin_confidence_state": margin_confidence_state,
        "margin_readiness_state": margin_readiness_state,
        "rate_card_truth_completeness_state": rate_card_truth_state,
        "infra_cost_truth_completeness_state": infra_cost_truth_state,
        "pricing_truth_completeness_state": pricing_truth_state,
        "customer_savings_money_truth_completeness_state": customer_savings_truth_state,
        "amai_cost_truth_completeness_state": amai_cost_truth_state,
        "margin_truth_completeness_state": margin_truth_state,
        "provider_usage_scope_alignment_state": provider_usage_alignment_state,
        "rate_card_scope_alignment_state": rate_card_alignment_state,
        "infra_cost_scope_alignment_state": infra_cost_alignment_state,
        "provider_identity_state": provider_identity_state,
        "temporal_truth_state": temporal_truth_state,
        "customer_saved_tokens_lower_bound": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
        "customer_saved_amount_lower_bound": customer_saved_amount_lower_bound,
        "amai_infra_cost_amount": amai_infra_cost_amount,
        "margin_amount": margin_amount,
        "savings_to_cost_ratio": savings_to_cost_ratio,
        "currency_profile": currency_profile,
        "coverage": statement_preview["coverage"].clone(),
        "reconciliation_state": reconciliation_preview["reconciliation_state"].clone(),
        "infra_cost_profile": infra_cost_profile.clone(),
        "required_sources_for_margin_truth": required_sources_for_margin_truth,
        "optional_sources_for_invoice_evidence": optional_sources_for_invoice_evidence,
        "unready_required_sources_for_margin_truth": unready_required_sources_for_margin_truth,
        "blocking_reasons": blocking_reasons,
        "note": "Margin preview опирается на confirmed lower bound, provider input rate и bound infra cost profile. Это всё ещё report-only preview, а не invoice."
    })
}

pub(super) fn statement_lifecycle_state(adjustment_preview: &Value) -> &'static str {
    if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_non_billable_dispute_hold"
    } else if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_non_billable_pending_adjustment"
    } else if adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_non_billable_adjusted_report_only"
    } else {
        "measured_non_billable_open"
    }
}

pub(super) fn settlement_stage(
    measured_events: usize,
    adjustment_preview: &Value,
    metering_freshness: &Value,
    provisional_close_candidate: bool,
) -> &'static str {
    if measured_events == 0 {
        "empty_report_only"
    } else if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_disputed_report_only"
    } else if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_pending_adjustment_report_only"
    } else if adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "measured_adjusted_report_only"
    } else if metering_freshness["can_treat_scope_as_stable"].as_bool() == Some(true)
        && provisional_close_candidate
    {
        "measured_review_ready_report_only"
    } else {
        "measured_open_report_only"
    }
}

pub(super) fn settlement_stage_family(stage: &str) -> &'static str {
    match stage {
        "empty_report_only" => "empty",
        "measured_disputed_report_only"
        | "measured_pending_adjustment_report_only"
        | "measured_adjusted_report_only"
        | "measured_review_ready_report_only"
        | "measured_open_report_only" => "measured_report_only",
        "billable_reserved" | "settled_reserved" | "invoiced_reserved" | "credited_reserved"
        | "disputed_reserved" | "closed_reserved" => "future_reserved",
        _ => "unknown",
    }
}

pub(super) fn future_reserved_settlement_stages() -> [&'static str; 6] {
    [
        "billable_reserved",
        "settled_reserved",
        "invoiced_reserved",
        "credited_reserved",
        "disputed_reserved",
        "closed_reserved",
    ]
}

pub(super) fn next_settlement_stage_candidate(
    measured_events: usize,
    metering_freshness: &Value,
    provisional_close_candidate: bool,
    billing_close_barriers: &[String],
) -> &'static str {
    if measured_events == 0 {
        "awaiting_measured_usage"
    } else if !(metering_freshness["can_treat_scope_as_stable"].as_bool() == Some(true)
        && provisional_close_candidate)
    {
        "review_ready_blocked"
    } else if !billing_close_barriers.is_empty() {
        "billable_blocked"
    } else {
        "billable_reserved"
    }
}

pub(super) fn next_settlement_stage_blockers(
    measured_events: usize,
    provisional_close_barriers: &[String],
    billing_close_barriers: &[String],
) -> Vec<String> {
    if measured_events == 0 {
        return vec!["no_measured_usage_events".to_string()];
    }
    if !provisional_close_barriers.is_empty() {
        return provisional_close_barriers.to_vec();
    }
    billing_close_barriers.to_vec()
}

pub(super) fn merge_string_slices(slices: &[&[String]]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();
    for slice in slices {
        for item in *slice {
            if seen.insert(item.clone()) {
                merged.push(item.clone());
            }
        }
    }
    merged
}

pub(super) fn transactional_status_entry(
    status: &str,
    boundary: &str,
    materialized: bool,
    blocking_reasons: Vec<String>,
) -> Value {
    json!({
        "status": status,
        "boundary": boundary,
        "materialized": materialized,
        "blocking_reasons": blocking_reasons,
    })
}

pub(super) fn build_transactional_statuses(
    contract: &TokenBudgetContractConfig,
    measured_events: usize,
    settlement_stage: &str,
    next_stage_candidate: &str,
    next_stage_blockers: &[String],
    billing_close_barriers: &[String],
    adjustment_preview: &Value,
) -> Value {
    let no_usage_reasons = vec!["no_measured_usage_events".to_string()];
    let review_ready = settlement_stage == "measured_review_ready_report_only";
    let measured = if measured_events == 0 {
        transactional_status_entry(
            "awaiting_measured_usage",
            "not_started",
            false,
            no_usage_reasons.clone(),
        )
    } else {
        transactional_status_entry(settlement_stage, "measured_report_only", true, Vec::new())
    };
    let review = if measured_events == 0 {
        transactional_status_entry(
            "awaiting_measured_usage",
            "not_started",
            false,
            no_usage_reasons.clone(),
        )
    } else if review_ready {
        transactional_status_entry(
            "review_ready_report_only",
            "measured_report_only",
            true,
            Vec::new(),
        )
    } else {
        transactional_status_entry(
            "review_blocked_report_only",
            "measured_report_only",
            true,
            next_stage_blockers.to_vec(),
        )
    };
    let billable = if next_stage_candidate == "billable_reserved" {
        transactional_status_entry("billable_reserved", "reserved_future", false, Vec::new())
    } else if measured_events == 0 {
        transactional_status_entry(
            "awaiting_measured_usage",
            "reserved_future",
            false,
            no_usage_reasons.clone(),
        )
    } else {
        transactional_status_entry(
            "billable_blocked_reserved",
            "reserved_future",
            false,
            if next_stage_blockers.is_empty() {
                billing_close_barriers.to_vec()
            } else {
                next_stage_blockers.to_vec()
            },
        )
    };
    let reserved_follow_on_blockers = if measured_events == 0 {
        no_usage_reasons.clone()
    } else {
        merge_string_slices(&[
            billing_close_barriers,
            &vec!["billable_not_materialized".to_string()],
        ])
    };
    let disputed = if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        transactional_status_entry(
            "dispute_hold_report_only",
            "measured_report_only",
            true,
            vec!["open_dispute_entries".to_string()],
        )
    } else {
        transactional_status_entry(
            "disputed_reserved",
            "reserved_future",
            false,
            vec!["no_open_dispute_entries".to_string()],
        )
    };

    json!({
        "model_version": contract.settlement_lifecycle_model_version.clone(),
        "measured": measured,
        "review": review,
        "billable": billable,
        "settled": transactional_status_entry(
            "settled_reserved",
            "reserved_future",
            false,
            reserved_follow_on_blockers.clone(),
        ),
        "invoiced": transactional_status_entry(
            "invoiced_reserved",
            "reserved_future",
            false,
            reserved_follow_on_blockers.clone(),
        ),
        "credited": transactional_status_entry(
            "credited_reserved",
            "reserved_future",
            false,
            reserved_follow_on_blockers.clone(),
        ),
        "disputed": disputed,
        "closed": transactional_status_entry(
            "closed_reserved",
            "reserved_future",
            false,
            reserved_follow_on_blockers,
        ),
        "note": "Transactional statuses честно разделяют уже materialized measured/report-only стадии и будущие reserved money-facing стадии. Reserved не означает включённый billing workflow."
    })
}

pub(super) fn provisional_close_barriers(
    summary: &Value,
    metering_freshness: &Value,
    adjustment_preview: &Value,
) -> Vec<String> {
    let mut barriers = Vec::new();
    if !matches!(
        summary["coverage"]["completeness_state"].as_str(),
        Some("confirmed" | "fully_confirmed")
    ) {
        barriers.push("coverage_not_final".to_string());
    }
    if metering_freshness["contractual_lag_state"].as_str() == Some("awaiting_late_events") {
        barriers.push("late_arrival_window_open".to_string());
    }
    if metering_freshness["metering_ingest_state"].as_str() == Some("lagging") {
        barriers.push("metering_pipeline_lagging".to_string());
    }
    if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        barriers.push("pending_adjustment_review".to_string());
    }
    if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        barriers.push("dispute_hold_open".to_string());
    }
    barriers
}

pub(super) fn freeze_status(
    events: &[TokenBudgetEvent],
    metering_freshness: &Value,
    provisional_close_candidate: bool,
) -> &'static str {
    if events.is_empty() {
        "empty"
    } else if metering_freshness["metering_ingest_state"].as_str() == Some("lagging") {
        "pipeline_lag_open"
    } else if metering_freshness["contractual_lag_state"].as_str() == Some("awaiting_late_events") {
        "late_arrival_window_open"
    } else if provisional_close_candidate {
        "provisionally_frozen_report_only"
    } else {
        "open_review_hold_report_only"
    }
}

pub(super) fn reason_strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

pub(super) fn merged_reason_strings(values: &[&Value]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut merged = Vec::new();
    for value in values {
        for reason in reason_strings(value) {
            if seen.insert(reason.clone()) {
                merged.push(reason);
            }
        }
    }
    merged
}

pub(super) fn push_unique_reason(reasons: &mut Vec<String>, reason: &str) {
    if !reasons.iter().any(|existing| existing == reason) {
        reasons.push(reason.to_string());
    }
}

pub(super) fn internal_money_arithmetic_readiness(
    reconciliation_preview: &Value,
    margin_scope: &Value,
) -> (&'static str, Vec<String>) {
    let mut blockers = merged_reason_strings(&[
        &reconciliation_preview["governance_blocking_reasons"],
        &margin_scope["blocking_reasons"],
    ]);
    blockers.retain(|reason| reason != "provider_invoice_source_error");

    if reconciliation_preview["provider_identity_state"]
        .as_str()
        .unwrap_or("provider_identity_aligned")
        == "provider_identity_mismatch"
        || margin_scope["provider_identity_state"]
            .as_str()
            .unwrap_or("provider_identity_aligned")
            == "provider_identity_mismatch"
    {
        push_unique_reason(&mut blockers, "provider_identity_mismatch");
        return ("provider_identity_mismatch", blockers);
    }

    if reconciliation_preview["temporal_truth_state"]
        .as_str()
        .unwrap_or("scope_period_aligned")
        != "scope_period_aligned"
    {
        push_unique_reason(&mut blockers, "reconciliation_scope_period_not_aligned");
        return ("reconciliation_scope_period_misaligned", blockers);
    }

    if margin_scope["temporal_truth_state"]
        .as_str()
        .unwrap_or("scope_period_aligned")
        != "scope_period_aligned"
    {
        push_unique_reason(&mut blockers, "margin_scope_period_not_aligned");
        return ("margin_scope_period_misaligned", blockers);
    }

    match reconciliation_preview["usage_truth_completeness_state"].as_str() {
        Some("provider_usage_bound") => {}
        Some("provider_usage_source_error") => {
            push_unique_reason(&mut blockers, "usage_truth_source_error");
            return ("usage_truth_source_error", blockers);
        }
        Some("provider_usage_not_yet_bound") => {
            push_unique_reason(&mut blockers, "usage_truth_not_yet_bound");
            return ("usage_truth_not_yet_bound", blockers);
        }
        _ => {
            push_unique_reason(&mut blockers, "usage_truth_not_ready");
            return ("awaiting_usage_truth", blockers);
        }
    }

    match reconciliation_preview["provider_cost_truth_completeness_state"].as_str() {
        Some("provider_cost_bound") => {}
        Some("provider_rate_card_error") => {
            push_unique_reason(&mut blockers, "provider_cost_truth_source_error");
            return ("provider_cost_truth_source_error", blockers);
        }
        Some("rate_card_bound_unpriced") => {
            push_unique_reason(&mut blockers, "provider_cost_truth_unpriced");
            return ("provider_cost_truth_unpriced", blockers);
        }
        Some("rate_card_bound_internal_estimate_only") => {
            push_unique_reason(&mut blockers, "provider_cost_truth_internal_estimate_only");
            return ("provider_cost_truth_internal_estimate_only", blockers);
        }
        _ => {
            push_unique_reason(&mut blockers, "provider_cost_truth_not_ready");
            return ("awaiting_provider_cost_truth", blockers);
        }
    }

    if margin_scope["pricing_truth_completeness_state"].as_str() != Some("pricing_truth_ready") {
        push_unique_reason(&mut blockers, "pricing_truth_not_ready");
        return ("awaiting_pricing_truth", blockers);
    }

    match margin_scope["margin_truth_completeness_state"].as_str() {
        Some("margin_preview_amounts_ready_report_only") => {
            ("money_arithmetic_preview_ready_report_only", blockers)
        }
        Some("margin_truth_source_error") => {
            push_unique_reason(&mut blockers, "margin_truth_source_error");
            ("margin_truth_source_error", blockers)
        }
        Some("margin_truth_bound_unpriced") => {
            push_unique_reason(&mut blockers, "margin_truth_unpriced");
            ("margin_truth_unpriced", blockers)
        }
        _ => {
            push_unique_reason(&mut blockers, "margin_truth_not_ready");
            ("awaiting_margin_truth", blockers)
        }
    }
}

pub(super) fn contractual_settlement_readiness(
    statement_preview: &Value,
    metering_freshness: &Value,
    internal_money_arithmetic_state: &str,
) -> (&'static str, Vec<String>) {
    let measured_events = statement_preview["coverage"]["measured_events"]
        .as_u64()
        .unwrap_or(0);
    let mut blockers = merged_reason_strings(&[
        &statement_preview["close_barriers"],
        &statement_preview["next_settlement_stage_blockers"],
        &metering_freshness["blocking_reasons"],
    ]);

    if internal_money_arithmetic_state != "money_arithmetic_preview_ready_report_only" {
        push_unique_reason(&mut blockers, "money_arithmetic_not_ready");
    }

    if measured_events == 0 {
        push_unique_reason(&mut blockers, "no_measured_usage_events");
        return ("empty", blockers);
    }

    let settlement_stage = statement_preview["settlement_stage"]
        .as_str()
        .unwrap_or("unknown");
    let next_stage_candidate = statement_preview["next_settlement_stage_candidate"]
        .as_str()
        .unwrap_or("unknown");

    if settlement_stage == "measured_review_ready_report_only" {
        if blockers.is_empty() && next_stage_candidate == "billable_ready" {
            ("settlement_ready_reserved", blockers)
        } else {
            (
                "customer_review_ready_settlement_activation_blocked_report_only",
                blockers,
            )
        }
    } else {
        ("review_not_yet_ready_report_only", blockers)
    }
}

pub(super) fn review_surface_state(
    statement_export_preview: &Value,
) -> (&'static str, Vec<String>) {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let measured_events = statement_preview["coverage"]["measured_events"]
        .as_u64()
        .or_else(|| statement_export_preview["included_events_count"].as_u64())
        .unwrap_or(0);
    let settlement_stage = statement_preview["settlement_stage"]
        .as_str()
        .unwrap_or("unknown");
    let mut blockers =
        merged_reason_strings(&[&statement_preview["next_settlement_stage_blockers"]]);

    if measured_events == 0 {
        push_unique_reason(&mut blockers, "no_measured_usage_events");
        return ("empty_report_only", blockers);
    }

    if settlement_stage == "measured_review_ready_report_only" {
        return ("customer_review_ready_report_only", blockers);
    }

    if statement_preview["provisional_close_candidate"].as_bool() == Some(true) {
        ("provisionally_stable_report_only", blockers)
    } else {
        ("provisional_report_only", blockers)
    }
}

pub(super) fn future_settlement_activation_state(
    contractual_settlement_state: &str,
) -> &'static str {
    match contractual_settlement_state {
        "settlement_ready_reserved" => "future_settlement_ready_reserved",
        "customer_review_ready_settlement_activation_blocked_report_only" => {
            "future_settlement_activation_blocked_report_only"
        }
        "review_not_yet_ready_report_only" => "review_not_yet_ready_for_future_settlement",
        "empty" => "empty_scope_report_only",
        _ => "future_settlement_state_unknown",
    }
}

pub(super) fn build_customer_contractual_boundary_from_export(
    contract: &TokenBudgetContractConfig,
    surface_kind: &str,
    statement_export_preview: &Value,
) -> Value {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let (review_surface_state, review_surface_blocking_reasons) =
        review_surface_state(statement_export_preview);
    let contractual_settlement_state = statement_export_preview
        .get("contractual_settlement_readiness_state")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    json!({
        "model_version": contract.customer_contractual_boundary_version.clone(),
        "surface_kind": surface_kind,
        "report_only": true,
        "self_serve_state": "self_serve_ready_report_only",
        "invoice_grade": false,
        "operational_telemetry_included": false,
        "review_surface_state": review_surface_state,
        "review_surface_blocking_reasons": review_surface_blocking_reasons,
        "future_settlement_activation_state": future_settlement_activation_state(contractual_settlement_state),
        "future_settlement_activation_blocking_reasons": statement_export_preview["contractual_settlement_blocking_reasons"].clone(),
        "settlement_stage": statement_preview["settlement_stage"].clone(),
        "settlement_stage_family": statement_preview["settlement_stage_family"].clone(),
        "next_settlement_stage_candidate": statement_preview["next_settlement_stage_candidate"].clone(),
        "contractual_readiness_model_version": statement_export_preview["contractual_readiness_model_version"].clone(),
        "contractual_settlement_readiness_state": statement_export_preview["contractual_settlement_readiness_state"].clone(),
        "note": "Этот boundary отделяет текущую review-ready report-only поверхность от более строгой будущей settlement activation semantics."
    })
}

pub(super) fn settlement_activation_governance_state(
    statement_export_preview: &Value,
) -> &'static str {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let adjustment_preview = &statement_preview["adjustment_preview"];

    if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "dispute_hold_open_report_only"
    } else if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "pending_adjustment_review_report_only"
    } else if adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "adjusted_report_only"
    } else if statement_export_preview["customer_contractual_boundary"]
        ["future_settlement_activation_state"]
        .as_str()
        == Some("future_settlement_ready_reserved")
    {
        "future_settlement_ready_reserved"
    } else {
        "activation_blocked_report_only"
    }
}

pub(super) fn build_settlement_activation_governance_from_export(
    contract: &TokenBudgetContractConfig,
    statement_export_preview: &Value,
) -> Value {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let adjustment_preview = &statement_preview["adjustment_preview"];
    let registry_status = adjustment_preview["registry_status"]
        .as_str()
        .or_else(|| adjustment_preview["status"].as_str())
        .unwrap_or("unknown");
    let adjustment_status = adjustment_preview["status"]
        .as_str()
        .or_else(|| adjustment_preview["registry_status"].as_str())
        .unwrap_or("unknown");

    json!({
        "model_version": contract.settlement_activation_governance_version.clone(),
        "governance_state": settlement_activation_governance_state(statement_export_preview),
        "future_settlement_activation_state": statement_export_preview["customer_contractual_boundary"]["future_settlement_activation_state"].clone(),
        "future_settlement_activation_blocking_reasons": statement_export_preview["customer_contractual_boundary"]["future_settlement_activation_blocking_reasons"].clone(),
        "next_settlement_stage_candidate": statement_preview["next_settlement_stage_candidate"].clone(),
        "next_settlement_stage_blockers": statement_preview["next_settlement_stage_blockers"].clone(),
        "provisional_close_state": statement_preview["provisional_close_state"].clone(),
        "provisional_close_candidate": statement_preview["provisional_close_candidate"].clone(),
        "provisional_close_barriers": statement_preview["provisional_close_barriers"].clone(),
        "billing_close_barriers": statement_preview["billing_close_barriers"].clone(),
        "close_barriers": statement_preview["close_barriers"].clone(),
        "registry_status": registry_status,
        "adjustment_status": adjustment_status,
        "correction_action_state": adjustment_preview["correction_action_state"].clone(),
        "credit_action_state": statement_export_preview["credit_action_state"].clone(),
        "dispute_action_state": statement_export_preview["dispute_action_state"].clone(),
        "pending_entries_count": adjustment_preview["pending_entries_count"].as_u64().unwrap_or(0),
        "applied_entries_count": adjustment_preview["applied_entries_count"].as_u64().unwrap_or(0),
        "disputed_entries_count": adjustment_preview["disputed_entries_count"].as_u64().unwrap_or(0),
        "allowed_future_actions": adjustment_preview["allowed_future_actions"].clone(),
        "note": "Этот governance-слой отдельно объясняет, какие barriers и adjustment semantics сейчас держат будущую settlement activation в report-only режиме."
    })
}

pub(super) fn adjustment_activation_governance_state(
    statement_export_preview: &Value,
) -> &'static str {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let adjustment_preview = &statement_preview["adjustment_preview"];
    let adjustment_status = adjustment_preview["status"].as_str().unwrap_or("unknown");

    if matches!(adjustment_status, "not_configured" | "default_path_missing") {
        "registry_not_configured_report_only"
    } else if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "dispute_hold_open_report_only"
    } else if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "pending_adjustment_review_report_only"
    } else if adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "adjusted_report_only"
    } else {
        "future_adjustment_ready_reserved"
    }
}

pub(super) fn future_adjustment_activation_state(statement_export_preview: &Value) -> &'static str {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let adjustment_preview = &statement_preview["adjustment_preview"];
    let adjustment_status = adjustment_preview["status"].as_str().unwrap_or("unknown");

    if matches!(adjustment_status, "not_configured" | "default_path_missing") {
        "future_adjustment_registry_not_bound"
    } else if adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "future_adjustment_blocked_by_dispute"
    } else if adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "future_adjustment_blocked_by_review"
    } else if adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0)
        > 0
    {
        "future_adjustment_materialized_report_only"
    } else {
        "future_adjustment_ready_reserved"
    }
}

pub(super) fn future_adjustment_activation_blocking_reasons(
    statement_export_preview: &Value,
) -> Vec<String> {
    match future_adjustment_activation_state(statement_export_preview) {
        "future_adjustment_registry_not_bound" => vec!["adjustment_registry_not_bound".to_string()],
        "future_adjustment_blocked_by_dispute" => vec!["dispute_hold_open".to_string()],
        "future_adjustment_blocked_by_review" => vec!["pending_adjustment_review".to_string()],
        _ => Vec::new(),
    }
}

pub(super) fn build_adjustment_activation_governance_from_export(
    contract: &TokenBudgetContractConfig,
    statement_export_preview: &Value,
) -> Value {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let adjustment_preview = &statement_preview["adjustment_preview"];
    let registry_status = adjustment_preview["registry_status"]
        .as_str()
        .or_else(|| adjustment_preview["status"].as_str())
        .unwrap_or("unknown");
    let adjustment_status = adjustment_preview["status"]
        .as_str()
        .or_else(|| adjustment_preview["registry_status"].as_str())
        .unwrap_or("unknown");

    json!({
        "model_version": contract.adjustment_activation_governance_version.clone(),
        "governance_state": adjustment_activation_governance_state(statement_export_preview),
        "future_adjustment_activation_state": future_adjustment_activation_state(statement_export_preview),
        "future_adjustment_activation_blocking_reasons": future_adjustment_activation_blocking_reasons(statement_export_preview),
        "registry_status": registry_status,
        "adjustment_status": adjustment_status,
        "request_schema_version": adjustment_preview["request_schema_version"].clone(),
        "registry_version": adjustment_preview["registry_version"].clone(),
        "correction_action_state": adjustment_preview["correction_action_state"].clone(),
        "credit_action_state": statement_export_preview["credit_action_state"].clone(),
        "dispute_action_state": statement_export_preview["dispute_action_state"].clone(),
        "pending_entries_count": adjustment_preview["pending_entries_count"].as_u64().unwrap_or(0),
        "applied_entries_count": adjustment_preview["applied_entries_count"].as_u64().unwrap_or(0),
        "disputed_entries_count": adjustment_preview["disputed_entries_count"].as_u64().unwrap_or(0),
        "allowed_future_actions": adjustment_preview["allowed_future_actions"].clone(),
        "note": "Этот governance-слой отдельно объясняет, готов ли future adjustment path, чем он сейчас заблокирован и где report-only layer уже materialized pending/applied/disputed semantics."
    })
}

pub(super) fn settlement_report_preview_from_export(
    contract: &TokenBudgetContractConfig,
    statement_export_preview: &Value,
) -> Value {
    let mut settlement_report_preview =
        statement_export_preview["settlement_report_preview"].clone();
    if settlement_report_preview["customer_contractual_boundary"].is_null() {
        settlement_report_preview["customer_contractual_boundary"] =
            build_customer_contractual_boundary_from_export(
                contract,
                "customer_settlement_report_preview_report_only",
                statement_export_preview,
            );
    }
    if settlement_report_preview["adjustment_activation_governance"].is_null() {
        settlement_report_preview["adjustment_activation_governance"] =
            build_adjustment_activation_governance_from_export(contract, statement_export_preview);
    }
    settlement_report_preview
}

pub(super) fn build_scope_suitability(
    contract: &TokenBudgetContractConfig,
    statement_preview: &Value,
    reconciliation_preview: &Value,
    margin_scope: &Value,
    metering_freshness: &Value,
) -> Value {
    if statement_preview.is_null()
        || reconciliation_preview.is_null()
        || margin_scope.is_null()
        || metering_freshness.is_null()
    {
        return Value::Null;
    }

    let measured_events = statement_preview["coverage"]["measured_events"]
        .as_u64()
        .unwrap_or(0);
    let confirmed_events = statement_preview["coverage"]["included_events"]
        .as_u64()
        .unwrap_or(0);
    let coverage_state = statement_preview["coverage"]["completeness_state"]
        .as_str()
        .unwrap_or("empty");
    let stable = metering_freshness["can_treat_scope_as_stable"].as_bool() == Some(true);
    let provisional_close_candidate =
        statement_preview["provisional_close_candidate"].as_bool() == Some(true);
    let provisional_close_barriers = &statement_preview["provisional_close_barriers"];
    let billing_close_barriers = &statement_preview["billing_close_barriers"];
    let governance_blocking_reasons = &reconciliation_preview["governance_blocking_reasons"];
    let review_reasons = merged_reason_strings(&[
        provisional_close_barriers,
        &metering_freshness["blocking_reasons"],
    ]);
    let billing_reasons = merged_reason_strings(&[
        billing_close_barriers,
        governance_blocking_reasons,
        &margin_scope["blocking_reasons"],
    ]);
    let compensation_reasons = {
        let mut reasons = billing_reasons.clone();
        if reconciliation_preview["money_truth_completeness_state"].as_str()
            != Some("provider_cost_and_invoice_bound")
            && !reasons.iter().any(|value| value == "money_truth_not_final")
        {
            reasons.push("money_truth_not_final".to_string());
        }
        if statement_preview["final_amount"].is_null()
            && !reasons
                .iter()
                .any(|value| value == "final_amount_unavailable")
        {
            reasons.push("final_amount_unavailable".to_string());
        }
        reasons
    };

    let operational_live = if measured_events == 0 {
        json!({
            "usable": false,
            "state": "empty",
            "blocking_reasons": ["no_measured_usage_events"]
        })
    } else {
        json!({
            "usable": true,
            "state": "live_operational",
            "blocking_reasons": []
        })
    };

    let product_kpi = if confirmed_events == 0 {
        json!({
            "usable": false,
            "state": "awaiting_confirmed_usage",
            "blocking_reasons": if coverage_state == "empty" {
                json!(["no_measured_usage_events"])
            } else {
                json!(["no_confirmed_usage"])
            }
        })
    } else if stable && provisional_close_candidate {
        json!({
            "usable": true,
            "state": "provisionally_stable_lower_bound_with_coverage",
            "blocking_reasons": review_reasons
        })
    } else {
        json!({
            "usable": true,
            "state": "provisional_lower_bound_with_coverage",
            "blocking_reasons": review_reasons
        })
    };

    let customer_review = if measured_events == 0 {
        json!({
            "usable": false,
            "state": "empty",
            "blocking_reasons": ["no_measured_usage_events"]
        })
    } else if stable && provisional_close_candidate {
        json!({
            "usable": true,
            "state": "review_ready_report_only_provisionally_stable",
            "blocking_reasons": review_reasons
        })
    } else {
        json!({
            "usable": true,
            "state": "review_ready_report_only_provisional",
            "blocking_reasons": review_reasons
        })
    };

    let contractual_export = if measured_events == 0 {
        json!({
            "usable": false,
            "state": "empty",
            "blocking_reasons": ["no_measured_usage_events"]
        })
    } else if stable && provisional_close_candidate {
        json!({
            "usable": true,
            "state": "export_ready_report_only_provisionally_stable",
            "blocking_reasons": review_reasons
        })
    } else {
        json!({
            "usable": true,
            "state": "export_ready_report_only_provisional",
            "blocking_reasons": review_reasons
        })
    };

    let billing_amount = if statement_preview["billable_lower_bound_tokens"].is_null() {
        json!({
            "usable": false,
            "state": "not_billable_report_only",
            "blocking_reasons": billing_reasons
        })
    } else {
        json!({
            "usable": true,
            "state": "billable_ready",
            "blocking_reasons": billing_reasons
        })
    };

    let compensation_pricing = if statement_preview["billable_lower_bound_tokens"].is_null()
        || statement_preview["final_amount"].is_null()
        || reconciliation_preview["money_truth_completeness_state"].as_str()
            != Some("provider_cost_and_invoice_bound")
    {
        json!({
            "usable": false,
            "state": "not_compensation_ready",
            "blocking_reasons": compensation_reasons
        })
    } else {
        json!({
            "usable": true,
            "state": "compensation_ready",
            "blocking_reasons": compensation_reasons
        })
    };

    json!({
        "model_version": contract.suitability_model_version.clone(),
        "surfaces": {
            "operational_live": operational_live,
            "product_kpi": product_kpi,
            "customer_review": customer_review,
            "contractual_export": contractual_export,
            "billing_amount": billing_amount,
            "compensation_pricing": compensation_pricing,
        },
        "truth_guardrail": {
            "retrieval_savings_floor": "real",
            "partial_whole_agent_cycle_lower_bound": "real",
            "full_session_economics": "not_fully_measured"
        },
        "note": "Suitability не маскирует отрицательную или положительную экономию. Она только фиксирует, где этот scope можно использовать без подмены смысла."
    })
}

pub(super) fn build_contractual_statement_summary(
    contract: &TokenBudgetContractConfig,
    scope_code: &str,
    scope_label: &str,
    statement_preview: &Value,
    reconciliation_preview: &Value,
    margin_scope: &Value,
    metering_freshness: &Value,
) -> Value {
    if statement_preview.is_null()
        || reconciliation_preview.is_null()
        || margin_scope.is_null()
        || metering_freshness.is_null()
    {
        return Value::Null;
    }
    let rate_card_binding =
        &reconciliation_preview["external_truth_bindings"]["provider_rate_card"];
    let provider_usage_binding =
        &reconciliation_preview["external_truth_bindings"]["provider_usage_export"];
    let provider_invoice_binding =
        &reconciliation_preview["external_truth_bindings"]["provider_invoice_export"];
    let settlement_stage = statement_preview["settlement_stage"]
        .as_str()
        .unwrap_or("unknown");
    let suitability = build_scope_suitability(
        contract,
        statement_preview,
        reconciliation_preview,
        margin_scope,
        metering_freshness,
    );
    let (internal_money_arithmetic_state, internal_money_arithmetic_blockers) =
        internal_money_arithmetic_readiness(reconciliation_preview, margin_scope);
    let (contractual_settlement_state, contractual_settlement_blockers) =
        contractual_settlement_readiness(
            statement_preview,
            metering_freshness,
            internal_money_arithmetic_state,
        );
    let mut summary = serde_json::Map::new();
    let mut insert = |key: &str, value: Value| {
        summary.insert(key.to_string(), value);
    };

    insert("scope_code", json!(scope_code));
    insert("scope_label", json!(scope_label));
    insert(
        "contractual_state",
        statement_preview["contractual_state"].clone(),
    );
    insert(
        "settlement_stage",
        statement_preview["settlement_stage"].clone(),
    );
    insert(
        "settlement_stage_family",
        statement_preview["settlement_stage_family"].clone(),
    );
    insert(
        "next_settlement_stage_candidate",
        statement_preview["next_settlement_stage_candidate"].clone(),
    );
    insert(
        "next_settlement_stage_blockers",
        statement_preview["next_settlement_stage_blockers"].clone(),
    );
    insert(
        "future_reserved_settlement_stages",
        statement_preview["future_reserved_settlement_stages"].clone(),
    );
    insert(
        "contractual_readiness_model_version",
        json!(contract.contractual_readiness_model_version.clone()),
    );
    insert(
        "transactional_statuses",
        statement_preview["transactional_statuses"].clone(),
    );
    insert(
        "coverage_state",
        statement_preview["coverage"]["completeness_state"].clone(),
    );
    insert(
        "provisional_close_state",
        statement_preview["provisional_close_state"].clone(),
    );
    insert(
        "provisional_close_candidate",
        statement_preview["provisional_close_candidate"].clone(),
    );
    insert(
        "provisional_close_barriers",
        statement_preview["provisional_close_barriers"].clone(),
    );
    insert(
        "billing_close_barriers",
        statement_preview["billing_close_barriers"].clone(),
    );
    insert(
        "usage_truth_completeness_state",
        reconciliation_preview["usage_truth_completeness_state"].clone(),
    );
    insert(
        "rate_card_truth_completeness_state",
        reconciliation_preview["rate_card_truth_completeness_state"].clone(),
    );
    insert(
        "provider_cost_truth_completeness_state",
        reconciliation_preview["provider_cost_truth_completeness_state"].clone(),
    );
    insert(
        "invoice_evidence_completeness_state",
        reconciliation_preview["invoice_evidence_completeness_state"].clone(),
    );
    insert(
        "money_truth_completeness_state",
        reconciliation_preview["money_truth_completeness_state"].clone(),
    );
    insert(
        "reconciliation_readiness_state",
        reconciliation_preview["reconciliation_readiness_state"].clone(),
    );
    insert(
        "required_sources_for_usage_truth",
        reconciliation_preview["required_sources_for_usage_truth"].clone(),
    );
    insert(
        "required_sources_for_cost_truth",
        reconciliation_preview["required_sources_for_cost_truth"].clone(),
    );
    insert(
        "optional_sources_for_invoice_evidence",
        reconciliation_preview["optional_sources_for_invoice_evidence"].clone(),
    );
    insert(
        "unready_required_sources_for_usage_truth",
        reconciliation_preview["unready_required_sources_for_usage_truth"].clone(),
    );
    insert(
        "unready_required_sources_for_cost_truth",
        reconciliation_preview["unready_required_sources_for_cost_truth"].clone(),
    );
    insert(
        "unready_optional_sources_for_invoice_evidence",
        reconciliation_preview["unready_optional_sources_for_invoice_evidence"].clone(),
    );
    insert(
        "reconciliation_governance_blocking_reasons",
        reconciliation_preview["governance_blocking_reasons"].clone(),
    );
    insert("rate_card_status", rate_card_binding["status"].clone());
    insert(
        "rate_card_version",
        rate_card_binding["bound_rate_card_version"].clone(),
    );
    insert("rate_card_provider", rate_card_binding["provider"].clone());
    insert(
        "rate_card_currency_profile",
        rate_card_binding["bound_currency_profile"].clone(),
    );
    insert(
        "provider_usage_provider",
        provider_usage_binding["provider"].clone(),
    );
    insert(
        "provider_invoice_provider",
        provider_invoice_binding["provider"].clone(),
    );
    insert(
        "provider_usage_scope_alignment_state",
        reconciliation_preview["provider_usage_scope_alignment_state"].clone(),
    );
    insert(
        "provider_invoice_scope_alignment_state",
        reconciliation_preview["provider_invoice_scope_alignment_state"].clone(),
    );
    insert(
        "rate_card_scope_alignment_state",
        reconciliation_preview["rate_card_scope_alignment_state"].clone(),
    );
    insert(
        "rate_card_provider_alignment_state",
        reconciliation_preview["rate_card_provider_alignment_state"].clone(),
    );
    insert(
        "invoice_provider_alignment_state",
        reconciliation_preview["invoice_provider_alignment_state"].clone(),
    );
    insert(
        "provider_identity_state",
        reconciliation_preview["provider_identity_state"].clone(),
    );
    insert(
        "reconciliation_temporal_truth_state",
        reconciliation_preview["temporal_truth_state"].clone(),
    );
    insert(
        "metering_ingest_state",
        metering_freshness["metering_ingest_state"].clone(),
    );
    insert(
        "contractual_lag_state",
        metering_freshness["contractual_lag_state"].clone(),
    );
    insert(
        "contractual_freshness_state",
        metering_freshness["contractual_freshness_state"].clone(),
    );
    insert(
        "can_treat_scope_as_stable",
        metering_freshness["can_treat_scope_as_stable"].clone(),
    );
    insert(
        "latest_event_age_ms",
        metering_freshness["latest_event_age_ms"].clone(),
    );
    insert(
        "latest_ingest_lag_ms",
        metering_freshness["latest_ingest_lag_ms"].clone(),
    );
    insert(
        "p95_ingest_lag_ms",
        metering_freshness["p95_ingest_lag_ms"].clone(),
    );
    insert(
        "provisional_close_earliest_at_epoch_ms",
        statement_preview["period"]["provisional_close_earliest_at_epoch_ms"].clone(),
    );
    insert(
        "late_arrival_deadline_epoch_ms",
        statement_preview["period"]["late_arrival_deadline_epoch_ms"].clone(),
    );
    insert(
        "measured_non_billable_lower_bound_tokens",
        statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
    );
    insert(
        "adjusted_measured_non_billable_lower_bound_tokens",
        statement_preview["adjusted_measured_non_billable_lower_bound_tokens"].clone(),
    );
    insert(
        "billable_lower_bound_tokens",
        statement_preview["billable_lower_bound_tokens"].clone(),
    );
    insert(
        "internal_provider_billed_tokens",
        reconciliation_preview["internal_provider_billed_tokens"].clone(),
    );
    insert(
        "internal_observed_whole_cycle_lower_bound_tokens",
        reconciliation_preview["internal_observed_whole_cycle_lower_bound_tokens"].clone(),
    );
    insert(
        "verified_internal_observed_whole_cycle_lower_bound_tokens",
        reconciliation_preview["verified_internal_observed_whole_cycle_lower_bound_tokens"].clone(),
    );
    insert(
        "internal_provider_cost_estimate_amount",
        reconciliation_preview["internal_provider_cost_estimate_amount"].clone(),
    );
    insert(
        "external_provider_usage_tokens",
        reconciliation_preview["external_provider_usage_tokens"].clone(),
    );
    insert(
        "external_provider_cost_amount",
        reconciliation_preview["external_provider_cost_amount"].clone(),
    );
    insert(
        "external_invoice_amount",
        reconciliation_preview["external_invoice_amount"].clone(),
    );
    insert(
        "drift_tokens",
        reconciliation_preview["drift_tokens"].clone(),
    );
    insert(
        "drift_amount",
        reconciliation_preview["drift_amount"].clone(),
    );
    insert(
        "invoice_drift_amount",
        reconciliation_preview["invoice_drift_amount"].clone(),
    );
    insert(
        "reconciliation_state",
        reconciliation_preview["reconciliation_state"].clone(),
    );
    insert("margin_state", margin_scope["margin_state"].clone());
    insert(
        "margin_confidence_state",
        margin_scope["margin_confidence_state"].clone(),
    );
    insert(
        "margin_readiness_state",
        margin_scope["margin_readiness_state"].clone(),
    );
    insert(
        "infra_cost_truth_completeness_state",
        margin_scope["infra_cost_truth_completeness_state"].clone(),
    );
    insert(
        "pricing_truth_completeness_state",
        margin_scope["pricing_truth_completeness_state"].clone(),
    );
    insert(
        "customer_savings_money_truth_completeness_state",
        margin_scope["customer_savings_money_truth_completeness_state"].clone(),
    );
    insert(
        "amai_cost_truth_completeness_state",
        margin_scope["amai_cost_truth_completeness_state"].clone(),
    );
    insert(
        "margin_truth_completeness_state",
        margin_scope["margin_truth_completeness_state"].clone(),
    );
    insert(
        "required_sources_for_margin_truth",
        margin_scope["required_sources_for_margin_truth"].clone(),
    );
    insert(
        "optional_sources_for_margin_invoice_evidence",
        margin_scope["optional_sources_for_invoice_evidence"].clone(),
    );
    insert(
        "unready_required_sources_for_margin_truth",
        margin_scope["unready_required_sources_for_margin_truth"].clone(),
    );
    insert(
        "margin_provider_identity_state",
        margin_scope["provider_identity_state"].clone(),
    );
    insert(
        "margin_temporal_truth_state",
        margin_scope["temporal_truth_state"].clone(),
    );
    insert(
        "infra_cost_scope_alignment_state",
        margin_scope["infra_cost_scope_alignment_state"].clone(),
    );
    insert(
        "margin_blocking_reasons",
        margin_scope["blocking_reasons"].clone(),
    );
    insert(
        "internal_money_arithmetic_readiness_state",
        json!(internal_money_arithmetic_state),
    );
    insert(
        "internal_money_arithmetic_blocking_reasons",
        json!(internal_money_arithmetic_blockers),
    );
    insert(
        "contractual_settlement_readiness_state",
        json!(contractual_settlement_state),
    );
    insert(
        "contractual_settlement_blocking_reasons",
        json!(contractual_settlement_blockers),
    );
    insert(
        "adjustment_state",
        statement_preview["adjustment_preview"]["correction_action_state"].clone(),
    );
    insert(
        "pending_adjustment_entries_count",
        statement_preview["adjustment_preview"]["pending_entries_count"].clone(),
    );
    insert(
        "applied_adjustment_entries_count",
        statement_preview["adjustment_preview"]["applied_entries_count"].clone(),
    );
    insert(
        "disputed_adjustment_entries_count",
        statement_preview["adjustment_preview"]["disputed_entries_count"].clone(),
    );
    insert(
        "close_barriers",
        statement_preview["close_barriers"].clone(),
    );
    insert(
        "blocking_reasons",
        combine_reason_arrays(&[
            &statement_preview["close_barriers"],
            &statement_preview["next_settlement_stage_blockers"],
            &reconciliation_preview["blocking_reasons"],
            &margin_scope["blocking_reasons"],
            &metering_freshness["blocking_reasons"],
        ]),
    );
    insert("suitability", suitability);
    insert("customer_review_ready", json!(true));
    insert("invoice_ready", json!(false));
    insert(
        "currency_profile",
        statement_preview["currency_profile"].clone(),
    );
    let client_limit_boundary_semantics =
        build_client_limit_boundary_review_surface(statement_preview);
    insert(
        "client_limit_boundary_semantics",
        client_limit_boundary_semantics.clone(),
    );
    insert(
        "reviewed_frozen_debt_export_surface",
        build_reviewed_frozen_debt_export_surface(
            contract,
            &client_limit_boundary_semantics,
            Some(scope_code),
        ),
    );
    insert(
        "note",
        json!(if settlement_stage == "measured_review_ready_report_only" {
            "Это короткий customer-facing summary поверх statement/reconciliation/margin/freshness previews. Он уже review-ready, но всё ещё остаётся report-only и не является invoice."
        } else {
            "Это короткий customer-facing summary поверх statement/reconciliation/margin/freshness previews. Он пригоден для review и audit, но не для invoice."
        }),
    );

    Value::Object(summary)
}

pub(super) fn build_statement_preview(
    scope_code: &str,
    scope_label: &str,
    now_epoch_ms: i64,
    events: &[TokenBudgetEvent],
    profile: &ResolvedProfile,
    summary: &Value,
    contract: &TokenBudgetContractConfig,
    adjustment_registry: &Value,
    rate_card: &Value,
    reconciliation_contract: &Value,
    metering_freshness: &Value,
    rollout_observations: &[codex_threads::RolloutAssistantGenerationObservation],
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let adjustment_preview =
        build_adjustment_preview_json(scope_code, contract, adjustment_registry);
    let provisional_close_barriers =
        provisional_close_barriers(summary, metering_freshness, &adjustment_preview);
    let provisional_close_candidate = provisional_close_barriers.is_empty();
    let mut billing_close_barriers = vec!["billing_mode_report_only".to_string()];
    if reconciliation_contract["ready_for_external_reconciliation"].as_bool() != Some(true) {
        billing_close_barriers.push("external_reconciliation_not_bound".to_string());
    }
    if rate_card["money_conversion_enabled"].as_bool() != Some(true) {
        billing_close_barriers.push("rate_card_unpriced".to_string());
    }
    if summary["verified_effective_saved_tokens"]
        .as_i64()
        .unwrap_or(0)
        <= 0
    {
        billing_close_barriers.push("no_positive_verified_lower_bound".to_string());
    }
    let mut close_barriers = billing_close_barriers.clone();
    for barrier in &provisional_close_barriers {
        if !close_barriers.contains(barrier) {
            close_barriers.push(barrier.clone());
        }
    }
    let measured_non_billable_lower_bound_tokens = summary["verified_effective_saved_tokens"]
        .as_i64()
        .unwrap_or(0);
    let applied_tokens_delta = adjustment_preview["applied_tokens_delta"]
        .as_i64()
        .unwrap_or(0);
    let lifecycle_state = statement_lifecycle_state(&adjustment_preview);
    let measured_events = events.len();
    let settlement_stage = settlement_stage(
        measured_events,
        &adjustment_preview,
        metering_freshness,
        provisional_close_candidate,
    );
    let provisional_close_state = if provisional_close_candidate {
        "report_only_preview_provisionally_stable"
    } else {
        "report_only_preview_provisional_hold"
    };
    let next_stage_candidate = next_settlement_stage_candidate(
        measured_events,
        metering_freshness,
        provisional_close_candidate,
        &billing_close_barriers,
    );
    let next_stage_blockers = next_settlement_stage_blockers(
        measured_events,
        &provisional_close_barriers,
        &billing_close_barriers,
    );
    let transactional_statuses = build_transactional_statuses(
        contract,
        measured_events,
        settlement_stage,
        next_stage_candidate,
        &next_stage_blockers,
        &billing_close_barriers,
        &adjustment_preview,
    );
    let internal_delivered_tokens = summary["delivered_tokens"].as_u64().unwrap_or(0);
    let internal_recovery_tokens = summary["recovery_tokens"].as_u64().unwrap_or(0);
    let internal_observed_whole_cycle_lower_bound_tokens =
        observed_whole_cycle_with_assistant_scope_tokens(summary, assistant_scope)
            .unwrap_or(internal_delivered_tokens.saturating_add(internal_recovery_tokens));
    let verified_internal_observed_whole_cycle_lower_bound_tokens =
        verified_observed_whole_cycle_with_assistant_scope_tokens(summary, assistant_scope)
            .unwrap_or(
                summary["verified_delivered_tokens"]
                    .as_u64()
                    .unwrap_or(0)
                    .saturating_add(summary["verified_recovery_tokens"].as_u64().unwrap_or(0)),
            );
    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "statement_status": "report_only_preview",
        "preliminary": summary["preliminary"].clone(),
        "counted_events": summary["counted_events"].clone(),
        "events_total": summary["events_total"].clone(),
        "lifecycle_state": lifecycle_state,
        "settlement_stage": settlement_stage,
        "settlement_stage_family": settlement_stage_family(settlement_stage),
        "next_settlement_stage_candidate": next_stage_candidate,
        "next_settlement_stage_blockers": next_stage_blockers,
        "future_reserved_settlement_stages": future_reserved_settlement_stages(),
        "transactional_statuses": transactional_statuses,
        "operational_state": "live_measurement_open",
        "contractual_state": match lifecycle_state {
            "measured_non_billable_dispute_hold" => "report_only_preview_dispute_hold",
            "measured_non_billable_pending_adjustment" => "report_only_preview_pending_adjustment",
            "measured_non_billable_adjusted_report_only" => "report_only_preview_adjusted",
            _ => "report_only_preview_open",
        },
        "close_readiness": if provisional_close_candidate {
            "provisionally_stable_report_only"
        } else {
            "provisionally_blocked_report_only"
        },
        "close_candidate": false,
        "provisional_close_state": provisional_close_state,
        "provisional_close_candidate": provisional_close_candidate,
        "provisional_close_barriers": provisional_close_barriers,
        "billing_close_barriers": billing_close_barriers,
        "close_barriers": close_barriers,
        "freeze_status": freeze_status(events, metering_freshness, provisional_close_candidate),
        "late_arrival_mode": "accepting_events_until_contractual_close_exists",
        "correction_mode": adjustment_preview["correction_action_state"].clone(),
        "dispute_mode": if adjustment_preview["disputed_entries_count"].as_u64().unwrap_or(0) > 0 {
            Value::String("open_dispute_hold_report_only".to_string())
        } else {
            Value::String("not_open_report_only".to_string())
        },
        "period": build_statement_period_json(
            scope_code,
            scope_label,
            now_epoch_ms,
            events,
            profile,
            contract,
            metering_freshness,
            provisional_close_candidate,
            &provisional_close_barriers
        ),
        "adjustment_preview": adjustment_preview.clone(),
        "coverage": summary["coverage"],
        "client_limit_meter_alignment": build_client_limit_meter_alignment(
            contract,
            "statement_preview",
            summary,
            Some(events),
            Some(rollout_observations),
            assistant_scope,
        ),
        "freshness": metering_freshness.clone(),
        "internal_delivered_tokens": internal_delivered_tokens,
        "internal_recovery_tokens": internal_recovery_tokens,
        "internal_observed_whole_cycle_lower_bound_tokens": internal_observed_whole_cycle_lower_bound_tokens,
        "verified_internal_observed_whole_cycle_lower_bound_tokens": verified_internal_observed_whole_cycle_lower_bound_tokens,
        "internal_provider_billed_tokens": internal_observed_whole_cycle_lower_bound_tokens,
        "measured_non_billable_lower_bound_tokens": measured_non_billable_lower_bound_tokens,
        "adjusted_measured_non_billable_lower_bound_tokens": measured_non_billable_lower_bound_tokens
            .saturating_add(applied_tokens_delta),
        "billable_lower_bound_tokens": Value::Null,
        "final_amount": Value::Null,
        "currency_profile": rate_card["bound_currency_profile"]
            .as_str()
            .unwrap_or(&contract.currency_profile)
            .to_string(),
        "settlement_status": contract.settlement_status.clone(),
        "note": if settlement_stage == "measured_review_ready_report_only" {
            "Это preview measured lower bound для scope. Он уже review-ready и provisionally stable, но по-прежнему не является billable statement или суммой к оплате."
        } else {
            "Это preview measured lower bound для scope, а не закрытый statement и не сумма к оплате."
        }
    })
}

pub(super) fn contractual_line_item_json(event: &TokenBudgetEvent) -> Value {
    json!({
        "event_id": event.event_id.clone(),
        "correlation_id": event.correlation_id.clone(),
        "occurred_at_epoch_ms": event.occurred_at_epoch_ms,
        "ingested_at_epoch_ms": event.ingested_at_epoch_ms,
        "project_code": event.project.clone(),
        "namespace_code": event.namespace.clone(),
        "source_kind": event.source_kind.clone(),
        "traffic_class": event.traffic_class.clone(),
        "measurement_scope": event.measurement_scope.clone(),
        "query_hash": event.query_hash.clone(),
        "query_type": event.query_type.clone(),
        "target_kind": event.target_kind.clone(),
        "baseline_strategy": event.baseline_strategy.clone(),
        "retrieval_mode": event.retrieval_mode.clone(),
        "baseline_tokens": event.naive_tokens,
        "delivered_tokens": event.context_tokens,
        "recovery_tokens": event.recovery_tokens,
        "whole_cycle_observed": {
            "client_prompt_tokens": event.client_prompt_tokens,
            "assistant_generation_tokens": event.assistant_generation_tokens,
            "tool_overhead_tokens": event.tool_overhead_tokens,
            "continuity_restore_tokens": event.continuity_restore_tokens,
        },
        "effective_saved_tokens": event.effective_saved_tokens,
        "quality_ok": event.quality_ok,
        "quality_method": event.quality_method.clone(),
        "quality_tier": event.quality_tier.clone(),
        "usage_state": {
            "lifecycle_status": usage_lifecycle_status(event),
            "reporting_layer": usage_reporting_layer(event),
            "excluded_reason_code": usage_excluded_reason_code(event),
        },
        "settlement_status": event.settlement_status.clone(),
    })
}

pub(super) fn build_contractual_line_item_sets(
    scope_events: &[TokenBudgetEvent],
) -> (Vec<Value>, Vec<Value>) {
    let included_items = scope_events
        .iter()
        .filter(|event| usage_excluded_reason_code(event).is_none())
        .map(contractual_line_item_json)
        .collect::<Vec<_>>();
    let excluded_items = scope_events
        .iter()
        .filter(|event| usage_excluded_reason_code(event).is_some())
        .map(contractual_line_item_json)
        .collect::<Vec<_>>();
    (included_items, excluded_items)
}

pub(super) fn hash_line_items(items: &[Value]) -> Result<String> {
    let bytes = serde_json::to_vec(items).context("failed to encode contractual line items")?;
    Ok(hex_sha256(&bytes))
}

pub(super) fn build_settlement_report_preview(
    contract: &TokenBudgetContractConfig,
    statement_export_preview: &Value,
) -> Value {
    let statement_preview = &statement_export_preview["line_item_surfaces"]["statement_preview"];
    let period = &statement_preview["period"];
    let adjustment_preview = &statement_preview["adjustment_preview"];
    let external_truth_manifest = &statement_export_preview["external_truth_manifest"];
    let reviewed_frozen_debt_export_surface =
        if statement_export_preview["reviewed_frozen_debt_export_surface"].is_null() {
            build_reviewed_frozen_debt_export_surface(
                contract,
                &statement_export_preview["client_limit_boundary_semantics"],
                statement_export_preview["scope_code"].as_str(),
            )
        } else {
            statement_export_preview["reviewed_frozen_debt_export_surface"].clone()
        };
    let settlement_report_identity = format!(
        "{}:{}:{}:{}:{}:{}:{}:{}",
        statement_export_preview["scope_code"]
            .as_str()
            .unwrap_or("unknown-scope"),
        contract.settlement_report_preview_version,
        statement_export_preview["statement_preview_id"]
            .as_str()
            .unwrap_or("missing-statement-id"),
        statement_export_preview["included_events_hash"]
            .as_str()
            .unwrap_or("missing-included-hash"),
        statement_export_preview["excluded_events_hash"]
            .as_str()
            .unwrap_or("missing-excluded-hash"),
        contract.billing_policy_version,
        contract.reconciliation_contract_version,
        external_truth_manifest["manifest_hash"]
            .as_str()
            .unwrap_or("missing-truth-manifest"),
    );
    json!({
        "model_version": contract.settlement_report_preview_version.clone(),
        "settlement_report_id": hex_sha256(settlement_report_identity.as_bytes()),
        "statement_preview_id": statement_export_preview["statement_preview_id"].clone(),
        "scope_code": statement_export_preview["scope_code"].clone(),
        "scope_label": statement_export_preview["scope_label"].clone(),
        "period_kind": period["period_kind"].clone(),
        "period_start_epoch_ms": period["period_start_epoch_ms"].clone(),
        "period_end_epoch_ms": period["period_end_epoch_ms"].clone(),
        "provisional_close_earliest_at_epoch_ms": statement_export_preview["provisional_close_earliest_at_epoch_ms"].clone(),
        "late_arrival_deadline_epoch_ms": period["late_arrival_deadline_epoch_ms"].clone(),
        "settlement_stage": statement_export_preview["settlement_stage"].clone(),
        "settlement_stage_family": statement_export_preview["settlement_stage_family"].clone(),
        "next_settlement_stage_candidate": statement_export_preview["next_settlement_stage_candidate"].clone(),
        "next_settlement_stage_blockers": statement_export_preview["next_settlement_stage_blockers"].clone(),
        "contractual_readiness_model_version": statement_export_preview["contractual_readiness_model_version"].clone(),
        "internal_money_arithmetic_readiness_state": statement_export_preview["internal_money_arithmetic_readiness_state"].clone(),
        "internal_money_arithmetic_blocking_reasons": statement_export_preview["internal_money_arithmetic_blocking_reasons"].clone(),
        "contractual_settlement_readiness_state": statement_export_preview["contractual_settlement_readiness_state"].clone(),
        "contractual_settlement_blocking_reasons": statement_export_preview["contractual_settlement_blocking_reasons"].clone(),
        "coverage_state": statement_export_preview["coverage_state"].clone(),
        "contractual_freshness_state": statement_export_preview["contractual_freshness_state"].clone(),
        "usage_truth_completeness_state": statement_export_preview["usage_truth_completeness_state"].clone(),
        "rate_card_truth_completeness_state": statement_export_preview["rate_card_truth_completeness_state"].clone(),
        "provider_cost_truth_completeness_state": statement_export_preview["provider_cost_truth_completeness_state"].clone(),
        "invoice_evidence_completeness_state": statement_export_preview["invoice_evidence_completeness_state"].clone(),
        "money_truth_completeness_state": statement_export_preview["money_truth_completeness_state"].clone(),
        "pricing_truth_completeness_state": statement_export_preview["pricing_truth_completeness_state"].clone(),
        "customer_savings_money_truth_completeness_state": statement_export_preview["customer_savings_money_truth_completeness_state"].clone(),
        "amai_cost_truth_completeness_state": statement_export_preview["amai_cost_truth_completeness_state"].clone(),
        "margin_truth_completeness_state": statement_export_preview["margin_truth_completeness_state"].clone(),
        "reconciliation_readiness_state": statement_export_preview["reconciliation_readiness_state"].clone(),
        "margin_readiness_state": statement_export_preview["margin_readiness_state"].clone(),
        "required_sources_for_usage_truth": statement_export_preview["required_sources_for_usage_truth"].clone(),
        "required_sources_for_cost_truth": statement_export_preview["required_sources_for_cost_truth"].clone(),
        "optional_sources_for_invoice_evidence": statement_export_preview["optional_sources_for_invoice_evidence"].clone(),
        "unready_required_sources_for_usage_truth": statement_export_preview["unready_required_sources_for_usage_truth"].clone(),
        "unready_required_sources_for_cost_truth": statement_export_preview["unready_required_sources_for_cost_truth"].clone(),
        "unready_optional_sources_for_invoice_evidence": statement_export_preview["unready_optional_sources_for_invoice_evidence"].clone(),
        "required_sources_for_margin_truth": statement_export_preview["required_sources_for_margin_truth"].clone(),
        "optional_sources_for_margin_invoice_evidence": statement_export_preview["optional_sources_for_margin_invoice_evidence"].clone(),
        "unready_required_sources_for_margin_truth": statement_export_preview["unready_required_sources_for_margin_truth"].clone(),
        "provider_identity_state": statement_export_preview["provider_identity_state"].clone(),
        "included_events_count": statement_export_preview["included_events_count"].clone(),
        "excluded_events_count": statement_export_preview["excluded_events_count"].clone(),
        "included_events_hash": statement_export_preview["included_events_hash"].clone(),
        "excluded_events_hash": statement_export_preview["excluded_events_hash"].clone(),
        "measured_non_billable_lower_bound_tokens": statement_preview["measured_non_billable_lower_bound_tokens"].clone(),
        "adjusted_measured_non_billable_lower_bound_tokens": statement_preview["adjusted_measured_non_billable_lower_bound_tokens"].clone(),
        "billable_lower_bound_tokens": statement_preview["billable_lower_bound_tokens"].clone(),
        "final_amount": statement_preview["final_amount"].clone(),
        "currency_profile": statement_export_preview["currency_profile"].clone(),
        "external_truth_manifest_hash": external_truth_manifest["manifest_hash"].clone(),
        "client_limit_boundary_semantics": statement_export_preview["client_limit_boundary_semantics"].clone(),
        "reviewed_frozen_debt_export_surface": reviewed_frozen_debt_export_surface,
        "customer_contractual_boundary": build_customer_contractual_boundary_from_export(
            contract,
            "customer_settlement_report_preview_report_only",
            statement_export_preview,
        ),
        "settlement_activation_governance": statement_export_preview["settlement_activation_governance"].clone(),
        "adjustment_summary": {
            "registry_status": adjustment_preview["registry_status"].clone(),
            "correction_action_state": adjustment_preview["correction_action_state"].clone(),
            "pending_entries_count": adjustment_preview["pending_entries_count"].clone(),
            "applied_entries_count": adjustment_preview["applied_entries_count"].clone(),
            "disputed_entries_count": adjustment_preview["disputed_entries_count"].clone(),
            "applied_tokens_delta": adjustment_preview["applied_tokens_delta"].clone(),
            "applied_amount_delta": adjustment_preview["applied_amount_delta"].clone(),
        },
        "policy_versions": {
            "settlement_statement_version": contract.settlement_statement_version.clone(),
            "settlement_report_preview_version": contract.settlement_report_preview_version.clone(),
            "billing_policy_version": contract.billing_policy_version.clone(),
            "freeze_close_policy_version": contract.freeze_close_policy_version.clone(),
            "late_arrival_policy_version": contract.late_arrival_policy_version.clone(),
            "correction_policy_version": contract.correction_policy_version.clone(),
            "dispute_policy_version": contract.dispute_policy_version.clone(),
            "settlement_lifecycle_model_version": contract.settlement_lifecycle_model_version.clone(),
            "statement_period_governance_version": contract.statement_period_governance_version.clone(),
            "adjustment_preview_model_version": contract.adjustment_preview_model_version.clone(),
            "adjustment_registry_version": contract.adjustment_registry_version.clone(),
            "reconciliation_contract_version": contract.reconciliation_contract_version.clone(),
            "margin_model_version": contract.margin_model_version.clone(),
            "rate_card_binding_model_version": contract.rate_card_binding_model_version.clone(),
            "infra_cost_binding_model_version": contract.infra_cost_binding_model_version.clone(),
            "contractual_readiness_model_version": contract.contractual_readiness_model_version.clone(),
        },
        "report_only": true,
        "invoice_grade": false,
        "blocking_reasons": statement_export_preview["blocking_reasons"].clone(),
        "note": "Settlement report preview собирает period anchors, hashes, policy snapshot и truth states в один review-grade object. Он пригоден для audit/review, но не является invoice или финальным settlement amount."
    })
}

pub(super) fn build_statement_export_preview(
    report: &Value,
    scope_code: &str,
    scope_label: &str,
    scope_events: &[TokenBudgetEvent],
    contract: &TokenBudgetContractConfig,
    include_verify_events: bool,
) -> Result<Value> {
    let statement_preview = report["token_budget_report"]["statement_previews"][scope_code].clone();
    let reconciliation_preview =
        report["token_budget_report"]["reconciliation_previews"][scope_code].clone();
    let margin_scope = report["token_budget_report"]["margin_view"][scope_code].clone();
    let contractual_summary =
        report["token_budget_report"]["contractual_statement_summaries"][scope_code].clone();

    let (included_items, excluded_items) = build_contractual_line_item_sets(scope_events);
    let included_hash = hash_line_items(&included_items)?;
    let excluded_hash = hash_line_items(&excluded_items)?;
    let export_identity = format!(
        "{}:{}:{}:{}:{}",
        scope_code,
        contract.settlement_statement_version,
        contract.contractual_statement_export_version,
        included_hash,
        excluded_hash
    );
    let adjustment_preview = statement_preview["adjustment_preview"].clone();
    let pending_entries = adjustment_preview["pending_entries_count"]
        .as_u64()
        .unwrap_or(0);
    let applied_entries = adjustment_preview["applied_entries_count"]
        .as_u64()
        .unwrap_or(0);
    let disputed_entries = adjustment_preview["disputed_entries_count"]
        .as_u64()
        .unwrap_or(0);
    let adjustment_status = adjustment_preview["status"].as_str().unwrap_or("unknown");
    let credit_action_state =
        if matches!(adjustment_status, "not_configured" | "default_path_missing") {
            "registry_not_configured"
        } else if pending_entries > 0 {
            "pending_review"
        } else if applied_entries > 0 {
            "applied_report_only_entries_present"
        } else {
            "no_credit_entries"
        };
    let dispute_action_state = if disputed_entries > 0 {
        "open_dispute_entries"
    } else {
        "no_open_disputes"
    };

    let mut preview = serde_json::Map::new();
    let mut insert = |key: &str, value: Value| {
        preview.insert(key.to_string(), value);
    };

    insert(
        "model_version",
        json!(contract.contractual_statement_export_version.clone()),
    );
    insert("scope_code", json!(scope_code));
    insert("scope_label", json!(scope_label));
    insert(
        "statement_preview_id",
        json!(hex_sha256(export_identity.as_bytes())),
    );
    insert(
        "contractual_state",
        contractual_summary["contractual_state"].clone(),
    );
    insert(
        "settlement_stage",
        contractual_summary["settlement_stage"].clone(),
    );
    insert(
        "settlement_stage_family",
        contractual_summary["settlement_stage_family"].clone(),
    );
    insert(
        "next_settlement_stage_candidate",
        contractual_summary["next_settlement_stage_candidate"].clone(),
    );
    insert(
        "next_settlement_stage_blockers",
        contractual_summary["next_settlement_stage_blockers"].clone(),
    );
    insert(
        "future_reserved_settlement_stages",
        contractual_summary["future_reserved_settlement_stages"].clone(),
    );
    insert(
        "contractual_readiness_model_version",
        contractual_summary["contractual_readiness_model_version"].clone(),
    );
    insert(
        "transactional_statuses",
        contractual_summary["transactional_statuses"].clone(),
    );
    insert(
        "coverage_state",
        contractual_summary["coverage_state"].clone(),
    );
    insert(
        "provisional_close_state",
        contractual_summary["provisional_close_state"].clone(),
    );
    insert(
        "provisional_close_candidate",
        contractual_summary["provisional_close_candidate"].clone(),
    );
    insert(
        "provisional_close_earliest_at_epoch_ms",
        contractual_summary["provisional_close_earliest_at_epoch_ms"].clone(),
    );
    insert(
        "usage_truth_completeness_state",
        contractual_summary["usage_truth_completeness_state"].clone(),
    );
    insert(
        "rate_card_truth_completeness_state",
        contractual_summary["rate_card_truth_completeness_state"].clone(),
    );
    insert(
        "provider_cost_truth_completeness_state",
        contractual_summary["provider_cost_truth_completeness_state"].clone(),
    );
    insert(
        "invoice_evidence_completeness_state",
        contractual_summary["invoice_evidence_completeness_state"].clone(),
    );
    insert(
        "money_truth_completeness_state",
        contractual_summary["money_truth_completeness_state"].clone(),
    );
    insert(
        "reconciliation_readiness_state",
        contractual_summary["reconciliation_readiness_state"].clone(),
    );
    insert(
        "required_sources_for_usage_truth",
        contractual_summary["required_sources_for_usage_truth"].clone(),
    );
    insert(
        "required_sources_for_cost_truth",
        contractual_summary["required_sources_for_cost_truth"].clone(),
    );
    insert(
        "optional_sources_for_invoice_evidence",
        contractual_summary["optional_sources_for_invoice_evidence"].clone(),
    );
    insert(
        "unready_required_sources_for_usage_truth",
        contractual_summary["unready_required_sources_for_usage_truth"].clone(),
    );
    insert(
        "unready_required_sources_for_cost_truth",
        contractual_summary["unready_required_sources_for_cost_truth"].clone(),
    );
    insert(
        "unready_optional_sources_for_invoice_evidence",
        contractual_summary["unready_optional_sources_for_invoice_evidence"].clone(),
    );
    insert(
        "rate_card_status",
        contractual_summary["rate_card_status"].clone(),
    );
    insert(
        "rate_card_version",
        contractual_summary["rate_card_version"].clone(),
    );
    insert(
        "rate_card_provider",
        contractual_summary["rate_card_provider"].clone(),
    );
    insert(
        "rate_card_currency_profile",
        contractual_summary["rate_card_currency_profile"].clone(),
    );
    insert(
        "provider_usage_provider",
        contractual_summary["provider_usage_provider"].clone(),
    );
    insert(
        "provider_invoice_provider",
        contractual_summary["provider_invoice_provider"].clone(),
    );
    insert(
        "provider_usage_scope_alignment_state",
        contractual_summary["provider_usage_scope_alignment_state"].clone(),
    );
    insert(
        "provider_invoice_scope_alignment_state",
        contractual_summary["provider_invoice_scope_alignment_state"].clone(),
    );
    insert(
        "rate_card_scope_alignment_state",
        contractual_summary["rate_card_scope_alignment_state"].clone(),
    );
    insert(
        "rate_card_provider_alignment_state",
        contractual_summary["rate_card_provider_alignment_state"].clone(),
    );
    insert(
        "invoice_provider_alignment_state",
        contractual_summary["invoice_provider_alignment_state"].clone(),
    );
    insert(
        "provider_identity_state",
        contractual_summary["provider_identity_state"].clone(),
    );
    insert(
        "reconciliation_temporal_truth_state",
        contractual_summary["reconciliation_temporal_truth_state"].clone(),
    );
    insert(
        "contractual_freshness_state",
        contractual_summary["contractual_freshness_state"].clone(),
    );
    insert(
        "reconciliation_state",
        contractual_summary["reconciliation_state"].clone(),
    );
    insert("margin_state", contractual_summary["margin_state"].clone());
    insert(
        "margin_confidence_state",
        contractual_summary["margin_confidence_state"].clone(),
    );
    insert(
        "margin_readiness_state",
        contractual_summary["margin_readiness_state"].clone(),
    );
    insert(
        "infra_cost_truth_completeness_state",
        contractual_summary["infra_cost_truth_completeness_state"].clone(),
    );
    insert(
        "pricing_truth_completeness_state",
        contractual_summary["pricing_truth_completeness_state"].clone(),
    );
    insert(
        "customer_savings_money_truth_completeness_state",
        contractual_summary["customer_savings_money_truth_completeness_state"].clone(),
    );
    insert(
        "amai_cost_truth_completeness_state",
        contractual_summary["amai_cost_truth_completeness_state"].clone(),
    );
    insert(
        "margin_truth_completeness_state",
        contractual_summary["margin_truth_completeness_state"].clone(),
    );
    insert(
        "required_sources_for_margin_truth",
        contractual_summary["required_sources_for_margin_truth"].clone(),
    );
    insert(
        "optional_sources_for_margin_invoice_evidence",
        contractual_summary["optional_sources_for_margin_invoice_evidence"].clone(),
    );
    insert(
        "unready_required_sources_for_margin_truth",
        contractual_summary["unready_required_sources_for_margin_truth"].clone(),
    );
    insert(
        "margin_provider_identity_state",
        contractual_summary["margin_provider_identity_state"].clone(),
    );
    insert(
        "margin_temporal_truth_state",
        contractual_summary["margin_temporal_truth_state"].clone(),
    );
    insert(
        "infra_cost_scope_alignment_state",
        contractual_summary["infra_cost_scope_alignment_state"].clone(),
    );
    insert(
        "margin_blocking_reasons",
        contractual_summary["margin_blocking_reasons"].clone(),
    );
    insert(
        "internal_money_arithmetic_readiness_state",
        contractual_summary["internal_money_arithmetic_readiness_state"].clone(),
    );
    insert(
        "internal_money_arithmetic_blocking_reasons",
        contractual_summary["internal_money_arithmetic_blocking_reasons"].clone(),
    );
    insert(
        "contractual_settlement_readiness_state",
        contractual_summary["contractual_settlement_readiness_state"].clone(),
    );
    insert(
        "contractual_settlement_blocking_reasons",
        contractual_summary["contractual_settlement_blocking_reasons"].clone(),
    );
    insert("export_status", json!("review_ready_report_only"));
    insert("included_events_count", json!(included_items.len()));
    insert("excluded_events_count", json!(excluded_items.len()));
    insert("included_events_hash", json!(included_hash));
    insert("excluded_events_hash", json!(excluded_hash));
    insert("customer_review_ready", json!(true));
    insert("invoice_ready", json!(false));
    insert("credit_action_state", json!(credit_action_state));
    insert("dispute_action_state", json!(dispute_action_state));
    insert("pending_adjustment_entries_count", json!(pending_entries));
    insert("disputed_entries_count", json!(disputed_entries));
    insert(
        "export_semantics",
        json!({
            "surface_kind": "customer_review_report_only",
            "self_serve_state": "self_serve_ready_report_only",
            "invoice_grade": false,
            "operational_telemetry_included": false,
            "redaction_policy": "raw_query_text_removed_keep_query_hash_and_token_state",
            "customer_visible_sections": [
                "statement_preview_id",
                "settlement_report_preview",
                "client_limit_boundary_semantics",
                "reviewed_frozen_debt_export_surface",
                "contractual_state",
                "coverage_state",
                "external_truth_manifest",
                "transactional_statuses",
                "included_events_hash",
                "excluded_events_hash",
                "suitability",
                "evidence_pack_command"
            ]
        }),
    );
    insert(
        "blocking_reasons",
        contractual_summary["blocking_reasons"].clone(),
    );
    insert(
        "external_truth_manifest",
        report["token_budget_report"]["external_truth_manifest"].clone(),
    );
    insert("suitability", contractual_summary["suitability"].clone());
    insert("evidence_pack_available", json!(true));
    insert(
        "evidence_pack_command",
        json!(format!(
            "./scripts/amai_exec.sh observe token-evidence-pack --scope {}{}",
            scope_code,
            if include_verify_events {
                " --include-verify-events true"
            } else {
                ""
            }
        )),
    );
    let line_item_surfaces = json!({
        "statement_preview": statement_preview,
        "reconciliation_preview": reconciliation_preview,
        "margin_scope": margin_scope,
    });
    let client_limit_boundary_semantics =
        if contractual_summary["client_limit_boundary_semantics"].is_null() {
            build_client_limit_boundary_review_surface(&line_item_surfaces["statement_preview"])
        } else {
            contractual_summary["client_limit_boundary_semantics"].clone()
        };
    insert("line_item_surfaces", line_item_surfaces);
    insert(
        "client_limit_boundary_semantics",
        client_limit_boundary_semantics.clone(),
    );
    insert(
        "reviewed_frozen_debt_export_surface",
        build_reviewed_frozen_debt_export_surface(
            contract,
            &client_limit_boundary_semantics,
            Some(scope_code),
        ),
    );
    insert(
        "note",
        json!(
            "Это stable export preview для customer review: hashes и scope states уже зафиксированы, но invoice-grade settlement всё ещё не materialized."
        ),
    );
    let mut preview = Value::Object(preview);
    preview["customer_contractual_boundary"] = build_customer_contractual_boundary_from_export(
        contract,
        "customer_review_report_only",
        &preview,
    );
    preview["settlement_activation_governance"] =
        build_settlement_activation_governance_from_export(contract, &preview);
    preview["adjustment_activation_governance"] =
        build_adjustment_activation_governance_from_export(contract, &preview);
    preview["settlement_report_preview"] = build_settlement_report_preview(contract, &preview);
    Ok(preview)
}

pub(super) fn build_contractual_evidence_pack(
    report: &Value,
    scope_code: &str,
    scope_label: &str,
    scope_events: &[TokenBudgetEvent],
    contract: &TokenBudgetContractConfig,
    profile: &ResolvedProfile,
    include_verify_events: bool,
    generated_at_epoch_ms: i64,
) -> Result<Value> {
    let (included_items, excluded_items) = build_contractual_line_item_sets(scope_events);

    let statement_preview = report["token_budget_report"]["statement_previews"][scope_code].clone();
    let reconciliation_preview =
        report["token_budget_report"]["reconciliation_previews"][scope_code].clone();
    let margin_scope = report["token_budget_report"]["margin_view"][scope_code].clone();
    let statement_export_preview =
        report["token_budget_report"]["statement_export_previews"][scope_code].clone();
    let mut customer_contractual_boundary =
        statement_export_preview["customer_contractual_boundary"].clone();
    customer_contractual_boundary["surface_kind"] = json!("customer_evidence_pack_report_only");
    let reviewed_frozen_debt_export_surface =
        if statement_export_preview["reviewed_frozen_debt_export_surface"].is_null() {
            build_reviewed_frozen_debt_export_surface(
                contract,
                &statement_export_preview["client_limit_boundary_semantics"],
                statement_export_preview["scope_code"].as_str(),
            )
        } else {
            statement_export_preview["reviewed_frozen_debt_export_surface"].clone()
        };
    let settlement_report_preview =
        settlement_report_preview_from_export(contract, &statement_export_preview);

    Ok(json!({
        "contractual_evidence_pack": {
            "pack_version": contract.contractual_evidence_pack_version.clone(),
            "generated_at_epoch_ms": generated_at_epoch_ms,
            "scope_code": scope_code,
            "scope_label": scope_label,
            "budget_profile": {
                "code": profile.code.clone(),
                "display_name": profile.display_name.clone(),
            },
        "include_verify_events": include_verify_events,
        "truth_guardrail": {
            "retrieval_savings_floor": "real",
            "partial_whole_agent_cycle_lower_bound": "real",
            "full_session_economics": "not_fully_measured"
        },
        "contract_versions": report["token_budget_report"]["contract"].clone(),
        "settlement_stage": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["settlement_stage"].clone(),
        "settlement_stage_family": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["settlement_stage_family"].clone(),
        "next_settlement_stage_candidate": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["next_settlement_stage_candidate"].clone(),
        "next_settlement_stage_blockers": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["next_settlement_stage_blockers"].clone(),
        "contractual_readiness_model_version": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["contractual_readiness_model_version"].clone(),
        "internal_money_arithmetic_readiness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["internal_money_arithmetic_readiness_state"].clone(),
        "internal_money_arithmetic_blocking_reasons": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["internal_money_arithmetic_blocking_reasons"].clone(),
        "contractual_settlement_readiness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["contractual_settlement_readiness_state"].clone(),
        "contractual_settlement_blocking_reasons": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["contractual_settlement_blocking_reasons"].clone(),
        "transactional_statuses": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["transactional_statuses"].clone(),
        "rate_card_status": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["rate_card_status"].clone(),
        "rate_card_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["rate_card_truth_completeness_state"].clone(),
        "provider_cost_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["provider_cost_truth_completeness_state"].clone(),
        "invoice_evidence_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["invoice_evidence_completeness_state"].clone(),
        "required_sources_for_usage_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["required_sources_for_usage_truth"].clone(),
        "required_sources_for_cost_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["required_sources_for_cost_truth"].clone(),
        "optional_sources_for_invoice_evidence": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["optional_sources_for_invoice_evidence"].clone(),
        "unready_required_sources_for_usage_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["unready_required_sources_for_usage_truth"].clone(),
        "unready_required_sources_for_cost_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["unready_required_sources_for_cost_truth"].clone(),
        "unready_optional_sources_for_invoice_evidence": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["unready_optional_sources_for_invoice_evidence"].clone(),
        "rate_card_version": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["rate_card_version"].clone(),
        "rate_card_provider": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["rate_card_provider"].clone(),
        "rate_card_currency_profile": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["rate_card_currency_profile"].clone(),
        "provider_usage_provider": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["provider_usage_provider"].clone(),
        "provider_invoice_provider": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["provider_invoice_provider"].clone(),
        "provider_identity_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["provider_identity_state"].clone(),
        "infra_cost_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["infra_cost_truth_completeness_state"].clone(),
        "pricing_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["pricing_truth_completeness_state"].clone(),
        "customer_savings_money_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["customer_savings_money_truth_completeness_state"].clone(),
        "amai_cost_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["amai_cost_truth_completeness_state"].clone(),
        "margin_truth_completeness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["margin_truth_completeness_state"].clone(),
        "required_sources_for_margin_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["required_sources_for_margin_truth"].clone(),
        "optional_sources_for_margin_invoice_evidence": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["optional_sources_for_margin_invoice_evidence"].clone(),
        "unready_required_sources_for_margin_truth": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["unready_required_sources_for_margin_truth"].clone(),
        "margin_readiness_state": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["margin_readiness_state"].clone(),
        "external_truth_manifest": report["token_budget_report"]["external_truth_manifest"].clone(),
        "client_limit_boundary_semantics": statement_export_preview["client_limit_boundary_semantics"].clone(),
        "reviewed_frozen_debt_export_surface": reviewed_frozen_debt_export_surface,
        "settlement_report_preview": settlement_report_preview,
        "customer_contractual_boundary": customer_contractual_boundary,
        "settlement_activation_governance": statement_export_preview["settlement_activation_governance"].clone(),
        "adjustment_activation_governance": statement_export_preview["adjustment_activation_governance"].clone(),
        "export_semantics": {
            "surface_kind": "customer_evidence_pack_report_only",
            "self_serve_state": "self_serve_ready_report_only",
            "invoice_grade": false,
            "operational_telemetry_included": false,
            "redaction_policy": "raw_query_text_removed_keep_query_hash_and_token_state",
            "customer_visible_sections": [
                "truth_guardrail",
                "contract_versions",
                "external_truth_manifest",
                "client_limit_boundary_semantics",
                "settlement_report_preview",
                "statement_preview",
                "reconciliation_preview",
                "margin_scope",
                "transactional_statuses",
                "line_items"
            ]
        },
        "statement_preview": statement_preview,
        "reconciliation_preview": reconciliation_preview,
        "margin_scope": margin_scope,
        "suitability": report["token_budget_report"]["contractual_statement_summaries"][scope_code]["suitability"].clone(),
        "included_events_count": included_items.len(),
            "excluded_events_count": excluded_items.len(),
            "included_events_hash": hash_line_items(&included_items)?,
            "excluded_events_hash": hash_line_items(&excluded_items)?,
            "line_items": {
                "included": included_items,
                "excluded": excluded_items,
            },
            "note": "Это contractual evidence pack для report-only tokenonomics: он доказывает состав измеренного scope, но не превращает lower bound в invoice."
        }
    }))
}
