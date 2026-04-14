use super::*;

fn codex_extension_relative_codex_path() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "bin/windows-x86_64/codex.exe"
    }
    #[cfg(target_os = "macos")]
    {
        "bin/macos-aarch64/codex"
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        "bin/linux-x86_64/codex"
    }
}

pub(crate) fn discover_local_codex_app_server_executable() -> Option<PathBuf> {
    let override_path = std::env::var_os("AMI_CODEX_APP_SERVER_BIN")
        .map(PathBuf::from)
        .filter(|path| path.is_file());
    if override_path.is_some() {
        return override_path;
    }
    let extensions_dir = dirs::home_dir()?.join(".vscode").join("extensions");
    let relative_path = PathBuf::from(codex_extension_relative_codex_path());
    let mut candidates = fs::read_dir(extensions_dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with("openai.chatgpt-"))
        })
        .map(|path| path.join(&relative_path))
        .filter(|path| path.is_file())
        .filter_map(|path| {
            let modified = path
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .unwrap_or(UNIX_EPOCH);
            Some((modified, path))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(modified, _)| *modified);
    candidates.pop().map(|(_, path)| path)
}

fn load_codex_chatgpt_auth_refresh_payload() -> Option<Value> {
    let auth_path = dirs::home_dir()?.join(".codex").join("auth.json");
    let raw = fs::read_to_string(auth_path).ok()?;
    let payload: Value = serde_json::from_str(&raw).ok()?;
    let access_token = payload["tokens"]["access_token"].as_str()?;
    let account_id = payload["tokens"]["account_id"].as_str()?;
    Some(json!({
        "accessToken": access_token,
        "chatgptAccountId": account_id,
        "chatgptPlanType": Value::Null,
    }))
}

async fn write_codex_app_server_json_line(
    stdin: &mut tokio::process::ChildStdin,
    value: &Value,
) -> Result<()> {
    stdin
        .write_all(serde_json::to_string(value)?.as_bytes())
        .await
        .context("failed to write codex app-server request")?;
    stdin
        .write_all(b"\n")
        .await
        .context("failed to terminate codex app-server request line")?;
    stdin
        .flush()
        .await
        .context("failed to flush codex app-server request")?;
    Ok(())
}

pub(crate) async fn query_codex_app_server_rate_limits(
    executable: &Path,
) -> Result<Option<CodexAppServerRateLimitsObservation>> {
    let mut child = ProcessCommand::new(executable)
        .arg("app-server")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| {
            format!(
                "failed to spawn codex app-server from {}",
                executable.display()
            )
        })?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("codex app-server stdin is unavailable"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("codex app-server stdout is unavailable"))?;
    let init_request = json!({
        "id": "1",
        "method": "initialize",
        "params": {
            "clientInfo": {
                "name": "amai-dashboard",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": Value::Null,
        }
    });
    write_codex_app_server_json_line(&mut stdin, &init_request).await?;
    let observed: Option<CodexAppServerRateLimitsObservation> =
        timeout(Duration::from_secs(5), async {
            let mut lines = BufReader::new(stdout).lines();
            let mut rate_limit_request_sent = false;
            while let Some(line) = lines
                .next_line()
                .await
                .context("failed to read codex app-server response")?
            {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let message: Value = serde_json::from_str(trimmed)
                    .context("failed to decode codex app-server JSON")?;
                if message["id"] == Value::from("1")
                    && message.get("result").is_some()
                    && !rate_limit_request_sent
                {
                    write_codex_app_server_json_line(
                        &mut stdin,
                        &json!({
                            "id": "2",
                            "method": "account/rateLimits/read",
                        }),
                    )
                    .await?;
                    rate_limit_request_sent = true;
                    continue;
                }
                if message["method"].as_str() == Some("account/chatgptAuthTokens/refresh") {
                    if let Some(refresh_payload) = load_codex_chatgpt_auth_refresh_payload() {
                        write_codex_app_server_json_line(
                            &mut stdin,
                            &json!({
                                "id": message["id"].clone(),
                                "result": refresh_payload,
                            }),
                        )
                        .await?;
                    }
                    continue;
                }
                if message["id"] != Value::from("2") {
                    continue;
                }
                let Some(result) = message.get("result") else {
                    return Ok::<Option<CodexAppServerRateLimitsObservation>, anyhow::Error>(None);
                };
                let parsed: CodexAppServerRateLimitsResponse =
                    serde_json::from_value(result.clone())
                        .context("failed to parse codex app-server rate limits response")?;
                let preferred_snapshot = parsed
                    .rate_limits_by_limit_id
                    .as_ref()
                    .and_then(|buckets| buckets.get("codex").cloned())
                    .unwrap_or(parsed.rate_limits);
                return Ok(Some(CodexAppServerRateLimitsObservation {
                    observed_at_epoch_ms: current_epoch_ms().unwrap_or_default() as u64,
                    rate_limits: preferred_snapshot,
                }));
            }
            Ok::<Option<CodexAppServerRateLimitsObservation>, anyhow::Error>(None)
        })
        .await
        .context("timed out while waiting for codex app-server rate limits")??;
    let _ = child.start_kill();
    let _ = timeout(Duration::from_secs(1), child.wait()).await;
    Ok(observed)
}

