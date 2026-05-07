use super::*;

pub(crate) async fn collect_default_report(db: &Client) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)?;
    let config = load_config(&repo_root)?;
    collect_report(
        &repo_root,
        db,
        None,
        config.measurement.include_verify_events_by_default,
        None,
    )
    .await
}

include!("token_budget_runtime_dashboard_body.inc");
