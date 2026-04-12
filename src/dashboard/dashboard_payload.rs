use super::*;

pub fn build_payload(
    cfg: &AppConfig,
    snapshot: &Value,
    bind: &str,
    refresh_ms: u64,
) -> Result<Value> {
    let repo_root = config::discover_repo_root(None)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    let install_state = dashboard_context::load_install_state(&repo_root).unwrap_or(None);
    let machine = collect_machine_summary(&repo_root).ok();
    let base_url = browser_base_url(bind);
    let captured_at_epoch_ms = snapshot["captured_at_epoch_ms"]
        .as_u64()
        .unwrap_or_default();
    let observe_refresh_total_ms = snapshot["observe_refresh"]["total_ms"].as_u64();
    let (observe_refresh_slowest_stage, observe_refresh_slowest_stage_ms) =
        slowest_observe_refresh_stage(snapshot);

    let top_cards = build_top_cards(snapshot);
    let live_compare_card = top_cards
        .iter()
        .find(|card| card["kind"].as_str() == Some("live_compare"))
        .cloned()
        .unwrap_or(Value::Null);

    Ok(json!({
        "meta": {
            "stack_name": cfg.stack_name,
            "package_version": env!("CARGO_PKG_VERSION"),
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "captured_at_label": human_timestamp(captured_at_epoch_ms),
            "refresh_ms": refresh_ms,
            "refresh_seconds": refresh_ms / 1000,
            "base_url": base_url,
            "observe_refresh_total_ms": observe_refresh_total_ms,
            "observe_refresh_slowest_stage": observe_refresh_slowest_stage,
            "observe_refresh_slowest_stage_ms": observe_refresh_slowest_stage_ms,
        },
        "headline": build_headline(snapshot, captured_at_epoch_ms),
        "hero_cards": build_hero_cards(snapshot),
        "top_cards": top_cards,
        "live_compare_card": live_compare_card,
        "benchmark_cards": build_benchmark_cards(snapshot),
        "client_budget_live": client_budget_live_payload(snapshot),
        "machine_cards": build_machine_cards(snapshot, machine.as_ref(), install_state.as_ref()),
        "service_cards": build_service_cards(snapshot),
        "warnings": build_warnings(snapshot, machine.as_ref()),
        "glossary": build_glossary(),
        "links": build_links(&base_url),
    }))
}

pub fn build_live_summary_payload(
    cfg: &AppConfig,
    snapshot: &Value,
    bind: &str,
    refresh_ms: u64,
) -> Result<Value> {
    let base_url = browser_base_url(bind);
    let captured_at_epoch_ms = snapshot["captured_at_epoch_ms"]
        .as_u64()
        .unwrap_or_default();
    let observe_refresh_total_ms = snapshot["observe_refresh"]["total_ms"].as_u64();
    let (observe_refresh_slowest_stage, observe_refresh_slowest_stage_ms) =
        slowest_observe_refresh_stage(snapshot);

    Ok(json!({
        "meta": {
            "stack_name": cfg.stack_name,
            "package_version": env!("CARGO_PKG_VERSION"),
            "captured_at_epoch_ms": captured_at_epoch_ms,
            "captured_at_label": human_timestamp(captured_at_epoch_ms),
            "refresh_ms": refresh_ms,
            "refresh_seconds": refresh_ms / 1000,
            "base_url": base_url,
            "observe_refresh_total_ms": observe_refresh_total_ms,
            "observe_refresh_slowest_stage": observe_refresh_slowest_stage,
            "observe_refresh_slowest_stage_ms": observe_refresh_slowest_stage_ms,
        },
        "headline": build_headline(snapshot, captured_at_epoch_ms),
        "active_agent_card": build_active_agent_budget_session_card(snapshot),
        "top_cards": build_top_cards(snapshot),
    }))
}
