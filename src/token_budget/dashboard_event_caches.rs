use super::*;

#[derive(Debug, Clone)]
pub(super) struct DashboardTokenEventsCache {
    pub(super) repo_root: PathBuf,
    pub(super) signature: String,
    pub(super) snapshot_kinds: Vec<String>,
    pub(super) summary: Vec<postgres::ObservabilitySnapshotKindSummary>,
    pub(super) raw_events: Vec<TokenBudgetEvent>,
}

#[derive(Debug, Clone)]
pub(super) struct DashboardCurrentSessionEventsCache {
    pub(super) repo_root: PathBuf,
    pub(super) signature: String,
    pub(super) session_gap_ms: i64,
    pub(super) events: Vec<TokenBudgetEvent>,
}

#[derive(Debug, Clone)]
pub(super) struct DashboardLiveTurnRetrievalCache {
    pub(super) repo_root: PathBuf,
    pub(super) thread_id: String,
    pub(super) lower_bound: i64,
    pub(super) upper_bound: i64,
    pub(super) invalidation_epoch_ms: i64,
    pub(super) context_pack_ids: BTreeSet<String>,
    pub(super) retrieval_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDashboardTokenEventsCache {
    cache_version: String,
    repo_root: String,
    signature: String,
    snapshot_kinds: Vec<String>,
    raw_events: Vec<TokenBudgetEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDashboardCurrentSessionEventsCache {
    cache_version: String,
    repo_root: String,
    signature: String,
    session_gap_ms: i64,
    events: Vec<TokenBudgetEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDashboardTokenEventsInvalidationCache {
    cache_version: String,
    repo_root: String,
    latest_recorded_at_epoch_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDashboardLiveTurnRetrievalCache {
    cache_version: String,
    repo_root: String,
    thread_id: String,
    lower_bound: i64,
    upper_bound: i64,
    invalidation_epoch_ms: i64,
    context_pack_ids: BTreeSet<String>,
    retrieval_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDashboardLiveTurnRetrievalInvalidationCache {
    cache_version: String,
    repo_root: String,
    latest_recorded_at_epoch_ms: i64,
}

fn dashboard_token_events_shared_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(DASHBOARD_TOKEN_EVENTS_SHARED_CACHE_RELATIVE_PATH)
}

fn dashboard_token_events_invalidation_shared_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(DASHBOARD_TOKEN_EVENTS_INVALIDATION_SHARED_CACHE_RELATIVE_PATH)
}

fn dashboard_current_session_events_shared_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(DASHBOARD_CURRENT_SESSION_EVENTS_SHARED_CACHE_RELATIVE_PATH)
}

fn dashboard_live_turn_retrieval_shared_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(DASHBOARD_LIVE_TURN_RETRIEVAL_SHARED_CACHE_RELATIVE_PATH)
}

fn dashboard_live_turn_retrieval_invalidation_shared_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(DASHBOARD_LIVE_TURN_RETRIEVAL_INVALIDATION_SHARED_CACHE_RELATIVE_PATH)
}

pub(super) fn load_shared_dashboard_token_events(
    repo_root: &Path,
) -> Option<DashboardTokenEventsCache> {
    let path = dashboard_token_events_shared_cache_path(repo_root);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedDashboardTokenEventsCache = serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != DASHBOARD_TOKEN_EVENTS_SHARED_CACHE_VERSION {
        return None;
    }
    if canonical_repo_root(Path::new(&persisted.repo_root)) != canonical_repo_root(repo_root) {
        return None;
    }
    Some(DashboardTokenEventsCache {
        repo_root: canonical_repo_root(repo_root),
        signature: persisted.signature,
        snapshot_kinds: persisted.snapshot_kinds,
        summary: Vec::new(),
        raw_events: persisted.raw_events,
    })
}

pub(super) fn load_shared_dashboard_token_events_invalidation(repo_root: &Path) -> Option<i64> {
    let path = dashboard_token_events_invalidation_shared_cache_path(repo_root);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedDashboardTokenEventsInvalidationCache =
        serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != DASHBOARD_TOKEN_EVENTS_INVALIDATION_SHARED_CACHE_VERSION {
        return None;
    }
    if canonical_repo_root(Path::new(&persisted.repo_root)) != canonical_repo_root(repo_root) {
        return None;
    }
    Some(persisted.latest_recorded_at_epoch_ms)
}

pub(super) fn load_shared_dashboard_current_session_events(
    repo_root: &Path,
    signature: &str,
    session_gap_ms: i64,
) -> Option<Vec<TokenBudgetEvent>> {
    let path = dashboard_current_session_events_shared_cache_path(repo_root);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedDashboardCurrentSessionEventsCache =
        serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != DASHBOARD_CURRENT_SESSION_EVENTS_SHARED_CACHE_VERSION {
        return None;
    }
    if canonical_repo_root(Path::new(&persisted.repo_root)) != canonical_repo_root(repo_root) {
        return None;
    }
    (persisted.signature == signature && persisted.session_gap_ms == session_gap_ms)
        .then_some(persisted.events)
}

