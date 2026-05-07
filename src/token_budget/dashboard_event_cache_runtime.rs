use super::*;

pub(super) fn cached_dashboard_token_events(
    repo_root: &Path,
    snapshot_kinds: &[&str],
    signature: &str,
) -> Option<Vec<TokenBudgetEvent>> {
    let cache = DASHBOARD_TOKEN_EVENTS_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().ok()?;
    let repo_root = canonical_repo_root(repo_root);
    let entry = guard
        .as_ref()
        .filter(|entry| repo_root == entry.repo_root)
        .cloned()
        .or_else(|| load_shared_dashboard_token_events(&repo_root))?;
    if entry.signature != signature
        || !entry
            .snapshot_kinds
            .iter()
            .map(String::as_str)
            .eq(snapshot_kinds.iter().copied())
    {
        return None;
    }
    *guard = Some(entry.clone());
    Some(entry.raw_events)
}

pub(super) fn cached_dashboard_current_session_events(
    repo_root: &Path,
    signature: &str,
    session_gap_ms: i64,
) -> Option<Vec<TokenBudgetEvent>> {
    let cache = DASHBOARD_CURRENT_SESSION_EVENTS_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().ok()?;
    let repo_root = canonical_repo_root(repo_root);
    let entry = guard
        .as_ref()
        .filter(|entry| repo_root == entry.repo_root)
        .cloned()
        .or_else(|| {
            load_shared_dashboard_current_session_events(&repo_root, signature, session_gap_ms).map(
                |events| DashboardCurrentSessionEventsCache {
                    repo_root: repo_root.clone(),
                    signature: signature.to_string(),
                    session_gap_ms,
                    events,
                },
            )
        })?;
    if entry.signature != signature || entry.session_gap_ms != session_gap_ms {
        return None;
    }
    *guard = Some(entry.clone());
    Some(entry.events)
}

pub(super) fn current_dashboard_live_turn_retrieval_invalidation_epoch_ms(repo_root: &Path) -> i64 {
    load_shared_dashboard_live_turn_retrieval_invalidation(repo_root).unwrap_or_default()
}

pub(super) fn current_dashboard_token_events_invalidation_epoch_ms(repo_root: &Path) -> i64 {
    load_shared_dashboard_token_events_invalidation(repo_root).unwrap_or_default()
}

pub(crate) fn bump_dashboard_token_events_invalidation(
    repo_root: &Path,
    recorded_at_epoch_ms: i64,
) -> Result<()> {
    let latest =
        current_dashboard_token_events_invalidation_epoch_ms(repo_root).max(recorded_at_epoch_ms);
    write_shared_dashboard_token_events_invalidation(repo_root, latest)
}

pub(crate) fn bump_dashboard_live_turn_retrieval_invalidation(
    repo_root: &Path,
    recorded_at_epoch_ms: i64,
) -> Result<()> {
    let latest = current_dashboard_live_turn_retrieval_invalidation_epoch_ms(repo_root)
        .max(recorded_at_epoch_ms);
    write_shared_dashboard_live_turn_retrieval_invalidation(repo_root, latest)
}

pub(super) fn cached_dashboard_live_turn_retrieval(
    repo_root: &Path,
    thread_id: &str,
    lower_bound: i64,
    upper_bound: i64,
    invalidation_epoch_ms: i64,
) -> Option<(BTreeSet<String>, u64)> {
    let cache = DASHBOARD_LIVE_TURN_RETRIEVAL_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().ok()?;
    let repo_root = canonical_repo_root(repo_root);
    let entry = guard
        .as_ref()
        .filter(|entry| repo_root == entry.repo_root)
        .cloned()
        .or_else(|| load_shared_dashboard_live_turn_retrieval_cache(&repo_root))?;
    let exact_window_match = entry.thread_id == thread_id
        && entry.lower_bound == lower_bound
        && entry.upper_bound == upper_bound
        && entry.invalidation_epoch_ms == invalidation_epoch_ms;
    let extendable_same_turn_window = entry.thread_id == thread_id
        && entry.lower_bound == lower_bound
        && entry.upper_bound <= upper_bound
        && entry.invalidation_epoch_ms == invalidation_epoch_ms;
    if !exact_window_match && !extendable_same_turn_window {
        return None;
    }
    let mut entry = entry;
    if extendable_same_turn_window && entry.upper_bound != upper_bound {
        entry.upper_bound = upper_bound;
        let _ = write_shared_dashboard_live_turn_retrieval_cache(&entry);
    }
    *guard = Some(entry.clone());
    Some((entry.context_pack_ids, entry.retrieval_count))
}

pub(super) fn store_dashboard_live_turn_retrieval(
    repo_root: &Path,
    thread_id: &str,
    lower_bound: i64,
    upper_bound: i64,
    invalidation_epoch_ms: i64,
    context_pack_ids: &BTreeSet<String>,
    retrieval_count: u64,
) {
    let cache = DASHBOARD_LIVE_TURN_RETRIEVAL_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    let entry = DashboardLiveTurnRetrievalCache {
        repo_root: canonical_repo_root(repo_root),
        thread_id: thread_id.to_string(),
        lower_bound,
        upper_bound,
        invalidation_epoch_ms,
        context_pack_ids: context_pack_ids.clone(),
        retrieval_count,
    };
    let _ = write_shared_dashboard_live_turn_retrieval_cache(&entry);
    *guard = Some(entry);
}

