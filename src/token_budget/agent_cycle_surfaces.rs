use super::*;

pub(crate) fn build_agent_cycle_economics(
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
    now_epoch_ms: i64,
    current_session_events: &[TokenBudgetEvent],
    rolling_window_events: Option<&[TokenBudgetEvent]>,
    lifetime_events: &[TokenBudgetEvent],
    rolling_window_label: &str,
    rollout_observations: &[codex_threads::RolloutAssistantGenerationObservation],
    current_session_assistant_scope: &AssistantGenerationScopeObservation,
    rolling_window_assistant_scope: Option<&AssistantGenerationScopeObservation>,
    lifetime_assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    json!({
        "model_version": contract.agent_cycle_model_version.clone(),
        "status": "partial_lower_bound",
        "contract": {
            "scope": "lower_bound_whole_agent_cycle",
            "status": "partial_lower_bound",
            "billing_mode": contract.billing_mode.clone(),
            "coverage_model_version": contract.coverage_model_version.clone(),
            "metering_freshness_model_version": contract.metering_freshness_model_version.clone(),
            "billing_policy_version": contract.billing_policy_version.clone(),
            "client_limit_meter_alignment": {
                "model_version": contract.client_limit_meter_alignment_version.clone(),
                "baseline_equivalence_model_version": contract
                    .client_limit_baseline_equivalence_version
                    .clone(),
                "strict_meter_slice_model_version": contract
                    .client_limit_strict_meter_slice_version
                    .clone(),
                "explicit_boundary_surface_model_version": contract
                    .client_limit_explicit_boundary_surface_version
                    .clone(),
                "continuity_boundary_rollup_model_version": contract
                    .client_limit_continuity_boundary_rollup_version
                    .clone(),
                "pre_amai_baseline_source_model_version": contract
                    .client_limit_pre_amai_baseline_source_version
                    .clone(),
                "frozen_gap_review_surface_model_version": contract
                    .client_limit_frozen_gap_review_surface_version
                    .clone(),
                "alignment_state": "partial_lower_bound_not_meter_equivalent",
                "same_meter_as_client_limit": false,
                "measured_components": [
                    "retrieval_payload",
                    "followup_recovery"
                ],
                "partially_measured_components": [],
                "observable_components": [
                    "client_prompt",
                    "assistant_generation",
                    "tool_overhead_outside_retrieval",
                    "continuity_restore_outside_retrieval"
                ],
                "missing_components": [
                    "client_prompt",
                    "assistant_generation",
                    "tool_overhead_outside_retrieval",
                    "continuity_restore_outside_retrieval"
                ],
                "blocking_reasons": [
                    "client_prompt_unmeasured",
                    "assistant_generation_unmeasured",
                    "tool_overhead_outside_retrieval_unmeasured",
                    "continuity_restore_outside_retrieval_unmeasured"
                ],
                "note": "Даже при высокой measured lower bound current meter ещё не эквивалентен полному клиентскому лимиту сессии. Whole-cycle observed components можно materialize-ить по мере появления event-level evidence, но same-meter claim запрещён раньше baseline-equivalent semantics."
            },
            "rate_card_version": contract.rate_card_version.clone(),
            "currency_profile": contract.currency_profile.clone(),
            "settlement_status": contract.settlement_status.clone(),
            "summary": "Это не весь токеновый бюджет клиента, а подтверждённая нижняя граница полного агентного цикла.",
            "measured_components": [
                {
                    "code": "retrieval_payload",
                    "label": "Контекст, который Amai реально вернул"
                },
                {
                    "code": "followup_recovery",
                    "label": "Доуточнения после неполного ответа, которые уже видно в ledger"
                }
            ],
            "missing_components": [
                {
                    "code": "client_prompt",
                    "label": "Токены исходного запроса клиента"
                },
                {
                    "code": "assistant_generation",
                    "label": "Токены генерации итогового ответа"
                },
                {
                    "code": "tool_overhead_outside_retrieval",
                    "label": "Tool-step и orchestration вне retrieval-контура"
                },
                {
                    "code": "continuity_restore_outside_retrieval",
                    "label": "Восстановление continuity, если оно прошло вне token-ledger retrieval-событий"
                }
            ],
            "reporting_layers": {
                "billable": {
                    "status": "disabled_report_only",
                    "note": "Пока billing policy работает только в report-only режиме, подтверждённая нижняя граница не используется как money-facing начисление."
                },
                "measured_non_billable": {
                    "status": "active",
                    "note": "Подтверждённые live lower-bound измерения уже видны и пригодны для анализа, но ещё не являются contractual billing amount."
                },
                "unmeasured": {
                    "status": "active",
                    "note": "Полный agent-cycle ещё не покрыт: missing components перечислены отдельно и не маскируются под измеренную экономию."
                }
            },
            "note": "Линия 'без Amai' здесь пока означает измеренный baseline retrieval-части цикла, а линия 'с Amai' — retrieval плюс уже зафиксированные доуточнения. Это честная нижняя граница, а не полная стоимость всей агентной сессии."
        },
        "chart_contract": {
            "timeline_type": "event_cumulative",
            "x_axis": "timestamp_epoch_ms",
            "y_axes": [
                "without_amai_measured_tokens",
                "with_amai_measured_tokens",
                "measured_saved_tokens"
            ],
            "series": [
                "all_live_timeline",
                "verified_live_timeline"
            ],
            "point_limit": AGENT_CYCLE_TIMELINE_MAX_POINTS
        },
        "current_session": build_agent_cycle_scope(
            measurement,
            contract,
            now_epoch_ms,
            "current_session",
            "текущая сессия",
            current_session_events,
            AGENT_CYCLE_TIMELINE_MAX_POINTS / 2,
            rollout_observations,
            materialized_assistant_scope(current_session_assistant_scope),
        ),
        "rolling_window": rolling_window_events
            .map(|events| {
                build_agent_cycle_scope(
                    measurement,
                    contract,
                    now_epoch_ms,
                    "rolling_window",
                    &format!("окно {}", rolling_window_label),
                    events,
                    AGENT_CYCLE_TIMELINE_MAX_POINTS,
                    rollout_observations,
                    rolling_window_assistant_scope,
                )
            })
            .unwrap_or(Value::Null),
        "lifetime": build_agent_cycle_scope(
            measurement,
            contract,
            now_epoch_ms,
            "lifetime",
            "всё время записи",
            lifetime_events,
            AGENT_CYCLE_TIMELINE_MAX_POINTS,
            rollout_observations,
            lifetime_assistant_scope,
        ),
    })
}

