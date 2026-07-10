use super::*;
use crate::cli::{
    ObserveBenchmarkContaminationPreflightArgs, VerifyBenchmarkContaminationPreflightArgs,
};
use std::collections::{BTreeSet, HashMap};
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command as ProcessCommand;
use tokio::time::{sleep, timeout};

const DEFAULT_OBSERVE_BIND: &str = "0.0.0.0:9464";
const PROCESS_SAMPLES_FIXTURE_FILE_ENV: &str = "AMAI_BENCHMARK_PROCESS_SAMPLES_FIXTURE_FILE";
const CLIENT_STATUS_FIXTURE_FILE_ENV: &str = "AMAI_BENCHMARK_CLIENT_STATUS_FIXTURE_FILE";
const POLICY_FILE_ENV: &str = "AMAI_BENCHMARK_CONTAMINATION_POLICY";
const PROOF_MODE_ENV: &str = "AMAI_BENCHMARK_PROOF_MODE";
const HEAVY_CPU_THRESHOLD_ENV: &str = "AMI_BENCHMARK_HEAVY_CPU_THRESHOLD";
const CPU_SAMPLE_WINDOW_ENV: &str = "AMI_BENCHMARK_CPU_SAMPLE_WINDOW_MS";
const MAX_HEAVY_PROCESS_PREVIEW: usize = 8;
const CMDLINE_REDACTION_VERSION: &str = "secret_like_tokens_redacted_v1";
const STRICT_HEAVY_REASON: &str = "heavy external process detected in strict benchmark mode";
const MULTIPLE_CANONICAL_OBSERVE_REASON: &str = "multiple canonical observe servers are running";
const NON_CANONICAL_OBSERVE_REASON: &str = "non-canonical observe server instance detected";
const PARALLEL_VERIFY_REASON: &str = "parallel benchmark or verify lane detected";
const BENCHMARK_CONTAMINATION_POLICY_PATH: &str = "config/benchmark_contamination_policy.toml";
const MAX_CPU_SAMPLE_INTERVAL_MS: u64 = 250;
const MIN_CPU_SAMPLE_INTERVAL_MS: u64 = 100;
const MAX_CPU_SAMPLE_WINDOWS: u64 = 10;

#[derive(Debug, Clone, serde::Deserialize)]
struct BenchmarkContaminationPolicy {
    policy_version: String,
    heavy_cpu_threshold_percent: f64,
    cpu_sample_window_ms: u64,
    parallel_verify_lanes: Vec<String>,
    #[serde(default)]
    allowed_heavy_process_names: Vec<String>,
    #[serde(default)]
    allowed_heavy_processes: Vec<AllowedHeavyProcessRule>,
    #[serde(default)]
    client_process_reconciliation: Vec<ClientProcessReconciliationRule>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct AllowedHeavyProcessRule {
    code: String,
    #[serde(default)]
    process_comm_names: Vec<String>,
    #[serde(default)]
    exe_path_contains: Vec<String>,
    #[serde(default)]
    cmdline_contains: Vec<String>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ClientProcessReconciliationRule {
    code: String,
    status_probe_code: String,
    status_timeout_ms: u64,
    #[serde(default)]
    process_comm_names: Vec<String>,
    #[serde(default)]
    exe_path_contains: Vec<String>,
    #[serde(default)]
    cmdline_contains: Vec<String>,
}

impl BenchmarkContaminationPolicy {
    fn validate(&self) -> Result<()> {
        if self.policy_version.trim().is_empty() {
            bail!("benchmark contamination policy_version must not be empty");
        }
        if self.heavy_cpu_threshold_percent <= 0.0 {
            bail!("benchmark contamination heavy_cpu_threshold_percent must be > 0");
        }
        if self.cpu_sample_window_ms == 0 {
            bail!("benchmark contamination cpu_sample_window_ms must be > 0");
        }
        if self.parallel_verify_lanes.is_empty() {
            bail!("benchmark contamination policy must define parallel_verify_lanes");
        }
        if !self.allowed_heavy_process_names.is_empty() {
            bail!(
                "benchmark contamination policy allowed_heavy_process_names is legacy broad matching; use allowed_heavy_processes with cmdline matchers"
            );
        }
        for rule in &self.allowed_heavy_processes {
            rule.validate()?;
        }
        for rule in &self.client_process_reconciliation {
            if rule.code.trim().is_empty() {
                bail!("client_process_reconciliation code must not be empty");
            }
            if rule.status_probe_code.trim().is_empty() {
                bail!(
                    "client_process_reconciliation {} status_probe_code must not be empty",
                    rule.code
                );
            }
            client_status_probe(rule)?;
            if rule.status_timeout_ms == 0 {
                bail!(
                    "client_process_reconciliation {} status_timeout_ms must be > 0",
                    rule.code
                );
            }
            if rule.process_comm_names.is_empty() {
                bail!(
                    "client_process_reconciliation {} must define process_comm_names",
                    rule.code
                );
            }
            if rule.exe_path_contains.is_empty() {
                bail!(
                    "client_process_reconciliation {} must define exe_path_contains",
                    rule.code
                );
            }
            if rule.cmdline_contains.is_empty() {
                bail!(
                    "client_process_reconciliation {} must define cmdline_contains",
                    rule.code
                );
            }
        }
        Ok(())
    }
}

impl AllowedHeavyProcessRule {
    fn validate(&self) -> Result<()> {
        if self.code.trim().is_empty() {
            bail!("allowed_heavy_processes code must not be empty");
        }
        if self.process_comm_names.is_empty() {
            bail!(
                "allowed_heavy_processes {} must define process_comm_names",
                self.code
            );
        }
        if self.cmdline_contains.is_empty() {
            bail!(
                "allowed_heavy_processes {} must define cmdline_contains",
                self.code
            );
        }
        if self.exe_path_contains.is_empty() {
            bail!(
                "allowed_heavy_processes {} must define exe_path_contains",
                self.code
            );
        }
        Ok(())
    }

    fn matches_process(&self, sample: &ProcessCpuSample) -> bool {
        self.process_comm_names
            .iter()
            .any(|name| sample.comm == *name)
            && path_or_empty_matches(sample.exe_path.as_deref(), &self.exe_path_contains)
            && cmdline_or_empty_matches(&sample.cmdline, &self.cmdline_contains)
    }
}

impl ClientProcessReconciliationRule {
    fn matches_process(&self, sample: &ProcessCpuSample) -> bool {
        self.process_comm_names
            .iter()
            .any(|name| sample.comm == *name)
            && path_or_empty_matches(sample.exe_path.as_deref(), &self.exe_path_contains)
            && self
                .cmdline_contains
                .iter()
                .any(|needle| sample.cmdline.contains(needle))
    }
}

fn path_or_empty_matches(path: Option<&str>, needles: &[String]) -> bool {
    if needles.is_empty() {
        return true;
    }
    let Some(path) = path else {
        return false;
    };
    needles.iter().any(|needle| path.contains(needle))
}

fn cmdline_or_empty_matches(cmdline: &str, needles: &[String]) -> bool {
    needles.is_empty() || needles.iter().any(|needle| cmdline.contains(needle))
}

#[derive(Debug, Clone)]
struct ProcessSample {
    pid: u32,
    comm: String,
    exe_path: Option<String>,
    cmdline: String,
    cpu_ticks: u64,
}

#[derive(Debug, Clone)]
struct ProcessCpuSample {
    pid: u32,
    comm: String,
    exe_path: Option<String>,
    cmdline: String,
    current_cpu_percent: f64,
}

#[derive(Debug, Clone)]
struct ClientStatusCpuSample {
    pid: u32,
    cpu_percent: f64,
    label: String,
    client_code: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ProcessCpuSampleFixture {
    pid: u32,
    comm: String,
    #[serde(default)]
    exe_path: Option<String>,
    cmdline: String,
    current_cpu_percent: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HeavyProcessVerdict {
    AdvisoryAllowedInfrastructure,
    AdvisoryClientLowCpu,
    BlockingExternal,
    BlockingClientStatusUnavailable,
    BlockingClientPidMissing,
    BlockingClientHighCpu,
}

#[derive(Debug, Clone)]
struct HeavyProcessAssessment {
    sample: ProcessCpuSample,
    verdict: HeavyProcessVerdict,
    client_code: Option<String>,
    client_status: Option<ClientStatusCpuSample>,
}

#[derive(Debug, Clone)]
struct HeavyProcessReportSets {
    preview: Vec<HeavyProcessAssessment>,
    blocking_all: Vec<HeavyProcessAssessment>,
    blocking_preview: Vec<HeavyProcessAssessment>,
}

#[derive(Debug, Clone, Copy)]
struct BenchmarkContaminationRunOptions {
    strict_heavy: bool,
    allow_fixtures: bool,
    allow_test_overrides: bool,
}

impl BenchmarkContaminationRunOptions {
    fn live(strict_heavy: bool) -> Self {
        Self {
            strict_heavy,
            allow_fixtures: false,
            allow_test_overrides: false,
        }
    }

    fn proof(strict_heavy: bool, allow_fixtures: bool, allow_test_overrides: bool) -> Self {
        Self {
            strict_heavy,
            allow_fixtures,
            allow_test_overrides,
        }
    }
}

struct ScopedEnvVar {
    key: &'static str,
    previous: Option<String>,
}

impl ScopedEnvVar {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = env::var(key).ok();
        unsafe {
            env::set_var(key, value);
        }
        Self { key, previous }
    }

    fn unset(key: &'static str) -> Self {
        let previous = env::var(key).ok();
        unsafe {
            env::remove_var(key);
        }
        Self { key, previous }
    }
}

impl Drop for ScopedEnvVar {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous {
            unsafe {
                env::set_var(self.key, previous);
            }
        } else {
            unsafe {
                env::remove_var(self.key);
            }
        }
    }
}

pub async fn print_benchmark_contamination_preflight(
    args: &ObserveBenchmarkContaminationPreflightArgs,
) -> Result<()> {
    let payload = collect_benchmark_contamination_preflight(
        BenchmarkContaminationRunOptions::live(args.strict_heavy),
    )
    .await?;
    if args.json {
        println!("{}", serde_json::to_string(&payload)?);
    } else {
        print_benchmark_contamination_text(&payload);
    }
    let status = payload["status"].as_str().unwrap_or("fail");
    if status != "pass" {
        let reasons = payload["reasons"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_else(|| "unknown benchmark contamination failure".to_string());
        return Err(anyhow!(
            "benchmark contamination preflight failed: {reasons}"
        ));
    }
    Ok(())
}

pub(crate) async fn collect_verify_benchmark_contamination_attestation(
    strict_heavy: bool,
) -> Result<Value> {
    collect_benchmark_contamination_preflight(BenchmarkContaminationRunOptions::live(strict_heavy))
        .await
}

#[derive(Debug, serde::Serialize)]
struct BenchmarkContaminationProofCase {
    name: &'static str,
    status: &'static str,
}

struct ProofTempDir {
    path: PathBuf,
}

impl ProofTempDir {
    fn new() -> Result<Self> {
        let path = env::temp_dir().join(format!(
            "amai-benchmark-contamination-proof-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&path)
            .with_context(|| format!("failed to create proof temp dir {}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }
}

impl Drop for ProofTempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug)]
struct PreflightCliOutput {
    success: bool,
    stdout: String,
    stderr: String,
    payload: Option<Value>,
}

pub async fn print_benchmark_contamination_preflight_proof(
    args: &VerifyBenchmarkContaminationPreflightArgs,
) -> Result<()> {
    let cases = run_benchmark_contamination_preflight_proof().await?;
    let payload = json!({
        "proof": "benchmark_contamination_preflight",
        "proof_version": "benchmark-contamination-preflight-proof-v1",
        "runtime_owner": "amai verify benchmark-contamination-preflight",
        "shell_role": "launcher_only",
        "cases": cases,
        "status": "pass",
    });
    if args.json {
        println!("{}", serde_json::to_string(&payload)?);
    } else {
        println!("proof_benchmark_contamination_preflight: ok");
    }
    Ok(())
}

async fn run_benchmark_contamination_preflight_proof()
-> Result<Vec<BenchmarkContaminationProofCase>> {
    let temp = ProofTempDir::new()?;
    let binary = env::current_exe().context("failed to resolve current Amai executable")?;
    let repo_root = crate::config::discover_repo_root(None)?;
    let clean_processes_json = temp.path("clean_processes.json");
    let debug_amai = repo_root.join("target/debug/amai").display().to_string();
    let release_amai = repo_root.join("target/release/amai").display().to_string();

    let fixture_processes_json = temp.path("vscode_processes.json");
    let fixture_observe_processes_json = temp.path("observe_processes.json");
    let fixture_canonical_observe_processes_json = temp.path("canonical_observe_processes.json");
    let fixture_parallel_mcp_json = temp.path("parallel_mcp_processes.json");
    let fixture_parallel_memory_json = temp.path("parallel_memory_processes.json");
    let fixture_heavy_mcp_serve_json = temp.path("heavy_mcp_serve_processes.json");
    let fixture_status_low_txt = temp.path("vscode_status_low.txt");
    let fixture_status_high_txt = temp.path("vscode_status_high.txt");
    let fixture_status_missing_pid_txt = temp.path("vscode_status_missing_pid.txt");
    let proof_observe_bind = "127.0.0.1:9465".to_string();

    write_text(&clean_processes_json, "[]\n")?;

    write_text(
        &fixture_processes_json,
        r#"[
  {
    "pid": 2244026,
    "comm": "code",
    "exe_path": "/usr/share/code/code",
    "cmdline": "/proc/self/exe --type=utility --utility-sub-type=node.mojom.NodeService --user-data-dir=/workspace/user/.config/Code --api-key sk-fixture-secret",
    "current_cpu_percent": 52.5
  }
]
"#,
    )?;
    write_text(
        &fixture_observe_processes_json,
        &format!(
            r#"[
  {{
    "pid": 3001,
    "comm": "amai",
    "exe_path": "{debug_amai}",
    "cmdline": "{debug_amai} observe serve --bind 127.0.0.1:9465",
    "current_cpu_percent": 0.1
  }}
]
"#
        ),
    )?;
    write_text(
        &fixture_canonical_observe_processes_json,
        &format!(
            r#"[
  {{
    "pid": 3010,
    "comm": "amai",
    "exe_path": "{release_amai}",
    "cmdline": "{release_amai} observe serve --bind 0.0.0.0:9464",
    "current_cpu_percent": 0.1
  }},
  {{
    "pid": 3011,
    "comm": "amai",
    "exe_path": "{release_amai}",
    "cmdline": "{release_amai} observe serve --bind 0.0.0.0:9464",
    "current_cpu_percent": 0.1
  }}
]
"#
        ),
    )?;
    write_text(
        &fixture_parallel_mcp_json,
        &format!(
            r#"[
  {{
    "pid": 3002,
    "comm": "amai",
    "exe_path": "{release_amai}",
    "cmdline": "{release_amai} verify mcp-matrix --matrix live_mcpbench_local",
    "current_cpu_percent": 0.1
  }}
]
"#
        ),
    )?;
    write_text(
        &fixture_parallel_memory_json,
        &format!(
            r#"[
  {{
    "pid": 3003,
    "comm": "amai",
    "exe_path": "{release_amai}",
    "cmdline": "{release_amai} verify memory-matrix --matrix letta_memory_local",
    "current_cpu_percent": 0.1
  }}
]
"#
        ),
    )?;
    write_text(
        &fixture_heavy_mcp_serve_json,
        &format!(
            r#"[
  {{
    "pid": 3004,
    "comm": "amai",
    "exe_path": "{release_amai}",
    "cmdline": "{release_amai} mcp serve",
    "current_cpu_percent": 63.0
  }}
]
"#
        ),
    )?;
    write_text(
        &fixture_status_low_txt,
        "CPU %\tMem MB\t   PID\tProcess\n    2\t539727720531\t2244026\textension-host [10] --api-key sk-status-secret\n\nWorkspace Stats:\n",
    )?;
    write_text(
        &fixture_status_high_txt,
        "CPU %\tMem MB\t   PID\tProcess\n   58\t539727720531\t2244026\textension-host [10]\n\nWorkspace Stats:\n",
    )?;
    write_text(
        &fixture_status_missing_pid_txt,
        "CPU %\tMem MB\t   PID\tProcess\n    2\t539727720531\t999999\textension-host [10]\n\nWorkspace Stats:\n",
    )?;

