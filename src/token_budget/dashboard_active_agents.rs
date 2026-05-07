use super::dashboard_agent_scope_activity::active_agent_activity_entries;
use super::*;

fn active_agent_profile_log(stage: &str, elapsed_ms: u128, extra: &str) {
    continuity_profile_log(&format!("active_agent_budget.{stage}"), elapsed_ms, extra);
}

fn parse_scope_parts(scope: &str) -> (Option<String>, Option<String>) {
    let mut parts = scope
        .split("::")
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let project_code = parts.next().map(str::to_string);
    let namespace_code = parts.next().map(str::to_string);
    (project_code, namespace_code)
}

fn active_agent_display_label(project_code: &str) -> String {
    let label = project_code
        .split(['_', '-'])
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => {
                    let mut title = first.to_uppercase().collect::<String>();
                    title.push_str(chars.as_str());
                    title
                }
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if label.is_empty() {
        project_code.to_string()
    } else {
        label
    }
}

pub(super) fn resolved_active_agent_label(
    override_display_name: Option<&str>,
    thread_meta: Option<&Value>,
    fallback_agent_label: &str,
    active_headline: Option<&str>,
    agent_scope: &str,
) -> String {
    override_display_name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            thread_meta
                .and_then(|thread| thread["agent_nickname"].as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            thread_meta
                .and_then(|thread| thread["agent_role"].as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            let fallback_agent_label = fallback_agent_label.trim();
            (!fallback_agent_label.is_empty()).then_some(fallback_agent_label)
        })
        .or_else(|| {
            active_headline
                .map(str::trim)
                .filter(|value| !value.is_empty())
        })
        .unwrap_or(agent_scope)
        .to_string()
}

