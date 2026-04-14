use crate::retrieval_science;
use anyhow::Result;
use serde_json::{Value, json};

pub(super) fn build_continuity_correctness_model(payload: &Value) -> Result<Value> {
    let verification = &payload["latest_continuity_verification"]["continuity_verification"];
    if !verification.is_object() {
        return Ok(json!({
            "summary": {
                "status": "unknown",
                "probe_count": 0,
                "verified_probes": 0,
                "failed_probes": 0,
                "recovered_useful": 0,
                "fail_closed": 0,
                "evidence_gap": true,
            },
            "failed_probe_names": [],
            "last_evidence_at_epoch_ms": Value::Null,
        }));
    }

    let canonical_eval = &verification["canonical_eval"];
    let probe_count = verification["probe_count"].as_u64().unwrap_or_else(|| {
        canonical_eval["probes"]
            .as_array()
            .map_or(0, |items| items.len() as u64)
    });
    let failed_probe_names = verification["failed_probes"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let failed_probes = failed_probe_names.len() as u64;
    let verified_probes = verification["verified_probes"]
        .as_u64()
        .unwrap_or_else(|| probe_count.saturating_sub(failed_probes));
    let verification_status =
        verification["verification_status"]
            .as_str()
            .unwrap_or(if failed_probes > 0 {
                "critical"
            } else {
                "pass"
            });
    Ok(json!({
        "summary": {
            "status": verification_status,
            "probe_count": probe_count,
            "verified_probes": verified_probes,
            "failed_probes": failed_probes,
            "recovered_useful": canonical_eval["verdict_counts"]["recovered_useful"].as_u64().unwrap_or(0),
            "fail_closed": canonical_eval["verdict_counts"]["hit_correct_target"].as_u64().unwrap_or(0),
            "evidence_gap": false,
        },
        "failed_probe_names": failed_probe_names,
        "last_evidence_at_epoch_ms": verification["captured_at_epoch_ms"].clone(),
    }))
}

pub(super) fn build_degradation_model(payload: &Value) -> Result<Value> {
    let entries = retrieval_science::degradation_matrix_entries()?;
    let matrix_json = retrieval_science::degradation_matrix_json()?;
    let truth_ranking = matrix_json
        .get("truth_ranking")
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    let classes = entries
        .into_iter()
        .map(|(class_key, entry)| evaluate_degradation_class(payload, &class_key, &entry))
        .collect::<Vec<_>>();

    let fail_closed_total = classes
        .iter()
        .filter(|item| item["mode"].as_str() == Some("fail_closed"))
        .count() as u64;
    let graceful_total = classes
        .iter()
        .filter(|item| item["mode"].as_str() == Some("graceful_fallback"))
        .count() as u64;
    let pass = classes
        .iter()
        .filter(|item| item["status"].as_str() == Some("pass"))
        .count() as u64;
    let critical = classes
        .iter()
        .filter(|item| item["status"].as_str() == Some("critical"))
        .count() as u64;
    let unknown = classes
        .iter()
        .filter(|item| item["status"].as_str() == Some("unknown"))
        .count() as u64;
    let evidence_gaps = classes
        .iter()
        .filter(|item| item["evidence_gap"].as_bool() == Some(true))
        .count() as u64;
    let overall_status = if critical > 0 {
        "critical"
    } else if unknown > 0 {
        "unknown"
    } else {
        "pass"
    };

    Ok(json!({
        "policy_version": matrix_json["policy_version"].clone(),
        "truth_ranking": truth_ranking,
        "summary": {
            "status": overall_status,
            "pass": pass,
            "critical": critical,
            "unknown": unknown,
            "fail_closed_total": fail_closed_total,
            "graceful_fallback_total": graceful_total,
            "evidence_gaps": evidence_gaps,
        },
        "classes": classes,
    }))
}

fn evaluate_degradation_class(
    payload: &Value,
    class_key: &str,
    entry: &retrieval_science::DegradationMatrixEntry,
) -> Value {
    match class_key {
        "cross_project_scope" => evaluate_accuracy_degradation_class(
            payload,
            class_key,
            entry,
            "cross_project_leakage",
            &[
                "strict_local_visible_projects_only",
                "strict_local_hits_do_not_leak_projects",
                "hostile_mixed_query_fail_closed",
                "hostile_mixed_query_visible_projects_only",
                "hostile_mixed_query_hits_do_not_leak_projects",
            ],
            "Последний accuracy / isolation прогон подтвердил zero leakage между проектами.",
        ),
        "cross_namespace_scope" => evaluate_accuracy_degradation_class(
            payload,
            class_key,
            entry,
            "cross_namespace_leakage",
            &[
                "strict_local_visible_namespaces_only",
                "strict_local_hits_do_not_leak_namespaces",
                "hostile_mixed_query_visible_namespaces_only",
                "hostile_mixed_query_hits_do_not_leak_namespaces",
                "namespace_strict_visible_projects_only",
                "namespace_strict_hits_do_not_leak_namespaces",
                "namespace_strict_fail_closed",
            ],
            "Последний accuracy / isolation прогон подтвердил zero leakage между namespace.",
        ),
        "cross_agent_scope"
        | "corrupt_scope_metadata"
        | "partial_refresh"
        | "qdrant_unavailable"
        | "stale_cache"
        | "partial_thread_index"
        | "empty_embeddings"
        | "stale_handoff"
        | "working_state_conflict" => {
            evaluate_degradation_verification_class(payload, class_key, entry)
        }
        _ => evaluate_policy_gap_class(payload, class_key, entry),
    }
}

fn evaluate_accuracy_degradation_class(
    payload: &Value,
    class_key: &str,
    entry: &retrieval_science::DegradationMatrixEntry,
    leakage_key: &str,
    invariant_names: &[&str],
    success_reason: &str,
) -> Value {
    let accuracy = &payload["latest_retrieval_accuracy"]["accuracy_verification"];
    if !accuracy.is_object() {
        return degradation_class_value(
            class_key,
            entry,
            "unknown",
            "Свежий accuracy / isolation verification ещё не записан.",
            None,
            None,
            true,
        );
    }
    let captured_at = accuracy["captured_at_epoch_ms"].as_u64();
    let leakage = accuracy[leakage_key]
        .as_f64()
        .or_else(|| accuracy[leakage_key].as_u64().map(|value| value as f64))
        .unwrap_or(0.0);
    let invariants = invariant_names
        .iter()
        .map(|name| {
            (
                *name,
                accuracy["formal_invariants"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|item| item["name"].as_str() == Some(*name))
                    .and_then(|item| item["pass"].as_bool()),
            )
        })
        .collect::<Vec<_>>();
    let missing = invariants
        .iter()
        .filter(|(_, pass)| pass.is_none())
        .map(|(name, _)| (*name).to_string())
        .collect::<Vec<_>>();
    let failed = invariants
        .iter()
        .filter_map(|(name, pass)| {
            if pass == &Some(false) {
                Some((*name).to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if leakage > 0.0 || !failed.is_empty() {
        let mut reasons = Vec::new();
        if leakage > 0.0 {
            reasons.push(format!("observed leakage = {}", leakage));
        }
        if !failed.is_empty() {
            reasons.push(format!("formal invariants failed: {}", failed.join(", ")));
        }
        return degradation_class_value(
            class_key,
            entry,
            "critical",
            &format!("Последний proof поймал нарушение: {}.", reasons.join("; ")),
            Some("retrieval_accuracy"),
            captured_at,
            false,
        );
    }

    if !missing.is_empty() {
        return degradation_class_value(
            class_key,
            entry,
            "unknown",
            &format!(
                "Последний accuracy proof неполный: не хватает formal invariants {}.",
                missing.join(", ")
            ),
            Some("retrieval_accuracy"),
            captured_at,
            true,
        );
    }

    degradation_class_value(
        class_key,
        entry,
        "pass",
        success_reason,
        Some("retrieval_accuracy"),
        captured_at,
        false,
    )
}

fn evaluate_policy_gap_class(
    payload: &Value,
    class_key: &str,
    entry: &retrieval_science::DegradationMatrixEntry,
) -> Value {
    let working_state = &payload["latest_working_state_restore"]["working_state_restore"];
    if class_key == "stale_handoff" && working_state.is_object() {
        let freshness = working_state["restore_freshness_state"]
            .as_str()
            .unwrap_or("ещё нет данных");
        return degradation_class_value(
            class_key,
            entry,
            "unknown",
            &format!(
                "Текущий рабочий снимок уже умеет помечать freshness = {freshness}, но отдельный degradation proof для этого класса ещё не записан."
            ),
            Some("working_state_restore"),
            working_state["captured_at_epoch_ms"].as_u64(),
            true,
        );
    }

    if class_key == "working_state_conflict" && working_state.is_object() {
        let confidence = working_state["restore_confidence"]
            .as_str()
            .unwrap_or("ещё нет данных");
        return degradation_class_value(
            class_key,
            entry,
            "unknown",
            &format!(
                "Текущий рабочий снимок уже даёт confidence = {confidence}, но отдельный conflict-proof для этого класса ещё не записан."
            ),
            Some("working_state_restore"),
            working_state["captured_at_epoch_ms"].as_u64(),
            true,
        );
    }

    degradation_class_value(
        class_key,
        entry,
        "unknown",
        &format!(
            "Этот класс уже описан в policy, но свежий machine-readable proof через '{}' пока не materialized.",
            entry.evidence_source
        ),
        None,
        None,
        true,
    )
}

fn evaluate_degradation_verification_class(
    payload: &Value,
    class_key: &str,
    entry: &retrieval_science::DegradationMatrixEntry,
) -> Value {
    let verification = &payload["latest_degradation_verification"]["degradation_verification"];
    if !verification.is_object() {
        return evaluate_policy_gap_class(payload, class_key, entry);
    }
    let scenario = verification["scenarios"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|item| item["class_key"].as_str() == Some(class_key));
    let Some(scenario) = scenario else {
        return evaluate_policy_gap_class(payload, class_key, entry);
    };

    degradation_class_value(
        class_key,
        entry,
        scenario["status"].as_str().unwrap_or("unknown"),
        scenario["reason"].as_str().unwrap_or("ещё нет деталей"),
        Some("degradation_verification"),
        verification["captured_at_epoch_ms"].as_u64(),
        scenario["status"].as_str() != Some("pass"),
    )
}

fn degradation_class_value(
    class_key: &str,
    entry: &retrieval_science::DegradationMatrixEntry,
    status: &str,
    reason: &str,
    last_evidence_kind: Option<&str>,
    last_evidence_at_epoch_ms: Option<u64>,
    evidence_gap: bool,
) -> Value {
    json!({
        "class_key": class_key,
        "title": entry.title,
        "mode": entry.mode,
        "summary": entry.summary,
        "expected_behavior": entry.expected_behavior,
        "user_signal": entry.user_signal,
        "evidence_source": entry.evidence_source,
        "runbook": entry.runbook,
        "status": status,
        "reason": reason,
        "last_evidence_kind": last_evidence_kind,
        "last_evidence_at_epoch_ms": last_evidence_at_epoch_ms,
        "evidence_gap": evidence_gap,
    })
}
