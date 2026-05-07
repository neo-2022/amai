use super::*;

pub(super) fn project_record_from_row(row: &Row) -> ProjectRecord {
    ProjectRecord {
        project_id: row.get(0),
        code: row.get(1),
        display_name: row.get(2),
        repo_root: row.get(3),
        visibility_scope: row.get(4),
        updated_at: row.get(5),
    }
}

pub(super) fn workspace_record_from_row(row: &Row) -> WorkspaceRecord {
    WorkspaceRecord {
        workspace_id: row.get(0),
        code: row.get(1),
        display_name: row.get(2),
        status: row.get(3),
    }
}

pub(super) fn team_record_from_row(row: &Row) -> TeamRecord {
    TeamRecord {
        team_id: row.get(0),
        workspace_code: row.get(1),
        code: row.get(2),
        display_name: row.get(3),
        status: row.get(4),
    }
}

pub(super) fn agent_role_record_from_row(row: &Row) -> AgentRoleRecord {
    AgentRoleRecord {
        role_id: row.get(0),
        workspace_code: row.get(1),
        code: row.get(2),
        display_name: row.get(3),
        status: row.get(4),
    }
}

pub(super) fn agent_record_from_row(row: &Row) -> AgentRecord {
    AgentRecord {
        agent_id: row.get(0),
        workspace_code: row.get(1),
        team_code: row.get(2),
        role_code: row.get(3),
        code: row.get(4),
        display_name: row.get(5),
        visibility_scope: row.get(6),
        status: row.get(7),
    }
}

pub(super) fn transfer_policy_record_from_row(row: &Row) -> TransferPolicyRecord {
    TransferPolicyRecord {
        transfer_policy_id: row.get(0),
        workspace_code: row.get(1),
        code: row.get(2),
        display_name: row.get(3),
        default_decision: row.get(4),
        allow_cross_project_read: row.get(5),
        allow_import: row.get(6),
        allow_verified_writeback: row.get(7),
        requires_human_approval: row.get(8),
    }
}

pub(super) fn access_policy_record_from_row(row: &Row) -> AccessPolicyRecord {
    AccessPolicyRecord {
        access_policy_id: row.get(0),
        workspace_code: row.get(1),
        team_code: row.get(2),
        project_code: row.get(3),
        role_code: row.get(4),
        code: row.get(5),
        display_name: row.get(6),
        object_class: row.get(7),
        scope_type: row.get(8),
        precedence: row.get(9),
        can_read: row.get(10),
        can_write: row.get(11),
        can_link: row.get(12),
        can_import: row.get(13),
        can_promote: row.get(14),
        can_share_further: row.get(15),
        can_archive: row.get(16),
        can_delete: row.get(17),
        can_quarantine: row.get(18),
        can_approve_transfer: row.get(19),
        human_override: row.get(20),
        override_reason: row.get(21),
        status: row.get(22),
    }
}

pub(super) fn shared_asset_record_from_row(row: &Row) -> SharedAssetRecord {
    SharedAssetRecord {
        shared_asset_id: row.get(0),
        workspace_code: row.get(1),
        code: row.get(2),
        display_name: row.get(3),
        asset_kind: row.get(4),
        source_project_code: row.get(5),
        transfer_policy_code: row.get(6),
        source_kind: row.get(7),
        source_event_ids: row.get(8),
        artifact_refs: row.get(9),
        message_refs: row.get(10),
        evidence_span: row.get(11),
        derivation_kind: row.get(12),
        schema_version: row.get(13),
        visibility_scope: row.get(14),
        status: row.get(15),
    }
}

pub(super) fn artifact_ref_record_from_row(row: &Row) -> ArtifactRefRecord {
    ArtifactRefRecord {
        artifact_ref_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        artifact_kind: row.get(4),
        bucket: row.get(5),
        object_key: row.get(6),
        content_type: row.get(7),
        source_kind: row.get(8),
        source_event_ids: row.get(9),
        message_refs: row.get(10),
        evidence_span: row.get(11),
        derivation_kind: row.get(12),
        schema_version: row.get(13),
        metadata: row.get(14),
    }
}

pub(super) fn memory_raw_event_record_from_row(row: &Row) -> MemoryRawEventRecord {
    MemoryRawEventRecord {
        memory_raw_event_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        source_project_code: row.get(4),
        import_packet_id: row.get(5),
        owner_agent_id: row.get(6),
        event_kind: row.get(7),
        item_kind: row.get(8),
        visibility_scope: row.get(9),
        sensitivity_class: row.get(10),
        derivation_kind: row.get(11),
        truth_state: row.get(12),
        trust_state: row.get(13),
        verification_state: row.get(14),
        lifecycle_state: row.get(15),
        identity_key: row.get(16),
        title: row.get(17),
        summary: row.get(18),
        body: row.get(19),
        source_event_ids: row.get(20),
        artifact_refs: row.get(21),
        message_refs: row.get(22),
        evidence_span: row.get(23),
        causation_id: row.get(24),
        correlation_id: row.get(25),
        source_epoch_ns: row.get(26),
        source_monotonic_ns: row.get(27),
        server_received_at_epoch_ms: row.get(28),
        server_order_seq: row.get(29),
        payload: row.get(30),
    }
}

