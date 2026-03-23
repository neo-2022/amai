use crate::config::AppConfig;
use crate::postgres;
use anyhow::{Context, Result, anyhow};
use reqwest::header::SERVER;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_postgres::Client;

#[derive(Debug, Clone, Deserialize)]
pub struct CompatibilityManifest {
    pub schema_version: i64,
    pub compatibility_profile: String,
    pub postgres: VersionRule,
    pub qdrant: VersionRule,
    pub nats: VersionRule,
    pub s3: S3Rule,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VersionRule {
    pub supported_major: u64,
    pub supported_minor: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct S3Rule {
    pub expected_server_family: Option<String>,
    pub strict_server_family: bool,
}

#[derive(Debug, Clone)]
pub struct CompatibilityReport {
    pub profile: String,
    pub schema_version: i64,
    pub postgres: ServiceCheck,
    pub qdrant: ServiceCheck,
    pub nats: ServiceCheck,
    pub s3: ServiceCheck,
    pub schema_meta_ok: bool,
    pub meta_reason: String,
}

#[derive(Debug, Clone)]
pub struct ServiceCheck {
    pub raw_version: String,
    pub compatible: bool,
    pub reason: String,
}

pub async fn print_report(cfg: &AppConfig) -> Result<()> {
    let report = check(cfg).await?;
    print_report_lines(&report);
    if !report.compatible() {
        return Err(anyhow!("compatibility check failed"));
    }
    Ok(())
}

pub async fn assert_supported(cfg: &AppConfig) -> Result<()> {
    let report = check(cfg).await?;
    if !report.compatible() {
        print_report_lines(&report);
        return Err(anyhow!(
            "stack drift detected outside supported compatibility profile"
        ));
    }
    Ok(())
}

pub async fn bootstrap_meta(_cfg: &AppConfig, client: &Client) -> Result<()> {
    let manifest = load_manifest()?;
    postgres::upsert_stack_meta(
        client,
        "compatibility",
        &json!({
            "schema_version": manifest.schema_version,
            "compatibility_profile": manifest.compatibility_profile
        }),
    )
    .await?;
    Ok(())
}

pub async fn check(cfg: &AppConfig) -> Result<CompatibilityReport> {
    let manifest = load_manifest()?;
    let http = http_client()?;

    let db = postgres::connect_admin(cfg).await?;
    let pg_version = db
        .query_one("SHOW server_version", &[])
        .await?
        .get::<_, String>(0);
    let postgres = evaluate_version_rule(&manifest.postgres, &pg_version, &pg_version)?;

    let qdrant_value: Value = http
        .get(format!("{}/", cfg.qdrant_http_url))
        .send()
        .await
        .context("failed to query qdrant root endpoint")?
        .json()
        .await
        .context("failed to decode qdrant root response")?;
    let qdrant_version = qdrant_value
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let qdrant = evaluate_version_rule(&manifest.qdrant, &qdrant_version, &qdrant_version)?;

    let nats_value: Value = http
        .get(format!("{}/varz", cfg.nats_http_url))
        .send()
        .await
        .context("failed to query nats /varz endpoint")?
        .json()
        .await
        .context("failed to decode nats /varz response")?;
    let nats_version = nats_value
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let nats = evaluate_version_rule(&manifest.nats, &nats_version, &nats_version)?;

    let s3_response = http
        .head(format!("{}/minio/health/live", cfg.s3_endpoint))
        .send()
        .await
        .context("failed to query s3-compatible health endpoint")?;
    let s3_server = s3_response
        .headers()
        .get(SERVER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let s3 = evaluate_s3(&manifest.s3, &s3_server);

    let stored_meta = postgres::get_stack_meta(&db, "compatibility").await?;
    let (schema_meta_ok, meta_reason) = match stored_meta {
        Some(value) => {
            let stored_schema = value
                .get("schema_version")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let stored_profile = value
                .get("compatibility_profile")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if stored_schema == manifest.schema_version
                && stored_profile == manifest.compatibility_profile
            {
                (true, "stack_meta matches manifest".to_string())
            } else {
                (
                    false,
                    format!(
                        "stack_meta mismatch: stored schema/profile = {stored_schema}/{stored_profile}, manifest = {}/{}",
                        manifest.schema_version, manifest.compatibility_profile
                    ),
                )
            }
        }
        None => (false, "stack_meta compatibility row missing".to_string()),
    };

    Ok(CompatibilityReport {
        profile: manifest.compatibility_profile,
        schema_version: manifest.schema_version,
        postgres,
        qdrant,
        nats,
        s3,
        schema_meta_ok,
        meta_reason,
    })
}

impl CompatibilityReport {
    pub fn compatible(&self) -> bool {
        self.postgres.compatible
            && self.qdrant.compatible
            && self.nats.compatible
            && self.s3.compatible
            && self.schema_meta_ok
    }
}

pub fn report_json(report: &CompatibilityReport) -> Value {
    json!({
        "profile": report.profile,
        "schema_version": report.schema_version,
        "compatible": report.compatible(),
        "schema_meta_ok": report.schema_meta_ok,
        "meta_reason": report.meta_reason,
        "services": {
            "postgres": {
                "raw_version": report.postgres.raw_version,
                "compatible": report.postgres.compatible,
                "reason": report.postgres.reason,
            },
            "qdrant": {
                "raw_version": report.qdrant.raw_version,
                "compatible": report.qdrant.compatible,
                "reason": report.qdrant.reason,
            },
            "nats": {
                "raw_version": report.nats.raw_version,
                "compatible": report.nats.compatible,
                "reason": report.nats.reason,
            },
            "s3": {
                "raw_version": report.s3.raw_version,
                "compatible": report.s3.compatible,
                "reason": report.s3.reason,
            },
        }
    })
}

fn print_report_lines(report: &CompatibilityReport) {
    println!(
        "compatibility_profile: {} (schema_version={})",
        report.profile, report.schema_version
    );
    println!(
        "compatibility.postgres: {} [{}]",
        fmt_state(report.postgres.compatible),
        report.postgres.reason
    );
    println!(
        "compatibility.qdrant: {} [{}]",
        fmt_state(report.qdrant.compatible),
        report.qdrant.reason
    );
    println!(
        "compatibility.nats: {} [{}]",
        fmt_state(report.nats.compatible),
        report.nats.reason
    );
    println!(
        "compatibility.s3: {} [{}]",
        fmt_state(report.s3.compatible),
        report.s3.reason
    );
    println!(
        "compatibility.stack_meta: {} [{}]",
        fmt_state(report.schema_meta_ok),
        report.meta_reason
    );
}

fn fmt_state(value: bool) -> &'static str {
    if value { "ok" } else { "FAIL" }
}

fn load_manifest() -> Result<CompatibilityManifest> {
    let path = manifest_path();
    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read compatibility manifest {}", path.display()))?;
    toml::from_str(&content).context("failed to parse compatibility manifest")
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .context("failed to build compatibility HTTP client")
}

fn manifest_path() -> PathBuf {
    let cwd_path = Path::new("config/compatibility.toml");
    if cwd_path.exists() {
        cwd_path.to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("config")
            .join("compatibility.toml")
    }
}

fn evaluate_version_rule(
    rule: &VersionRule,
    raw_version: &str,
    parse_source: &str,
) -> Result<ServiceCheck> {
    let parsed = parse_version(parse_source)?;
    let mut compatible = parsed.0 == rule.supported_major;
    let reason = if let Some(minor) = rule.supported_minor {
        if parsed.1 != minor {
            compatible = false;
        }
        format!(
            "live={raw_version}, supported={}{}{}",
            rule.supported_major, ".", minor
        )
    } else {
        format!(
            "live={raw_version}, supported_major={}",
            rule.supported_major
        )
    };
    Ok(ServiceCheck {
        raw_version: raw_version.to_string(),
        compatible,
        reason,
    })
}

fn evaluate_s3(rule: &S3Rule, server_header: &str) -> ServiceCheck {
    let expected = rule
        .expected_server_family
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    let compatible = if rule.strict_server_family {
        server_header.contains(&expected)
    } else {
        true
    };
    let reason = if rule.strict_server_family {
        format!("server={server_header}, expected_family={expected}")
    } else {
        format!("server={server_header}, expected_family={expected}, strict=false")
    };
    ServiceCheck {
        raw_version: server_header.to_string(),
        compatible,
        reason,
    }
}

fn parse_version(raw: &str) -> Result<(u64, u64, u64)> {
    let first = raw
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .find(|segment| segment.contains('.'))
        .or_else(|| raw.split_whitespace().find(|segment| segment.contains('.')))
        .unwrap_or(raw);
    let numbers = first
        .split('.')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            segment
                .parse::<u64>()
                .with_context(|| format!("failed to parse version component in {raw}"))
        })
        .collect::<Result<Vec<_>>>()?;
    let major = *numbers
        .first()
        .ok_or_else(|| anyhow!("missing version major in {raw}"))?;
    let minor = *numbers.get(1).unwrap_or(&0);
    let patch = *numbers.get(2).unwrap_or(&0);
    Ok((major, minor, patch))
}
