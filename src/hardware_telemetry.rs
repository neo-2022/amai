use anyhow::Result;
use serde_json::Value;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::thread;
use sysinfo::{DiskRefreshKind, Disks, MINIMUM_CPU_UPDATE_INTERVAL, MemoryRefreshKind, System};

#[derive(Debug, Clone)]
pub struct MachineSummary {
    pub cpu_model: String,
    pub logical_cpus: usize,
    pub physical_cpus: Option<usize>,
    pub cpu_usage_percent: Option<f64>,
    pub cpu_temperature_celsius: Option<f64>,
    pub cpu_max_mhz: Option<f64>,
    pub cpu_source_label: String,
    pub total_memory_gib: f64,
    pub available_memory_gib: f64,
    pub used_memory_gib: f64,
    pub memory_used_percent: Option<f64>,
    pub memory_type: String,
    pub memory_speed_label: String,
    pub memory_source_label: String,
    pub swap_total_gib: f64,
    pub swap_used_gib: f64,
    pub disk_device: Option<String>,
    pub disk_model: String,
    pub disk_kind: String,
    pub disk_source_label: String,
    pub disk_total_gib: f64,
    pub disk_available_gib: f64,
    pub disk_used_percent: Option<f64>,
    pub disk_busy_percent: Option<f64>,
    pub disk_read_mib_per_sec: Option<f64>,
    pub disk_write_mib_per_sec: Option<f64>,
    pub disk_temperature_celsius: Option<f64>,
    pub disk_firmware: String,
    pub accelerators: Vec<AcceleratorSummary>,
}

#[derive(Debug, Clone)]
pub struct AcceleratorSummary {
    pub detected: bool,
    pub kind_label: String,
    pub model: String,
    pub backend: String,
    pub driver_version: Option<String>,
    pub total_vram_gib: Option<f64>,
    pub used_vram_gib: Option<f64>,
    pub utilization_percent: Option<f64>,
    pub temperature_celsius: Option<f64>,
    pub power_watts: Option<f64>,
    pub bus_label: Option<String>,
    pub source_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AcceleratorKind {
    ExternalGpu,
    DiscreteGpu,
    IntegratedGpu,
    Npu,
    Tpu,
    Asic,
    Fpga,
    Accelerator,
    Unknown,
}

#[derive(Debug, Clone)]
struct DiskTelemetry {
    device: Option<String>,
    model: String,
    kind: String,
    firmware: String,
    source_label: String,
    busy_percent: Option<f64>,
    read_mib_per_sec: Option<f64>,
    write_mib_per_sec: Option<f64>,
    temperature_celsius: Option<f64>,
}

#[derive(Debug, Clone)]
struct DiskIoSample {
    device: String,
    read_sectors: u64,
    write_sectors: u64,
    io_millis: u64,
    captured_at_ms: u64,
}

#[derive(Debug, Clone)]
struct DiskLiveStats {
    busy_percent: Option<f64>,
    read_mib_per_sec: Option<f64>,
    write_mib_per_sec: Option<f64>,
}

#[derive(Debug, Clone)]
struct CachedMachineSummary {
    repo_root: PathBuf,
    captured_at_ms: u64,
    summary: MachineSummary,
}

#[derive(Debug, Clone)]
struct CachedMemoryCharacteristics {
    platform: HostPlatform,
    captured_at_ms: u64,
    memory_type: String,
    memory_speed_label: String,
    provider: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostPlatform {
    Linux,
    Macos,
    Windows,
    Unknown,
}

impl HostPlatform {
    fn current() -> Self {
        match std::env::consts::OS {
            "linux" => Self::Linux,
            "macos" => Self::Macos,
            "windows" => Self::Windows,
            _ => Self::Unknown,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Linux => "Linux",
            Self::Macos => "macOS",
            Self::Windows => "Windows",
            Self::Unknown => "unknown OS",
        }
    }
}

static CPU_SYSTEM_CACHE: OnceLock<Mutex<System>> = OnceLock::new();
static DISK_IO_CACHE: OnceLock<Mutex<Option<DiskIoSample>>> = OnceLock::new();
static MACHINE_SUMMARY_CACHE: OnceLock<Mutex<Option<CachedMachineSummary>>> = OnceLock::new();
static MEMORY_CHARACTERISTICS_CACHE: OnceLock<Mutex<Option<CachedMemoryCharacteristics>>> =
    OnceLock::new();
const MACHINE_SUMMARY_CACHE_TTL_MS: u64 = 60_000;
const MEMORY_CHARACTERISTICS_CACHE_TTL_MS: u64 = 6 * 60 * 60 * 1000;

pub fn collect_machine_summary(repo_root: &Path) -> Result<MachineSummary> {
    if let Some(summary) = cached_machine_summary(repo_root) {
        return Ok(summary);
    }
    let summary = collect_machine_summary_uncached(repo_root)?;
    store_machine_summary_cache(repo_root, &summary);
    Ok(summary)
}

fn collect_machine_summary_uncached(repo_root: &Path) -> Result<MachineSummary> {
    let platform = HostPlatform::current();
    let mut system = System::new();
    system.refresh_memory_specifics(MemoryRefreshKind::everything());

    let cpu_model = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "модель CPU не определена".to_string());
    let logical_cpus = system.cpus().len();
    let physical_cpus = System::physical_core_count();
    let cpu_usage_percent = read_cpu_usage_percent();
    let (cpu_temperature_celsius, cpu_temp_provider) = detect_cpu_temperature(platform);
    let (cpu_max_mhz, cpu_frequency_provider) = detect_cpu_max_mhz(platform, &system);

    let total_memory_gib = bytes_to_gib(system.total_memory());
    let available_memory_gib = bytes_to_gib(system.available_memory());
    let used_memory_gib = (total_memory_gib - available_memory_gib).max(0.0);
    let memory_used_percent = percentage_from_parts(used_memory_gib, total_memory_gib);
    let swap_total_gib = bytes_to_gib(system.total_swap());
    let swap_used_gib = bytes_to_gib(system.used_swap());
    let (memory_type, memory_speed_label, memory_provider) =
        detect_memory_characteristics(platform);

    let disks = Disks::new_with_refreshed_list_specifics(DiskRefreshKind::nothing().with_storage());
    let disk_space = disk_space_for_path(&disks, repo_root);
    let disk_total_gib = disk_space
        .map(|(total, _)| bytes_to_gib(total))
        .unwrap_or_default();
    let disk_available_gib = disk_space
        .map(|(_, available)| bytes_to_gib(available))
        .unwrap_or_default();
    let disk_used_percent = percentage_from_parts(
        (disk_total_gib - disk_available_gib).max(0.0),
        disk_total_gib,
    );
    let disk = detect_disk_telemetry(platform, repo_root);
    let accelerators = detect_accelerators(platform);