pub(super) fn memory_write_outbox_record_from_row(row: &Row) -> MemoryWriteOutboxRecord {
    MemoryWriteOutboxRecord {
        memory_write_outbox_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        memory_raw_event_id: row.get(4),
        memory_item_id: row.get(5),
        subject: row.get(6),
        delivery_kind: row.get(7),
        delivery_state: row.get(8),
        payload: row.get(9),
        attempt_count: row.get(10),
        last_error: row.get(11),
        published_at_epoch_ms: row.get(12),
        acknowledged_at_epoch_ms: row.get(13),
    }
}

pub(super) fn context_pack_record_from_row(row: &Row) -> ContextPackRecord {
    ContextPackRecord {
        context_pack_id: row.get(0),
        project_code: row.get(1),
        namespace_code: row.get(2),
        retrieval_mode: row.get(3),
        query_text: row.get(4),
        visible_projects: row.get(5),
        payload: row.get(6),
        artifact_ref_id: row.get(7),
        artifact_bucket: row.get(8),
        artifact_object_key: row.get(9),
        artifact_state: row.get(10),
        artifact_last_error: row.get(11),
        artifact_updated_at_epoch_ms: row.get(12),
    }
}

pub(super) fn restore_pack_record_from_row(row: &Row) -> RestorePackRecord {
    RestorePackRecord {
        restore_pack_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        agent_scope: row.get(4),
        session_id: row.get(5),
        thread_id: row.get(6),
        source_snapshot_id: row.get(7),
        pack_kind: row.get(8),
        source_kind: row.get(9),
        source_event_ids: row.get(10),
        artifact_refs: row.get(11),
        message_refs: row.get(12),
        evidence_span: row.get(13),
        derivation_kind: row.get(14),
        schema_version: row.get(15),
        headline: row.get(16),
        summary: row.get(17),
        payload: row.get(18),
        captured_at_epoch_ms: row.get(19),
    }
}

pub(super) fn policy_rule_record_from_row(row: &Row) -> PolicyRuleRecord {
    PolicyRuleRecord {
        policy_rule_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        rule_code: row.get(4),
        rule_scope: row.get(5),
        rule_kind: row.get(6),
        rule_status: row.get(7),
        precedence: row.get(8),
        source_kind: row.get(9),
        source_event_ids: row.get(10),
        artifact_refs: row.get(11),
        message_refs: row.get(12),
        evidence_span: row.get(13),
        derivation_kind: row.get(14),
        schema_version: row.get(15),
        rule_payload: row.get(16),
    }
}

pub(super) fn quarantine_item_record_from_row(row: &Row) -> QuarantineItemRecord {
    QuarantineItemRecord {
        quarantine_item_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        entity_kind: row.get(4),
        entity_id: row.get(5),
        quarantine_reason: row.get(6),
        quarantine_state: row.get(7),
        evidence: row.get(8),
        source_kind: row.get(9),
        source_event_ids: row.get(10),
        artifact_refs: row.get(11),
        message_refs: row.get(12),
        evidence_span: row.get(13),
        derivation_kind: row.get(14),
        schema_version: row.get(15),
        quarantined_at_epoch_ms: row.get(16),
        released_at_epoch_ms: row.get(17),
    }
}

pub(super) fn memory_edge_record_from_row(row: &Row) -> MemoryEdgeRecord {
    MemoryEdgeRecord {
        memory_edge_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        source_memory_item_id: row.get(4),
        target_memory_item_id: row.get(5),
        edge_kind: row.get(6),
        edge_state: row.get(7),
        trust_state: row.get(8),
        validity_basis: row.get(9),
        score: row.get(10),
        evidence: row.get(11),
        source_kind: row.get(12),
        source_event_ids: row.get(13),
        artifact_refs: row.get(14),
        message_refs: row.get(15),
        evidence_span: row.get(16),
        derivation_kind: row.get(17),
        schema_version: row.get(18),
        valid_from_epoch_ms: row.get(19),
        valid_to_epoch_ms: row.get(20),
    }
}

pub(super) fn memory_conflict_record_from_row(row: &Row) -> MemoryConflictRecord {
    MemoryConflictRecord {
        memory_conflict_id: row.get(0),
        workspace_code: row.get(1),
        project_code: row.get(2),
        namespace_code: row.get(3),
        left_memory_item_id: row.get(4),
        right_memory_item_id: row.get(5),
        conflict_kind: row.get(6),
        conflict_state: row.get(7),
        severity: row.get(8),
        summary: row.get(9),
        evidence: row.get(10),
        source_kind: row.get(11),
        source_event_ids: row.get(12),
        artifact_refs: row.get(13),
        message_refs: row.get(14),
        evidence_span: row.get(15),
        derivation_kind: row.get(16),
        schema_version: row.get(17),
        resolution: row.get(18),
        detected_at_epoch_ms: row.get(19),
        resolved_at_epoch_ms: row.get(20),
    }
}
