pub const SHARED_RUNTIME_MARKER: &str = "shared_runtime_marker";
pub const BETA_ONLY_TOKEN: &str = "beta_only_token";

pub fn beta_runtime_summary() -> String {
    format!(
        "beta runtime summary: {} {}",
        SHARED_RUNTIME_MARKER, BETA_ONLY_TOKEN
    )
}
