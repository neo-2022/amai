use std::env;
use std::fmt;

pub const PLATFORM_THREAD_ID_ENV: &str = "AMAI_PLATFORM_THREAD_ID";
pub const LEGACY_CODEX_THREAD_ID_ENV: &str = "CODEX_THREAD_ID";
pub const HERMES_SESSION_ID_ENV: &str = "HERMES_SESSION_ID";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadBindingConflict {
    pub platform_thread_id: String,
    pub legacy_thread_id: String,
}

impl fmt::Display for ThreadBindingConflict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "conflicting thread identity aliases: {PLATFORM_THREAD_ID_ENV}='{}' and {LEGACY_CODEX_THREAD_ID_ENV}='{}' differ",
            self.platform_thread_id, self.legacy_thread_id
        )
    }
}

impl std::error::Error for ThreadBindingConflict {}

fn normalized_thread_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
pub fn resolve_thread_id_candidates(
    platform_thread_id: Option<&str>,
    legacy_thread_id: Option<&str>,
) -> Option<String> {
    resolve_thread_id_candidates_result(platform_thread_id, legacy_thread_id)
        .ok()
        .flatten()
}

pub fn resolve_thread_id_candidates_result(
    platform_thread_id: Option<&str>,
    legacy_thread_id: Option<&str>,
) -> Result<Option<String>, ThreadBindingConflict> {
    let platform_thread_id = normalized_thread_id(platform_thread_id);
    let legacy_thread_id = normalized_thread_id(legacy_thread_id);
    match (platform_thread_id, legacy_thread_id) {
        (Some(platform_thread_id), Some(legacy_thread_id))
            if platform_thread_id != legacy_thread_id =>
        {
            Err(ThreadBindingConflict {
                platform_thread_id,
                legacy_thread_id,
            })
        }
        (Some(platform_thread_id), _) => Ok(Some(platform_thread_id)),
        (None, Some(legacy_thread_id)) => Ok(Some(legacy_thread_id)),
        (None, None) => Ok(None),
    }
}

pub fn current_thread_id_result() -> Result<Option<String>, ThreadBindingConflict> {
    let platform_thread_id = env::var(PLATFORM_THREAD_ID_ENV).ok();
    let legacy_thread_id = env::var(LEGACY_CODEX_THREAD_ID_ENV).ok();
    let hermes_session_id = normalized_thread_id(env::var(HERMES_SESSION_ID_ENV).ok().as_deref());
    let explicit_thread_id = resolve_thread_id_candidates_result(
        platform_thread_id.as_deref(),
        legacy_thread_id.as_deref(),
    )?;
    Ok(explicit_thread_id.or(hermes_session_id))
}

