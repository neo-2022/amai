use super::*;
use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, Deserialize)]
pub(super) struct InstallState {
    pub package_version: String,
    pub repo_revision: String,
    pub client_key: String,
    pub client_config: String,
    pub stack_profile: String,
    pub installed_at_epoch_seconds: u64,
}

pub fn browser_base_url(bind: &str) -> String {
    let Some((host, port)) = bind.rsplit_once(':') else {
        return format!("http://{bind}");
    };
    let normalized_host = match host {
        "0.0.0.0" => "127.0.0.1".to_string(),
        "::" | "[::]" => "[::1]".to_string(),
        _ => host.to_string(),
    };
    format!("http://{normalized_host}:{port}")
}

pub(super) fn load_install_state(repo_root: &Path) -> Result<Option<InstallState>> {
    let path = repo_root.join("state/install_state.json");
    if !path.is_file() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let state =
        serde_json::from_str(&content).context("failed to parse dashboard install state")?;
    Ok(Some(state))
}