    Ok(MachineSummary {
        cpu_model,
        logical_cpus,
        physical_cpus,
        cpu_usage_percent,
        cpu_temperature_celsius,
        cpu_max_mhz,
        cpu_source_label: format!(
            "Источник: {} auto-detect. Нагрузка: sysinfo; температура: {}; частота: {}. Dashboard reuses machine summary for up to {} s.",
            platform.label(),
            cpu_temp_provider,
            cpu_frequency_provider,
            MACHINE_SUMMARY_CACHE_TTL_MS / 1000
        ),
        total_memory_gib,
        available_memory_gib,
        used_memory_gib,
        memory_used_percent,
        memory_type,
        memory_speed_label,
        memory_source_label: format!(
            "Источник: {} auto-detect provider chain: {}. Dashboard reuses live machine summary for up to {} s; static memory inventory is cached up to {} h.",
            platform.label(),
            memory_provider,
            MACHINE_SUMMARY_CACHE_TTL_MS / 1000,
            MEMORY_CHARACTERISTICS_CACHE_TTL_MS / (60 * 60 * 1000)
        ),
        swap_total_gib,
        swap_used_gib,
        disk_device: disk.device,
        disk_model: disk.model,
        disk_kind: disk.kind,
        disk_source_label: format!(
            "Источник: {} auto-detect provider chain: {}. Dashboard reuses machine summary for up to {} s.",
            platform.label(),
            disk.source_label,
            MACHINE_SUMMARY_CACHE_TTL_MS / 1000
        ),
        disk_total_gib,
        disk_available_gib,
        disk_used_percent,
        disk_busy_percent: disk.busy_percent,
        disk_read_mib_per_sec: disk.read_mib_per_sec,
        disk_write_mib_per_sec: disk.write_mib_per_sec,
        disk_temperature_celsius: disk.temperature_celsius,
        disk_firmware: disk.firmware,
        accelerators,
    })
}

fn read_cpu_usage_percent() -> Option<f64> {
    let cache = CPU_SYSTEM_CACHE.get_or_init(|| {
        let mut system = System::new();
        system.refresh_cpu_all();
        thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
        system.refresh_cpu_usage();
        Mutex::new(system)
    });
    let mut guard = cache.lock().ok()?;
    guard.refresh_cpu_usage();
    let value = guard.global_cpu_usage() as f64;
    if value.is_nan() { None } else { Some(value) }
}

fn detect_cpu_temperature(platform: HostPlatform) -> (Option<f64>, &'static str) {
    match platform {
        HostPlatform::Linux => {
            for (chip, label) in [
                ("k10temp", Some("Tctl")),
                ("k10temp", Some("Tdie")),
                ("zenpower", Some("Tdie")),
                ("coretemp", Some("Package id 0")),
                ("cpu_thermal", None),
            ] {
                if let Some(value) = read_linux_hwmon_temperature(chip, label) {
                    return (Some(value), "Linux hwmon");
                }
            }
            (None, "нет доступного Linux hwmon sensor")
        }
        HostPlatform::Macos => {
            if let Some(value) = read_macos_cpu_temperature() {
                return (Some(value), "powermetrics/istats");
            }
            (None, "нет доступного macOS thermal provider")
        }
        HostPlatform::Windows => {
            if let Some(value) = read_windows_cpu_temperature() {
                return (Some(value), "PowerShell WMI thermal zone");
            }
            (None, "нет доступного Windows thermal provider")
        }
        HostPlatform::Unknown => (None, "неизвестная ОС"),
    }
}

fn detect_cpu_max_mhz(platform: HostPlatform, system: &System) -> (Option<f64>, &'static str) {
    match platform {
        HostPlatform::Linux => {
            if let Some(value) = read_linux_lscpu_numeric_field("CPU max MHz:") {
                return (Some(value), "lscpu");
            }
        }
        HostPlatform::Macos => {
            if let Some(value) = run_command_text("sysctl", ["-n", "hw.cpufrequency_max"])
                .and_then(|text| text.trim().parse::<f64>().ok())
            {
                return (Some(value / 1_000_000.0), "sysctl hw.cpufrequency_max");
            }
        }
        HostPlatform::Windows => {
            if let Some(value) = run_powershell_json(
                "Get-CimInstance Win32_Processor | Select-Object -First 1 Name,MaxClockSpeed | ConvertTo-Json -Compress"
            ).and_then(|json| json["MaxClockSpeed"].as_f64()) {
                return (Some(value), "PowerShell Win32_Processor");
            }
        }
        HostPlatform::Unknown => {}
    }
    let fallback = system.cpus().first().map(|cpu| cpu.frequency() as f64);
    (fallback, "sysinfo fallback")
}

fn detect_memory_characteristics(platform: HostPlatform) -> (String, String, String) {
    if let Some(cached) = cached_memory_characteristics(platform) {
        return cached;
    }
    let detected = match platform {
        HostPlatform::Linux => detect_linux_memory_characteristics(),
        HostPlatform::Macos => detect_macos_memory_characteristics(),
        HostPlatform::Windows => detect_windows_memory_characteristics(),
        HostPlatform::Unknown => (
            "система не дала определить автоматически".to_string(),
            "не удалось определить автоматически".to_string(),
            "неизвестная ОС".to_string(),
        ),
    };
    store_memory_characteristics_cache(platform, &detected.0, &detected.1, &detected.2);
    detected
}

fn detect_linux_memory_characteristics() -> (String, String, String) {
    for (provider, program, args) in [
        (
            "sudo dmidecode",
            "sudo",
            vec!["-n", "dmidecode", "--type", "17"],
        ),
        ("dmidecode", "dmidecode", vec!["--type", "17"]),
        ("lshw", "lshw", vec!["-class", "memory"]),
        ("inxi", "inxi", vec!["-m"]),
    ] {
        let Some(text) = run_command_text(program, args) else {
            continue;
        };
        let memory_type = extract_memory_generation(&text)
            .unwrap_or_else(|| "система не дала определить автоматически".to_string());
        let memory_speed = extract_memory_speed(&text)
            .map(|value| format!("{value} MT/s"))
            .unwrap_or_else(|| "не удалось определить автоматически".to_string());
        return (memory_type, memory_speed, provider.to_string());
    }
    (
        "система не дала определить автоматически".to_string(),
        "не удалось определить автоматически".to_string(),
        "Linux provider chain exhausted".to_string(),
    )
}

fn detect_macos_memory_characteristics() -> (String, String, String) {
    if let Some(json) = run_command_json("system_profiler", ["SPMemoryDataType", "-json"]) {
        let memory_type = find_first_string_by_key_contains(&json, &["dimm_type", "memory type"])
            .or_else(|| find_first_string_by_key_contains(&json, &["type"]))
            .unwrap_or_else(detect_apple_unified_memory_type);
        let memory_speed = find_first_string_by_key_contains(&json, &["dimm_speed", "speed"])
            .unwrap_or_else(|| "не удалось определить автоматически".to_string());
        return (
            memory_type,
            normalize_speed_label(&memory_speed),
            "system_profiler SPMemoryDataType".to_string(),
        );
    }
    (
        detect_apple_unified_memory_type(),
        "не удалось определить автоматически".to_string(),
        "system_profiler fallback".to_string(),
    )
}

fn detect_apple_unified_memory_type() -> String {
    let arm64 = run_command_text("sysctl", ["-n", "hw.optional.arm64"])
        .map(|value| value.trim() == "1")
        .unwrap_or(false);
    if arm64 {
        "Unified".to_string()
    } else {
        "система не дала определить автоматически".to_string()
    }
}

fn detect_windows_memory_characteristics() -> (String, String, String) {
    let Some(json) = run_powershell_json(
        "Get-CimInstance Win32_PhysicalMemory | Select-Object -First 1 SMBIOSMemoryType,MemoryType,ConfiguredClockSpeed,Speed | ConvertTo-Json -Compress",
    ) else {
        return (
            "система не дала определить автоматически".to_string(),
            "не удалось определить автоматически".to_string(),
            "PowerShell Win32_PhysicalMemory unavailable".to_string(),
        );
    };
    let memory_type = json["SMBIOSMemoryType"]
        .as_u64()
        .and_then(map_windows_smbios_memory_type)
        .or_else(|| {
            json["MemoryType"]
                .as_u64()
                .and_then(map_windows_legacy_memory_type)
        })
        .unwrap_or_else(|| "система не дала определить автоматически".to_string());
    let memory_speed = json["ConfiguredClockSpeed"]
        .as_u64()
        .or_else(|| json["Speed"].as_u64())
        .map(|value| format!("{value} MT/s"))
        .unwrap_or_else(|| "не удалось определить автоматически".to_string());
    (
        memory_type,
        memory_speed,
        "PowerShell Win32_PhysicalMemory".to_string(),
    )
}

fn detect_disk_telemetry(platform: HostPlatform, repo_root: &Path) -> DiskTelemetry {
    match platform {
        HostPlatform::Linux => detect_linux_disk_telemetry(repo_root),
        HostPlatform::Macos => detect_macos_disk_telemetry(repo_root),
        HostPlatform::Windows => detect_windows_disk_telemetry(repo_root),
        HostPlatform::Unknown => DiskTelemetry {
            device: None,
            model: "модель диска не определена".to_string(),
            kind: "тип диска не определён".to_string(),
            firmware: "данные недоступны".to_string(),
            source_label: "неизвестная ОС".to_string(),
            busy_percent: None,
            read_mib_per_sec: None,
            write_mib_per_sec: None,
            temperature_celsius: None,
        },
    }
}

fn detect_linux_disk_telemetry(repo_root: &Path) -> DiskTelemetry {
    let disk_device = detect_linux_disk_device_for_path(repo_root);
    let model = disk_device
        .as_deref()
        .and_then(read_linux_disk_model)
        .unwrap_or_else(|| "модель диска не определена".to_string());
    let kind = disk_device
        .as_deref()
        .map(detect_linux_disk_kind)
        .unwrap_or_else(|| "тип диска не определён".to_string());
    let live = disk_device.as_deref().and_then(read_linux_disk_live_stats);
    DiskTelemetry {
        device: disk_device.clone(),
        model,
        kind,
        firmware: disk_device
            .as_deref()
            .and_then(read_linux_disk_firmware)
            .unwrap_or_else(|| "данные недоступны".to_string()),
        source_label: "Linux df + sysfs + hwmon + /proc/diskstats".to_string(),
        busy_percent: live.as_ref().and_then(|stats| stats.busy_percent),
        read_mib_per_sec: live.as_ref().and_then(|stats| stats.read_mib_per_sec),
        write_mib_per_sec: live.as_ref().and_then(|stats| stats.write_mib_per_sec),
        temperature_celsius: disk_device.as_deref().and_then(read_linux_disk_temperature),
    }
}

fn detect_macos_disk_telemetry(repo_root: &Path) -> DiskTelemetry {
    let repo_root_str = repo_root.display().to_string();
    let Some(text) = run_command_text_dynamic("diskutil", ["info", repo_root_str.as_str()]) else {
        return DiskTelemetry {
            device: None,
            model: "модель диска не определена".to_string(),
            kind: "тип диска не определён".to_string(),
            firmware: "данные недоступны".to_string(),
            source_label: "diskutil unavailable".to_string(),
            busy_percent: None,
            read_mib_per_sec: None,
            write_mib_per_sec: None,
            temperature_celsius: None,
        };
    };
    let device = find_line_value(
        &text,
        &[
            "Device Identifier:",
            "Part of Whole:",
            "Disk / Partition UUID:",
        ],
    )
    .and_then(|value| normalize_macos_disk_device(&value));
    let model = find_line_value(&text, &["Device / Media Name:", "Media Name:"])
        .unwrap_or_else(|| "модель диска не определена".to_string());
    let kind = if find_line_value(&text, &["Solid State:"])
        .map(|value| value.eq_ignore_ascii_case("yes"))
        .unwrap_or(false)
    {
        if text.contains("Protocol:               NVMe") || text.contains("Protocol: NVMe") {
            "NVMe SSD".to_string()
        } else {
            "SSD".to_string()
        }
    } else {
        "тип диска не определён".to_string()
    };
    DiskTelemetry {
        device,
        model,
        kind,
        firmware: "данные недоступны".to_string(),
        source_label: "diskutil info".to_string(),
        busy_percent: None,
        read_mib_per_sec: None,
        write_mib_per_sec: None,
        temperature_celsius: None,
    }
}

fn detect_windows_disk_telemetry(repo_root: &Path) -> DiskTelemetry {
    let Some(drive_letter) = detect_windows_drive_letter(repo_root) else {
        return DiskTelemetry {
            device: None,
            model: "модель диска не определена".to_string(),
            kind: "тип диска не определён".to_string(),
            firmware: "данные недоступны".to_string(),
            source_label: "drive letter not detected".to_string(),
            busy_percent: None,
            read_mib_per_sec: None,
            write_mib_per_sec: None,
            temperature_celsius: None,
        };
    };
    let script = format!(
        "$drive='{drive_letter}'; \
        $ld = Get-CimInstance Win32_LogicalDisk -Filter \"DeviceID='$drive'\"; \
        $part = Get-Partition -DriveLetter $drive.TrimEnd(':') -ErrorAction SilentlyContinue | Select-Object -First 1; \
        $disk = $null; \
        if ($part) {{ $disk = $part | Get-Disk -ErrorAction SilentlyContinue | Select-Object -First 1; }}; \
        [pscustomobject]@{{ \
            DeviceId = $ld.DeviceID; \
            Model = $disk.Model; \
            FriendlyName = $disk.FriendlyName; \
            BusType = $disk.BusType; \
            MediaType = $disk.MediaType; \
            FirmwareVersion = $disk.FirmwareVersion \
        }} | ConvertTo-Json -Compress"
    );
    let Some(json) = run_powershell_json(&script) else {
        return DiskTelemetry {
            device: Some(drive_letter),
            model: "модель диска не определена".to_string(),
            kind: "тип диска не определён".to_string(),
            firmware: "данные недоступны".to_string(),
            source_label: "PowerShell disk inventory unavailable".to_string(),
            busy_percent: None,
            read_mib_per_sec: None,
            write_mib_per_sec: None,
            temperature_celsius: None,
        };
    };
    let bus_type = json["BusType"].as_str().unwrap_or_default();
    let media_type = json["MediaType"].as_str().unwrap_or_default();
    let kind = if bus_type.eq_ignore_ascii_case("NVMe") {
        "NVMe SSD".to_string()
    } else if media_type.eq_ignore_ascii_case("SSD") {
        "SSD".to_string()
    } else if media_type.eq_ignore_ascii_case("HDD") {
        "HDD".to_string()
    } else {
        "тип диска не определён".to_string()
    };
    DiskTelemetry {
        device: json["DeviceId"]
            .as_str()
            .map(|value| value.to_string())
            .or(Some(drive_letter)),
        model: json["Model"]
            .as_str()
            .or_else(|| json["FriendlyName"].as_str())
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "модель диска не определена".to_string()),
        kind,
        firmware: json["FirmwareVersion"]
            .as_str()
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "данные недоступны".to_string()),
        source_label: "PowerShell Win32_LogicalDisk + Get-Disk".to_string(),
        busy_percent: None,
        read_mib_per_sec: None,
        write_mib_per_sec: None,
        temperature_celsius: None,
    }
}

