use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::cmp::Reverse;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SUMMARY_RELATIVE_PATH: &str = "state/tooling/artifact_cleanup/latest.json";

#[derive(Debug, Clone, Deserialize)]
struct ArtifactCleanupDocument {
    artifact_cleanup: ArtifactCleanupProfile,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArtifactCleanupProfile {
    pub sweep_interval_seconds: u64,
    #[serde(default = "default_unmanaged_root_alert_bytes")]
    pub unmanaged_root_alert_bytes: u64,
    #[serde(default = "default_max_unmanaged_roots")]
    pub max_unmanaged_roots: usize,
    pub targets: Vec<ArtifactCleanupTarget>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArtifactCleanupTarget {
    pub path: String,
    pub description: String,
    pub ttl_hours: u64,
    pub keep_latest: usize,
    #[serde(default)]
    pub auto_apply: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CleanupMode {
    Conservative,
    Aggressive,
}

impl CleanupMode {
    fn as_str(self) -> &'static str {
        match self {
            CleanupMode::Conservative => "conservative",
            CleanupMode::Aggressive => "aggressive",
        }
    }

    fn enforce_ttl(self) -> bool {
        matches!(self, CleanupMode::Conservative)
    }
}

#[derive(Debug, Clone)]
struct CleanupEntry {
    path: PathBuf,
    modified: SystemTime,
    size_bytes: u64,
}

#[derive(Debug, Clone)]
struct PlannedCleanupEntry {
    path: PathBuf,
    age_hours: f64,
    size_bytes: u64,
}

#[derive(Debug, Clone)]
struct RootInventoryEntry {
    relative_path: String,
    total_bytes: u64,
    managed_cleanup_scope_bytes: u64,
    unmanaged_bytes: u64,
}

#[derive(Debug, Clone, Default)]
struct TargetCleanupPlan {
    scanned: u64,
    kept_latest: u64,
    protected: u64,
    expired: u64,
    selected: u64,
    selected_entries: Vec<PlannedCleanupEntry>,
}

fn default_unmanaged_root_alert_bytes() -> u64 {
    10 * 1024 * 1024 * 1024
}

fn default_max_unmanaged_roots() -> usize {
    3
}

pub fn load_profile() -> Result<ArtifactCleanupProfile> {
    let path = profile_path();
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read artifact cleanup profile {}", path.display()))?;
    let document: ArtifactCleanupDocument =
        toml::from_str(&content).context("failed to parse artifact cleanup profile")?;
    Ok(document.artifact_cleanup)
}

pub fn sweep_interval() -> Result<Duration> {
    Ok(Duration::from_secs(load_profile()?.sweep_interval_seconds))
}

pub fn run_cleanup(
    repo_root: &Path,
    apply: bool,
    auto_only: bool,
    limit: Option<usize>,
    aggressive: bool,
) -> Result<Value> {
    let profile = load_profile()?;
    let now = SystemTime::now();
    let captured_at_epoch_ms = now
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as u64;
    let protected_paths = current_protected_paths();
    let mode = if aggressive {
        CleanupMode::Aggressive
    } else {
        CleanupMode::Conservative
    };
    let mut remaining_limit = limit.unwrap_or(usize::MAX);
    let mut preview_limit = usize::MAX;

    let mut targets_json = Vec::new();
    let mut selected_json = Vec::new();
    let mut targets_scanned = 0_u64;
    let mut expired_total = 0_u64;
    let mut selected_total = 0_u64;
    let mut selected_reclaimable_bytes_total = 0_u64;
    let mut deleted_total = 0_u64;
    let mut reclaimed_bytes_total = 0_u64;
    let mut kept_latest_total = 0_u64;
    let mut protected_total = 0_u64;
    let mut aggressive_preview_total = 0_u64;
    let mut aggressive_preview_reclaimed_bytes = 0_u64;
    let mut managed_target_sizes = Vec::new();

    for target in profile
        .targets
        .iter()
        .filter(|target| !auto_only || target.auto_apply)
    {
        targets_scanned += 1;
        let root = repo_root.join(&target.path);
        if !root.exists() {
            targets_json.push(json!({
                "path": target.path,
                "description": target.description,
                "ttl_hours": target.ttl_hours,
                "keep_latest": target.keep_latest,
                "auto_apply": target.auto_apply,
                "missing": true,
                "entries_scanned": 0,
                "expired": 0,
                "selected": 0,
                "selected_reclaimable_bytes": 0,
                "deleted": 0,
                "reclaimed_bytes": 0,
                "kept_latest": 0,
                "protected": 0,
                "aggressive_preview_selected": 0,
                "aggressive_preview_reclaimable_bytes": 0,
            }));
            continue;
        }

        let entries = immediate_entries(&root)?;
        let target_total_bytes = entries.iter().map(|entry| entry.size_bytes).sum::<u64>();
        managed_target_sizes.push((root.clone(), target_total_bytes));
        let active_plan = plan_target_cleanup(
            entries.clone(),
            now,
            target.ttl_hours,
            target.keep_latest,
            &protected_paths,
            &mut remaining_limit,
            mode,
        )?;
        let aggressive_preview = if aggressive {
            active_plan.clone()
        } else {
            plan_target_cleanup(
                entries,
                now,
                target.ttl_hours,
                target.keep_latest,
                &protected_paths,
                &mut preview_limit,
                CleanupMode::Aggressive,
            )?
        };

        expired_total += active_plan.expired;
        selected_total += active_plan.selected;
        selected_reclaimable_bytes_total += selected_reclaimable_bytes(&active_plan);
        kept_latest_total += active_plan.kept_latest;
        protected_total += active_plan.protected;
        aggressive_preview_total += aggressive_preview.selected;
        aggressive_preview_reclaimed_bytes += selected_reclaimable_bytes(&aggressive_preview);

        let mut deleted = 0_u64;
        let mut reclaimed_bytes = 0_u64;
        for selected in &active_plan.selected_entries {
            selected_json.push(json!({
                "target_path": target.path,
                "description": target.description,
                "path": selected.path.display().to_string(),
                "age_hours": format!("{:.2}", selected.age_hours),
                "size_bytes": selected.size_bytes,
            }));
            if apply {
                delete_path(&selected.path)?;
                deleted += 1;
                reclaimed_bytes += selected.size_bytes;
            }
        }

        deleted_total += deleted;
        reclaimed_bytes_total += reclaimed_bytes;

        targets_json.push(json!({
            "path": target.path,
            "description": target.description,
            "ttl_hours": target.ttl_hours,
            "keep_latest": target.keep_latest,
            "auto_apply": target.auto_apply,
            "missing": false,
            "entries_scanned": active_plan.scanned,
            "expired": active_plan.expired,
            "selected": active_plan.selected,
            "selected_reclaimable_bytes": selected_reclaimable_bytes(&active_plan),
            "deleted": deleted,
            "reclaimed_bytes": reclaimed_bytes,
            "kept_latest": active_plan.kept_latest,
            "protected": active_plan.protected,
            "aggressive_preview_selected": aggressive_preview.selected,
            "aggressive_preview_reclaimable_bytes": selected_reclaimable_bytes(&aggressive_preview),
        }));
    }

    Ok(json!({
        "artifact_cleanup": {
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "mode": mode.as_str(),
            "apply": apply,
            "auto_only": auto_only,
            "targets_scanned": targets_scanned,
            "expired": expired_total,
            "selected": selected_total,
            "selected_reclaimable_bytes": selected_reclaimable_bytes_total,
            "deleted": deleted_total,
            "reclaimed_bytes": reclaimed_bytes_total,
            "kept_latest": kept_latest_total,
            "protected": protected_total,
            "aggressive_preview_selected": aggressive_preview_total,
            "aggressive_preview_reclaimable_bytes": aggressive_preview_reclaimed_bytes,
            "protected_paths": protected_paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            "targets": targets_json,
            "candidates": selected_json,
            "repo_inventory": collect_repo_inventory(
                repo_root,
                &managed_target_sizes,
                profile.unmanaged_root_alert_bytes,
                profile.max_unmanaged_roots,
            )?,
        }
    }))
}

pub fn write_latest_summary(repo_root: &Path, summary: &Value) -> Result<PathBuf> {
    let path = summary_path(repo_root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(
        &path,
        serde_json::to_vec_pretty(summary).context("failed to serialize cleanup summary")?,
    )
    .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

pub fn read_latest_summary(repo_root: &Path) -> Result<Option<Value>> {
    let path = summary_path(repo_root);
    if !path.exists() {
        return Ok(None);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let value = serde_json::from_str(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(value))
}

fn plan_target_cleanup(
    mut entries: Vec<CleanupEntry>,
    now: SystemTime,
    ttl_hours: u64,
    keep_latest: usize,
    protected_paths: &[PathBuf],
    remaining_limit: &mut usize,
    mode: CleanupMode,
) -> Result<TargetCleanupPlan> {
    entries.sort_by_key(|entry| Reverse(entry.modified));
    let ttl = Duration::from_secs(ttl_hours.saturating_mul(3_600));
    let keep_latest = if matches!(mode, CleanupMode::Aggressive) {
        0
    } else {
        keep_latest
    };
    let mut plan = TargetCleanupPlan {
        scanned: entries.len() as u64,
        ..TargetCleanupPlan::default()
    };

    for (index, entry) in entries.into_iter().enumerate() {
        if index < keep_latest {
            plan.kept_latest += 1;
            continue;
        }

        let canonical = entry
            .path
            .canonicalize()
            .unwrap_or_else(|_| entry.path.clone());
        if protected_paths.iter().any(|path| path == &canonical) {
            plan.protected += 1;
            continue;
        }

        let age = now
            .duration_since(entry.modified)
            .unwrap_or_else(|_| Duration::from_secs(0));
        let expired = !mode.enforce_ttl() || age >= ttl;
        if !expired {
            continue;
        }

        plan.expired += 1;
        if *remaining_limit == 0 {
            continue;
        }

        *remaining_limit -= 1;
        plan.selected += 1;
        plan.selected_entries.push(PlannedCleanupEntry {
            path: entry.path,
            age_hours: age.as_secs_f64() / 3600.0,
            size_bytes: entry.size_bytes,
        });
    }

    Ok(plan)
}

fn immediate_entries(root: &Path) -> Result<Vec<CleanupEntry>> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(root)
        .with_context(|| format!("failed to read cleanup root {}", root.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to iterate cleanup root {}", root.display()))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("failed to stat cleanup entry {}", path.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        let modified = metadata
            .modified()
            .with_context(|| format!("failed to read mtime for {}", path.display()))?;
        let size_bytes = path_size_bytes(&path)?;
        entries.push(CleanupEntry {
            path,
            modified,
            size_bytes,
        });
    }
    Ok(entries)
}

fn path_size_bytes(path: &Path) -> Result<u64> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Ok(0);
    }
    if metadata.is_file() {
        return Ok(metadata.len());
    }
    if metadata.is_dir() {
        let mut total = 0_u64;
        for entry in
            fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))?
        {
            let entry = entry.with_context(|| format!("failed to iterate {}", path.display()))?;
            total = total.saturating_add(path_size_bytes(&entry.path())?);
        }
        return Ok(total);
    }
    Ok(0)
}

fn collect_repo_inventory(
    repo_root: &Path,
    managed_target_sizes: &[(PathBuf, u64)],
    unmanaged_root_alert_bytes: u64,
    max_unmanaged_roots: usize,
) -> Result<Value> {
    let mut repo_total_bytes = 0_u64;
    let cleanup_scope_bytes = managed_target_sizes
        .iter()
        .map(|(_, size_bytes)| *size_bytes)
        .sum::<u64>();
    let mut unreadable_paths_sample = Vec::new();
    let mut unreadable_paths_count = 0_u64;
    let mut roots = Vec::new();

    for entry in
        fs::read_dir(repo_root).with_context(|| format!("failed to read {}", repo_root.display()))?
    {
        let entry =
            entry.with_context(|| format!("failed to iterate {}", repo_root.display()))?;
        let path = entry.path();
        let total_bytes = path_size_bytes_lossy(
            &path,
            &mut unreadable_paths_count,
            &mut unreadable_paths_sample,
            max_unmanaged_roots.max(3),
        );
        repo_total_bytes = repo_total_bytes.saturating_add(total_bytes);
        let managed_cleanup_scope_bytes = managed_target_sizes
            .iter()
            .filter(|(target_root, _)| target_root.starts_with(&path))
            .map(|(_, size_bytes)| *size_bytes)
            .sum::<u64>();
        let unmanaged_bytes = total_bytes.saturating_sub(managed_cleanup_scope_bytes);
        roots.push(RootInventoryEntry {
            relative_path: relative_repo_path(repo_root, &path),
            total_bytes,
            managed_cleanup_scope_bytes,
            unmanaged_bytes,
        });
    }

    roots.sort_by_key(|entry| Reverse(entry.unmanaged_bytes));
    let large_unmanaged_roots = roots
        .iter()
        .filter(|entry| entry.unmanaged_bytes >= unmanaged_root_alert_bytes)
        .take(max_unmanaged_roots)
        .map(|entry| {
            json!({
                "path": entry.relative_path,
                "total_bytes": entry.total_bytes,
                "managed_cleanup_scope_bytes": entry.managed_cleanup_scope_bytes,
                "unmanaged_bytes": entry.unmanaged_bytes,
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "repo_total_bytes": repo_total_bytes,
        "cleanup_scope_bytes": cleanup_scope_bytes,
        "out_of_policy_bytes": repo_total_bytes.saturating_sub(cleanup_scope_bytes),
        "unmanaged_root_alert_bytes": unmanaged_root_alert_bytes,
        "unmanaged_alert_triggered": !large_unmanaged_roots.is_empty(),
        "large_unmanaged_roots": large_unmanaged_roots,
        "unreadable_paths_count": unreadable_paths_count,
        "unreadable_paths_sample": unreadable_paths_sample,
    }))
}

fn path_size_bytes_lossy(
    path: &Path,
    unreadable_paths_count: &mut u64,
    unreadable_paths_sample: &mut Vec<String>,
    unreadable_sample_limit: usize,
) -> u64 {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => {
            record_unreadable_path(path, unreadable_paths_count, unreadable_paths_sample, unreadable_sample_limit);
            return 0;
        }
    };
    if metadata.file_type().is_symlink() {
        return 0;
    }
    if metadata.is_file() {
        return metadata.len();
    }
    if metadata.is_dir() {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(_) => {
                record_unreadable_path(path, unreadable_paths_count, unreadable_paths_sample, unreadable_sample_limit);
                return 0;
            }
        };
        let mut total = 0_u64;
        for entry in entries {
            match entry {
                Ok(entry) => {
                    total = total.saturating_add(path_size_bytes_lossy(
                        &entry.path(),
                        unreadable_paths_count,
                        unreadable_paths_sample,
                        unreadable_sample_limit,
                    ));
                }
                Err(_) => {
                    record_unreadable_path(
                        path,
                        unreadable_paths_count,
                        unreadable_paths_sample,
                        unreadable_sample_limit,
                    );
                }
            }
        }
        return total;
    }
    0
}

