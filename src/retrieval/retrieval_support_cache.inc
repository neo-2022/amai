fn build_query_embedder(cfg: &AppConfig) -> Result<TextEmbedding> {
    let model = match cfg.code_embed_model.as_str() {
        "jina_base_code" => EmbeddingModel::JinaEmbeddingsV2BaseCode,
        "multilingual_e5_small" => EmbeddingModel::MultilingualE5Small,
        "multilingual_e5_base" => EmbeddingModel::MultilingualE5Base,
        other => return Err(anyhow!("unsupported code embedding model: {other}")),
    };
    TextEmbedding::try_new(InitOptions::new(model).with_show_download_progress(false))
}

fn embed_query(cfg: &AppConfig, query: &str) -> Result<(Vec<f32>, u128)> {
    let started = Instant::now();
    let cache = QUERY_EMBEDDER.get_or_init(|| Mutex::new(None));
    let mut guard = cache
        .lock()
        .map_err(|_| anyhow!("query embedder cache lock poisoned"))?;
    let needs_rebuild = guard
        .as_ref()
        .map(|cached| cached.model != cfg.code_embed_model)
        .unwrap_or(true);
    if needs_rebuild {
        *guard = Some(CachedQueryEmbedder {
            model: cfg.code_embed_model.clone(),
            embedder: build_query_embedder(cfg)?,
            query_cache: HashMap::new(),
        });
    }
    let cached = guard
        .as_mut()
        .ok_or_else(|| anyhow!("query embedder cache unexpectedly empty"))?;
    if let Some(vector) = cached.query_cache.get(query).cloned() {
        return Ok((vector, started.elapsed().as_millis()));
    }
    let embeddings = cached.embedder.embed(&[query.to_string()], Some(1))?;
    let vector = embeddings
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("semantic embedder returned no query vector"))?;
    if cached.query_cache.len() >= 64
        && let Some(oldest_key) = cached.query_cache.keys().next().cloned()
    {
        cached.query_cache.remove(&oldest_key);
    }
    cached.query_cache.insert(query.to_string(), vector.clone());
    Ok((vector, started.elapsed().as_millis()))
}

