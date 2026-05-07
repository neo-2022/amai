use super::*;
use crate::codex_threads;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct CodexAppServerRateLimitsResponse {
    #[serde(rename = "rateLimits")]
    pub(crate) rate_limits: CodexAppServerRateLimitSnapshot,
    #[serde(rename = "rateLimitsByLimitId")]
    pub(crate) rate_limits_by_limit_id: Option<BTreeMap<String, CodexAppServerRateLimitSnapshot>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CodexAppServerRateLimitSnapshot {
    #[serde(rename = "limitId")]
    pub(crate) limit_id: Option<String>,
    #[serde(rename = "limitName")]
    pub(crate) limit_name: Option<String>,
    pub(crate) primary: Option<CodexAppServerRateLimitWindow>,
    pub(crate) secondary: Option<CodexAppServerRateLimitWindow>,
    pub(crate) credits: Option<CodexAppServerCreditsSnapshot>,
    #[serde(rename = "planType")]
    pub(crate) plan_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CodexAppServerRateLimitWindow {
    #[serde(
        rename = "usedPercent",
        deserialize_with = "deserialize_f64_from_number_or_string"
    )]
    pub(crate) used_percent: f64,
    #[serde(
        rename = "windowDurationMins",
        deserialize_with = "deserialize_option_u64_from_number_or_string"
    )]
    pub(crate) window_duration_mins: Option<u64>,
    #[serde(
        rename = "resetsAt",
        deserialize_with = "deserialize_option_u64_from_number_or_string"
    )]
    pub(crate) resets_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CodexAppServerCreditsSnapshot {
    #[serde(rename = "hasCredits")]
    pub(crate) has_credits: bool,
    pub(crate) unlimited: bool,
    #[serde(deserialize_with = "deserialize_option_f64_from_number_or_string")]
    pub(crate) balance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CodexAppServerRateLimitsObservation {
    pub(crate) observed_at_epoch_ms: u64,
    pub(crate) rate_limits: CodexAppServerRateLimitSnapshot,
}

#[derive(Debug, Clone)]
pub(crate) struct ExactClientLimitSample {
    pub(crate) observed_at_epoch_ms: u64,
    pub(crate) primary_used_percent: f64,
    pub(crate) primary_window_duration_mins: Option<u64>,
    pub(crate) primary_resets_at_epoch_seconds: Option<u64>,
    pub(crate) source: String,
}

pub(crate) fn deserialize_f64_from_number_or_string<'de, D>(
    deserializer: D,
) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    parse_f64_from_json_value(&value).ok_or_else(|| {
        de::Error::custom(format!(
            "expected numeric JSON number or numeric string, got {}",
            value
        ))
    })
}

pub(crate) fn deserialize_option_f64_from_number_or_string<'de, D>(
    deserializer: D,
) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    parse_f64_from_json_value(&value).map(Some).ok_or_else(|| {
        de::Error::custom(format!(
            "expected null, numeric JSON number, or numeric string, got {}",
            value
        ))
    })
}

pub(crate) fn deserialize_option_u64_from_number_or_string<'de, D>(
    deserializer: D,
) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    if value.is_null() {
        return Ok(None);
    }
    parse_u64_from_json_value(&value).map(Some).ok_or_else(|| {
        de::Error::custom(format!(
            "expected null, integer JSON number, or integer string, got {}",
            value
        ))
    })
}

fn parse_f64_from_json_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn parse_u64_from_json_value(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    }
}

fn client_limit_meter_alignment_counts(
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
) -> (u64, u64, u64, u64) {
    let events_total = summary["events_total"]
        .as_u64()
        .or_else(|| events.map(|items| items.len() as u64))
        .unwrap_or(0);
    let live_events_count = summary["live_events_count"]
        .as_u64()
        .or_else(|| {
            events.map(|items| {
                items
                    .iter()
                    .filter(|event| event.traffic_class == "live")
                    .count() as u64
            })
        })
        .unwrap_or(0);
    let non_live_events_count = summary["non_live_events_count"]
        .as_u64()
        .or_else(|| events_total.checked_sub(live_events_count))
        .unwrap_or(0);
    let counted_events = summary["meter_counted_events"]
        .as_u64()
        .or_else(|| summary["counted_events"].as_u64())
        .or_else(|| {
            events.map(|items| {
                items
                    .iter()
                    .filter(|event| event.traffic_class == "live" && event.quality_ok)
                    .count() as u64
            })
        })
        .unwrap_or(0);
    (
        events_total,
        live_events_count,
        non_live_events_count,
        counted_events,
    )
}

