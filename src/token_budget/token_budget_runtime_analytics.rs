use super::token_budget_runtime_shared::{
    OPERATOR_CLIENT_BUDGET_CONTEXT_CONTRACT_VERSION, OPERATOR_GUARD_METRIC_DOMAIN,
    PUBLIC_PRODUCT_HEADLINE_CONTRACT_VERSION, PUBLIC_SAVINGS_METRIC_CODE,
    PUBLIC_SAVINGS_METRIC_DOMAIN, PUBLIC_SAVINGS_PROJECTION_CONFIDENCE_EXACT_SAME_METER,
    PUBLIC_SAVINGS_PROJECTION_CONFIDENCE_EXACT_SAME_METER_VERIFIED,
    PUBLIC_SAVINGS_PROJECTION_CONFIDENCE_MISSING,
    PUBLIC_SAVINGS_PROJECTION_CONFIDENCE_PRELIMINARY_SUMMARY,
    PUBLIC_SAVINGS_PROJECTION_CONFIDENCE_VERIFIED_SUMMARY,
    PUBLIC_SAVINGS_PROJECTION_CONTRACT_VERSION,
    PUBLIC_SAVINGS_PROJECTION_PAIR_SOURCE_EXACT_SAME_METER,
    PUBLIC_SAVINGS_PROJECTION_PAIR_SOURCE_MEASURED_SUMMARY,
    PUBLIC_SAVINGS_PROJECTION_PAIR_SOURCE_MISSING,
    PUBLIC_SAVINGS_PROJECTION_PAIR_SOURCE_VERIFIED_SUMMARY,
    PUBLIC_SAVINGS_PROJECTION_STATUS_MISSING, PUBLIC_SAVINGS_PROJECTION_STATUS_OBSERVED,
    PUBLIC_SAVINGS_PROJECTION_STATUS_PRELIMINARY,
};
use super::*;
use crate::dashboard_format::format_u64;

fn derived_client_prompt_tokens(
    event: &TokenBudgetEvent,
    tokenizer_cache: &mut HashMap<String, Option<CoreBPE>>,
) -> Option<u64> {
    if let Some(tokens) = event.client_prompt_tokens {
        return Some(tokens);
    }
    if event.query.is_empty() {
        return None;
    }
    if !tokenizer_cache.contains_key(&event.tokenizer) {
        tokenizer_cache.insert(
            event.tokenizer.clone(),
            build_tokenizer(&event.tokenizer).ok(),
        );
    }
    tokenizer_cache
        .get(&event.tokenizer)
        .and_then(|tokenizer| tokenizer.as_ref())
        .map(|tokenizer| tokenizer.encode_with_special_tokens(&event.query).len() as u64)
}

