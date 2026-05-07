use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedContinuityRestoreObservedDedupeCache {
    pub(super) cache_version: String,
    pub(super) updated_at_epoch_ms: u64,
    pub(super) project_code: String,
    pub(super) namespace_code: String,
    pub(super) source_kind: String,
    pub(super) prompt_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersistedActiveThreadHintCache {
    pub(super) cache_version: String,
    pub(super) updated_at_epoch_ms: u64,
    pub(super) thread_id: String,
}

pub(super) fn active_thread_hint_shared_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(ACTIVE_THREAD_HINT_SHARED_CACHE_RELATIVE_PATH)
}

fn continuity_restore_observed_dedupe_shared_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(CONTINUITY_RESTORE_OBSERVED_DEDUPE_SHARED_CACHE_RELATIVE_PATH)
}

pub(super) fn continuity_restore_observed_event_recently_recorded(
    repo_root: &Path,
    project_code: &str,
    namespace_code: &str,
    source_kind: &str,
    prompt_hash: &str,
    now_epoch_ms: u64,
) -> bool {
    let path = continuity_restore_observed_dedupe_shared_cache_path(repo_root);
    let payload = match fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return false,
    };
    let persisted: PersistedContinuityRestoreObservedDedupeCache =
        match serde_json::from_str(&payload) {
            Ok(value) => value,
            Err(_) => return false,
        };
    persisted.cache_version == CONTINUITY_RESTORE_OBSERVED_DEDUPE_SHARED_CACHE_VERSION
        && persisted.project_code == project_code
        && persisted.namespace_code == namespace_code
        && persisted.source_kind == source_kind
        && persisted.prompt_hash == prompt_hash
        && now_epoch_ms.saturating_sub(persisted.updated_at_epoch_ms)
            <= CONTINUITY_RESTORE_OBSERVED_DEDUPE_TTL_MS
}

pub(super) fn write_continuity_restore_observed_dedupe_cache(
    repo_root: &Path,
    project_code: &str,
    namespace_code: &str,
    source_kind: &str,
    prompt_hash: &str,
    now_epoch_ms: u64,
) -> Result<()> {
    let path = continuity_restore_observed_dedupe_shared_cache_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let payload = PersistedContinuityRestoreObservedDedupeCache {
        cache_version: CONTINUITY_RESTORE_OBSERVED_DEDUPE_SHARED_CACHE_VERSION.to_string(),
        updated_at_epoch_ms: now_epoch_ms,
        project_code: project_code.to_string(),
        namespace_code: namespace_code.to_string(),
        source_kind: source_kind.to_string(),
        prompt_hash: prompt_hash.to_string(),
    };
    fs::write(&path, serde_json::to_vec(&payload)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub(super) fn load_shared_active_thread_hint(
    repo_root: &Path,
    now_epoch_ms: u64,
) -> Option<String> {
    let path = active_thread_hint_shared_cache_path(repo_root);
    let payload = fs::read_to_string(path).ok()?;
    let persisted: PersistedActiveThreadHintCache = serde_json::from_str(&payload).ok()?;
    if persisted.cache_version != ACTIVE_THREAD_HINT_SHARED_CACHE_VERSION {
        return None;
    }
    let thread_id = persisted.thread_id.trim();
    if thread_id.is_empty() {
        return None;
    }
    if now_epoch_ms.saturating_sub(persisted.updated_at_epoch_ms) > ACTIVE_THREAD_HINT_MAX_AGE_MS {
        return None;
    }
    Some(thread_id.to_string())
}