fn tool_overhead_observed_stats(
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> (u64, u64) {
    let Some(events) = events else {
        return (
            summary["observed_tool_overhead_live_events"]
                .as_u64()
                .unwrap_or(0),
            summary["observed_tool_overhead_tokens"]
                .as_u64()
                .unwrap_or(0),
        );
    };
    let target_events = tool_overhead_target_events(events, assistant_scope);
    let observed_live_events = target_events
        .iter()
        .filter(|event| event.tool_overhead_tokens.is_some())
        .count() as u64;
    let observed_tokens = target_events
        .iter()
        .filter_map(|event| event.tool_overhead_tokens)
        .sum::<u64>();
    (observed_live_events, observed_tokens)
}

fn client_limit_component_stats(
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> [(&'static str, u64, u64); 4] {
    let (observed_tool_overhead_live_events, observed_tool_overhead_tokens) =
        tool_overhead_observed_stats(summary, events, assistant_scope);
    [
        (
            "client_prompt",
            summary["observed_client_prompt_live_events"]
                .as_u64()
                .unwrap_or(0),
            summary["observed_client_prompt_tokens"]
                .as_u64()
                .unwrap_or(0),
        ),
        (
            "assistant_generation",
            assistant_scope
                .map(|scope| scope.observed_group_count)
                .unwrap_or_else(|| {
                    summary["observed_assistant_generation_live_events"]
                        .as_u64()
                        .unwrap_or(0)
                }),
            assistant_scope
                .map(|scope| scope.observed_tokens)
                .unwrap_or_else(|| {
                    summary["observed_assistant_generation_tokens"]
                        .as_u64()
                        .unwrap_or(0)
                }),
        ),
        (
            "tool_overhead_outside_retrieval",
            observed_tool_overhead_live_events,
            observed_tool_overhead_tokens,
        ),
        (
            "continuity_restore_outside_retrieval",
            summary["observed_continuity_restore_live_events"]
                .as_u64()
                .unwrap_or(0),
            summary["observed_continuity_restore_tokens"]
                .as_u64()
                .unwrap_or(0),
        ),
    ]
}

fn client_limit_component_target_scope_kind(code: &str) -> &'static str {
    match code {
        "client_prompt" => "all_live_scope",
        "assistant_generation" => "assistant_generation_turn_scope",
        "tool_overhead_outside_retrieval" => "retrieval_live_scope",
        "continuity_restore_outside_retrieval" => "continuity_restore_live_scope",
        _ => "all_live_scope",
    }
}

fn is_client_limit_component_target_event(code: &str, event: &TokenBudgetEvent) -> bool {
    if event.traffic_class != "live" {
        return false;
    }
    match code {
        "client_prompt" => true,
        "assistant_generation" | "tool_overhead_outside_retrieval" => {
            event.measurement_scope == "retrieval_lower_bound"
        }
        "continuity_restore_outside_retrieval" => {
            event.measurement_scope == "whole_cycle_observed_lower_bound"
                && (event.query_type == "continuity_restore"
                    || event.target_kind == "continuity_restore")
        }
        _ => false,
    }
}

pub(crate) fn client_limit_component_target_live_events(
    code: &str,
    events: Option<&[TokenBudgetEvent]>,
    live_events_count: u64,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> u64 {
    if code == "assistant_generation" {
        return assistant_scope
            .map(|scope| scope.target_group_count)
            .unwrap_or_else(|| {
                events
                    .map(|items| {
                        items
                            .iter()
                            .filter(|event| is_client_limit_component_target_event(code, event))
                            .count() as u64
                    })
                    .unwrap_or(live_events_count)
            });
    }
    events
        .map(|items| {
            items
                .iter()
                .filter(|event| {
                    if !is_client_limit_component_target_event(code, event) {
                        return false;
                    }
                    if code != "tool_overhead_outside_retrieval" {
                        return true;
                    }
                    if is_legacy_continuity_bootstrap_context_pack(event) {
                        return false;
                    }
                    let Some(scope) = assistant_scope else {
                        return true;
                    };
                    let Some(context_pack_id) = event_context_pack_id(event) else {
                        return true;
                    };
                    !(scope
                        .helper_only_non_model_visible_context_pack_ids
                        .contains(&context_pack_id)
                        && !event_has_model_visible_with_amai_tokens(event))
                })
                .count() as u64
        })
        .unwrap_or(live_events_count)
}

fn tool_overhead_target_events<'a>(
    events: &'a [TokenBudgetEvent],
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Vec<&'a TokenBudgetEvent> {
    events
        .iter()
        .filter(|event| {
            if !is_client_limit_component_target_event("tool_overhead_outside_retrieval", event) {
                return false;
            }
            if is_legacy_continuity_bootstrap_context_pack(event) {
                return false;
            }
            let Some(scope) = assistant_scope else {
                return true;
            };
            let Some(context_pack_id) = event_context_pack_id(event) else {
                return true;
            };
            !(scope
                .helper_only_non_model_visible_context_pack_ids
                .contains(&context_pack_id)
                && !event_has_model_visible_with_amai_tokens(event))
        })
        .collect()
}

pub(crate) fn tool_overhead_observation_source_status(
    events: Option<&[TokenBudgetEvent]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let Some(events) = events else {
        return json!({
            "source_kind": "context_pack_payload_attach_v1",
            "state": "tool_overhead_scope_unavailable",
            "target_live_events": 0,
            "observed_live_events": 0,
            "missing_live_events": 0,
            "missing_class_sample": [],
            "missing_context_pack_id_sample": [],
            "note": "Без live events этот слой не может показать, какие retrieval context packs ещё не получили tool_overhead whole-cycle attach."
        });
    };
    let target_events = tool_overhead_target_events(events, assistant_scope);
    let observed_live_events = target_events
        .iter()
        .filter(|event| event.tool_overhead_tokens.is_some())
        .count() as u64;
    let missing_events = target_events
        .iter()
        .filter(|event| event.tool_overhead_tokens.is_none())
        .copied()
        .collect::<Vec<_>>();
    let mut missing_source_states = BTreeMap::<String, u64>::new();
    let mut missing_classes = BTreeMap::<(String, String, String, String), u64>::new();
    for event in &missing_events {
        let key = (
            event.source_kind.clone(),
            event.namespace.clone(),
            event.query_type.clone(),
            event.target_kind.clone(),
        );
        *missing_classes.entry(key).or_default() += 1;
        let state = event
            .tool_overhead_source
            .as_ref()
            .and_then(|value| value["state"].as_str())
            .unwrap_or("source_state_unknown")
            .to_string();
        *missing_source_states.entry(state).or_default() += 1;
    }
    let mut missing_class_sample = missing_classes
        .into_iter()
        .map(
            |((source_kind, namespace, query_type, target_kind), count)| {
                json!({
                    "source_kind": source_kind,
                    "namespace": namespace,
                    "query_type": query_type,
                    "target_kind": target_kind,
                    "count": count,
                })
            },
        )
        .collect::<Vec<_>>();
    missing_class_sample.sort_by(|left, right| {
        right["count"]
            .as_u64()
            .cmp(&left["count"].as_u64())
            .then_with(|| {
                left["source_kind"]
                    .as_str()
                    .cmp(&right["source_kind"].as_str())
            })
            .then_with(|| left["namespace"].as_str().cmp(&right["namespace"].as_str()))
            .then_with(|| {
                left["query_type"]
                    .as_str()
                    .cmp(&right["query_type"].as_str())
            })
            .then_with(|| {
                left["target_kind"]
                    .as_str()
                    .cmp(&right["target_kind"].as_str())
            })
    });
    let mut missing_source_state_sample = missing_source_states
        .into_iter()
        .map(|(state, count)| json!({ "state": state, "count": count }))
        .collect::<Vec<_>>();
    missing_source_state_sample.sort_by(|left, right| {
        right["count"]
            .as_u64()
            .cmp(&left["count"].as_u64())
            .then_with(|| left["state"].as_str().cmp(&right["state"].as_str()))
    });
    let irrecoverable_missing_live_events = missing_events
        .iter()
        .filter(|event| {
            event
                .tool_overhead_source
                .as_ref()
                .and_then(|value| value["state"].as_str())
                .is_some_and(is_irrecoverable_tool_overhead_source_state)
        })
        .count() as u64;
    let recoverable_missing_live_events =
        (missing_events.len() as u64).saturating_sub(irrecoverable_missing_live_events);
    let recoverability_state = if missing_events.is_empty() {
        "not_applicable"
    } else if irrecoverable_missing_live_events == 0 {
        "missing_scope_recoverability_unknown"
    } else if irrecoverable_missing_live_events == missing_events.len() as u64 {
        "source_loss_irrecoverable"
    } else {
        "mixed_recoverable_and_irrecoverable"
    };
    let gap_semantics = if target_events.is_empty() {
        "not_applicable"
    } else if missing_events.is_empty() {
        "fully_materialized"
    } else if irrecoverable_missing_live_events == 0 {
        "recoverable_measurement_lag_only"
    } else if recoverable_missing_live_events == 0 {
        "irrecoverable_historical_debt_only"
    } else {
        "mixed_measurement_lag_and_irrecoverable_debt"
    };

    json!({
        "source_kind": "context_pack_payload_attach_v1",
        "state": if target_events.is_empty() {
            "no_tool_overhead_target_scope"
        } else if missing_events.is_empty() {
            "tool_overhead_source_covers_missing_scope"
        } else if irrecoverable_missing_live_events == missing_events.len() as u64 {
            "tool_overhead_irrecoverable_debt_only"
        } else {
            "tool_overhead_source_partial_scope_overlap"
        },
        "target_live_events": target_events.len(),
        "observed_live_events": observed_live_events,
        "missing_live_events": missing_events.len(),
        "recoverability_state": recoverability_state,
        "gap_semantics": gap_semantics,
        "irrecoverable_missing_live_events": irrecoverable_missing_live_events,
        "recoverable_missing_live_events": recoverable_missing_live_events,
        "missing_source_state_sample": missing_source_state_sample.into_iter().take(8).collect::<Vec<_>>(),
        "missing_class_sample": missing_class_sample.into_iter().take(8).collect::<Vec<_>>(),
        "missing_context_pack_id_sample": missing_events
            .iter()
            .filter_map(|event| event_context_pack_id(event))
            .take(8)
            .collect::<Vec<_>>(),
        "note": "Этот слой показывает, какие retrieval live events ещё не получили whole-cycle tool_overhead attach после payload-based sync, как они сгруппированы по классам, и есть ли среди них irrecoverable source-loss без stored context pack payload."
    })
}

fn client_limit_component_event_coverage(
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    live_events_count: u64,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Vec<Value> {
    client_limit_component_stats(summary, events, assistant_scope)
        .into_iter()
        .map(|(code, observed_live_events, observed_tokens)| {
            let target_live_events_count = client_limit_component_target_live_events(
                code,
                events,
                live_events_count,
                assistant_scope,
            );
            json!({
                "code": code,
                "observed_live_events": observed_live_events,
                "live_events_count": live_events_count,
                "target_live_events_count": target_live_events_count,
                "target_scope_kind": client_limit_component_target_scope_kind(code),
                "target_scope_applicable": target_live_events_count > 0,
                "event_coverage_pct": percent_share(observed_live_events, target_live_events_count),
                "observed_tokens": observed_tokens,
            })
        })
        .collect()
}

fn client_limit_meter_alignment_blocking_reasons(
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Vec<String> {
    let mut reasons = Vec::new();
    let (events_total, live_events_count, non_live_events_count, counted_events) =
        client_limit_meter_alignment_counts(summary, events);
    for (code, observed_live_events, _observed_tokens) in
        client_limit_component_stats(summary, events, assistant_scope)
    {
        let target_live_events = client_limit_component_target_live_events(
            code,
            events,
            live_events_count,
            assistant_scope,
        );
        if target_live_events == 0 {
            continue;
        }
        if observed_live_events == 0 {
            reasons.push(format!("{code}_unmeasured"));
        } else if observed_live_events < target_live_events {
            reasons.push(format!("{code}_partially_measured"));
        }
    }

    if events_total == 0 {
        reasons.push("no_usage_observed_in_scope".to_string());
    } else {
        if live_events_count == 0 {
            reasons.push("no_live_usage_in_scope".to_string());
        }
        if non_live_events_count > 0 {
            reasons.push("non_live_events_present_in_scope".to_string());
        }
        if live_events_count > 0 && counted_events == 0 {
            reasons.push("no_confirmed_live_usage_in_scope".to_string());
        }
    }
    reasons
}

fn client_limit_meter_alignment_state(
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
    baseline_equivalence: &Value,
    same_meter_as_client_limit: bool,
) -> &'static str {
    let (events_total, live_events_count, _non_live_events_count, counted_events) =
        client_limit_meter_alignment_counts(summary, events);
    let component_stats = client_limit_component_stats(summary, events, assistant_scope);
    let any_component_applicable =
        component_stats
            .iter()
            .any(|(code, _observed_live_events, _observed_tokens)| {
                client_limit_component_target_live_events(
                    code,
                    events,
                    live_events_count,
                    assistant_scope,
                ) > 0
            });
    let all_components_observed = live_events_count > 0
        && component_stats
            .iter()
            .all(|(code, observed_live_events, _observed_tokens)| {
                let target_live_events = client_limit_component_target_live_events(
                    code,
                    events,
                    live_events_count,
                    assistant_scope,
                );
                target_live_events == 0 || *observed_live_events == target_live_events
            });
    let any_component_observed = component_stats
        .iter()
        .any(|(_code, observed_live_events, _observed_tokens)| *observed_live_events > 0);

    if same_meter_as_client_limit {
        "same_meter_equivalent"
    } else if events_total == 0 {
        "no_usage_observed"
    } else if live_events_count == 0 {
        "only_non_live_scope_activity"
    } else if counted_events == 0 {
        "live_usage_unconfirmed_not_meter_equivalent"
    } else if any_component_applicable && all_components_observed {
        if baseline_equivalence["state"].as_str()
            == Some("baseline_component_semantics_explicit_boundary")
        {
            "whole_cycle_observed_explicit_boundary_not_meter_equivalent"
        } else {
            "whole_cycle_observed_baseline_partial"
        }
    } else if any_component_observed {
        "whole_cycle_partially_observed_not_meter_equivalent"
    } else {
        "partial_lower_bound_not_meter_equivalent"
    }
}

pub(crate) fn assistant_generation_missing_scope_context_pack_ids(
    events: Option<&[TokenBudgetEvent]>,
) -> BTreeSet<String> {
    events
        .into_iter()
        .flatten()
        .filter(|event| {
            event.traffic_class == "live"
                && event.measurement_scope == "retrieval_lower_bound"
                && event_has_model_visible_with_amai_tokens(event)
                && !is_legacy_continuity_bootstrap_context_pack(event)
                && event.assistant_generation_tokens.is_none()
        })
        .filter_map(event_context_pack_id)
        .collect()
}

pub(crate) fn assistant_generation_observation_source_status(
    events: Option<&[TokenBudgetEvent]>,
    rollout_observations: Option<&[codex_threads::RolloutAssistantGenerationObservation]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let target_ids = assistant_generation_missing_scope_context_pack_ids(events);
    if let Some(scope) = assistant_scope {
        let source_kind = match (
            scope.available_direct_turns > 0,
            scope.available_rollout_turns > 0,
        ) {
            (true, true) => "direct_turn_attach_plus_rollout_turn_timeline_v1",
            (true, false) => "direct_turn_attach_v1",
            (false, true) => "codex_rollout_turn_timeline_v1",
            (false, false) => "assistant_generation_source_unavailable_v1",
        };
        let state = if scope.target_context_pack_ids.is_empty() {
            "no_missing_live_retrieval_events"
        } else if scope.available_turns == 0 {
            "assistant_generation_source_unavailable"
        } else if scope.matched_context_pack_ids.is_empty() {
            "assistant_generation_source_no_scope_overlap"
        } else if !scope.unmatched_context_pack_ids.is_empty() {
            "assistant_generation_source_partial_scope_overlap"
        } else {
            "assistant_generation_source_covers_missing_scope"
        };
        return json!({
            "source_kind": source_kind,
            "state": state,
            "usable_turns": scope.available_turns,
            "usable_direct_turns": scope.available_direct_turns,
            "usable_rollout_turns": scope.available_rollout_turns,
            "matched_turn_ids": scope.matched_turn_ids.len(),
            "matched_direct_turn_ids": scope.matched_direct_turn_ids.len(),
            "matched_rollout_turn_ids": scope.matched_rollout_turn_ids.len(),
            "target_missing_context_pack_ids": scope.target_context_pack_ids.len(),
            "matched_context_pack_ids": scope.matched_context_pack_ids.len(),
            "unmatched_context_pack_ids": scope.unmatched_context_pack_ids.len(),
            "matched_context_pack_id_sample": scope.matched_context_pack_ids.iter().take(8).cloned().collect::<Vec<_>>(),
            "unmatched_context_pack_id_sample": scope.unmatched_context_pack_ids.iter().take(8).cloned().collect::<Vec<_>>(),
            "note": "Этот слой показывает, покрывают ли direct turn attach и rollout turn-timelines именно текущий live retrieval scope и можно ли честно привязать assistant_generation к turn-группам без дублирования токенов по каждому context pack."
        });
    }

    let Some(rollout_observations) = rollout_observations else {
        return json!({
            "source_kind": "assistant_generation_source_unavailable_v1",
            "state": if target_ids.is_empty() {
                "no_missing_live_retrieval_events"
            } else {
                "assistant_generation_source_unavailable"
            },
            "usable_turns": 0,
            "target_missing_context_pack_ids": target_ids.len(),
            "matched_context_pack_ids": 0,
            "unmatched_context_pack_ids": target_ids.len(),
            "matched_context_pack_id_sample": [],
            "unmatched_context_pack_id_sample": target_ids.iter().take(8).cloned().collect::<Vec<_>>(),
            "note": "Без materialized assistant-generation scope или rollout turn timeline этот слой не может доказать, покрывает ли source нужный retrieval scope."
        });
    };

    let available_turn_ids = rollout_observations
        .iter()
        .map(|observation| observation.turn_id.trim())
        .filter(|turn_id| !turn_id.is_empty())
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>();
    let matched_context_pack_ids = events
        .into_iter()
        .flatten()
        .filter(|event| {
            event.traffic_class == "live"
                && event.measurement_scope == "retrieval_lower_bound"
                && event_has_model_visible_with_amai_tokens(event)
                && !is_legacy_continuity_bootstrap_context_pack(event)
        })
        .filter_map(|event| {
            let turn_id = event.turn_id.as_deref().map(str::trim).unwrap_or_default();
            let context_pack_id = event_context_pack_id(event)?;
            if turn_id.is_empty() || !available_turn_ids.contains(turn_id) {
                return None;
            }
            Some(context_pack_id)
        })
        .collect::<BTreeSet<_>>();
    let unmatched_context_pack_ids = target_ids
        .difference(&matched_context_pack_ids)
        .cloned()
        .collect::<BTreeSet<_>>();
    let state = if target_ids.is_empty() {
        "no_missing_live_retrieval_events"
    } else if matched_context_pack_ids.is_empty() {
        "assistant_generation_source_no_scope_overlap"
    } else if !unmatched_context_pack_ids.is_empty() {
        "assistant_generation_source_partial_scope_overlap"
    } else {
        "assistant_generation_source_covers_missing_scope"
    };
    json!({
        "source_kind": "codex_rollout_turn_timeline_v1",
        "state": state,
        "usable_turns": available_turn_ids.len(),
        "target_missing_context_pack_ids": target_ids.len(),
        "matched_context_pack_ids": matched_context_pack_ids.len(),
        "unmatched_context_pack_ids": unmatched_context_pack_ids.len(),
        "matched_context_pack_id_sample": matched_context_pack_ids.iter().take(8).cloned().collect::<Vec<_>>(),
        "unmatched_context_pack_id_sample": unmatched_context_pack_ids.iter().take(8).cloned().collect::<Vec<_>>(),
        "note": "Fallback rollout timeline может показать overlap только по turn_id и поэтому слабее полного assistant-generation scope."
    })
}

fn baseline_component_semantics(
    code: &str,
    summary: &Value,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
    events: Option<&[TokenBudgetEvent]>,
) -> Option<(u64, &'static str, Option<String>)> {
    match code {
        "client_prompt" => summary["observed_client_prompt_tokens"].as_u64().map(|tokens| {
            (
                tokens,
                "baseline_semantics_materialized",
                Some("Client prompt baseline materialized from observed whole-cycle tokens.".to_string()),
            )
        }),
        "assistant_generation" => assistant_scope
            .map(|scope| scope.observed_tokens)
            .filter(|tokens| *tokens > 0)
            .map(|tokens| {
                (
                    tokens,
                    "observed_tokens_passthrough",
                    Some("Assistant generation baseline materialized as observed turn-group passthrough.".to_string()),
                )
            }),
        "tool_overhead_outside_retrieval" => summary["observed_tool_overhead_tokens"]
            .as_u64()
            .map(|tokens| {
                (
                    tokens,
                    "baseline_semantics_materialized",
                    Some("Tool-overhead baseline materialized from observed whole-cycle tokens.".to_string()),
                )
            }),
        "continuity_restore_outside_retrieval" => events
            .into_iter()
            .flatten()
            .find(|event| {
                event.target_kind == "continuity_restore"
                    && event
                        .pre_amai_baseline_source
                        .as_ref()
                        .and_then(|value| value["source_family"].as_str())
                        == Some("truthful_pre_amai_baseline_source")
            })
            .map(|event| {
                (
                    event.naive_tokens,
                    "baseline_semantics_materialized",
                    Some(
                        "Continuity restore baseline materialized from truthful pre-Amai baseline source."
                            .to_string(),
                    ),
                )
            }),
        _ => None,
    }
}

fn baseline_equivalence_component_semantics<'a>(
    baseline_equivalence: &'a Value,
    code: &str,
) -> Option<&'a Value> {
    baseline_equivalence["component_semantics"]
        .as_array()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component["code"].as_str() == Some(code))
        })
}