pub(crate) fn summarize_events(
    events: &[TokenBudgetEvent],
    now_epoch_ms: i64,
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
) -> Value {
    if events.is_empty() {
        return json!({
            "events_total": 0,
            "events_count": 0,
            "live_events_count": 0,
            "non_live_events_count": 0,
            "counted_events": 0,
            "task_success_like_counted_events": 0,
            "answer_like_counted_events": 0,
            "legacy_unverified_events": 0,
            "preliminary": true,
            "baseline_tokens": 0,
            "delivered_tokens": 0,
            "recovery_tokens": 0,
            "observed_client_prompt_tokens": 0,
            "observed_assistant_generation_tokens": 0,
            "observed_tool_overhead_tokens": 0,
            "observed_continuity_restore_tokens": 0,
            "observed_client_prompt_live_events": 0,
            "observed_assistant_generation_live_events": 0,
            "observed_tool_overhead_live_events": 0,
            "observed_continuity_restore_live_events": 0,
            "observed_whole_cycle_with_amai_tokens": 0,
            "verified_observed_whole_cycle_with_amai_tokens": 0,
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
            "coverage": build_coverage_summary(contract, 0, 0, 0, 0, 0),
            "excluded_breakdown": build_excluded_breakdown(contract, &[]),
        });
    }

    let mut tokenizer_cache = HashMap::<String, Option<CoreBPE>>::new();
    let total_saved_tokens = events.iter().map(|event| event.saved_tokens).sum::<u64>();
    let total_naive_tokens = events.iter().map(|event| event.naive_tokens).sum::<u64>();
    let total_context_tokens = events.iter().map(|event| event.context_tokens).sum::<u64>();
    let total_recovery_tokens = events
        .iter()
        .map(|event| event.recovery_tokens)
        .sum::<u64>();
    let observed_client_prompt_tokens = events
        .iter()
        .filter_map(|event| derived_client_prompt_tokens(event, &mut tokenizer_cache))
        .sum::<u64>();
    let observed_assistant_generation_tokens = events
        .iter()
        .filter_map(|event| event.assistant_generation_tokens)
        .sum::<u64>();
    let observed_tool_overhead_tokens = events
        .iter()
        .filter_map(|event| event.tool_overhead_tokens)
        .sum::<u64>();
    let observed_continuity_restore_tokens = events
        .iter()
        .filter_map(|event| event.continuity_restore_tokens)
        .sum::<u64>();
    let live_events_count = events
        .iter()
        .filter(|event| event.traffic_class == "live")
        .count();
    let non_live_events_count = events.len().saturating_sub(live_events_count);
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
    let verified_delivered_tokens = verified_events
        .iter()
        .map(|event| event.context_tokens)
        .sum::<u64>();
    let verified_recovery_tokens = verified_events
        .iter()
        .map(|event| event.recovery_tokens)
        .sum::<u64>();
    let verified_observed_client_prompt_tokens = verified_events
        .iter()
        .filter_map(|event| derived_client_prompt_tokens(event, &mut tokenizer_cache))
        .sum::<u64>();
    let verified_observed_assistant_generation_tokens = verified_events
        .iter()
        .filter_map(|event| event.assistant_generation_tokens)
        .sum::<u64>();
    let verified_observed_tool_overhead_tokens = verified_events
        .iter()
        .filter_map(|event| event.tool_overhead_tokens)
        .sum::<u64>();
    let verified_observed_continuity_restore_tokens = verified_events
        .iter()
        .filter_map(|event| event.continuity_restore_tokens)
        .sum::<u64>();
    let observed_client_prompt_live_events = events
        .iter()
        .filter(|event| {
            event.traffic_class == "live"
                && derived_client_prompt_tokens(event, &mut tokenizer_cache).is_some()
        })
        .count() as u64;
    let observed_assistant_generation_live_events = events
        .iter()
        .filter(|event| {
            event.traffic_class == "live" && event.assistant_generation_tokens.is_some()
        })
        .count() as u64;
    let observed_tool_overhead_live_events = events
        .iter()
        .filter(|event| event.traffic_class == "live" && event.tool_overhead_tokens.is_some())
        .count() as u64;
    let observed_continuity_restore_live_events = events
        .iter()
        .filter(|event| event.traffic_class == "live" && event.continuity_restore_tokens.is_some())
        .count() as u64;
    let observed_whole_cycle_with_amai_tokens = total_context_tokens
        .saturating_add(total_recovery_tokens)
        .saturating_add(observed_client_prompt_tokens)
        .saturating_add(observed_assistant_generation_tokens)
        .saturating_add(observed_tool_overhead_tokens)
        .saturating_add(observed_continuity_restore_tokens);
    let verified_observed_whole_cycle_with_amai_tokens = verified_delivered_tokens
        .saturating_add(verified_recovery_tokens)
        .saturating_add(verified_observed_client_prompt_tokens)
        .saturating_add(verified_observed_assistant_generation_tokens)
        .saturating_add(verified_observed_tool_overhead_tokens)
        .saturating_add(verified_observed_continuity_restore_tokens);
    let excluded_events = events
        .iter()
        .filter(|event| !(event.traffic_class == "live" && event.quality_ok))
        .collect::<Vec<_>>();
    let excluded_effective_saved_tokens = excluded_events
        .iter()
        .map(|event| event.effective_saved_tokens)
        .sum::<i64>();
    let excluded_baseline_tokens = excluded_events
        .iter()
        .map(|event| event.naive_tokens)
        .sum::<u64>();
    let excluded_delivered_tokens = excluded_events
        .iter()
        .map(|event| event.context_tokens)
        .sum::<u64>();
    let excluded_recovery_tokens = excluded_events
        .iter()
        .map(|event| event.recovery_tokens)
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
    let latency_events = events
        .iter()
        .filter(|event| event.measurement_scope == "retrieval_lower_bound")
        .collect::<Vec<_>>();
    let mut latency_values = latency_events
        .iter()
        .map(|event| event.latency_ms)
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    latency_values
        .sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let latency_sample_count = latency_values.len();
    let current_latency_ms = latency_events
        .iter()
        .rev()
        .map(|event| event.latency_ms)
        .find(|value| value.is_finite())
        .unwrap_or_default();
    let p50_latency_ms = percentile_from_sorted(&latency_values, 0.50);
    let p95_latency_ms = percentile_from_sorted(&latency_values, 0.95);
    let p99_latency_ms = percentile_from_sorted(&latency_values, 0.99);
    let max_latency_ms = latency_values.last().copied().unwrap_or_default();
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
    let coverage = build_coverage_summary(
        contract,
        events.len() as u64,
        verified_events.len() as u64,
        excluded_events.len() as u64,
        total_naive_tokens,
        verified_baseline_tokens,
    );
    let excluded_breakdown = build_excluded_breakdown(contract, &excluded_events);

    json!({
        "events_total": events.len(),
        "events_count": events.len(),
        "live_events_count": live_events_count,
        "non_live_events_count": non_live_events_count,
        "counted_events": verified_events.len(),
        "task_success_like_counted_events": task_like_events.len(),
        "answer_like_counted_events": answer_like_events.len(),
        "legacy_unverified_events": legacy_unverified_events,
        "preliminary": preliminary,
        "baseline_tokens": total_naive_tokens,
        "delivered_tokens": total_context_tokens,
        "recovery_tokens": total_recovery_tokens,
        "observed_client_prompt_tokens": observed_client_prompt_tokens,
        "observed_assistant_generation_tokens": observed_assistant_generation_tokens,
        "observed_tool_overhead_tokens": observed_tool_overhead_tokens,
        "observed_continuity_restore_tokens": observed_continuity_restore_tokens,
        "observed_client_prompt_live_events": observed_client_prompt_live_events,
        "observed_assistant_generation_live_events": observed_assistant_generation_live_events,
        "observed_tool_overhead_live_events": observed_tool_overhead_live_events,
        "observed_continuity_restore_live_events": observed_continuity_restore_live_events,
        "observed_whole_cycle_with_amai_tokens": observed_whole_cycle_with_amai_tokens,
        "verified_observed_whole_cycle_with_amai_tokens": verified_observed_whole_cycle_with_amai_tokens,
        "effective_saved_tokens": total_effective_saved_tokens,
        "total_saved_tokens": total_saved_tokens,
        "total_effective_saved_tokens": total_effective_saved_tokens,
        "verified_effective_saved_tokens": verified_effective_saved_tokens,
        "verified_baseline_tokens": verified_baseline_tokens,
        "verified_delivered_tokens": verified_delivered_tokens,
        "verified_recovery_tokens": verified_recovery_tokens,
        "verified_task_like_saved_tokens": verified_task_like_saved_tokens,
        "verified_answer_like_saved_tokens": verified_answer_like_saved_tokens,
        "excluded_events_count": excluded_events.len(),
        "excluded_effective_saved_tokens": excluded_effective_saved_tokens,
        "excluded_baseline_tokens": excluded_baseline_tokens,
        "excluded_delivered_tokens": excluded_delivered_tokens,
        "excluded_recovery_tokens": excluded_recovery_tokens,
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
        "sample_count": latency_sample_count,
        "current_latency_ms": current_latency_ms,
        "p50_latency_ms": p50_latency_ms,
        "p95_latency_ms": p95_latency_ms,
        "p99_latency_ms": p99_latency_ms,
        "max_latency_ms": max_latency_ms,
        "latency_slices": latency_slice_breakdown(events),
        "started_at_epoch_ms": started_at_epoch_ms,
        "ended_at_epoch_ms": ended_at_epoch_ms,
        "age_ms_since_latest": now_epoch_ms.saturating_sub(ended_at_epoch_ms),
        "coverage": coverage,
        "excluded_breakdown": excluded_breakdown,
    })
}