pub(super) fn load_shared_dashboard_live_turn_retrieval_cache(
    repo_root: &Path,
) -> Option<DashboardLiveTurnRetrievalCache> {
    let path = dashboard_live_turn_retrieval_shared_cache_path(repo_root);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedDashboardLiveTurnRetrievalCache =
        serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != DASHBOARD_LIVE_TURN_RETRIEVAL_SHARED_CACHE_VERSION {
        return None;
    }
    if canonical_repo_root(Path::new(&persisted.repo_root)) != canonical_repo_root(repo_root) {
        return None;
    }
    Some(DashboardLiveTurnRetrievalCache {
        repo_root: canonical_repo_root(repo_root),
        thread_id: persisted.thread_id,
        lower_bound: persisted.lower_bound,
        upper_bound: persisted.upper_bound,
        invalidation_epoch_ms: persisted.invalidation_epoch_ms,
        context_pack_ids: persisted.context_pack_ids,
        retrieval_count: persisted.retrieval_count,
    })
}

pub(super) fn write_shared_dashboard_live_turn_retrieval_cache(
    entry: &DashboardLiveTurnRetrievalCache,
) -> Result<()> {
    let path = dashboard_live_turn_retrieval_shared_cache_path(&entry.repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedDashboardLiveTurnRetrievalCache {
        cache_version: DASHBOARD_LIVE_TURN_RETRIEVAL_SHARED_CACHE_VERSION.to_string(),
        repo_root: entry.repo_root.display().to_string(),
        thread_id: entry.thread_id.clone(),
        lower_bound: entry.lower_bound,
        upper_bound: entry.upper_bound,
        invalidation_epoch_ms: entry.invalidation_epoch_ms,
        context_pack_ids: entry.context_pack_ids.clone(),
        retrieval_count: entry.retrieval_count,
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(super) fn load_shared_dashboard_live_turn_retrieval_invalidation(
    repo_root: &Path,
) -> Option<i64> {
    let path = dashboard_live_turn_retrieval_invalidation_shared_cache_path(repo_root);
    let bytes = fs::read(&path).ok()?;
    let persisted: PersistedDashboardLiveTurnRetrievalInvalidationCache =
        serde_json::from_slice(&bytes).ok()?;
    if persisted.cache_version != DASHBOARD_LIVE_TURN_RETRIEVAL_INVALIDATION_SHARED_CACHE_VERSION {
        return None;
    }
    if canonical_repo_root(Path::new(&persisted.repo_root)) != canonical_repo_root(repo_root) {
        return None;
    }
    Some(persisted.latest_recorded_at_epoch_ms)
}

pub(super) fn write_shared_dashboard_live_turn_retrieval_invalidation(
    repo_root: &Path,
    latest_recorded_at_epoch_ms: i64,
) -> Result<()> {
    let repo_root = canonical_repo_root(repo_root);
    let path = dashboard_live_turn_retrieval_invalidation_shared_cache_path(&repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedDashboardLiveTurnRetrievalInvalidationCache {
        cache_version: DASHBOARD_LIVE_TURN_RETRIEVAL_INVALIDATION_SHARED_CACHE_VERSION.to_string(),
        repo_root: repo_root.display().to_string(),
        latest_recorded_at_epoch_ms,
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(super) fn write_shared_dashboard_token_events(entry: &DashboardTokenEventsCache) -> Result<()> {
    let path = dashboard_token_events_shared_cache_path(&entry.repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedDashboardTokenEventsCache {
        cache_version: DASHBOARD_TOKEN_EVENTS_SHARED_CACHE_VERSION.to_string(),
        repo_root: entry.repo_root.display().to_string(),
        signature: entry.signature.clone(),
        snapshot_kinds: entry.snapshot_kinds.clone(),
        raw_events: entry.raw_events.clone(),
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(super) fn write_shared_dashboard_current_session_events(
    entry: &DashboardCurrentSessionEventsCache,
) -> Result<()> {
    let path = dashboard_current_session_events_shared_cache_path(&entry.repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedDashboardCurrentSessionEventsCache {
        cache_version: DASHBOARD_CURRENT_SESSION_EVENTS_SHARED_CACHE_VERSION.to_string(),
        repo_root: entry.repo_root.display().to_string(),
        signature: entry.signature.clone(),
        session_gap_ms: entry.session_gap_ms,
        events: entry.events.clone(),
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(super) fn write_shared_dashboard_token_events_invalidation(
    repo_root: &Path,
    latest_recorded_at_epoch_ms: i64,
) -> Result<()> {
    let repo_root = canonical_repo_root(repo_root);
    let path = dashboard_token_events_invalidation_shared_cache_path(&repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let persisted = PersistedDashboardTokenEventsInvalidationCache {
        cache_version: DASHBOARD_TOKEN_EVENTS_INVALIDATION_SHARED_CACHE_VERSION.to_string(),
        repo_root: repo_root.display().to_string(),
        latest_recorded_at_epoch_ms,
    };
    fs::write(&path, serde_json::to_vec(&persisted)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}
