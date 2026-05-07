use super::*;

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn current_session_live_response_turns(
    turns: &[LiveResponseTurnObservation],
    current_thread_id: Option<&str>,
    session_gap_minutes: u64,
) -> Vec<LiveResponseTurnObservation> {
    let Some(current_thread_id) = current_thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    let mut thread_turns = turns
        .iter()
        .filter(|turn| turn.thread_id == current_thread_id)
        .cloned()
        .collect::<Vec<_>>();
    if thread_turns.is_empty() {
        return Vec::new();
    }
    thread_turns.sort_by_key(|turn| (turn.started_at_epoch_ms, turn.ended_at_epoch_ms));
    let session_gap_ms = session_gap_minutes as i64 * 60_000;
    let mut start_index = thread_turns.len().saturating_sub(1);
    while start_index > 0 {
        let previous_ended_at = thread_turns[start_index - 1]
            .ended_at_epoch_ms
            .max(thread_turns[start_index - 1].started_at_epoch_ms);
        let current_started_at = thread_turns[start_index].started_at_epoch_ms;
        if current_started_at.saturating_sub(previous_ended_at) > session_gap_ms.max(0) {
            break;
        }
        start_index -= 1;
    }
    thread_turns.split_off(start_index)
}

fn live_response_latency_slice_breakdown(turns: &[LiveResponseTurnObservation]) -> Value {
    let mut grouped = BTreeMap::<String, Vec<f64>>::new();
    let mut current_latency = BTreeMap::<String, f64>::new();

    for turn in turns {
        grouped
            .entry("mixed".to_string())
            .or_default()
            .push(turn.latency_ms);
        current_latency.insert("mixed".to_string(), turn.latency_ms);
        grouped
            .entry(turn.state.clone())
            .or_default()
            .push(turn.latency_ms);
        current_latency.insert(turn.state.clone(), turn.latency_ms);
    }

    let order = ["mixed", "hot", "cold"];
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

    Value::Array(slices)
}

fn live_response_turn_json(turn: Option<&LiveResponseTurnObservation>) -> Value {
    let Some(turn) = turn else {
        return Value::Null;
    };
    json!({
        "thread_id": turn.thread_id,
        "turn_id": turn.turn_id,
        "state": turn.state,
        "latency_ms": turn.latency_ms,
        "started_at_epoch_ms": turn.started_at_epoch_ms,
        "ended_at_epoch_ms": turn.ended_at_epoch_ms,
    })
}

fn live_response_latency_scope(turns: &[LiveResponseTurnObservation]) -> Value {
    let started_at_epoch_ms = turns
        .first()
        .map(|turn| turn.started_at_epoch_ms)
        .unwrap_or_default();
    let ended_at_epoch_ms = turns
        .last()
        .map(|turn| turn.ended_at_epoch_ms)
        .unwrap_or_default();
    json!({
        "sample_count": turns.len(),
        "started_at_epoch_ms": started_at_epoch_ms,
        "ended_at_epoch_ms": ended_at_epoch_ms,
        "latency_slices": live_response_latency_slice_breakdown(turns),
        "latest_turn": live_response_turn_json(turns.last()),
    })
}

pub(super) fn live_response_latency_surface_signature(surface: &Value) -> String {
    hex_sha256(&serde_json::to_vec(surface).unwrap_or_else(|_| surface.to_string().into_bytes()))
}

