const DEFAULT_FAST_CACHE_MODE: &str = "__namespace_default__";

fn should_bypass_fast_context_pack_cache(args: &ContextPackArgs) -> bool {
    args.disable_cache
        || matches!(
            args.token_source_kind.as_str(),
            "proof_context_pack" | "verify_context_pack"
        )
}

fn selected_fast_cache_key(cfg: &AppConfig, args: &ContextPackArgs) -> FastCacheKey {
    fast_cache_key(
        cfg,
        args,
        args.retrieval_mode
            .as_deref()
            .unwrap_or(DEFAULT_FAST_CACHE_MODE),
    )
}

pub fn try_execute_context_pack_fast_cached(
    cfg: &AppConfig,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<Option<ContextPackResult>> {
    let started = Instant::now();
    if should_bypass_fast_context_pack_cache(args) {
        return Ok(None);
    }
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
        return Ok(Some(context_pack_result_from_local_entry(
            cached,
            precise_lower_bound_ms,
        )));
    };
    if persist && !cached.durably_persisted {
        return Ok(None);
    }
    let precise_lower_bound_ms = precise_elapsed_ms(started);
    Ok(Some(context_pack_result_from_local_entry(
        cached,
        precise_lower_bound_ms,
    )))
}

pub fn prepare_fast_context_pack_probe(
    cfg: &AppConfig,
    args: &ContextPackArgs,
    persist: bool,
) -> Result<Option<FastContextPackProbe>> {
    if should_bypass_fast_context_pack_cache(args) {
        return Ok(None);
    }
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
        local_fast_context_pack_cache_put(fast_cache_key, &cached)?;
        cached
    };
    if persist && !cached.durably_persisted {
        return Ok(None);
    }
    Ok(Some(FastContextPackProbe {
        fast_cache_key,
        ttl_ms: cfg.local_fast_cache_ttl_ms,
        require_persist: persist,
        stats: cached_fast_context_pack_stats(&cached, 0.0),
    }))
}

pub fn fast_context_pack_probe_hit(probe: &FastContextPackProbe) -> Result<bool> {
    local_fast_context_pack_cache_contains(
        probe.fast_cache_key,
        probe.ttl_ms,
        probe.require_persist,
    )
}
