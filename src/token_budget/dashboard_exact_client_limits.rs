use super::*;

#[derive(Debug, Clone)]
pub(super) struct DashboardExactClientLimitsCache {
    pub(super) fetched_at_epoch_ms: u64,
    pub(super) observation: Option<CodexAppServerRateLimitsObservation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DashboardExactClientLimitsResolutionSource {
    LiveAppServer,
    InProcessCache,
    SharedFileCache,
    Missing,
}

impl DashboardExactClientLimitsResolutionSource {
    pub(super) fn should_persist_exact_sample(self) -> bool {
        matches!(self, Self::LiveAppServer)
    }
}

#[derive(Debug, Clone)]
pub(super) struct DashboardExactClientLimitsResolution {
    pub(super) observation: Option<CodexAppServerRateLimitsObservation>,
    pub(super) source: DashboardExactClientLimitsResolutionSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedDashboardExactClientLimitsCache {
    cache_version: String,
    fetched_at_epoch_ms: u64,
    observation: Option<CodexAppServerRateLimitsObservation>,
}

pub(super) fn cached_dashboard_exact_client_limits_observation()
-> Option<CodexAppServerRateLimitsObservation> {
    let cache = DASHBOARD_EXACT_CLIENT_LIMITS_CACHE.get_or_init(|| Mutex::new(None));
    cache
        .lock()
        .ok()
        .and_then(|guard| guard.as_ref().and_then(|entry| entry.observation.clone()))
}

pub(super) fn fresh_dashboard_exact_client_limits_cache_entry(
    now_epoch_ms: u64,
) -> Option<DashboardExactClientLimitsCache> {
    let cache = DASHBOARD_EXACT_CLIENT_LIMITS_CACHE.get_or_init(|| Mutex::new(None));
    cache.lock().ok().and_then(|guard| {
        guard.as_ref().and_then(|entry| {
            (now_epoch_ms.saturating_sub(entry.fetched_at_epoch_ms)
                <= DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS)
                .then(|| entry.clone())
        })
    })
}

pub(super) fn set_dashboard_exact_client_limits_cache(entry: DashboardExactClientLimitsCache) {
    let cache = DASHBOARD_EXACT_CLIENT_LIMITS_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = cache.lock() {
        *guard = Some(entry);
    }
}

fn dashboard_exact_client_limits_shared_cache_path(repo_root: &Path) -> PathBuf {
    repo_root.join(DASHBOARD_EXACT_CLIENT_LIMITS_SHARED_CACHE_RELATIVE_PATH)
}

fn discovered_dashboard_exact_client_limits_shared_cache_path() -> Option<PathBuf> {
    config::discover_repo_root(None)
        .ok()
        .map(|repo_root| dashboard_exact_client_limits_shared_cache_path(&repo_root))
}

pub(super) fn load_shared_dashboard_exact_client_limits_cache(
    path: &Path,
    now_epoch_ms: u64,
) -> Option<DashboardExactClientLimitsCache> {
    let payload = fs::read_to_string(path).ok()?;
    let persisted: PersistedDashboardExactClientLimitsCache =
        serde_json::from_str(&payload).ok()?;
    if persisted.cache_version != DASHBOARD_EXACT_CLIENT_LIMITS_SHARED_CACHE_VERSION {
        return None;
    }
    if now_epoch_ms.saturating_sub(persisted.fetched_at_epoch_ms)
        > DASHBOARD_EXACT_CLIENT_LIMITS_SOURCE_TTL_MS
    {
        return None;
    }
    Some(DashboardExactClientLimitsCache {
        fetched_at_epoch_ms: persisted.fetched_at_epoch_ms,
        observation: persisted.observation,
    })
}

pub(super) fn write_shared_dashboard_exact_client_limits_cache(
    path: &Path,
    entry: &DashboardExactClientLimitsCache,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let payload = PersistedDashboardExactClientLimitsCache {
        cache_version: DASHBOARD_EXACT_CLIENT_LIMITS_SHARED_CACHE_VERSION.to_string(),
        fetched_at_epoch_ms: entry.fetched_at_epoch_ms,
        observation: entry.observation.clone(),
    };
    fs::write(path, serde_json::to_vec(&payload)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
pub(super) fn best_effort_exact_client_limit_observation_from_result(
    result: Result<Option<CodexAppServerRateLimitsObservation>>,
    cached: Option<CodexAppServerRateLimitsObservation>,
) -> Option<CodexAppServerRateLimitsObservation> {
    match result {
        Ok(Some(observation)) => Some(observation),
        Ok(None) => cached,
        Err(_) => cached,
    }
}

pub(super) async fn dashboard_exact_client_rate_limits_resolution()
-> Result<DashboardExactClientLimitsResolution> {
    let now_epoch_ms = current_epoch_ms().unwrap_or_default() as u64;
    if let Some(entry) = fresh_dashboard_exact_client_limits_cache_entry(now_epoch_ms) {
        return Ok(DashboardExactClientLimitsResolution {
            source: if entry.observation.is_some() {
                DashboardExactClientLimitsResolutionSource::InProcessCache
            } else {
                DashboardExactClientLimitsResolutionSource::Missing
            },
            observation: entry.observation,
        });
    }
    if let Some(path) = discovered_dashboard_exact_client_limits_shared_cache_path() {
        if let Some(entry) = load_shared_dashboard_exact_client_limits_cache(&path, now_epoch_ms) {
            set_dashboard_exact_client_limits_cache(entry.clone());
            return Ok(DashboardExactClientLimitsResolution {
                source: if entry.observation.is_some() {
                    DashboardExactClientLimitsResolutionSource::SharedFileCache
                } else {
                    DashboardExactClientLimitsResolutionSource::Missing
                },
                observation: entry.observation,
            });
        }
    }
    let stale_cached_observation = cached_dashboard_exact_client_limits_observation();
    let live_query_result = if let Some(executable) = discover_local_codex_app_server_executable() {
        query_codex_app_server_rate_limits(&executable).await
    } else {
        Ok(None)
    };
    let (observation, source) = match live_query_result {
        Ok(observation) => {
            let cache_entry = DashboardExactClientLimitsCache {
                fetched_at_epoch_ms: now_epoch_ms,
                observation: observation.clone(),
            };
            set_dashboard_exact_client_limits_cache(cache_entry.clone());
            if let Some(path) = discovered_dashboard_exact_client_limits_shared_cache_path() {
                let _ = write_shared_dashboard_exact_client_limits_cache(&path, &cache_entry);
            }
            let source = if observation.is_some() {
                DashboardExactClientLimitsResolutionSource::LiveAppServer
            } else {
                DashboardExactClientLimitsResolutionSource::Missing
            };
            (observation, source)
        }
        Err(_) => {
            let source = if stale_cached_observation.is_some() {
                DashboardExactClientLimitsResolutionSource::InProcessCache
            } else {
                DashboardExactClientLimitsResolutionSource::Missing
            };
            (stale_cached_observation, source)
        }
    };
    Ok(DashboardExactClientLimitsResolution {
        observation,
        source,
    })
}
