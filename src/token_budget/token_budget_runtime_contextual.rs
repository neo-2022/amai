use super::*;

fn continuity_profile_enabled() -> bool {
    std::env::var("AMAI_PROFILE_CONTINUITY")
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            !normalized.is_empty() && normalized != "0" && normalized != "false"
        })
        .unwrap_or(false)
}

pub(crate) fn continuity_profile_log(stage: &str, elapsed_ms: u128, extra: &str) {
    if continuity_profile_enabled() {
        eprintln!("[amai-continuity-profile] stage={stage} elapsed_ms={elapsed_ms} {extra}");
    }
}

pub(crate) async fn load_agent_display_name_overrides_for_scopes(
    db: &Client,
    scopes: impl IntoIterator<Item = String>,
) -> Result<HashMap<String, String>> {
    let mut overrides = HashMap::new();
    let mut seen = BTreeSet::new();
    for scope in scopes {
        let scope = scope.trim();
        if scope.is_empty() || !seen.insert(scope.to_string()) {
            continue;
        }
        if let Some(display_name) = postgres::find_agent_display_name_by_code(db, scope).await? {
            overrides.insert(scope.to_string(), display_name);
        }
    }
    Ok(overrides)
}

pub(crate) fn recent_client_thread_record_has_connected_model(
    thread: &codex_threads::RecentClientThreadRecord,
) -> bool {
    thread
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
}

pub(crate) fn recent_client_thread_json_has_connected_model(thread: &Value) -> bool {
    thread["model"]
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
}

pub(crate) fn json_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().map(|value| value as i64))
}

pub(crate) async fn preferred_dashboard_thread_binding_hint_with_override(
    db: &Client,
    repo_root: &Path,
    explicit_thread_id_hint: Option<&str>,
) -> Result<Option<String>> {
    if let Some(thread_id) = explicit_thread_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(Some(thread_id.to_string()));
    }
    preferred_dashboard_thread_binding_hint(db, repo_root).await
}

pub(crate) async fn preferred_rollout_client_meter_observation(
    _db: &Client,
    _repo_root: &Path,
    repo_root_str: &str,
    preferred_thread_id_hint: Option<&str>,
) -> Result<Option<codex_threads::RolloutClientMeterObservation>> {
    if let Some(thread_id) = preferred_thread_id_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if let Some(observation) =
            codex_threads::latest_rollout_client_meter_observation_for_thread(thread_id)?
        {
            return Ok(Some(observation));
        }
    }

    codex_threads::latest_rollout_client_meter_observation(repo_root_str, None)
}

