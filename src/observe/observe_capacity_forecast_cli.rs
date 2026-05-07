use super::*;

pub async fn print_capacity_forecast(
    cfg: &AppConfig,
    args: &ObserveCapacityForecastArgs,
) -> Result<()> {
    maybe_cleanup_local_artifacts().await?;
    let snapshot = collect_snapshot(cfg).await?;
    let payload = snapshot["capacity_forecast"].clone();
    let rendered = match args.surface.as_str() {
        "snapshot" => payload,
        "dashboard" | "card" => dashboard::build_capacity_forecast_card(&snapshot),
        "summary" => payload["summary"].clone(),
        "window" => render_capacity_forecast_window(&payload, &args.window)?,
        other => {
            return Err(anyhow!(
                "unsupported capacity forecast surface: {other} (expected snapshot|dashboard|card|summary|window)"
            ));
        }
    };
    println!("{}", serde_json::to_string_pretty(&rendered)?);
    Ok(())
}

fn render_capacity_forecast_window(payload: &Value, window_key: &str) -> Result<Value> {
    let family = payload["families"]
        .as_array()
        .and_then(|families| families.first())
        .ok_or_else(|| anyhow!("capacity forecast families are unavailable"))?;
    let window = family["windows"]
        .as_array()
        .and_then(|windows| {
            windows
                .iter()
                .find(|item| item["window_key"].as_str() == Some(window_key))
        })
        .cloned()
        .ok_or_else(|| anyhow!("capacity forecast window not found: {window_key}"))?;
    Ok(json!({
        "family_key": family["family_key"].clone(),
        "title": family["title"].clone(),
        "window": window,
        "history_scope": payload["history_scope"].clone(),
    }))
}