fn active_agent_selector_from_activity(
    active: &Value,
) -> Option<(PersonalKpiSelector, Option<String>)> {
    let agent_scope = active["agent_scope"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let (parsed_project_code, parsed_namespace_code) = parse_scope_parts(&agent_scope);
    let project_code = active["project_code"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or(parsed_project_code)?;
    let namespace_code = active["namespace_code"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or(parsed_namespace_code)
        .unwrap_or_else(|| "continuity".to_string());
    let thread_id = active["owner_thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let project_repo_root = active["project_repo_root"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Some((
        PersonalKpiSelector {
            project_code,
            namespace_code,
            agent_scope,
            thread_id,
        },
        project_repo_root,
    ))
}

pub(super) fn active_agent_kpi_aggregate(entries: &[Value]) -> Value {
    let active_count = entries.len() as u64;
    let observed = entries
        .iter()
        .filter_map(|entry| entry["personal_agent_kpi"]["signed_kpi_percent"].as_f64())
        .collect::<Vec<_>>();
    let observed_count = observed.len() as u64;
    let missing_count = active_count.saturating_sub(observed_count);
    if active_count == 0 {
        return json!({
            "status": "missing",
            "active_count": active_count,
            "observed_count": observed_count,
            "missing_count": missing_count,
            "reply_prefix": "5ч KPI: н/д",
            "scope_label": "активных агентов сейчас нет",
            "summary": "Active lease сейчас нет, поэтому средний личный KPI по работающим агентам не считается."
        });
    }
    if missing_count > 0 || observed.is_empty() {
        return json!({
            "status": "partial",
            "active_count": active_count,
            "observed_count": observed_count,
            "missing_count": missing_count,
            "reply_prefix": "5ч KPI: н/д",
            "scope_label": format!("из {} активных агентов KPI materialized у {}", active_count, observed_count),
            "summary": "Не у всех активных агентов уже есть личный measured 5ч KPI, поэтому среднее fail-closed не считается."
        });
    }
    let signed_average = observed.iter().sum::<f64>() / observed.len() as f64;
    let classification = signed_kpi_classification(signed_average);
    json!({
        "status": "observed",
        "active_count": active_count,
        "observed_count": observed_count,
        "missing_count": missing_count,
        "classification": classification,
        "signed_kpi_percent": signed_average,
        "kpi_percent": signed_average.abs(),
        "reply_prefix": reply_prefix_for_signed_kpi_percent(signed_average),
        "scope_label": format!("среднее по {} активным агентам", active_count),
        "summary": match classification {
            "saving" => format!("Средний личный 5ч KPI по активным агентам сейчас в экономии {:.2}%.", signed_average),
            "overspend" => format!("Средний личный 5ч KPI по активным агентам сейчас в переплате {:.2}%.", signed_average.abs()),
            _ => "Средний личный 5ч KPI по активным агентам сейчас идёт примерно 1:1.".to_string(),
        }
    })
}

fn active_agent_limit_weight_tokens(summary: &Value) -> u64 {
    summary["verified_observed_whole_cycle_with_amai_tokens"]
        .as_u64()
        .filter(|value| *value > 0)
        .or_else(|| summary["observed_whole_cycle_with_amai_tokens"].as_u64())
        .unwrap_or(0)
}

fn proof_like_runtime_marker(value: Option<&str>) -> bool {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some_and(|value| {
            let lower = value.to_ascii_lowercase();
            value.starts_with("proof-")
                || value.starts_with("proof_")
                || value.starts_with("turn-proof-")
                || value.starts_with("turn_proof_")
                || value.contains("::proof_")
                || value.contains("::proof-")
                || lower.contains("proof_execctl_restore")
                || lower.contains("proof-execctl-restore")
                || lower.contains("execctl_restore_stress")
                || lower.contains("execctl restore stress")
        })
}

pub(super) fn user_visible_agent_activity_is_proof_runtime(
    project_code: Option<&str>,
    agent_scope: Option<&str>,
    thread_id: Option<&str>,
    headline: Option<&str>,
    title: Option<&str>,
) -> bool {
    [project_code, agent_scope, thread_id, headline, title]
        .into_iter()
        .any(proof_like_runtime_marker)
}

fn active_agent_identity_key(agent: &Value) -> Option<String> {
    let agent_scope = agent["agent_scope"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let project_code = agent["project_code"].as_str().unwrap_or_default().trim();
    let namespace_code = agent["namespace_code"].as_str().unwrap_or_default().trim();
    Some(format!(
        "{project_code}\u{1f}{namespace_code}\u{1f}{agent_scope}"
    ))
}

fn active_agent_candidate_score(agent: &Value) -> (i32, i32, i32, i32, i64) {
    let current_thread_bound =
        agent["client_live_meter"]["current_thread_bound"].as_bool() == Some(true);
    let observed_meter = agent["client_live_meter"]["status"].as_str() == Some("observed");
    let has_thread_id = agent["thread_id"]
        .as_str()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let has_bound_identity =
        agent["activity_source"].as_str() != Some("recent_thread_unbound_fallback");
    let heartbeat_at_epoch_ms = json_i64(&agent["heartbeat_at_epoch_ms"]).unwrap_or_default();
    (
        if current_thread_bound { 1 } else { 0 },
        if observed_meter { 1 } else { 0 },
        if has_thread_id { 1 } else { 0 },
        if has_bound_identity { 1 } else { 0 },
        heartbeat_at_epoch_ms,
    )
}

pub(super) fn dedup_active_agents_by_identity(agents: Vec<Value>) -> Vec<Value> {
    let mut deduped = Vec::new();
    let mut index_by_key = HashMap::<String, usize>::new();
    for agent in agents {
        let Some(key) = active_agent_identity_key(&agent) else {
            deduped.push(agent);
            continue;
        };
        if let Some(existing_index) = index_by_key.get(&key).copied() {
            if active_agent_candidate_score(&agent)
                > active_agent_candidate_score(&deduped[existing_index])
            {
                deduped[existing_index] = agent;
            }
            continue;
        }
        index_by_key.insert(key, deduped.len());
        deduped.push(agent);
    }
    deduped
}

pub(super) fn attach_active_agent_personal_limit_surfaces(agents: &mut [Value]) {
    for agent in agents.iter_mut() {
        let preferred_limits = preferred_active_agent_limit_surface(&agent["client_live_meter"]);
        if preferred_limits.is_none() {
            if let Some(root) = agent.as_object_mut() {
                root.insert(
                    "personal_client_limit".to_string(),
                    json!({
                        "status": "missing",
                        "label_text": "Лимит клиента сейчас:",
                        "value_text": "н/д",
                        "tooltip": "Личный online limit surface этого агента ещё не materialized из его собственного VS Code workspace/thread contour. Другие источники для строки лимитов запрещены.",
                    }),
                );
            }
            continue;
        }
        let primary_remaining_percent = preferred_limits
            .as_ref()
            .map(|limits| limits.primary_remaining_percent)
            .unwrap_or(0.0);
        let primary_used_percent = preferred_limits
            .as_ref()
            .map(|limits| limits.primary_used_percent)
            .unwrap_or(100.0 - primary_remaining_percent);
        let secondary_remaining_percent = preferred_limits
            .as_ref()
            .map(|limits| limits.secondary_remaining_percent)
            .unwrap_or(0.0);
        let secondary_used_percent = preferred_limits
            .as_ref()
            .map(|limits| limits.secondary_used_percent)
            .unwrap_or(100.0 - secondary_remaining_percent);
        let source_label = preferred_limits
            .as_ref()
            .map(|limits| limits.source_label)
            .unwrap_or("thread-local VS Code workspace limit contour");
        let source_kind = preferred_limits
            .as_ref()
            .map(|limits| limits.source_kind)
            .unwrap_or("missing");
        let (label_text, source_note) = match source_kind {
            "status_bar_exact" => (
                "Лимит клиента сейчас:",
                format!(
                    "Это exact global live limit contour клиента из {}. Он должен совпадать с VS Code toolbar и с отдельной строкой `Лимит клиента сейчас` в live budget surface.",
                    source_label
                ),
            ),
            "thread_local_rollout" => (
                "Личный thread-limit агента:",
                format!(
                    "Это текущий live limit contour именно этого агента, materialized из {}.",
                    source_label
                ),
            ),
            _ => (
                "Личный thread-limit агента:",
                format!(
                    "Это текущий live limit contour именно этого агента, materialized из {}.",
                    source_label
                ),
            ),
        };
        if let Some(root) = agent.as_object_mut() {
            root.insert(
                "personal_client_limit".to_string(),
                json!({
                    "status": "observed",
                    "label_text": label_text,
                    "value_text": format!(
                        "5ч остаётся {}, 7д остаётся {}",
                        active_agent_limit_percent_text(primary_remaining_percent),
                        active_agent_limit_percent_text(secondary_remaining_percent),
                    ),
                    "primary_used_percent": primary_used_percent,
                    "primary_remaining_percent": primary_remaining_percent,
                    "secondary_used_percent": secondary_used_percent,
                    "secondary_remaining_percent": secondary_remaining_percent,
                    "tooltip": format!(
                        "Этот ряд показывает online limit contour именно этого агента из {}.\n- Лимит 5ч: остаётся {} (использовано {})\n- Лимит 7д: остаётся {} (использовано {})\n- {}",
                        source_label,
                        active_agent_limit_percent_text(primary_remaining_percent),
                        active_agent_limit_percent_text(primary_used_percent),
                        active_agent_limit_percent_text(secondary_remaining_percent),
                        active_agent_limit_percent_text(secondary_used_percent),
                        source_note,
                    ),
                }),
            );
        }
    }
}

pub(crate) async fn collect_active_agent_live_budget_surface(
    db: &Client,
    current_repo_root: &Path,
    activity: &Value,
) -> Result<Value> {
    let total_started = std::time::Instant::now();
    let config = load_config(current_repo_root)?;
    let profile = resolve_profile(&config, None, current_repo_root)?;
    let session_gap_ms = profile.session_gap_minutes as i64 * 60_000;
    let live_events_started = std::time::Instant::now();
    let mut live_events = load_dashboard_token_events(db, current_repo_root, false).await?;
    active_agent_profile_log(
        "load_dashboard_token_events",
        live_events_started.elapsed().as_millis(),
        &format!("events={}", live_events.len()),
    );
    live_events.sort_by_key(|event| event.created_at_epoch_ms);
    let live_events = reconcile_followup_recovery(&live_events, session_gap_ms);
    let now_epoch_ms = current_epoch_ms()?;
    let exact_limits_started = std::time::Instant::now();
    let exact_client_limits_observation = dashboard_exact_client_rate_limits_resolution()
        .await?
        .observation;
    active_agent_profile_log(
        "exact_client_limits_resolution",
        exact_limits_started.elapsed().as_millis(),
        &format!("observed={}", exact_client_limits_observation.is_some()),
    );
    let current_repo_root_fallback = current_repo_root.display().to_string();
    let threads_by_id = activity["client_recent_threads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|thread| {
            let thread_id = thread["thread_id"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            Some((thread_id.to_string(), thread.clone()))
        })
        .collect::<HashMap<_, _>>();

    let active_entries = active_agent_activity_entries(activity, now_epoch_ms);
    let active_agent_scopes = active_entries
        .iter()
        .filter_map(|item| item["agent_scope"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let overrides_started = std::time::Instant::now();
    let agent_display_name_overrides =
        load_agent_display_name_overrides_for_scopes(db, active_agent_scopes).await?;
    active_agent_profile_log(
        "load_agent_display_name_overrides",
        overrides_started.elapsed().as_millis(),
        &format!("overrides={}", agent_display_name_overrides.len()),
    );
    let mut seen = BTreeSet::new();
    let mut agents = Vec::new();
    for active in active_entries {
        let entry_started = std::time::Instant::now();
        let Some((selector, project_repo_root)) = active_agent_selector_from_activity(&active)
        else {
            continue;
        };
        if !seen.insert(selector.signature_key()) {
            continue;
        }
        let (scoped_events, kpi_selector, used_scope_fallback) =
            active_agent_personal_kpi_window(&live_events, &selector, now_epoch_ms);
        let scoped_summary = summarize_events(
            &scoped_events,
            now_epoch_ms,
            &config.measurement,
            &config.contract,
        );
        let scoped_live_events =
            filter_events_for_personal_kpi_selector(&live_events, &kpi_selector);
        let primary_limit_events = rolling_window_events_for_duration(
            &scoped_live_events,
            now_epoch_ms,
            PERSONAL_AGENT_KPI_WINDOW_HOURS,
        );
        let secondary_limit_events = rolling_window_events_for_duration(
            &scoped_live_events,
            now_epoch_ms,
            ACTIVE_AGENT_SECONDARY_LIMIT_WINDOW_HOURS,
        );
        let primary_limit_summary = summarize_events(
            &primary_limit_events,
            now_epoch_ms,
            &config.measurement,
            &config.contract,
        );
        let secondary_limit_summary = summarize_events(
            &secondary_limit_events,
            now_epoch_ms,
            &config.measurement,
            &config.contract,
        );
        let thread_meta = selector
            .thread_id
            .as_deref()
            .and_then(|thread_id| threads_by_id.get(thread_id));
        if !thread_meta.is_some_and(recent_client_thread_json_has_connected_model) {
            continue;
        }
        let repo_root_string = project_repo_root
            .clone()
            .unwrap_or_else(|| current_repo_root_fallback.clone());
        let rollout_started = std::time::Instant::now();
        let live_rollout_meter = selector.thread_id.as_deref().and_then(|thread_id| {
            codex_threads::latest_rollout_client_meter_observation_for_thread(thread_id)
                .ok()
                .flatten()
        });
        let rollout_elapsed_ms = rollout_started.elapsed().as_millis();
        let snapshot_fields_started = std::time::Instant::now();
        let snapshot_fields = active_agent_budget_fields_from_thread_bound_snapshot(
            current_repo_root,
            &selector,
            now_epoch_ms as u64,
        );
        let snapshot_fields_present = snapshot_fields.is_some();
        let snapshot_fields_elapsed_ms = snapshot_fields_started.elapsed().as_millis();
        let client_meter_started = std::time::Instant::now();
        let (client_live_meter, mut personal_agent_kpi) =
            if let Some(rollout_meter) = live_rollout_meter.as_ref() {
                let client_live_meter = build_client_live_meter_json(
                    Some(rollout_meter),
                    selector.thread_id.as_deref(),
                    exact_client_limits_observation.as_ref(),
                );
                let personal_agent_kpi = preferred_personal_agent_kpi(
                    &scoped_summary,
                    Some(&kpi_selector),
                    Some(&client_live_meter),
                );
                (client_live_meter, personal_agent_kpi)
            } else if let Some((client_live_meter, personal_agent_kpi)) = snapshot_fields {
                (
                    client_live_meter_with_exact_status_bar(
                        client_live_meter,
                        exact_client_limits_observation.as_ref(),
                    ),
                    personal_agent_kpi,
                )
            } else {
                let client_live_meter = build_client_live_meter_json(
                    None,
                    selector.thread_id.as_deref(),
                    exact_client_limits_observation.as_ref(),
                );
                let personal_agent_kpi = preferred_personal_agent_kpi(
                    &scoped_summary,
                    Some(&kpi_selector),
                    Some(&client_live_meter),
                );
                (client_live_meter, personal_agent_kpi)
            };
        let client_meter_elapsed_ms = client_meter_started.elapsed().as_millis();
        if used_scope_fallback && personal_agent_kpi["status"].as_str() == Some("missing") {
            if let Some(node) = personal_agent_kpi.as_object_mut() {
                node.insert(
                    "scope_resolution".to_string(),
                    Value::from("online_limit_contour_missing_for_thread"),
                );
                node.insert(
                    "summary".to_string(),
                    Value::from(
                        "Для личного 5ч KPI thread-bound online contour не materialized. Same-agent_scope measured fallback для этого KPI запрещён.",
                    ),
                );
            }
        }
        let fallback_agent_label = active_agent_display_label(&selector.project_code);
        let agent_label = resolved_active_agent_label(
            agent_display_name_overrides
                .get(&selector.agent_scope)
                .map(String::as_str),
            thread_meta,
            &fallback_agent_label,
            active["headline"].as_str(),
            selector.agent_scope.as_str(),
        );
        if user_visible_agent_activity_is_proof_runtime(
            Some(&selector.project_code),
            Some(&selector.agent_scope),
            selector.thread_id.as_deref(),
            active["headline"].as_str(),
            thread_meta
                .and_then(|thread| thread["title"].as_str())
                .or(Some(agent_label.as_str())),
        ) {
            continue;
        }
        agents.push(json!({
            "project_code": selector.project_code,
            "namespace_code": selector.namespace_code,
            "project_repo_root": repo_root_string,
            "agent_scope": selector.agent_scope,
            "thread_id": selector.thread_id,
            "agent_label": agent_label,
            "thread_title": thread_meta.and_then(|thread| thread["title"].as_str()),
            "cwd": thread_meta.and_then(|thread| thread["cwd"].as_str()),
            "heartbeat_at_epoch_ms": active["heartbeat_at_epoch_ms"].clone(),
            "expires_at_epoch_ms": active["expires_at_epoch_ms"].clone(),
            "personal_agent_kpi": personal_agent_kpi,
            "client_live_meter": client_live_meter,
            "limit_attribution": {
                "primary_window_tokens": active_agent_limit_weight_tokens(&primary_limit_summary),
                "secondary_window_tokens": active_agent_limit_weight_tokens(&secondary_limit_summary),
            },
        }));
        active_agent_profile_log(
            "entry",
            entry_started.elapsed().as_millis(),
            &format!(
                "scope={} thread_id={} rollout_ms={} snapshot_fields_ms={} client_meter_ms={} rollout_present={} snapshot_present={}",
                selector.agent_scope,
                selector.thread_id.as_deref().unwrap_or("none"),
                rollout_elapsed_ms,
                snapshot_fields_elapsed_ms,
                client_meter_elapsed_ms,
                live_rollout_meter.is_some(),
                snapshot_fields_present,
            ),
        );
    }
    let mut agents = dedup_active_agents_by_identity(agents);
    attach_active_agent_personal_limit_surfaces(&mut agents);
    let aggregate = active_agent_kpi_aggregate(&agents);
    let result = json!({
        "source": "observe_active_agent_budget_v1",
        "captured_at_epoch_ms": now_epoch_ms,
        "headline": {
            "title": "Средний KPI активных агентов",
            "value_text": aggregate["reply_prefix"].clone(),
            "scope_label": aggregate["scope_label"].clone(),
        },
        "aggregate": aggregate,
        "agents": agents,
    });
    active_agent_profile_log(
        "total",
        total_started.elapsed().as_millis(),
        &format!(
            "agents={}",
            result["agents"]
                .as_array()
                .map(|items| items.len())
                .unwrap_or(0)
        ),
    );
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolved_active_agent_label_prefers_user_defined_display_name() {
        let thread = json!({
            "agent_nickname": "Codex",
            "agent_role": "Assistant"
        });
        let label = resolved_active_agent_label(
            Some("Мой агент"),
            Some(&thread),
            "Amai",
            Some("active headline"),
            "amai::continuity::default",
        );
        assert_eq!(label, "Мой агент");
    }

    #[test]
    fn attach_active_agent_personal_limit_surfaces_prefers_exact_status_bar_limits() {
        let mut agents = vec![json!({
            "client_live_meter": {
                "status": "observed",
                "current_thread_bound": true,
                "ended_at_epoch_ms": 1775056740000u64,
                "primary_limit_used_percent": 39,
                "primary_limit_remaining_percent": 61,
                "primary_window_duration_mins": 300,
                "primary_resets_at_epoch_seconds": 1775063220u64,
                "secondary_limit_used_percent": 72,
                "secondary_limit_remaining_percent": 28,
                "status_bar_rate_limits": {
                    "status": "observed",
                    "observed_at_epoch_ms": 1775056740000u64,
                    "primary_limit_used_percent": 7.0,
                    "primary_limit_remaining_percent": 93.0,
                    "secondary_limit_used_percent": 2.0,
                    "secondary_limit_remaining_percent": 98.0,
                    "primary_window_duration_mins": 300,
                    "primary_resets_at_epoch_seconds": 1775063220u64
                }
            }
        })];

        attach_active_agent_personal_limit_surfaces(&mut agents);

        assert_eq!(
            agents[0]["personal_client_limit"]["value_text"].as_str(),
            Some("5ч остаётся 93.00%, 7д остаётся 98.00%")
        );
        assert_eq!(
            agents[0]["personal_client_limit"]["primary_used_percent"].as_f64(),
            Some(7.0)
        );
        assert_eq!(
            agents[0]["personal_client_limit"]["label_text"].as_str(),
            Some("Лимит клиента сейчас:")
        );
        assert!(
            agents[0]["personal_client_limit"]["tooltip"]
                .as_str()
                .is_some_and(|tooltip| tooltip.contains("status-bar"))
        );
    }

    #[test]
    fn attach_active_agent_personal_limit_surfaces_uses_thread_local_limits_without_status_bar() {
        let mut agents = vec![json!({
            "client_live_meter": {
                "status": "observed",
                "thread_id": "thread-amai",
                "current_thread_bound": true,
                "ended_at_epoch_ms": 1775056740000u64,
                "primary_limit_used_percent": 7.0,
                "primary_limit_remaining_percent": 93.0,
                "primary_window_duration_mins": 300,
                "primary_resets_at_epoch_seconds": 1775063220u64,
                "secondary_limit_used_percent": 2.0,
                "secondary_limit_remaining_percent": 98.0,
                "status_bar_rate_limits": {
                    "status": "missing"
                }
            }
        })];

        attach_active_agent_personal_limit_surfaces(&mut agents);

        assert_eq!(
            agents[0]["personal_client_limit"]["status"].as_str(),
            Some("observed")
        );
        assert_eq!(
            agents[0]["personal_client_limit"]["label_text"].as_str(),
            Some("Личный thread-limit агента:")
        );
        assert_eq!(
            agents[0]["personal_client_limit"]["value_text"].as_str(),
            Some("5ч остаётся 93.00%, 7д остаётся 98.00%")
        );
    }

    #[test]
    fn attach_active_agent_personal_limit_surfaces_fail_closed_without_any_online_limit_source() {
        let mut agents = vec![json!({
            "client_live_meter": {
                "status": "observed",
                "thread_id": "thread-amai",
                "current_thread_bound": true,
                "status_bar_rate_limits": {
                    "status": "missing"
                }
            }
        })];

        attach_active_agent_personal_limit_surfaces(&mut agents);

        assert_eq!(
            agents[0]["personal_client_limit"]["status"].as_str(),
            Some("missing")
        );
        assert_eq!(
            agents[0]["personal_client_limit"]["label_text"].as_str(),
            Some("Лимит клиента сейчас:")
        );
        assert_eq!(
            agents[0]["personal_client_limit"]["value_text"].as_str(),
            Some("н/д")
        );
        assert!(
            agents[0]["personal_client_limit"]["tooltip"]
                .as_str()
                .is_some_and(|tooltip| tooltip.contains("Другие источники"))
        );
    }

    #[test]
    fn dedup_active_agents_by_identity_prefers_thread_bound_observed_entry() {
        let deduped = dedup_active_agents_by_identity(vec![
            json!({
                "project_code": "amai",
                "namespace_code": "continuity",
                "agent_scope": "amai::continuity::default",
                "thread_id": null,
                "heartbeat_at_epoch_ms": 100,
                "client_live_meter": {
                    "status": "missing",
                    "current_thread_bound": false
                },
                "personal_agent_kpi": {
                    "reply_prefix": "5ч KPI: экономия 64.81%"
                }
            }),
            json!({
                "project_code": "amai",
                "namespace_code": "continuity",
                "agent_scope": "amai::continuity::default",
                "thread_id": "thread-live",
                "heartbeat_at_epoch_ms": 200,
                "client_live_meter": {
                    "status": "observed",
                    "current_thread_bound": true
                },
                "personal_agent_kpi": {
                    "reply_prefix": "5ч KPI: экономия 66.15%"
                }
            }),
            json!({
                "project_code": "bug_bounty",
                "namespace_code": "continuity",
                "agent_scope": "bug_bounty::continuity::default",
                "thread_id": "thread-bounty",
                "heartbeat_at_epoch_ms": 150,
                "client_live_meter": {
                    "status": "observed",
                    "current_thread_bound": true
                },
                "personal_agent_kpi": {
                    "reply_prefix": "5ч KPI: переплата 72.23%"
                }
            }),
        ]);

        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0]["thread_id"], json!("thread-live"));
        assert_eq!(
            deduped[0]["personal_agent_kpi"]["reply_prefix"],
            json!("5ч KPI: экономия 66.15%")
        );
        assert_eq!(deduped[1]["thread_id"], json!("thread-bounty"));
    }

    #[test]
    fn active_agent_kpi_aggregate_averages_only_when_all_active_agents_are_observed() {
        let value = active_agent_kpi_aggregate(&[
            json!({
                "personal_agent_kpi": {
                    "signed_kpi_percent": 60.0
                }
            }),
            json!({
                "personal_agent_kpi": {
                    "signed_kpi_percent": 20.0
                }
            }),
        ]);
        assert_eq!(value["status"].as_str(), Some("observed"));
        assert_eq!(
            value["reply_prefix"].as_str(),
            Some("5ч KPI: экономия 40.00%")
        );
        assert_eq!(value["active_count"].as_u64(), Some(2));
        assert_eq!(value["missing_count"].as_u64(), Some(0));
    }

    #[test]
    fn active_agent_kpi_aggregate_averages_mixed_saving_and_overspend_by_sign() {
        let value = active_agent_kpi_aggregate(&[
            json!({
                "personal_agent_kpi": {
                    "signed_kpi_percent": 66.15
                }
            }),
            json!({
                "personal_agent_kpi": {
                    "signed_kpi_percent": -44.0
                }
            }),
        ]);
        assert_eq!(value["status"].as_str(), Some("observed"));
        assert_eq!(value["classification"].as_str(), Some("saving"));
        assert_eq!(
            value["reply_prefix"].as_str(),
            Some("5ч KPI: экономия 11.08%")
        );
        assert_eq!(value["active_count"].as_u64(), Some(2));
        assert_eq!(value["missing_count"].as_u64(), Some(0));
    }

    #[test]
    fn active_agent_kpi_aggregate_fails_closed_when_any_active_agent_is_missing() {
        let value = active_agent_kpi_aggregate(&[
            json!({
                "personal_agent_kpi": {
                    "signed_kpi_percent": 60.0
                }
            }),
            json!({
                "personal_agent_kpi": {
                    "reply_prefix": "5ч KPI: н/д"
                }
            }),
        ]);
        assert_eq!(value["status"].as_str(), Some("partial"));
        assert_eq!(value["reply_prefix"].as_str(), Some("5ч KPI: н/д"));
        assert_eq!(value["active_count"].as_u64(), Some(2));
        assert_eq!(value["missing_count"].as_u64(), Some(1));
    }
}