fn build_coverage_summary(
    contract: &TokenBudgetContractConfig,
    measured_events: u64,
    included_events: u64,
    excluded_events: u64,
    measured_baseline_tokens: u64,
    included_baseline_tokens: u64,
) -> Value {
    let excluded_baseline_tokens =
        measured_baseline_tokens.saturating_sub(included_baseline_tokens);
    let event_coverage_pct = percent_share(included_events, measured_events);
    let baseline_token_coverage_pct =
        percent_share(included_baseline_tokens, measured_baseline_tokens);
    let completeness_state = if measured_events == 0 {
        "empty"
    } else if included_events == 0 {
        "no_confirmed_usage"
    } else if included_events == measured_events {
        "fully_confirmed"
    } else {
        "partially_confirmed"
    };
    json!({
        "model_version": contract.coverage_model_version.clone(),
        "completeness_state": completeness_state,
        "measured_events": measured_events,
        "included_events": included_events,
        "excluded_events": excluded_events,
        "event_coverage_pct": event_coverage_pct,
        "measured_baseline_tokens": measured_baseline_tokens,
        "included_baseline_tokens": included_baseline_tokens,
        "excluded_baseline_tokens": excluded_baseline_tokens,
        "baseline_token_coverage_pct": baseline_token_coverage_pct,
    })
}

fn build_excluded_breakdown(
    contract: &TokenBudgetContractConfig,
    excluded_events: &[&TokenBudgetEvent],
) -> Value {
    let mut grouped = BTreeMap::<String, (u64, u64, u64, u64, i64)>::new();
    for event in excluded_events {
        let code = excluded_event_code(event).to_string();
        let entry = grouped.entry(code).or_insert((0, 0, 0, 0, 0));
        entry.0 = entry.0.saturating_add(1);
        entry.1 = entry.1.saturating_add(event.naive_tokens);
        entry.2 = entry.2.saturating_add(event.context_tokens);
        entry.3 = entry.3.saturating_add(event.recovery_tokens);
        entry.4 = entry.4.saturating_add(event.effective_saved_tokens);
    }
    let items = grouped
        .into_iter()
        .map(
            |(
                code,
                (
                    events_count,
                    baseline_tokens,
                    delivered_tokens,
                    recovery_tokens,
                    effective_saved_tokens,
                ),
            )| {
                json!({
                    "code": code,
                    "label": excluded_event_label(&code),
                    "events_count": events_count,
                    "baseline_tokens": baseline_tokens,
                    "delivered_tokens": delivered_tokens,
                    "recovery_tokens": recovery_tokens,
                    "effective_saved_tokens": effective_saved_tokens,
                })
            },
        )
        .collect::<Vec<_>>();
    json!({
        "model_version": contract.excluded_taxonomy_version.clone(),
        "items": items,
    })
}

pub(crate) fn excluded_event_code(event: &TokenBudgetEvent) -> &'static str {
    match event.traffic_class.as_str() {
        "verify" => "synthetic_verify",
        "proof" => "synthetic_proof",
        "benchmark" => "synthetic_benchmark",
        "live" => {
            if event.quality_method == "legacy_unverified" {
                "legacy_unverified"
            } else if event.needed_followup && event.resolved_by_event_id.is_none() {
                "awaiting_followup_reconciliation"
            } else {
                "quality_gate_failed"
            }
        }
        _ => "non_live_other",
    }
}

fn excluded_event_label(code: &str) -> &'static str {
    match code {
        "synthetic_verify" => "engineering verify-событие",
        "synthetic_proof" => "engineering proof-событие",
        "synthetic_benchmark" => "benchmark-событие",
        "legacy_unverified" => "старое live-событие без quality-блока",
        "awaiting_followup_reconciliation" => "ожидает полезного follow-up или подтверждения",
        "quality_gate_failed" => "не прошло quality gate",
        _ => "другое исключённое событие",
    }
}

fn public_savings_projection_pair_text(
    without_amai_tokens: u64,
    with_amai_tokens: u64,
) -> (&'static str, i64, f64, String) {
    let saved_tokens = without_amai_tokens as i64 - with_amai_tokens as i64;
    let saved_pct = saved_tokens as f64 * 100.0 / without_amai_tokens as f64;
    let classification = signed_kpi_classification(saved_pct);
    let value_text = match classification {
        "saving" => format!(
            "Amai savings: без Amai {}, с Amai {}, экономия {} ({:.2}%)",
            format_u64(Some(without_amai_tokens)),
            format_u64(Some(with_amai_tokens)),
            format_u64(Some(saved_tokens as u64)),
            saved_pct.abs()
        ),
        "overspend" => format!(
            "Amai savings: без Amai {}, с Amai {}, перерасход {} ({:.2}%)",
            format_u64(Some(without_amai_tokens)),
            format_u64(Some(with_amai_tokens)),
            format_u64(Some(saved_tokens.unsigned_abs())),
            saved_pct.abs()
        ),
        _ => format!(
            "Amai savings: без Amai {}, с Amai {}, 1:1",
            format_u64(Some(without_amai_tokens)),
            format_u64(Some(with_amai_tokens)),
        ),
    };
    (classification, saved_tokens, saved_pct, value_text)
}

fn public_savings_projection_pair(
    without_amai_tokens: u64,
    with_amai_tokens: u64,
) -> Option<(&'static str, i64, f64, String)> {
    if without_amai_tokens == 0 {
        return None;
    }
    Some(public_savings_projection_pair_text(
        without_amai_tokens,
        with_amai_tokens,
    ))
}