pub(crate) fn build_status_bar_rate_limits_json(
    observation: Option<&CodexAppServerRateLimitsObservation>,
) -> Value {
    let Some(observation) = observation else {
        return json!({
            "status": "missing",
            "source": "codex_app_server_account_rate_limits_read_v1",
            "status_bar_correlated": true,
            "note": "Exact upstream rate-limit source через codex app-server пока недоступен, поэтому dashboard должен честно деградировать к rollout fallback и не называть это точной копией VS Code status bar.",
        });
    };
    let primary_used_percent = observation
        .rate_limits
        .primary
        .as_ref()
        .map(|window| window.used_percent);
    let secondary_used_percent = observation
        .rate_limits
        .secondary
        .as_ref()
        .map(|window| window.used_percent);
    json!({
        "status": "observed",
        "source": "codex_app_server_account_rate_limits_read_v1",
        "status_bar_correlated": true,
        "observed_at_epoch_ms": observation.observed_at_epoch_ms,
        "ended_at_epoch_ms": observation.observed_at_epoch_ms,
        "limit_id": observation.rate_limits.limit_id,
        "limit_name": observation.rate_limits.limit_name,
        "plan_type": observation.rate_limits.plan_type,
        "primary_limit_used_percent": primary_used_percent,
        "primary_limit_remaining_percent": primary_used_percent.map(|value| (100.0 - value).max(0.0)),
        "primary_window_duration_mins": observation
            .rate_limits
            .primary
            .as_ref()
            .and_then(|window| window.window_duration_mins),
        "primary_resets_at_epoch_seconds": observation
            .rate_limits
            .primary
            .as_ref()
            .and_then(|window| window.resets_at),
        "secondary_limit_used_percent": secondary_used_percent,
        "secondary_limit_remaining_percent": secondary_used_percent.map(|value| (100.0 - value).max(0.0)),
        "secondary_window_duration_mins": observation
            .rate_limits
            .secondary
            .as_ref()
            .and_then(|window| window.window_duration_mins),
        "secondary_resets_at_epoch_seconds": observation
            .rate_limits
            .secondary
            .as_ref()
            .and_then(|window| window.resets_at),
        "credits": observation.rate_limits.credits.as_ref().map(|credits| {
            json!({
                "has_credits": credits.has_credits,
                "unlimited": credits.unlimited,
                "balance": credits.balance,
            })
        }).unwrap_or(Value::Null),
        "note": "Этот source поднимается через codex app-server account/rateLimits/read и совпадает с тем же upstream rate-limit contour, который extension использует для VS Code status bar и /wham/usage.",
    })
}

fn build_exact_client_limit_sample_payload(
    observation: &CodexAppServerRateLimitsObservation,
) -> Value {
    json!({
        "_observability": {
            "source_event_id": format!(
                "client-status-bar-rate-limits-{}",
                observation.observed_at_epoch_ms
            ),
            "source_kind": "codex_app_server_account_rate_limits_read_v1",
            "captured_at_epoch_ms": observation.observed_at_epoch_ms,
        },
        "captured_at_epoch_ms": observation.observed_at_epoch_ms,
        "client_status_bar_rate_limits": build_status_bar_rate_limits_json(Some(observation)),
    })
}