fn detect_accelerators(platform: HostPlatform) -> Vec<AcceleratorSummary> {
    let mut accelerators = Vec::new();
    merge_accelerators(&mut accelerators, detect_nvidia_accelerators());
    merge_accelerators(&mut accelerators, detect_rocm_accelerators());
    match platform {
        HostPlatform::Linux => {
            merge_accelerators(&mut accelerators, detect_linux_inventory_accelerators());
        }
        HostPlatform::Macos => {
            merge_accelerators(&mut accelerators, detect_macos_inventory_accelerators());
        }
        HostPlatform::Windows => {
            merge_accelerators(&mut accelerators, detect_windows_inventory_accelerators());
        }
        HostPlatform::Unknown => {}
    }
    accelerators.sort_by(|left, right| {
        accelerator_priority(right)
            .cmp(&accelerator_priority(left))
            .then_with(|| left.model.cmp(&right.model))
    });
    accelerators
}

fn read_linux_lscpu_numeric_field(prefix: &str) -> Option<f64> {
    let output = Command::new("lscpu").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with(prefix))?;
    let digits = line
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .find(|part| !part.is_empty())?;
    digits.parse::<f64>().ok()
}

fn detect_nvidia_accelerators() -> Vec<AcceleratorSummary> {
    let text = run_command_text(
        "nvidia-smi",
        [
            "--query-gpu=name,driver_version,utilization.gpu,temperature.gpu,memory.total,memory.used,power.draw,pci.bus_id",
            "--format=csv,noheader,nounits",
        ],
    );
    let Some(text) = text else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let parts = line.split(',').map(|part| part.trim()).collect::<Vec<_>>();
            if parts.len() < 8 {
                return None;
            }
            let kind = classify_accelerator_kind(Some("gpu"), parts[0]);
            Some(AcceleratorSummary {
                detected: true,
                kind_label: accelerator_kind_label(kind).to_string(),
                model: parts[0].to_string(),
                backend: "NVIDIA".to_string(),
                driver_version: Some(parts[1].to_string()).filter(|value| !value.is_empty()),
                utilization_percent: parts[2].parse::<f64>().ok(),
                temperature_celsius: parts[3].parse::<f64>().ok(),
                total_vram_gib: parts[4].parse::<f64>().ok().map(|value| value / 1024.0),
                used_vram_gib: parts[5].parse::<f64>().ok().map(|value| value / 1024.0),
                power_watts: extract_first_number(parts[6]),
                bus_label: normalize_pci_bus_label(parts[7]),
                source_label: "nvidia-smi".to_string(),
            })
        })
        .collect()
}

