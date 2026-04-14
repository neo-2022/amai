use crate::cli::{
    ContextPackArgs, ObserveClientLimitHourlyBurnArgs, ObserveClientLimitTrendAnalysisArgs,
    ObserveTokenContractualSourcesArgs, ObserveTokenEvidencePackArgs, ObserveTokenReportArgs,
    ObserveTokenRolloutAssistantGenerationArgs, ObserveTokenStatementExportArgs,
    ObserveTokenWholeCycleAttachArgs, ObserveTokenWholeCycleTurnAttachArgs,
};
use crate::codex_threads;
use crate::config::{self, AppConfig};
use crate::dashboard;
use crate::language;
use crate::postgres::{self, ObservabilitySnapshotRecord};
use crate::retrieval;
use crate::working_state;
use anyhow::{Context, Result, anyhow, bail};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tiktoken_rs::{CoreBPE, cl100k_base, o200k_base};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command as ProcessCommand;
use tokio::time::timeout;
use tokio_postgres::Client;
use uuid::Uuid;

#[path = "token_budget/active_agent_support.rs"]
mod active_agent_support;
#[path = "token_budget/agent_cycle_surfaces.rs"]
mod agent_cycle_surfaces;
#[path = "token_budget/client_limit_runtime.rs"]
mod client_limit_runtime;
#[path = "token_budget/client_meter_kpi.rs"]
mod client_meter_kpi;
#[path = "token_budget/client_rate_limit_surfaces.rs"]
mod client_rate_limit_surfaces;
#[path = "token_budget/context_pack_thread_bindings.rs"]
mod context_pack_thread_bindings;
#[path = "token_budget/dashboard_active_agents.rs"]
mod dashboard_active_agents;
#[path = "token_budget/dashboard_agent_scope_activity.rs"]
mod dashboard_agent_scope_activity;
#[path = "token_budget/dashboard_assistant_scope.rs"]
mod dashboard_assistant_scope;
#[path = "token_budget/dashboard_current_session_report.rs"]
mod dashboard_current_session_report;
#[path = "token_budget/dashboard_event_cache_runtime.rs"]
mod dashboard_event_cache_runtime;
#[path = "token_budget/dashboard_event_caches.rs"]
mod dashboard_event_caches;
#[path = "token_budget/dashboard_exact_client_limits.rs"]
mod dashboard_exact_client_limits;
#[path = "token_budget/dashboard_live_response_latency.rs"]
mod dashboard_live_response_latency;
#[path = "token_budget/dashboard_report_cache_support.rs"]
mod dashboard_report_cache_support;
#[path = "token_budget/dashboard_report_core.rs"]
mod dashboard_report_core;
#[path = "token_budget/dashboard_report_surface.rs"]
mod dashboard_report_surface;
#[path = "token_budget/dashboard_shared_hints.rs"]
mod dashboard_shared_hints;
#[path = "token_budget/dashboard_statement_preview.rs"]
mod dashboard_statement_preview;
#[path = "token_budget/external_truth.rs"]
mod external_truth;
#[path = "token_budget/personal_kpi_window.rs"]
mod personal_kpi_window;
#[path = "token_budget/token_adjustments.rs"]
mod token_adjustments;
#[path = "token_budget/token_budget_contractual_surfaces.rs"]
mod token_budget_contractual_surfaces;
#[path = "token_budget/token_budget_models.rs"]
mod token_budget_models;
#[path = "token_budget/token_budget_runtime_analytics.rs"]
mod token_budget_runtime_analytics;
#[path = "token_budget/token_budget_runtime_contextual.rs"]
mod token_budget_runtime_contextual;
#[path = "token_budget/token_budget_runtime_dashboard.rs"]
mod token_budget_runtime_dashboard;
#[path = "token_budget/token_budget_runtime_event_flow.rs"]
mod token_budget_runtime_event_flow;
#[path = "token_budget/token_budget_runtime_maintenance.rs"]
mod token_budget_runtime_maintenance;
#[path = "token_budget/token_budget_runtime_observed.rs"]
mod token_budget_runtime_observed;
#[path = "token_budget/token_budget_runtime_reporting.rs"]
mod token_budget_runtime_reporting;
#[path = "token_budget/token_budget_runtime_shared.rs"]
mod token_budget_runtime_shared;
#[path = "token_budget/token_budget_runtime_support.rs"]
mod token_budget_runtime_support;
#[cfg(test)]
#[path = "token_budget/token_budget_runtime_tests.rs"]
mod token_budget_runtime_tests;

include!("token_budget/token_budget_runtime.rs");