async fn persist_exact_client_limit_sample(
    db: &Client,
    observation: &CodexAppServerRateLimitsObservation,
) -> Result<()> {
    let payload = build_exact_client_limit_sample_payload(observation);
    postgres::insert_observability_snapshot(db, EXACT_CLIENT_LIMIT_SAMPLE_SNAPSHOT_KIND, &payload)
        .await?;
    Ok(())
}

fn exact_client_limit_sample_from_surface(surface: &Value) -> Option<ExactClientLimitSample> {
    if surface["status"].as_str() != Some("observed") {
        return None;
    }
    let observed_at_epoch_ms = surface["observed_at_epoch_ms"]
        .as_u64()
        .or_else(|| surface["ended_at_epoch_ms"].as_u64())?;
    let primary_used_percent = surface["primary_limit_used_percent"].as_f64().or_else(|| {
        surface["primary_limit_used_percent"]
            .as_u64()
            .map(|value| value as f64)
    })?;
    let primary_window_duration_mins = surface["primary_window_duration_mins"].as_u64();
    let primary_resets_at_epoch_seconds = surface["primary_resets_at_epoch_seconds"].as_u64();
    let source = surface["source"]
        .as_str()
        .unwrap_or("codex_app_server_account_rate_limits_read_v1")
        .to_string();
    Some(ExactClientLimitSample {
        observed_at_epoch_ms,
        primary_used_percent,
        primary_window_duration_mins,
        primary_resets_at_epoch_seconds,
        source,
    })
}

fn exact_client_limit_sample_from_snapshot(
    row: &ObservabilitySnapshotRecord,
) -> Option<ExactClientLimitSample> {
    exact_client_limit_sample_from_surface(&row.payload["client_status_bar_rate_limits"])
}

fn dedup_exact_client_limit_samples(
    samples: impl IntoIterator<Item = ExactClientLimitSample>,
) -> Vec<ExactClientLimitSample> {
    let mut latest_by_observed = BTreeMap::<u64, ExactClientLimitSample>::new();
    for sample in samples {
        latest_by_observed.insert(sample.observed_at_epoch_ms, sample);
    }
    latest_by_observed.into_values().collect()
}

