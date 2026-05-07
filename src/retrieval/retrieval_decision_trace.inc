fn ensure_context_pack_decision_trace(payload: &mut Value) {
    if payload["decision_trace"].is_object() {
        return;
    }
    payload["decision_trace"] = build_context_pack_decision_trace(payload);
}

fn build_context_pack_intent_classifier(payload: &Value) -> Value {
    let query = payload["query"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let namespace_code = payload["namespace"]["code"].as_str().unwrap_or_default();

    let has_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));

    let continuity_keywords = [
        "continuity",
        "last chat",
        "previous chat",
        "past chat",
        "resume",
        "handoff",
        "startup",
        "restore",
        "chat at",
        "на чем закончили",
        "прошлый чат",
    ];
    let policy_keywords = [
        "policy",
        "allowed",
        "forbidden",
        "permission",
        "scope",
        "private",
        "public",
        "legal",
        "trust",
        "can i",
        "можно",
        "разреш",
        "запрещ",
        "доступ",
        "scope",
    ];
    let artifact_keywords = [
        "artifact",
        "file",
        "path",
        "log",
        "trace",
        "bundle",
        "snapshot",
        "screenshot",
        ".rs",
        ".md",
        ".json",
        ".toml",
        "/",
    ];
    let procedural_keywords = [
        "how to",
        "how do",
        "steps",
        "procedure",
        "runbook",
        "playbook",
        "workflow",
        "command",
        "commands",
        "setup",
        "deploy",
        "fix",
        "repair",
        "как",
        "шаг",
        "процед",
        "команд",
    ];

    let (classification, reason, signals): (&str, &str, Vec<&str>) = if namespace_code
        == "continuity"
        || has_any(&continuity_keywords)
    {
        (
            "continuity",
            "Namespace/query indicates chat restore or continuity recall.",
            vec!["namespace_or_query_continuity"],
        )
    } else if has_any(&policy_keywords) {
        (
            "policy_check",
            "Query contains access/policy/trust language and should be routed as policy recall.",
            vec!["policy_keywords"],
        )
    } else if has_any(&artifact_keywords) {
        (
            "artifact_lookup",
            "Query targets file/log/artifact style evidence rather than pure fact recall.",
            vec!["artifact_keywords"],
        )
    } else if has_any(&procedural_keywords) {
        (
            "procedural_recall",
            "Query asks for steps/workflow/commands and is routed as procedural recall.",
            vec!["procedural_keywords"],
        )
    } else {
        (
            "factual_recall",
            "Default retrieval intent for project-scoped factual/context recall.",
            vec!["default_factual_recall"],
        )
    };

    json!({
        "classification": classification,
        "reason": reason,
        "signals": signals,
        "classifier_version": "retrieval-intent-classifier-v1"
    })
}

fn count_temporal_historical_memory_cards(value: &Value) -> usize {
    value.as_array().map_or(0, |items| {
        items
            .iter()
            .filter(|item| {
                item["truth_state"].as_str() == Some("superseded")
                    || item["truth_state"].as_str() == Some("retracted")
                    || item["valid_to_epoch_ms"].as_i64().is_some()
            })
            .count()
    })
}

fn count_temporal_historical_raw_evidence(value: &Value) -> usize {
    value.as_array().map_or(0, |items| {
        items
            .iter()
            .filter(|item| {
                item["truth_state"].as_str() == Some("superseded")
                    || item["truth_state"].as_str() == Some("retracted")
                    || item["valid_to_epoch_ms"].as_i64().is_some()
            })
            .count()
    })
}

fn read_temporal_stat(value: &Value, lane: &str, key: &str) -> u64 {
    value["retrieval_temporal_stats"][lane][key]
        .as_u64()
        .unwrap_or(0)
}

