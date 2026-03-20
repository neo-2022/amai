use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use sysinfo::{Disks, System};

#[derive(Debug, Deserialize)]
struct DeploymentProfilesFile {
    profiles: std::collections::BTreeMap<String, DeploymentProfile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DeploymentProfile {
    pub display_name: String,
    pub summary: String,
    pub suitable_for: Vec<String>,
    pub not_for: Vec<String>,
    pub minimum_cpu_logical: usize,
    pub minimum_memory_gib: f64,
    pub minimum_disk_gib: f64,
    pub recommended_cpu_logical: usize,
    pub recommended_memory_gib: f64,
    pub recommended_disk_gib: f64,
    pub supports_peak_benchmarks: bool,
    pub start_monitoring_by_default: bool,
    pub remote_mode_recommended: bool,
}

#[derive(Debug)]
pub struct PreflightReport {
    pub profile_code: String,
    pub profile: DeploymentProfile,
    pub host_logical_cpus: usize,
    pub host_total_memory_gib: f64,
    pub host_available_memory_gib: f64,
    pub host_available_disk_gib: f64,
    pub verdict: &'static str,
    pub unmet_minimums: Vec<String>,
    pub unmet_recommendations: Vec<String>,
}

pub fn load_profile(repo_root: &Path, profile_code: &str) -> Result<DeploymentProfile> {
    let path = profiles_path(repo_root);
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read deployment profiles {}", path.display()))?;
    let registry: DeploymentProfilesFile =
        toml::from_str(&content).context("failed to parse deployment profiles")?;
    registry
        .profiles
        .get(profile_code)
        .cloned()
        .ok_or_else(|| anyhow!("unknown deployment profile: {profile_code}"))
}

pub fn print_preflight(repo_root: &Path, profile_code: &str) -> Result<()> {
    let report = preflight_report(repo_root, profile_code)?;
    println!("deployment profile: {}", report.profile_code);
    println!("profile display name: {}", report.profile.display_name);
    println!("summary: {}", report.profile.summary);
    println!("host logical cpu: {}", report.host_logical_cpus);
    println!("host total memory gib: {:.2}", report.host_total_memory_gib);
    println!(
        "host available memory gib: {:.2}",
        report.host_available_memory_gib
    );
    println!(
        "host available disk gib: {:.2}",
        report.host_available_disk_gib
    );
    println!("verdict: {}", report.verdict);
    println!(
        "supports peak benchmarks: {}",
        report.profile.supports_peak_benchmarks
    );
    println!(
        "monitoring by default: {}",
        report.profile.start_monitoring_by_default
    );
    println!(
        "remote mode recommended: {}",
        report.profile.remote_mode_recommended
    );
    println!(
        "minimum requirements: {} logical cpu, {:.1} GiB memory, {:.1} GiB disk",
        report.profile.minimum_cpu_logical,
        report.profile.minimum_memory_gib,
        report.profile.minimum_disk_gib
    );
    println!(
        "recommended requirements: {} logical cpu, {:.1} GiB memory, {:.1} GiB disk",
        report.profile.recommended_cpu_logical,
        report.profile.recommended_memory_gib,
        report.profile.recommended_disk_gib
    );
    println!("suitable for:");
    for item in &report.profile.suitable_for {
        println!("- {}", item);
    }
    println!("not for:");
    for item in &report.profile.not_for {
        println!("- {}", item);
    }
    if !report.unmet_minimums.is_empty() {
        println!("minimum risks:");
        for item in &report.unmet_minimums {
            println!("- {}", item);
        }
    }
    if !report.unmet_recommendations.is_empty() {
        println!("recommendation gaps:");
        for item in &report.unmet_recommendations {
            println!("- {}", item);
        }
    }
    Ok(())
}

pub fn preflight_report(repo_root: &Path, profile_code: &str) -> Result<PreflightReport> {
    let profile = load_profile(repo_root, profile_code)?;
    let host = probe_host(repo_root)?;

    let mut unmet_minimums = Vec::new();
    let mut unmet_recommendations = Vec::new();

    if host.logical_cpus < profile.minimum_cpu_logical {
        unmet_minimums.push(format!(
            "logical CPU below minimum: {} < {}",
            host.logical_cpus, profile.minimum_cpu_logical
        ));
    } else if host.logical_cpus < profile.recommended_cpu_logical {
        unmet_recommendations.push(format!(
            "logical CPU below recommendation: {} < {}",
            host.logical_cpus, profile.recommended_cpu_logical
        ));
    }

    if host.total_memory_gib < profile.minimum_memory_gib {
        unmet_minimums.push(format!(
            "total memory below minimum: {:.2} < {:.1} GiB",
            host.total_memory_gib, profile.minimum_memory_gib
        ));
    } else if host.total_memory_gib < profile.recommended_memory_gib {
        unmet_recommendations.push(format!(
            "total memory below recommendation: {:.2} < {:.1} GiB",
            host.total_memory_gib, profile.recommended_memory_gib
        ));
    }

    if host.available_disk_gib < profile.minimum_disk_gib {
        unmet_minimums.push(format!(
            "available disk below minimum: {:.2} < {:.1} GiB",
            host.available_disk_gib, profile.minimum_disk_gib
        ));
    } else if host.available_disk_gib < profile.recommended_disk_gib {
        unmet_recommendations.push(format!(
            "available disk below recommendation: {:.2} < {:.1} GiB",
            host.available_disk_gib, profile.recommended_disk_gib
        ));
    }

    let verdict = if !unmet_minimums.is_empty() {
        "fail"
    } else if !unmet_recommendations.is_empty() {
        "warn"
    } else {
        "pass"
    };

    Ok(PreflightReport {
        profile_code: profile_code.to_string(),
        profile,
        host_logical_cpus: host.logical_cpus,
        host_total_memory_gib: host.total_memory_gib,
        host_available_memory_gib: host.available_memory_gib,
        host_available_disk_gib: host.available_disk_gib,
        verdict,
        unmet_minimums,
        unmet_recommendations,
    })
}

fn profiles_path(repo_root: &Path) -> PathBuf {
    repo_root.join("config/deployment_profiles.toml")
}

struct HostSnapshot {
    logical_cpus: usize,
    total_memory_gib: f64,
    available_memory_gib: f64,
    available_disk_gib: f64,
}

fn probe_host(repo_root: &Path) -> Result<HostSnapshot> {
    let mut system = System::new_all();
    system.refresh_memory();

    let disks = Disks::new_with_refreshed_list();
    let available_disk_bytes = match disk_available_for_path(&disks, repo_root) {
        Some(value) => value,
        None => bail!(
            "failed to detect available disk space for {}",
            repo_root.display()
        ),
    };

    Ok(HostSnapshot {
        logical_cpus: system.cpus().len(),
        total_memory_gib: bytes_to_gib(system.total_memory()),
        available_memory_gib: bytes_to_gib(system.available_memory()),
        available_disk_gib: bytes_to_gib(available_disk_bytes),
    })
}

fn disk_available_for_path(disks: &Disks, path: &Path) -> Option<u64> {
    let canonical = path.canonicalize().ok()?;
    disks
        .iter()
        .filter(|disk| canonical.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| disk.available_space())
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / 1024_f64 / 1024_f64 / 1024_f64
}

#[cfg(test)]
mod tests {
    use super::DeploymentProfile;

    #[test]
    fn profile_supports_plain_human_fields() {
        let profile = DeploymentProfile {
            display_name: "Lite VPS".to_string(),
            summary: "cheap remote smoke profile".to_string(),
            suitable_for: vec!["small smoke".to_string()],
            not_for: vec!["peak benchmark".to_string()],
            minimum_cpu_logical: 1,
            minimum_memory_gib: 2.0,
            minimum_disk_gib: 20.0,
            recommended_cpu_logical: 2,
            recommended_memory_gib: 4.0,
            recommended_disk_gib: 30.0,
            supports_peak_benchmarks: false,
            start_monitoring_by_default: false,
            remote_mode_recommended: true,
        };

        assert_eq!(profile.display_name, "Lite VPS");
        assert!(!profile.supports_peak_benchmarks);
        assert!(profile.remote_mode_recommended);
    }
}