pub(crate) fn exact_client_limit_hourly_burn_value(
    samples: &[ExactClientLimitSample],
    now_epoch_ms: u64,
    window_minutes: u64,
    max_live_age_seconds: u64,
    min_history_span_minutes: u64,
) -> Value {
    let window_minutes = window_minutes.max(1);
    let max_live_age_seconds = max_live_age_seconds.max(1);
    let latest = match samples.last() {
        Some(sample) => sample,
        None => {
            return json!({
                "status": "missing",
                "status_bar_correlated": true,
                "source": "codex_app_server_account_rate_limits_read_v1",
                "window_minutes": window_minutes,
                "summary": "Exact 5ч source пока недоступен, поэтому KPI честно не посчитан.",
                "reply_prefix": "5ч KPI: н/д",
            });
        }
    };
    let live_age_ms = now_epoch_ms.saturating_sub(latest.observed_at_epoch_ms);
    if live_age_ms > max_live_age_seconds * 1000 {
        return json!({
            "status": "stale",
            "status_bar_correlated": true,
            "source": latest.source,
            "window_minutes": window_minutes,
            "latest_observed_at_epoch_ms": latest.observed_at_epoch_ms,
            "latest_live_age_seconds": live_age_ms as f64 / 1000.0,
            "summary": "Exact sample 5ч лимита устарел, поэтому KPI fail-closed не считается.",
            "reply_prefix": "5ч KPI: н/д",
        });
    }
    let window_duration_minutes = latest
        .primary_window_duration_mins
        .unwrap_or(window_minutes);
    let Some(primary_resets_at_epoch_seconds) = latest.primary_resets_at_epoch_seconds else {
        return json!({
            "status": "missing_reset",
            "status_bar_correlated": true,
            "source": latest.source,
            "window_minutes": window_duration_minutes,
            "latest_observed_at_epoch_ms": latest.observed_at_epoch_ms,
            "summary": "Exact 5ч source не дал reset time, поэтому KPI fail-closed не считается.",
            "reply_prefix": "5ч KPI: н/д",
        });
    };
    let reset_at_epoch_ms = primary_resets_at_epoch_seconds.saturating_mul(1000);
    let remaining_window_minutes = if reset_at_epoch_ms <= latest.observed_at_epoch_ms {
        0.0
    } else {
        (reset_at_epoch_ms - latest.observed_at_epoch_ms) as f64 / 60_000.0
    };
    let elapsed_window_minutes =
        (window_duration_minutes as f64 - remaining_window_minutes).max(0.0);
    let actual_remaining_percent = (100.0 - latest.primary_used_percent).clamp(0.0, 100.0);
    let ideal_remaining_percent = if window_duration_minutes == 0 {
        0.0
    } else {
        (remaining_window_minutes * 100.0 / window_duration_minutes as f64).clamp(0.0, 100.0)
    };
    let actual_used_percent = (100.0 - actual_remaining_percent).clamp(0.0, 100.0);
    let ideal_used_percent = (100.0 - ideal_remaining_percent).clamp(0.0, 100.0);
    let projected_primary_used_per_hour_percent = if elapsed_window_minutes <= f64::EPSILON {
        0.0
    } else {
        actual_used_percent * 60.0 / elapsed_window_minutes
    };
    let ideal_primary_used_per_hour_percent = 20.0;
    let ratio_to_ideal = if ideal_used_percent <= 0.01 {
        if actual_used_percent <= 0.01 {
            1.0
        } else {
            f64::INFINITY
        }
    } else {
        actual_used_percent / ideal_used_percent
    };
    let raw_classification = if (ratio_to_ideal - 1.0).abs() <= 0.005 {
        "one_to_one"
    } else if ratio_to_ideal > 1.0 {
        "overspend"
    } else {
        "saving"
    };
    let raw_kpi_percent = match raw_classification {
        "overspend" => (ratio_to_ideal - 1.0) * 100.0,
        "saving" => (1.0 - ratio_to_ideal) * 100.0,
        _ => 0.0,
    };
    let raw_signed_kpi_percent = match raw_classification {
        "overspend" => -raw_kpi_percent,
        "saving" => raw_kpi_percent,
        _ => 0.0,
    };
    let (signed_kpi_percent, window_progress_state, window_progress_ratio) =
        damp_signed_kpi_percent_for_window_progress(
            raw_signed_kpi_percent,
            elapsed_window_minutes,
            min_history_span_minutes,
        );
    let kpi_percent = signed_kpi_percent.abs();
    let classification = signed_kpi_classification(signed_kpi_percent);
    let reply_prefix = match classification {
        "overspend" => format!("5ч KPI: переплата {kpi_percent:.2}%"),
        "saving" => format!("5ч KPI: экономия {kpi_percent:.2}%"),
        _ => "5ч KPI: 1:1".to_string(),
    };
    let projected_full_window_minutes =
        if actual_used_percent <= 0.01 || elapsed_window_minutes <= 0.01 {
            Value::Null
        } else {
            Value::from(elapsed_window_minutes * 100.0 / actual_used_percent)
        };
    let projected_reset_delta_minutes = projected_full_window_minutes
        .as_f64()
        .map(|value| value - window_duration_minutes as f64)
        .map(Value::from)
        .unwrap_or(Value::Null);
    json!({
        "status": "observed",
        "status_bar_correlated": true,
        "source": latest.source,
        "window_minutes": window_duration_minutes,
        "latest_observed_at_epoch_ms": latest.observed_at_epoch_ms,
        "latest_live_age_seconds": live_age_ms as f64 / 1000.0,
        "window_duration_minutes": window_duration_minutes,
        "reset_at_epoch_ms": reset_at_epoch_ms,
        "remaining_window_minutes": remaining_window_minutes,
        "elapsed_window_minutes": elapsed_window_minutes,
        "actual_remaining_percent": actual_remaining_percent,
        "ideal_remaining_percent": ideal_remaining_percent,
        "actual_used_percent": actual_used_percent,
        "ideal_used_percent": ideal_used_percent,
        "projected_primary_used_per_hour_percent": projected_primary_used_per_hour_percent,
        "ideal_primary_used_per_hour_percent": ideal_primary_used_per_hour_percent,
        "equivalent_5h_budget_minutes_per_hour": projected_primary_used_per_hour_percent * 3.0,
        "projected_full_window_minutes": projected_full_window_minutes,
        "projected_reset_delta_minutes": projected_reset_delta_minutes,
        "classification": classification,
        "raw_classification": raw_classification,
        "window_progress_state": window_progress_state,
        "window_progress_ratio": window_progress_ratio,
        "minimum_elapsed_window_minutes": min_history_span_minutes,
        "raw_kpi_percent": raw_kpi_percent,
        "raw_signed_kpi_percent": raw_signed_kpi_percent,
        "kpi_percent": kpi_percent,
        "reply_prefix": reply_prefix,
        "summary": match classification {
            "overspend" => format!(
                "По текущему положению окна 5ч burn идёт быстрее нормы: использовано {actual_used_percent:.2}% вместо идеальных {ideal_used_percent:.2}% к этому моменту{}."
                ,
                if window_progress_state == "preliminary" {
                    " (раннее окно сглажено)"
                } else {
                    ""
                }
            ),
            "saving" => format!(
                "По текущему положению окна 5ч burn идёт экономно: использовано {actual_used_percent:.2}% вместо идеальных {ideal_used_percent:.2}% к этому моменту{}."
                ,
                if window_progress_state == "preliminary" {
                    " (раннее окно сглажено)"
                } else {
                    ""
                }
            ),
            _ => format!(
                "По текущему положению окна 5ч burn идёт почти один в один: использовано {actual_used_percent:.2}% при идеальных {ideal_used_percent:.2}%{}."
                ,
                if window_progress_state == "preliminary" {
                    " (раннее окно сглажено)"
                } else {
                    ""
                }
            ),
        },
    })
}