pub(super) fn annotate_live_response_latency_surface(
    surface: &mut Value,
    current_live_turn: &Value,
) {
    let current_session = &surface["current_session"];
    let current_sample_count = current_session["sample_count"].as_u64().unwrap_or_default();
    let current_thread_id = surface["current_thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let current_turn_status = current_live_turn["status"].as_str().unwrap_or("missing");
    let current_turn_thread_id = current_live_turn["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let same_thread = current_thread_id.is_some() && current_thread_id == current_turn_thread_id;
    let current_thread_id_value = current_thread_id.map(str::to_string);
    let current_turn_thread_id_value = current_turn_thread_id.map(str::to_string);
    let latest_turn_ended_at_epoch_ms =
        current_session["latest_turn"]["ended_at_epoch_ms"].as_i64();
    let current_turn_started_at_epoch_ms = current_live_turn["started_at_epoch_ms"].as_i64();
    let current_turn_started_after_latest_series = matches!(
        (current_turn_started_at_epoch_ms, latest_turn_ended_at_epoch_ms),
        (Some(started_at), Some(latest_ended_at)) if started_at > latest_ended_at
    );

    let (status, note) = if current_sample_count == 0 {
        (
            "no_current_series",
            "В текущем чате пока нет накопленной серии ответов Amai, поэтому online-карточка ждёт первый реальный ответ.",
        )
    } else if same_thread
        && current_turn_status == "no_amai_activity_in_current_live_turn"
        && current_turn_started_after_latest_series
    {
        (
            "recent_same_chat_series_previous_turn",
            "Текущий live-turn уже начался, но в нём пока нет новых Amai-событий. Показанная текущая серия относится к недавним ответам этого же чата из предыдущего turn.",
        )
    } else if same_thread
        && matches!(
            current_turn_status,
            "exact_pair_materialized"
                | "activity_observed_exact_pair_unavailable"
                | "thread_activity_observed_turn_open"
        )
    {
        (
            "current_turn_activity_visible",
            "Показанная текущая серия уже совпадает с live-turn этого чата и растёт по мере новых ответов Amai.",
        )
    } else if current_turn_status == "current_thread_unbound" {
        (
            "current_turn_unbound",
            "Текущая серия уже видна по thread-окну Amai, но rollout meter для текущего turn ещё не materialized как current-thread bound.",
        )
    } else {
        (
            "recent_series_without_turn_alignment",
            "Показанная текущая серия относится к свежим ответам Amai этого чата, но текущий live-turn snapshot пока не совпал с ней по turn-границе.",
        )
    };

    if let Some(object) = surface.as_object_mut() {
        object.insert(
            "current_session_relation".to_string(),
            json!({
                "status": status,
                "note": note,
                "current_sample_count": current_sample_count,
                "current_thread_id": current_thread_id_value,
                "current_live_turn_status": current_turn_status,
                "current_live_turn_thread_id": current_turn_thread_id_value,
                "current_turn_started_after_latest_series": current_turn_started_after_latest_series,
            }),
        );
    }
}

fn file_hint_label_from_query(query: &str) -> Option<String> {
    let query = query.trim();
    if query.is_empty() {
        return None;
    }
    let label = Path::new(query)
        .file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(query)
        .trim();
    (!label.is_empty()).then(|| label.to_string())
}

pub(super) async fn current_workspace_live_response_scope(
    db: &Client,
    repo_root: &Path,
) -> Result<Option<(String, String)>> {
    let repo_root_display = repo_root.display().to_string();
    let Ok(project) = postgres::get_project_by_repo_root(db, &repo_root_display).await else {
        return Ok(None);
    };
    let snapshot = postgres::latest_observability_snapshot_for_project(
        db,
        "working_state_restore",
        "working_state_restore",
        &project.code,
    )
    .await?;
    let namespace_code = snapshot
        .as_ref()
        .and_then(|value| value["working_state_restore"]["namespace"]["code"].as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("continuity")
        .to_string();
    Ok(Some((project.code, namespace_code)))
}

pub(super) fn build_current_thread_live_file_hints(
    rolling_window_events: &[TokenBudgetEvent],
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    current_thread_id: Option<&str>,
    session_gap_minutes: u64,
) -> Value {
    let Some(current_thread_id) = current_thread_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return json!({
            "thread_id": Value::Null,
            "sample_count": 0,
            "hints": [],
            "note": "Для текущего thread ещё нет живых file-target hints от Amai."
        });
    };
    let mut file_events = rolling_window_events
        .iter()
        .filter(|event| {
            project_code.is_none_or(|value| event.project == value)
                && namespace_code.is_none_or(|value| event.namespace == value)
        })
        .filter(|event| event.thread_id.as_deref().map(str::trim) == Some(current_thread_id))
        .filter(|event| event.traffic_class == "live")
        .filter(|event| event.target_kind == "file")
        .filter(|event| event.measurement_scope == "retrieval_lower_bound")
        .filter(|event| event.quality_ok)
        .filter_map(|event| {
            file_hint_label_from_query(&event.query).map(|hint| {
                (
                    event.occurred_at_epoch_ms.max(event.created_at_epoch_ms),
                    hint,
                    event.query.clone(),
                )
            })
        })
        .collect::<Vec<_>>();
    if file_events.is_empty() {
        return json!({
            "thread_id": current_thread_id,
            "sample_count": 0,
            "hints": [],
            "note": "Для текущего thread ещё нет живых file-target hints от Amai."
        });
    }
    file_events.sort_by_key(|(ts, _, _)| *ts);
    let session_gap_ms = session_gap_minutes as i64 * 60_000;
    let mut start_index = file_events.len().saturating_sub(1);
    while start_index > 0 {
        let previous_ts = file_events[start_index - 1].0;
        let current_ts = file_events[start_index].0;
        if current_ts.saturating_sub(previous_ts) > session_gap_ms.max(0) {
            break;
        }
        start_index -= 1;
    }
    let current_session = &file_events[start_index..];
    let mut unique_hints = Vec::new();
    let mut seen = BTreeSet::new();
    for (_, hint, query) in current_session.iter().rev() {
        let key = format!("{hint}\u{0}{query}");
        if seen.insert(key) {
            unique_hints.push(json!({
                "label": hint,
                "query": query,
            }));
        }
        if unique_hints.len() >= 3 {
            break;
        }
    }
    unique_hints.reverse();
    json!({
        "thread_id": current_thread_id,
        "sample_count": current_session.len(),
        "hints": unique_hints,
        "note": "Живые file-target hints собираются из последних same-thread retrieval_lower_bound file-событий Amai и нужны только как ранняя операторская подсказка до полного working-state snapshot."
    })
}

pub(super) fn build_live_response_latency_surface(
    repo_root: &Path,
    project_code: Option<&str>,
    namespace_code: Option<&str>,
    rolling_window_events: &[TokenBudgetEvent],
    rolling_window_hours: Option<u64>,
    session_gap_minutes: u64,
    current_series_minutes: u64,
    now_epoch_ms: i64,
) -> Result<Value> {
    let repo_root_str = repo_root.to_str().unwrap_or_default();
    let current_thread_id = codex_threads::current_thread_id().or_else(|| {
        codex_threads::preferred_thread_id_for_repo(repo_root_str)
            .ok()
            .flatten()
    });
    let mut missing_thread_id = 0u64;
    let mut quality_rejected = 0u64;
    let mut invalid_latency = 0u64;
    let mut rolling_window_turns = Vec::new();
    for event in rolling_window_events.iter().filter(|event| {
        project_code.is_none_or(|value| event.project == value)
            && namespace_code.is_none_or(|value| event.namespace == value)
    }) {
        if event.measurement_scope != "retrieval_lower_bound" {
            continue;
        }
        if !event.latency_ms.is_finite() {
            invalid_latency = invalid_latency.saturating_add(1);
            continue;
        }
        if event.traffic_class == "live" && !event.quality_ok {
            quality_rejected = quality_rejected.saturating_add(1);
            continue;
        }
        let thread_id = event.thread_id.as_deref().map(str::trim).unwrap_or("");
        if thread_id.is_empty() {
            missing_thread_id = missing_thread_id.saturating_add(1);
            continue;
        }
        let turn_id = event.turn_id.as_deref().unwrap_or("").trim().to_string();
        let state = normalize_latency_state(&event.cold_warm_state).to_string();
        rolling_window_turns.push(LiveResponseTurnObservation {
            thread_id: thread_id.to_string(),
            turn_id,
            state,
            started_at_epoch_ms: event.occurred_at_epoch_ms.max(event.created_at_epoch_ms),
            ended_at_epoch_ms: event.created_at_epoch_ms.max(event.occurred_at_epoch_ms),
            latency_ms: event.latency_ms,
        });
    }
    rolling_window_turns.sort_by_key(|turn| (turn.started_at_epoch_ms, turn.ended_at_epoch_ms));
    let current_series_window_ms = current_series_minutes.saturating_mul(60_000) as i64;
    let current_session_turns = current_thread_id
        .as_deref()
        .map(|thread_id| {
            rolling_window_turns
                .iter()
                .filter(|turn| turn.thread_id == thread_id)
                .filter(|turn| {
                    let ended_at = turn.ended_at_epoch_ms as i64;
                    ended_at >= now_epoch_ms.saturating_sub(current_series_window_ms)
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let current_thread_turn_count = current_thread_id
        .as_deref()
        .map(|thread| {
            rolling_window_turns
                .iter()
                .filter(|turn| turn.thread_id == thread)
                .count() as u64
        })
        .unwrap_or_default();
    let current_session_count = current_session_turns.len() as u64;
    let outside_current_series_window =
        current_thread_turn_count.saturating_sub(current_session_count);
    let current_thread_live_file_hints = build_current_thread_live_file_hints(
        rolling_window_events,
        project_code,
        namespace_code,
        current_thread_id.as_deref(),
        session_gap_minutes,
    );
    let excluded_total = missing_thread_id
        .saturating_add(quality_rejected)
        .saturating_add(invalid_latency);
    let current_session_exclusions = json!({
        "total": excluded_total.saturating_add(outside_current_series_window),
        "missing_thread_id": missing_thread_id,
        "quality_rejected": quality_rejected,
        "invalid_latency": invalid_latency,
        "outside_current_series_window": outside_current_series_window,
        "current_series_minutes": current_series_minutes,
    });
    Ok(json!({
        "source": "amai_retrieval_lower_bound_thread_window_v1",
        "rolling_window_hours": rolling_window_hours.unwrap_or(24),
        "current_thread_id": current_thread_id,
        "rolling_window": live_response_latency_scope(&rolling_window_turns),
        "current_session": live_response_latency_scope(&current_session_turns),
        "current_session_exclusions": current_session_exclusions,
        "current_thread_live_file_hints": current_thread_live_file_hints,
        "note": format!(
            "Время считается по Amai retrieval_lower_bound событиям текущего проекта. Текущая серия = последние {} минут текущего thread, а окно 24 часов показывает накопительный Amai-only тренд по проекту.",
            current_series_minutes
        )
    }))
}
