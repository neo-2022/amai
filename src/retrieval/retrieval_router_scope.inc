async fn resolve_visible_projects(
    db: &Client,
    project: &ProjectRecord,
    effective_mode: &str,
) -> Result<Vec<VisibleProjectRecord>> {
    if effective_mode == "audit_global" {
        let mut visible = Vec::new();
        for candidate in postgres::list_projects(db, None, None).await? {
            let is_local = candidate.project_id == project.project_id;
            if !is_local
                && !postgres::project_allows_cross_project_read(
                    db,
                    project.project_id,
                    "org_global",
                )
                .await?
            {
                continue;
            }
            visible.push(VisibleProjectRecord {
                project: candidate,
                relation_type: if is_local {
                    "local".to_string()
                } else {
                    "audit_global".to_string()
                },
                project_link_type: if is_local {
                    "local".to_string()
                } else {
                    "audit_global".to_string()
                },
                shared_contour: if is_local {
                    "self".to_string()
                } else {
                    "global_audit".to_string()
                },
                visibility_scope: if is_local {
                    "project_shared".to_string()
                } else {
                    "org_global".to_string()
                },
                relation_status: "active".to_string(),
                requires_approval: false,
                transfer_policy_code: None,
                access_mode: "audit_global".to_string(),
            });
        }
        return Ok(visible);
    }

    let mut visible = vec![VisibleProjectRecord {
        project: project.clone(),
        relation_type: "local".to_string(),
        project_link_type: "local".to_string(),
        shared_contour: "self".to_string(),
        visibility_scope: "project_shared".to_string(),
        relation_status: "active".to_string(),
        requires_approval: false,
        transfer_policy_code: None,
        access_mode: "local_strict".to_string(),
    }];
    if effective_mode == "local_strict" {
        return Ok(visible);
    }
    let allowed_rank = mode_rank(effective_mode)?;
    let related = postgres::list_related_projects(db, project.project_id).await?;
    for relation in related {
        if mode_rank(&relation.access_mode)? <= allowed_rank
            && postgres::project_allows_cross_project_read(
                db,
                project.project_id,
                &relation.visibility_scope,
            )
            .await?
        {
            visible.push(relation);
        }
    }
    Ok(visible)
}

async fn resolve_visible_scopes(
    db: &Client,
    visible_projects: &[VisibleProjectRecord],
    local_project_id: Uuid,
    local_namespace: &postgres::NamespaceRecord,
) -> Result<Vec<ResolvedVisibleScope>> {
    let mut scopes = Vec::new();
    for visible in visible_projects {
        let namespace = if visible.project.project_id == local_project_id {
            local_namespace.clone()
        } else {
            let Some(namespace) = postgres::find_namespace_by_code(
                db,
                visible.project.project_id,
                &local_namespace.code,
            )
            .await?
            else {
                continue;
            };
            namespace
        };
        scopes.push(ResolvedVisibleScope {
            visible: visible.clone(),
            namespace,
        });
    }
    Ok(scopes)
}

fn is_common_document_basename(query: &str) -> bool {
    let normalized = query.trim().to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "makefile"
            | "dockerfile"
            | "containerfile"
            | "license"
            | "copying"
            | "notice"
            | "readme"
            | "contributing"
            | "changelog"
            | "justfile"
            | "workspace"
            | "build"
    )
}

fn slugified_document_lookup_alias(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty()
        || trimmed.len() > 256
        || trimmed.contains('/')
        || trimmed.contains('.')
        || !trimmed.chars().any(char::is_whitespace)
    {
        return None;
    }
    let mut slug = String::with_capacity(trimmed.len());
    let mut previous_was_separator = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            previous_was_separator = false;
        } else if !previous_was_separator {
            slug.push('-');
            previous_was_separator = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() || slug == trimmed.to_ascii_lowercase() {
        return None;
    }
    Some(slug)
}

fn exact_document_lookup_queries(query: &str) -> Vec<String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let mut candidates = vec![trimmed.to_string()];
    if let Some(slug) = slugified_document_lookup_alias(trimmed) {
        candidates.push(slug.clone());
        candidates.push(format!("{slug}.md"));
    }
    candidates
}

fn should_prefer_document_lookup_only(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 256
        && (trimmed.contains('/') || trimmed.contains('.') || is_common_document_basename(trimmed))
}

fn should_try_exact_document_lookup(query: &str) -> bool {
    let trimmed = query.trim();
    should_prefer_document_lookup_only(trimmed)
        || (!trimmed.is_empty()
            && trimmed.len() <= 256
            && (trimmed.contains('.') || slugified_document_lookup_alias(trimmed).is_some()))
}

fn should_try_exact_symbol_lookup(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 128
        && !trimmed.chars().any(char::is_whitespace)
        && !trimmed.contains('/')
        && !trimmed.contains('-')
}

fn should_try_exact_chunk_lookup(query: &str) -> bool {
    should_try_exact_symbol_lookup(query)
}

fn summary_layer_sufficiency(
    exact_count: usize,
    symbol_count: usize,
    lexical_count: usize,
    memory_card_count: usize,
) -> (bool, &'static str) {
    if exact_count > 0 {
        return (true, "exact_documents_sufficient");
    }
    if symbol_count > 0 {
        return (true, "symbol_hits_sufficient");
    }
    if lexical_count > 0 {
        return (true, "lexical_chunks_sufficient");
    }
    if memory_card_count >= 2 {
        return (true, "multiple_memory_cards_sufficient");
    }
    if memory_card_count == 1 {
        return (false, "single_memory_card_needs_structured_neighborhood");
    }
    (false, "no_summary_signal")
}

fn should_skip_memory_lookup(
    args: &ContextPackArgs,
    exact_count: usize,
    symbol_count: usize,
    lexical_count: usize,
    memory_card_count: usize,
) -> bool {
    args.token_source_kind != "verify_context_pack"
        && summary_layer_sufficiency(exact_count, symbol_count, lexical_count, memory_card_count).0
}

fn should_persist_retrieval_trace(token_source_kind: &str) -> bool {
    !matches!(
        token_source_kind,
        "benchmark_context_pack" | "benchmark_cold_context_pack"
    )
}

fn should_use_benchmark_compact_payload(token_source_kind: &str) -> bool {
    matches!(
        token_source_kind,
        "benchmark_context_pack" | "benchmark_cold_context_pack"
    )
}

fn structured_layer_sufficiency(
    summary_sufficient: bool,
    relation_edge_count: usize,
) -> (bool, &'static str) {
    if summary_sufficient {
        return (true, "summary_layer_already_sufficient");
    }
    if relation_edge_count > 0 {
        return (true, "structured_graph_neighborhood_sufficient");
    }
    (false, "no_structured_graph_support")
}

fn semantic_skip_reason_for_router(
    args: &ContextPackArgs,
    summary_sufficient: bool,
    structured_sufficient: bool,
) -> Option<&'static str> {
    if args.limit_semantic_chunks == 0 {
        return Some("semantic_limit_disabled");
    }
    if args.token_source_kind == "verify_context_pack" {
        return None;
    }
    if summary_sufficient || structured_sufficient {
        return Some("cheapest_sufficient_evidence_already_found");
    }
    None
}