pub(crate) async fn collect_exact_client_limit_hourly_burn(
    db: &Client,
    exact_observation: Option<&CodexAppServerRateLimitsObservation>,
    persist_exact_sample: bool,
    window_minutes: u64,
    max_live_age_seconds: u64,
    min_history_span_minutes: u64,
) -> Result<Value> {
    if persist_exact_sample {
        if let Some(observation) = exact_observation {
            persist_exact_client_limit_sample(db, observation).await?;
        }
    }
    // Hourly burn surface is defined by the latest exact 5h sample only, so the
    // current-session/client-budget path should not pull the whole sample history.
    let rows = postgres::list_observability_snapshots_by_kinds(
        db,
        &[EXACT_CLIENT_LIMIT_SAMPLE_SNAPSHOT_KIND],
        Some(1),
    )
    .await?;
    let mut samples = rows
        .iter()
        .filter_map(exact_client_limit_sample_from_snapshot)
        .collect::<Vec<_>>();
    if let Some(observation) = exact_observation {
        if let Some(sample) = exact_client_limit_sample_from_surface(
            &build_status_bar_rate_limits_json(Some(observation)),
        ) {
            samples.push(sample);
        }
    }
    let samples = dedup_exact_client_limit_samples(samples);
    let now_epoch_ms = exact_observation
        .map(|observation| observation.observed_at_epoch_ms)
        .unwrap_or_else(|| current_epoch_ms().unwrap_or_default() as u64);
    Ok(exact_client_limit_hourly_burn_value(
        &samples,
        now_epoch_ms,
        window_minutes,
        max_live_age_seconds,
        min_history_span_minutes,
    ))
}

pub(crate) async fn collect_client_limit_hourly_burn_surface(
    db: &Client,
    window_minutes: u64,
    max_live_age_seconds: u64,
    min_history_span_minutes: u64,
) -> Result<Value> {
    let resolution = dashboard_exact_client_rate_limits_resolution().await?;
    collect_exact_client_limit_hourly_burn(
        db,
        resolution.observation.as_ref(),
        resolution.source.should_persist_exact_sample(),
        window_minutes,
        max_live_age_seconds,
        min_history_span_minutes,
    )
    .await
}

pub(crate) async fn collect_default_client_limit_hourly_burn_surface(db: &Client) -> Result<Value> {
    collect_client_limit_hourly_burn_surface(
        db,
        DEFAULT_CLIENT_LIMIT_HOURLY_BURN_WINDOW_MINUTES,
        DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MAX_LIVE_AGE_SECONDS,
        DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MIN_HISTORY_SPAN_MINUTES,
    )
    .await
}

fn exact_client_limit_hourly_burn_point(
    sample: &ExactClientLimitSample,
    window_minutes: u64,
) -> Value {
    exact_client_limit_hourly_burn_value(
        std::slice::from_ref(sample),
        sample.observed_at_epoch_ms,
        window_minutes,
        DEFAULT_CLIENT_LIMIT_HOURLY_BURN_MAX_LIVE_AGE_SECONDS,
        0,
    )
}