fn local_repo_document_snippet(repo_root: &str, relative_path: &str) -> Result<Option<String>> {
    let trimmed = relative_path.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let repo_root = Path::new(repo_root);
    let joined = if let Some(clean_relative_path) = normalized_safe_relative_path(trimmed) {
        repo_root.join(clean_relative_path)
    } else if Path::new(trimmed).is_absolute() {
        PathBuf::from(trimmed)
    } else {
        let canonical_repo_root = match repo_root.canonicalize() {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        let joined = repo_root.join(trimmed);
        let canonical_path = match joined.canonicalize() {
            Ok(value) => value,
            Err(_) => return Ok(None),
        };
        if !canonical_path.starts_with(&canonical_repo_root) || !canonical_path.is_file() {
            return Ok(None);
        }
        return read_file_snippet(&canonical_path, 2000).map(Some);
    };
    if !joined.is_file() {
        return Ok(None);
    }
    read_file_snippet(&joined, 2000).map(Some)
}

fn read_file_snippet(path: &PathBuf, max_bytes: usize) -> Result<String> {
    let mut file = File::open(path)?;
    let mut buffer = vec![0_u8; max_bytes];
    let read = file.read(&mut buffer)?;
    buffer.truncate(read);
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

fn normalized_safe_relative_path(value: &str) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in Path::new(value).components() {
        match component {
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return None,
        }
    }
    (!normalized.as_os_str().is_empty()).then_some(normalized)
}

fn scope_signature(visible_scopes: &[ResolvedVisibleScope]) -> String {
    visible_scopes
        .iter()
        .map(|scope| {
            format!(
                "{}:{}:{}:{}:{}:{}:{}",
                scope.visible.project.code,
                scope.visible.project.updated_at,
                scope.visible.relation_type,
                scope.visible.shared_contour,
                scope.visible.access_mode,
                scope.namespace.namespace_id,
                scope.namespace.code
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn cache_key(cfg: &AppConfig, args: &ContextPackArgs, effective_mode: &str) -> String {
    format!(
        "{}::{}::{}::{}::{}::{}::{}::{}::{}::{}",
        cfg.stack_name,
        args.project,
        args.namespace,
        effective_mode,
        args.limit_documents,
        args.limit_symbols,
        args.limit_chunks,
        args.limit_semantic_chunks,
        args.at_epoch_ms
            .map(|value| value.to_string())
            .unwrap_or_else(|| "latest".to_string()),
        hex_sha256(args.query.as_bytes())
    )
}

fn fast_cache_key(cfg: &AppConfig, args: &ContextPackArgs, effective_mode: &str) -> FastCacheKey {
    let mut hasher = Sha256::new();
    hasher.update(cfg.stack_name.as_bytes());
    hasher.update([0]);
    hasher.update(args.project.as_bytes());
    hasher.update([0]);
    hasher.update(args.namespace.as_bytes());
    hasher.update([0]);
    hasher.update(effective_mode.as_bytes());
    hasher.update([0]);
    hasher.update(args.limit_documents.to_le_bytes());
    hasher.update(args.limit_symbols.to_le_bytes());
    hasher.update(args.limit_chunks.to_le_bytes());
    hasher.update(args.limit_semantic_chunks.to_le_bytes());
    hasher.update(args.at_epoch_ms.unwrap_or(-1).to_le_bytes());
    hasher.update(args.query.as_bytes());
    let digest = hasher.finalize();
    let mut key = [0_u8; 16];
    key.copy_from_slice(&digest[..16]);
    u128::from_be_bytes(key)
}

fn prepared_from_cached_payload(
    context: CacheHydrationContext<'_>,
    cached: LocalContextPackEntry,
) -> Result<PreparedContextPack> {
    let stats = cached_context_pack_stats(
        &cached,
        context.scope_signature.clone(),
        context.resolve_scope_ms,
        context.cache_lookup_ms,
        context.precise_lower_bound_ms,
    );
    let mut payload = cached.payload.as_ref().clone();
    payload["retrieval_runtime"] = runtime_json(&stats);
    ensure_context_pack_decision_trace(&mut payload);
    let payload_json: Arc<str> = Arc::from(serde_json::to_string(&payload)?);

    Ok(PreparedContextPack {
        context_pack_id: cached.context_pack_id,
        project: context.project.clone(),
        namespace_id: context.namespace_id,
        effective_mode: context.effective_mode.to_string(),
        visible_projects_json: payload["visible_projects"].clone(),
        payload: Arc::new(payload),
        payload_json,
        stats,
        cache_key: context.cache_key,
        scope_signature: context.scope_signature,
        durably_persisted: cached.durably_persisted,
        artifact_bucket: None,
        artifact_object_key: None,
        artifact_state: None,
    })
}

async fn try_execute_context_pack_stats_cached(
    cfg: &AppConfig,
    db: &mut Client,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<Option<ContextPackStats>> {
    if let Some(stats) = try_context_pack_stats_from_local_fast_cache(cfg, args, persist)? {
        return Ok(Some(stats));
    }
    if args.disable_cache {
        return Ok(None);
    }

    let resolve_started = Instant::now();
    let project = postgres::get_project_by_code(db, &args.project).await?;
    let namespace =
        postgres::get_namespace_by_code(db, project.project_id, &args.namespace).await?;
    let effective_mode = args
        .retrieval_mode
        .clone()
        .unwrap_or_else(|| namespace.retrieval_mode.clone());
    let visible_projects = resolve_visible_projects(db, &project, &effective_mode).await?;
    let visible_scopes =
        resolve_visible_scopes(db, &visible_projects, project.project_id, &namespace).await?;
    let resolve_scope_ms = resolve_started.elapsed().as_millis();
    let scope_signature = scope_signature(&visible_scopes);
    let cache_key = cache_key(cfg, args, &effective_mode);
    let edge_cache_path = edge_cache::ensure(&cfg.edge_cache_path)?;

    let cache_lookup_started = Instant::now();
    if let Some(cached) = local_context_pack_cache_get(&cache_key, &scope_signature)? {
        if persist && !cached.durably_persisted {
            return Ok(None);
        }
        let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();
        return Ok(Some(cached_context_pack_stats(
            &cached,
            scope_signature,
            resolve_scope_ms,
            cache_lookup_ms,
            precise_elapsed_ms(resolve_started),
        )));
    }

    if let Some(cached) =
        edge_cache::get_context_pack_cache_entry(&edge_cache_path, &cache_key, &scope_signature)?
    {
        let local_cached = local_entry_from_edge(cached)?;
        if persist && !local_cached.durably_persisted {
            return Ok(None);
        }
        local_context_pack_cache_put(&cache_key, &scope_signature, &local_cached)?;
        let cache_lookup_ms = cache_lookup_started.elapsed().as_millis();
        return Ok(Some(cached_context_pack_stats(
            &local_cached,
            scope_signature,
            resolve_scope_ms,
            cache_lookup_ms,
            precise_elapsed_ms(resolve_started),
        )));
    }

    Ok(None)
}

fn try_context_pack_stats_from_local_fast_cache(
    cfg: &AppConfig,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<Option<ContextPackStats>> {
    if args.disable_cache {
        return Ok(None);
    }
    let started = Instant::now();
    let fast_cache_key = selected_fast_cache_key(cfg, args);
    let cached = if let Some(cached) =
        local_fast_context_pack_cache_get(fast_cache_key, cfg.local_fast_cache_ttl_ms)?
    {
        cached
    } else {
        let Some(cached) = edge_fast_context_pack_cache_get(
            &cfg.edge_cache_path,
            fast_cache_key,
            cfg.local_fast_cache_ttl_ms,
        )?
        else {
            return Ok(None);
        };
        let precise_lower_bound_ms = precise_elapsed_ms(started);
        local_fast_context_pack_cache_put(fast_cache_key, &cached)?;
        if persist && !cached.durably_persisted {
            return Ok(None);
        }
        return Ok(Some(cached_fast_context_pack_stats(
            &cached,
            precise_lower_bound_ms,
        )));
    };
    if persist && !cached.durably_persisted {
        return Ok(None);
    }
    Ok(Some(cached_fast_context_pack_stats(&cached, 0.0)))
}

fn local_context_pack_cache_get(
    cache_key: &str,
    scope_signature: &str,
) -> Result<Option<LocalContextPackEntry>> {
    let cache = LOCAL_CONTEXT_PACK_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let guard = cache
        .read()
        .map_err(|_| anyhow!("local context-pack cache lock poisoned"))?;
    let key = format!("{cache_key}::{scope_signature}");
    Ok(guard.get(&key).cloned())
}

fn local_context_pack_cache_put(
    cache_key: &str,
    scope_signature: &str,
    entry: &LocalContextPackEntry,
) -> Result<()> {
    let cache = LOCAL_CONTEXT_PACK_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let mut guard = cache
        .write()
        .map_err(|_| anyhow!("local context-pack cache lock poisoned"))?;
    let key = format!("{cache_key}::{scope_signature}");
    guard.insert(key, entry.clone());
    Ok(())
}

fn local_entry_from_prepared(
    prepared: &PreparedContextPack,
    durably_persisted: bool,
) -> LocalContextPackEntry {
    LocalContextPackEntry {
        context_pack_id: prepared.context_pack_id,
        payload: Arc::clone(&prepared.payload),
        exact_documents: prepared.stats.exact_documents,
        symbol_hits: prepared.stats.symbol_hits,
        lexical_chunks: prepared.stats.lexical_chunks,
        semantic_chunks: prepared.stats.semantic_chunks,
        durably_persisted,
        cached_at_epoch_ms: now_epoch_ms(),
    }
}

fn local_entry_from_edge(
    cached: edge_cache::CachedContextPackEntry,
) -> Result<LocalContextPackEntry> {
    let payload: Value = serde_json::from_str(&cached.payload_json)
        .context("failed to decode cached context pack payload")?;
    let context_pack_id =
        Uuid::parse_str(&cached.context_pack_id).context("cached context pack id is not a UUID")?;
    Ok(LocalContextPackEntry {
        context_pack_id,
        exact_documents: payload["retrieval"]["exact_documents"]
            .as_array()
            .map_or(0, Vec::len),
        symbol_hits: payload["retrieval"]["symbol_hits"]
            .as_array()
            .map_or(0, Vec::len),
        lexical_chunks: payload["retrieval"]["lexical_chunks"]
            .as_array()
            .map_or(0, Vec::len),
        semantic_chunks: payload["retrieval"]["semantic_chunks"]
            .as_array()
            .map_or(0, Vec::len),
        payload: Arc::new(payload),
        durably_persisted: cached.durably_persisted,
        cached_at_epoch_ms: now_epoch_ms(),
    })
}

fn local_entry_from_edge_fast(
    cached: edge_cache::CachedFastContextPackEntry,
) -> Result<LocalContextPackEntry> {
    let payload: Value = serde_json::from_str(&cached.payload_json)
        .context("failed to decode cached fast context pack payload")?;
    let context_pack_id = Uuid::parse_str(&cached.context_pack_id)
        .context("cached fast context pack id is not a UUID")?;
    Ok(LocalContextPackEntry {
        context_pack_id,
        exact_documents: payload["retrieval"]["exact_documents"]
            .as_array()
            .map_or(0, Vec::len),
        symbol_hits: payload["retrieval"]["symbol_hits"]
            .as_array()
            .map_or(0, Vec::len),
        lexical_chunks: payload["retrieval"]["lexical_chunks"]
            .as_array()
            .map_or(0, Vec::len),
        semantic_chunks: payload["retrieval"]["semantic_chunks"]
            .as_array()
            .map_or(0, Vec::len),
        payload: Arc::new(payload),
        durably_persisted: cached.durably_persisted,
        cached_at_epoch_ms: cached.cached_at_epoch_ms,
    })
}

fn cached_context_pack_stats(
    cached: &LocalContextPackEntry,
    scope_signature: String,
    resolve_scope_ms: u128,
    cache_lookup_ms: u128,
    retrieval_lower_bound_ms_precise: f64,
) -> ContextPackStats {
    ContextPackStats {
        context_pack_id: cached.context_pack_id,
        exact_documents: cached.exact_documents,
        symbol_hits: cached.symbol_hits,
        lexical_chunks: cached.lexical_chunks,
        semantic_chunks: cached.semantic_chunks,
        cache_hit: true,
        scope_signature,
        timings: ContextPackTimings {
            resolve_scope_ms,
            cache_lookup_ms,
            exact_lookup_ms: 0,
            symbol_lookup_ms: 0,
            lexical_lookup_ms: 0,
            query_embed_ms: 0,
            semantic_search_ms: 0,
            semantic_hydrate_ms: 0,
            ranking_ms: 0,
            provenance_ms: 0,
            pack_assembly_ms: 0,
            serialize_ms: 0,
            persist_ms: 0,
        },
        retrieval_lower_bound_ms_precise: Some(retrieval_lower_bound_ms_precise),
    }
}

fn cached_fast_context_pack_stats(
    cached: &LocalContextPackEntry,
    retrieval_lower_bound_ms_precise: f64,
) -> ContextPackStats {
    cached_context_pack_stats(
        cached,
        "local_fast_cache".to_string(),
        0,
        0,
        retrieval_lower_bound_ms_precise,
    )
}

fn context_pack_result_from_local_entry(
    cached: LocalContextPackEntry,
    retrieval_lower_bound_ms_precise: f64,
) -> ContextPackResult {
    let stats = cached_fast_context_pack_stats(&cached, retrieval_lower_bound_ms_precise);
    let mut payload = cached.payload.as_ref().clone();
    payload["retrieval_runtime"] = runtime_json(&stats);
    ensure_context_pack_decision_trace(&mut payload);
    ContextPackResult { payload, stats }
}

fn runtime_json(stats: &ContextPackStats) -> Value {
    let timings = &stats.timings;
    let policy_ms = timings.resolve_scope_ms + timings.cache_lookup_ms;
    let retrieval_ms = timings.exact_lookup_ms
        + timings.symbol_lookup_ms
        + timings.lexical_lookup_ms
        + timings.query_embed_ms
        + timings.semantic_search_ms
        + timings.semantic_hydrate_ms;
    let retrieval_lower_bound_ms = stats
        .retrieval_lower_bound_ms_precise
        .unwrap_or((policy_ms + retrieval_ms) as f64);
    let total_ms = timings.resolve_scope_ms
        + timings.cache_lookup_ms
        + timings.exact_lookup_ms
        + timings.symbol_lookup_ms
        + timings.lexical_lookup_ms
        + timings.query_embed_ms
        + timings.semantic_search_ms
        + timings.semantic_hydrate_ms
        + timings.ranking_ms
        + timings.provenance_ms
        + timings.pack_assembly_ms
        + timings.serialize_ms
        + timings.persist_ms;
    json!({
        "cache_hit": stats.cache_hit,
        "scope_signature": stats.scope_signature,
        "resolve_scope_ms": timings.resolve_scope_ms as f64,
        "cache_lookup_ms": timings.cache_lookup_ms as f64,
        "exact_lookup_ms": timings.exact_lookup_ms as f64,
        "symbol_lookup_ms": timings.symbol_lookup_ms as f64,
        "lexical_lookup_ms": timings.lexical_lookup_ms as f64,
        "query_embed_ms": timings.query_embed_ms as f64,
        "semantic_search_ms": timings.semantic_search_ms as f64,
        "semantic_hydrate_ms": timings.semantic_hydrate_ms as f64,
        "ranking_ms": timings.ranking_ms as f64,
        "provenance_ms": timings.provenance_ms as f64,
        "pack_assembly_ms": timings.pack_assembly_ms as f64,
        "serialize_ms": timings.serialize_ms as f64,
        "persist_ms": timings.persist_ms as f64,
        "retrieval_lower_bound_ms": retrieval_lower_bound_ms,
        "stage_group_ms": {
            "policy_ms": policy_ms as f64,
            "retrieval_ms": retrieval_ms as f64,
            "ranking_ms": timings.ranking_ms as f64,
            "provenance_ms": timings.provenance_ms as f64,
            "pack_assembly_ms": (timings.pack_assembly_ms + timings.serialize_ms) as f64,
            "orchestration_total_ms": total_ms as f64
        },
        "total_ms": total_ms as f64
    })
}

fn local_fast_context_pack_cache_get(
    fast_cache_key: FastCacheKey,
    ttl_ms: u128,
) -> Result<Option<LocalContextPackEntry>> {
    let cache = LOCAL_FAST_CONTEXT_PACK_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let entry = {
        let guard = cache
            .read()
            .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
        guard.get(&fast_cache_key).cloned()
    };
    let Some(entry) = entry else {
        return Ok(None);
    };
    if now_epoch_ms().saturating_sub(entry.cached_at_epoch_ms) > ttl_ms {
        let mut guard = cache
            .write()
            .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
        guard.remove(&fast_cache_key);
        return Ok(None);
    }
    Ok(Some(entry))
}

fn precise_elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn edge_fast_context_pack_cache_get(
    edge_cache_path: &std::path::Path,
    fast_cache_key: FastCacheKey,
    ttl_ms: u128,
) -> Result<Option<LocalContextPackEntry>> {
    let Some(entry) =
        edge_cache::get_fast_context_pack_cache_entry(edge_cache_path, fast_cache_key)?
    else {
        return Ok(None);
    };
    let entry = local_entry_from_edge_fast(entry)?;
    if now_epoch_ms().saturating_sub(entry.cached_at_epoch_ms) > ttl_ms {
        return Ok(None);
    }
    Ok(Some(entry))
}

fn local_fast_context_pack_cache_contains(
    fast_cache_key: FastCacheKey,
    ttl_ms: u128,
    require_persist: bool,
) -> Result<bool> {
    let cache = LOCAL_FAST_CONTEXT_PACK_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let status = {
        let guard = cache
            .read()
            .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
        let Some(entry) = guard.get(&fast_cache_key) else {
            return Ok(false);
        };
        let expired = now_epoch_ms().saturating_sub(entry.cached_at_epoch_ms) > ttl_ms;
        let persisted_ok = !require_persist || entry.durably_persisted;
        (expired, persisted_ok)
    };
    if status.0 {
        let mut guard = cache
            .write()
            .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
        guard.remove(&fast_cache_key);
        return Ok(false);
    }
    if !status.1 {
        return Ok(false);
    }
    Ok(true)
}

fn local_fast_context_pack_cache_put(
    fast_cache_key: FastCacheKey,
    entry: &LocalContextPackEntry,
) -> Result<()> {
    let cache = LOCAL_FAST_CONTEXT_PACK_CACHE.get_or_init(|| RwLock::new(HashMap::new()));
    let mut guard = cache
        .write()
        .map_err(|_| anyhow!("local fast context-pack cache lock poisoned"))?;
    guard.insert(fast_cache_key, entry.clone());
    Ok(())
}

fn now_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis())
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn mode_rank(mode: &str) -> Result<u8> {
    match mode {
        "local_strict" => Ok(0),
        "local_plus_related" => Ok(1),
        "explicit_foreign" => Ok(2),
        "audit_global" => Ok(3),
        other => Err(anyhow!("unsupported retrieval mode: {other}")),
    }
}

