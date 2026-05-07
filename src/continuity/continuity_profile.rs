use std::env;

pub(super) fn continuity_profile_enabled() -> bool {
    env::var("AMAI_PROFILE_CONTINUITY")
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !normalized.is_empty() && normalized != "0" && normalized != "false"
        })
        .unwrap_or(false)
}

pub(super) fn continuity_profile_log(stage: &str, elapsed_ms: u128, extra: &str) {
    if continuity_profile_enabled() {
        eprintln!("[amai-continuity-profile] stage={stage} elapsed_ms={elapsed_ms} {extra}");
    }
}
