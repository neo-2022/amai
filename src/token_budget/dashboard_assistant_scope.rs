use super::*;

const DASHBOARD_ASSISTANT_SCOPE_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/token_budget/dashboard_assistant_scope_cache.json";
const DASHBOARD_ASSISTANT_SCOPE_SHARED_CACHE_VERSION: &str = "dashboard-assistant-scope-cache-v1";
const DASHBOARD_ASSISTANT_SCOPE_SOURCE_SHARED_CACHE_RELATIVE_PATH: &str =
    "state/token_budget/dashboard_assistant_scope_source_cache.json";
const DASHBOARD_ASSISTANT_SCOPE_SOURCE_SHARED_CACHE_VERSION: &str =
    "dashboard-assistant-scope-source-cache-v1";

static DASHBOARD_ASSISTANT_SCOPE_SOURCE_CACHE: OnceLock<
    Mutex<Option<DashboardAssistantScopeSourceCache>>,
> = OnceLock::new();
static DASHBOARD_ASSISTANT_SCOPE_CACHE: OnceLock<Mutex<Option<DashboardAssistantScopeCache>>> =
    OnceLock::new();

#[derive(Debug, Clone)]
struct DashboardAssistantScopeCache {
    repo_root: PathBuf,
    signature: String,
    scopes: Vec<AssistantGenerationScopeObservation>,
}