    let mut cases = Vec::new();

    let clean = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(false, true, false),
        &[(
            PROCESS_SAMPLES_FIXTURE_FILE_ENV,
            clean_processes_json.display().to_string(),
        )],
    )
    .await?;
    expect_success_json(&clean, "clean preflight")?;
    require_json_bool(&clean, &["strict_heavy"], false, "clean preflight")?;
    require_json_bool(&clean, &["fixtures_allowed"], true, "clean preflight")?;
    require_json_bool(
        &clean,
        &["test_overrides_allowed"],
        false,
        "clean preflight",
    )?;
    require_json_string(
        &clean,
        &["cmdline_redaction"],
        CMDLINE_REDACTION_VERSION,
        "clean preflight",
    )?;
    cases.push(pass_case("clean_fixture_preflight"));

    let fixture_without_allow = run_preflight_cli(
        &binary,
        &["--json", "--strict-heavy"],
        &[
            (
                PROCESS_SAMPLES_FIXTURE_FILE_ENV,
                fixture_processes_json.display().to_string(),
            ),
            (
                CLIENT_STATUS_FIXTURE_FILE_ENV,
                fixture_status_low_txt.display().to_string(),
            ),
        ],
    )
    .await?;
    expect_failure_contains(
        &fixture_without_allow,
        "--allow-fixtures",
        "fixture env without allow flag",
    )?;
    cases.push(pass_case("fixture_env_requires_allow_flag"));

    let fixture_low = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(true, true, false),
        &[
            (
                PROCESS_SAMPLES_FIXTURE_FILE_ENV,
                fixture_processes_json.display().to_string(),
            ),
            (
                CLIENT_STATUS_FIXTURE_FILE_ENV,
                fixture_status_low_txt.display().to_string(),
            ),
        ],
    )
    .await?;
    expect_success_json(&fixture_low, "low CPU client fixture")?;
    require_json_array_len(
        &fixture_low,
        &["strict_heavy_processes"],
        0,
        "low CPU client fixture",
    )?;
    require_json_array_len(
        &fixture_low,
        &["advisory_client_processes"],
        1,
        "low CPU client fixture",
    )?;
    require_json_string(
        &fixture_low,
        &["heavy_process_assessments", "0", "verdict"],
        "advisory_client_low_cpu",
        "low CPU client fixture",
    )?;
    require_json_string(
        &fixture_low,
        &["heavy_process_assessments", "0", "client_code"],
        "vscode",
        "low CPU client fixture",
    )?;
    require_json_string(
        &fixture_low,
        &["heavy_process_assessments", "0", "cmdline_redaction"],
        CMDLINE_REDACTION_VERSION,
        "low CPU client fixture",
    )?;
    require_not_contains(
        &fixture_low.stdout,
        "sk-fixture-secret",
        "process cmdline redaction",
    )?;
    require_not_contains(
        &fixture_low.stdout,
        "sk-status-secret",
        "client status label redaction",
    )?;
    cases.push(pass_case("client_low_cpu_downgraded_and_redacted"));

    let vscode_high = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(true, true, false),
        &[
            (
                PROCESS_SAMPLES_FIXTURE_FILE_ENV,
                fixture_processes_json.display().to_string(),
            ),
            (
                CLIENT_STATUS_FIXTURE_FILE_ENV,
                fixture_status_high_txt.display().to_string(),
            ),
        ],
    )
    .await?;
    expect_failure_json(&vscode_high, "high CPU client fixture")?;
    require_json_array_len(
        &vscode_high,
        &["strict_heavy_processes"],
        1,
        "high CPU client fixture",
    )?;
    require_json_string(
        &vscode_high,
        &["heavy_process_assessments", "0", "verdict"],
        "blocking_client_high_cpu",
        "high CPU client fixture",
    )?;
    require_json_array_contains_string(
        &vscode_high,
        &["reasons"],
        STRICT_HEAVY_REASON,
        "high CPU client fixture",
    )?;
    cases.push(pass_case("client_high_cpu_blocks_strict_heavy"));

    let threshold_without_allow = run_preflight_cli(
        &binary,
        &["--json", "--strict-heavy"],
        &[(HEAVY_CPU_THRESHOLD_ENV, "0".to_string())],
    )
    .await?;
    expect_failure_contains(
        &threshold_without_allow,
        "--allow-test-overrides",
        "threshold override without allow flag",
    )?;
    cases.push(pass_case("test_override_requires_allow_flag"));

    let observe_bind_without_allow = run_preflight_cli(
        &binary,
        &["--json", "--strict-heavy"],
        &[("AMI_OBSERVE_BIND", proof_observe_bind.clone())],
    )
    .await?;
    expect_failure_contains(
        &observe_bind_without_allow,
        "--allow-test-overrides",
        "observe bind override without allow flag",
    )?;
    cases.push(pass_case("observe_bind_override_requires_allow_flag"));

    let strict_fail = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(true, false, true),
        &[(HEAVY_CPU_THRESHOLD_ENV, "0".to_string())],
    )
    .await?;
    expect_failure_json(&strict_fail, "strict heavy live threshold fixture")?;
    require_json_bool(
        &strict_fail,
        &["test_overrides_allowed"],
        true,
        "strict heavy live threshold fixture",
    )?;
    require_json_array_contains_string(
        &strict_fail,
        &["reasons"],
        STRICT_HEAVY_REASON,
        "strict heavy live threshold fixture",
    )?;
    cases.push(pass_case("strict_heavy_blocks_live_heavy_processes"));

    let heavy_mcp_serve = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(true, true, false),
        &[(
            PROCESS_SAMPLES_FIXTURE_FILE_ENV,
            fixture_heavy_mcp_serve_json.display().to_string(),
        )],
    )
    .await?;
    expect_failure_json(&heavy_mcp_serve, "heavy mcp serve fixture")?;
    require_json_string(
        &heavy_mcp_serve,
        &["heavy_process_assessments", "0", "verdict"],
        "blocking_external",
        "heavy mcp serve fixture",
    )?;
    require_json_array_len(
        &heavy_mcp_serve,
        &["strict_heavy_processes"],
        1,
        "heavy mcp serve fixture",
    )?;
    require_json_array_contains_string(
        &heavy_mcp_serve,
        &["reasons"],
        STRICT_HEAVY_REASON,
        "heavy mcp serve fixture",
    )?;
    cases.push(pass_case("default_policy_blocks_heavy_mcp_serve"));

    let observe_bind_override = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(false, false, true),
        &[("AMI_OBSERVE_BIND", proof_observe_bind.clone())],
    )
    .await?;
    expect_success_json(&observe_bind_override, "observe bind proof override")?;
    require_json_string(
        &observe_bind_override,
        &["canonical_bind"],
        &proof_observe_bind,
        "observe bind proof override",
    )?;
    require_json_bool(
        &observe_bind_override,
        &["runtime_overrides", "observe_bind"],
        true,
        "observe bind proof override",
    )?;
    cases.push(pass_case("observe_bind_override_allowed_in_proof_only"));

    let observe_fail = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(false, true, false),
        &[(
            PROCESS_SAMPLES_FIXTURE_FILE_ENV,
            fixture_observe_processes_json.display().to_string(),
        )],
    )
    .await?;
    expect_failure_json(&observe_fail, "non canonical observe fixture")?;
    require_json_array_min_len(
        &observe_fail,
        &["blocking_observe_instances"],
        1,
        "non canonical observe fixture",
    )?;
    cases.push(pass_case("non_canonical_observe_blocks"));

    let canonical_duplicate = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(false, true, false),
        &[(
            PROCESS_SAMPLES_FIXTURE_FILE_ENV,
            fixture_canonical_observe_processes_json
                .display()
                .to_string(),
        )],
    )
    .await?;
    expect_failure_json(&canonical_duplicate, "multiple canonical observe fixture")?;
    require_json_array_contains_string(
        &canonical_duplicate,
        &["reasons"],
        MULTIPLE_CANONICAL_OBSERVE_REASON,
        "multiple canonical observe fixture",
    )?;
    cases.push(pass_case("multiple_canonical_observe_blocks"));

    let parallel_mcp = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(false, true, false),
        &[(
            PROCESS_SAMPLES_FIXTURE_FILE_ENV,
            fixture_parallel_mcp_json.display().to_string(),
        )],
    )
    .await?;
    expect_failure_json(&parallel_mcp, "parallel mcp matrix fixture")?;
    require_json_array_min_len(
        &parallel_mcp,
        &["blocking_benchmark_instances"],
        1,
        "parallel mcp matrix fixture",
    )?;
    require_json_array_contains_string(
        &parallel_mcp,
        &["reasons"],
        PARALLEL_VERIFY_REASON,
        "parallel mcp matrix fixture",
    )?;
    cases.push(pass_case("parallel_mcp_matrix_blocks"));

    let parallel_memory = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(false, true, false),
        &[(
            PROCESS_SAMPLES_FIXTURE_FILE_ENV,
            fixture_parallel_memory_json.display().to_string(),
        )],
    )
    .await?;
    expect_failure_json(&parallel_memory, "parallel memory matrix fixture")?;
    require_json_array_min_len(
        &parallel_memory,
        &["blocking_benchmark_instances"],
        1,
        "parallel memory matrix fixture",
    )?;
    require_json_array_contains_string(
        &parallel_memory,
        &["reasons"],
        PARALLEL_VERIFY_REASON,
        "parallel memory matrix fixture",
    )?;
    cases.push(pass_case("parallel_memory_matrix_blocks"));

    let missing_pid = run_preflight_internal(
        BenchmarkContaminationRunOptions::proof(true, true, false),
        &[
            (
                PROCESS_SAMPLES_FIXTURE_FILE_ENV,
                fixture_processes_json.display().to_string(),
            ),
            (
                CLIENT_STATUS_FIXTURE_FILE_ENV,
                fixture_status_missing_pid_txt.display().to_string(),
            ),
        ],
    )
    .await?;
    expect_failure_json(&missing_pid, "client status missing pid fixture")?;
    require_json_string(
        &missing_pid,
        &["heavy_process_assessments", "0", "verdict"],
        "blocking_client_pid_missing",
        "client status missing pid fixture",
    )?;
    require_json_array_contains_string(
        &missing_pid,
        &["reasons"],
        STRICT_HEAVY_REASON,
        "client status missing pid fixture",
    )?;
    cases.push(pass_case("client_status_pid_missing_blocks_strict_heavy"));

    Ok(cases)
}