pub fn current_thread_id() -> Option<String> {
    current_thread_id_result().ok().flatten()
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, OnceLock};

    use super::{
        HERMES_SESSION_ID_ENV, LEGACY_CODEX_THREAD_ID_ENV, PLATFORM_THREAD_ID_ENV,
        current_thread_id, current_thread_id_result, resolve_thread_id_candidates,
        resolve_thread_id_candidates_result,
    };

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("thread-binding env lock")
    }

    struct ScopedEnvVar {
        key: &'static str,
        previous: Option<String>,
    }

    impl ScopedEnvVar {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }

        fn unset(key: &'static str) -> Self {
            let previous = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
            }
            Self { key, previous }
        }
    }

    impl Drop for ScopedEnvVar {
        fn drop(&mut self) {
            match self.previous.as_deref() {
                Some(value) => unsafe {
                    std::env::set_var(self.key, value);
                },
                None => unsafe {
                    std::env::remove_var(self.key);
                },
            }
        }
    }

    #[test]
    fn resolve_thread_id_candidates_accepts_matching_aliases() {
        assert_eq!(
            resolve_thread_id_candidates(Some("same-thread"), Some("same-thread")).as_deref(),
            Some("same-thread")
        );
    }

    #[test]
    fn resolve_thread_id_candidates_fails_closed_on_alias_disagreement() {
        assert_eq!(
            resolve_thread_id_candidates(Some("platform-thread"), Some("legacy-thread")),
            None
        );
        let conflict =
            resolve_thread_id_candidates_result(Some("platform-thread"), Some("legacy-thread"))
                .expect_err("alias conflict");
        assert_eq!(conflict.platform_thread_id, "platform-thread");
        assert_eq!(conflict.legacy_thread_id, "legacy-thread");
    }

    #[test]
    fn resolve_thread_id_candidates_preserves_legacy_compatibility() {
        assert_eq!(
            resolve_thread_id_candidates(None, Some("legacy-thread")).as_deref(),
            Some("legacy-thread")
        );
    }

    #[test]
    fn resolve_thread_id_candidates_ignores_blank_alias_values() {
        assert_eq!(
            resolve_thread_id_candidates(Some("   "), Some("legacy-thread")).as_deref(),
            Some("legacy-thread")
        );
        assert_eq!(resolve_thread_id_candidates(Some("   "), Some("  ")), None);
    }

    #[test]
    fn current_thread_id_accepts_matching_aliases() {
        let _guard = env_lock();
        let _platform = ScopedEnvVar::set(PLATFORM_THREAD_ID_ENV, "same-thread");
        let _legacy = ScopedEnvVar::set(LEGACY_CODEX_THREAD_ID_ENV, "same-thread");
        let _no_hermes = ScopedEnvVar::unset(HERMES_SESSION_ID_ENV);
        assert_eq!(current_thread_id().as_deref(), Some("same-thread"));
    }

    #[test]
    fn current_thread_id_fails_closed_on_alias_disagreement() {
        let _guard = env_lock();
        let _platform = ScopedEnvVar::set(PLATFORM_THREAD_ID_ENV, "platform-thread");
        let _legacy = ScopedEnvVar::set(LEGACY_CODEX_THREAD_ID_ENV, "legacy-thread");
        let _no_hermes = ScopedEnvVar::unset(HERMES_SESSION_ID_ENV);
        assert_eq!(current_thread_id(), None);
        assert!(current_thread_id_result().is_err());
    }

    #[test]
    fn current_thread_id_falls_back_to_legacy_alias() {
        let _guard = env_lock();
        let _no_platform = ScopedEnvVar::unset(PLATFORM_THREAD_ID_ENV);
        let _legacy = ScopedEnvVar::set(LEGACY_CODEX_THREAD_ID_ENV, "legacy-thread");
        let _hermes = ScopedEnvVar::set(HERMES_SESSION_ID_ENV, "foreign-hermes-session");
        assert_eq!(current_thread_id().as_deref(), Some("legacy-thread"));
    }

    #[test]
    fn current_thread_id_prefers_platform_alias_over_hermes_session() {
        let _guard = env_lock();
        let _platform = ScopedEnvVar::set(PLATFORM_THREAD_ID_ENV, "platform-thread");
        let _no_legacy = ScopedEnvVar::unset(LEGACY_CODEX_THREAD_ID_ENV);
        let _hermes = ScopedEnvVar::set(HERMES_SESSION_ID_ENV, "foreign-hermes-session");
        assert_eq!(current_thread_id().as_deref(), Some("platform-thread"));
    }

    #[test]
    fn current_thread_id_falls_back_to_hermes_session() {
        let _guard = env_lock();
        let _no_platform = ScopedEnvVar::unset(PLATFORM_THREAD_ID_ENV);
        let _no_legacy = ScopedEnvVar::unset(LEGACY_CODEX_THREAD_ID_ENV);
        let _hermes = ScopedEnvVar::set(HERMES_SESSION_ID_ENV, "hermes-session");
        assert_eq!(current_thread_id().as_deref(), Some("hermes-session"));
    }

    #[test]
    fn current_thread_id_returns_none_when_all_aliases_missing_or_blank() {
        let _guard = env_lock();
        let _no_platform = ScopedEnvVar::unset(PLATFORM_THREAD_ID_ENV);
        let _blank_legacy = ScopedEnvVar::set(LEGACY_CODEX_THREAD_ID_ENV, "   ");
        let _no_hermes = ScopedEnvVar::unset(HERMES_SESSION_ID_ENV);
        assert_eq!(current_thread_id(), None);
    }
}