fn detect_rocm_accelerators() -> Vec<AcceleratorSummary> {
    let json = run_command_json(
        "rocm-smi",
        [
            "--showproductname",
            "--showuse",
            "--showtemp",
            "--showmemuse",
            "--showpower",
            "--json",
        ],
    );
    let Some(json) = json else {
        return Vec::new();
    };
    let model = find_first_string_by_key_contains(
        &json,
        &["card series", "device name", "product name", "card model"],
    );
    let Some(model) = model else {
        return Vec::new();
    };
    vec![AcceleratorSummary {
        detected: true,
        kind_label: accelerator_kind_label(classify_accelerator_kind(Some("gpu"), &model))
            .to_string(),
        model,
        backend: "AMD/ROCm".to_string(),
        driver_version: None,
        total_vram_gib: None,
        used_vram_gib: None,
        utilization_percent: find_first_f64_by_key_contains(&json, &["gpu use", "gpu_util"]),
        temperature_celsius: find_first_f64_by_key_contains(&json, &["temperature", "temp"]),
        power_watts: find_first_f64_by_key_contains(
            &json,
            &["average graphics package power", "power"],
        ),
        bus_label: None,
        source_label: "rocm-smi".to_string(),
    }]
}

fn detect_linux_inventory_accelerators() -> Vec<AcceleratorSummary> {
    let text = run_command_text_dynamic(
        "sh",
        [
            "-lc",
            "lspci | grep -Ei 'vga|3d|display|processing accelerators|co-processor'",
        ],
    );
    let Some(text) = text else {
        return Vec::new();
    };
    text.lines()
        .filter_map(parse_linux_accelerator_from_lspci_line)
        .collect()
}

fn detect_macos_inventory_accelerators() -> Vec<AcceleratorSummary> {
    let json = run_command_json("system_profiler", ["SPDisplaysDataType", "-json"]);
    let Some(json) = json else {
        return Vec::new();
    };
    json_items(&json)
        .into_iter()
        .filter_map(|item| {
            let model = find_first_string_by_key_contains(
                item,
                &["sppci_model", "spdisplays_ndrvs", "_name", "chipset model"],
            )?;
            let backend = find_first_string_by_key_contains(item, &["spdisplays_vendor", "vendor"])
                .unwrap_or_else(|| derive_gpu_backend_from_model(&model));
            let kind = classify_accelerator_kind(Some("gpu"), &model);
            Some(AcceleratorSummary {
                detected: true,
                kind_label: accelerator_kind_label(kind).to_string(),
                model,
                backend,
                driver_version: None,
                total_vram_gib: find_first_string_by_key_contains(
                    item,
                    &["spdisplays_vram", "spdisplays_vram_shared", "vram"],
                )
                .and_then(|value| parse_capacity_to_gib(&value)),
                used_vram_gib: None,
                utilization_percent: None,
                temperature_celsius: None,
                power_watts: None,
                bus_label: None,
                source_label: "system_profiler SPDisplaysDataType".to_string(),
            })
        })
        .collect()
}

fn detect_windows_inventory_accelerators() -> Vec<AcceleratorSummary> {
    let mut accelerators = Vec::new();
    if let Some(json) = run_powershell_json(
        "Get-CimInstance Win32_VideoController | Select-Object Name,AdapterRAM,DriverVersion,VideoProcessor | ConvertTo-Json -Compress",
    ) {
        for item in json_items(&json) {
            let Some(model) = item["Name"]
                .as_str()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let kind = classify_accelerator_kind(Some("gpu"), &model);
            accelerators.push(AcceleratorSummary {
                detected: true,
                kind_label: accelerator_kind_label(kind).to_string(),
                model: model.clone(),
                backend: item["VideoProcessor"]
                    .as_str()
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| derive_gpu_backend_from_model(&model)),
                driver_version: item["DriverVersion"]
                    .as_str()
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty()),
                total_vram_gib: item["AdapterRAM"].as_f64().map(bytes_to_gib_f64),
                used_vram_gib: None,
                utilization_percent: None,
                temperature_celsius: None,
                power_watts: None,
                bus_label: None,
                source_label: "PowerShell Win32_VideoController".to_string(),
            });
        }
    }
    if let Some(json) = run_powershell_json(
        "Get-CimInstance Win32_PnPEntity | Where-Object {$_.Name -match 'NPU|TPU|Neural|AI Boost|Gaudi|Coral|XDNA|ASIC|FPGA'} | Select-Object Name,Manufacturer | ConvertTo-Json -Compress",
    ) {
        for item in json_items(&json) {
            let Some(model) = item["Name"]
                .as_str()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            let kind = classify_accelerator_kind(Some("accelerator"), &model);
            accelerators.push(AcceleratorSummary {
                detected: true,
                kind_label: accelerator_kind_label(kind).to_string(),
                model,
                backend: item["Manufacturer"]
                    .as_str()
                    .map(|value| value.to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "данные недоступны".to_string()),
                driver_version: None,
                total_vram_gib: None,
                used_vram_gib: None,
                utilization_percent: None,
                temperature_celsius: None,
                power_watts: None,
                bus_label: None,
                source_label: "PowerShell Win32_PnPEntity".to_string(),
            });
        }
    }
    accelerators
}