fn product_headline_same_meter_exact_pair_tokens(summary: &Value) -> Option<(u64, u64, i64, f64)> {
    let alignment = &summary["client_limit_meter_alignment"];
    if alignment["same_meter_as_client_limit"].as_bool() != Some(true) {
        return None;
    }
    let without_amai = alignment["strict_client_meter_slice"]["lower_bound_tokens"]
        .as_u64()
        .or_else(|| {
            alignment["baseline_equivalence"]["measured_baseline_tokens_lower_bound"].as_u64()
        })
        .or_else(|| summary["verified_without_amai_measured_tokens"].as_u64())
        .or_else(|| summary["verified_baseline_tokens"].as_u64())?;
    let with_amai = summary["observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .or_else(|| summary["verified_observed_whole_cycle_with_amai_tokens"].as_u64())
        .or_else(|| summary["verified_with_amai_measured_tokens"].as_u64())
        .or_else(|| summary["with_amai_measured_tokens"].as_u64())?;
    let (_, saved_tokens, saved_pct, _) = public_savings_projection_pair(without_amai, with_amai)?;
    Some((without_amai, with_amai, saved_tokens, saved_pct))
}

fn verified_summary_pair_tokens(summary: &Value) -> Option<(u64, u64, i64, f64)> {
    let without_amai = summary["verified_baseline_tokens"].as_u64()?;
    let with_amai = summary["verified_observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .or_else(|| summary["verified_with_amai_measured_tokens"].as_u64())?;
    let (_, saved_tokens, saved_pct, _) = public_savings_projection_pair(without_amai, with_amai)?;
    Some((without_amai, with_amai, saved_tokens, saved_pct))
}

fn measured_summary_pair_tokens(summary: &Value) -> Option<(u64, u64, i64, f64)> {
    let without_amai = summary["baseline_tokens"].as_u64()?;
    let with_amai = summary["observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .or_else(|| summary["with_amai_measured_tokens"].as_u64())?;
    let (_, saved_tokens, saved_pct, _) = public_savings_projection_pair(without_amai, with_amai)?;
    Some((without_amai, with_amai, saved_tokens, saved_pct))
}

fn build_public_savings_projection(summary: &Value, scope_label: &str) -> Value {
    let events_total = summary["events_total"].as_u64().unwrap_or(0);
    let counted_events = summary["counted_events"].as_u64().unwrap_or(0);
    let coverage = if summary["coverage"].is_object() {
        summary["coverage"].clone()
    } else {
        Value::Null
    };

    let observed_projection = |confidence: &'static str,
                               pair_source: &'static str,
                               same_meter_correlated: bool,
                               without_amai_tokens: u64,
                               with_amai_tokens: u64,
                               saved_tokens: i64,
                               saved_pct: f64,
                               status: &'static str| {
        let (classification, _, _, value_text) =
            public_savings_projection_pair_text(without_amai_tokens, with_amai_tokens);
        json!({
            "metric_domain": PUBLIC_SAVINGS_METRIC_DOMAIN,
            "metric_code": PUBLIC_SAVINGS_METRIC_CODE,
            "projection_contract_version": PUBLIC_SAVINGS_PROJECTION_CONTRACT_VERSION,
            "scope_label": scope_label,
            "status": status,
            "preliminary": status == PUBLIC_SAVINGS_PROJECTION_STATUS_PRELIMINARY,
            "classification": classification,
            "confidence": confidence,
            "pair_source": pair_source,
            "same_meter_correlated": same_meter_correlated,
            "without_amai_tokens": without_amai_tokens,
            "with_amai_tokens": with_amai_tokens,
            "saved_tokens": saved_tokens,
            "saved_pct": saved_pct,
            "events_count": events_total,
            "counted_events": counted_events,
            "coverage": coverage.clone(),
            "value_text": value_text,
        })
    };

    if let Some((without_amai_tokens, with_amai_tokens, saved_tokens, saved_pct)) =
        product_headline_same_meter_exact_pair_tokens(summary)
    {
        return observed_projection(
            if counted_events > 0 {
                PUBLIC_SAVINGS_PROJECTION_CONFIDENCE_EXACT_SAME_METER_VERIFIED
            } else {
                PUBLIC_SAVINGS_PROJECTION_CONFIDENCE_EXACT_SAME_METER
            },
            PUBLIC_SAVINGS_PROJECTION_PAIR_SOURCE_EXACT_SAME_METER,
            true,
            without_amai_tokens,
            with_amai_tokens,
            saved_tokens,
            saved_pct,
            PUBLIC_SAVINGS_PROJECTION_STATUS_OBSERVED,
        );
    }

    if let Some((without_amai_tokens, with_amai_tokens, saved_tokens, saved_pct)) =
        verified_summary_pair_tokens(summary)
    {
        return observed_projection(
            PUBLIC_SAVINGS_PROJECTION_CONFIDENCE_VERIFIED_SUMMARY,
            PUBLIC_SAVINGS_PROJECTION_PAIR_SOURCE_VERIFIED_SUMMARY,
            false,
            without_amai_tokens,
            with_amai_tokens,
            saved_tokens,
            saved_pct,
            PUBLIC_SAVINGS_PROJECTION_STATUS_OBSERVED,
        );
    }

    if let Some((without_amai_tokens, with_amai_tokens, saved_tokens, saved_pct)) =
        measured_summary_pair_tokens(summary)
    {
        return observed_projection(
            PUBLIC_SAVINGS_PROJECTION_CONFIDENCE_PRELIMINARY_SUMMARY,
            PUBLIC_SAVINGS_PROJECTION_PAIR_SOURCE_MEASURED_SUMMARY,
            false,
            without_amai_tokens,
            with_amai_tokens,
            saved_tokens,
            saved_pct,
            PUBLIC_SAVINGS_PROJECTION_STATUS_PRELIMINARY,
        );
    }

    json!({
        "metric_domain": PUBLIC_SAVINGS_METRIC_DOMAIN,
        "metric_code": PUBLIC_SAVINGS_METRIC_CODE,
        "projection_contract_version": PUBLIC_SAVINGS_PROJECTION_CONTRACT_VERSION,
        "scope_label": scope_label,
        "status": PUBLIC_SAVINGS_PROJECTION_STATUS_MISSING,
        "preliminary": false,
        "classification": "missing",
        "confidence": PUBLIC_SAVINGS_PROJECTION_CONFIDENCE_MISSING,
        "pair_source": PUBLIC_SAVINGS_PROJECTION_PAIR_SOURCE_MISSING,
        "same_meter_correlated": false,
        "without_amai_tokens": Value::Null,
        "with_amai_tokens": Value::Null,
        "saved_tokens": Value::Null,
        "saved_pct": Value::Null,
        "events_count": events_total,
        "counted_events": counted_events,
        "coverage": coverage,
        "value_text": "Amai savings: н/д",
    })
}

#[cfg(test)]
pub(crate) fn build_product_headline(
    summary: &Value,
    scope_label: &str,
    client_limit_boundary_semantics: Option<&Value>,
) -> Value {
    build_product_headline_with_target(
        summary,
        scope_label,
        client_limit_boundary_semantics,
        working_state::default_client_budget_target_percent(),
    )
}

pub(crate) fn build_product_headline_with_target(
    summary: &Value,
    scope_label: &str,
    client_limit_boundary_semantics: Option<&Value>,
    client_budget_target_percent: u64,
) -> Value {
    let events_total = summary["events_total"].as_u64().unwrap_or(0);
    let counted_events = summary["counted_events"].as_u64().unwrap_or(0);
    let projection = build_public_savings_projection(summary, scope_label);
    let projection_status = projection["status"]
        .as_str()
        .unwrap_or(PUBLIC_SAVINGS_PROJECTION_STATUS_MISSING);
    let projection_preliminary = projection["preliminary"].as_bool() == Some(true);
    let classification = projection["classification"].as_str().unwrap_or("missing");
    let status_bar_correlated = projection["same_meter_correlated"].as_bool() == Some(true);
    let target_percent = client_budget_target_percent as f64;
    let target_active = client_budget_target_percent > 0;
    let projection_value_percent = projection["saved_pct"].as_f64();
    let below_status_bar_target = target_active
        && status_bar_correlated
        && projection_value_percent.is_some_and(|value| value < target_percent);
    let client_limit_boundary_semantics =
        client_limit_boundary_semantics.cloned().unwrap_or_else(|| {
            if summary["client_limit_meter_alignment"].is_object() {
                build_client_limit_boundary_review_surface(summary)
            } else {
                Value::Null
            }
        });
    let boundary_note = match client_limit_boundary_semantics["review_state"]
        .as_str()
        .unwrap_or("client_limit_boundary_review_unknown")
    {
        "strict_slice_plus_observed_amai_continuity_boundary" => Some(
            "При этом headline не равен клиентскому лимиту: measured strict slice уже materialized, а observed Amai continuity boundary вынесена отдельно.",
        ),
        "strict_slice_plus_empty_amai_continuity_boundary" => Some(
            "При этом headline не равен клиентскому лимиту: explicit Amai continuity boundary уже объявлена отдельно, но в текущем scope у неё ещё нет observed token weight.",
        ),
        "amai_continuity_boundary_present" => Some(
            "При этом headline не равен клиентскому лимиту: в scope остаётся explicit Amai continuity boundary вне strict client-meter slice.",
        ),
        "strict_slice_partial_without_explicit_boundary" => Some(
            "При этом headline не должен читаться как полный client-limit meter: strict same-meter slice пока покрывает только часть applicable components.",
        ),
        _ => None,
    };
    let frozen_gap_note = if client_limit_boundary_semantics["frozen_gap_review_surface"]["state"]
        .as_str()
        == Some("review_required")
    {
        Some(
            "Raw exact history здесь дополнительно unavailable: irrecoverable historical debt уже требует отдельного frozen-gap review.",
        )
    } else {
        None
    };
    let annotate_note = |base: &str| -> String {
        let mut note = base.to_string();
        if let Some(extra) = boundary_note {
            note.push(' ');
            note.push_str(extra);
        }
        if let Some(extra) = frozen_gap_note {
            note.push(' ');
            note.push_str(extra);
        }
        note
    };
    let operator_client_budget_context = json!({
        "metric_domain": OPERATOR_GUARD_METRIC_DOMAIN,
        "visibility": "operator_only",
        "context_contract_version": OPERATOR_CLIENT_BUDGET_CONTEXT_CONTRACT_VERSION,
        "target_percent": target_percent,
        "same_meter_correlated": status_bar_correlated,
        "below_target": below_status_bar_target,
        "target_met": status_bar_correlated && !below_status_bar_target,
    });
    let base_note = match projection_status {
        PUBLIC_SAVINGS_PROJECTION_STATUS_OBSERVED
            if status_bar_correlated && counted_events > 0 =>
        {
            "Это честная Amai savings-пара по exact same-meter шкале: provider-independent, quality-gated и считающая без Amai и с Amai по одной и той же шкале токенов модели пользователя."
        }
        PUBLIC_SAVINGS_PROJECTION_STATUS_OBSERVED if status_bar_correlated => {
            "Это честная Amai savings-пара по exact same-meter шкале: она уже materialized по real model meter, даже если quality-gated confirmed lane для этого scope ещё пуст."
        }
        PUBLIC_SAVINGS_PROJECTION_STATUS_OBSERVED => {
            "Это честная Amai savings-пара по подтверждённой части live summary: provider-independent и quality-gated, но не притворяется client-limit same-meter шкалой."
        }
        PUBLIC_SAVINGS_PROJECTION_STATUS_PRELIMINARY if classification == "overspend" => {
            "Это честная Amai savings-пара пока предварительная и уже показывает перерасход: без Amai и с Amai посчитаны, но final quality-gated или exact same-meter подтверждение для этого scope ещё не materialized."
        }
        PUBLIC_SAVINGS_PROJECTION_STATUS_PRELIMINARY => {
            "Это честная Amai savings-пара пока предварительная: без Amai и с Amai уже посчитаны, но final quality-gated или exact same-meter состояние для этого scope ещё не materialized."
        }
        _ if events_total > 0 => {
            "Live-события уже есть, но честная savings-пара без Amai/с Amai для этого scope ещё не materialized."
        }
        _ => "Amai ещё не накопил live-события для этой честной savings-пары.",
    };
    json!({
        "headline_contract_version": PUBLIC_PRODUCT_HEADLINE_CONTRACT_VERSION,
        "metric_domain": PUBLIC_SAVINGS_METRIC_DOMAIN,
        "metric_code": PUBLIC_SAVINGS_METRIC_CODE,
        "title": match projection_status {
            PUBLIC_SAVINGS_PROJECTION_STATUS_OBSERVED => "Честная экономия Amai",
            PUBLIC_SAVINGS_PROJECTION_STATUS_PRELIMINARY => "Честная экономия Amai пока предварительная",
            _ => "Честная экономия Amai пока не накоплена",
        },
        "scope_label": scope_label,
        "classification": projection["classification"].clone(),
        "confidence": projection["confidence"].clone(),
        "status": match projection_status {
            PUBLIC_SAVINGS_PROJECTION_STATUS_OBSERVED if classification == "overspend" => "alert",
            PUBLIC_SAVINGS_PROJECTION_STATUS_OBSERVED => "pass",
            PUBLIC_SAVINGS_PROJECTION_STATUS_PRELIMINARY => "waiting",
            _ => "unknown",
        },
        "preliminary": projection_preliminary,
        "value_percent": projection["saved_pct"].clone(),
        "saved_tokens": projection["saved_tokens"].clone(),
        "value_text": projection["value_text"].clone(),
        "events_count": events_total,
        "counted_events": counted_events,
        "client_limit_boundary_semantics": client_limit_boundary_semantics,
        "operator_client_budget_context": operator_client_budget_context,
        "public_savings_projection": projection,
        "note": annotate_note(base_note),
    })
}

#[cfg(test)]
pub(crate) fn scope_same_meter_exact_pair(scope_summary: &Value) -> Option<(u64, u64, i64, f64)> {
    product_headline_same_meter_exact_pair_tokens(scope_summary)
}

fn current_live_turn_alignment_component<'a>(
    scope_summary: &'a Value,
    code: &str,
) -> Option<&'a Value> {
    scope_summary["client_limit_meter_alignment"]["baseline_equivalence"]["component_semantics"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["code"].as_str() == Some(code))
}