pub(super) fn cached_dashboard_token_events_entry(
    repo_root: &Path,
    snapshot_kinds: &[&str],
) -> Option<(
    Vec<postgres::ObservabilitySnapshotKindSummary>,
    Vec<TokenBudgetEvent>,
)> {
    let cache = DASHBOARD_TOKEN_EVENTS_CACHE.get_or_init(|| Mutex::new(None));
    let guard = cache.lock().ok()?;
    let entry = guard.as_ref()?;
    if canonical_repo_root(repo_root) == entry.repo_root
        && entry
            .snapshot_kinds
            .iter()
            .map(String::as_str)
            .eq(snapshot_kinds.iter().copied())
    {
        Some((entry.summary.clone(), entry.raw_events.clone()))
    } else {
        None
    }
}

pub(super) fn store_dashboard_token_events(
    repo_root: &Path,
    snapshot_kinds: &[&str],
    signature: &str,
    summary: &[postgres::ObservabilitySnapshotKindSummary],
    raw_events: &[TokenBudgetEvent],
) {
    let cache = DASHBOARD_TOKEN_EVENTS_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    let entry = DashboardTokenEventsCache {
        repo_root: canonical_repo_root(repo_root),
        signature: signature.to_string(),
        snapshot_kinds: snapshot_kinds
            .iter()
            .map(|kind| (*kind).to_string())
            .collect(),
        summary: summary.to_vec(),
        raw_events: raw_events.to_vec(),
    };
    let _ = write_shared_dashboard_token_events(&entry);
    *guard = Some(entry);
}

pub(super) fn store_dashboard_current_session_events(
    repo_root: &Path,
    signature: &str,
    session_gap_ms: i64,
    events: &[TokenBudgetEvent],
) {
    let cache = DASHBOARD_CURRENT_SESSION_EVENTS_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    let entry = DashboardCurrentSessionEventsCache {
        repo_root: canonical_repo_root(repo_root),
        signature: signature.to_string(),
        session_gap_ms,
        events: events.to_vec(),
    };
    let _ = write_shared_dashboard_current_session_events(&entry);
    *guard = Some(entry);
}

pub(super) fn dashboard_token_events_delta_limit(
    previous: &[postgres::ObservabilitySnapshotKindSummary],
    current: &[postgres::ObservabilitySnapshotKindSummary],
    max_delta: i64,
) -> Option<i64> {
    let previous = previous
        .iter()
        .map(|item| (item.snapshot_kind.clone(), item))
        .collect::<BTreeMap<_, _>>();
    let current = current
        .iter()
        .map(|item| (item.snapshot_kind.clone(), item))
        .collect::<BTreeMap<_, _>>();
    let kinds = previous
        .keys()
        .chain(current.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut delta_total = 0_i64;
    for kind in kinds {
        let previous_item = previous.get(&kind);
        let current_item = current.get(&kind);
        let previous_count = previous_item.map(|item| item.snapshots_count).unwrap_or(0);
        let current_count = current_item.map(|item| item.snapshots_count).unwrap_or(0);
        if current_count < previous_count {
            return None;
        }
        let previous_latest = previous_item.and_then(|item| item.latest_created_at_epoch_ms);
        let current_latest = current_item.and_then(|item| item.latest_created_at_epoch_ms);
        if current_latest < previous_latest {
            return None;
        }
        delta_total = delta_total.saturating_add(current_count.saturating_sub(previous_count));
    }
    if delta_total == 0 || delta_total > max_delta {
        None
    } else {
        Some(delta_total)
    }
}

pub(super) fn event_shadow_version_key(event: &TokenBudgetEvent) -> (i64, i64, String) {
    (
        event.created_at_epoch_ms,
        event.ingested_at_epoch_ms,
        event
            .snapshot_id
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| event.event_id.clone()),
    )
}

pub(super) fn live_usage_identity_shadow_key(event: &TokenBudgetEvent) -> Option<String> {
    if event.traffic_class != "live" {
        return None;
    }
    let event_id = event.event_id.trim();
    if event_id.is_empty() {
        return None;
    }
    Some(format!(
        "{}:{}:{}:{}:{}:{}",
        event.project,
        event.namespace,
        event.agent_scope,
        event.measurement_scope,
        event.source_kind,
        event_id
    ))
}

pub(super) fn dashboard_token_event_merge_key(event: &TokenBudgetEvent) -> String {
    live_usage_identity_shadow_key(event).unwrap_or_else(|| {
        format!(
            "{}:{}:{}:{}:{}:{}",
            event.project,
            event.namespace,
            event.measurement_scope,
            event.traffic_class,
            event.source_kind,
            event.event_id
        )
    })
}

pub(super) fn merge_dashboard_token_events(
    previous_events: Vec<TokenBudgetEvent>,
    delta_events: Vec<TokenBudgetEvent>,
) -> Vec<TokenBudgetEvent> {
    let mut merged = BTreeMap::<String, TokenBudgetEvent>::new();
    for event in previous_events.into_iter().chain(delta_events) {
        let key = dashboard_token_event_merge_key(&event);
        let version = event_shadow_version_key(&event);
        match merged.get(&key) {
            Some(existing) if event_shadow_version_key(existing) >= version => {}
            _ => {
                merged.insert(key, event);
            }
        }
    }
    merged.into_values().collect()
}