fn parse_linux_accelerator_from_lspci_line(line: &str) -> Option<AcceleratorSummary> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let address = normalize_pci_bus_label(trimmed.split_whitespace().next()?);
    let parts = trimmed.splitn(3, ':').collect::<Vec<_>>();
    let class_hint = parts.get(1).map(|value| value.trim()).unwrap_or_default();
    let model = parts
        .get(2)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| trimmed.to_string());
    let kind = classify_accelerator_kind(Some(class_hint), &model);
    Some(AcceleratorSummary {
        detected: true,
        kind_label: accelerator_kind_label(kind).to_string(),
        model: model.clone(),
        backend: derive_gpu_backend_from_model(&model),
        driver_version: None,
        total_vram_gib: None,
        used_vram_gib: None,
        utilization_percent: None,
        temperature_celsius: None,
        power_watts: None,
        bus_label: address,
        source_label: "lspci".to_string(),
    })
}

fn merge_accelerators(target: &mut Vec<AcceleratorSummary>, incoming: Vec<AcceleratorSummary>) {
    for candidate in incoming {
        if let Some(existing) = target
            .iter_mut()
            .find(|existing| accelerators_match(existing, &candidate))
        {
            merge_accelerator(existing, candidate);
        } else {
            target.push(candidate);
        }
    }
}

fn merge_accelerator(existing: &mut AcceleratorSummary, candidate: AcceleratorSummary) {
    existing.detected |= candidate.detected;
    if existing.kind_label == accelerator_kind_label(AcceleratorKind::Unknown)
        && candidate.kind_label != accelerator_kind_label(AcceleratorKind::Unknown)
    {
        existing.kind_label = candidate.kind_label.clone();
    }
    if existing.backend == "данные недоступны" && candidate.backend != "данные недоступны"
    {
        existing.backend = candidate.backend.clone();
    }
    if existing.driver_version.is_none() {
        existing.driver_version = candidate.driver_version.clone();
    }
    if existing.total_vram_gib.is_none() {
        existing.total_vram_gib = candidate.total_vram_gib;
    }
    if existing.used_vram_gib.is_none() {
        existing.used_vram_gib = candidate.used_vram_gib;
    }
    if existing.utilization_percent.is_none() {
        existing.utilization_percent = candidate.utilization_percent;
    }
    if existing.temperature_celsius.is_none() {
        existing.temperature_celsius = candidate.temperature_celsius;
    }
    if existing.power_watts.is_none() {
        existing.power_watts = candidate.power_watts;
    }
    if existing.bus_label.is_none() {
        existing.bus_label = candidate.bus_label.clone();
    }
    if accelerator_live_score(&candidate) > accelerator_live_score(existing) {
        existing.model = candidate.model.clone();
        existing.source_label = candidate.source_label.clone();
    }
}

fn accelerators_match(left: &AcceleratorSummary, right: &AcceleratorSummary) -> bool {
    if let (Some(left_bus), Some(right_bus)) = (&left.bus_label, &right.bus_label)
        && left_bus.eq_ignore_ascii_case(right_bus)
    {
        return true;
    }
    let left_model = normalize_identity(&left.model);
    let right_model = normalize_identity(&right.model);
    if left_model.is_empty() || right_model.is_empty() {
        return false;
    }
    left_model == right_model
        || left_model.contains(&right_model)
        || right_model.contains(&left_model)
}

fn accelerator_priority(accelerator: &AcceleratorSummary) -> (u8, u8, u8) {
    let kind = classify_accelerator_kind(Some(&accelerator.kind_label), &accelerator.model);
    (
        accelerator_live_score(accelerator),
        accelerator_kind_priority(kind),
        u8::from(accelerator.total_vram_gib.is_some()),
    )
}

fn accelerator_live_score(accelerator: &AcceleratorSummary) -> u8 {
    [
        accelerator.used_vram_gib.is_some(),
        accelerator.utilization_percent.is_some(),
        accelerator.temperature_celsius.is_some(),
        accelerator.power_watts.is_some(),
        accelerator.driver_version.is_some(),
        accelerator.total_vram_gib.is_some(),
    ]
    .into_iter()
    .filter(|present| *present)
    .count() as u8
}

fn accelerator_kind_priority(kind: AcceleratorKind) -> u8 {
    match kind {
        AcceleratorKind::ExternalGpu => 9,
        AcceleratorKind::DiscreteGpu => 8,
        AcceleratorKind::Asic => 7,
        AcceleratorKind::Tpu => 6,
        AcceleratorKind::Npu => 5,
        AcceleratorKind::Fpga => 4,
        AcceleratorKind::IntegratedGpu => 3,
        AcceleratorKind::Accelerator => 2,
        AcceleratorKind::Unknown => 1,
    }
}

fn classify_accelerator_kind(kind_hint: Option<&str>, model: &str) -> AcceleratorKind {
    let hint = kind_hint.unwrap_or_default().to_ascii_lowercase();
    let normalized = format!("{hint} {}", model.to_ascii_lowercase());
    if normalized.contains("external gpu")
        || normalized.contains("egpu")
        || normalized.contains("thunderbolt")
    {
        AcceleratorKind::ExternalGpu
    } else if normalized.contains("tpu") || normalized.contains("tensor processing") {
        AcceleratorKind::Tpu
    } else if normalized.contains("npu")
        || normalized.contains("neural")
        || normalized.contains("ai boost")
        || normalized.contains("gaudi")
        || normalized.contains("habana")
        || normalized.contains("xdna")
    {
        AcceleratorKind::Npu
    } else if normalized.contains("asic") || normalized.contains("edge tpu") {
        AcceleratorKind::Asic
    } else if normalized.contains("fpga") {
        AcceleratorKind::Fpga
    } else if normalized.contains("processing accelerators")
        || normalized.contains("co-processor")
        || normalized.contains("accelerator")
    {
        AcceleratorKind::Accelerator
    } else if normalized.contains("vga")
        || normalized.contains("3d")
        || normalized.contains("display")
        || normalized.contains("graphics")
        || normalized.contains("geforce")
        || normalized.contains("rtx")
        || normalized.contains("radeon")
        || normalized.contains("iris")
        || normalized.contains("uhd")
        || normalized.contains("intel arc")
        || normalized.contains("metal")
    {
        if looks_integrated_graphics(&normalized) {
            AcceleratorKind::IntegratedGpu
        } else {
            AcceleratorKind::DiscreteGpu
        }
    } else {
        AcceleratorKind::Unknown
    }
}