fn pass_case(name: &'static str) -> BenchmarkContaminationProofCase {
    BenchmarkContaminationProofCase {
        name,
        status: "pass",
    }
}

fn write_text(path: &std::path::Path, content: &str) -> Result<()> {
    std::fs::write(path, content).with_context(|| format!("failed to write {}", path.display()))
}

async fn run_preflight_cli(
    binary: &std::path::Path,
    args: &[&str],
    envs: &[(&str, String)],
) -> Result<PreflightCliOutput> {
    let mut command = ProcessCommand::new(binary);
    for key in [
        PROCESS_SAMPLES_FIXTURE_FILE_ENV,
        CLIENT_STATUS_FIXTURE_FILE_ENV,
        POLICY_FILE_ENV,
        PROOF_MODE_ENV,
        HEAVY_CPU_THRESHOLD_ENV,
        CPU_SAMPLE_WINDOW_ENV,
        "AMI_BENCHMARK_STRICT_HEAVY",
    ] {
        command.env_remove(key);
    }
    command.args(["observe", "benchmark-contamination-preflight"]);
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command
        .output()
        .await
        .with_context(|| format!("failed to run {}", binary.display()))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let payload = serde_json::from_str::<Value>(stdout.trim()).ok();
    Ok(PreflightCliOutput {
        success: output.status.success(),
        stdout,
        stderr,
        payload,
    })
}

