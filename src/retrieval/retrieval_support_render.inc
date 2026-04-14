fn sort_and_truncate_documents(documents: &mut Vec<DocumentHit>, limit: usize) {
    documents.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    documents.retain(|document| {
        seen.insert(format!(
            "{}::{}::{}",
            document.project_code, document.namespace_code, document.relative_path
        ))
    });
    documents.truncate(limit);
}

fn sort_and_truncate_symbols(symbols: &mut Vec<SymbolHit>, limit: usize) {
    symbols.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    symbols.retain(|symbol| {
        seen.insert(format!(
            "{}::{}::{}::{}::{}",
            symbol.project_code,
            symbol.namespace_code,
            symbol.relative_path,
            symbol.name,
            symbol.start_line
        ))
    });
    symbols.truncate(limit);
}

fn sort_and_truncate_chunks(chunks: &mut Vec<ChunkHit>, limit: usize) {
    chunks.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut seen = HashSet::new();
    chunks.retain(|chunk| seen.insert(chunk.chunk_id));
    chunks.truncate(limit);
}

fn document_to_json(hit: &DocumentHit) -> Value {
    json!({
        "score": hit.score,
        "project_code": hit.project_code,
        "namespace_code": hit.namespace_code,
        "language": hit.language,
        "source_kind": hit.source_kind,
        "relative_path": hit.relative_path,
        "snippet": hit.snippet,
        "provenance": {
            "source_project": hit.project_code,
            "repo_root": hit.repo_root,
            "commit_sha": hit.git_commit_sha,
            "path": hit.relative_path,
            "symbol": Value::Null,
            "chunk_id": Value::Null,
            "source_kind": hit.source_kind,
            "trust_level": "local_repo"
        }
    })
}

fn symbol_to_json(hit: &SymbolHit) -> Value {
    json!({
        "score": hit.score,
        "project_code": hit.project_code,
        "namespace_code": hit.namespace_code,
        "relative_path": hit.relative_path,
        "name": hit.name,
        "kind": hit.kind,
        "start_line": hit.start_line,
        "end_line": hit.end_line,
        "start_byte": hit.start_byte,
        "end_byte": hit.end_byte,
        "metadata": hit.metadata,
        "provenance": {
            "source_project": hit.project_code,
            "repo_root": hit.repo_root,
            "commit_sha": Value::Null,
            "path": hit.relative_path,
            "symbol": hit.name,
            "chunk_id": Value::Null,
            "source_kind": "symbol",
            "trust_level": "local_repo"
        }
    })
}

fn chunk_to_json(hit: &ChunkHit) -> Value {
    json!({
        "score": hit.score,
        "project_code": hit.project_code,
        "namespace_code": hit.namespace_code,
        "relative_path": hit.relative_path,
        "content": hit.content,
        "chunk": {
            "chunk_id": hit.chunk_id,
            "chunk_index": hit.chunk_index,
            "start_line": hit.start_line,
            "end_line": hit.end_line,
            "metadata": hit.metadata
        },
        "provenance": {
            "source_project": hit.project_code,
            "repo_root": hit.repo_root,
            "commit_sha": Value::Null,
            "path": hit.relative_path,
            "symbol": Value::Null,
            "chunk_id": hit.chunk_id,
            "source_kind": "code_chunk",
            "trust_level": "local_repo"
        }
    })
}

fn memory_card_temporal_exclusion_to_json(
    diagnostic: &postgres::MemoryCardTemporalExclusionDiagnostic,
) -> Value {
    json!({
        "memory_card_id": diagnostic.memory_card_id,
        "title": diagnostic.title,
        "truth_state": diagnostic.truth_state,
        "status": diagnostic.status,
        "valid_from_epoch_ms": diagnostic.valid_from_epoch_ms,
        "valid_to_epoch_ms": diagnostic.valid_to_epoch_ms,
        "exclusion_reason": diagnostic.exclusion_reason
    })
}