pub(crate) fn current_live_turn_full_turn_exact_pair(
    scope_summary: &Value,
    observation: &codex_threads::RolloutClientMeterObservation,
) -> Option<(u64, u64, i64, f64)> {
    let with_amai_total_tokens = observation.client_turn_total_tokens;
    if with_amai_total_tokens == 0 {
        return None;
    }

    let retrieval_without_amai_tokens = scope_summary["without_amai_measured_tokens"].as_u64()?;
    let retrieval_with_amai_tokens = scope_summary["with_amai_measured_tokens"].as_u64()?;
    let observed_tool_overhead_tokens = scope_summary["observed_tool_overhead_tokens"]
        .as_u64()
        .unwrap_or(0);
    let observed_continuity_restore_tokens = scope_summary["observed_continuity_restore_tokens"]
        .as_u64()
        .unwrap_or(0);

    if let Some(component) =
        current_live_turn_alignment_component(scope_summary, "tool_overhead_outside_retrieval")
    {
        let target_live_events = component["target_live_events_count"].as_u64().unwrap_or(0);
        if target_live_events > 0
            && component["whole_cycle_observed_complete"].as_bool() != Some(true)
        {
            return None;
        }
    }

    let continuity_without_amai_tokens = if observed_continuity_restore_tokens == 0 {
        0
    } else {
        let component = current_live_turn_alignment_component(
            scope_summary,
            "continuity_restore_outside_retrieval",
        )?;
        if component["whole_cycle_observed_complete"].as_bool() != Some(true) {
            return None;
        }
        component["baseline_measured_tokens"].as_u64()?
    };

    let saved_tokens = retrieval_without_amai_tokens as i64
        - retrieval_with_amai_tokens as i64
        - observed_tool_overhead_tokens as i64
        + continuity_without_amai_tokens as i64
        - observed_continuity_restore_tokens as i64;
    let without_amai_total_tokens = if saved_tokens >= 0 {
        with_amai_total_tokens.saturating_add(saved_tokens as u64)
    } else {
        with_amai_total_tokens.saturating_sub(saved_tokens.unsigned_abs())
    };
    if without_amai_total_tokens == 0 {
        return None;
    }
    let saved_pct = saved_tokens as f64 * 100.0 / without_amai_total_tokens as f64;
    Some((
        without_amai_total_tokens,
        with_amai_total_tokens,
        saved_tokens,
        saved_pct,
    ))
}