pub(crate) async fn live_turn_retrieval_context_pack_ids(
    repo_root: &Path,
    db: &Client,
    thread_id: &str,
    started_at_epoch_ms: i64,
    ended_at_epoch_ms: i64,
    grace_ms: i64,
) -> Result<(BTreeSet<String>, u64)> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Ok((BTreeSet::new(), 0));
    }
    let Some((lower_bound, upper_bound)) = current_live_turn_context_pack_match_bounds(
        started_at_epoch_ms,
        ended_at_epoch_ms,
        grace_ms,
    ) else {
        return Ok((BTreeSet::new(), 0));
    };
    let invalidation_epoch_ms =
        current_dashboard_live_turn_retrieval_invalidation_epoch_ms(repo_root);
    if let Some(cached) = cached_dashboard_live_turn_retrieval(
        repo_root,
        thread_id,
        lower_bound,
        upper_bound,
        invalidation_epoch_ms,
    ) {
        return Ok(cached);
    }
    let rows = db
        .query(
            "
            SELECT
                payload->'working_state_event'->>'context_pack_id' AS context_pack_id
            FROM ami.observability_snapshots
            WHERE snapshot_kind = 'working_state_event'
              AND payload->'working_state_event'->>'event_kind' = 'retrieval_context_pack'
              AND payload->'working_state_event'->>'thread_id' = $1
              AND captured_at_epoch_ms BETWEEN $2 AND $3
              AND (
                    payload->'working_state_event'->>'traffic_class' = 'live'
                    OR payload->'working_state_event'->>'token_source_kind' LIKE 'live\\_%' ESCAPE '\\'
                  )
            ORDER BY captured_at_epoch_ms DESC, created_at DESC
            ",
            &[&thread_id, &lower_bound, &upper_bound],
        )
        .await?;
    let retrieval_count = rows.len() as u64;
    let context_pack_ids = rows
        .into_iter()
        .filter_map(|row| row.get::<_, Option<String>>("context_pack_id"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    store_dashboard_live_turn_retrieval(
        repo_root,
        thread_id,
        lower_bound,
        upper_bound,
        invalidation_epoch_ms,
        &context_pack_ids,
        retrieval_count,
    );
    Ok((context_pack_ids, retrieval_count))
}

pub(crate) async fn recent_thread_live_retrieval_context_pack_ids_after_turn(
    db: &Client,
    thread_id: &str,
    ended_at_epoch_ms: i64,
    grace_ms: i64,
) -> Result<(BTreeSet<String>, u64)> {
    let thread_id = thread_id.trim();
    if thread_id.is_empty() {
        return Ok((BTreeSet::new(), 0));
    }
    let lower_bound = ended_at_epoch_ms.saturating_add(1);
    let upper_bound = current_epoch_ms()
        .unwrap_or(ended_at_epoch_ms)
        .saturating_add(grace_ms.max(0));
    if lower_bound <= 0 || upper_bound < lower_bound {
        return Ok((BTreeSet::new(), 0));
    }
    let rows = db
        .query(
            "
            SELECT
                payload->'working_state_event'->>'context_pack_id' AS context_pack_id
            FROM ami.observability_snapshots
            WHERE snapshot_kind = 'working_state_event'
              AND payload->'working_state_event'->>'event_kind' = 'retrieval_context_pack'
              AND payload->'working_state_event'->>'thread_id' = $1
              AND captured_at_epoch_ms BETWEEN $2 AND $3
              AND (
                    payload->'working_state_event'->>'traffic_class' = 'live'
                    OR payload->'working_state_event'->>'token_source_kind' LIKE 'live\\_%' ESCAPE '\\'
                  )
            ORDER BY captured_at_epoch_ms DESC, created_at DESC
            ",
            &[&thread_id, &lower_bound, &upper_bound],
        )
        .await?;
    let retrieval_count = rows.len() as u64;
    let context_pack_ids = rows
        .into_iter()
        .filter_map(|row| row.get::<_, Option<String>>("context_pack_id"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    Ok((context_pack_ids, retrieval_count))
}

pub(crate) fn apply_open_turn_pending_activity_surface(
    surface: &mut Value,
    pending_context_pack_ids_count: u64,
    pending_retrieval_context_pack_count: u64,
) -> bool {
    if pending_retrieval_context_pack_count == 0 {
        return false;
    }
    surface["status"] = Value::from("thread_activity_observed_turn_open");
    surface["matched_context_pack_ids_count"] = Value::from(pending_context_pack_ids_count);
    surface["retrieval_context_pack_count"] = Value::from(pending_retrieval_context_pack_count);
    surface["note"] = Value::from(
        "На текущем thread уже observed новые retrieval_context_pack от Amai после последнего завершённого client-meter turn. Exact pair materialize-ится после закрытия текущего turn, поэтому вклад Amai уже виден как thread activity, но ещё не сводится в same-turn exact pair.",
    );
    true
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn working_state_retrieval_context_pack_is_live(
    traffic_class: Option<&str>,
    token_source_kind: Option<&str>,
) -> bool {
    if traffic_class.is_some_and(|value| value.trim() == "live") {
        return true;
    }
    token_source_kind.is_some_and(|value| derive_traffic_class(value.trim()) == "live")
}

pub(crate) fn current_live_turn_context_pack_match_grace_ms() -> i64 {
    ASSISTANT_GENERATION_TURN_MATCH_GRACE_MS
}

pub(crate) fn current_live_turn_context_pack_match_bounds(
    started_at_epoch_ms: i64,
    ended_at_epoch_ms: i64,
    grace_ms: i64,
) -> Option<(i64, i64)> {
    let upper_bound = ended_at_epoch_ms.max(started_at_epoch_ms);
    if started_at_epoch_ms <= 0 || upper_bound <= 0 {
        return None;
    }
    let grace_ms = grace_ms.max(0);
    Some((
        started_at_epoch_ms.saturating_sub(grace_ms),
        upper_bound.saturating_add(grace_ms),
    ))
}

pub(crate) fn percent_from_signed(saved_tokens: i64, baseline_tokens: u64) -> f64 {
    if baseline_tokens == 0 {
        0.0
    } else {
        saved_tokens as f64 * 100.0 / baseline_tokens as f64
    }
}

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub(crate) fn collect_naive_scope(
    payload: &Value,
    limit_files: usize,
    max_bytes_per_file: usize,
    baseline_strategy: &str,
    query: &str,
) -> Result<NaiveScope> {
    let mut files = Vec::new();
    let strategy_files =
        collect_payload_scope_files_by_strategy(payload, baseline_strategy, limit_files)?;
    if !strategy_files.is_empty() {
        for (project_code, repo_root, path) in strategy_files {
            files.push(read_scope_file(
                &project_code,
                &repo_root,
                &path,
                max_bytes_per_file,
            )?);
        }
    } else {
        for project in payload["visible_projects"].as_array().into_iter().flatten() {
            let Some(project_code) = project["project_code"].as_str() else {
                continue;
            };
            let Some(repo_root) = project["repo_root"].as_str() else {
                continue;
            };
            for path in collect_scope_files_by_strategy(
                Path::new(repo_root),
                query,
                baseline_strategy,
                limit_files,
                max_bytes_per_file.min(16 * 1024),
            )? {
                files.push(read_scope_file(
                    project_code,
                    Path::new(repo_root),
                    &path,
                    max_bytes_per_file,
                )?);
            }
        }
    }

    files.sort_by(|left, right| {
        left.project_code
            .cmp(&right.project_code)
            .then_with(|| left.relative_path.cmp(&right.relative_path))
    });
    if limit_files > 0 {
        files.truncate(limit_files);
    }

    let metadata = files
        .iter()
        .map(|file| {
            json!({
                "project_code": file.project_code,
                "relative_path": file.relative_path,
                "original_bytes": file.original_bytes,
                "bytes_used": file.bytes_used,
                "truncated": file.truncated,
            })
        })
        .collect();

    Ok(NaiveScope {
        files: metadata,
        rendered_files: files,
    })
}

fn read_scope_file(
    project_code: &str,
    repo_root: &Path,
    path: &Path,
    max_bytes_per_file: usize,
) -> Result<NaiveScopeFile> {
    let relative_path = path
        .strip_prefix(repo_root)
        .unwrap_or(path)
        .display()
        .to_string();
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read naive scope file {}", path.display()))?;
    let original_bytes = bytes.len();
    let bytes_used = original_bytes.min(max_bytes_per_file);
    let content = safe_lossy_prefix(&bytes, bytes_used);
    Ok(NaiveScopeFile {
        project_code: project_code.to_string(),
        relative_path,
        original_bytes,
        bytes_used: content.len(),
        truncated: original_bytes > content.len(),
        content,
    })
}

pub(crate) fn collect_payload_scope_files_by_strategy(
    payload: &Value,
    baseline_strategy: &str,
    limit_files: usize,
) -> Result<Vec<(String, PathBuf, PathBuf)>> {
    let sections: &[&str] = match baseline_strategy {
        "ide_search_top_files" => &["exact_documents", "symbol_hits", "lexical_chunks"],
        "semantic_top_k" => &["semantic_chunks"],
        _ => return Ok(Vec::new()),
    };
    let repo_roots = payload["visible_projects"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|project| {
            Some((
                project["project_code"].as_str()?.to_string(),
                PathBuf::from(project["repo_root"].as_str()?),
            ))
        })
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut files = Vec::new();
    for section in sections {
        for item in payload["retrieval"][section]
            .as_array()
            .into_iter()
            .flatten()
        {
            let Some(project_code) = ledger_item_project_code(item) else {
                continue;
            };
            let Some(relative_path) = ledger_item_relative_path(item) else {
                continue;
            };
            let Some(repo_root) = repo_roots.get(project_code) else {
                continue;
            };
            let path = repo_root.join(relative_path);
            if !path.is_file() {
                continue;
            }
            if seen.insert(format!("{project_code}::{relative_path}")) {
                files.push((project_code.to_string(), repo_root.clone(), path));
            }
        }
    }
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

pub(crate) fn collect_scope_files_by_strategy(
    root: &Path,
    query: &str,
    baseline_strategy: &str,
    limit_files: usize,
    score_bytes_per_file: usize,
) -> Result<Vec<PathBuf>> {
    match baseline_strategy {
        "grep_top_files" => {
            collect_grep_scope_files(root, query, limit_files, score_bytes_per_file)
        }
        "legacy_pre_amai" => {
            collect_legacy_scope_files(root, query, limit_files, score_bytes_per_file)
        }
        _ => collect_scope_files(root, limit_files),
    }
}

pub(crate) fn collect_scope_files(root: &Path, limit_files: usize) -> Result<Vec<PathBuf>> {
    if !root.exists() {
        bail!("visible project root does not exist: {}", root.display());
    }
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(true)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(true);
    let mut files = builder
        .build()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .map(|entry| entry.into_path())
        .filter(|path| language::detect(path).is_some())
        .collect::<Vec<_>>();
    files.sort();
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn collect_grep_scope_files(
    root: &Path,
    query: &str,
    limit_files: usize,
    score_bytes_per_file: usize,
) -> Result<Vec<PathBuf>> {
    let files = collect_scope_files(root, 0)?;
    let terms = extract_query_terms(query);
    if terms.is_empty() {
        return collect_scope_files(root, limit_files);
    }

    let mut scored = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .display()
            .to_string()
            .to_lowercase();
        let mut score = text_match_score(&relative, &terms) * 8;

        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read grep scope file {}", path.display()))?;
        let content = safe_lossy_prefix(&bytes, score_bytes_per_file).to_lowercase();
        score += text_match_score(&content, &terms);

        if score > 0 {
            scored.push((score, path));
        }
    }

    if scored.is_empty() {
        return collect_scope_files(root, limit_files);
    }

    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut files = scored.into_iter().map(|(_, path)| path).collect::<Vec<_>>();
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

fn collect_legacy_scope_files(
    root: &Path,
    query: &str,
    limit_files: usize,
    score_bytes_per_file: usize,
) -> Result<Vec<PathBuf>> {
    let files = collect_scope_files(root, 0)?;
    let terms = extract_query_terms(query);
    let mut scored = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .display()
            .to_string()
            .to_lowercase();
        let docs_bias = if relative.contains("readme")
            || relative.contains("docs/")
            || relative.contains("guide")
            || relative.contains("install")
            || relative.contains("setup")
        {
            12
        } else {
            0
        };
        let mut score = docs_bias + text_match_score(&relative, &terms) * 6;
        let bytes = fs::read(&path)
            .with_context(|| format!("failed to read legacy scope file {}", path.display()))?;
        let content = safe_lossy_prefix(&bytes, score_bytes_per_file).to_lowercase();
        score += text_match_score(&content, &terms);
        if score > 0 {
            scored.push((score, path));
        }
    }
    if scored.is_empty() {
        return collect_scope_files(root, limit_files);
    }
    scored.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    let mut files = scored.into_iter().map(|(_, path)| path).collect::<Vec<_>>();
    if limit_files > 0 {
        files.truncate(limit_files);
    }
    Ok(files)
}

pub(crate) fn ledger_item_project_code(item: &Value) -> Option<&str> {
    item["project_code"]
        .as_str()
        .or_else(|| item["provenance"]["source_project"].as_str())
}

pub(crate) fn ledger_item_relative_path(item: &Value) -> Option<&str> {
    item["relative_path"]
        .as_str()
        .or_else(|| item["provenance"]["path"].as_str())
}

pub(crate) fn extract_query_terms(query: &str) -> Vec<String> {
    let mut terms = query
        .to_lowercase()
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '.')
        .filter(|term| term.len() >= 3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms
}

pub(crate) fn text_match_score(haystack: &str, terms: &[String]) -> usize {
    terms
        .iter()
        .map(|term| haystack.match_indices(term).count())
        .sum()
}

pub(crate) fn safe_lossy_prefix(bytes: &[u8], max_bytes: usize) -> String {
    let slice = &bytes[..bytes.len().min(max_bytes)];
    String::from_utf8_lossy(slice).into_owned()
}

pub(crate) fn render_naive_scope_prompt(payload: &Value, scope: &NaiveScope) -> String {
    let mut prompt = String::new();
    prompt.push_str("NAIVE_SCOPE\n");
    prompt.push_str(
        "This bundle represents the visible project scope without retrieval reduction.\n",
    );
    prompt.push_str("Query: ");
    prompt.push_str(payload["query"].as_str().unwrap_or_default());
    prompt.push_str("\nVisible projects:\n");
    for project in payload["visible_projects"].as_array().into_iter().flatten() {
        prompt.push_str("- ");
        prompt.push_str(project["project_code"].as_str().unwrap_or_default());
        prompt.push_str(" :: ");
        prompt.push_str(project["repo_root"].as_str().unwrap_or_default());
        prompt.push('\n');
    }
    prompt.push('\n');
    for file in &scope.rendered_files {
        prompt.push_str("## PROJECT ");
        prompt.push_str(&file.project_code);
        prompt.push('\n');
        prompt.push_str("### FILE ");
        prompt.push_str(&file.relative_path);
        prompt.push('\n');
        prompt.push_str(&file.content);
        prompt.push_str("\n\n");
    }
    prompt
}

pub(crate) fn render_context_pack_prompt(payload: &Value) -> String {
    if payload["cache_reuse_reference"]["state"].as_str() == Some("same_thread_context_pack_replay")
    {
        return render_same_thread_cache_reuse_prompt(payload);
    }
    let mut excerpt_paths = HashSet::new();
    let mut exact_lines = Vec::new();
    let mut symbol_lines = Vec::new();
    let mut seen_symbols = HashSet::new();
    for item in payload["retrieval"]["symbol_hits"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let line = format!(
            "[{}] {} :: {} :: {}",
            item["provenance"]["source_project"]
                .as_str()
                .unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
            item["name"].as_str().unwrap_or_default(),
            item["kind"].as_str().unwrap_or_default(),
        );
        if seen_symbols.insert(line.clone()) {
            symbol_lines.push(line);
        }
    }

    let mut excerpt_lines = Vec::new();
    let mut seen_excerpts = HashSet::new();
    for section in ["lexical_chunks", "semantic_chunks"] {
        for item in payload["retrieval"][section]
            .as_array()
            .into_iter()
            .flatten()
        {
            let line = format!(
                "[{}] {} :: {}",
                item["provenance"]["source_project"]
                    .as_str()
                    .or_else(|| item["project_code"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default(),
                item["content"].as_str().unwrap_or_default(),
            );
            if seen_excerpts.insert(line.clone()) {
                excerpt_lines.push(line);
            }
            excerpt_paths.insert(format!(
                "{}::{}",
                item["provenance"]["source_project"]
                    .as_str()
                    .or_else(|| item["project_code"].as_str())
                    .unwrap_or_default(),
                item["relative_path"].as_str().unwrap_or_default()
            ));
        }
    }

    let mut seen_exact = HashSet::new();
    for item in payload["retrieval"]["exact_documents"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let key = format!(
            "{}::{}",
            item["project_code"].as_str().unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default()
        );
        if excerpt_paths.contains(&key) {
            continue;
        }
        let line = format!(
            "[{}] {} {}",
            item["project_code"].as_str().unwrap_or_default(),
            item["relative_path"].as_str().unwrap_or_default(),
            item["snippet"].as_str().unwrap_or_default(),
        );
        if seen_exact.insert(line.clone()) {
            exact_lines.push(line);
        }
    }

    let mut prompt = String::new();
    prompt.push_str("Q:");
    prompt.push_str(payload["query"].as_str().unwrap_or_default());
    prompt.push('\n');
    prompt.push_str("M:");
    prompt.push_str(
        payload["effective_retrieval_mode"]
            .as_str()
            .unwrap_or_default(),
    );
    prompt.push('\n');
    prompt.push_str("P\n");
    for project in payload["visible_projects"].as_array().into_iter().flatten() {
        prompt.push('[');
        prompt.push_str(project["project_code"].as_str().unwrap_or_default());
        prompt.push_str("] ");
        prompt.push_str(project["repo_root"].as_str().unwrap_or_default());
        prompt.push('\n');
    }
    prompt.push('\n');
    push_compact_lines(&mut prompt, "D", &exact_lines);
    push_compact_lines(&mut prompt, "S", &symbol_lines);
    push_compact_lines(&mut prompt, "E", &excerpt_lines);
    prompt
}

fn render_same_thread_cache_reuse_prompt(payload: &Value) -> String {
    let mut prompt = String::new();
    let mut reuse_lines = Vec::new();
    if let Some(context_pack_id) = payload["cache_reuse_reference"]["source_context_pack_id"]
        .as_str()
        .filter(|value| !value.is_empty())
    {
        reuse_lines.push(format!("ctx={context_pack_id}"));
    }
    let counts = &payload["cache_reuse_reference"]["retrieval_counts"];
    reuse_lines.push(format!(
        "counts docs={} symbols={} lexical={} semantic={}",
        counts["exact_documents"].as_u64().unwrap_or(0),
        counts["symbol_hits"].as_u64().unwrap_or(0),
        counts["lexical_chunks"].as_u64().unwrap_or(0),
        counts["semantic_chunks"].as_u64().unwrap_or(0),
    ));
    for path in payload["cache_reuse_reference"]["active_files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str())
        .take(6)
    {
        reuse_lines.push(format!("file={path}"));
    }
    reuse_lines.push("reuse=prior_full_payload_same_thread".to_string());
    push_compact_lines(&mut prompt, "R", &reuse_lines);
    prompt
}

fn push_compact_lines(prompt: &mut String, title: &str, lines: &[String]) {
    prompt.push_str(title);
    prompt.push('\n');
    for line in lines {
        prompt.push_str(line);
        prompt.push('\n');
    }
    prompt.push('\n');
}

pub(crate) fn build_tokenizer(name: &str) -> Result<CoreBPE> {
    match name {
        "o200k_base" => o200k_base().context("failed to initialize o200k_base tokenizer"),
        "cl100k_base" => cl100k_base().context("failed to initialize cl100k_base tokenizer"),
        other => Err(anyhow!("unsupported tokenizer: {other}")),
    }
}

pub(crate) fn shared_tokenizer(name: &str) -> Result<&'static CoreBPE> {
    static O200K_BASE: OnceLock<CoreBPE> = OnceLock::new();
    static CL100K_BASE: OnceLock<CoreBPE> = OnceLock::new();
    match name {
        "o200k_base" => Ok(O200K_BASE
            .get_or_init(|| o200k_base().expect("failed to initialize o200k_base tokenizer"))),
        "cl100k_base" => Ok(CL100K_BASE
            .get_or_init(|| cl100k_base().expect("failed to initialize cl100k_base tokenizer"))),
        other => Err(anyhow!("unsupported tokenizer: {other}")),
    }
}