fn looks_integrated_graphics(normalized: &str) -> bool {
    normalized.contains("integrated")
        || normalized.contains("igpu")
        || normalized.contains("intel uhd")
        || normalized.contains("intel hd")
        || normalized.contains("iris xe")
        || normalized.contains("radeon graphics")
        || normalized.contains("raphael")
        || normalized.contains("rembrandt")
        || normalized.contains("phoenix")
        || normalized.contains("cezanne")
        || normalized.contains("apple m")
        || normalized.contains("apple ")
}

fn accelerator_kind_label(kind: AcceleratorKind) -> &'static str {
    match kind {
        AcceleratorKind::ExternalGpu => "Внешняя GPU",
        AcceleratorKind::DiscreteGpu => "Дискретная GPU",
        AcceleratorKind::IntegratedGpu => "Встроенная графика",
        AcceleratorKind::Npu => "NPU / AI ускоритель",
        AcceleratorKind::Tpu => "TPU",
        AcceleratorKind::Asic => "ASIC",
        AcceleratorKind::Fpga => "FPGA",
        AcceleratorKind::Accelerator => "Ускоритель",
        AcceleratorKind::Unknown => "Тип не определён",
    }
}

fn normalize_identity(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_pci_bus_label(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let normalized = trimmed
        .trim_start_matches("00000000:")
        .trim_start_matches("0000:")
        .to_ascii_lowercase();
    Some(normalized)
}

fn json_items(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) => map
            .values()
            .find_map(|candidate| candidate.as_array().map(|items| items.iter().collect()))
            .unwrap_or_else(|| vec![value]),
        _ => vec![value],
    }
}

fn derive_gpu_backend_from_model(model: &str) -> String {
    let lowered = model.to_ascii_lowercase();
    if lowered.contains("nvidia") || lowered.contains("geforce") || lowered.contains("rtx") {
        "NVIDIA".to_string()
    } else if lowered.contains("amd") || lowered.contains("radeon") {
        "AMD".to_string()
    } else if lowered.contains("intel") {
        "Intel".to_string()
    } else if lowered.contains("apple") || lowered.contains("metal") {
        "Apple".to_string()
    } else {
        "данные недоступны".to_string()
    }
}

fn detect_linux_disk_device_for_path(path: &Path) -> Option<String> {
    let path_string = path.display().to_string();
    let output = Command::new("df")
        .arg("--output=source")
        .arg(path_string)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let source = text
        .lines()
        .skip(1)
        .find(|line| !line.trim().is_empty())?
        .trim();
    normalize_linux_block_device_name(source)
}

fn normalize_linux_block_device_name(source: &str) -> Option<String> {
    let source = source.strip_prefix("/dev/").unwrap_or(source).trim();
    if source.is_empty() {
        return None;
    }
    if let Some((base, suffix)) = source.rsplit_once('p')
        && suffix.chars().all(|ch| ch.is_ascii_digit())
    {
        return Some(base.to_string());
    }
    let trimmed = source.trim_end_matches(|ch: char| ch.is_ascii_digit());
    if trimmed.is_empty() {
        Some(source.to_string())
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_macos_disk_device(source: &str) -> Option<String> {
    let source = source.strip_prefix("/dev/").unwrap_or(source).trim();
    if source.is_empty() {
        return None;
    }
    let trimmed = source
        .trim_end_matches(|ch: char| ch.is_ascii_digit() || ch == 's')
        .trim();
    if trimmed.is_empty() {
        Some(source.to_string())
    } else {
        Some(trimmed.to_string())
    }
}

fn detect_windows_drive_letter(path: &Path) -> Option<String> {
    let rendered = path.as_os_str().to_string_lossy();
    let bytes = rendered.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        Some(rendered[..2].to_ascii_uppercase())
    } else {
        None
    }
}

fn read_linux_disk_model(device: &str) -> Option<String> {
    fs::read_to_string(format!("/sys/class/block/{device}/device/model"))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn read_linux_disk_firmware(device: &str) -> Option<String> {
    fs::read_to_string(format!("/sys/class/block/{device}/device/firmware_rev"))
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn detect_linux_disk_kind(device: &str) -> String {
    if device.starts_with("nvme") {
        return "NVMe SSD".to_string();
    }
    let rotational = fs::read_to_string(format!("/sys/class/block/{device}/queue/rotational"))
        .ok()
        .map(|text| text.trim() == "1");
    match rotational {
        Some(false) => "SSD".to_string(),
        Some(true) => "HDD".to_string(),
        None => "тип диска не определён".to_string(),
    }
}

fn read_linux_disk_temperature(device: &str) -> Option<f64> {
    let device_model = read_linux_disk_model(device);
    let device_serial = fs::read_to_string(format!("/sys/class/block/{device}/device/serial"))
        .ok()
        .map(|text| text.trim().to_string());
    for entry in fs::read_dir("/sys/class/hwmon").ok()?.flatten() {
        let path = entry.path();
        let name = fs::read_to_string(path.join("name")).ok()?;
        if name.trim() != "nvme" {
            continue;
        }
        let model_matches = device_model.as_deref().is_some_and(|expected| {
            fs::read_to_string(path.join("device/model"))
                .ok()
                .map(|actual| actual.trim() == expected)
                .unwrap_or(false)
        });
        let serial_matches = device_serial.as_deref().is_some_and(|expected| {
            fs::read_to_string(path.join("device/serial"))
                .ok()
                .map(|actual| actual.trim() == expected)
                .unwrap_or(false)
        });
        if !model_matches && !serial_matches {
            continue;
        }
        let raw = fs::read_to_string(path.join("temp1_input")).ok()?;
        let milli_celsius = raw.trim().parse::<f64>().ok()?;
        return Some(milli_celsius / 1000.0);
    }
    None
}

fn read_linux_disk_live_stats(device: &str) -> Option<DiskLiveStats> {
    let (read_sectors, write_sectors, io_millis) = read_linux_disk_counters(device)?;
    let captured_at_ms = now_epoch_ms();
    let cache = DISK_IO_CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().ok()?;
    let previous = guard.replace(DiskIoSample {
        device: device.to_string(),
        read_sectors,
        write_sectors,
        io_millis,
        captured_at_ms,
    });
    let previous = previous?;
    if previous.device != device {
        return None;
    }
    let delta_ms = captured_at_ms.saturating_sub(previous.captured_at_ms);
    if delta_ms == 0 {
        return None;
    }
    let delta_seconds = delta_ms as f64 / 1000.0;
    let read_bytes = read_sectors
        .saturating_sub(previous.read_sectors)
        .saturating_mul(512);
    let write_bytes = write_sectors
        .saturating_sub(previous.write_sectors)
        .saturating_mul(512);
    let busy_percent =
        ((io_millis.saturating_sub(previous.io_millis)) as f64 / delta_ms as f64) * 100.0;
    Some(DiskLiveStats {
        busy_percent: Some(busy_percent.min(100.0)),
        read_mib_per_sec: Some(read_bytes as f64 / 1024.0 / 1024.0 / delta_seconds),
        write_mib_per_sec: Some(write_bytes as f64 / 1024.0 / 1024.0 / delta_seconds),
    })
}

fn read_linux_disk_counters(device: &str) -> Option<(u64, u64, u64)> {
    let line = fs::read_to_string("/proc/diskstats")
        .ok()?
        .lines()
        .find(|line| line.split_whitespace().nth(2) == Some(device))?
        .to_string();
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 14 {
        return None;
    }
    let read_sectors = fields[5].parse::<u64>().ok()?;
    let write_sectors = fields[9].parse::<u64>().ok()?;
    let io_millis = fields[12].parse::<u64>().ok()?;
    Some((read_sectors, write_sectors, io_millis))
}

fn read_linux_hwmon_temperature(chip_name: &str, label: Option<&str>) -> Option<f64> {
    let hwmon_root = Path::new("/sys/class/hwmon");
    for entry in fs::read_dir(hwmon_root).ok()?.flatten() {
        let path = entry.path();
        let name = fs::read_to_string(path.join("name")).ok()?;
        if name.trim() != chip_name {
            continue;
        }
        for index in 1..=10 {
            let input_path = path.join(format!("temp{index}_input"));
            if !input_path.is_file() {
                continue;
            }
            let matches_label = match label {
                Some(expected) => fs::read_to_string(path.join(format!("temp{index}_label")))
                    .ok()
                    .map(|text| text.trim() == expected)
                    .unwrap_or(false),
                None => true,
            };
            if !matches_label {
                continue;
            }
            let raw = fs::read_to_string(&input_path).ok()?;
            let milli_celsius = raw.trim().parse::<f64>().ok()?;
            return Some(milli_celsius / 1000.0);
        }
    }
    None
}

fn read_macos_cpu_temperature() -> Option<f64> {
    for (provider, args) in [
        ("powermetrics", vec!["--samplers", "smc", "-n", "1"]),
        (
            "sudo",
            vec!["-n", "powermetrics", "--samplers", "smc", "-n", "1"],
        ),
    ] {
        let text = run_command_text(provider, args)?;
        if let Some(value) = find_line_value(&text, &["CPU die temperature:"])
            .and_then(|line| extract_first_number(&line))
        {
            return Some(value);
        }
    }
    run_command_text("istats", ["cpu", "temp"]).and_then(|text| extract_first_number(&text))
}

fn read_windows_cpu_temperature() -> Option<f64> {
    let json = run_powershell_json(
        "Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature | Select-Object -First 1 CurrentTemperature | ConvertTo-Json -Compress",
    )?;
    let raw = json["CurrentTemperature"].as_f64()?;
    Some((raw / 10.0) - 273.15)
}

fn run_command_text<I, S>(program: &str, args: I) -> Option<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stdout.is_empty() {
        return Some(stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        None
    } else {
        Some(stderr)
    }
}

fn run_command_text_dynamic<'a>(
    program: &str,
    args: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    run_command_text(program, args)
}

fn run_command_json<I, S>(program: &str, args: I) -> Option<Value>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let text = run_command_text(program, args)?;
    serde_json::from_str(&text).ok()
}

fn run_powershell_json(script: &str) -> Option<Value> {
    let args = ["-NoProfile", "-Command", script];
    run_command_json("powershell", args).or_else(|| run_command_json("powershell.exe", args))
}

fn find_line_value(text: &str, prefixes: &[&str]) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        for prefix in prefixes {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                return Some(rest.trim().to_string());
            }
        }
    }
    None
}