#[derive(Debug, Clone)]
struct DashboardAssistantScopeSourceCache {
    repo_root: PathBuf,
    signature: String,
    target_context_pack_ids: BTreeSet<String>,
    direct_turns: Vec<AssistantGenerationTurnObservedSnapshot>,
    metadata: BTreeMap<String, WorkingStateContextPackMeta>,
    helper_only_context_pack_ids_by_thread: BTreeMap<String, BTreeSet<String>>,
    turns_by_thread:
        BTreeMap<String, Vec<codex_threads::RolloutAssistantGenerationTurnObservation>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDashboardAssistantScopeCache {
    cache_version: String,
    repo_root: String,
    signature: String,
    scopes: Vec<AssistantGenerationScopeObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDashboardAssistantScopeSourceCache {
    cache_version: String,
    repo_root: String,
    signature: String,
    target_context_pack_ids: BTreeSet<String>,
    direct_turns: Vec<AssistantGenerationTurnObservedSnapshot>,
    metadata: BTreeMap<String, WorkingStateContextPackMeta>,
    helper_only_context_pack_ids_by_thread: BTreeMap<String, BTreeSet<String>>,
    turns_by_thread:
        BTreeMap<String, Vec<codex_threads::RolloutAssistantGenerationTurnObservation>>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct DashboardAssistantScopeDebug {
    pub(super) source_cache_status: String,
    pub(super) source_stage_ms: BTreeMap<String, u64>,
    pub(super) source_total_ms: u64,
    pub(super) scope_cache_status: String,
    pub(super) scope_stage_ms: BTreeMap<String, u64>,
    pub(super) scope_total_ms: u64,
}

async fn dashboard_assistant_scope_sources(
    db: &Client,
    repo_root: &Path,
    union_target_ids: &BTreeSet<String>,
) -> Result<(
    Vec<AssistantGenerationTurnObservedSnapshot>,
    BTreeMap<String, WorkingStateContextPackMeta>,
    BTreeMap<String, BTreeSet<String>>,
    BTreeMap<String, Vec<codex_threads::RolloutAssistantGenerationTurnObservation>>,
    DashboardAssistantScopeDebug,
)> {
    let source_started_at = Instant::now();
    let mut debug = DashboardAssistantScopeDebug {
        source_cache_status: "miss".to_string(),
        scope_cache_status: "unknown".to_string(),
        ..Default::default()
    };
    let stage_started_at = Instant::now();
    let working_state_summary =
        postgres::summarize_observability_snapshots_by_kinds(db, &["working_state_event"]).await?;
    record_dashboard_stage_ms(
        &mut debug.source_stage_ms,
        "working_state_summary",
        stage_started_at,
    );
    let stage_started_at = Instant::now();
    let direct_turn_summary = postgres::summarize_observability_snapshots_by_kinds(
        db,
        &[ASSISTANT_GENERATION_TURN_OBSERVED_SNAPSHOT_KIND],
    )
    .await?;
    record_dashboard_stage_ms(
        &mut debug.source_stage_ms,
        "direct_turn_summary",
        stage_started_at,
    );
    let stage_started_at = Instant::now();
    let token_budget_summary =
        postgres::summarize_observability_snapshots_by_kinds(db, &["token_budget_event"]).await?;
    record_dashboard_stage_ms(
        &mut debug.source_stage_ms,
        "token_budget_summary",
        stage_started_at,
    );
    let stage_started_at = Instant::now();
    if let Some((direct_turns, metadata, helper_only_context_pack_ids_by_thread, turns_by_thread)) =
        cached_dashboard_assistant_scope_sources(
            repo_root,
            union_target_ids,
            &working_state_summary,
            &direct_turn_summary,
            &token_budget_summary,
        )?
    {
        record_dashboard_stage_ms(
            &mut debug.source_stage_ms,
            "source_cache_lookup",
            stage_started_at,
        );
        debug.source_cache_status = "hit".to_string();
        debug.source_total_ms = source_started_at.elapsed().as_millis() as u64;
        return Ok((
            direct_turns,
            metadata,
            helper_only_context_pack_ids_by_thread,
            turns_by_thread,
            debug,
        ));
    }
    record_dashboard_stage_ms(
        &mut debug.source_stage_ms,
        "source_cache_lookup",
        stage_started_at,
    );

    let stage_started_at = Instant::now();
    let direct_turns =
        assistant_generation_turn_observed_snapshots_for_context_packs(db, union_target_ids)
            .await?;
    record_dashboard_stage_ms(
        &mut debug.source_stage_ms,
        "direct_turn_snapshots",
        stage_started_at,
    );
    let direct_turn_covered_context_pack_ids = direct_turns
        .iter()
        .flat_map(|turn| turn.context_pack_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    if direct_turn_covered_context_pack_ids == *union_target_ids {
        debug
            .source_stage_ms
            .insert("direct_turn_full_coverage_short_circuit".to_string(), 0);
        let empty_metadata = BTreeMap::new();
        let empty_turns_by_thread = BTreeMap::new();
        let thread_signatures = BTreeMap::new();
        let signature = dashboard_assistant_scope_source_signature(
            union_target_ids,
            &working_state_summary,
            &direct_turn_summary,
            &token_budget_summary,
            &thread_signatures,
        );
        store_dashboard_assistant_scope_sources(
            repo_root,
            &signature,
            union_target_ids,
            &direct_turns,
            &empty_metadata,
            &BTreeMap::new(),
            &empty_turns_by_thread,
        );
        debug.source_total_ms = source_started_at.elapsed().as_millis() as u64;
        return Ok((
            direct_turns,
            empty_metadata,
            BTreeMap::new(),
            empty_turns_by_thread,
            debug,
        ));
    }
    let stage_started_at = Instant::now();
    let token_budget_rows =
        latest_token_budget_snapshots_for_context_packs(db, union_target_ids).await?;
    let metadata = merged_context_pack_rollout_metadata(
        &latest_working_state_context_pack_metadata(db, union_target_ids).await?,
        &token_budget_rows,
        union_target_ids,
    );
    record_dashboard_stage_ms(
        &mut debug.source_stage_ms,
        "working_state_plus_token_budget_lineage",
        stage_started_at,
    );
    let thread_ids = metadata
        .values()
        .map(|item| item.thread_id.clone())
        .collect::<BTreeSet<_>>();
    let mut turns_by_thread =
        BTreeMap::<String, Vec<codex_threads::RolloutAssistantGenerationTurnObservation>>::new();
    let mut helper_only_context_pack_ids_by_thread = BTreeMap::<String, BTreeSet<String>>::new();
    let stage_started_at = Instant::now();
    for thread_id in &thread_ids {
        let turns =
            codex_threads::rollout_assistant_generation_turn_observations_for_thread(thread_id)?;
        if !turns.is_empty() {
            turns_by_thread.insert(thread_id.clone(), turns);
        }
        let helper_only_context_pack_ids =
            codex_threads::rollout_helper_only_context_pack_ids_for_thread(thread_id)?;
        if !helper_only_context_pack_ids.is_empty() {
            helper_only_context_pack_ids_by_thread
                .insert(thread_id.clone(), helper_only_context_pack_ids);
        }
    }
    record_dashboard_stage_ms(
        &mut debug.source_stage_ms,
        "rollout_turn_threads",
        stage_started_at,
    );
    let stage_started_at = Instant::now();
    let thread_signatures = dashboard_assistant_scope_thread_signatures(&thread_ids)?;
    let signature = dashboard_assistant_scope_source_signature(
        union_target_ids,
        &working_state_summary,
        &direct_turn_summary,
        &token_budget_summary,
        &thread_signatures,
    );
    record_dashboard_stage_ms(
        &mut debug.source_stage_ms,
        "source_signature",
        stage_started_at,
    );
    store_dashboard_assistant_scope_sources(
        repo_root,
        &signature,
        union_target_ids,
        &direct_turns,
        &metadata,
        &helper_only_context_pack_ids_by_thread,
        &turns_by_thread,
    );
    debug.source_total_ms = source_started_at.elapsed().as_millis() as u64;
    Ok((
        direct_turns,
        metadata,
        helper_only_context_pack_ids_by_thread,
        turns_by_thread,
        debug,
    ))
}

pub(super) async fn derive_dashboard_rollout_assistant_generation_scopes(
    db: &Client,
    repo_root: &Path,
    events_by_scope: &[&[TokenBudgetEvent]],
) -> Result<(
    Vec<AssistantGenerationScopeObservation>,
    DashboardAssistantScopeDebug,
)> {
    let scope_started_at = Instant::now();
    let mut debug = DashboardAssistantScopeDebug {
        source_cache_status: "miss".to_string(),
        scope_cache_status: "miss".to_string(),
        ..Default::default()
    };
    let target_sets = events_by_scope
        .iter()
        .map(|events| assistant_generation_missing_scope_context_pack_ids(Some(events)))
        .collect::<Vec<_>>();
    let union_target_ids = target_sets
        .iter()
        .flat_map(|set| set.iter().cloned())
        .collect::<BTreeSet<_>>();
    if union_target_ids.is_empty() {
        debug.source_cache_status = "not_applicable".to_string();
        debug.scope_cache_status = "not_applicable".to_string();
        debug.scope_total_ms = scope_started_at.elapsed().as_millis() as u64;
        return Ok((
            vec![AssistantGenerationScopeObservation::default(); events_by_scope.len()],
            debug,
        ));
    }

    let stage_started_at = Instant::now();
    let (
        direct_turns,
        metadata,
        helper_only_context_pack_ids_by_thread,
        turns_by_thread,
        source_debug,
    ) = dashboard_assistant_scope_sources(db, repo_root, &union_target_ids).await?;
    debug.source_cache_status = source_debug.source_cache_status;
    debug.source_stage_ms = source_debug.source_stage_ms;
    debug.source_total_ms = source_debug.source_total_ms;
    record_dashboard_stage_ms(&mut debug.scope_stage_ms, "source_bundle", stage_started_at);
    let stage_started_at = Instant::now();
    let signature = dashboard_assistant_scope_signature(
        &target_sets,
        &direct_turns,
        &metadata,
        &helper_only_context_pack_ids_by_thread,
        &turns_by_thread,
    );
    record_dashboard_stage_ms(
        &mut debug.scope_stage_ms,
        "scope_signature",
        stage_started_at,
    );
    if let Some(scopes) = cached_dashboard_assistant_generation_scopes(repo_root, &signature) {
        debug.scope_cache_status = "hit".to_string();
        debug.scope_total_ms = scope_started_at.elapsed().as_millis() as u64;
        return Ok((scopes, debug));
    }
    let stage_started_at = Instant::now();
    let scopes = target_sets
        .iter()
        .zip(events_by_scope.iter())
        .map(|(target_context_pack_ids, scope_events)| {
            derive_rollout_assistant_generation_scope_from_sources(
                target_context_pack_ids,
                &direct_turns,
                &metadata,
                &turns_by_thread,
                &helper_only_context_pack_ids_for_scope(
                    &metadata,
                    &helper_only_context_pack_ids_by_thread,
                ),
                &helper_only_non_model_visible_context_pack_ids(
                    scope_events,
                    &metadata,
                    &helper_only_context_pack_ids_by_thread,
                ),
            )
        })
        .collect::<Vec<_>>();
    record_dashboard_stage_ms(&mut debug.scope_stage_ms, "scope_build", stage_started_at);
    store_dashboard_assistant_generation_scopes(repo_root, &signature, &scopes);
    debug.scope_total_ms = scope_started_at.elapsed().as_millis() as u64;
    Ok((scopes, debug))
}

fn dashboard_assistant_scope_signature(
    target_sets: &[BTreeSet<String>],
    direct_turns: &[AssistantGenerationTurnObservedSnapshot],
    metadata: &BTreeMap<String, WorkingStateContextPackMeta>,
    helper_only_context_pack_ids_by_thread: &BTreeMap<String, BTreeSet<String>>,
    turns_by_thread: &BTreeMap<
        String,
        Vec<codex_threads::RolloutAssistantGenerationTurnObservation>,
    >,
) -> String {
    let target_sets = target_sets
        .iter()
        .map(|set| set.iter().cloned().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let direct_turns = direct_turns
        .iter()
        .map(|item| {
            json!({
                "thread_id": item.thread_id,
                "turn_id": item.turn_id,
                "assistant_generation_tokens": item.assistant_generation_tokens,
                "context_pack_ids": item.context_pack_ids,
            })
        })
        .collect::<Vec<_>>();
    let metadata = metadata
        .iter()
        .map(|(context_pack_id, item)| {
            json!({
                "context_pack_id": context_pack_id,
                "thread_id": item.thread_id,
                "captured_at_epoch_ms": item.captured_at_epoch_ms,
                "turn_id": item.turn_id,
            })
        })
        .collect::<Vec<_>>();
    let helper_only_context_pack_ids_by_thread = helper_only_context_pack_ids_by_thread
        .iter()
        .map(|(thread_id, context_pack_ids)| {
            json!({
                "thread_id": thread_id,
                "context_pack_ids": context_pack_ids,
            })
        })
        .collect::<Vec<_>>();
    let turns_by_thread = turns_by_thread
        .iter()
        .map(|(thread_id, turns)| {
            let turns = turns
                .iter()
                .map(|turn| {
                    json!({
                        "thread_id": turn.thread_id,
                        "turn_id": turn.turn_id,
                        "started_at_epoch_ms": turn.started_at_epoch_ms,
                        "ended_at_epoch_ms": turn.ended_at_epoch_ms,
                        "assistant_generation_tokens": turn.assistant_generation_tokens,
                        "token_count_events": turn.token_count_events,
                        "approved_context_pack_calls": turn.approved_context_pack_calls,
                    })
                })
                .collect::<Vec<_>>();
            json!({
                "thread_id": thread_id,
                "turns": turns,
            })
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "target_sets": target_sets,
        "direct_turns": direct_turns,
        "metadata": metadata,
        "helper_only_context_pack_ids_by_thread": helper_only_context_pack_ids_by_thread,
        "turns_by_thread": turns_by_thread,
    });
    hex_sha256(&serde_json::to_vec(&payload).unwrap_or_else(|_| payload.to_string().into_bytes()))
}

pub(super) fn dashboard_assistant_scope_source_signature(
    target_context_pack_ids: &BTreeSet<String>,
    working_state_summary: &[postgres::ObservabilitySnapshotKindSummary],
    direct_turn_summary: &[postgres::ObservabilitySnapshotKindSummary],
    token_budget_summary: &[postgres::ObservabilitySnapshotKindSummary],
    rollout_thread_signatures: &BTreeMap<String, String>,
) -> String {
    let summary_json = |items: &[postgres::ObservabilitySnapshotKindSummary]| {
        items
            .iter()
            .map(|item| {
                json!({
                    "snapshot_kind": item.snapshot_kind,
                    "snapshots_count": item.snapshots_count,
                    "latest_created_at_epoch_ms": item.latest_created_at_epoch_ms,
                })
            })
            .collect::<Vec<_>>()
    };
    let payload = json!({
        "target_context_pack_ids": target_context_pack_ids.iter().cloned().collect::<Vec<_>>(),
        "working_state_summary": summary_json(working_state_summary),
        "direct_turn_summary": summary_json(direct_turn_summary),
        "token_budget_summary": summary_json(token_budget_summary),
        "rollout_thread_signatures": rollout_thread_signatures,
    });
    hex_sha256(&serde_json::to_vec(&payload).unwrap_or_else(|_| payload.to_string().into_bytes()))
}

fn dashboard_assistant_scope_thread_signatures(
    thread_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, String>> {
    let mut signatures = BTreeMap::new();
    for thread_id in thread_ids {
        let signature =
            codex_threads::rollout_assistant_generation_turn_source_signature_for_thread(
                thread_id,
            )?
            .unwrap_or_else(|| "no_rollout_source".to_string());
        signatures.insert(thread_id.clone(), signature);
    }
    Ok(signatures)
}

pub(super) fn dashboard_assistant_scope_shared_cache_path(repo_root: &Path) -> PathBuf {
    canonical_repo_root(repo_root).join(DASHBOARD_ASSISTANT_SCOPE_SHARED_CACHE_RELATIVE_PATH)
}

fn dashboard_assistant_scope_source_shared_cache_path(repo_root: &Path) -> PathBuf {
    canonical_repo_root(repo_root).join(DASHBOARD_ASSISTANT_SCOPE_SOURCE_SHARED_CACHE_RELATIVE_PATH)
}

fn load_shared_dashboard_assistant_scope_sources(
    repo_root: &Path,
) -> Option<DashboardAssistantScopeSourceCache> {
    let path = dashboard_assistant_scope_source_shared_cache_path(repo_root);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedDashboardAssistantScopeSourceCache =
        serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != DASHBOARD_ASSISTANT_SCOPE_SOURCE_SHARED_CACHE_VERSION {
        return None;
    }
    if canonical_repo_root(Path::new(&persisted.repo_root)) != canonical_repo_root(repo_root) {
        return None;
    }
    Some(DashboardAssistantScopeSourceCache {
        repo_root: canonical_repo_root(repo_root),
        signature: persisted.signature,
        target_context_pack_ids: persisted.target_context_pack_ids,
        direct_turns: persisted.direct_turns,
        metadata: persisted.metadata,
        helper_only_context_pack_ids_by_thread: persisted.helper_only_context_pack_ids_by_thread,
        turns_by_thread: persisted.turns_by_thread,
    })
}

fn write_shared_dashboard_assistant_scope_sources(
    repo_root: &Path,
    entry: &DashboardAssistantScopeSourceCache,
) -> Result<()> {
    let path = dashboard_assistant_scope_source_shared_cache_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedDashboardAssistantScopeSourceCache {
        cache_version: DASHBOARD_ASSISTANT_SCOPE_SOURCE_SHARED_CACHE_VERSION.to_string(),
        repo_root: canonical_repo_root(repo_root).display().to_string(),
        signature: entry.signature.clone(),
        target_context_pack_ids: entry.target_context_pack_ids.clone(),
        direct_turns: entry.direct_turns.clone(),
        metadata: entry.metadata.clone(),
        helper_only_context_pack_ids_by_thread: entry
            .helper_only_context_pack_ids_by_thread
            .clone(),
        turns_by_thread: entry.turns_by_thread.clone(),
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn load_shared_dashboard_assistant_generation_scopes(
    repo_root: &Path,
) -> Option<DashboardAssistantScopeCache> {
    let path = dashboard_assistant_scope_shared_cache_path(repo_root);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedDashboardAssistantScopeCache = serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != DASHBOARD_ASSISTANT_SCOPE_SHARED_CACHE_VERSION {
        return None;
    }
    if canonical_repo_root(Path::new(&persisted.repo_root)) != canonical_repo_root(repo_root) {
        return None;
    }
    Some(DashboardAssistantScopeCache {
        repo_root: canonical_repo_root(repo_root),
        signature: persisted.signature,
        scopes: persisted.scopes,
    })
}

fn write_shared_dashboard_assistant_generation_scopes(
    repo_root: &Path,
    entry: &DashboardAssistantScopeCache,
) -> Result<()> {
    let path = dashboard_assistant_scope_shared_cache_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedDashboardAssistantScopeCache {
        cache_version: DASHBOARD_ASSISTANT_SCOPE_SHARED_CACHE_VERSION.to_string(),
        repo_root: canonical_repo_root(repo_root).display().to_string(),
        signature: entry.signature.clone(),
        scopes: entry.scopes.clone(),
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn cached_dashboard_assistant_scope_sources(
    repo_root: &Path,
    target_context_pack_ids: &BTreeSet<String>,
    working_state_summary: &[postgres::ObservabilitySnapshotKindSummary],
    direct_turn_summary: &[postgres::ObservabilitySnapshotKindSummary],
    token_budget_summary: &[postgres::ObservabilitySnapshotKindSummary],
) -> Result<
    Option<(
        Vec<AssistantGenerationTurnObservedSnapshot>,
        BTreeMap<String, WorkingStateContextPackMeta>,
        BTreeMap<String, BTreeSet<String>>,
        BTreeMap<String, Vec<codex_threads::RolloutAssistantGenerationTurnObservation>>,
    )>,
> {
    let cache = DASHBOARD_ASSISTANT_SCOPE_SOURCE_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow!("dashboard assistant scope source cache poisoned"))?;
    let repo_root = canonical_repo_root(repo_root);
    let candidate = guard
        .as_ref()
        .filter(|entry| repo_root == entry.repo_root)
        .cloned()
        .or_else(|| load_shared_dashboard_assistant_scope_sources(&repo_root));
    let Some(entry) = candidate else {
        return Ok(None);
    };
    if target_context_pack_ids != &entry.target_context_pack_ids {
        return Ok(None);
    }
    let thread_ids = entry
        .metadata
        .values()
        .map(|item| item.thread_id.clone())
        .collect::<BTreeSet<_>>();
    let thread_signatures = dashboard_assistant_scope_thread_signatures(&thread_ids)?;
    let signature = dashboard_assistant_scope_source_signature(
        target_context_pack_ids,
        working_state_summary,
        direct_turn_summary,
        token_budget_summary,
        &thread_signatures,
    );
    if entry.signature != signature {
        return Ok(None);
    }
    *guard = Some(entry.clone());
    Ok(Some((
        entry.direct_turns,
        entry.metadata,
        entry.helper_only_context_pack_ids_by_thread,
        entry.turns_by_thread,
    )))
}

fn store_dashboard_assistant_scope_sources(
    repo_root: &Path,
    signature: &str,
    target_context_pack_ids: &BTreeSet<String>,
    direct_turns: &[AssistantGenerationTurnObservedSnapshot],
    metadata: &BTreeMap<String, WorkingStateContextPackMeta>,
    helper_only_context_pack_ids_by_thread: &BTreeMap<String, BTreeSet<String>>,
    turns_by_thread: &BTreeMap<
        String,
        Vec<codex_threads::RolloutAssistantGenerationTurnObservation>,
    >,
) {
    let cache = DASHBOARD_ASSISTANT_SCOPE_SOURCE_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    let entry = DashboardAssistantScopeSourceCache {
        repo_root: canonical_repo_root(repo_root),
        signature: signature.to_string(),
        target_context_pack_ids: target_context_pack_ids.clone(),
        direct_turns: direct_turns.to_vec(),
        metadata: metadata.clone(),
        helper_only_context_pack_ids_by_thread: helper_only_context_pack_ids_by_thread.clone(),
        turns_by_thread: turns_by_thread.clone(),
    };
    let _ = write_shared_dashboard_assistant_scope_sources(repo_root, &entry);
    *guard = Some(entry);
}

fn cached_dashboard_assistant_generation_scopes(
    repo_root: &Path,
    signature: &str,
) -> Option<Vec<AssistantGenerationScopeObservation>> {
    let cache = DASHBOARD_ASSISTANT_SCOPE_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().ok()?;
    let repo_root = canonical_repo_root(repo_root);
    let entry = guard
        .as_ref()
        .filter(|entry| repo_root == entry.repo_root)
        .cloned()
        .or_else(|| load_shared_dashboard_assistant_generation_scopes(&repo_root))?;
    if entry.signature != signature {
        return None;
    }
    *guard = Some(entry.clone());
    Some(entry.scopes)
}

fn store_dashboard_assistant_generation_scopes(
    repo_root: &Path,
    signature: &str,
    scopes: &[AssistantGenerationScopeObservation],
) {
    let cache = DASHBOARD_ASSISTANT_SCOPE_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    let entry = DashboardAssistantScopeCache {
        repo_root: canonical_repo_root(repo_root),
        signature: signature.to_string(),
        scopes: scopes.to_vec(),
    };
    let _ = write_shared_dashboard_assistant_generation_scopes(repo_root, &entry);
    *guard = Some(entry);
}

pub(super) fn dashboard_assistant_scope_debug_value(debug: &DashboardAssistantScopeDebug) -> Value {
    json!({
        "source_cache_status": debug.source_cache_status,
        "source_stage_ms": debug.source_stage_ms,
        "source_total_ms": debug.source_total_ms,
        "scope_cache_status": debug.scope_cache_status,
        "scope_stage_ms": debug.scope_stage_ms,
        "scope_total_ms": debug.scope_total_ms,
    })
}
