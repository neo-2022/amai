pub const SHARED_RUNTIME_MARKER: &str = "shared_runtime_marker";
pub const ALPHA_ONLY_TOKEN: &str = "alpha_only_token";

pub fn alpha_runtime_summary() -> String {
    format!(
        "alpha runtime summary: {} {}",
        SHARED_RUNTIME_MARKER, ALPHA_ONLY_TOKEN
    )
}
