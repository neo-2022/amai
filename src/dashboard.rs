use crate::codex_threads;
use crate::config::{self, AppConfig};
use crate::continuity;
use crate::dashboard_format::*;
use crate::hardware_telemetry::{AcceleratorSummary, MachineSummary, collect_machine_summary};
use crate::onboarding;
use crate::working_state;
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

mod dashboard_benchmark_cards;
mod dashboard_card_support;
mod dashboard_client_budget_diagnostics;
mod dashboard_client_budget_support;
mod dashboard_client_limit_alignment;
mod dashboard_context;
mod dashboard_current_session_budget_guard;
mod dashboard_hero_cards;
mod dashboard_live_latency_compare;
mod dashboard_live_response_latency_support;
mod dashboard_metric_status_support;
mod dashboard_overview;
mod dashboard_payload;
mod dashboard_renderer;
mod dashboard_runtime_support;
mod dashboard_service_cards;
mod dashboard_working_state_card;
use self::dashboard_benchmark_cards::build_benchmark_cards;
pub use self::dashboard_card_support::monitoring_url;
use self::dashboard_card_support::{
    card, card_with_rows, compact_dashboard_text, humanize_identifier, metric_row,
    metric_row_with_key, status_label, status_reason_tooltip, tcp_port_is_open, with_extra_class,
    with_status, with_status_label, with_status_tooltip, with_table_orientation,
};
#[allow(unused_imports)]
pub(crate) use self::dashboard_client_budget_diagnostics::client_budget_root_cause_payload_with_guard;
use self::dashboard_client_budget_diagnostics::*;
use self::dashboard_client_budget_support::*;
pub(crate) use self::dashboard_client_budget_support::{
    CLIENT_TURN_PRESSURE_ROTATE_STATUS_LABELS, client_budget_live_payload,
    client_turn_pressure_display_status_label,
};
use self::dashboard_client_limit_alignment::*;
pub use self::dashboard_context::browser_base_url;
pub use self::dashboard_current_session_budget_guard::current_session_budget_guard;
pub(crate) use self::dashboard_hero_cards::build_active_agent_budget_session_card_from_surface;
use self::dashboard_hero_cards::{
    build_active_agent_budget_session_card, build_hero_cards,
    humanize_tracked_slice_exactness_value, humanize_tracked_slice_savings_value,
};
use self::dashboard_live_latency_compare::{
    live_latency_compare_card, live_latency_compare_status,
};
use self::dashboard_metric_status_support::*;
use self::dashboard_overview::{build_headline, build_top_cards};
pub use self::dashboard_payload::{build_live_summary_payload, build_payload};
pub use self::dashboard_renderer::render_html;
use self::dashboard_runtime_support::{
    build_glossary, build_governance_card, build_links, build_machine_cards, build_warnings,
};
use self::dashboard_service_cards::build_service_cards;
pub(crate) use self::dashboard_service_cards::{
    build_capacity_forecast_card, build_regression_explain_card,
};
use self::dashboard_working_state_card::*;

pub use crate::dashboard_assets::{brand_lockup_svg, brand_mark_svg, favicon_ico};

const CLIENT_LIVE_CONTEXT_ROW_KEY: &str = "client_live_context";
const CLIENT_LIVE_FULL_TURN_SAVINGS_ROW_KEY: &str = "client_live_full_turn_savings";
const CLIENT_LIVE_LIMIT_ROW_KEY: &str = "client_live_limit";
const CLIENT_LIMIT_HOURLY_BURN_ROW_KEY: &str = "client_limit_hourly_burn";