fn find_first_string_by_key_contains(value: &Value, needles: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                let normalized_key = key.to_ascii_lowercase();
                if needles
                    .iter()
                    .any(|needle| normalized_key.contains(&needle.to_ascii_lowercase()))
                {
                    if let Some(string) = value.as_str().map(|text| text.trim().to_string())
                        && !string.is_empty()
                    {
                        return Some(string);
                    }
                    if let Some(number) = value.as_f64() {
                        return Some(number.to_string());
                    }
                }
                if let Some(found) = find_first_string_by_key_contains(value, needles) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_first_string_by_key_contains(item, needles)),
        _ => None,
    }
}

fn find_first_f64_by_key_contains(value: &Value, needles: &[&str]) -> Option<f64> {
    find_first_string_by_key_contains(value, needles).and_then(|text| extract_first_number(&text))
}

fn extract_memory_speed(text: &str) -> Option<u64> {
    for line in text.lines() {
        let line = line.trim();
        if !(line.contains("Speed:")
            || line.contains("Configured Memory Speed:")
            || line.contains("clock:")
            || line.contains("MT/s"))
        {
            continue;
        }
        let digits = line
            .split(|ch: char| !ch.is_ascii_digit())
            .find(|part| !part.is_empty())?;
        if let Ok(value) = digits.parse::<u64>() {
            return Some(value);
        }
    }
    None
}

fn extract_memory_generation(text: &str) -> Option<String> {
    for candidate in [
        "DDR5", "LPDDR5", "DDR4", "LPDDR4X", "LPDDR4", "DDR3", "Unified",
    ] {
        if text.contains(candidate) {
            return Some(candidate.to_string());
        }
    }
    None
}

fn map_windows_smbios_memory_type(code: u64) -> Option<String> {
    match code {
        20 => Some("DDR".to_string()),
        21 => Some("DDR2".to_string()),
        24 => Some("DDR3".to_string()),
        26 => Some("DDR4".to_string()),
        27 => Some("LPDDR".to_string()),
        28 => Some("LPDDR2".to_string()),
        29 => Some("LPDDR3".to_string()),
        30 => Some("LPDDR4".to_string()),
        34 => Some("DDR5".to_string()),
        35 => Some("LPDDR5".to_string()),
        _ => None,
    }
}

fn map_windows_legacy_memory_type(code: u64) -> Option<String> {
    match code {
        20 => Some("DDR".to_string()),
        21 => Some("DDR2".to_string()),
        24 => Some("DDR3".to_string()),
        26 => Some("DDR4".to_string()),
        _ => None,
    }
}

fn normalize_speed_label(value: &str) -> String {
    if value.contains("MT/s") {
        value.to_string()
    } else if let Some(number) = extract_first_number(value) {
        format!("{number:.0} MT/s")
    } else {
        value.to_string()
    }
}

fn parse_capacity_to_gib(text: &str) -> Option<f64> {
    let normalized = text.trim().replace(',', ".");
    let value = extract_first_number(&normalized)?;
    let lowered = normalized.to_ascii_lowercase();
    if lowered.contains("tib") || lowered.contains("tb") {
        Some(value * 1024.0)
    } else if lowered.contains("gib") || lowered.contains("gb") {
        Some(value)
    } else if lowered.contains("mib") || lowered.contains("mb") {
        Some(value / 1024.0)
    } else if lowered.contains("kib") || lowered.contains("kb") {
        Some(value / 1024.0 / 1024.0)
    } else {
        None
    }
}

fn extract_first_number(text: &str) -> Option<f64> {
    let mut buffer = String::new();
    let mut started = false;
    for ch in text.chars() {
        if ch.is_ascii_digit() || (started && ch == '.') {
            buffer.push(ch);
            started = true;
        } else if started {
            break;
        }
    }
    if buffer.is_empty() {
        None
    } else {
        buffer.parse::<f64>().ok()
    }
}

fn disk_space_for_path(disks: &Disks, path: &Path) -> Option<(u64, u64)> {
    let canonical = path.canonicalize().ok()?;
    disks
        .iter()
        .filter(|disk| canonical.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().as_os_str().len())
        .map(|disk| (disk.total_space(), disk.available_space()))
}