fn build_agent_cycle_scope(
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
    now_epoch_ms: i64,
    scope_code: &str,
    scope_label: &str,
    events: &[TokenBudgetEvent],
    max_points: usize,
    rollout_observations: &[codex_threads::RolloutAssistantGenerationObservation],
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let live_events = events
        .iter()
        .filter(|event| event.traffic_class == "live")
        .cloned()
        .collect::<Vec<_>>();
    let summary = summarize_events(&live_events, now_epoch_ms, measurement, contract);
    let with_amai_measured_tokens = summary["total_context_tokens"]
        .as_u64()
        .unwrap_or(0)
        .saturating_add(summary["total_recovery_tokens"].as_u64().unwrap_or(0));
    let verified_with_amai_measured_tokens = summary["verified_delivered_tokens"]
        .as_u64()
        .unwrap_or(0)
        .saturating_add(summary["verified_recovery_tokens"].as_u64().unwrap_or(0));
    let observed_whole_cycle_with_amai_tokens =
        observed_whole_cycle_with_assistant_scope_tokens(&summary, assistant_scope)
            .unwrap_or(with_amai_measured_tokens);
    let verified_observed_whole_cycle_with_amai_tokens =
        verified_observed_whole_cycle_with_assistant_scope_tokens(&summary, assistant_scope)
            .unwrap_or(verified_with_amai_measured_tokens);
    let verified_share_pct = percent_share(
        summary["counted_events"].as_u64().unwrap_or(0),
        summary["events_total"].as_u64().unwrap_or(0),
    );
    let client_limit_meter_alignment =
        normalize_agent_cycle_client_limit_meter_alignment(build_client_limit_meter_alignment(
            contract,
            "agent_cycle_scope",
            &summary,
            Some(&live_events),
            Some(rollout_observations),
            assistant_scope,
        ));
    json!({
        "scope_code": scope_code,
        "scope_label": scope_label,
        "status": "partial_lower_bound",
        "events_total": summary["events_total"].as_u64().unwrap_or(0),
        "counted_events": summary["counted_events"].as_u64().unwrap_or(0),
        "excluded_events_count": summary["excluded_events_count"].as_u64().unwrap_or(0),
        "coverage": summary["coverage"].clone(),
        "excluded_breakdown": summary["excluded_breakdown"].clone(),
        "client_limit_meter_alignment": client_limit_meter_alignment,
        "observed_client_prompt_tokens": summary["observed_client_prompt_tokens"].clone(),
        "observed_assistant_generation_tokens": Value::from(
            assistant_scope
                .map(|scope| scope.observed_tokens)
                .unwrap_or_else(|| summary["observed_assistant_generation_tokens"].as_u64().unwrap_or(0))
        ),
        "observed_tool_overhead_tokens": summary["observed_tool_overhead_tokens"].clone(),
        "observed_continuity_restore_tokens": summary["observed_continuity_restore_tokens"].clone(),
        "verified_share_pct": verified_share_pct,
        "without_amai_measured_tokens": summary["total_naive_tokens"].as_u64().unwrap_or(0),
        "with_amai_measured_tokens": with_amai_measured_tokens,
        "observed_whole_cycle_with_amai_tokens": observed_whole_cycle_with_amai_tokens,
        "measured_saved_tokens": summary["total_effective_saved_tokens"].as_i64().unwrap_or(0),
        "measured_saved_pct": summary["effective_savings_pct"].as_f64().unwrap_or(0.0),
        "verified_without_amai_measured_tokens": summary["verified_baseline_tokens"].as_u64().unwrap_or(0),
        "verified_with_amai_measured_tokens": verified_with_amai_measured_tokens,
        "verified_observed_whole_cycle_with_amai_tokens": verified_observed_whole_cycle_with_amai_tokens,
        "verified_measured_saved_tokens": summary["verified_effective_saved_tokens"].as_i64().unwrap_or(0),
        "verified_measured_saved_pct": summary["verified_effective_savings_pct"].as_f64().unwrap_or(0.0),
        "answer_like_counted_events": summary["answer_like_counted_events"].as_u64().unwrap_or(0),
        "answer_like_rate": summary["answer_like_rate"].as_f64().unwrap_or(0.0),
        "started_at_epoch_ms": summary["started_at_epoch_ms"].clone(),
        "ended_at_epoch_ms": summary["ended_at_epoch_ms"].clone(),
        "all_live_timeline": build_agent_cycle_timeline(&live_events, false, max_points),
        "verified_live_timeline": build_agent_cycle_timeline(&live_events, true, max_points),
    })
}

