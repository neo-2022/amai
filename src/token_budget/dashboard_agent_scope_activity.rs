use super::*;

fn fallback_project_code_from_repo_root_hint(repo_root_hint: Option<&str>) -> Option<String> {
    let raw_name = repo_root_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| Path::new(value).file_name())
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let mut normalized = String::new();
    let mut previous_was_separator = false;
    for ch in raw_name.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            normalized.push('_');
            previous_was_separator = true;
        }
    }
    let normalized = normalized.trim_matches('_').to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn dedup_activity_items_by_thread_key(items: Vec<Value>, thread_key: &str) -> Vec<Value> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for item in items {
        let Some(thread_id) = item[thread_key]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if !seen.insert(thread_id.to_string()) {
            continue;
        }
        deduped.push(item);
    }
    deduped
}

async fn latest_recent_scope_for_repo_root(db: &Client, repo_root: &str) -> Result<Option<Value>> {
    let repo_root = repo_root.trim();
    if repo_root.is_empty() {
        return Ok(None);
    }
    let Ok(project) = postgres::get_project_by_repo_root(db, repo_root).await else {
        return Ok(None);
    };
    let Some(snapshot) = postgres::latest_observability_snapshot_for_project(
        db,
        "working_state_restore",
        "working_state_restore",
        &project.code,
    )
    .await?
    else {
        return Ok(None);
    };
    let restore = &snapshot["working_state_restore"];
    let Some(thread_id) = restore["thread_id"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let namespace_code = restore["namespace"]["code"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("continuity");
    let agent_scope = restore["agent_scope"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| default_agent_scope_label(&project.code, namespace_code));
    let captured_at_epoch_ms = json_i64(&restore["captured_at_epoch_ms"])
        .or_else(|| json_i64(&restore["recorded_at_epoch_ms"]))
        .unwrap_or_default();
    Ok(Some(json!({
        "project_code": project.code,
        "namespace_code": namespace_code,
        "project_repo_root": project.repo_root,
        "agent_scope": agent_scope,
        "thread_id": thread_id,
        "current_goal": restore["current_goal"].clone(),
        "captured_at_epoch_ms": captured_at_epoch_ms,
    })))
}

pub(super) fn active_agent_activity_entries(activity: &Value, now_epoch_ms: i64) -> Vec<Value> {
    let recent_threads = activity["client_recent_threads"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|thread| recent_client_thread_json_has_connected_model(thread))
        .filter_map(|thread| {
            let thread_id = thread["thread_id"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())?;
            Some((thread_id.to_string(), thread.clone()))
        })
        .collect::<HashMap<_, _>>();
    let connected_thread_ids = recent_threads.keys().cloned().collect::<BTreeSet<_>>();
    let mut entries = activity["active_now_scopes"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|item| {
            let Some(owner_thread_id) = item["owner_thread_id"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                return false;
            };
            if !connected_thread_ids.contains(owner_thread_id) {
                return false;
            }
            !user_visible_agent_activity_is_proof_runtime(
                item["project_code"].as_str(),
                item["agent_scope"].as_str(),
                Some(owner_thread_id),
                item["headline"].as_str(),
                None,
            )
        })
        .collect::<Vec<_>>();
    let mut active_thread_ids = entries
        .iter()
        .filter_map(|item| {
            item["owner_thread_id"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();
    for scope in activity["recent_scopes"].as_array().into_iter().flatten() {
        let Some(thread_id) = scope["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if active_thread_ids.contains(thread_id) {
            continue;
        }
        let Some(thread) = recent_threads.get(thread_id) else {
            continue;
        };
        let Some(updated_at_epoch_ms) = json_i64(&thread["updated_at_epoch_ms"]) else {
            continue;
        };
        if now_epoch_ms.saturating_sub(updated_at_epoch_ms)
            > ACTIVE_AGENT_RECENT_THREAD_FALLBACK_MAX_AGE_MS
        {
            continue;
        }
        let Some(agent_scope) = scope["agent_scope"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if user_visible_agent_activity_is_proof_runtime(
            scope["project_code"].as_str(),
            Some(agent_scope),
            Some(thread_id),
            scope["current_goal"].as_str(),
            thread["title"].as_str(),
        ) {
            continue;
        }
        entries.push(json!({
            "project_code": scope["project_code"].clone(),
            "namespace_code": scope["namespace_code"].clone(),
            "project_repo_root": thread["cwd"].clone(),
            "agent_scope": agent_scope,
            "owner_thread_id": thread_id,
            "heartbeat_at_epoch_ms": updated_at_epoch_ms,
            "expires_at_epoch_ms": updated_at_epoch_ms + ACTIVE_AGENT_RECENT_THREAD_FALLBACK_MAX_AGE_MS,
            "headline": scope["current_goal"].clone(),
            "activity_source": "recent_thread_binding_fallback",
        }));
        active_thread_ids.insert(thread_id.to_string());
    }
    for (thread_id, thread) in &recent_threads {
        if active_thread_ids.contains(thread_id) {
            continue;
        }
        let Some(updated_at_epoch_ms) = json_i64(&thread["updated_at_epoch_ms"]) else {
            continue;
        };
        if now_epoch_ms.saturating_sub(updated_at_epoch_ms)
            > ACTIVE_AGENT_RECENT_THREAD_FALLBACK_MAX_AGE_MS
        {
            continue;
        }
        if user_visible_agent_activity_is_proof_runtime(
            None,
            None,
            Some(thread_id.as_str()),
            thread["title"].as_str(),
            thread["agent_nickname"]
                .as_str()
                .or_else(|| thread["agent_role"].as_str())
                .or_else(|| thread["title"].as_str()),
        ) {
            continue;
        }
        let project_repo_root = thread["cwd"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        let Some(project_code) =
            fallback_project_code_from_repo_root_hint(project_repo_root.as_deref())
        else {
            continue;
        };
        entries.push(json!({
            "project_code": project_code.clone(),
            "namespace_code": "continuity",
            "project_repo_root": project_repo_root,
            "agent_scope": format!("{project_code}::continuity::default"),
            "owner_thread_id": thread_id,
            "heartbeat_at_epoch_ms": updated_at_epoch_ms,
            "expires_at_epoch_ms": updated_at_epoch_ms + ACTIVE_AGENT_RECENT_THREAD_FALLBACK_MAX_AGE_MS,
            "headline": thread["title"].clone(),
            "activity_source": "recent_thread_unbound_fallback",
        }));
        active_thread_ids.insert(thread_id.to_string());
    }
    entries
}

pub(crate) fn active_agent_thread_ids_from_activity(
    activity: &Value,
    now_epoch_ms: i64,
) -> Vec<String> {
    let mut thread_ids = BTreeSet::new();
    for active in active_agent_activity_entries(activity, now_epoch_ms) {
        if let Some(thread_id) = active["owner_thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            thread_ids.insert(thread_id.to_string());
        }
    }
    thread_ids.into_iter().collect()
}

pub(crate) async fn collect_agent_scope_activity(db: &Client) -> Result<Value> {
    let now_epoch_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before unix epoch")?
        .as_millis() as i64;
    let recent_window_hours = 24_i64;
    let client_recent_window_minutes = 30_i64;
    let client_recent_threads =
        codex_threads::recent_client_thread_records(client_recent_window_minutes * 60)?
            .into_iter()
            .filter(|item| {
                recent_client_thread_record_has_connected_model(item)
                    && !user_visible_agent_activity_is_proof_runtime(
                        None,
                        None,
                        Some(item.thread_id.as_str()),
                        Some(item.title.as_str()),
                        item.agent_nickname
                            .as_deref()
                            .or(item.agent_role.as_deref()),
                    )
            })
            .map(|item| {
                json!({
                    "thread_id": item.thread_id,
                    "cwd": item.cwd,
                    "rollout_path": item.rollout_path,
                    "title": item.title,
                    "agent_nickname": item.agent_nickname,
                    "agent_role": item.agent_role,
                    "model_provider": item.model_provider,
                    "model": item.model,
                    "reasoning_effort": item.reasoning_effort,
                    "updated_at_epoch_ms": item.updated_at_epoch_s.saturating_mul(1000),
                })
            })
            .collect::<Vec<_>>();
    let connected_thread_ids = client_recent_threads
        .iter()
        .filter_map(|item| {
            item["thread_id"]
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<BTreeSet<_>>();

    let active_rows = db
        .query(
            r#"
            SELECT
                projects.code AS project_code,
                namespaces.code AS namespace_code,
                projects.repo_root AS project_repo_root,
                leases.agent_scope,
                leases.owner_thread_id,
                leases.heartbeat_at_epoch_ms,
                leases.expires_at_epoch_ms,
                leases.headline
            FROM ami.execctl_task_leases AS leases
            LEFT JOIN ami.projects AS projects
              ON projects.project_id = leases.project_id
            LEFT JOIN ami.namespaces AS namespaces
              ON namespaces.namespace_id = leases.namespace_id
            WHERE lease_state = 'active'
              AND leases.expires_at_epoch_ms > $1
            ORDER BY leases.heartbeat_at_epoch_ms DESC, leases.agent_scope ASC
            LIMIT 64
            "#,
            &[&now_epoch_ms],
        )
        .await
        .context("failed to query active execctl task leases for agent scope activity")?;
    let mut active_now_scopes = dedup_activity_items_by_thread_key(
        active_rows
            .into_iter()
            .filter_map(|row| {
                let agent_scope = row.get::<_, String>(3);
                let owner_thread_id = row.get::<_, Option<String>>(4);
                let headline = row.get::<_, Option<String>>(7);
                let Some(owner_thread_id) = owner_thread_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .filter(|value| connected_thread_ids.contains(*value))
                    .map(str::to_string)
                else {
                    return None;
                };
                if user_visible_agent_activity_is_proof_runtime(
                    row.get::<_, Option<String>>(0).as_deref(),
                    Some(agent_scope.as_str()),
                    Some(owner_thread_id.as_str()),
                    headline.as_deref(),
                    None,
                ) {
                    return None;
                }
                Some(json!({
                    "project_code": row.get::<_, Option<String>>(0),
                    "namespace_code": row.get::<_, Option<String>>(1),
                    "project_repo_root": row.get::<_, Option<String>>(2),
                    "agent_scope": agent_scope,
                    "owner_thread_id": owner_thread_id,
                    "heartbeat_at_epoch_ms": row.get::<_, i64>(5),
                    "expires_at_epoch_ms": row.get::<_, i64>(6),
                    "headline": headline,
                }))
            })
            .collect::<Vec<_>>(),
        "owner_thread_id",
    );

    let mut visible_repo_roots = BTreeSet::new();
    for thread in &client_recent_threads {
        let Some(repo_root) = thread["cwd"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        visible_repo_roots.insert(repo_root.to_string());
    }
    let mut recent_scope_items = Vec::new();
    for repo_root in visible_repo_roots {
        let Some(scope) = latest_recent_scope_for_repo_root(db, &repo_root).await? else {
            continue;
        };
        let Some(thread_id) = scope["thread_id"]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        if !connected_thread_ids.contains(thread_id) {
            continue;
        }
        if now_epoch_ms.saturating_sub(json_i64(&scope["captured_at_epoch_ms"]).unwrap_or_default())
            > recent_window_hours * 60 * 60 * 1000
        {
            continue;
        }
        if user_visible_agent_activity_is_proof_runtime(
            scope["project_code"].as_str(),
            scope["agent_scope"].as_str(),
            Some(thread_id),
            scope["current_goal"].as_str(),
            None,
        ) {
            continue;
        }
        recent_scope_items.push(scope);
    }
    recent_scope_items.sort_by(|left, right| {
        json_i64(&right["captured_at_epoch_ms"]).cmp(&json_i64(&left["captured_at_epoch_ms"]))
    });
    let mut recent_scopes = dedup_activity_items_by_thread_key(recent_scope_items, "thread_id");
    let activity_agent_scopes = active_now_scopes
        .iter()
        .chain(recent_scopes.iter())
        .filter_map(|item| item["agent_scope"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    let agent_display_name_overrides =
        load_agent_display_name_overrides_for_scopes(db, activity_agent_scopes).await?;
    for item in active_now_scopes.iter_mut().chain(recent_scopes.iter_mut()) {
        let Some(agent_scope) = item["agent_scope"].as_str().map(str::trim) else {
            continue;
        };
        let Some(display_name) = agent_display_name_overrides.get(agent_scope) else {
            continue;
        };
        if let Some(root) = item.as_object_mut() {
            root.insert(
                "agent_display_name".to_string(),
                Value::String(display_name.clone()),
            );
        }
    }

    Ok(json!({
        "source": "observe_agent_scope_activity_v2",
        "captured_at_epoch_ms": now_epoch_ms,
        "client_recent_window_minutes": client_recent_window_minutes,
        "client_recent_thread_count": client_recent_threads.len(),
        "client_recent_threads": client_recent_threads,
        "active_now_count": active_now_scopes.len(),
        "active_now_scopes": active_now_scopes,
        "recent_scope_window_hours": recent_window_hours,
        "recent_scope_count": recent_scopes.len(),
        "recent_scopes": recent_scopes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn active_agent_activity_entries_adds_recent_thread_fallback_when_lease_missing() {
        let activity = json!({
            "active_now_scopes": [
                {
                    "project_code": "amai",
                    "namespace_code": "continuity",
                    "project_repo_root": "/home/art/agent-memory-index",
                    "agent_scope": "amai::continuity::default",
                    "owner_thread_id": "thread-amai",
                    "heartbeat_at_epoch_ms": 9_900,
                    "expires_at_epoch_ms": 20_000,
                    "headline": "live"
                }
            ],
            "client_recent_threads": [
                {
                    "thread_id": "thread-amai",
                    "cwd": "/home/art/agent-memory-index",
                    "model": "gpt-5.4",
                    "updated_at_epoch_ms": 9_900
                },
                {
                    "thread_id": "thread-bounty",
                    "cwd": "/home/art/Bug-Bounty",
                    "model": "gpt-5.4",
                    "updated_at_epoch_ms": 9_800
                }
            ],
            "recent_scopes": [
                {
                    "project_code": "bug_bounty",
                    "namespace_code": "continuity",
                    "agent_scope": "bug_bounty::continuity::default",
                    "thread_id": "thread-bounty",
                    "current_goal": "recent fallback"
                }
            ]
        });

        let entries = active_agent_activity_entries(&activity, 10_000);

        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[1]["agent_scope"],
            json!("bug_bounty::continuity::default")
        );
        assert_eq!(entries[1]["owner_thread_id"], json!("thread-bounty"));
        assert_eq!(
            entries[1]["activity_source"],
            json!("recent_thread_binding_fallback")
        );
        assert_eq!(
            entries[1]["project_repo_root"],
            json!("/home/art/Bug-Bounty")
        );
    }

    #[test]
    fn active_agent_activity_entries_adds_unbound_recent_thread_fallback_when_binding_missing() {
        let activity = json!({
            "active_now_scopes": [
                {
                    "project_code": "amai",
                    "namespace_code": "continuity",
                    "project_repo_root": "/home/art/agent-memory-index",
                    "agent_scope": "amai::continuity::default",
                    "owner_thread_id": "thread-amai",
                    "heartbeat_at_epoch_ms": 9_900,
                    "expires_at_epoch_ms": 20_000,
                    "headline": "live"
                }
            ],
            "client_recent_threads": [
                {
                    "thread_id": "thread-amai",
                    "cwd": "/home/art/agent-memory-index",
                    "title": "дальше работай",
                    "model": "gpt-5.4",
                    "updated_at_epoch_ms": 9_900
                },
                {
                    "thread_id": "thread-bounty",
                    "cwd": "/home/art/Bug-Bounty",
                    "title": "Авито продолжаем. Проверь возможность записи в Амаи",
                    "model": "gpt-5.4",
                    "updated_at_epoch_ms": 9_800
                }
            ],
            "recent_scopes": []
        });

        let entries = active_agent_activity_entries(&activity, 10_000);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1]["project_code"], json!("bug_bounty"));
        assert_eq!(
            entries[1]["agent_scope"],
            json!("bug_bounty::continuity::default")
        );
        assert_eq!(entries[1]["owner_thread_id"], json!("thread-bounty"));
        assert_eq!(
            entries[1]["activity_source"],
            json!("recent_thread_unbound_fallback")
        );
        assert_eq!(
            entries[1]["project_repo_root"],
            json!("/home/art/Bug-Bounty")
        );
    }

    #[test]
    fn active_agent_activity_entries_skip_proof_runtime_entries() {
        let activity = json!({
            "active_now_scopes": [
                {
                    "project_code": "amai",
                    "namespace_code": "continuity",
                    "project_repo_root": "/home/art/agent-memory-index",
                    "agent_scope": "proof_execctl_restore_primary_123",
                    "owner_thread_id": "proof-execctl-restore-primary-123",
                    "heartbeat_at_epoch_ms": 9_900,
                    "expires_at_epoch_ms": 20_000,
                    "headline": "proof live"
                },
                {
                    "project_code": "amai",
                    "namespace_code": "continuity",
                    "project_repo_root": "/home/art/agent-memory-index",
                    "agent_scope": "amai::continuity::default",
                    "owner_thread_id": "thread-amai",
                    "heartbeat_at_epoch_ms": 9_950,
                    "expires_at_epoch_ms": 20_000,
                    "headline": "real live"
                },
                {
                    "project_code": "execctl_restore_stress_123",
                    "namespace_code": "continuity",
                    "project_repo_root": "/tmp/proof",
                    "agent_scope": "shared",
                    "owner_thread_id": "thread-proofish",
                    "heartbeat_at_epoch_ms": 9_960,
                    "expires_at_epoch_ms": 20_000,
                    "headline": "Execctl Restore Stress 123"
                }
            ],
            "client_recent_threads": [
                {
                    "thread_id": "thread-amai",
                    "cwd": "/home/art/agent-memory-index",
                    "model": "gpt-5.4",
                    "updated_at_epoch_ms": 9_800
                },
                {
                    "thread_id": "proof-execctl-restore-foreign-123",
                    "cwd": "/tmp/proof",
                    "model": null,
                    "updated_at_epoch_ms": 9_800
                },
                {
                    "thread_id": "thread-bounty",
                    "cwd": "/home/art/Bug-Bounty",
                    "model": "gpt-5.4",
                    "updated_at_epoch_ms": 9_800
                }
            ],
            "recent_scopes": [
                {
                    "project_code": "execctl_restore_stress",
                    "namespace_code": "continuity",
                    "agent_scope": "proof_execctl_restore_foreign_123",
                    "thread_id": "proof-execctl-restore-foreign-123",
                    "current_goal": "proof fallback"
                },
                {
                    "project_code": "bug_bounty",
                    "namespace_code": "continuity",
                    "agent_scope": "bug_bounty::continuity::default",
                    "thread_id": "thread-bounty",
                    "current_goal": "recent fallback"
                }
            ]
        });

        let entries = active_agent_activity_entries(&activity, 10_000);

        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0]["agent_scope"],
            json!("amai::continuity::default")
        );
        assert_eq!(
            entries[1]["agent_scope"],
            json!("bug_bounty::continuity::default")
        );
    }

    #[test]
    fn active_agent_activity_entries_skip_threads_without_connected_model() {
        let activity = json!({
            "active_now_scopes": [
                {
                    "project_code": "amai",
                    "namespace_code": "continuity",
                    "project_repo_root": "/home/art/agent-memory-index",
                    "agent_scope": "amai::continuity::default",
                    "owner_thread_id": "thread-amai",
                    "heartbeat_at_epoch_ms": 9_950,
                    "expires_at_epoch_ms": 20_000,
                    "headline": "real live"
                },
                {
                    "project_code": "execctl_restore_stress_123",
                    "namespace_code": "continuity",
                    "project_repo_root": "/tmp/proof",
                    "agent_scope": "execctl_restore_stress_123::continuity::default",
                    "owner_thread_id": "thread-stress",
                    "heartbeat_at_epoch_ms": 9_960,
                    "expires_at_epoch_ms": 20_000,
                    "headline": "Execctl Restore Stress 123"
                }
            ],
            "client_recent_threads": [
                {
                    "thread_id": "thread-amai",
                    "cwd": "/home/art/agent-memory-index",
                    "title": "дальше работай",
                    "model": "gpt-5.4",
                    "updated_at_epoch_ms": 9_900
                },
                {
                    "thread_id": "thread-stress",
                    "cwd": "/tmp/proof",
                    "title": "Execctl Restore Stress 123",
                    "model": null,
                    "updated_at_epoch_ms": 9_900
                }
            ],
            "recent_scopes": [
                {
                    "project_code": "execctl_restore_stress_123",
                    "namespace_code": "continuity",
                    "agent_scope": "execctl_restore_stress_123::continuity::default",
                    "thread_id": "thread-stress",
                    "current_goal": "Execctl Restore Stress 123"
                }
            ]
        });

        let entries = active_agent_activity_entries(&activity, 10_000);

        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0]["agent_scope"],
            json!("amai::continuity::default")
        );
    }

    #[test]
    fn active_agent_thread_ids_from_activity_returns_only_connected_active_threads() {
        let activity = json!({
            "active_now_scopes": [
                {
                    "project_code": "amai",
                    "namespace_code": "continuity",
                    "project_repo_root": "/home/art/agent-memory-index",
                    "agent_scope": "amai::continuity::default",
                    "owner_thread_id": "thread-amai",
                    "heartbeat_at_epoch_ms": 9_950,
                    "expires_at_epoch_ms": 20_000,
                    "headline": "real live"
                },
                {
                    "project_code": "proof_execctl_restore",
                    "namespace_code": "continuity",
                    "project_repo_root": "/tmp/proof",
                    "agent_scope": "proof_execctl_restore::continuity::default",
                    "owner_thread_id": "thread-proof",
                    "heartbeat_at_epoch_ms": 9_960,
                    "expires_at_epoch_ms": 20_000,
                    "headline": "proof live"
                }
            ],
            "client_recent_threads": [
                {
                    "thread_id": "thread-amai",
                    "cwd": "/home/art/agent-memory-index",
                    "title": "дальше работай",
                    "model": "gpt-5.4",
                    "updated_at_epoch_ms": 9_900
                },
                {
                    "thread_id": "thread-proof",
                    "cwd": "/tmp/proof",
                    "title": "Execctl Restore Stress 123",
                    "model": "gpt-5.4",
                    "updated_at_epoch_ms": 9_900
                },
                {
                    "thread_id": "thread-bounty",
                    "cwd": "/home/art/Bug-Bounty",
                    "title": "Авито",
                    "model": "gpt-5.4",
                    "updated_at_epoch_ms": 9_800
                }
            ],
            "recent_scopes": [
                {
                    "project_code": "bug_bounty",
                    "namespace_code": "continuity",
                    "agent_scope": "bug_bounty::continuity::default",
                    "thread_id": "thread-bounty",
                    "current_goal": "recent fallback"
                }
            ]
        });

        let thread_ids = active_agent_thread_ids_from_activity(&activity, 10_000);

        assert_eq!(
            thread_ids,
            vec!["thread-amai".to_string(), "thread-bounty".to_string()]
        );
    }
}