fn percentage_from_parts(value: f64, total: f64) -> Option<f64> {
    if total <= 0.0 {
        None
    } else {
        Some((value / total) * 100.0)
    }
}

fn now_epoch_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn cached_machine_summary(repo_root: &Path) -> Option<MachineSummary> {
    let cache = MACHINE_SUMMARY_CACHE.get_or_init(|| Mutex::new(None));
    let guard = cache.lock().ok()?;
    let entry = guard.as_ref()?;
    let now_ms = now_epoch_ms();
    if should_reuse_cached_machine_summary(
        repo_root,
        &entry.repo_root,
        entry.captured_at_ms,
        now_ms,
    ) {
        Some(entry.summary.clone())
    } else {
        None
    }
}

fn store_machine_summary_cache(repo_root: &Path, summary: &MachineSummary) {
    let cache = MACHINE_SUMMARY_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    *guard = Some(CachedMachineSummary {
        repo_root: canonicalize_repo_root(repo_root),
        captured_at_ms: now_epoch_ms(),
        summary: summary.clone(),
    });
}

fn cached_memory_characteristics(platform: HostPlatform) -> Option<(String, String, String)> {
    let cache = MEMORY_CHARACTERISTICS_CACHE.get_or_init(|| Mutex::new(None));
    let guard = cache.lock().ok()?;
    let entry = guard.as_ref()?;
    let now_ms = now_epoch_ms();
    if should_reuse_cached_memory_characteristics(
        platform,
        entry.platform,
        entry.captured_at_ms,
        now_ms,
    ) {
        Some((
            entry.memory_type.clone(),
            entry.memory_speed_label.clone(),
            entry.provider.clone(),
        ))
    } else {
        None
    }
}

fn store_memory_characteristics_cache(
    platform: HostPlatform,
    memory_type: &str,
    memory_speed_label: &str,
    provider: &str,
) {
    let cache = MEMORY_CHARACTERISTICS_CACHE.get_or_init(|| Mutex::new(None));
    let Some(mut guard) = cache.lock().ok() else {
        return;
    };
    *guard = Some(CachedMemoryCharacteristics {
        platform,
        captured_at_ms: now_epoch_ms(),
        memory_type: memory_type.to_string(),
        memory_speed_label: memory_speed_label.to_string(),
        provider: provider.to_string(),
    });
}

fn should_reuse_cached_machine_summary(
    requested_repo_root: &Path,
    cached_repo_root: &Path,
    captured_at_ms: u64,
    now_ms: u64,
) -> bool {
    let requested_repo_root = canonicalize_repo_root(requested_repo_root);
    requested_repo_root == cached_repo_root
        && now_ms.saturating_sub(captured_at_ms) <= MACHINE_SUMMARY_CACHE_TTL_MS
}

fn should_reuse_cached_memory_characteristics(
    requested_platform: HostPlatform,
    cached_platform: HostPlatform,
    captured_at_ms: u64,
    now_ms: u64,
) -> bool {
    requested_platform == cached_platform
        && now_ms.saturating_sub(captured_at_ms) <= MEMORY_CHARACTERISTICS_CACHE_TTL_MS
}

fn canonicalize_repo_root(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn bytes_to_gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn bytes_to_gib_f64(bytes: f64) -> f64 {
    bytes / (1024.0 * 1024.0 * 1024.0)
}

#[cfg(test)]
mod tests {
    use super::{
        AcceleratorKind, HostPlatform, MACHINE_SUMMARY_CACHE_TTL_MS,
        MEMORY_CHARACTERISTICS_CACHE_TTL_MS, classify_accelerator_kind,
        derive_gpu_backend_from_model, extract_first_number, map_windows_smbios_memory_type,
        normalize_linux_block_device_name, normalize_pci_bus_label, parse_capacity_to_gib,
        should_reuse_cached_machine_summary, should_reuse_cached_memory_characteristics,
    };
    use std::path::Path;

    #[test]
    fn normalizes_linux_nvme_partition_name() {
        assert_eq!(
            normalize_linux_block_device_name("/dev/nvme0n1p2"),
            Some("nvme0n1".to_string())
        );
    }

    #[test]
    fn parses_gpu_backend_from_model() {
        assert_eq!(
            derive_gpu_backend_from_model("NVIDIA GeForce RTX 4070 Ti SUPER"),
            "NVIDIA"
        );
        assert_eq!(
            derive_gpu_backend_from_model("AMD Radeon RX 7900 XTX"),
            "AMD"
        );
    }

    #[test]
    fn parses_capacity_strings_to_gib() {
        assert_eq!(parse_capacity_to_gib("16384 MB"), Some(16.0));
        assert_eq!(parse_capacity_to_gib("1.5 TB"), Some(1536.0));
    }

    #[test]
    fn parses_first_number_from_text() {
        assert_eq!(extract_first_number("24.95 W"), Some(24.95));
    }

    #[test]
    fn maps_windows_memory_types() {
        assert_eq!(map_windows_smbios_memory_type(34), Some("DDR5".to_string()));
        assert_eq!(map_windows_smbios_memory_type(26), Some("DDR4".to_string()));
    }

    #[test]
    fn normalizes_pci_bus_label() {
        assert_eq!(
            normalize_pci_bus_label("00000000:01:00.0"),
            Some("01:00.0".to_string())
        );
    }

    #[test]
    fn classifies_integrated_gpu_from_model() {
        assert_eq!(
            classify_accelerator_kind(Some("gpu"), "Intel UHD Graphics 770"),
            AcceleratorKind::IntegratedGpu
        );
    }

    #[test]
    fn cached_machine_summary_reuse_requires_same_repo_root_and_fresh_age() {
        let repo_root = Path::new("/tmp/amai");
        let other_repo_root = Path::new("/tmp/other");
        let now_ms = 1_000_000;
        assert!(should_reuse_cached_machine_summary(
            repo_root,
            repo_root,
            now_ms - MACHINE_SUMMARY_CACHE_TTL_MS,
            now_ms,
        ));
        assert!(!should_reuse_cached_machine_summary(
            other_repo_root,
            repo_root,
            now_ms - MACHINE_SUMMARY_CACHE_TTL_MS,
            now_ms,
        ));
        assert!(!should_reuse_cached_machine_summary(
            repo_root,
            repo_root,
            now_ms - MACHINE_SUMMARY_CACHE_TTL_MS - 1,
            now_ms,
        ));
    }

    #[test]
    fn cached_memory_characteristics_reuse_requires_same_platform_and_fresh_age() {
        let now_ms = MEMORY_CHARACTERISTICS_CACHE_TTL_MS + 1_000_000;
        assert!(should_reuse_cached_memory_characteristics(
            HostPlatform::Linux,
            HostPlatform::Linux,
            now_ms - MEMORY_CHARACTERISTICS_CACHE_TTL_MS,
            now_ms,
        ));
        assert!(!should_reuse_cached_memory_characteristics(
            HostPlatform::Macos,
            HostPlatform::Linux,
            now_ms - MEMORY_CHARACTERISTICS_CACHE_TTL_MS,
            now_ms,
        ));
        assert!(!should_reuse_cached_memory_characteristics(
            HostPlatform::Linux,
            HostPlatform::Linux,
            now_ms - MEMORY_CHARACTERISTICS_CACHE_TTL_MS - 1,
            now_ms,
        ));
    }
}