fn client_limit_trend_score(classification: Option<&str>, kpi_percent: f64) -> f64 {
    match classification {
        Some("saving") => kpi_percent,
        Some("overspend") => -kpi_percent,
        Some("one_to_one") => 0.0,
        _ => 0.0,
    }
}

pub(crate) fn client_limit_trend_direction(
    first_classification: Option<&str>,
    first_kpi_percent: f64,
    last_classification: Option<&str>,
    last_kpi_percent: f64,
) -> &'static str {
    let delta = client_limit_trend_score(last_classification, last_kpi_percent)
        - client_limit_trend_score(first_classification, first_kpi_percent);
    if delta > 0.25 {
        "toward_saving"
    } else if delta < -0.25 {
        "away_from_saving"
    } else {
        "stable"
    }
}

fn build_exact_client_limit_trend_analysis_value(
    samples: &[ExactClientLimitSample],
    now_epoch_ms: u64,
    window_minutes: u64,
    max_live_age_seconds: u64,
    lookback_minutes: u64,
) -> Value {
    let latest = match samples.last() {
        Some(sample) => sample,
        None => {
            return json!({
                "status": "missing",
                "source": "codex_app_server_account_rate_limits_read_v1",
                "status_bar_correlated": true,
                "analysis_window_minutes": lookback_minutes,
                "summary": "История exact 5ч samples пока пуста, поэтому тренд не посчитан.",
            });
        }
    };
    let latest_point = exact_client_limit_hourly_burn_value(
        samples,
        now_epoch_ms,
        window_minutes,
        max_live_age_seconds,
        0,
    );
    if latest_point["status"].as_str() != Some("observed") {
        return json!({
            "status": latest_point["status"].clone(),
            "source": latest_point["source"].clone(),
            "status_bar_correlated": true,
            "analysis_window_minutes": lookback_minutes,
            "latest_observed_at_epoch_ms": latest_point["latest_observed_at_epoch_ms"].clone(),
            "summary": "Последняя exact точка 5ч KPI сейчас не пригодна для тренд-анализа.",
            "latest_point": latest_point,
        });
    }
    let lookback_start_epoch_ms = latest
        .observed_at_epoch_ms
        .saturating_sub(lookback_minutes.saturating_mul(60_000));
    let trend_samples = samples
        .iter()
        .filter(|sample| sample.observed_at_epoch_ms >= lookback_start_epoch_ms)
        .cloned()
        .collect::<Vec<_>>();
    if trend_samples.len() < 2 {
        return json!({
            "status": "insufficient_history",
            "source": latest_point["source"].clone(),
            "status_bar_correlated": true,
            "analysis_window_minutes": lookback_minutes,
            "latest_observed_at_epoch_ms": latest.observed_at_epoch_ms,
            "sample_count": trend_samples.len(),
            "summary": format!(
                "За последние {} мин пока меньше двух exact samples, поэтому направление KPI ещё не доказывается.",
                lookback_minutes
            ),
            "latest_point": latest_point,
        });
    }
    let first = trend_samples.first().expect("first trend sample");
    let last = trend_samples.last().expect("last trend sample");
    let first_point = exact_client_limit_hourly_burn_point(first, window_minutes);
    let last_point = exact_client_limit_hourly_burn_point(last, window_minutes);
    let first_kpi_percent = first_point["kpi_percent"].as_f64().unwrap_or(0.0);
    let last_kpi_percent = last_point["kpi_percent"].as_f64().unwrap_or(0.0);
    let first_classification = first_point["classification"].as_str();
    let last_classification = last_point["classification"].as_str();
    let direction = client_limit_trend_direction(
        first_classification,
        first_kpi_percent,
        last_classification,
        last_kpi_percent,
    );
    let direction_summary = match direction {
        "toward_saving" => "KPI движется в сторону экономии.",
        "away_from_saving" => "KPI уходит от экономии и становится дороже.",
        _ => "KPI почти не меняет направление.",
    };
    let span_minutes = (last
        .observed_at_epoch_ms
        .saturating_sub(first.observed_at_epoch_ms)) as f64
        / 60_000.0;
    let delta_kpi_percent = last_kpi_percent - first_kpi_percent;
    json!({
        "status": "observed",
        "source": latest_point["source"].clone(),
        "status_bar_correlated": true,
        "analysis_window_minutes": lookback_minutes,
        "analysis_span_minutes": span_minutes,
        "sample_count": trend_samples.len(),
        "first_observed_at_epoch_ms": first.observed_at_epoch_ms,
        "last_observed_at_epoch_ms": last.observed_at_epoch_ms,
        "first_reply_prefix": first_point["reply_prefix"].clone(),
        "last_reply_prefix": last_point["reply_prefix"].clone(),
        "first_classification": first_point["classification"].clone(),
        "last_classification": last_point["classification"].clone(),
        "first_kpi_percent": first_kpi_percent,
        "last_kpi_percent": last_kpi_percent,
        "delta_kpi_percent": delta_kpi_percent,
        "trend_direction": direction,
        "summary": format!(
            "{} За последние {:.2} мин KPI был `{}` и стал `{}` (delta {:+.2} п.п.).",
            direction_summary,
            span_minutes,
            first_point["reply_prefix"].as_str().unwrap_or("5ч KPI: н/д"),
            last_point["reply_prefix"].as_str().unwrap_or("5ч KPI: н/д"),
            delta_kpi_percent
        ),
        "latest_point": latest_point,
    })
}