fn record_unreadable_path(
    path: &Path,
    unreadable_paths_count: &mut u64,
    unreadable_paths_sample: &mut Vec<String>,
    unreadable_sample_limit: usize,
) {
    *unreadable_paths_count = unreadable_paths_count.saturating_add(1);
    if unreadable_paths_sample.len() < unreadable_sample_limit {
        unreadable_paths_sample.push(path.display().to_string());
    }
}

fn relative_repo_path(repo_root: &Path, path: &Path) -> String {
    path.strip_prefix(repo_root)
        .ok()
        .map(|relative| relative.display().to_string())
        .filter(|relative| !relative.is_empty())
        .unwrap_or_else(|| path.display().to_string())
}

fn delete_path(path: &Path) -> Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove directory {}", path.display()))?;
    } else {
        fs::remove_file(path)
            .with_context(|| format!("failed to remove file {}", path.display()))?;
    }
    Ok(())
}

fn selected_reclaimable_bytes(plan: &TargetCleanupPlan) -> u64 {
    plan.selected_entries
        .iter()
        .map(|entry| entry.size_bytes)
        .sum::<u64>()
}

fn current_protected_paths() -> Vec<PathBuf> {
    env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .into_iter()
        .collect()
}

fn profile_path() -> PathBuf {
    let cwd_path = Path::new("config/observability.toml");
    if cwd_path.exists() {
        cwd_path.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("config")
            .join("observability.toml")
    }
}