fn normalize_agent_cycle_client_limit_meter_alignment(mut alignment: Value) -> Value {
    let Some(component_semantics) = alignment["baseline_equivalence"]["component_semantics"]
        .as_array()
        .cloned()
    else {
        return alignment;
    };
    let assistant_component = component_semantics
        .iter()
        .find(|component| component["code"].as_str() == Some("assistant_generation"));
    let assistant_passthrough = assistant_component.is_some_and(|component| {
        component["baseline_semantics_state"].as_str() == Some("observed_tokens_passthrough")
            && component["baseline_measured_tokens"].as_u64().unwrap_or(0) > 0
    });
    let assistant_missing = assistant_component.is_some_and(|component| {
        component["baseline_measured_tokens"].is_null()
            && component["observed_tokens"].as_u64().unwrap_or(0) > 0
    });
    if !assistant_passthrough && !assistant_missing {
        return alignment;
    }

    let countable_codes: &[&str] = if assistant_missing {
        &["client_prompt"]
    } else {
        &["client_prompt", "tool_overhead_outside_retrieval"]
    };
    let measured_component_codes = ["client_prompt", "tool_overhead_outside_retrieval"];
    let measured_components = component_semantics
        .iter()
        .filter(|component| {
            component["code"]
                .as_str()
                .is_some_and(|code| measured_component_codes.contains(&code))
        })
        .filter_map(|component| {
            component["baseline_measured_tokens"]
                .as_u64()
                .filter(|tokens| *tokens > 0)
                .and(component["code"].as_str().map(ToOwned::to_owned))
        })
        .collect::<Vec<_>>();
    let measured_baseline_tokens_lower_bound = component_semantics
        .iter()
        .filter(|component| {
            component["code"]
                .as_str()
                .is_some_and(|code| countable_codes.contains(&code))
        })
        .filter_map(|component| component["baseline_measured_tokens"].as_u64())
        .sum::<u64>();
    let mut missing_components = alignment["baseline_equivalence"]["missing_baseline_components"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    if !missing_components
        .iter()
        .any(|code| code == "assistant_generation")
    {
        missing_components.push("assistant_generation".to_string());
    }

    if let Some(root) = alignment["baseline_equivalence"].as_object_mut() {
        root.insert(
            "measured_baseline_components".to_string(),
            json!(measured_components.clone()),
        );
        root.insert(
            "missing_baseline_components".to_string(),
            json!(missing_components),
        );
        root.insert(
            "measured_baseline_tokens_lower_bound".to_string(),
            json!(measured_baseline_tokens_lower_bound),
        );
        root.insert(
            "state".to_string(),
            json!("baseline_component_semantics_partial"),
        );
    }
    if let Some(root) = alignment["strict_client_meter_slice"].as_object_mut() {
        root.insert(
            "lower_bound_tokens".to_string(),
            json!(measured_baseline_tokens_lower_bound),
        );
        root.insert(
            "measured_baseline_tokens_lower_bound".to_string(),
            json!(measured_baseline_tokens_lower_bound),
        );
        root.insert("components".to_string(), json!(measured_components));
    }
    alignment
}

fn build_agent_cycle_timeline(
    events: &[TokenBudgetEvent],
    verified_only: bool,
    max_points: usize,
) -> Value {
    let filtered = events
        .iter()
        .filter(|event| !verified_only || event.quality_ok)
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        return Value::Array(Vec::new());
    }

    let mut without_amai_cumulative = 0_u64;
    let mut with_amai_cumulative = 0_u64;
    let mut points = Vec::with_capacity(filtered.len());
    for (index, event) in filtered.into_iter().enumerate() {
        without_amai_cumulative = without_amai_cumulative.saturating_add(event.naive_tokens);
        with_amai_cumulative = with_amai_cumulative
            .saturating_add(event.context_tokens)
            .saturating_add(event.recovery_tokens);
        let measured_saved_tokens = without_amai_cumulative as i64 - with_amai_cumulative as i64;
        points.push(json!({
            "point_index": index + 1,
            "timestamp_epoch_ms": event.created_at_epoch_ms,
            "event_id": event.event_id,
            "query_type": event.query_type,
            "cold_warm_state": event.cold_warm_state,
            "answer_like": is_answer_like_event(event),
            "without_amai_measured_tokens": without_amai_cumulative,
            "with_amai_measured_tokens": with_amai_cumulative,
            "measured_saved_tokens": measured_saved_tokens,
            "measured_saved_pct": percent_from_signed(measured_saved_tokens, without_amai_cumulative),
        }));
    }
    downsample_timeline(points, max_points)
}