fn build_client_limit_trend_analysis_snapshot_payload(analysis: &Value) -> Value {
    let captured_at_epoch_ms = analysis["last_observed_at_epoch_ms"]
        .as_u64()
        .or_else(|| analysis["latest_observed_at_epoch_ms"].as_u64())
        .unwrap_or_default();
    let lookback_minutes = analysis["analysis_window_minutes"]
        .as_u64()
        .unwrap_or_default();
    json!({
        "_observability": {
            "source_event_id": format!(
                "client-limit-hourly-burn-trend-{}-{}",
                lookback_minutes,
                captured_at_epoch_ms
            ),
            "source_kind": "client_limit_hourly_burn_trend_v1",
            "captured_at_epoch_ms": captured_at_epoch_ms,
        },
        "captured_at_epoch_ms": captured_at_epoch_ms,
        "client_limit_hourly_burn_trend": analysis.clone(),
    })
}

async fn persist_client_limit_trend_analysis_snapshot(db: &Client, analysis: &Value) -> Result<()> {
    let payload = build_client_limit_trend_analysis_snapshot_payload(analysis);
    postgres::insert_observability_snapshot(
        db,
        CLIENT_LIMIT_TREND_ANALYSIS_SNAPSHOT_KIND,
        &payload,
    )
    .await?;
    Ok(())
}

pub(crate) async fn collect_exact_client_limit_trend_analysis(
    db: &Client,
    window_minutes: u64,
    max_live_age_seconds: u64,
    lookback_minutes: u64,
    persist_snapshot: bool,
) -> Result<Value> {
    let exact_resolution = dashboard_exact_client_rate_limits_resolution().await?;
    let exact_observation = exact_resolution.observation;
    if exact_resolution.source.should_persist_exact_sample() {
        if let Some(observation) = exact_observation.as_ref() {
            persist_exact_client_limit_sample(db, observation).await?;
        }
    }
    let rows = postgres::list_observability_snapshots_by_kinds(
        db,
        &[EXACT_CLIENT_LIMIT_SAMPLE_SNAPSHOT_KIND],
        Some(2048),
    )
    .await?;
    let mut samples = rows
        .iter()
        .filter_map(exact_client_limit_sample_from_snapshot)
        .collect::<Vec<_>>();
    if let Some(observation) = exact_observation.as_ref() {
        if let Some(sample) = exact_client_limit_sample_from_surface(
            &build_status_bar_rate_limits_json(Some(observation)),
        ) {
            samples.push(sample);
        }
    }
    let samples = dedup_exact_client_limit_samples(samples);
    let now_epoch_ms = exact_observation
        .as_ref()
        .map(|observation| observation.observed_at_epoch_ms)
        .unwrap_or_else(|| current_epoch_ms().unwrap_or_default() as u64);
    let analysis = build_exact_client_limit_trend_analysis_value(
        &samples,
        now_epoch_ms,
        window_minutes,
        max_live_age_seconds,
        lookback_minutes,
    );
    if persist_snapshot {
        persist_client_limit_trend_analysis_snapshot(db, &analysis).await?;
    }
    Ok(analysis)
}