fn build_context_pack_decision_trace(payload: &Value) -> Value {
    let exact_count = payload["retrieval"]["exact_documents"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let symbol_count = payload["retrieval"]["symbol_hits"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let lexical_count = payload["retrieval"]["lexical_chunks"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let semantic_count = payload["retrieval"]["semantic_chunks"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let raw_evidence_count = payload["retrieval"]["raw_evidence"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let memory_card_count = payload["retrieval"]["memory_cards"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let memory_relation_edge_count = payload["retrieval"]["memory_relation_edges"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default();
    let semantic_guard = &payload["quality"]["semantic_guard"];
    let summary_layer_count = exact_count + symbol_count + lexical_count + memory_card_count;
    let structured_layer_count = memory_relation_edge_count;
    let raw_layer_count = raw_evidence_count;
    let (summary_sufficient, summary_reason) =
        summary_layer_sufficiency(exact_count, symbol_count, lexical_count, memory_card_count);
    let (structured_sufficient, structured_reason) =
        structured_layer_sufficiency(summary_sufficient, memory_relation_edge_count);

    let mut included = Vec::new();
    if exact_count > 0 {
        included.push(json!({
            "strategy": "exact_documents",
            "count": exact_count,
            "reason": "Нашлись точные document/path совпадения внутри видимого контура."
        }));
    }
    if symbol_count > 0 {
        included.push(json!({
            "strategy": "symbol_hits",
            "count": symbol_count,
            "reason": "Нашлись symbol-совпадения по именам и доступным namespace."
        }));
    }
    if lexical_count > 0 {
        included.push(json!({
            "strategy": "lexical_chunks",
            "count": lexical_count,
            "reason": "Лексический поиск добавил текстовые фрагменты с прямым совпадением по запросу."
        }));
    }
    if memory_card_count > 0 {
        included.push(json!({
            "strategy": "memory_cards",
            "count": memory_card_count,
            "reason": "Compact semantic cards дали summary/profile слой для factual recall."
        }));
    }
    if memory_relation_edge_count > 0 {
        included.push(json!({
            "strategy": "memory_relation_edges",
            "count": memory_relation_edge_count,
            "reason": "Structured graph neighborhood поднял relation edges как соседний доказательный слой."
        }));
    }
    if raw_evidence_count > 0 {
        included.push(json!({
            "strategy": "raw_evidence",
            "count": raw_evidence_count,
            "reason": "Immutable raw/artifact/log evidence был поднят только после недостаточности summary и structured слоя."
        }));
    }
    if semantic_count > 0 {
        included.push(json!({
            "strategy": "semantic_chunks",
            "count": semantic_count,
            "reason": "Semantic candidate layer добавил вспомогательные vector hits, но не считается final raw evidence."
        }));
    }

    let mut not_included = Vec::new();
    if exact_count == 0 {
        not_included.push(json!({
            "strategy": "exact_documents",
            "reason": "Точных document/path совпадений по этому запросу не нашлось."
        }));
    }
    if symbol_count == 0 {
        not_included.push(json!({
            "strategy": "symbol_hits",
            "reason": "По этому запросу не нашлось symbol-совпадений в видимом контуре."
        }));
    }
    if lexical_count == 0 {
        not_included.push(json!({
            "strategy": "lexical_chunks",
            "reason": "Лексический поиск не дал новых chunk-фрагментов."
        }));
    }
    if memory_card_count == 0 {
        not_included.push(json!({
            "strategy": "memory_cards",
            "reason": "Compact semantic card слой не дал новых factual summary."
        }));
    }
    if memory_relation_edge_count == 0 {
        not_included.push(json!({
            "strategy": "memory_relation_edges",
            "reason": "Structured graph neighborhood не дал relation edges для этого запроса."
        }));
    }
    if raw_evidence_count == 0 {
        not_included.push(json!({
            "strategy": "raw_evidence",
            "reason": if structured_sufficient {
                "Raw evidence не поднимался, потому что более дешёвый summary/structured слой уже был достаточен."
            } else {
                "Raw evidence не дал допустимых immutable sources для этого запроса."
            }
        }));
    }
    if semantic_count == 0 {
        let reason = if semantic_guard["abstained"].as_bool() == Some(true) {
            semantic_guard["detail"]
                .as_str()
                .or_else(|| semantic_guard["reason"].as_str())
                .unwrap_or("Semantic candidate layer честно abstained и не добавил vector hits.")
                .to_string()
        } else {
            "Semantic candidate layer не добавил новых vector hits после scope и relevance проверки."
                .to_string()
        };
        not_included.push(json!({
            "strategy": "semantic_chunks",
            "reason": reason
        }));
    }

    let cheapest_sufficient_layer = if summary_sufficient {
        "summary_compact"
    } else if structured_sufficient {
        "structured_graph"
    } else if raw_layer_count > 0 {
        "raw_evidence"
    } else {
        "none"
    };
    let final_decision = if summary_sufficient || structured_sufficient || raw_layer_count > 0 {
        "continue"
    } else {
        "abstain"
    };
    let next_action = match final_decision {
        "continue" => "answer_on_cheapest_sufficient_evidence",
        _ => "abstain_due_to_insufficient_evidence",
    };
    let escalate_target_layer =
        if !summary_sufficient && !structured_sufficient && raw_layer_count == 0 {
            Some("raw_evidence")
        } else {
            None
        };
    let abstain_reason = if final_decision == "abstain" {
        Some("No sufficient allowed evidence remained after candidate generation and rerank")
    } else {
        None
    };
    let temporal_query_requested = payload["temporal_query_epoch_ms"].as_i64().is_some();
    let temporal_query_epoch_ms = payload["temporal_query_epoch_ms"].as_i64();
    let temporal_candidate_count = if temporal_query_requested {
        summary_layer_count + structured_layer_count + raw_layer_count + semantic_count
    } else {
        0
    };
    let historical_memory_card_count =
        count_temporal_historical_memory_cards(&payload["retrieval"]["memory_cards"]);
    let historical_raw_evidence_count =
        count_temporal_historical_raw_evidence(&payload["retrieval"]["raw_evidence"]);
    let prefilter_memory_card_count =
        read_temporal_stat(payload, "memory_cards", "prefilter_match_count");
    let excluded_memory_cards_by_temporal_window =
        read_temporal_stat(payload, "memory_cards", "excluded_by_temporal_window");
    let excluded_memory_cards_by_current_truth_state =
        read_temporal_stat(payload, "memory_cards", "excluded_by_current_truth_state");
    let prefilter_raw_evidence_count =
        read_temporal_stat(payload, "raw_evidence", "prefilter_match_count");
    let excluded_raw_evidence_by_temporal_window =
        read_temporal_stat(payload, "raw_evidence", "excluded_by_temporal_window");
    let excluded_raw_evidence_by_current_truth_state =
        read_temporal_stat(payload, "raw_evidence", "excluded_by_current_truth_state");
    let memory_card_excluded_candidates =
        payload["retrieval_temporal_stats"]["memory_cards"]["excluded_candidates"].clone();
    let raw_evidence_excluded_candidates =
        payload["retrieval_temporal_stats"]["raw_evidence"]["excluded_candidates"].clone();
    let temporal_explanation = if !temporal_query_requested {
        if excluded_memory_cards_by_current_truth_state > 0
            || excluded_raw_evidence_by_current_truth_state > 0
        {
            "Temporal slice was not requested; latest valid truth window was used and stale current-state candidates were excluded."
        } else {
            "Temporal slice was not requested; latest valid truth window was used."
        }
    } else if historical_memory_card_count > 0 || historical_raw_evidence_count > 0 {
        if excluded_memory_cards_by_temporal_window > 0
            || excluded_raw_evidence_by_temporal_window > 0
        {
            "Historical-but-valid candidates remained admissible at the requested timestamp while out-of-window candidates were excluded."
        } else {
            "Historical-but-valid candidates remained admissible at the requested timestamp."
        }
    } else if final_decision == "abstain" {
        "Requested timestamp left no sufficient admissible evidence after temporal legality filtering."
    } else {
        if excluded_memory_cards_by_temporal_window > 0
            || excluded_raw_evidence_by_temporal_window > 0
        {
            "Requested timestamp stayed within the currently valid truth window for returned evidence after excluding out-of-window candidates."
        } else {
            "Requested timestamp stayed within the currently valid truth window for returned evidence."
        }
    };

    json!({
        "intent_classifier": build_context_pack_intent_classifier(payload),
        "selection_priority": [
            "exact_documents",
            "symbol_hits",
            "lexical_chunks",
            "memory_cards",
            "memory_relation_edges",
            "raw_evidence",
            "semantic_chunks"
        ],
        "scope": {
            "project_code": payload["project"]["code"].clone(),
            "namespace_code": payload["namespace"]["code"].clone(),
            "effective_retrieval_mode": payload["effective_retrieval_mode"].clone(),
            "visible_projects_total": payload["visible_projects"].as_array().map(Vec::len).unwrap_or_default(),
        },
        "scope_resolver": {
            "project_code": payload["project"]["code"].clone(),
            "namespace_code": payload["namespace"]["code"].clone(),
            "effective_retrieval_mode": payload["effective_retrieval_mode"].clone(),
            "visible_projects_total": payload["visible_projects"].as_array().map(Vec::len).unwrap_or_default(),
            "status": "resolved"
        },
        "candidate_generation": {
            "exact_documents": exact_count,
            "symbol_hits": symbol_count,
            "lexical_chunks": lexical_count,
            "memory_cards": memory_card_count,
            "memory_relation_edges": memory_relation_edge_count,
            "raw_evidence": raw_evidence_count,
            "semantic_chunks": semantic_count,
            "grouped_strategies": {
                "exact": exact_count + symbol_count,
                "lexical": lexical_count,
                "graph": memory_relation_edge_count,
                "vector": semantic_count,
                "temporal": temporal_candidate_count,
            },
            "temporal_query_requested": temporal_query_requested,
        },
        "rerank_legality_relevance": {
            "scope_filtered": true,
            "temporal_query_epoch_ms": payload["temporal_query_epoch_ms"].clone(),
            "temporal_query_requested": temporal_query_requested,
            "temporal_legality": {
                "status": if temporal_query_requested {
                    "applied_exact_time_slice"
                } else {
                    "latest_truth_window"
                },
                "query_epoch_ms": temporal_query_epoch_ms,
                "prefilter_memory_cards": prefilter_memory_card_count,
                "memory_cards_after_filter": memory_card_count,
                "excluded_memory_cards_by_temporal_window": excluded_memory_cards_by_temporal_window,
                "excluded_memory_cards_by_current_truth_state": excluded_memory_cards_by_current_truth_state,
                "excluded_memory_card_candidates": memory_card_excluded_candidates,
                "prefilter_raw_evidence": prefilter_raw_evidence_count,
                "memory_relation_edges_after_filter": memory_relation_edge_count,
                "raw_evidence_after_filter": raw_evidence_count,
                "excluded_raw_evidence_by_temporal_window": excluded_raw_evidence_by_temporal_window,
                "excluded_raw_evidence_by_current_truth_state": excluded_raw_evidence_by_current_truth_state,
                "excluded_raw_evidence_candidates": raw_evidence_excluded_candidates,
                "historical_memory_cards_after_filter": historical_memory_card_count,
                "historical_raw_evidence_after_filter": historical_raw_evidence_count,
                "explanation": temporal_explanation,
            },
            "semantic_guard": semantic_guard.clone(),
        },
        "evidence_ladder": {
            "cheapest_sufficient_layer": cheapest_sufficient_layer,
            "layers": [
                {
                    "layer": "summary_compact",
                    "count": summary_layer_count,
                    "strategies": ["exact_documents", "symbol_hits", "lexical_chunks", "memory_cards"],
                    "sufficient": summary_sufficient,
                    "reason": summary_reason
                },
                {
                    "layer": "structured_graph",
                    "count": structured_layer_count,
                    "strategies": ["memory_relation_edges"],
                    "sufficient": !summary_sufficient && structured_sufficient,
                    "reason": structured_reason
                },
                {
                    "layer": "raw_evidence",
                    "count": raw_layer_count,
                    "strategies": ["raw_evidence"],
                    "sufficient": !summary_sufficient && !structured_sufficient && raw_layer_count > 0,
                    "reason": if raw_layer_count > 0 {
                        "raw_evidence_raised_after_summary_and_structured_were_insufficient"
                    } else {
                        "raw_evidence_not_required_or_not_found"
                    }
                },
                {
                    "layer": "semantic_candidates",
                    "count": semantic_count,
                    "strategies": ["semantic_chunks"],
                    "sufficient": false,
                    "reason": "semantic auxiliary lane is candidate generation support, not final evidence authority"
                }
            ]
        },
        "evidence_sufficiency_check": {
            "status": if final_decision == "abstain" { "insufficient" } else { "sufficient" },
            "next_action": next_action,
            "cheapest_sufficient_layer": cheapest_sufficient_layer,
        },
        "escalate_if_needed": {
            "required": !summary_sufficient && !structured_sufficient && raw_layer_count == 0,
            "target_layer": escalate_target_layer,
            "reason": if !summary_sufficient && !structured_sufficient && raw_layer_count > 0 {
                "summary and structured layers were insufficient, so the router escalated to raw evidence and found admissible sources"
            } else if !summary_sufficient && !structured_sufficient {
                "summary and structured layers were insufficient, and raw evidence was still missing"
            } else if raw_layer_count > 0 {
                "raw evidence was available but not required because a cheaper sufficient layer already existed"
            } else {
                "no raw escalation required"
            }
        },
        "abstain_if_insufficient": {
            "abstained": final_decision == "abstain",
            "reason": abstain_reason
        },
        "final_decision": final_decision,
        "included": included,
        "not_included": not_included,
        "semantic_guard": semantic_guard.clone(),
    })
}

fn build_retrieval_trace_insert<'a>(
    workspace_id: Uuid,
    prepared: &'a PreparedContextPack,
    args: &'a ContextPackArgs,
) -> postgres::RetrievalTraceInsert {
    let decision_trace = &prepared.payload["decision_trace"];
    let candidate_summary = json!({
        "candidate_generation": decision_trace["candidate_generation"].clone(),
        "selection_priority": decision_trace["selection_priority"].clone(),
    });
    let rerank_summary = json!({
        "scope": decision_trace["scope"].clone(),
        "scope_resolver": decision_trace["scope_resolver"].clone(),
        "rerank_legality_relevance": decision_trace["rerank_legality_relevance"].clone(),
        "included": decision_trace["included"].clone(),
        "not_included": decision_trace["not_included"].clone(),
    });
    let evidence_sufficiency = json!({
        "evidence_ladder": decision_trace["evidence_ladder"].clone(),
        "evidence_sufficiency_check": decision_trace["evidence_sufficiency_check"].clone(),
        "escalate_if_needed": decision_trace["escalate_if_needed"].clone(),
        "abstain_if_insufficient": decision_trace["abstain_if_insufficient"].clone(),
    });
    let artifact_refs = collect_trace_string_refs(&prepared.payload, "artifact_refs");
    let message_refs = collect_trace_string_refs(&prepared.payload, "message_refs");
    let source_event_ids = json!([format!("context_pack:{}", prepared.context_pack_id)]);
    let evidence_span = json!({
        "kind": "retrieval_trace",
        "context_pack_id": prepared.context_pack_id,
        "cheapest_sufficient_layer": decision_trace["evidence_ladder"]["cheapest_sufficient_layer"]
            .clone(),
        "final_decision": decision_trace["final_decision"].clone(),
    });
    postgres::RetrievalTraceInsert {
        workspace_id,
        project_id: prepared.project.project_id,
        namespace_id: prepared.namespace_id,
        context_pack_id: Some(prepared.context_pack_id),
        query_text: args.query.clone(),
        requested_mode: args.retrieval_mode.clone(),
        effective_mode: Some(prepared.effective_mode.clone()),
        scope_filter: decision_trace["scope"].clone(),
        candidate_summary,
        rerank_summary,
        evidence_sufficiency,
        source_kind: Some("context_pack_retrieval_runtime".to_string()),
        source_event_ids,
        artifact_refs,
        message_refs,
        evidence_span,
        derivation_kind: Some("extract".to_string()),
        schema_version: Some("retrieval-trace-envelope-v1".to_string()),
        final_decision: decision_trace["final_decision"]
            .as_str()
            .unwrap_or("abstain")
            .to_string(),
        temporal_query_epoch_ms: args.at_epoch_ms,
        trace_payload: decision_trace.clone(),
    }
}

fn collect_trace_string_refs(value: &Value, key: &str) -> Value {
    fn walk(value: &Value, key: &str, refs: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (entry_key, entry_value) in map {
                    if entry_key == key {
                        match entry_value {
                            Value::Array(items) => {
                                for item in items {
                                    if let Some(item) = item.as_str().filter(|item| !item.is_empty())
                                    {
                                        refs.push(item.to_string());
                                    }
                                }
                            }
                            Value::String(item) if !item.is_empty() => refs.push(item.clone()),
                            _ => {}
                        }
                    }
                    walk(entry_value, key, refs);
                }
            }
            Value::Array(items) => {
                for item in items {
                    walk(item, key, refs);
                }
            }
            _ => {}
        }
    }

    let mut refs = Vec::new();
    walk(value, key, &mut refs);
    refs.sort();
    refs.dedup();
    json!(refs)
}
