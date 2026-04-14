use super::*;

pub(super) async fn collect_governance_surface(db: &Client) -> Result<Value> {
    let open_conflicts: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_conflicts
            WHERE conflict_state = 'open'
            "#,
            &[],
        )
        .await
        .context("governance: count open conflicts")?
        .get(0);

    let active_quarantine: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.quarantine_items
            WHERE quarantine_state = 'active'
            "#,
            &[],
        )
        .await
        .context("governance: count active quarantine items")?
        .get(0);

    let poisoned_provenance: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_provenance
            WHERE details->>'poisoned' = 'true'
               OR details->'safety'->>'poisoned' = 'true'
            "#,
            &[],
        )
        .await
        .context("governance: count poisoned provenance")?
        .get(0);

    let disputed_memory_items: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_items
            WHERE trust_state = 'disputed'
            "#,
            &[],
        )
        .await
        .context("governance: count disputed memory items")?
        .get(0);

    let quarantined_memory_items: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_items
            WHERE trust_state = 'quarantined'
            "#,
            &[],
        )
        .await
        .context("governance: count quarantined memory items")?
        .get(0);

    let stale_memory_items: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_items
            WHERE consolidation_status IN ('archived', 'pruned')
            "#,
            &[],
        )
        .await
        .context("governance: count stale (archived/pruned) memory items")?
        .get(0);

    let total_memory_items: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_items
            "#,
            &[],
        )
        .await
        .context("governance: count total memory items")?
        .get(0);

    let active_memory_items: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.memory_items
            WHERE consolidation_status = 'active'
            "#,
            &[],
        )
        .await
        .context("governance: count active memory items")?
        .get(0);

    let duplicate_fact_triples: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0) FROM (
                SELECT fact_subject, fact_predicate, fact_object
                FROM ami.memory_cards
                WHERE truth_state = 'current'
                  AND status = 'active'
                  AND fact_subject IS NOT NULL
                  AND fact_predicate IS NOT NULL
                  AND fact_object IS NOT NULL
                  AND superseded_by_memory_card_id IS NULL
                GROUP BY fact_subject, fact_predicate, fact_object
                HAVING COUNT(*) > 1
            ) dup
            "#,
            &[],
        )
        .await
        .context("governance: count duplicate active truth fact triples")?
        .get(0);

    let scope_override_events_count: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.scope_override_events
            "#,
            &[],
        )
        .await
        .context("governance: count scope override events")?
        .get(0);

    let forgetting_actions_count: i64 = db
        .query_one(
            r#"
            SELECT COALESCE(COUNT(*)::bigint, 0)
            FROM ami.forgetting_audit_log
            "#,
            &[],
        )
        .await
        .context("governance: count forgetting audit log entries")?
        .get(0);

    let forgetting_action_breakdown_rows = db
        .query(
            r#"
            SELECT action, COUNT(*)::bigint
            FROM ami.forgetting_audit_log
            GROUP BY action
            ORDER BY action ASC
            "#,
            &[],
        )
        .await
        .context("governance: forgetting action breakdown")?;

    let quarantine_breakdown_rows = db
        .query(
            r#"
            SELECT
                COALESCE(quarantine_reason, 'unknown') AS quarantine_reason,
                COALESCE(entity_kind, 'unknown') AS entity_kind,
                COALESCE(source_kind, 'unknown') AS source_kind,
                COUNT(*)::bigint AS item_count
            FROM ami.quarantine_items
            WHERE quarantine_state = 'active'
            GROUP BY 1, 2, 3
            ORDER BY item_count DESC, quarantine_reason ASC, entity_kind ASC, source_kind ASC
            LIMIT 5
            "#,
            &[],
        )
        .await
        .context("governance: quarantine breakdown")?;

    let conflict_breakdown_rows = db
        .query(
            r#"
            SELECT
                COALESCE(summary, 'unknown') AS summary,
                COALESCE(source_kind, 'unknown') AS source_kind,
                COUNT(*)::bigint AS item_count
            FROM ami.memory_conflicts
            WHERE conflict_state = 'open'
            GROUP BY 1, 2
            ORDER BY item_count DESC, summary ASC, source_kind ASC
            LIMIT 5
            "#,
            &[],
        )
        .await
        .context("governance: conflict breakdown")?;

    let mut prune_ttl_expired_count = 0_i64;
    let mut prune_low_utility_count = 0_i64;
    let mut archive_cold_tier_count = 0_i64;
    let mut revalidate_stale_count = 0_i64;
    let mut dedup_compacted_count = 0_i64;
    let mut forgetting_action_breakdown = serde_json::Map::new();
    for row in forgetting_action_breakdown_rows {
        let action: String = row.get(0);
        let count: i64 = row.get(1);
        match action.as_str() {
            "prune_ttl_expired" => prune_ttl_expired_count = count,
            "prune_low_utility" => prune_low_utility_count = count,
            "archive_cold_tier" => archive_cold_tier_count = count,
            "revalidate_stale" => revalidate_stale_count = count,
            "dedup_compacted" => dedup_compacted_count = count,
            _ => {}
        }
        forgetting_action_breakdown.insert(action, json!(count));
    }

    let forgetting_job_breakdown = json!({
        "de_duplication_job": dedup_compacted_count,
        "summarization_job": 0,
        "compaction_job": dedup_compacted_count,
        "pruning_job": prune_ttl_expired_count + prune_low_utility_count,
        "cold_archive_job": archive_cold_tier_count,
        "revalidation_job": revalidate_stale_count
    });

    let quarantine_breakdown: Vec<Value> = quarantine_breakdown_rows
        .into_iter()
        .map(|row| {
            let reason: String = row.get(0);
            let entity_kind: String = row.get(1);
            let source_kind: String = row.get(2);
            let item_count: i64 = row.get(3);
            json!({
                "quarantine_reason": reason,
                "entity_kind": entity_kind,
                "source_kind": source_kind,
                "item_count": item_count
            })
        })
        .collect();

    let conflict_breakdown: Vec<Value> = conflict_breakdown_rows
        .into_iter()
        .map(|row| {
            let summary: String = row.get(0);
            let source_kind: String = row.get(1);
            let item_count: i64 = row.get(2);
            json!({
                "summary": summary,
                "source_kind": source_kind,
                "item_count": item_count
            })
        })
        .collect();

    let stale_memory_error_rate = if total_memory_items > 0 {
        stale_memory_items as f64 / total_memory_items as f64
    } else {
        0.0
    };

    let duplicate_branch_rate = if total_memory_items > 0 {
        duplicate_fact_triples as f64 / total_memory_items as f64
    } else {
        0.0
    };

    Ok(json!({
        "governance_surface_version": "governance-surface-v2",
        "wrong_link_rate": {
            "open_conflict_count": open_conflicts,
            "note": "wrong-link rate is proxied by open memory_conflicts with kind='scope' or 'truth'"
        },
        "duplicate_branch_rate": {
            "duplicate_active_truth_fact_triples": duplicate_fact_triples,
            "rate": duplicate_branch_rate,
            "note": "duplicate truth branches: same fact triple active as current without supersession"
        },
        "stale_memory_error_rate": {
            "stale_items_archived_or_pruned": stale_memory_items,
            "total_memory_items": total_memory_items,
            "active_memory_items": active_memory_items,
            "rate": stale_memory_error_rate,
            "note": "ratio of archived/pruned items to total — higher means more aggressive cleanup"
        },
        "cross_project_leak_rate": {
            "note": "surfaced via degradation_model.cross_project_scope and latest_retrieval_accuracy.accuracy_verification.cross_project_leakage"
        },
        "poisoning_alert_count": {
            "poisoned_provenance_count": poisoned_provenance,
            "active_quarantine_items": active_quarantine,
            "quarantined_memory_items": quarantined_memory_items,
            "active_quarantine_breakdown": quarantine_breakdown,
            "note": "sum of poisoned provenance marks and active quarantine items"
        },
        "open_conflict_breakdown": conflict_breakdown,
        "abstention_quality": {
            "note": "surfaced via continuity_correctness_model and latest_memory_benchmark_score.memory_benchmark_score.capability_breakdown.longmemeval_abstention_accuracy"
        },
        "recovery_quality": {
            "note": "surfaced via continuity_correctness_model.summary.recovered_useful"
        },
        "trust_state_distribution": {
            "disputed_memory_items": disputed_memory_items,
            "quarantined_memory_items": quarantined_memory_items
        },
        "human_override_audit": {
            "scope_override_events_total": scope_override_events_count,
            "forgetting_audit_log_entries_total": forgetting_actions_count
        },
        "forgetting_job_breakdown": forgetting_job_breakdown,
        "forgetting_action_breakdown": forgetting_action_breakdown
    }))
}