pub(crate) fn build_client_live_meter_json(
    observation: Option<&codex_threads::RolloutClientMeterObservation>,
    binding_thread_id_hint: Option<&str>,
    exact_client_limits: Option<&CodexAppServerRateLimitsObservation>,
) -> Value {
    let Some(observation) = observation else {
        return json!({
            "status": "missing",
            "status_bar_rate_limits": build_status_bar_rate_limits_json(exact_client_limits),
            "note": "Текущий client-side live meter ещё не materialized из rollout token_count/rate_limits, поэтому карточки пока не могут честно показать полный turn/context pressure клиента."
        });
    };
    let (thread_binding_state, current_thread_bound) =
        client_live_meter_thread_binding_state(binding_thread_id_hint, &observation.thread_id);
    let context_used_percent = if observation.latest_model_context_window == 0 {
        None
    } else {
        Some(
            observation.client_turn_total_tokens as f64 * 100.0
                / observation.latest_model_context_window as f64,
        )
    };
    let context_remaining_tokens = observation
        .latest_model_context_window
        .saturating_sub(observation.client_turn_total_tokens);
    json!({
        "status": "observed",
        "observation_source": observation.observation_source,
        "thread_id": observation.thread_id,
        "thread_binding_state": thread_binding_state,
        "current_thread_bound": current_thread_bound,
        "turn_id": observation.turn_id,
        "started_at_epoch_ms": observation.started_at_epoch_ms,
        "ended_at_epoch_ms": observation.ended_at_epoch_ms,
        "client_turn_total_tokens": observation.client_turn_total_tokens,
        "client_turn_input_tokens": observation.client_turn_input_tokens,
        "client_turn_cached_input_tokens": observation.client_turn_cached_input_tokens,
        "client_turn_output_tokens": observation.client_turn_output_tokens,
        "client_turn_reasoning_output_tokens": observation.client_turn_reasoning_output_tokens,
        "latest_cumulative_total_tokens": observation.latest_cumulative_total_tokens,
        "latest_model_context_window": observation.latest_model_context_window,
        "context_used_percent": context_used_percent,
        "context_remaining_tokens": context_remaining_tokens,
        "primary_limit_used_percent": observation.latest_primary_limit_used_percent,
        "primary_limit_remaining_percent": 100_u64.saturating_sub(observation.latest_primary_limit_used_percent),
        "primary_window_duration_mins": observation.latest_primary_window_duration_mins,
        "primary_resets_at_epoch_seconds": observation.latest_primary_resets_at_epoch_seconds,
        "secondary_limit_used_percent": observation.latest_secondary_limit_used_percent,
        "secondary_limit_remaining_percent": 100_u64.saturating_sub(observation.latest_secondary_limit_used_percent),
        "secondary_window_duration_mins": observation.latest_secondary_window_duration_mins,
        "secondary_resets_at_epoch_seconds": observation.latest_secondary_resets_at_epoch_seconds,
        "rollout_jsonl_tolerance_summary": observation.rollout_jsonl_tolerance_summary,
        "rollout_jsonl_tolerated_skips_present": observation.rollout_jsonl_tolerance_summary.has_tolerated_skips(),
        "rollout_jsonl_malformed_objects_fail_closed": true,
        "status_bar_rate_limits": build_status_bar_rate_limits_json(exact_client_limits),
        "note": if current_thread_bound {
            "Этот surface поднимает именно live meter клиента из rollout token_count/rate_limits: per-thread 5ч/7д contour берётся из собственного workspace агента, а codex app-server status-bar source остаётся только fallback/global surface."
        } else {
            "Этот surface поднят из rollout token_count/rate_limits, но текущий thread ещё не привязан к observation. Пока не materialized current-thread meter, live-turn rows и rotate-pressure должны деградировать до unknown/stale, а не наследоваться от предыдущего thread. Codex app-server status-bar source при этом остаётся только fallback/global surface."
        }
    })
}

pub(crate) fn client_live_meter_with_exact_status_bar(
    mut client_live_meter: Value,
    exact_client_limits: Option<&CodexAppServerRateLimitsObservation>,
) -> Value {
    if let Some(root) = client_live_meter.as_object_mut() {
        root.insert(
            "status_bar_rate_limits".to_string(),
            build_status_bar_rate_limits_json(exact_client_limits),
        );
    }
    client_live_meter
}