fn downsample_timeline(points: Vec<Value>, max_points: usize) -> Value {
    if points.len() <= max_points || max_points < 2 {
        return Value::Array(points);
    }

    let last_index = points.len() - 1;
    let step = last_index as f64 / (max_points - 1) as f64;
    let mut sampled = Vec::with_capacity(max_points);
    let mut last_taken = None::<usize>;
    for bucket in 0..max_points {
        let index = if bucket == max_points - 1 {
            last_index
        } else {
            (bucket as f64 * step).round() as usize
        }
        .min(last_index);
        if last_taken == Some(index) {
            continue;
        }
        sampled.push(points[index].clone());
        last_taken = Some(index);
    }
    if last_taken != Some(last_index) {
        sampled.push(points[last_index].clone());
    }
    Value::Array(sampled)
}

pub(crate) fn percent_share(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

pub(crate) fn source_breakdown(
    events: &[TokenBudgetEvent],
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
) -> Value {
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
                        contract,
                    ),
                })
            })
            .collect(),
    )
}

pub(crate) fn query_slice_breakdown(
    events: &[TokenBudgetEvent],
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
) -> Value {
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
                    contract,
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
                    "sample_count": summary["sample_count"],
                    "current_latency_ms": summary["current_latency_ms"],
                    "p50_latency_ms": summary["p50_latency_ms"],
                    "p95_latency_ms": summary["p95_latency_ms"],
                    "p99_latency_ms": summary["p99_latency_ms"],
                    "max_latency_ms": summary["max_latency_ms"],
                })
            })
            .collect(),
    )
}

pub(crate) fn baseline_strategy_breakdown(
    events: &[TokenBudgetEvent],
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
) -> Value {
    let allowed = allowed_baseline_classes()
        .into_iter()
        .collect::<HashSet<_>>();
    let mut grouped = BTreeMap::<String, Vec<TokenBudgetEvent>>::new();
    for event in events {
        grouped
            .entry(event.baseline_strategy.clone())
            .or_default()
            .push(event.clone());
    }
    Value::Array(
        grouped
            .into_iter()
            .map(|(baseline_strategy, items)| {
                let summary = summarize_events(
                    &items,
                    items
                        .last()
                        .map(|item| item.created_at_epoch_ms)
                        .unwrap_or_default(),
                    measurement,
                    contract,
                );
                json!({
                    "baseline_strategy": baseline_strategy,
                    "allowed_class": allowed.contains(baseline_strategy.as_str()),
                    "events_count": summary["events_count"],
                    "counted_events": summary["counted_events"],
                    "verified_effective_savings_pct": summary["verified_effective_savings_pct"],
                    "quality_ok_rate": summary["quality_ok_rate"],
                    "coverage": summary["coverage"],
                })
            })
            .collect(),
    )
}