pub(crate) fn event_to_json(event: &TokenBudgetEvent) -> Value {
    let excluded_reason_code = usage_excluded_reason_code(event);
    let mut object = serde_json::Map::new();
    let whole_cycle_observed = json!({
        "client_prompt_tokens": event.client_prompt_tokens,
        "assistant_generation_tokens": event.assistant_generation_tokens,
        "tool_overhead_tokens": event.tool_overhead_tokens,
        "continuity_restore_tokens": event.continuity_restore_tokens,
    });
    let recovery = json!({
        "recovery_tokens": event.recovery_tokens,
        "fallback_triggered": event.fallback_triggered,
        "fallback_count": event.fallback_count,
    });
    let quality = json!({
        "quality_ok": event.quality_ok,
        "quality_score": event.quality_score,
        "quality_method": event.quality_method.clone(),
        "quality_tier": event.quality_tier.clone(),
        "head_hit_target": event.head_hit_target,
    });
    let followup = json!({
        "needed_followup": event.needed_followup,
        "followup_count": event.followup_count,
        "followup_of_event_id": event
            .followup_of_event_id
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
        "resolved_by_event_id": event
            .resolved_by_event_id
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    });
    let shape = json!({
        "document_hits": event.document_hits,
        "symbol_hits": event.symbol_hits_count,
        "file_hits": event.file_hits,
        "sources_count": event.sources_count,
        "chunks_count": event.chunks_count,
        "pack_token_count": event.pack_token_count,
        "deduped_token_count": event.deduped_token_count,
    });
    let savings = json!({
        "saved_tokens": event.saved_tokens,
        "effective_saved_tokens": event.effective_saved_tokens,
        "savings_factor": event.savings_factor,
        "savings_percent": event.savings_percent,
        "effective_savings_percent": event.effective_savings_percent,
    });
    let accounting = build_tokenomics_accounting_json(
        build_tokenomics_raw_section(
            event.naive_tokens,
            event.context_tokens,
            event.recovery_tokens,
            whole_cycle_observed.clone(),
            recovery.clone(),
            shape.clone(),
        ),
        build_tokenomics_policy_section(quality.clone(), followup.clone()),
        build_tokenomics_projection_section(savings.clone()),
    );
    object.insert(
        "created_at_epoch_ms".to_string(),
        Value::from(event.created_at_epoch_ms),
    );
    object.insert(
        "event_id".to_string(),
        Value::String(event.event_id.clone()),
    );
    object.insert(
        "correlation_id".to_string(),
        Value::String(event.correlation_id.clone()),
    );
    object.insert(
        "context_pack_id".to_string(),
        event
            .context_pack_id
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "thread_id".to_string(),
        event
            .thread_id
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "turn_id".to_string(),
        event
            .turn_id
            .as_ref()
            .map(|value| Value::String(value.clone()))
            .unwrap_or(Value::Null),
    );
    object.insert(
        "payload_origin".to_string(),
        Value::String(event.payload_origin.clone()),
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
        "occurred_at_epoch_ms".to_string(),
        Value::from(event.occurred_at_epoch_ms),
    );
    object.insert(
        "ingested_at_epoch_ms".to_string(),
        Value::from(event.ingested_at_epoch_ms),
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
    object.insert(
        "measurement_scope".to_string(),
        Value::String(event.measurement_scope.clone()),
    );
    object.insert(
        "contract".to_string(),
        json!({
            "usage_event_schema_version": event.usage_event_schema_version.clone(),
            "settlement_statement_version": event.settlement_statement_version.clone(),
            "metering_event_schema_version": event.metering_event_schema_version.clone(),
            "usage_lifecycle_model_version": event.usage_lifecycle_model_version.clone(),
            "baseline_method_version": event.baseline_method_version.clone(),
            "quality_method_version": event.quality_method_version.clone(),
            "coverage_model_version": event.coverage_model_version.clone(),
            "metering_freshness_model_version": event.metering_freshness_model_version.clone(),
            "excluded_taxonomy_version": event.excluded_taxonomy_version.clone(),
            "dedup_contract_version": event.dedup_contract_version.clone(),
            "backfill_policy_version": event.backfill_policy_version.clone(),
            "correction_policy_version": event.correction_policy_version.clone(),
            "freeze_close_policy_version": event.freeze_close_policy_version.clone(),
            "late_arrival_policy_version": event.late_arrival_policy_version.clone(),
            "dispute_policy_version": event.dispute_policy_version.clone(),
            "settlement_lifecycle_model_version": event.settlement_lifecycle_model_version.clone(),
            "statement_period_governance_version": event.statement_period_governance_version.clone(),
            "adjustment_preview_model_version": event.adjustment_preview_model_version.clone(),
            "adjustment_request_schema_version": event.adjustment_request_schema_version.clone(),
            "adjustment_registry_version": event.adjustment_registry_version.clone(),
            "rate_card_binding_model_version": event.rate_card_binding_model_version.clone(),
            "telemetry_surface_split_version": event.telemetry_surface_split_version.clone(),
            "event_time_policy_version": event.event_time_policy_version.clone(),
            "billing_policy_version": event.billing_policy_version.clone(),
            "suitability_model_version": event.suitability_model_version.clone(),
            "billing_mode": event.billing_mode.clone(),
            "reconciliation_contract_version": event.reconciliation_contract_version.clone(),
            "margin_model_version": event.margin_model_version.clone(),
            "infra_cost_profile_version": event.infra_cost_profile_version.clone(),
            "contractual_evidence_pack_version": event.contractual_evidence_pack_version.clone(),
            "rate_card_version": event.rate_card_version.clone(),
            "currency_profile": event.currency_profile.clone(),
            "settlement_status": event.settlement_status.clone(),
        }),
    );
    object.insert(
        "usage_identity".to_string(),
        json!({
            "dedup_key": usage_dedup_key(&event.source_kind, &event.event_id),
            "idempotency_scope": "source_kind + event_id",
            "canonical_window_time_field": "occurred_at_epoch_ms",
            "event_id": event.event_id.clone(),
            "correlation_id": event.correlation_id.clone(),
        }),
    );
    object.insert(
        "usage_state".to_string(),
        json!({
            "lifecycle_status": usage_lifecycle_status(event),
            "reporting_layer": usage_reporting_layer(event),
            "included_in_verified_rollup": excluded_reason_code.is_none(),
            "excluded_reason_code": excluded_reason_code,
            "backfill_status": usage_backfill_status(event),
            "settlement_status": event.settlement_status.clone(),
        }),
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
    object.insert(
        "whole_cycle_observed".to_string(),
        whole_cycle_observed.clone(),
    );
    if let Some(source) = &event.tool_overhead_source {
        object.insert(
            "whole_cycle_observed_source".to_string(),
            json!({
                "tool_overhead": source.clone(),
            }),
        );
    }
    if let Some(source) = &event.pre_amai_baseline_source {
        object.insert("pre_amai_baseline_source".to_string(), source.clone());
    }
    object.insert("recovery".to_string(), recovery.clone());
    object.insert("quality".to_string(), quality.clone());
    object.insert("followup".to_string(), followup.clone());
    object.insert("shape".to_string(), shape.clone());
    object.insert("savings".to_string(), savings.clone());
    object.insert("accounting".to_string(), accounting);
    Value::Object(object)
}

pub(crate) fn build_event_payload(
    payload: &Value,
    measurement: &MeasurementConfig,
    contract: &TokenBudgetContractConfig,
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
    let context_pack_id = payload["context_pack_id"].as_str().map(ToOwned::to_owned);
    let event_id = Uuid::new_v4().to_string();
    let timestamp_utc = current_epoch_ms()?;
    let correlation_id = context_pack_id.clone().unwrap_or_else(|| event_id.clone());
    let latency_ms = total_latency_ms(payload);
    let whole_cycle_observed = &payload["whole_cycle_observed"];
    let client_prompt_tokens = whole_cycle_observed["client_prompt_tokens"]
        .as_u64()
        .or_else(|| {
            if query.is_empty() {
                None
            } else {
                Some(tokenizer.encode_with_special_tokens(query).len() as u64)
            }
        });
    let assistant_generation_tokens = whole_cycle_observed["assistant_generation_tokens"].as_u64();
    let tool_overhead_tokens = whole_cycle_observed["tool_overhead_tokens"].as_u64();
    let continuity_restore_tokens = whole_cycle_observed["continuity_restore_tokens"].as_u64();
    let whole_cycle_observed = json!({
        "client_prompt_tokens": client_prompt_tokens,
        "assistant_generation_tokens": assistant_generation_tokens,
        "tool_overhead_tokens": tool_overhead_tokens,
        "continuity_restore_tokens": continuity_restore_tokens,
    });
    let recovery = json!({
        "recovery_tokens": recovery_tokens,
        "fallback_triggered": fallback_triggered,
        "fallback_count": fallback_count,
    });
    let quality_json = json!({
        "quality_ok": quality.quality_ok,
        "quality_score": quality.quality_score,
        "quality_method": quality.quality_method,
        "quality_tier": quality.quality_tier,
        "head_hit_target": quality.head_hit_target,
    });
    let followup = json!({
        "needed_followup": quality.needed_followup,
        "followup_count": quality.followup_count,
        "followup_of_event_id": Value::Null,
        "resolved_by_event_id": Value::Null,
    });
    let shape = json!({
        "document_hits": document_hits,
        "symbol_hits": symbol_hits,
        "file_hits": file_hits,
        "sources_count": sources_count,
        "chunks_count": chunks_count,
        "pack_token_count": context_tokens,
        "deduped_token_count": context_tokens,
    });
    let savings = json!({
        "saved_tokens": saved_tokens,
        "effective_saved_tokens": effective_saved_tokens,
        "savings_factor": savings_factor,
        "savings_percent": savings_percent,
        "effective_savings_percent": effective_savings_percent,
    });
    let accounting = build_tokenomics_accounting_json(
        build_tokenomics_raw_section(
            naive_tokens as u64,
            context_tokens as u64,
            recovery_tokens,
            whole_cycle_observed.clone(),
            recovery.clone(),
            shape.clone(),
        ),
        build_tokenomics_policy_section(quality_json.clone(), followup.clone()),
        build_tokenomics_projection_section(savings.clone()),
    );

    Ok(json!({
        "token_budget_event": {
            "event_id": event_id,
            "correlation_id": correlation_id,
            "context_pack_id": context_pack_id,
            "timestamp_utc": timestamp_utc,
            "occurred_at_epoch_ms": timestamp_utc,
            "ingested_at_epoch_ms": timestamp_utc,
            "source_kind": source_kind,
            "traffic_class": traffic_class,
            "measurement_scope": "retrieval_lower_bound",
            "payload_origin": payload_origin,
            "contract": token_contract_metadata_json(contract),
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
            "retrieval_runtime": compact_token_budget_retrieval_runtime(&payload["retrieval_runtime"]),
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
            "whole_cycle_observed": whole_cycle_observed.clone(),
            "recovery": recovery.clone(),
            "quality": quality_json.clone(),
            "followup": followup.clone(),
            "shape": shape.clone(),
            "savings": savings.clone(),
            "accounting": accounting,
        }
    }))
}

pub(crate) fn derive_traffic_class(source_kind: &str) -> String {
    if source_kind.starts_with("live_")
        || source_kind.starts_with("operator_continuity_")
        || source_kind == "operator_client_budget_target"
    {
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

pub(crate) fn normalize_token_event_traffic_class(
    raw_traffic_class: Option<&str>,
    source_kind: &str,
) -> String {
    let derived = derive_traffic_class(source_kind);
    let raw = raw_traffic_class
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match raw {
        Some("unknown") if derived != "unknown" => derived,
        Some(value) => value.to_string(),
        None => derived,
    }
}

pub(crate) fn include_traffic_class_in_report(
    traffic_class: &str,
    include_verify_events: bool,
) -> bool {
    include_verify_events || traffic_class == "live"
}

pub(crate) fn derive_baseline_strategy(query_type: &str) -> &'static str {
    match query_type {
        "onboarding_query" => "legacy_pre_amai",
        "config_lookup" | "symbol_lookup" | "code_lookup" => "ide_search_top_files",
        "docs_lookup" | "cross_file_trace" => "grep_top_files",
        "architecture_question" | "bugfix_context" => "semantic_top_k",
        _ => "naive_top_files",
    }
}

pub(crate) fn derive_query_type(query: &str) -> &'static str {
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

pub(crate) fn derive_quality_verdict(
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

pub(crate) fn answer_like_from_counts(
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

pub(crate) fn is_answer_like_event(event: &TokenBudgetEvent) -> bool {
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

pub(crate) fn current_epoch_ms() -> Result<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64)
}

fn total_latency_ms(payload: &Value) -> f64 {
    let runtime = &payload["retrieval_runtime"];
    if let Some(value) = runtime["retrieval_lower_bound_ms"].as_f64() {
        return value;
    }
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

#[cfg(test)]
mod latency_measurement_tests {
    use super::total_latency_ms;
    use serde_json::json;

    #[test]
    fn total_latency_prefers_retrieval_lower_bound_when_present() {
        let payload = json!({
            "retrieval_runtime": {
                "retrieval_lower_bound_ms": 7.0,
                "total_ms": 99.0,
                "resolve_scope_ms": 1.0,
                "cache_lookup_ms": 1.0
            }
        });
        assert_eq!(total_latency_ms(&payload), 7.0);
    }
}