pub(crate) fn build_client_limit_baseline_equivalence(
    contract: &TokenBudgetContractConfig,
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let (_events_total, live_events_count, _non_live_events_count, counted_events) =
        client_limit_meter_alignment_counts(summary, events);
    let retrieval_quality_gate_confirmed = live_events_count > 0 && counted_events > 0;
    let mut measured_baseline_components = Vec::new();
    let mut missing_baseline_components = Vec::new();
    let mut explicitly_unmodeled_baseline_components = Vec::new();
    let component_semantics = client_limit_component_stats(summary, events, assistant_scope)
        .into_iter()
        .map(|(code, observed_live_events, observed_tokens)| {
            let target_live_events_count = client_limit_component_target_live_events(
                code,
                events,
                live_events_count,
                assistant_scope,
            );
            let whole_cycle_observed_complete =
                target_live_events_count == 0 || observed_live_events == target_live_events_count;
            let (baseline_measured_tokens, baseline_semantics_state, baseline_note) =
                if whole_cycle_observed_complete {
                    baseline_component_semantics(code, summary, assistant_scope, events).map_or(
                        (None, "baseline_semantics_unmaterialized", None),
                        |(tokens, state, note)| (Some(tokens), state, note),
                    )
                } else {
                    (None, "whole_cycle_observation_incomplete", None)
                };
            if target_live_events_count == 0 {
                // Non-applicable components must not leak into missing baseline debt.
            } else if whole_cycle_observed_complete && baseline_measured_tokens.is_some() {
                measured_baseline_components.push(code.to_string());
            } else if !whole_cycle_observed_complete {
                missing_baseline_components.push(code.to_string());
            } else {
                missing_baseline_components.push(code.to_string());
            }
            if code == "continuity_restore_outside_retrieval"
                && target_live_events_count > 0
                && whole_cycle_observed_complete
                && baseline_measured_tokens.is_none()
            {
                missing_baseline_components.retain(|item| item != code);
                measured_baseline_components.retain(|item| item != code);
                explicitly_unmodeled_baseline_components.push(code.to_string());
            }
            json!({
                "code": code,
                "observed_live_events": observed_live_events,
                "target_live_events_count": target_live_events_count,
                "whole_cycle_observed_complete": whole_cycle_observed_complete,
                "observed_tokens": observed_tokens,
                "baseline_measured_tokens": baseline_measured_tokens,
                "baseline_semantics_state": baseline_semantics_state,
                "note": baseline_note,
            })
        })
        .collect::<Vec<_>>();
    let measured_baseline_tokens_lower_bound = component_semantics
        .iter()
        .filter_map(|component| component["baseline_measured_tokens"].as_u64())
        .sum::<u64>();
    let incomplete_components = component_semantics
        .iter()
        .filter(|component| component["whole_cycle_observed_complete"].as_bool() != Some(true))
        .filter_map(|component| component["code"].as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let same_meter_baseline_measured = missing_baseline_components.is_empty()
        && explicitly_unmodeled_baseline_components.is_empty();
    let state = if !incomplete_components.is_empty() {
        "whole_cycle_components_incomplete"
    } else if !explicitly_unmodeled_baseline_components.is_empty() {
        "baseline_component_semantics_explicit_boundary"
    } else if measured_baseline_components.is_empty() {
        "baseline_semantics_unmaterialized"
    } else if retrieval_quality_gate_confirmed {
        "baseline_semantics_materialized"
    } else {
        "baseline_semantics_materialized_without_quality_gate"
    };
    json!({
        "model_version": contract.client_limit_baseline_equivalence_version.clone(),
        "state": state,
        "baseline_equivalent_to_client_limit": same_meter_baseline_measured,
        "same_meter_baseline_measured": same_meter_baseline_measured,
        "retrieval_quality_gate_confirmed": retrieval_quality_gate_confirmed,
        "measured_baseline_tokens_lower_bound": measured_baseline_tokens_lower_bound,
        "measured_baseline_components": measured_baseline_components,
        "missing_baseline_components": missing_baseline_components,
        "explicitly_unmodeled_baseline_components": explicitly_unmodeled_baseline_components,
        "incomplete_components": incomplete_components,
        "component_semantics": component_semantics,
        "note": "Baseline equivalence показывает, достаточно ли whole-cycle observed components и их semantics, чтобы честно объявить same meter с client-limit contour."
    })
}

pub(crate) fn build_client_limit_strict_meter_slice(
    contract: &TokenBudgetContractConfig,
    baseline_equivalence: &Value,
) -> Value {
    let measured_baseline_tokens_lower_bound =
        baseline_equivalence["measured_baseline_tokens_lower_bound"]
            .as_u64()
            .unwrap_or(0);
    let state =
        if baseline_equivalence["baseline_equivalent_to_client_limit"].as_bool() == Some(true) {
            "strict_slice_covers_all_applicable_components"
        } else if measured_baseline_tokens_lower_bound > 0 {
            "strict_same_meter_slice_partial"
        } else {
            "strict_same_meter_slice_unavailable"
        };
    json!({
        "model_version": contract.client_limit_strict_meter_slice_version.clone(),
        "state": state,
        "lower_bound_tokens": measured_baseline_tokens_lower_bound,
        "measured_baseline_tokens_lower_bound": measured_baseline_tokens_lower_bound,
        "components": baseline_equivalence["measured_baseline_components"].clone(),
        "same_meter_available": baseline_equivalence["baseline_equivalent_to_client_limit"].clone(),
        "note": "Strict same-meter slice хранит только ту baseline-часть, для которой semantics уже совместимы с exact client-limit contour."
    })
}

pub(crate) fn build_client_limit_explicit_boundary_surface(
    contract: &TokenBudgetContractConfig,
    baseline_equivalence: &Value,
) -> Value {
    let explicit_components = baseline_equivalence["explicitly_unmodeled_baseline_components"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let state = if explicit_components.is_empty() {
        "no_explicit_boundary"
    } else {
        "amai_continuity_boundary"
    };
    json!({
        "model_version": contract.client_limit_explicit_boundary_surface_version.clone(),
        "state": state,
        "components": explicit_components.clone(),
        "explicitly_unmodeled_baseline_components": explicit_components,
        "equivalence_resume_condition": if state == "amai_continuity_boundary" {
            json!("truthful_pre_amai_baseline_source")
        } else {
            Value::Null
        },
        "resolution_state": if state == "amai_continuity_boundary" {
            json!("truthful_pre_amai_baseline_required")
        } else {
            Value::Null
        },
        "guessed_baseline_prohibited": state == "amai_continuity_boundary",
        "note": "Explicit boundary surface запрещает притворяться same-meter эквивалентностью там, где truthful pre-Amai baseline пока не materialized."
    })
}

pub(crate) fn build_client_limit_continuity_boundary_rollup(
    contract: &TokenBudgetContractConfig,
    baseline_equivalence: &Value,
    explicit_boundary_surface: &Value,
) -> Value {
    json!({
        "model_version": contract.client_limit_continuity_boundary_rollup_version.clone(),
        "state": if explicit_boundary_surface["state"].as_str() == Some("amai_continuity_boundary") {
            json!("amai_continuity_boundary_observed")
        } else {
            json!("no_amai_continuity_boundary")
        },
        "explicit_boundary_present": explicit_boundary_surface["state"].as_str() == Some("amai_continuity_boundary"),
        "observed_tokens": baseline_equivalence["component_semantics"]
            .as_array()
            .and_then(|items| items.iter().find(|item| item["code"].as_str() == Some("continuity_restore_outside_retrieval")))
            .and_then(|item| item["observed_tokens"].as_u64())
            .unwrap_or(0),
        "explicitly_unmodeled_baseline_components": baseline_equivalence["explicitly_unmodeled_baseline_components"].clone(),
        "resolution_state": if explicit_boundary_surface["state"].as_str() == Some("amai_continuity_boundary") {
            json!("observed_boundary_only_not_client_meter")
        } else {
            Value::Null
        },
        "guessed_baseline_prohibited": explicit_boundary_surface["guessed_baseline_prohibited"].clone(),
        "equivalence_resume_condition": explicit_boundary_surface["equivalence_resume_condition"].clone(),
        "note": "Continuity boundary rollup поднимает explicit baseline boundary на верхний уровень client-limit alignment surface."
    })
}

pub(crate) fn build_client_limit_pre_amai_baseline_source_status(
    contract: &TokenBudgetContractConfig,
    events: Option<&[TokenBudgetEvent]>,
    _baseline_equivalence: &Value,
    explicit_boundary_surface: &Value,
) -> Value {
    let requires_truthful_pre_amai_baseline =
        explicit_boundary_surface["state"].as_str() == Some("amai_continuity_boundary");
    let materialized = events.into_iter().flatten().any(|event| {
        event.target_kind == "continuity_restore"
            && event
                .pre_amai_baseline_source
                .as_ref()
                .and_then(|value| value["source_family"].as_str())
                == Some("truthful_pre_amai_baseline_source")
    });
    let state = if materialized {
        "materialized"
    } else if !requires_truthful_pre_amai_baseline {
        "not_required"
    } else {
        "required_not_materialized"
    };
    json!({
        "model_version": contract.client_limit_pre_amai_baseline_source_version.clone(),
        "state": state,
        "required": requires_truthful_pre_amai_baseline,
        "materialized": materialized,
        "source_family": "truthful_pre_amai_baseline_source",
        "same_meter_resume_possible": materialized,
        "blocking_reason": if state == "required_not_materialized" {
            json!("missing_truthful_pre_amai_baseline_source")
        } else {
            Value::Null
        },
        "note": if state == "required_not_materialized" {
            json!("Current scope упирается в continuity explicit boundary: truthful pre-Amai baseline source ещё не materialized, поэтому same-meter claim запрещён.")
        } else {
            json!("Pre-Amai baseline source либо не нужен, либо уже materialized.")
        }
    })
}

pub(crate) fn build_client_limit_exact_pair_status(
    contract: &TokenBudgetContractConfig,
    baseline_equivalence: &Value,
    tool_overhead_observation_source: &Value,
    explicit_boundary_surface: &Value,
    pre_amai_baseline_source_status: &Value,
    blocking_reasons: &[String],
    same_meter_as_client_limit: bool,
) -> Value {
    let mut blockers = Vec::new();
    let mut covered_reasons = BTreeSet::new();
    let incomplete_components = baseline_equivalence["incomplete_components"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    if incomplete_components
        .iter()
        .any(|item| item.as_str() == Some("tool_overhead_outside_retrieval"))
    {
        let observed_live_events = tool_overhead_observation_source["observed_live_events"]
            .as_u64()
            .unwrap_or(0);
        let target_live_events = tool_overhead_observation_source["target_live_events"]
            .as_u64()
            .unwrap_or(0);
        let irrecoverable_missing_live_events =
            tool_overhead_observation_source["irrecoverable_missing_live_events"]
                .as_u64()
                .unwrap_or(0);
        let recoverable_missing_live_events =
            tool_overhead_observation_source["recoverable_missing_live_events"]
                .as_u64()
                .unwrap_or(0);
        let blocking_reason =
            if irrecoverable_missing_live_events > 0 && recoverable_missing_live_events == 0 {
                "tool_overhead_outside_retrieval_irrecoverable_debt"
            } else if observed_live_events > 0 {
                "tool_overhead_outside_retrieval_partially_measured"
            } else {
                "tool_overhead_outside_retrieval_unmeasured"
            };
        covered_reasons.insert(blocking_reason.to_string());
        blockers.push(json!({
            "code": "tool_overhead_outside_retrieval",
            "blocker_kind": "whole_cycle_observation_gap",
            "state": tool_overhead_observation_source["state"].clone(),
            "blocking_reason": blocking_reason,
            "observed_live_events": observed_live_events,
            "target_live_events": target_live_events,
            "missing_live_events": tool_overhead_observation_source["missing_live_events"].clone(),
            "recoverability_state": tool_overhead_observation_source["recoverability_state"].clone(),
            "gap_semantics": tool_overhead_observation_source["gap_semantics"].clone(),
            "irrecoverable_missing_live_events": tool_overhead_observation_source["irrecoverable_missing_live_events"].clone(),
            "recoverable_missing_live_events": tool_overhead_observation_source["recoverable_missing_live_events"].clone(),
            "frozen_gap_candidate": irrecoverable_missing_live_events > 0 && recoverable_missing_live_events == 0,
            "resolution_condition": if irrecoverable_missing_live_events > 0
                && recoverable_missing_live_events == 0
            {
                json!("freeze_irrecoverable_gap_or_keep_exact_pair_unavailable")
            } else if irrecoverable_missing_live_events > 0 {
                json!("recover_historical_tool_overhead_source_or_freeze_irrecoverable_gap")
            } else {
                json!("tool_overhead_source_covers_missing_scope")
            },
            "note": if irrecoverable_missing_live_events > 0 && recoverable_missing_live_events == 0 {
                Value::String("Whole-cycle exact pair по этому компоненту ещё не materialized: recoverable lag уже снят, но остался только irrecoverable historical source-loss без stored context pack payload. Дальше нужен либо explicit frozen gap, либо exact pair честно остаётся unavailable.".to_string())
            } else if irrecoverable_missing_live_events > 0 {
                Value::String("Whole-cycle exact pair по этому компоненту ещё не materialized: часть retrieval live scope уже упирается в irrecoverable source-loss без stored context pack payload, поэтому дальше нужен либо recovery source, либо explicit frozen gap.".to_string())
            } else {
                Value::String("Whole-cycle exact pair по этому компоненту ещё не materialized: retrieval live scope имеет missing tool_overhead attach и пока даёт только partial same-meter coverage.".to_string())
            }
        }));
    }

    if let Some(component) =
        baseline_equivalence_component_semantics(baseline_equivalence, "assistant_generation")
    {
        let semantics_state = component["baseline_semantics_state"]
            .as_str()
            .unwrap_or_default();
        if component["target_live_events_count"].as_u64().unwrap_or(0) > 0
            && component["whole_cycle_observed_complete"].as_bool() == Some(true)
            && !component["baseline_measured_tokens"].is_u64()
            && semantics_state == "baseline_semantics_unmaterialized"
        {
            let blocking_reason = "assistant_generation_baseline_semantics_unmaterialized";
            covered_reasons.insert(blocking_reason.to_string());
            blockers.push(json!({
                "code": "assistant_generation",
                "blocker_kind": "baseline_semantics_gap",
                "state": semantics_state,
                "blocking_reason": blocking_reason,
                "observed_live_events": component["observed_live_events"].clone(),
                "target_live_events": component["target_live_events_count"].clone(),
                "observed_tokens": component["observed_tokens"].clone(),
                "resolution_condition": "assistant_generation_baseline_semantics_materialized",
                "note": component["note"].clone(),
            }));
        }
    }

    if explicit_boundary_surface["state"].as_str() == Some("amai_continuity_boundary")
        && pre_amai_baseline_source_status["state"].as_str() == Some("required_not_materialized")
    {
        let blocking_reason = pre_amai_baseline_source_status["blocking_reason"]
            .as_str()
            .unwrap_or("missing_truthful_pre_amai_baseline_source");
        covered_reasons.insert(blocking_reason.to_string());
        covered_reasons.insert("same_meter_baseline_explicit_boundary".to_string());
        blockers.push(json!({
            "code": "continuity_restore_outside_retrieval",
            "blocker_kind": "explicit_truth_boundary",
            "state": pre_amai_baseline_source_status["state"].clone(),
            "blocking_reason": blocking_reason,
            "resolution_condition": explicit_boundary_surface["equivalence_resume_condition"].clone(),
            "resolution_state": explicit_boundary_surface["resolution_state"].clone(),
            "guessed_baseline_prohibited": explicit_boundary_surface["guessed_baseline_prohibited"].clone(),
            "note": pre_amai_baseline_source_status["note"].clone(),
        }));
    }

    for reason in blocking_reasons {
        if covered_reasons.contains(reason) {
            continue;
        }
        blockers.push(json!({
            "code": Value::Null,
            "blocker_kind": "generic_alignment_gap",
            "state": "reported_by_alignment_blocking_reasons",
            "blocking_reason": reason,
            "resolution_condition": Value::Null,
            "note": "Этот blocker уже surfaced top-level alignment blocking_reasons и остаётся общим exact-pair gap без отдельного per-component explainer."
        }));
    }

    let (state, primary_blocking_reason, note) = if same_meter_as_client_limit {
        (
            "exact_pair_materialized",
            Value::Null,
            "В этом scope exact same-meter pair уже materialized: процент model-token savings можно читать как точный client-limit correlation.",
        )
    } else if blockers.is_empty() {
        (
            "exact_pair_blocked_unknown",
            Value::Null,
            "Exact same-meter pair ещё не materialized, но отдельный blocker surface пока не смог разложить причину детальнее текущего alignment state.",
        )
    } else {
        (
            "exact_pair_blocked",
            blockers[0]["blocking_reason"].clone(),
            "Exact same-meter pair ещё не materialized: этот surface перечисляет именно те truth-gaps, которые сейчас держат scope вне точной correlation с client-limit meter.",
        )
    };

    json!({
        "model_version": contract.client_limit_meter_alignment_version.clone(),
        "state": state,
        "exact_pair_available": same_meter_as_client_limit,
        "blocking_reason_count": blockers.len(),
        "primary_blocking_reason": primary_blocking_reason,
        "blockers": blockers,
        "note": note,
    })
}

pub(crate) fn build_client_limit_frozen_gap_review_surface(
    contract: &TokenBudgetContractConfig,
    exact_pair_status: &Value,
) -> Value {
    let exact_pair_available = exact_pair_status["exact_pair_available"].as_bool() == Some(true);
    let blocker = exact_pair_status["blockers"]
        .as_array()
        .and_then(|items| items.first());
    let review_required =
        blocker.and_then(|value| value["frozen_gap_candidate"].as_bool()) == Some(true);
    let state = if exact_pair_available {
        "not_applicable_exact_pair_materialized"
    } else if review_required {
        "review_required"
    } else if exact_pair_status["state"].as_str() == Some("exact_pair_blocked") {
        "not_applicable_without_frozen_gap_candidate"
    } else {
        "not_applicable"
    };
    let allowed_paths = if review_required {
        json!([
            "keep_exact_pair_unavailable",
            "formalize_reviewed_frozen_debt_export"
        ])
    } else {
        json!([])
    };
    let forbidden_paths = if review_required {
        json!(["claim_raw_exact_history"])
    } else {
        json!([])
    };
    let note = match state {
        "review_required" => {
            "Этот surface формализует product decision point для irrecoverable historical debt: raw exact history уже недоступна, поэтому дальше допустимы только keep-exact-unavailable или отдельный reviewed frozen-debt export без притворства raw exact history."
        }
        "not_applicable_exact_pair_materialized" => {
            "Exact same-meter pair уже materialized, поэтому отдельный frozen-gap review не нужен."
        }
        "not_applicable_without_frozen_gap_candidate" => {
            "Exact pair ещё blocked, но текущий blocker пока не классифицирован как irrecoverable frozen debt candidate."
        }
        _ => "Frozen-gap review surface здесь не требуется.",
    };
    json!({
        "model_version": contract.client_limit_frozen_gap_review_surface_version.clone(),
        "state": state,
        "review_required": review_required,
        "raw_exact_history_available": exact_pair_available,
        "blocking_component": blocker.and_then(|value| value["code"].as_str()).map(|value| value.to_string()),
        "missing_live_events": blocker.and_then(|value| value["missing_live_events"].as_u64()),
        "irrecoverable_missing_live_events": blocker.and_then(|value| value["irrecoverable_missing_live_events"].as_u64()),
        "recoverable_missing_live_events": blocker.and_then(|value| value["recoverable_missing_live_events"].as_u64()),
        "resolution_condition": blocker.and_then(|value| value["resolution_condition"].as_str()).map(|value| value.to_string()),
        "allowed_paths": allowed_paths,
        "forbidden_paths": forbidden_paths,
        "note": note,
    })
}

pub(crate) fn build_reviewed_frozen_debt_export_surface(
    contract: &TokenBudgetContractConfig,
    client_limit_boundary_semantics: &Value,
    scope_code: Option<&str>,
) -> Value {
    let exact_pair_status = &client_limit_boundary_semantics["exact_pair_status"];
    let frozen_gap_review_surface = &client_limit_boundary_semantics["frozen_gap_review_surface"];
    let review_required = frozen_gap_review_surface["review_required"].as_bool() == Some(true)
        || frozen_gap_review_surface["state"].as_str() == Some("review_required");
    let exact_pair_available = exact_pair_status["exact_pair_available"].as_bool() == Some(true)
        || client_limit_boundary_semantics["same_meter_as_client_limit"].as_bool() == Some(true);
    let export_ready_report_only = review_required && !exact_pair_available;
    let state = if export_ready_report_only {
        "reviewed_frozen_debt_export_ready_report_only"
    } else if exact_pair_available {
        "not_applicable_exact_pair_materialized"
    } else if review_required {
        "review_required_but_export_not_ready"
    } else if exact_pair_status["state"].as_str() == Some("exact_pair_blocked") {
        "not_applicable_without_frozen_gap_candidate"
    } else {
        "not_applicable"
    };
    let allowed_claims = if export_ready_report_only {
        json!([
            "reviewed_frozen_debt_report_only",
            "historical_source_loss_disclosed_non_exact",
            "raw_exact_history_unavailable"
        ])
    } else {
        json!([])
    };
    let forbidden_claims = if review_required {
        json!([
            "claim_raw_exact_history",
            "claim_exact_same_meter_pair_materialized"
        ])
    } else {
        json!([])
    };
    let required_disclosures = if export_ready_report_only {
        json!([
            "irrecoverable_historical_debt_present",
            "raw_exact_history_unavailable",
            "review_only_non_exact_surface"
        ])
    } else {
        json!([])
    };
    let propagated_surfaces = if export_ready_report_only {
        json!([
            "contractual_statement_summary",
            "statement_export_preview",
            "settlement_report_preview",
            "contractual_evidence_pack"
        ])
    } else {
        json!([])
    };
    let review_bundle_command = if export_ready_report_only {
        scope_code.map(|scope_code| {
            format!("./scripts/amai_exec.sh observe token-statement-export --scope {scope_code}")
        })
    } else {
        None
    };
    let evidence_pack_command = if export_ready_report_only {
        scope_code.map(|scope_code| {
            format!("./scripts/amai_exec.sh observe token-evidence-pack --scope {scope_code}")
        })
    } else {
        None
    };
    let note = match state {
        "reviewed_frozen_debt_export_ready_report_only" => {
            "Этот surface materialize-ит отдельный reviewed frozen-debt export: historical source-loss можно показывать только как report-only review contour с явным раскрытием debt и без притворства raw exact history."
        }
        "not_applicable_exact_pair_materialized" => {
            "Exact same-meter pair уже materialized, поэтому отдельный reviewed frozen-debt export не нужен."
        }
        "review_required_but_export_not_ready" => {
            "Frozen-gap review уже требуется, но report-only export contour ещё не готов к честной публикации."
        }
        "not_applicable_without_frozen_gap_candidate" => {
            "Exact pair ещё blocked, но blocker пока не классифицирован как irrecoverable frozen debt candidate."
        }
        _ => "Reviewed frozen-debt export surface здесь не требуется.",
    };
    json!({
        "model_version": contract
            .client_limit_reviewed_frozen_debt_export_surface_version
            .clone(),
        "state": state,
        "review_required": review_required,
        "export_ready_report_only": export_ready_report_only,
        "surface_kind": if export_ready_report_only {
            json!("reviewed_frozen_debt_report_only")
        } else {
            Value::Null
        },
        "exact_pair_available": exact_pair_available,
        "raw_exact_history_available": exact_pair_available,
        "blocking_component": frozen_gap_review_surface["blocking_component"].clone(),
        "missing_live_events": frozen_gap_review_surface["missing_live_events"].clone(),
        "irrecoverable_missing_live_events": frozen_gap_review_surface
            ["irrecoverable_missing_live_events"]
            .clone(),
        "recoverable_missing_live_events": frozen_gap_review_surface
            ["recoverable_missing_live_events"]
            .clone(),
        "resolution_condition": frozen_gap_review_surface["resolution_condition"].clone(),
        "allowed_claims": allowed_claims,
        "forbidden_claims": forbidden_claims,
        "required_disclosures": required_disclosures,
        "propagated_surfaces": propagated_surfaces,
        "review_bundle_command": review_bundle_command,
        "evidence_pack_command": evidence_pack_command,
        "note": note,
    })
}

pub(crate) fn build_client_limit_meter_alignment(
    contract: &TokenBudgetContractConfig,
    surface_kind: &str,
    summary: &Value,
    events: Option<&[TokenBudgetEvent]>,
    rollout_observations: Option<&[codex_threads::RolloutAssistantGenerationObservation]>,
    assistant_scope: Option<&AssistantGenerationScopeObservation>,
) -> Value {
    let (events_total, live_events_count, non_live_events_count, counted_events) =
        client_limit_meter_alignment_counts(summary, events);
    let component_coverage =
        client_limit_component_event_coverage(summary, events, live_events_count, assistant_scope);
    let component_stats = client_limit_component_stats(summary, events, assistant_scope);
    let mut measured_components = vec![
        "retrieval_payload".to_string(),
        "followup_recovery".to_string(),
    ];
    let mut not_applicable_components = Vec::new();
    let mut partially_measured_components = Vec::new();
    let mut missing_components = Vec::new();
    for (code, observed_live_events, _observed_tokens) in component_stats {
        let target_live_events = client_limit_component_target_live_events(
            code,
            events,
            live_events_count,
            assistant_scope,
        );
        if target_live_events == 0 {
            not_applicable_components.push(code.to_string());
        } else if observed_live_events == target_live_events {
            measured_components.push(code.to_string());
        } else {
            missing_components.push(code.to_string());
            if observed_live_events > 0 {
                partially_measured_components.push(code.to_string());
            }
        }
    }
    let assistant_generation_observation_source = assistant_generation_observation_source_status(
        events,
        rollout_observations,
        assistant_scope,
    );
    let tool_overhead_observation_source =
        tool_overhead_observation_source_status(events, assistant_scope);
    let baseline_equivalence =
        build_client_limit_baseline_equivalence(contract, summary, events, assistant_scope);
    let strict_client_meter_slice =
        build_client_limit_strict_meter_slice(contract, &baseline_equivalence);
    let explicit_boundary_surface =
        build_client_limit_explicit_boundary_surface(contract, &baseline_equivalence);
    let continuity_boundary_rollup = build_client_limit_continuity_boundary_rollup(
        contract,
        &baseline_equivalence,
        &explicit_boundary_surface,
    );
    let pre_amai_baseline_source_status = build_client_limit_pre_amai_baseline_source_status(
        contract,
        events,
        &baseline_equivalence,
        &explicit_boundary_surface,
    );
    let mut blocking_reasons =
        client_limit_meter_alignment_blocking_reasons(summary, events, assistant_scope);
    if baseline_equivalence["baseline_equivalent_to_client_limit"].as_bool() == Some(true) {
        blocking_reasons.retain(|reason| reason != "no_confirmed_live_usage_in_scope");
    }
    match assistant_generation_observation_source["state"]
        .as_str()
        .unwrap_or_default()
    {
        "assistant_generation_source_unavailable" => {
            blocking_reasons.push("assistant_generation_source_unavailable".to_string());
        }
        "assistant_generation_source_no_scope_overlap" => {
            blocking_reasons.push("assistant_generation_source_no_scope_overlap".to_string());
        }
        "assistant_generation_source_partial_scope_overlap" => {
            blocking_reasons.push("assistant_generation_source_partial_scope_overlap".to_string());
        }
        _ => {}
    }
    match baseline_equivalence["state"].as_str().unwrap_or_default() {
        "baseline_semantics_unmaterialized" => {
            blocking_reasons.push("same_meter_baseline_unmeasured".to_string());
        }
        "baseline_component_semantics_explicit_boundary" => {
            blocking_reasons.push("same_meter_baseline_explicit_boundary".to_string());
        }
        "baseline_component_semantics_partial" => {
            blocking_reasons.push("same_meter_baseline_partially_measured".to_string());
        }
        _ => {}
    }
    let same_meter_as_client_limit = blocking_reasons.is_empty()
        && baseline_equivalence["baseline_equivalent_to_client_limit"].as_bool() == Some(true);
    let exact_pair_status = build_client_limit_exact_pair_status(
        contract,
        &baseline_equivalence,
        &tool_overhead_observation_source,
        &explicit_boundary_surface,
        &pre_amai_baseline_source_status,
        &blocking_reasons,
        same_meter_as_client_limit,
    );
    let frozen_gap_review_surface =
        build_client_limit_frozen_gap_review_surface(contract, &exact_pair_status);
    json!({
        "model_version": contract.client_limit_meter_alignment_version.clone(),
        "surface_kind": surface_kind,
        "alignment_state": client_limit_meter_alignment_state(
            summary,
            events,
            assistant_scope,
            &baseline_equivalence,
            same_meter_as_client_limit,
        ),
        "same_meter_as_client_limit": same_meter_as_client_limit,
        "events_total": events_total,
        "live_events_count": live_events_count,
        "non_live_events_count": non_live_events_count,
        "counted_live_events": counted_events,
        "measured_components": measured_components,
        "not_applicable_components": not_applicable_components,
        "partially_measured_components": partially_measured_components,
        "missing_components": missing_components,
        "component_event_coverage": component_coverage,
        "blocking_reasons": blocking_reasons,
        "assistant_generation_observation_source": assistant_generation_observation_source,
        "tool_overhead_observation_source": tool_overhead_observation_source,
        "baseline_equivalence": baseline_equivalence,
        "strict_client_meter_slice": strict_client_meter_slice,
        "explicit_boundary_surface": explicit_boundary_surface,
        "continuity_boundary_rollup": continuity_boundary_rollup,
        "pre_amai_baseline_source_status": pre_amai_baseline_source_status,
        "exact_pair_status": exact_pair_status,
        "frozen_gap_review_surface": frozen_gap_review_surface,
        "note": "Этот слой честно показывает, что текущие savings пока считаются как lower-bound части агентного цикла. Whole-cycle observed components могут постепенно materialize-иться, но same meter с клиентским лимитом нельзя объявлять раньше, чем появится и полное observed покрытие, и baseline-equivalent semantics."
    })
}