fn summary_path(repo_root: &Path) -> PathBuf {
    repo_root.join(SUMMARY_RELATIVE_PATH)
}

#[cfg(test)]
mod tests {
    use super::{
        CleanupEntry, CleanupMode, collect_repo_inventory, current_protected_paths,
        default_max_unmanaged_roots, default_unmanaged_root_alert_bytes, immediate_entries,
        plan_target_cleanup,
    };
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};

    #[test]
    fn cleanup_plan_keeps_latest_and_selects_only_old_entries() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(48 * 3600);
        let entries = vec![
            CleanupEntry {
                path: PathBuf::from("/tmp/newest"),
                modified: now - Duration::from_secs(60),
                size_bytes: 10,
            },
            CleanupEntry {
                path: PathBuf::from("/tmp/old_a"),
                modified: now - Duration::from_secs(30 * 3600),
                size_bytes: 20,
            },
            CleanupEntry {
                path: PathBuf::from("/tmp/old_b"),
                modified: now - Duration::from_secs(40 * 3600),
                size_bytes: 30,
            },
        ];
        let mut limit = usize::MAX;
        let plan = plan_target_cleanup(
            entries,
            now,
            24,
            1,
            &[],
            &mut limit,
            CleanupMode::Conservative,
        )
        .expect("plan");
        assert_eq!(plan.scanned, 3);
        assert_eq!(plan.kept_latest, 1);
        assert_eq!(plan.expired, 2);
        assert_eq!(plan.selected, 2);
        assert_eq!(plan.selected_entries.len(), 2);
        assert_eq!(plan.selected_entries[0].path, PathBuf::from("/tmp/old_a"));
        assert_eq!(plan.selected_entries[1].path, PathBuf::from("/tmp/old_b"));
    }

    #[test]
    fn cleanup_plan_respects_limit_and_protected_paths() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(48 * 3600);
        let protected = vec![PathBuf::from("/tmp/protected")];
        let entries = vec![
            CleanupEntry {
                path: PathBuf::from("/tmp/protected"),
                modified: now - Duration::from_secs(40 * 3600),
                size_bytes: 10,
            },
            CleanupEntry {
                path: PathBuf::from("/tmp/old_a"),
                modified: now - Duration::from_secs(39 * 3600),
                size_bytes: 20,
            },
            CleanupEntry {
                path: PathBuf::from("/tmp/old_b"),
                modified: now - Duration::from_secs(38 * 3600),
                size_bytes: 30,
            },
        ];
        let mut limit = 1;
        let plan = plan_target_cleanup(
            entries,
            now,
            24,
            0,
            &protected,
            &mut limit,
            CleanupMode::Conservative,
        )
        .expect("plan");
        assert_eq!(plan.protected, 1);
        assert_eq!(plan.expired, 2);
        assert_eq!(plan.selected, 1);
        assert_eq!(plan.selected_entries[0].path, PathBuf::from("/tmp/old_b"));
        assert_eq!(limit, 0);
        assert!(!current_protected_paths().contains(&PathBuf::from("/tmp/protected")));
    }

    #[test]
    fn aggressive_cleanup_ignores_ttl_and_keep_latest() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(48 * 3600);
        let entries = vec![
            CleanupEntry {
                path: PathBuf::from("/tmp/newest"),
                modified: now - Duration::from_secs(60),
                size_bytes: 10,
            },
            CleanupEntry {
                path: PathBuf::from("/tmp/not_old_enough"),
                modified: now - Duration::from_secs(2 * 3600),
                size_bytes: 20,
            },
        ];
        let mut limit = usize::MAX;
        let plan = plan_target_cleanup(
            entries,
            now,
            24,
            1,
            &[],
            &mut limit,
            CleanupMode::Aggressive,
        )
        .expect("plan");
        assert_eq!(plan.kept_latest, 0);
        assert_eq!(plan.expired, 2);
        assert_eq!(plan.selected, 2);
        assert_eq!(plan.selected_entries[0].path, PathBuf::from("/tmp/newest"));
        assert_eq!(
            plan.selected_entries[1].path,
            PathBuf::from("/tmp/not_old_enough")
        );
    }

    #[test]
    fn repo_inventory_surfaces_large_unmanaged_roots() {
        let repo_root = std::env::temp_dir().join(format!(
            "amai-artifact-cleanup-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&repo_root);
        fs::create_dir_all(repo_root.join("target/debug")).expect("target/debug");
        fs::create_dir_all(repo_root.join("output/windows-vm-lab")).expect("windows-vm-lab");
        fs::write(repo_root.join("target/debug/amai"), vec![0_u8; 64]).expect("managed file");
        fs::write(
            repo_root.join("output/windows-vm-lab/system.qcow2"),
            vec![0_u8; 128],
        )
        .expect("unmanaged file");

        let inventory = collect_repo_inventory(
            &repo_root,
            &[(repo_root.join("target/debug"), 64)],
            100,
            default_max_unmanaged_roots(),
        )
        .expect("inventory");

        assert_eq!(inventory["repo_total_bytes"].as_u64(), Some(192));
        assert_eq!(inventory["cleanup_scope_bytes"].as_u64(), Some(64));
        assert_eq!(inventory["out_of_policy_bytes"].as_u64(), Some(128));
        assert_eq!(inventory["unmanaged_root_alert_bytes"].as_u64(), Some(100));
        assert_eq!(inventory["unmanaged_alert_triggered"].as_bool(), Some(true));
        assert_eq!(
            inventory["large_unmanaged_roots"][0]["path"].as_str(),
            Some("output")
        );
        assert_eq!(
            inventory["large_unmanaged_roots"][0]["unmanaged_bytes"].as_u64(),
            Some(128)
        );

        let _ = fs::remove_dir_all(&repo_root);
        let _ = default_unmanaged_root_alert_bytes();
    }

    #[test]
    fn immediate_entries_skip_symlinks() {
        let repo_root = std::env::temp_dir().join(format!(
            "amai-artifact-cleanup-symlink-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&repo_root);
        fs::create_dir_all(&repo_root).expect("repo root");
        fs::create_dir_all(repo_root.join("20260325-proof")).expect("proof dir");
        fs::write(repo_root.join("20260325-proof/serial.log"), b"log").expect("log file");
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            repo_root.join("20260325-proof"),
            repo_root.join("latest"),
        )
        .expect("latest symlink");

        let entries = immediate_entries(&repo_root).expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, repo_root.join("20260325-proof"));

        let _ = fs::remove_dir_all(&repo_root);
    }
}
