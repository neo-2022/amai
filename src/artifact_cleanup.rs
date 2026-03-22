use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use std::cmp::Reverse;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Deserialize)]
struct ArtifactCleanupDocument {
    artifact_cleanup: ArtifactCleanupProfile,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ArtifactCleanupProfile {
    pub sweep_interval_seconds: u64,
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

#[derive(Debug, Clone, Default)]
struct TargetCleanupPlan {
    scanned: u64,
    kept_latest: u64,
    protected: u64,
    expired: u64,
    selected: u64,
    selected_entries: Vec<PlannedCleanupEntry>,
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
) -> Result<Value> {
    let profile = load_profile()?;
    let now = SystemTime::now();
    let protected_paths = current_protected_paths();
    let mut remaining_limit = limit.unwrap_or(usize::MAX);

    let mut targets_json = Vec::new();
    let mut selected_json = Vec::new();
    let mut targets_scanned = 0_u64;
    let mut expired_total = 0_u64;
    let mut selected_total = 0_u64;
    let mut deleted_total = 0_u64;
    let mut reclaimed_bytes_total = 0_u64;

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
                "deleted": 0,
                "reclaimed_bytes": 0,
                "kept_latest": 0,
                "protected": 0,
            }));
            continue;
        }

        let entries = immediate_entries(&root)?;
        let plan = plan_target_cleanup(
            entries,
            now,
            target.ttl_hours,
            target.keep_latest,
            &protected_paths,
            &mut remaining_limit,
        )?;

        expired_total += plan.expired;
        selected_total += plan.selected;

        let mut deleted = 0_u64;
        let mut reclaimed_bytes = 0_u64;
        for selected in &plan.selected_entries {
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
            "entries_scanned": plan.scanned,
            "expired": plan.expired,
            "selected": plan.selected,
            "deleted": deleted,
            "reclaimed_bytes": reclaimed_bytes,
            "kept_latest": plan.kept_latest,
            "protected": plan.protected,
        }));
    }

    Ok(json!({
        "artifact_cleanup": {
            "apply": apply,
            "auto_only": auto_only,
            "targets_scanned": targets_scanned,
            "expired": expired_total,
            "selected": selected_total,
            "deleted": deleted_total,
            "reclaimed_bytes": reclaimed_bytes_total,
            "protected_paths": protected_paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>(),
            "targets": targets_json,
            "candidates": selected_json,
        }
    }))
}

fn plan_target_cleanup(
    mut entries: Vec<CleanupEntry>,
    now: SystemTime,
    ttl_hours: u64,
    keep_latest: usize,
    protected_paths: &[PathBuf],
    remaining_limit: &mut usize,
) -> Result<TargetCleanupPlan> {
    entries.sort_by_key(|entry| Reverse(entry.modified));
    let ttl = Duration::from_secs(ttl_hours.saturating_mul(3_600));
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
        if age < ttl {
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

#[cfg(test)]
mod tests {
    use super::{CleanupEntry, current_protected_paths, plan_target_cleanup};
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
        let plan = plan_target_cleanup(entries, now, 24, 1, &[], &mut limit).expect("plan");
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
        let plan = plan_target_cleanup(entries, now, 24, 0, &protected, &mut limit).expect("plan");
        assert_eq!(plan.protected, 1);
        assert_eq!(plan.expired, 2);
        assert_eq!(plan.selected, 1);
        assert_eq!(plan.selected_entries[0].path, PathBuf::from("/tmp/old_b"));
        assert_eq!(limit, 0);
        assert!(!current_protected_paths().contains(&PathBuf::from("/tmp/protected")));
    }
}
