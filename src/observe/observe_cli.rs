use super::*;

pub async fn print_snapshot(cfg: &AppConfig) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let snapshot = collect_snapshot(cfg).await?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

pub async fn print_snapshot_preview(cfg: &AppConfig) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let snapshot = collect_snapshot_preview(cfg).await?;
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    Ok(())
}

pub async fn print_budget_snapshot_preview(cfg: &AppConfig) -> Result<()> {
    let snapshot =
        if let Some(payload) = try_fetch_local_observe_budget_snapshot_preview_via_http().await {
            payload
        } else {
            compact_budget_snapshot_preview_payload(&collect_budget_snapshot_preview(cfg).await?)
        };
    println!("{}", serde_json::to_string(&snapshot)?);
    Ok(())
}

pub async fn run_sla_check(cfg: &AppConfig) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let snapshot = collect_snapshot(cfg).await?;
    let summary = &snapshot["sla"]["summary"];
    let critical = summary["critical"].as_u64().unwrap_or(0);
    let unknown = summary["unknown"].as_u64().unwrap_or(0);
    println!("{}", serde_json::to_string_pretty(&snapshot)?);
    if critical > 0 || unknown > 0 {
        return Err(anyhow!(
            "sla check failed: critical={critical}, unknown={unknown}"
        ));
    }
    Ok(())
}

pub async fn print_guardrails(cfg: &AppConfig) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let db = postgres::connect_admin(cfg).await?;
    postgres::bootstrap_schema(&db, cfg).await?;
    let prefix = format!("observe-guardrail-{}", Uuid::new_v4());
    let result = collect_guardrail_report(&db, &prefix).await;
    let cleanup_result = cleanup_guardrail_rows(&db, &prefix).await;
    match (result, cleanup_result) {
        (Ok(report), Ok(())) => {
            println!("{}", serde_json::to_string_pretty(&report)?);
            Ok(())
        }
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup_error)) => Err(cleanup_error),
        (Err(error), Err(cleanup_error)) => Err(anyhow!(
            "{error:#}\nsecondary cleanup failure: {cleanup_error:#}"
        )),
    }
}

pub async fn print_retention_cleanup(
    cfg: &AppConfig,
    apply: bool,
    limit: Option<i64>,
) -> Result<()> {
    let summary = run_retention_cleanup(cfg, apply, limit).await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

pub async fn print_artifact_cleanup(
    _cfg: &AppConfig,
    apply: bool,
    limit: Option<usize>,
    aggressive: bool,
    target: Option<&str>,
) -> Result<()> {
    let repo_root = discover_repo_root(None)?;
    let summary =
        collect_artifact_cleanup_summary(&repo_root, apply, false, limit, aggressive, target)?;
    let _ = artifact_cleanup::write_latest_summary(&repo_root, &summary)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}