fn memory_card_to_json(card: &postgres::MemoryCardRecord) -> Value {
    json!({
        "memory_card_id": card.memory_card_id,
        "project_code": card.project_code,
        "namespace_code": card.namespace_code,
        "title": card.title,
        "summary": card.summary,
        "body": card.body,
        "tags": card.tags,
        "provenance": card.provenance,
        "fact_subject": card.fact_subject,
        "fact_predicate": card.fact_predicate,
        "fact_object": card.fact_object,
        "truth_state": card.truth_state,
        "verification_state": card.verification_state,
        "status": card.status,
        "derivation_kind": card.derivation_kind,
        "candidate_class": card.candidate_class,
        "source_kind": card.source_kind,
        "hot_path_write_eligible": card.hot_path_write_eligible,
        "background_consolidation_recommended": card.background_consolidation_recommended,
        "observed_at_epoch_ms": card.observed_at_epoch_ms,
        "recorded_at_epoch_ms": card.recorded_at_epoch_ms,
        "valid_from_epoch_ms": card.valid_from_epoch_ms,
        "valid_to_epoch_ms": card.valid_to_epoch_ms,
        "last_verified_at_epoch_ms": card.last_verified_at_epoch_ms,
        "superseded_by_memory_card_id": card.superseded_by_memory_card_id,
        "created_at": card.created_at,
    })
}

fn memory_relation_edge_to_json(edge: &postgres::MemoryRelationEdgeRecord) -> Value {
    json!({
        "memory_relation_edge_id": edge.memory_relation_edge_id,
        "project_code": edge.project_code,
        "namespace_code": edge.namespace_code,
        "source_memory_card_id": edge.source_memory_card_id,
        "target_memory_card_id": edge.target_memory_card_id,
        "relation_type": edge.relation_type,
        "relation_state": edge.relation_state,
        "evidence": edge.evidence,
        "recorded_at_epoch_ms": edge.recorded_at_epoch_ms,
        "valid_from_epoch_ms": edge.valid_from_epoch_ms,
        "valid_to_epoch_ms": edge.valid_to_epoch_ms,
        "created_at": edge.created_at,
    })
}

fn raw_evidence_temporal_exclusion_to_json(
    diagnostic: &postgres::RawEvidenceTemporalExclusionDiagnostic,
) -> Value {
    json!({
        "memory_item_id": diagnostic.memory_item_id,
        "title": diagnostic.title,
        "truth_state": diagnostic.truth_state,
        "verification_state": diagnostic.verification_state,
        "valid_from_epoch_ms": diagnostic.valid_from_epoch_ms,
        "valid_to_epoch_ms": diagnostic.valid_to_epoch_ms,
        "exclusion_reason": diagnostic.exclusion_reason
    })
}

fn raw_evidence_to_json(hit: &postgres::RawEvidenceRecord) -> Value {
    let relative_path = hit
        .evidence_span
        .get("path")
        .and_then(Value::as_str)
        .or_else(|| {
            hit.details
                .get("path")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            hit.details
                .get("relative_path")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
        });
    json!({
        "memory_item_id": hit.memory_item_id,
        "memory_provenance_id": hit.memory_provenance_id,
        "project_code": hit.project_code,
        "namespace_code": hit.namespace_code,
        "title": hit.title,
        "summary": hit.summary,
        "content": hit.content,
        "source_kind": hit.source_kind,
        "source_event_id": hit.source_event_id,
        "artifact_refs": hit.artifact_refs,
        "message_refs": hit.message_refs,
        "evidence_span": hit.evidence_span,
        "details": hit.details,
        "derivation_kind": hit.derivation_kind,
        "truth_state": hit.truth_state,
        "trust_state": hit.trust_state,
        "verification_state": hit.verification_state,
        "observed_at_epoch_ms": hit.observed_at_epoch_ms,
        "recorded_at_epoch_ms": hit.recorded_at_epoch_ms,
        "valid_from_epoch_ms": hit.valid_from_epoch_ms,
        "valid_to_epoch_ms": hit.valid_to_epoch_ms,
        "last_verified_at_epoch_ms": hit.last_verified_at_epoch_ms,
        "relative_path": relative_path,
        "provenance": {
            "source_kind": hit.source_kind,
            "source_event_id": hit.source_event_id,
            "artifact_refs": hit.artifact_refs,
            "message_refs": hit.message_refs,
            "evidence_span": hit.evidence_span,
        }
    })
}