pub(crate) fn temperature_slice_breakdown(
    events: &[TokenBudgetEvent],
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
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
                    contract,
                );
                json!({
                    "state": state,
                    "events_count": summary["events_count"],
                    "counted_events": summary["counted_events"],
                    "verified_effective_savings_pct": summary["verified_effective_savings_pct"],
                    "median_recovery_tokens": summary["median_recovery_tokens"],
                    "sample_count": summary["sample_count"],
                    "current_latency_ms": summary["current_latency_ms"],
                    "p50_latency_ms": summary["p50_latency_ms"],
                    "p95_latency_ms": summary["p95_latency_ms"],
                    "p99_latency_ms": summary["p99_latency_ms"],
                    "max_latency_ms": summary["max_latency_ms"],
                })
            })
            .collect(),
    )
}

pub(crate) fn latency_slice_breakdown(events: &[TokenBudgetEvent]) -> Value {
    let active_live_origin = [
        "context_pack_token_budget_v13",
        "context_pack_token_budget_v12",
        "context_pack_token_budget_v11",
        "context_pack_token_budget_v10",
        "context_pack_token_budget_v9",
        "context_pack_token_budget_v8",
        "context_pack_token_budget_v7",
        "context_pack_token_budget_v6",
        "context_pack_token_budget_v5",
        "context_pack_token_budget_v4",
        "context_pack_token_budget_v3",
        "context_pack_token_budget_v2",
    ]
    .into_iter()
    .find(|origin| {
        events.iter().any(|event| {
            event.measurement_scope == "retrieval_lower_bound" && event.payload_origin == *origin
        })
    });
    let mut grouped = BTreeMap::<String, Vec<f64>>::new();
    let mut current_latency = BTreeMap::<String, f64>::new();

    for event in events {
        if event.measurement_scope != "retrieval_lower_bound" {
            continue;
        }
        if event.traffic_class == "live" && !event.quality_ok {
            continue;
        }
        if event.traffic_class == "live"
            && active_live_origin.is_some()
            && Some(event.payload_origin.as_str()) != active_live_origin
        {
            continue;
        }
        if !event.latency_ms.is_finite() {
            continue;
        }
        grouped
            .entry("mixed".to_string())
            .or_default()
            .push(event.latency_ms);
        current_latency.insert("mixed".to_string(), event.latency_ms);

        let state = normalize_latency_state(&event.cold_warm_state);
        grouped
            .entry(state.to_string())
            .or_default()
            .push(event.latency_ms);
        current_latency.insert(state.to_string(), event.latency_ms);
    }

    let order = ["mixed", "hot", "cold", "benchmark"];
    let mut slices = Vec::new();
    for state in order {
        if let Some(values) = grouped.get(state) {
            slices.push(latency_slice_json(
                state,
                current_latency.get(state).copied().unwrap_or_default(),
                values,
            ));
        }
    }

    for (state, values) in grouped {
        if order.contains(&state.as_str()) {
            continue;
        }
        slices.push(latency_slice_json(
            &state,
            current_latency.get(&state).copied().unwrap_or_default(),
            &values,
        ));
    }

    Value::Array(slices)
}

pub(crate) fn latency_slice_json(state: &str, current_latency_ms: f64, values: &[f64]) -> Value {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    json!({
        "state": state,
        "display_name": latency_state_display_name(state),
        "sample_count": sorted.len(),
        "current_latency_ms": current_latency_ms,
        "p50_latency_ms": percentile_from_sorted(&sorted, 0.50),
        "p95_latency_ms": percentile_from_sorted(&sorted, 0.95),
        "p99_latency_ms": percentile_from_sorted(&sorted, 0.99),
        "max_latency_ms": sorted.last().copied().unwrap_or_default(),
    })
}

pub(crate) fn normalize_latency_state(state: &str) -> &'static str {
    match state {
        "warm" => "hot",
        "cold" => "cold",
        "benchmark" => "benchmark",
        _ => "mixed",
    }
}

fn latency_state_display_name(state: &str) -> &'static str {
    match state {
        "mixed" => "mix",
        "hot" => "hot",
        "cold" => "cold",
        "benchmark" => "benchmark",
        _ => "other",
    }
}

pub(crate) fn percentile_from_sorted(values: &[f64], percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let percentile = percentile.clamp(0.0, 1.0);
    let index = ((values.len() - 1) as f64 * percentile).ceil() as usize;
    values[index.min(values.len() - 1)]
}