async fn run_preflight_internal(
    options: BenchmarkContaminationRunOptions,
    envs: &[(&'static str, String)],
) -> Result<PreflightCliOutput> {
    let mut scoped_env = vec![
        ScopedEnvVar::unset(PROCESS_SAMPLES_FIXTURE_FILE_ENV),
        ScopedEnvVar::unset(CLIENT_STATUS_FIXTURE_FILE_ENV),
        ScopedEnvVar::unset(POLICY_FILE_ENV),
        ScopedEnvVar::unset(PROOF_MODE_ENV),
        ScopedEnvVar::unset(HEAVY_CPU_THRESHOLD_ENV),
        ScopedEnvVar::unset(CPU_SAMPLE_WINDOW_ENV),
        ScopedEnvVar::unset("AMI_BENCHMARK_STRICT_HEAVY"),
        ScopedEnvVar::unset("AMI_OBSERVE_BIND"),
    ];
    scoped_env.push(ScopedEnvVar::set(PROOF_MODE_ENV, "1"));
    for (key, value) in envs {
        scoped_env.push(ScopedEnvVar::set(key, value));
    }
    let payload = collect_benchmark_contamination_preflight(options).await?;
    let stdout = format!("{}\n", serde_json::to_string(&payload)?);
    let success = payload["status"].as_str() == Some("pass");
    let stderr = if success {
        String::new()
    } else {
        let reasons = payload["reasons"]
            .as_array()
            .map(|values| {
                values
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_else(|| "unknown benchmark contamination failure".to_string());
        format!("benchmark contamination preflight failed: {reasons}")
    };
    drop(scoped_env);
    Ok(PreflightCliOutput {
        success,
        stdout,
        stderr,
        payload: Some(payload),
    })
}

fn expect_success_json(output: &PreflightCliOutput, label: &str) -> Result<()> {
    if !output.success {
        bail!("{label} unexpectedly failed: {}", output.stderr);
    }
    require_json_string(output, &["status"], "pass", label)
}

fn expect_failure_json(output: &PreflightCliOutput, label: &str) -> Result<()> {
    if output.success {
        bail!("{label} unexpectedly passed");
    }
    require_json_string(output, &["status"], "fail", label)
}

fn expect_failure_contains(output: &PreflightCliOutput, needle: &str, label: &str) -> Result<()> {
    if output.success {
        bail!("{label} unexpectedly passed");
    }
    let combined = format!("{}{}", output.stdout, output.stderr);
    if !combined.contains(needle) {
        bail!("{label} did not contain {needle:?}: {combined}");
    }
    Ok(())
}

fn proof_payload<'a>(output: &'a PreflightCliOutput, label: &str) -> Result<&'a Value> {
    output
        .payload
        .as_ref()
        .ok_or_else(|| anyhow!("{label} did not emit JSON payload: {}", output.stdout))
}

fn json_at_path<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    let mut current = value;
    for segment in path {
        if let Ok(index) = segment.parse::<usize>() {
            current = &current[index];
        } else {
            current = &current[*segment];
        }
    }
    current
}

fn require_json_string(
    output: &PreflightCliOutput,
    path: &[&str],
    expected: &str,
    label: &str,
) -> Result<()> {
    let payload = proof_payload(output, label)?;
    let actual = json_at_path(payload, path)
        .as_str()
        .ok_or_else(|| anyhow!("{label} missing string at {}", path.join(".")))?;
    if actual != expected {
        bail!(
            "{label} expected {} at {}, got {}",
            expected,
            path.join("."),
            actual
        );
    }
    Ok(())
}

fn require_json_bool(
    output: &PreflightCliOutput,
    path: &[&str],
    expected: bool,
    label: &str,
) -> Result<()> {
    let payload = proof_payload(output, label)?;
    let actual = json_at_path(payload, path)
        .as_bool()
        .ok_or_else(|| anyhow!("{label} missing bool at {}", path.join(".")))?;
    if actual != expected {
        bail!(
            "{label} expected {} at {}, got {}",
            expected,
            path.join("."),
            actual
        );
    }
    Ok(())
}

fn require_json_array_len(
    output: &PreflightCliOutput,
    path: &[&str],
    expected: usize,
    label: &str,
) -> Result<()> {
    let payload = proof_payload(output, label)?;
    let actual = json_at_path(payload, path)
        .as_array()
        .ok_or_else(|| anyhow!("{label} missing array at {}", path.join(".")))?;
    if actual.len() != expected {
        bail!(
            "{label} expected array length {} at {}, got {}",
            expected,
            path.join("."),
            actual.len()
        );
    }
    Ok(())
}

fn require_json_array_min_len(
    output: &PreflightCliOutput,
    path: &[&str],
    minimum: usize,
    label: &str,
) -> Result<()> {
    let payload = proof_payload(output, label)?;
    let actual = json_at_path(payload, path)
        .as_array()
        .ok_or_else(|| anyhow!("{label} missing array at {}", path.join(".")))?;
    if actual.len() < minimum {
        bail!(
            "{label} expected array length >= {} at {}, got {}",
            minimum,
            path.join("."),
            actual.len()
        );
    }
    Ok(())
}

fn require_json_array_contains_string(
    output: &PreflightCliOutput,
    path: &[&str],
    expected: &str,
    label: &str,
) -> Result<()> {
    let payload = proof_payload(output, label)?;
    let actual = json_at_path(payload, path)
        .as_array()
        .ok_or_else(|| anyhow!("{label} missing array at {}", path.join(".")))?;
    if !actual.iter().any(|value| value.as_str() == Some(expected)) {
        bail!("{label} missing {expected:?} at {}", path.join("."));
    }
    Ok(())
}

fn require_not_contains(haystack: &str, needle: &str, label: &str) -> Result<()> {
    if haystack.contains(needle) {
        bail!("{label} leaked {needle:?}");
    }
    Ok(())
}

async fn collect_benchmark_contamination_preflight(
    options: BenchmarkContaminationRunOptions,
) -> Result<Value> {
    ensure_internal_proof_mode(options)?;
    ensure_fixture_env_allowed(options.allow_fixtures)?;
    ensure_test_override_env_allowed(options.allow_test_overrides)?;
    let canonical_bind =
        env::var("AMI_OBSERVE_BIND").unwrap_or_else(|_| DEFAULT_OBSERVE_BIND.to_string());
    let policy = load_benchmark_contamination_policy()?;
    let (heavy_cpu_threshold, heavy_cpu_threshold_override_applied) =
        env_f64_override(HEAVY_CPU_THRESHOLD_ENV, policy.heavy_cpu_threshold_percent)?;
    let (sample_window_ms, cpu_sample_window_override_applied) =
        env_u64_override(CPU_SAMPLE_WINDOW_ENV, policy.cpu_sample_window_ms)?;
    let strict_heavy_env_enabled = env_truthy("AMI_BENCHMARK_STRICT_HEAVY");
    let strict_heavy = options.strict_heavy || strict_heavy_env_enabled;

    let process_samples =
        sample_process_cpu_usage(sample_window_ms.max(1), options.allow_fixtures).await?;
    let client_status_samples = collect_client_status_cpu_samples_for_policy(
        &policy,
        &process_samples,
        options.allow_fixtures,
    )
    .await?;
    let observe_samples = process_samples
        .iter()
        .filter(|sample| !is_current_process_sample(sample))
        .filter(|sample| is_observe_instance(sample))
        .cloned()
        .collect::<Vec<_>>();
    let observe_instances = observe_samples
        .iter()
        .map(format_process_sample)
        .collect::<Vec<_>>();
    let blocking_benchmark_instances = process_samples
        .iter()
        .filter(|sample| !is_current_process_sample(sample))
        .filter(|sample| is_parallel_verify_lane(sample, &policy))
        .map(format_process_sample)
        .collect::<Vec<_>>();

    let mut canonical_release_count = 0_u64;
    let mut blocking_observe_instances = Vec::new();
    for sample in &observe_samples {
        if is_canonical_release_observe(sample, &canonical_bind) {
            canonical_release_count += 1;
        } else {
            blocking_observe_instances.push(format_process_sample(sample));
        }
    }

    let all_heavy_process_assessments = process_samples
        .iter()
        .filter(|sample| !is_current_process_sample(sample))
        .filter(|sample| sample.current_cpu_percent >= heavy_cpu_threshold)
        .cloned()
        .map(|sample| {
            assess_heavy_process(sample, heavy_cpu_threshold, &policy, &client_status_samples)
        })
        .collect::<Vec<_>>();
    let heavy_report = heavy_process_report_sets(all_heavy_process_assessments);
    let heavy_process_lines = heavy_report
        .preview
        .iter()
        .filter(|assessment| assessment.verdict.is_blocking())
        .map(format_heavy_process_assessment_line)
        .collect::<Vec<_>>();
    let advisory_client_process_lines = heavy_report
        .preview
        .iter()
        .filter(|assessment| assessment.verdict == HeavyProcessVerdict::AdvisoryClientLowCpu)
        .map(format_heavy_process_assessment_line)
        .collect::<Vec<_>>();

    let strict_heavy_processes = if strict_heavy {
        heavy_report
            .blocking_preview
            .iter()
            .map(format_heavy_process_assessment_line)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let mut status = "pass";
    let mut reasons = Vec::new();
    if canonical_release_count > 1 {
        status = "fail";
        reasons.push(MULTIPLE_CANONICAL_OBSERVE_REASON);
    }
    if !blocking_observe_instances.is_empty() {
        status = "fail";
        reasons.push(NON_CANONICAL_OBSERVE_REASON);
    }
    if !blocking_benchmark_instances.is_empty() {
        status = "fail";
        reasons.push(PARALLEL_VERIFY_REASON);
    }
    if strict_heavy && !heavy_report.blocking_all.is_empty() {
        status = "fail";
        reasons.push(STRICT_HEAVY_REASON);
    }
    let mut client_status_clients_checked =
        client_status_samples.keys().cloned().collect::<Vec<_>>();
    client_status_clients_checked.sort();

    Ok(json!({
        "status": status,
        "canonical_bind": canonical_bind,
        "heavy_cpu_threshold": heavy_cpu_threshold,
        "cpu_sample_window_ms": sample_window_ms,
        "cpu_sample_kind": "max_subwindow_cpu_percent_v2",
        "strict_heavy": strict_heavy,
        "policy_version": policy.policy_version,
        "policy_path": benchmark_contamination_policy_path()?.display().to_string(),
        "policy_env_override": env::var(POLICY_FILE_ENV).is_ok(),
        "cmdline_redaction": CMDLINE_REDACTION_VERSION,
        "policy_defaults": {
            "heavy_cpu_threshold_percent": policy.heavy_cpu_threshold_percent,
            "cpu_sample_window_ms": policy.cpu_sample_window_ms,
        },
        "runtime_overrides": {
            "heavy_cpu_threshold_percent": heavy_cpu_threshold_override_applied,
            "cpu_sample_window_ms": cpu_sample_window_override_applied,
            "strict_heavy": options.strict_heavy || strict_heavy_env_enabled,
            "observe_bind": observe_bind_override_requested(),
        },
        "fixtures_allowed": options.allow_fixtures,
        "test_overrides_allowed": options.allow_test_overrides,
        "parallel_verify_lanes": policy.parallel_verify_lanes.clone(),
        "allowed_heavy_processes": policy.allowed_heavy_processes.iter().map(|rule| {
            json!({
                "code": rule.code,
                "process_comm_names": rule.process_comm_names,
                "exe_path_contains": rule.exe_path_contains,
                "cmdline_contains": rule.cmdline_contains,
            })
        }).collect::<Vec<_>>(),
        "client_reconciliation_rules": policy.client_process_reconciliation.iter().map(|rule| {
            json!({
                "code": rule.code,
                "status_probe_code": rule.status_probe_code,
                "status_timeout_ms": rule.status_timeout_ms,
                "process_comm_names": rule.process_comm_names,
                "exe_path_contains": rule.exe_path_contains,
                "cmdline_contains": rule.cmdline_contains,
            })
        }).collect::<Vec<_>>(),
        "canonical_release_count": canonical_release_count,
        "observe_instances": observe_instances,
        "blocking_observe_instances": blocking_observe_instances,
        "blocking_benchmark_instances": blocking_benchmark_instances,
        "heavy_external_processes": heavy_process_lines,
        "blocking_heavy_process_count": heavy_report.blocking_all.len(),
        "advisory_client_processes": advisory_client_process_lines,
        "heavy_process_assessments": heavy_report.preview
            .iter()
            .map(heavy_process_assessment_json)
            .collect::<Vec<_>>(),
        "strict_heavy_processes": strict_heavy_processes,
        "client_status_checked": !client_status_samples.is_empty(),
        "client_status_available": client_status_samples.values().any(|samples| samples.is_some()),
        "client_status_clients_checked": client_status_clients_checked,
        "reasons": reasons,
    }))
}

fn ensure_internal_proof_mode(options: BenchmarkContaminationRunOptions) -> Result<()> {
    if (options.allow_fixtures || options.allow_test_overrides) && !env_truthy(PROOF_MODE_ENV) {
        bail!("benchmark contamination internal proof options require {PROOF_MODE_ENV}=1");
    }
    Ok(())
}

fn heavy_process_report_sets(
    mut assessments: Vec<HeavyProcessAssessment>,
) -> HeavyProcessReportSets {
    assessments.sort_by(|left, right| {
        right
            .sample
            .current_cpu_percent
            .partial_cmp(&left.sample.current_cpu_percent)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let blocking_all = assessments
        .iter()
        .filter(|assessment| assessment.verdict.is_blocking())
        .cloned()
        .collect::<Vec<_>>();
    let preview = assessments
        .iter()
        .take(MAX_HEAVY_PROCESS_PREVIEW)
        .cloned()
        .collect::<Vec<_>>();
    let blocking_preview = blocking_all
        .iter()
        .take(MAX_HEAVY_PROCESS_PREVIEW)
        .cloned()
        .collect::<Vec<_>>();
    HeavyProcessReportSets {
        preview,
        blocking_all,
        blocking_preview,
    }
}

fn print_benchmark_contamination_text(payload: &Value) {
    let canonical_bind = payload["canonical_bind"]
        .as_str()
        .unwrap_or(DEFAULT_OBSERVE_BIND);
    let heavy_cpu_threshold = payload["heavy_cpu_threshold"].as_f64().unwrap_or(0.0);
    let sample_window_ms = payload["cpu_sample_window_ms"].as_u64().unwrap_or(0);
    let strict_heavy = payload["strict_heavy"].as_bool().unwrap_or(false);
    let canonical_release_count = payload["canonical_release_count"].as_u64().unwrap_or(0);

    println!("benchmark contamination preflight");
    println!("canonical observe bind: {canonical_bind}");
    println!("heavy CPU threshold: {heavy_cpu_threshold}%");
    println!("cpu sample window: {sample_window_ms}ms");
    println!("strict heavy process mode: {strict_heavy}");
    println!("canonical release observe count: {canonical_release_count}");

    print_text_lines(
        "blocking observe instances:",
        payload["blocking_observe_instances"].as_array(),
    );
    print_text_lines(
        "blocking benchmark instances:",
        payload["blocking_benchmark_instances"].as_array(),
    );
    if strict_heavy {
        print_text_lines(
            "heavy external processes (blocking):",
            payload["strict_heavy_processes"].as_array(),
        );
    } else {
        print_text_lines(
            "heavy external processes (advisory):",
            payload["heavy_external_processes"].as_array(),
        );
    }
    print_text_lines(
        "client processes downgraded by authoritative status truth:",
        payload["advisory_client_processes"].as_array(),
    );
}

fn print_text_lines(header: &str, values: Option<&Vec<Value>>) {
    let Some(values) = values else {
        return;
    };
    if values.is_empty() {
        return;
    }
    println!("{header}");
    for value in values {
        if let Some(line) = value.as_str() {
            println!("  {line}");
        }
    }
}

async fn sample_process_cpu_usage(
    sample_window_ms: u64,
    allow_fixtures: bool,
) -> Result<Vec<ProcessCpuSample>> {
    if let Some(samples) = load_process_samples_fixture(allow_fixtures)? {
        return Ok(samples);
    }
    let cpu_count = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(1) as f64;
    let intervals = cpu_sample_intervals(sample_window_ms.max(1));
    let mut previous_total_ticks = read_total_cpu_ticks()?;
    let mut previous_samples = read_process_samples()?;
    let mut accumulated = HashMap::<u32, ProcessCpuSample>::new();
    for interval_ms in intervals {
        sleep(Duration::from_millis(interval_ms)).await;
        let next_total_ticks = read_total_cpu_ticks()?;
        let next_samples = read_process_samples()?;
        let total_delta = next_total_ticks.saturating_sub(previous_total_ticks);
        merge_process_cpu_samples(
            &mut accumulated,
            process_cpu_samples_from_snapshots(
                previous_samples,
                next_samples.clone(),
                total_delta,
                cpu_count,
            ),
        );
        previous_total_ticks = next_total_ticks;
        previous_samples = next_samples;
    }
    let mut samples = accumulated.into_values().collect::<Vec<_>>();
    samples.sort_by_key(|sample| sample.pid);
    Ok(samples)
}

fn cpu_sample_intervals(sample_window_ms: u64) -> Vec<u64> {
    let total = sample_window_ms.max(1);
    let mut step = if total <= MAX_CPU_SAMPLE_INTERVAL_MS {
        total
    } else {
        MAX_CPU_SAMPLE_INTERVAL_MS
    };
    if total > MAX_CPU_SAMPLE_INTERVAL_MS.saturating_mul(MAX_CPU_SAMPLE_WINDOWS) {
        step = total.div_ceil(MAX_CPU_SAMPLE_WINDOWS);
    }
    step = step.clamp(1, total.max(MIN_CPU_SAMPLE_INTERVAL_MS));
    if total > MIN_CPU_SAMPLE_INTERVAL_MS {
        step = step.max(MIN_CPU_SAMPLE_INTERVAL_MS).min(total);
    }
    let mut remaining = total;
    let mut intervals = Vec::new();
    while remaining > 0 {
        let current = remaining.min(step);
        intervals.push(current);
        remaining = remaining.saturating_sub(current);
    }
    intervals
}

fn merge_process_cpu_samples(
    accumulated: &mut HashMap<u32, ProcessCpuSample>,
    interval_samples: Vec<ProcessCpuSample>,
) {
    for sample in interval_samples {
        match accumulated.entry(sample.pid) {
            std::collections::hash_map::Entry::Occupied(mut current) => {
                if sample.current_cpu_percent > current.get().current_cpu_percent {
                    current.insert(sample);
                }
            }
            std::collections::hash_map::Entry::Vacant(vacant) => {
                vacant.insert(sample);
            }
        }
    }
}

fn process_cpu_samples_from_snapshots(
    before: Vec<ProcessSample>,
    after: Vec<ProcessSample>,
    total_delta: u64,
    cpu_count: f64,
) -> Vec<ProcessCpuSample> {
    let before_map = before
        .into_iter()
        .map(|sample| (sample.pid, sample))
        .collect::<HashMap<_, _>>();
    let after_map = after
        .into_iter()
        .map(|sample| (sample.pid, sample))
        .collect::<HashMap<_, _>>();
    let mut pids = before_map.keys().copied().collect::<BTreeSet<_>>();
    pids.extend(after_map.keys().copied());
    let mut samples = Vec::with_capacity(pids.len());
    for pid in pids {
        match (before_map.get(&pid), after_map.get(&pid)) {
            (Some(before_process), Some(after_process)) => samples.push(ProcessCpuSample {
                pid,
                comm: after_process.comm.clone(),
                exe_path: after_process.exe_path.clone(),
                cmdline: after_process.cmdline.clone(),
                current_cpu_percent: current_cpu_percent(
                    total_delta,
                    cpu_count,
                    before_process.cpu_ticks,
                    after_process.cpu_ticks,
                ),
            }),
            (Some(before_process), None) => samples.push(ProcessCpuSample {
                pid,
                comm: before_process.comm.clone(),
                exe_path: before_process.exe_path.clone(),
                cmdline: before_process.cmdline.clone(),
                current_cpu_percent: 0.0,
            }),
            (None, Some(after_process)) => samples.push(ProcessCpuSample {
                pid,
                comm: after_process.comm.clone(),
                exe_path: after_process.exe_path.clone(),
                cmdline: after_process.cmdline.clone(),
                current_cpu_percent: current_cpu_percent(
                    total_delta,
                    cpu_count,
                    0,
                    after_process.cpu_ticks,
                ),
            }),
            (None, None) => {}
        }
    }
    samples
}

fn ensure_fixture_env_allowed(allow_fixtures: bool) -> Result<()> {
    let requested = [
        PROCESS_SAMPLES_FIXTURE_FILE_ENV,
        CLIENT_STATUS_FIXTURE_FILE_ENV,
    ]
    .into_iter()
    .filter(|key| env::var_os(key).is_some())
    .collect::<Vec<_>>();
    reject_requested_env_without_flag(
        &requested,
        allow_fixtures,
        "--allow-fixtures",
        "benchmark contamination fixture env",
    )
}

fn ensure_test_override_env_allowed(allow_test_overrides: bool) -> Result<()> {
    let mut requested = [
        POLICY_FILE_ENV,
        HEAVY_CPU_THRESHOLD_ENV,
        CPU_SAMPLE_WINDOW_ENV,
    ]
    .into_iter()
    .filter(|key| env::var_os(key).is_some())
    .collect::<Vec<_>>();
    if observe_bind_override_requested() {
        requested.push("AMI_OBSERVE_BIND");
    }
    reject_requested_env_without_flag(
        &requested,
        allow_test_overrides,
        "--allow-test-overrides",
        "benchmark contamination test override env",
    )
}

fn reject_requested_env_without_flag(
    requested: &[&str],
    allowed: bool,
    flag_name: &str,
    label: &str,
) -> Result<()> {
    if !requested.is_empty() && !allowed {
        bail!("{label} requires {flag_name}: {}", requested.join(", "));
    }
    Ok(())
}

fn observe_bind_override_requested() -> bool {
    env::var("AMI_OBSERVE_BIND")
        .ok()
        .is_some_and(|value| value != DEFAULT_OBSERVE_BIND)
}

fn load_process_samples_fixture(allow_fixtures: bool) -> Result<Option<Vec<ProcessCpuSample>>> {
    let Ok(fixture_path) = env::var(PROCESS_SAMPLES_FIXTURE_FILE_ENV) else {
        return Ok(None);
    };
    if !allow_fixtures {
        bail!("{PROCESS_SAMPLES_FIXTURE_FILE_ENV} requires --allow-fixtures");
    }
    let rendered = std::fs::read_to_string(&fixture_path)
        .with_context(|| format!("failed to read {}", fixture_path))?;
    let fixtures = serde_json::from_str::<Vec<ProcessCpuSampleFixture>>(&rendered)
        .with_context(|| format!("failed to parse {}", fixture_path))?;
    Ok(Some(
        fixtures
            .into_iter()
            .map(|fixture| ProcessCpuSample {
                pid: fixture.pid,
                comm: fixture.comm,
                exe_path: fixture.exe_path,
                cmdline: fixture.cmdline,
                current_cpu_percent: fixture.current_cpu_percent,
            })
            .collect(),
    ))
}

fn current_cpu_percent(
    total_delta_ticks: u64,
    cpu_count: f64,
    before_process_ticks: u64,
    after_process_ticks: u64,
) -> f64 {
    if total_delta_ticks == 0 {
        return 0.0;
    }
    let process_delta = after_process_ticks.saturating_sub(before_process_ticks) as f64;
    (process_delta / total_delta_ticks as f64) * cpu_count * 100.0
}

fn read_process_samples() -> Result<Vec<ProcessSample>> {
    let mut samples = Vec::new();
    for entry in std::fs::read_dir("/proc").context("failed to read /proc")? {
        let entry = match entry {
            Ok(value) => value,
            Err(_) => continue,
        };
        let file_name = entry.file_name();
        let pid = match file_name.to_string_lossy().parse::<u32>() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let proc_path = entry.path();
        let cpu_ticks = match read_process_cpu_ticks(&proc_path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let comm = read_trimmed_string(proc_path.join("comm")).unwrap_or_default();
        let exe_path = read_exe_path(proc_path.join("exe")).ok();
        let cmdline = read_cmdline(proc_path.join("cmdline")).unwrap_or_else(|_| comm.clone());
        samples.push(ProcessSample {
            pid,
            comm,
            exe_path,
            cmdline,
            cpu_ticks,
        });
    }
    Ok(samples)
}

fn read_total_cpu_ticks() -> Result<u64> {
    let stat = std::fs::read_to_string("/proc/stat").context("failed to read /proc/stat")?;
    let line = stat
        .lines()
        .find(|line| line.starts_with("cpu "))
        .ok_or_else(|| anyhow!("missing aggregate cpu line in /proc/stat"))?;
    let total = line
        .split_whitespace()
        .skip(1)
        .filter_map(|token| token.parse::<u64>().ok())
        .sum::<u64>();
    Ok(total)
}

fn read_process_cpu_ticks(proc_path: &std::path::Path) -> Result<u64> {
    let stat = std::fs::read_to_string(proc_path.join("stat"))
        .with_context(|| format!("failed to read {}", proc_path.join("stat").display()))?;
    parse_process_cpu_ticks(&stat)
}

fn parse_process_cpu_ticks(stat: &str) -> Result<u64> {
    let comm_end = stat
        .rfind(") ")
        .ok_or_else(|| anyhow!("unexpected /proc stat format"))?;
    let rest = &stat[(comm_end + 2)..];
    let fields = rest.split_whitespace().collect::<Vec<_>>();
    let utime = fields
        .get(11)
        .ok_or_else(|| anyhow!("missing utime in /proc stat"))?
        .parse::<u64>()
        .context("invalid utime in /proc stat")?;
    let stime = fields
        .get(12)
        .ok_or_else(|| anyhow!("missing stime in /proc stat"))?
        .parse::<u64>()
        .context("invalid stime in /proc stat")?;
    Ok(utime + stime)
}

fn read_cmdline(path: std::path::PathBuf) -> Result<String> {
    let raw = std::fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    if raw.is_empty() {
        return Ok(String::new());
    }
    let parts = raw
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect::<Vec<_>>();
    Ok(parts.join(" "))
}

fn read_trimmed_string(path: std::path::PathBuf) -> Result<String> {
    Ok(std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .trim()
        .to_string())
}

fn read_exe_path(path: std::path::PathBuf) -> Result<String> {
    Ok(std::fs::read_link(&path)
        .with_context(|| format!("failed to read {}", path.display()))?
        .display()
        .to_string())
}

fn benchmark_contamination_policy_path() -> Result<PathBuf> {
    if let Ok(path) = env::var(POLICY_FILE_ENV) {
        return Ok(PathBuf::from(path));
    }
    Ok(crate::config::discover_repo_root(None)?.join(BENCHMARK_CONTAMINATION_POLICY_PATH))
}

fn load_benchmark_contamination_policy() -> Result<BenchmarkContaminationPolicy> {
    let policy_path = benchmark_contamination_policy_path()?;
    let rendered = std::fs::read_to_string(&policy_path)
        .with_context(|| format!("failed to read {}", policy_path.display()))?;
    let policy = toml::from_str::<BenchmarkContaminationPolicy>(&rendered)
        .with_context(|| format!("failed to parse {}", policy_path.display()))?;
    policy.validate()?;
    Ok(policy)
}

fn is_current_process_sample(sample: &ProcessCpuSample) -> bool {
    sample.pid == std::process::id()
}

fn is_observe_instance(sample: &ProcessCpuSample) -> bool {
    sample.cmdline.contains(" observe serve") && trusted_amai_process(sample)
}

fn is_canonical_release_observe(sample: &ProcessCpuSample, canonical_bind: &str) -> bool {
    is_observe_instance(sample) && sample.cmdline.contains(&format!("--bind {canonical_bind}"))
}

fn is_parallel_verify_lane(
    sample: &ProcessCpuSample,
    policy: &BenchmarkContaminationPolicy,
) -> bool {
    if !trusted_amai_process(sample) {
        return false;
    }
    let tokens = sample.cmdline.split_whitespace().collect::<Vec<_>>();
    tokens.windows(2).any(|window| {
        window[0] == "verify"
            && policy
                .parallel_verify_lanes
                .iter()
                .any(|lane| window[1] == lane)
    })
}

fn trusted_amai_process(sample: &ProcessCpuSample) -> bool {
    let Some(exe_path) = sample.exe_path.as_deref() else {
        return false;
    };
    if std::path::Path::new(exe_path)
        .file_name()
        .and_then(|name| name.to_str())
        != Some("amai")
    {
        return false;
    }
    trusted_amai_exe_path(exe_path)
}

fn trusted_amai_exe_path(exe_path: &str) -> bool {
    let exe_path = Path::new(exe_path);
    trusted_amai_candidate_paths()
        .iter()
        .any(|candidate| same_canonical_path(exe_path, candidate))
}

fn trusted_amai_candidate_paths() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(current_exe) = env::current_exe() {
        candidates.push(current_exe);
    }
    if let Ok(repo_root) = crate::config::discover_repo_root(None) {
        candidates.push(repo_root.join("target/debug/amai"));
        candidates.push(repo_root.join("target/release/amai"));
    }
    candidates
}

fn same_canonical_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

fn is_allowed_heavy_process(
    sample: &ProcessCpuSample,
    policy: &BenchmarkContaminationPolicy,
) -> bool {
    policy
        .allowed_heavy_processes
        .iter()
        .any(|rule| rule.matches_process(sample))
}

fn assess_heavy_process(
    sample: ProcessCpuSample,
    heavy_cpu_threshold: f64,
    policy: &BenchmarkContaminationPolicy,
    client_status_samples: &HashMap<String, Option<HashMap<u32, ClientStatusCpuSample>>>,
) -> HeavyProcessAssessment {
    if is_allowed_heavy_process(&sample, policy) {
        return HeavyProcessAssessment {
            sample,
            verdict: HeavyProcessVerdict::AdvisoryAllowedInfrastructure,
            client_code: None,
            client_status: None,
        };
    }

    if let Some(rule) = matching_client_rule(&sample, policy) {
        let client_code = rule.code.clone();
        let verdict = match client_status_samples.get(&client_code) {
            Some(Some(samples)) => match samples.get(&sample.pid).cloned() {
                Some(status) if status.cpu_percent < heavy_cpu_threshold => {
                    return HeavyProcessAssessment {
                        sample,
                        verdict: HeavyProcessVerdict::AdvisoryClientLowCpu,
                        client_code: Some(client_code),
                        client_status: Some(status),
                    };
                }
                Some(status) => {
                    return HeavyProcessAssessment {
                        sample,
                        verdict: HeavyProcessVerdict::BlockingClientHighCpu,
                        client_code: Some(client_code),
                        client_status: Some(status),
                    };
                }
                None => HeavyProcessVerdict::BlockingClientPidMissing,
            },
            Some(None) | None => HeavyProcessVerdict::BlockingClientStatusUnavailable,
        };
        return HeavyProcessAssessment {
            sample,
            verdict,
            client_code: Some(client_code),
            client_status: None,
        };
    }

    HeavyProcessAssessment {
        sample,
        verdict: HeavyProcessVerdict::BlockingExternal,
        client_code: None,
        client_status: None,
    }
}

fn matching_client_rule<'a>(
    sample: &ProcessCpuSample,
    policy: &'a BenchmarkContaminationPolicy,
) -> Option<&'a ClientProcessReconciliationRule> {
    policy
        .client_process_reconciliation
        .iter()
        .find(|rule| rule.matches_process(sample))
}

fn format_heavy_process_assessment_line(assessment: &HeavyProcessAssessment) -> String {
    let mut line = format_process_sample(&assessment.sample);
    line.push_str(" verdict=");
    line.push_str(assessment.verdict.as_str());
    if let Some(client_code) = &assessment.client_code {
        line.push_str(&format!(" client={client_code}"));
    }
    if let Some(status) = &assessment.client_status {
        line.push_str(&format!(
            " client_status_cpu={:.1} client_status_label={}",
            status.cpu_percent,
            redact_cmdline(&status.label)
        ));
    }
    line
}

fn heavy_process_assessment_json(assessment: &HeavyProcessAssessment) -> Value {
    let client_status = assessment.client_status.as_ref().map(|status| {
        json!({
            "pid": status.pid,
            "cpu_percent": status.cpu_percent,
            "label": redact_cmdline(&status.label),
            "client_code": status.client_code,
        })
    });
    json!({
        "pid": assessment.sample.pid,
        "comm": assessment.sample.comm,
        "exe_path": assessment.sample.exe_path.as_deref().map(redact_cmdline),
        "cmdline": redact_cmdline(&assessment.sample.cmdline),
        "cmdline_redaction": CMDLINE_REDACTION_VERSION,
        "current_cpu_percent": assessment.sample.current_cpu_percent,
        "verdict": assessment.verdict.as_str(),
        "blocking": assessment.verdict.is_blocking(),
        "client_code": assessment.client_code,
        "client_status": client_status,
    })
}

fn format_process_sample(sample: &ProcessCpuSample) -> String {
    let exe_path = sample
        .exe_path
        .as_deref()
        .map(redact_cmdline)
        .unwrap_or_else(|| "<unavailable>".to_string());
    format!(
        "{} {:.1} {} exe={} cmdline={}",
        sample.pid,
        sample.current_cpu_percent,
        sample.comm,
        exe_path,
        redact_cmdline(&sample.cmdline)
    )
}

fn redact_cmdline(cmdline: &str) -> String {
    let mut previous_was_secret_flag = false;
    cmdline
        .split_whitespace()
        .map(|token| {
            let redacted = redact_arg(token, previous_was_secret_flag);
            previous_was_secret_flag = is_secret_flag(token);
            redacted
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_arg(token: &str, previous_was_secret_flag: bool) -> String {
    if previous_was_secret_flag || is_secret_value(token) {
        return "<redacted>".to_string();
    }
    if let Some((key, _value)) = token.split_once('=') {
        if is_secret_key(key) {
            return format!("{key}=<redacted>");
        }
    }
    redact_home_path(token)
}

fn redact_home_path(token: &str) -> String {
    let Ok(home) = env::var("HOME") else {
        return token.to_string();
    };
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return token.to_string();
    }
    token.replace(home, "$HOME")
}

fn is_secret_flag(token: &str) -> bool {
    let normalized = token
        .trim_start_matches('-')
        .trim_end_matches(':')
        .to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "api-key"
            | "apikey"
            | "api_key"
            | "authorization"
            | "bearer"
            | "key"
            | "password"
            | "secret"
            | "token"
    )
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key
        .trim_start_matches('-')
        .trim_end_matches(':')
        .to_ascii_lowercase();
    normalized.contains("api_key")
        || normalized.contains("apikey")
        || normalized.contains("authorization")
        || normalized.contains("password")
        || normalized.contains("secret")
        || normalized.contains("token")
        || normalized.ends_with("key")
}

fn is_secret_value(token: &str) -> bool {
    token.starts_with("sk-") || token.starts_with("Bearer ")
}

impl HeavyProcessVerdict {
    fn as_str(self) -> &'static str {
        match self {
            HeavyProcessVerdict::AdvisoryAllowedInfrastructure => "advisory_allowed_infrastructure",
            HeavyProcessVerdict::AdvisoryClientLowCpu => "advisory_client_low_cpu",
            HeavyProcessVerdict::BlockingExternal => "blocking_external",
            HeavyProcessVerdict::BlockingClientStatusUnavailable => {
                "blocking_client_status_unavailable"
            }
            HeavyProcessVerdict::BlockingClientPidMissing => "blocking_client_pid_missing",
            HeavyProcessVerdict::BlockingClientHighCpu => "blocking_client_high_cpu",
        }
    }

    fn is_blocking(self) -> bool {
        !matches!(
            self,
            HeavyProcessVerdict::AdvisoryAllowedInfrastructure
                | HeavyProcessVerdict::AdvisoryClientLowCpu
        )
    }
}

async fn collect_client_status_cpu_samples_for_policy(
    policy: &BenchmarkContaminationPolicy,
    process_samples: &[ProcessCpuSample],
    allow_fixtures: bool,
) -> Result<HashMap<String, Option<HashMap<u32, ClientStatusCpuSample>>>> {
    let mut samples_by_client = HashMap::new();
    for rule in &policy.client_process_reconciliation {
        if !process_samples
            .iter()
            .any(|sample| rule.matches_process(sample))
        {
            continue;
        }
        let samples = collect_client_status_cpu_samples(rule, allow_fixtures).await?;
        samples_by_client.insert(rule.code.clone(), samples);
    }
    Ok(samples_by_client)
}

async fn collect_client_status_cpu_samples(
    rule: &ClientProcessReconciliationRule,
    allow_fixtures: bool,
) -> Result<Option<HashMap<u32, ClientStatusCpuSample>>> {
    if let Some(fixture_path) = client_status_fixture_path(rule, allow_fixtures)? {
        let rendered = std::fs::read_to_string(&fixture_path)
            .with_context(|| format!("failed to read {}", fixture_path.display()))?;
        let samples = parse_client_status_cpu_samples(&rendered, &rule.code);
        if samples.is_empty() {
            return Ok(None);
        }
        return Ok(Some(samples));
    }
    let probe = client_status_probe(rule)?;
    let output = match timeout(
        Duration::from_millis(rule.status_timeout_ms),
        ProcessCommand::new(&probe.program)
            .args(probe.args)
            .output(),
    )
    .await
    {
        Ok(Ok(output)) if output.status.success() => output,
        _ => return Ok(None),
    };
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let samples = parse_client_status_cpu_samples(&rendered, &rule.code);
    if samples.is_empty() {
        return Ok(None);
    }
    Ok(Some(samples))
}

#[derive(Debug, Clone)]
struct ClientStatusProbe {
    program: String,
    args: &'static [&'static str],
}

fn client_status_probe(rule: &ClientProcessReconciliationRule) -> Result<ClientStatusProbe> {
    match rule.status_probe_code.as_str() {
        "vscode_code_status" => Ok(ClientStatusProbe {
            program: "/usr/bin/code".to_string(),
            args: &["--status"],
        }),
        _ => bail!(
            "unknown benchmark contamination client status probe: {}",
            rule.status_probe_code
        ),
    }
}

fn client_status_fixture_path(
    _rule: &ClientProcessReconciliationRule,
    allow_fixtures: bool,
) -> Result<Option<PathBuf>> {
    let path = env::var(CLIENT_STATUS_FIXTURE_FILE_ENV)
        .ok()
        .map(PathBuf::from);
    if path.is_some() && !allow_fixtures {
        bail!("benchmark contamination client status fixture requires --allow-fixtures");
    }
    Ok(path)
}

fn parse_client_status_cpu_samples(
    rendered: &str,
    client_code: &str,
) -> HashMap<u32, ClientStatusCpuSample> {
    let mut in_table = false;
    let mut rows = HashMap::new();
    for line in rendered.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("CPU %") {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        if trimmed.starts_with("Workspace Stats:") {
            break;
        }
        if trimmed.is_empty() {
            continue;
        }
        let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
        if tokens.len() < 4 {
            continue;
        }
        let cpu_percent = match tokens[0].parse::<f64>() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let pid = match tokens[2].parse::<u32>() {
            Ok(value) => value,
            Err(_) => continue,
        };
        let label = tokens[3..].join(" ");
        if label.ends_with("--status")
            || label.contains("vscode_live_cpu_truth.sh")
            || label.contains("electron-nodejs (cli.js)")
        {
            continue;
        }
        rows.insert(
            pid,
            ClientStatusCpuSample {
                pid,
                cpu_percent,
                label,
                client_code: client_code.to_string(),
            },
        );
    }
    rows
}

fn env_truthy(key: &str) -> bool {
    match env::var(key) {
        Ok(value) => matches!(value.trim(), "1" | "true" | "TRUE" | "True"),
        Err(_) => false,
    }
}

fn env_f64_override(key: &str, default: f64) -> Result<(f64, bool)> {
    match env::var(key) {
        Ok(value) => Ok((
            value
                .trim()
                .parse::<f64>()
                .with_context(|| format!("invalid {key}: expected number, got {value:?}"))?,
            true,
        )),
        Err(_) => Ok((default, false)),
    }
}

fn env_u64_override(key: &str, default: u64) -> Result<(u64, bool)> {
    match env::var(key) {
        Ok(value) => Ok((
            value
                .trim()
                .parse::<u64>()
                .with_context(|| format!("invalid {key}: expected integer, got {value:?}"))?,
            true,
        )),
        Err(_) => Ok((default, false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_policy() -> BenchmarkContaminationPolicy {
        BenchmarkContaminationPolicy {
            policy_version: "benchmark-contamination-policy-test".to_string(),
            heavy_cpu_threshold_percent: 50.0,
            cpu_sample_window_ms: 1000,
            parallel_verify_lanes: vec![
                "load".to_string(),
                "benchmark".to_string(),
                "accuracy".to_string(),
                "memory-matrix".to_string(),
                "mcp-matrix".to_string(),
            ],
            allowed_heavy_process_names: vec![],
            allowed_heavy_processes: vec![AllowedHeavyProcessRule {
                code: "amai_mcp_serve".to_string(),
                process_comm_names: vec!["amai".to_string()],
                exe_path_contains: vec!["/target/release/amai".to_string()],
                cmdline_contains: vec!["target/release/amai mcp serve".to_string()],
            }],
            client_process_reconciliation: vec![ClientProcessReconciliationRule {
                code: "vscode".to_string(),
                status_probe_code: "vscode_code_status".to_string(),
                status_timeout_ms: 5000,
                process_comm_names: vec!["code".to_string()],
                exe_path_contains: vec!["/usr/share/code/code".to_string()],
                cmdline_contains: vec![
                    "/usr/share/code/code".to_string(),
                    ".config/Code".to_string(),
                    "vscode-webview".to_string(),
                    ".vscode/extensions/".to_string(),
                ],
            }],
        }
    }

    fn client_status_map(
        client_code: &str,
        rows: Vec<ClientStatusCpuSample>,
    ) -> HashMap<String, Option<HashMap<u32, ClientStatusCpuSample>>> {
        let mut by_pid = HashMap::new();
        for row in rows {
            by_pid.insert(row.pid, row);
        }
        let mut by_client = HashMap::new();
        by_client.insert(client_code.to_string(), Some(by_pid));
        by_client
    }

    #[test]
    fn allowed_heavy_process_requires_comm_and_cmdline_rule_match() {
        let policy = test_policy();
        let allowed = ProcessCpuSample {
            pid: 3100,
            comm: "amai".to_string(),
            exe_path: Some("/workspace/agent-memory-index/target/release/amai".to_string()),
            cmdline: "target/release/amai mcp serve".to_string(),
            current_cpu_percent: 80.0,
        };
        assert!(is_allowed_heavy_process(&allowed, &policy));

        let same_name_wrong_cmdline = ProcessCpuSample {
            pid: 3101,
            comm: "amai".to_string(),
            exe_path: Some("/workspace/agent-memory-index/target/release/amai".to_string()),
            cmdline: "target/release/amai verify mcp-matrix".to_string(),
            current_cpu_percent: 80.0,
        };
        assert!(!is_allowed_heavy_process(&same_name_wrong_cmdline, &policy));

        let postgres_name_only = ProcessCpuSample {
            pid: 3102,
            comm: "postgres".to_string(),
            exe_path: Some("/usr/lib/postgresql/postgres".to_string()),
            cmdline: "postgres: checkpointer".to_string(),
            current_cpu_percent: 80.0,
        };
        let rows = HashMap::new();
        let assessment = assess_heavy_process(postgres_name_only, 50.0, &policy, &rows);
        assert_eq!(assessment.verdict, HeavyProcessVerdict::BlockingExternal);
    }

    #[test]
    fn allowed_heavy_process_requires_cmdline_matcher() {
        let rule = AllowedHeavyProcessRule {
            code: "postgres_checkpointer".to_string(),
            process_comm_names: vec!["postgres".to_string()],
            exe_path_contains: vec!["/usr/lib/postgresql/postgres".to_string()],
            cmdline_contains: vec![],
        };
        let error = rule
            .validate()
            .expect_err("exe-only allowed rule must fail closed");
        assert!(error.to_string().contains("cmdline_contains"));
    }

    #[test]
    fn allowed_heavy_process_requires_exe_path_matcher() {
        let rule = AllowedHeavyProcessRule {
            code: "amai_mcp_serve".to_string(),
            process_comm_names: vec!["amai".to_string()],
            exe_path_contains: vec![],
            cmdline_contains: vec!["target/release/amai mcp serve".to_string()],
        };
        let error = rule
            .validate()
            .expect_err("allow rule without exe_path matcher must fail closed");
        assert!(error.to_string().contains("exe_path_contains"));
    }

    #[test]
    fn parse_process_cpu_ticks_reads_utime_plus_stime() {
        let stat = "1234 (code helper) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20";
        assert_eq!(parse_process_cpu_ticks(stat).unwrap(), 23);
    }

    #[test]
    fn process_cpu_samples_include_late_spawned_processes() {
        let samples = process_cpu_samples_from_snapshots(
            vec![
                ProcessSample {
                    pid: 100,
                    comm: "before-only".to_string(),
                    exe_path: Some("/usr/bin/before-only".to_string()),
                    cmdline: "before-only".to_string(),
                    cpu_ticks: 10,
                },
                ProcessSample {
                    pid: 200,
                    comm: "stable".to_string(),
                    exe_path: Some("/usr/bin/stable".to_string()),
                    cmdline: "stable".to_string(),
                    cpu_ticks: 20,
                },
            ],
            vec![
                ProcessSample {
                    pid: 200,
                    comm: "stable".to_string(),
                    exe_path: Some("/usr/bin/stable".to_string()),
                    cmdline: "stable".to_string(),
                    cpu_ticks: 50,
                },
                ProcessSample {
                    pid: 300,
                    comm: "late".to_string(),
                    exe_path: Some("/usr/bin/late".to_string()),
                    cmdline: "late".to_string(),
                    cpu_ticks: 40,
                },
            ],
            100,
            4.0,
        );

        assert_eq!(samples.len(), 3);
        assert_eq!(samples[0].pid, 100);
        assert_eq!(samples[0].current_cpu_percent, 0.0);
        assert_eq!(samples[1].pid, 200);
        assert_eq!(samples[1].current_cpu_percent, 120.0);
        assert_eq!(samples[2].pid, 300);
        assert_eq!(samples[2].current_cpu_percent, 160.0);
    }

    #[test]
    fn cpu_sample_intervals_split_long_windows_into_multiple_samples() {
        assert_eq!(cpu_sample_intervals(50), vec![50]);
        assert_eq!(cpu_sample_intervals(250), vec![250]);
        assert_eq!(cpu_sample_intervals(1000), vec![250, 250, 250, 250]);
        assert_eq!(
            cpu_sample_intervals(3000),
            vec![300, 300, 300, 300, 300, 300, 300, 300, 300, 300]
        );
    }

    #[test]
    fn multi_interval_merge_keeps_transient_mid_window_processes() {
        let mut accumulated = HashMap::new();
        merge_process_cpu_samples(
            &mut accumulated,
            process_cpu_samples_from_snapshots(
                vec![ProcessSample {
                    pid: 200,
                    comm: "stable".to_string(),
                    exe_path: Some("/usr/bin/stable".to_string()),
                    cmdline: "stable".to_string(),
                    cpu_ticks: 10,
                }],
                vec![
                    ProcessSample {
                        pid: 200,
                        comm: "stable".to_string(),
                        exe_path: Some("/usr/bin/stable".to_string()),
                        cmdline: "stable".to_string(),
                        cpu_ticks: 20,
                    },
                    ProcessSample {
                        pid: 300,
                        comm: "transient".to_string(),
                        exe_path: Some("/usr/bin/transient".to_string()),
                        cmdline: "transient".to_string(),
                        cpu_ticks: 30,
                    },
                ],
                100,
                4.0,
            ),
        );
        merge_process_cpu_samples(
            &mut accumulated,
            process_cpu_samples_from_snapshots(
                vec![
                    ProcessSample {
                        pid: 200,
                        comm: "stable".to_string(),
                        exe_path: Some("/usr/bin/stable".to_string()),
                        cmdline: "stable".to_string(),
                        cpu_ticks: 20,
                    },
                    ProcessSample {
                        pid: 300,
                        comm: "transient".to_string(),
                        exe_path: Some("/usr/bin/transient".to_string()),
                        cmdline: "transient".to_string(),
                        cpu_ticks: 30,
                    },
                ],
                vec![ProcessSample {
                    pid: 200,
                    comm: "stable".to_string(),
                    exe_path: Some("/usr/bin/stable".to_string()),
                    cmdline: "stable".to_string(),
                    cpu_ticks: 30,
                }],
                100,
                4.0,
            ),
        );
        let mut samples = accumulated.into_values().collect::<Vec<_>>();
        samples.sort_by_key(|sample| sample.pid);
        assert_eq!(samples.len(), 2);
        assert_eq!(samples[1].pid, 300);
        assert_eq!(samples[1].current_cpu_percent, 120.0);
    }

    #[test]
    fn observe_instance_match_requires_trusted_binary_identity() {
        let release = ProcessCpuSample {
            pid: 4100,
            comm: "amai".to_string(),
            exe_path: Some("/home/art/agent-memory-index/target/release/amai".to_string()),
            cmdline:
                "/home/art/agent-memory-index/target/release/amai observe serve --bind 0.0.0.0:9464"
                    .to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(is_observe_instance(&release));
        let cargo = ProcessCpuSample {
            pid: 4101,
            comm: "cargo".to_string(),
            exe_path: Some("/usr/bin/cargo".to_string()),
            cmdline: "cargo run --release --quiet -- observe serve --bind 0.0.0.0:9464".to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(!is_observe_instance(&cargo));
        let spoofed = ProcessCpuSample {
            pid: 4102,
            comm: "sleep".to_string(),
            exe_path: Some("/usr/bin/sleep".to_string()),
            cmdline: "target/release/amai observe serve --bind 0.0.0.0:9464".to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(!is_observe_instance(&spoofed));
        let non_standard_target = ProcessCpuSample {
            pid: 4104,
            comm: "amai".to_string(),
            exe_path: Some("/home/art/agent-memory-index/target/custom/amai".to_string()),
            cmdline:
                "/home/art/agent-memory-index/target/custom/amai observe serve --bind 0.0.0.0:9464"
                    .to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(!is_observe_instance(&non_standard_target));
        let snapshot = ProcessCpuSample {
            pid: 4105,
            comm: "amai".to_string(),
            exe_path: Some("/home/art/agent-memory-index/target/release/amai".to_string()),
            cmdline: "/home/art/agent-memory-index/target/release/amai observe snapshot"
                .to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(!is_observe_instance(&snapshot));
    }

    #[test]
    fn verify_lane_match_is_limited_to_parallel_benchmark_commands() {
        let policy = test_policy();
        let memory = ProcessCpuSample {
            pid: 4200,
            comm: "amai".to_string(),
            exe_path: Some("/home/art/agent-memory-index/target/release/amai".to_string()),
            cmdline: "/home/art/agent-memory-index/target/release/amai verify memory-matrix --matrix letta_memory_local"
                .to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(is_parallel_verify_lane(&memory, &policy));
        let load = ProcessCpuSample {
            pid: 4201,
            comm: "amai".to_string(),
            exe_path: Some("/home/art/agent-memory-index/target/release/amai".to_string()),
            cmdline: "/home/art/agent-memory-index/target/release/amai verify load --workers 8"
                .to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(is_parallel_verify_lane(&load, &policy));
        let mcp = ProcessCpuSample {
            pid: 4202,
            comm: "amai".to_string(),
            exe_path: Some("/home/art/agent-memory-index/target/release/amai".to_string()),
            cmdline: "/home/art/agent-memory-index/target/release/amai verify mcp-matrix --matrix live_mcpbench_local".to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(is_parallel_verify_lane(&mcp, &policy));
        let near_mcp = ProcessCpuSample {
            pid: 4203,
            comm: "amai".to_string(),
            exe_path: Some("/home/art/agent-memory-index/target/release/amai".to_string()),
            cmdline: "/home/art/agent-memory-index/target/release/amai verify mcp-matrix-old --matrix live_mcpbench_local".to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(!is_parallel_verify_lane(&near_mcp, &policy));
        let near_memory = ProcessCpuSample {
            pid: 4204,
            comm: "amai".to_string(),
            exe_path: Some("/home/art/agent-memory-index/target/release/amai".to_string()),
            cmdline: "/home/art/agent-memory-index/target/release/amai verify memory-matrixer --matrix letta_memory_local".to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(!is_parallel_verify_lane(&near_memory, &policy));
        let spoofed = ProcessCpuSample {
            pid: 4205,
            comm: "sleep".to_string(),
            exe_path: Some("/usr/bin/sleep".to_string()),
            cmdline: "/home/art/agent-memory-index/target/release/amai verify mcp-matrix --matrix live_mcpbench_local".to_string(),
            current_cpu_percent: 1.0,
        };
        assert!(!is_parallel_verify_lane(&spoofed, &policy));
    }

    #[test]
    fn client_reconciliation_requires_comm_and_cmdline_rule_match() {
        let policy = test_policy();
        let matching = ProcessCpuSample {
            pid: 2244026,
            comm: "code".to_string(),
            exe_path: Some("/usr/share/code/code".to_string()),
            cmdline: "/proc/self/exe --type=utility --user-data-dir=/home/art/.config/Code"
                .to_string(),
            current_cpu_percent: 52.5,
        };
        assert!(matching_client_rule(&matching, &policy).is_some());

        let same_comm_unrelated_cmdline = ProcessCpuSample {
            pid: 2244027,
            comm: "code".to_string(),
            exe_path: Some("/usr/share/code/code".to_string()),
            cmdline: "codegen-benchmark-helper --not-a-client".to_string(),
            current_cpu_percent: 52.5,
        };
        assert!(matching_client_rule(&same_comm_unrelated_cmdline, &policy).is_none());

        let same_cmdline_wrong_comm = ProcessCpuSample {
            pid: 2244028,
            comm: "node".to_string(),
            exe_path: Some("/usr/share/code/code".to_string()),
            cmdline: "/usr/share/code/code --type=zygote".to_string(),
            current_cpu_percent: 52.5,
        };
        assert!(matching_client_rule(&same_cmdline_wrong_comm, &policy).is_none());

        let same_comm_and_cmdline_wrong_exe = ProcessCpuSample {
            pid: 2244029,
            comm: "code".to_string(),
            exe_path: Some("/tmp/fake-code".to_string()),
            cmdline: "/proc/self/exe --type=utility --user-data-dir=/home/art/.config/Code"
                .to_string(),
            current_cpu_percent: 52.5,
        };
        assert!(matching_client_rule(&same_comm_and_cmdline_wrong_exe, &policy).is_none());
    }

    #[test]
    fn cmdline_redaction_hides_secret_like_tokens_and_home_path() {
        let home = env::var("HOME").unwrap_or_else(|_| "/home/art".to_string());
        let cmdline = format!(
            "amai --api-key sk-1234567890abcdef token=plain --password hunter2 {home}/.config/Code"
        );
        let redacted = redact_cmdline(&cmdline);
        assert!(!redacted.contains("sk-1234567890abcdef"));
        assert!(!redacted.contains("hunter2"));
        assert!(!redacted.contains(&home));
        assert!(redacted.contains("--api-key <redacted>"));
        assert!(redacted.contains("token=<redacted>"));
        assert!(redacted.contains("$HOME/.config/Code"));
    }

    #[test]
    fn fixture_env_guard_requires_explicit_allow_flag() {
        let requested = vec![
            PROCESS_SAMPLES_FIXTURE_FILE_ENV,
            CLIENT_STATUS_FIXTURE_FILE_ENV,
        ];
        let error = reject_requested_env_without_flag(
            &requested,
            false,
            "--allow-fixtures",
            "benchmark contamination fixture env",
        )
        .expect_err("fixture env must be rejected without explicit flag");
        assert!(error.to_string().contains("--allow-fixtures"));
        reject_requested_env_without_flag(
            &requested,
            true,
            "--allow-fixtures",
            "benchmark contamination fixture env",
        )
        .expect("explicit fixture flag allows fixture env");
    }

    #[test]
    fn test_override_env_guard_requires_explicit_allow_flag() {
        let requested = vec![
            POLICY_FILE_ENV,
            HEAVY_CPU_THRESHOLD_ENV,
            CPU_SAMPLE_WINDOW_ENV,
        ];
        let error = reject_requested_env_without_flag(
            &requested,
            false,
            "--allow-test-overrides",
            "benchmark contamination test override env",
        )
        .expect_err("test override env must be rejected without explicit flag");
        assert!(error.to_string().contains("--allow-test-overrides"));
        reject_requested_env_without_flag(
            &requested,
            true,
            "--allow-test-overrides",
            "benchmark contamination test override env",
        )
        .expect("explicit test override flag allows override env");
    }

    #[test]
    fn internal_proof_options_require_proof_mode() {
        let denied =
            ensure_internal_proof_mode(BenchmarkContaminationRunOptions::proof(false, true, false))
                .expect_err("proof-only flags must fail closed outside proof mode");
        assert!(denied.to_string().contains(PROOF_MODE_ENV));
        ensure_internal_proof_mode(BenchmarkContaminationRunOptions::live(false))
            .expect("live front door without proof flags stays allowed");
    }

    #[test]
    fn benchmark_contamination_policy_rejects_invalid_threshold_window_and_legacy_matchers() {
        let mut empty_policy_version = test_policy();
        empty_policy_version.policy_version.clear();
        assert!(empty_policy_version.validate().is_err());

        let mut zero_threshold = test_policy();
        zero_threshold.heavy_cpu_threshold_percent = 0.0;
        assert!(zero_threshold.validate().is_err());

        let mut zero_window = test_policy();
        zero_window.cpu_sample_window_ms = 0;
        assert!(zero_window.validate().is_err());

        let mut empty_lanes = test_policy();
        empty_lanes.parallel_verify_lanes.clear();
        assert!(empty_lanes.validate().is_err());

        let mut legacy_broad_names = test_policy();
        legacy_broad_names.allowed_heavy_process_names = vec!["postgres".to_string()];
        assert!(legacy_broad_names.validate().is_err());
    }

    #[test]
    fn benchmark_contamination_policy_rejects_unknown_status_probe_code() {
        let mut policy = test_policy();
        policy.client_process_reconciliation[0].status_probe_code =
            "shell_from_policy_is_forbidden".to_string();
        let error = policy
            .validate()
            .expect_err("unknown client status probe must fail closed");
        assert!(
            error
                .to_string()
                .contains("unknown benchmark contamination client status probe"),
            "{error:#}"
        );
        assert!(client_status_probe(&policy.client_process_reconciliation[0]).is_err());
    }

    #[test]
    fn client_status_probe_uses_pinned_wrapper_program() {
        let probe =
            client_status_probe(&test_policy().client_process_reconciliation[0]).expect("probe");
        assert_eq!(probe.program, "/usr/bin/code");
    }

    #[test]
    fn canonical_release_observe_requires_matching_bind() {
        let sample = ProcessCpuSample {
            pid: 1234,
            comm: "amai".to_string(),
            exe_path: Some("/home/art/agent-memory-index/target/release/amai".to_string()),
            cmdline:
                "/home/art/agent-memory-index/target/release/amai observe serve --bind 0.0.0.0:9464"
                    .to_string(),
            current_cpu_percent: 0.0,
        };
        assert!(is_canonical_release_observe(&sample, "0.0.0.0:9464"));
        assert!(!is_canonical_release_observe(&sample, "127.0.0.1:9464"));
    }

    #[test]
    fn parse_client_status_cpu_samples_extracts_pid_cpu_label_and_client_code() {
        let rendered = r#"
CPU %	Mem MB	   PID	Process
   12	539727720531	2244026	extension-host [10]
    1	788832822314	301677	zygote

Workspace Stats:
"#;
        let rows = parse_client_status_cpu_samples(rendered, "vscode");
        assert_eq!(rows.get(&2244026).map(|row| row.cpu_percent), Some(12.0));
        assert_eq!(
            rows.get(&2244026).map(|row| row.label.as_str()),
            Some("extension-host [10]")
        );
        assert_eq!(
            rows.get(&2244026).map(|row| row.client_code.as_str()),
            Some("vscode")
        );
        assert_eq!(rows.get(&301677).map(|row| row.cpu_percent), Some(1.0));
    }

    #[test]
    fn client_heavy_process_downgrades_when_client_status_cpu_is_low() {
        let sample = ProcessCpuSample {
            pid: 2244026,
            comm: "code".to_string(),
            exe_path: Some("/usr/share/code/code".to_string()),
            cmdline: "/proc/self/exe --type=utility --utility-sub-type=node.mojom.NodeService --user-data-dir=/home/art/.config/Code".to_string(),
            current_cpu_percent: 52.5,
        };
        let rows = client_status_map(
            "vscode",
            vec![ClientStatusCpuSample {
                pid: 2244026,
                cpu_percent: 12.0,
                label: "extension-host [10]".to_string(),
                client_code: "vscode".to_string(),
            }],
        );
        let policy = test_policy();
        let assessment = assess_heavy_process(sample, 50.0, &policy, &rows);
        assert_eq!(
            assessment.verdict,
            HeavyProcessVerdict::AdvisoryClientLowCpu
        );
        assert_eq!(
            assessment.client_status.as_ref().map(|row| row.cpu_percent),
            Some(12.0)
        );
        assert_eq!(assessment.client_code.as_deref(), Some("vscode"));
    }

    #[test]
    fn client_heavy_process_stays_blocking_when_client_status_cpu_is_high() {
        let sample = ProcessCpuSample {
            pid: 2244026,
            comm: "code".to_string(),
            exe_path: Some("/usr/share/code/code".to_string()),
            cmdline: "/usr/share/code/code --type=zygote".to_string(),
            current_cpu_percent: 58.7,
        };
        let rows = client_status_map(
            "vscode",
            vec![ClientStatusCpuSample {
                pid: 2244026,
                cpu_percent: 58.0,
                label: "zygote".to_string(),
                client_code: "vscode".to_string(),
            }],
        );
        let policy = test_policy();
        let assessment = assess_heavy_process(sample, 50.0, &policy, &rows);
        assert_eq!(
            assessment.verdict,
            HeavyProcessVerdict::BlockingClientHighCpu
        );
        assert!(assessment.verdict.is_blocking());
        assert_eq!(assessment.client_code.as_deref(), Some("vscode"));
    }

    #[test]
    fn client_heavy_process_fail_closes_when_client_status_is_unavailable() {
        let sample = ProcessCpuSample {
            pid: 2244026,
            comm: "code".to_string(),
            exe_path: Some("/usr/share/code/code".to_string()),
            cmdline: "/usr/share/code/code --type=zygote".to_string(),
            current_cpu_percent: 58.7,
        };
        let policy = test_policy();
        let rows = HashMap::new();
        let assessment = assess_heavy_process(sample, 50.0, &policy, &rows);
        assert_eq!(
            assessment.verdict,
            HeavyProcessVerdict::BlockingClientStatusUnavailable
        );
        assert!(assessment.verdict.is_blocking());
        assert_eq!(assessment.client_code.as_deref(), Some("vscode"));
    }

    #[test]
    fn client_heavy_process_fail_closes_when_client_status_pid_is_missing() {
        let sample = ProcessCpuSample {
            pid: 2244026,
            comm: "code".to_string(),
            exe_path: Some("/usr/share/code/code".to_string()),
            cmdline: "/usr/share/code/code --type=zygote".to_string(),
            current_cpu_percent: 58.7,
        };
        let rows = client_status_map(
            "vscode",
            vec![ClientStatusCpuSample {
                pid: 999999,
                cpu_percent: 2.0,
                label: "extension-host [10]".to_string(),
                client_code: "vscode".to_string(),
            }],
        );
        let policy = test_policy();
        let assessment = assess_heavy_process(sample, 50.0, &policy, &rows);
        assert_eq!(
            assessment.verdict,
            HeavyProcessVerdict::BlockingClientPidMissing
        );
        assert!(assessment.verdict.is_blocking());
    }

    #[test]
    fn blocking_heavy_process_lines_exclude_advisory_client_rows() {
        let assessments = vec![
            HeavyProcessAssessment {
                sample: ProcessCpuSample {
                    pid: 550593,
                    comm: "yandex_browser".to_string(),
                    exe_path: Some("/opt/yandex/browser/yandex_browser".to_string()),
                    cmdline: "/opt/yandex/browser/yandex_browser --type=renderer".to_string(),
                    current_cpu_percent: 73.2,
                },
                verdict: HeavyProcessVerdict::BlockingExternal,
                client_code: None,
                client_status: None,
            },
            HeavyProcessAssessment {
                sample: ProcessCpuSample {
                    pid: 2244026,
                    comm: "code".to_string(),
                    exe_path: Some("/usr/share/code/code".to_string()),
                    cmdline:
                        "/proc/self/exe --type=utility --utility-sub-type=node.mojom.NodeService"
                            .to_string(),
                    current_cpu_percent: 52.5,
                },
                verdict: HeavyProcessVerdict::AdvisoryClientLowCpu,
                client_code: Some("vscode".to_string()),
                client_status: Some(ClientStatusCpuSample {
                    pid: 2244026,
                    cpu_percent: 2.0,
                    label: "extension-host [10]".to_string(),
                    client_code: "vscode".to_string(),
                }),
            },
        ];
        let lines = assessments
            .iter()
            .filter(|assessment| assessment.verdict.is_blocking())
            .map(format_heavy_process_assessment_line)
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("blocking_external"));
        assert!(!lines[0].contains("advisory_client_low_cpu"));
    }

    #[test]
    fn blocking_heavy_process_after_preview_limit_still_fails_strict_gate() {
        let mut assessments = (0..MAX_HEAVY_PROCESS_PREVIEW)
            .map(|index| HeavyProcessAssessment {
                sample: ProcessCpuSample {
                    pid: 1000 + index as u32,
                    comm: "postgres".to_string(),
                    exe_path: Some("/usr/lib/postgresql/postgres".to_string()),
                    cmdline: format!("postgres worker {index}"),
                    current_cpu_percent: 90.0 - index as f64,
                },
                verdict: HeavyProcessVerdict::AdvisoryAllowedInfrastructure,
                client_code: None,
                client_status: None,
            })
            .collect::<Vec<_>>();
        assessments.push(HeavyProcessAssessment {
            sample: ProcessCpuSample {
                pid: 2000,
                comm: "browser".to_string(),
                exe_path: Some("/opt/browser/browser".to_string()),
                cmdline: "/opt/browser/renderer".to_string(),
                current_cpu_percent: 70.0,
            },
            verdict: HeavyProcessVerdict::BlockingExternal,
            client_code: None,
            client_status: None,
        });

        let report = heavy_process_report_sets(assessments);

        assert_eq!(report.preview.len(), MAX_HEAVY_PROCESS_PREVIEW);
        assert!(
            report
                .preview
                .iter()
                .all(|assessment| !assessment.verdict.is_blocking())
        );
        assert_eq!(report.blocking_all.len(), 1);
        assert_eq!(report.blocking_preview.len(), 1);
        assert_eq!(
            report.blocking_preview[0].verdict,
            HeavyProcessVerdict::BlockingExternal
        );
    }

    #[test]
    fn default_policy_contains_queue1_parallel_matrix_lanes() {
        let policy = load_benchmark_contamination_policy().unwrap();
        assert!(
            policy
                .parallel_verify_lanes
                .iter()
                .any(|lane| lane == "memory-matrix")
        );
        assert!(
            policy
                .parallel_verify_lanes
                .iter()
                .any(|lane| lane == "mcp-matrix")
        );
        assert!(
            policy
                .client_process_reconciliation
                .iter()
                .any(|rule| rule.code == "vscode")
        );
    }
}
