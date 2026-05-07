CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE SCHEMA IF NOT EXISTS ami;

CREATE TABLE IF NOT EXISTS ami.workspaces (
    workspace_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL UNIQUE CHECK (code <> ''),
    display_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived', 'disabled')),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO ami.workspaces(code, display_name, status)
VALUES ('default', 'Default workspace', 'active')
ON CONFLICT (code) DO UPDATE SET
    display_name = EXCLUDED.display_name,
    status = EXCLUDED.status,
    updated_at = now();

CREATE TABLE IF NOT EXISTS ami.teams (
    team_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    code TEXT NOT NULL CHECK (code <> ''),
    display_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived', 'disabled')),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, code)
);

CREATE TABLE IF NOT EXISTS ami.agent_roles (
    role_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    code TEXT NOT NULL CHECK (code <> ''),
    display_name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived', 'disabled')),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, code)
);

CREATE TABLE IF NOT EXISTS ami.transfer_policies (
    transfer_policy_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    code TEXT NOT NULL CHECK (code <> ''),
    display_name TEXT NOT NULL,
    default_decision TEXT NOT NULL CHECK (
        default_decision IN (
            'default_deny',
            'manual_review',
            'borrowed_unverified',
            'verified_writeback'
        )
    ),
    allow_cross_project_read BOOLEAN NOT NULL DEFAULT FALSE,
    allow_import BOOLEAN NOT NULL DEFAULT FALSE,
    allow_verified_writeback BOOLEAN NOT NULL DEFAULT FALSE,
    requires_human_approval BOOLEAN NOT NULL DEFAULT TRUE,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, code)
);

CREATE TABLE IF NOT EXISTS ami.projects (
    project_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE RESTRICT,
    code TEXT NOT NULL UNIQUE CHECK (code <> ''),
    display_name TEXT NOT NULL,
    repo_root TEXT NOT NULL UNIQUE,
    default_branch TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived', 'disabled')),
    visibility_scope TEXT NOT NULL DEFAULT 'project_shared' CHECK (
        visibility_scope IN (
            'agent_private',
            'team_shared',
            'project_shared',
            'cross_project_linked',
            'org_global',
            'quarantine',
            'imported'
        )
    ),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.projects
    ADD COLUMN IF NOT EXISTS workspace_id UUID REFERENCES ami.workspaces(workspace_id) ON DELETE RESTRICT;

UPDATE ami.projects
SET workspace_id = w.workspace_id
FROM ami.workspaces w
WHERE ami.projects.workspace_id IS NULL
  AND w.code = 'default';

ALTER TABLE ami.projects
    ALTER COLUMN workspace_id SET NOT NULL;

ALTER TABLE ami.projects
    ADD COLUMN IF NOT EXISTS visibility_scope TEXT NOT NULL DEFAULT 'project_shared';

UPDATE ami.projects
SET visibility_scope = 'project_shared'
WHERE visibility_scope IS NULL
   OR visibility_scope NOT IN (
       'agent_private',
       'team_shared',
       'project_shared',
       'cross_project_linked',
       'org_global',
       'quarantine',
       'imported'
   );

CREATE TABLE IF NOT EXISTS ami.project_repo_roots (
    repo_root_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    repo_root TEXT NOT NULL UNIQUE,
    root_kind TEXT NOT NULL CHECK (
        root_kind IN ('primary', 'relocated_from', 'workspace_alias')
    ),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_project_repo_roots_primary_per_project
    ON ami.project_repo_roots(project_id)
    WHERE root_kind = 'primary';

CREATE INDEX IF NOT EXISTS idx_project_repo_roots_project_id
    ON ami.project_repo_roots(project_id);

UPDATE ami.project_repo_roots r
SET root_kind = 'relocated_from',
    updated_at = now()
FROM ami.projects p
WHERE r.project_id = p.project_id
  AND r.repo_root <> p.repo_root
  AND r.root_kind = 'primary';

UPDATE ami.project_repo_roots r
SET root_kind = 'primary',
    updated_at = now()
FROM ami.projects p
WHERE r.project_id = p.project_id
  AND r.repo_root = p.repo_root
  AND r.root_kind <> 'primary';

INSERT INTO ami.project_repo_roots(project_id, repo_root, root_kind)
SELECT p.project_id, p.repo_root, 'primary'
FROM ami.projects p
WHERE NOT EXISTS (
    SELECT 1
    FROM ami.project_repo_roots r
    WHERE r.repo_root = p.repo_root
);

CREATE TABLE IF NOT EXISTS ami.namespaces (
    namespace_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    code TEXT NOT NULL CHECK (code <> ''),
    display_name TEXT NOT NULL,
    retrieval_mode TEXT NOT NULL CHECK (
        retrieval_mode IN ('local_strict', 'local_plus_related', 'explicit_foreign', 'audit_global')
    ),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, code)
);

CREATE TABLE IF NOT EXISTS ami.project_relations (
    relation_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    target_project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL,
    project_link_type TEXT NOT NULL,
    shared_contour TEXT NOT NULL,
    visibility_scope TEXT NOT NULL DEFAULT 'cross_project_linked' CHECK (
        visibility_scope IN (
            'agent_private',
            'team_shared',
            'project_shared',
            'cross_project_linked',
            'org_global',
            'quarantine',
            'imported'
        )
    ),
    relation_status TEXT NOT NULL DEFAULT 'active' CHECK (
        relation_status IN ('active', 'disabled', 'forbidden', 'quarantined')
    ),
    requires_approval BOOLEAN NOT NULL DEFAULT FALSE,
    transfer_policy_id UUID REFERENCES ami.transfer_policies(transfer_policy_id) ON DELETE SET NULL,
    access_mode TEXT NOT NULL CHECK (
        access_mode IN ('local_strict', 'local_plus_related', 'explicit_foreign', 'audit_global')
    ),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_project_id, target_project_id, relation_type, shared_contour)
);

ALTER TABLE ami.project_relations
    ADD COLUMN IF NOT EXISTS project_link_type TEXT;

UPDATE ami.project_relations
SET project_link_type = relation_type
WHERE project_link_type IS NULL
   OR btrim(project_link_type) = '';

ALTER TABLE ami.project_relations
    ALTER COLUMN project_link_type SET NOT NULL;

ALTER TABLE ami.project_relations
    ADD COLUMN IF NOT EXISTS visibility_scope TEXT NOT NULL DEFAULT 'cross_project_linked';

UPDATE ami.project_relations
SET visibility_scope = 'cross_project_linked'
WHERE visibility_scope IS NULL
   OR visibility_scope NOT IN (
       'agent_private',
       'team_shared',
       'project_shared',
       'cross_project_linked',
       'org_global',
       'quarantine',
       'imported'
   );

ALTER TABLE ami.project_relations
    ADD COLUMN IF NOT EXISTS relation_status TEXT NOT NULL DEFAULT 'active';

UPDATE ami.project_relations
SET relation_status = 'active'
WHERE relation_status IS NULL
   OR relation_status NOT IN ('active', 'disabled', 'forbidden', 'quarantined');

ALTER TABLE ami.project_relations
    ADD COLUMN IF NOT EXISTS requires_approval BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE ami.project_relations
    ADD COLUMN IF NOT EXISTS transfer_policy_id UUID REFERENCES ami.transfer_policies(transfer_policy_id) ON DELETE SET NULL;

CREATE TABLE IF NOT EXISTS ami.retrieval_policies (
    policy_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    policy_code TEXT NOT NULL UNIQUE,
    default_mode TEXT NOT NULL CHECK (
        default_mode IN ('local_strict', 'local_plus_related', 'explicit_foreign', 'audit_global')
    ),
    allow_local BOOLEAN NOT NULL DEFAULT TRUE,
    allow_related BOOLEAN NOT NULL DEFAULT FALSE,
    allow_foreign BOOLEAN NOT NULL DEFAULT FALSE,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.agents (
    agent_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE RESTRICT,
    team_id UUID REFERENCES ami.teams(team_id) ON DELETE SET NULL,
    visibility_scope TEXT NOT NULL DEFAULT 'agent_private' CHECK (
        visibility_scope IN (
            'agent_private',
            'team_shared',
            'project_shared',
            'cross_project_linked',
            'org_global',
            'quarantine',
            'imported'
        )
    ),
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived', 'disabled')),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.agents
    ADD COLUMN IF NOT EXISTS workspace_id UUID REFERENCES ami.workspaces(workspace_id) ON DELETE RESTRICT;

UPDATE ami.agents
SET workspace_id = w.workspace_id
FROM ami.workspaces w
WHERE ami.agents.workspace_id IS NULL
  AND w.code = 'default';

ALTER TABLE ami.agents
    ALTER COLUMN workspace_id SET NOT NULL;

ALTER TABLE ami.agents
    ADD COLUMN IF NOT EXISTS team_id UUID REFERENCES ami.teams(team_id) ON DELETE SET NULL;

ALTER TABLE ami.agents
    ADD COLUMN IF NOT EXISTS role_id UUID REFERENCES ami.agent_roles(role_id) ON DELETE SET NULL;

ALTER TABLE ami.agents
    ADD COLUMN IF NOT EXISTS visibility_scope TEXT NOT NULL DEFAULT 'agent_private';

UPDATE ami.agents
SET visibility_scope = 'agent_private'
WHERE visibility_scope IS NULL
   OR visibility_scope NOT IN (
       'agent_private',
       'team_shared',
       'project_shared',
       'cross_project_linked',
       'org_global',
       'quarantine',
       'imported'
   );

ALTER TABLE ami.agents
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active';

UPDATE ami.agents
SET status = 'active'
WHERE status IS NULL
   OR status NOT IN ('active', 'archived', 'disabled');

CREATE TABLE IF NOT EXISTS ami.access_policies (
    access_policy_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    team_id UUID REFERENCES ami.teams(team_id) ON DELETE SET NULL,
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE SET NULL,
    role_id UUID REFERENCES ami.agent_roles(role_id) ON DELETE SET NULL,
    code TEXT NOT NULL CHECK (code <> ''),
    display_name TEXT NOT NULL,
    object_class TEXT NOT NULL CHECK (
        object_class IN (
            'task',
            'fact',
            'artifact',
            'policy',
            'procedure',
            'raw_log',
            'benchmark_evidence'
        )
    ),
    scope_type TEXT NOT NULL CHECK (
        scope_type IN (
            'agent_private',
            'team_shared',
            'project_shared',
            'cross_project_linked',
            'org_global',
            'quarantine',
            'imported'
        )
    ),
    precedence INTEGER NOT NULL DEFAULT 100,
    can_read BOOLEAN NOT NULL DEFAULT FALSE,
    can_write BOOLEAN NOT NULL DEFAULT FALSE,
    can_link BOOLEAN NOT NULL DEFAULT FALSE,
    can_import BOOLEAN NOT NULL DEFAULT FALSE,
    can_promote BOOLEAN NOT NULL DEFAULT FALSE,
    can_share_further BOOLEAN NOT NULL DEFAULT FALSE,
    can_archive BOOLEAN NOT NULL DEFAULT FALSE,
    can_delete BOOLEAN NOT NULL DEFAULT FALSE,
    can_quarantine BOOLEAN NOT NULL DEFAULT FALSE,
    can_approve_transfer BOOLEAN NOT NULL DEFAULT FALSE,
    human_override BOOLEAN NOT NULL DEFAULT FALSE,
    override_reason TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (
        status IN ('active', 'archived', 'disabled', 'quarantined')
    ),
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'shared-asset-envelope-v1',
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, code)
);

INSERT INTO ami.transfer_policies(
    workspace_id,
    code,
    display_name,
    default_decision,
    allow_cross_project_read,
    allow_import,
    allow_verified_writeback,
    requires_human_approval
)
SELECT
    w.workspace_id,
    'default_deny',
    'Default deny',
    'default_deny',
    FALSE,
    FALSE,
    FALSE,
    TRUE
FROM ami.workspaces w
WHERE w.code = 'default'
ON CONFLICT (workspace_id, code) DO UPDATE SET
    display_name = EXCLUDED.display_name,
    default_decision = EXCLUDED.default_decision,
    allow_cross_project_read = EXCLUDED.allow_cross_project_read,
    allow_import = EXCLUDED.allow_import,
    allow_verified_writeback = EXCLUDED.allow_verified_writeback,
    requires_human_approval = EXCLUDED.requires_human_approval,
    updated_at = now();

CREATE TABLE IF NOT EXISTS ami.import_packets (
    import_packet_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    target_project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    transfer_policy_id UUID REFERENCES ami.transfer_policies(transfer_policy_id) ON DELETE SET NULL,
    requested_by_agent_id UUID REFERENCES ami.agents(agent_id) ON DELETE SET NULL,
    status TEXT NOT NULL CHECK (
        status IN (
            'proposed',
            'borrowed_unverified',
            'verified',
            'rejected',
            'revoked',
            'quarantined'
        )
    ),
    summary TEXT,
    allowed_by_project_link BOOLEAN NOT NULL DEFAULT FALSE,
    memory_object_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'import-packet-envelope-v1',
    reason TEXT,
    imported_by_agent_scope TEXT NOT NULL DEFAULT 'imported' CHECK (
        imported_by_agent_scope IN (
            'agent_private',
            'team_shared',
            'project_shared',
            'cross_project_linked',
            'org_global',
            'quarantine',
            'imported'
        )
    ),
    imported_at TIMESTAMPTZ,
    trust_state TEXT NOT NULL DEFAULT 'proposed' CHECK (
        trust_state IN (
            'raw',
            'extracted',
            'proposed',
            'verified',
            'disputed',
            'quarantined',
            'deprecated'
        )
    ),
    verification_state TEXT NOT NULL DEFAULT 'unverified' CHECK (
        verification_state IN (
            'unverified',
            'verified',
            'rejected',
            'disputed'
        )
    ),
    borrowed_status TEXT NOT NULL DEFAULT 'borrowed' CHECK (
        borrowed_status IN (
            'borrowed',
            'unverified',
            'verified_local_copy',
            'rejected',
            'expired'
        )
    ),
    can_promote_after_verification BOOLEAN NOT NULL DEFAULT FALSE,
    updated_by_agent_id UUID REFERENCES ami.agents(agent_id) ON DELETE SET NULL,
    override_reason TEXT,
    provenance JSONB NOT NULL DEFAULT '{}'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS allowed_by_project_link BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS memory_object_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'import-packet-envelope-v1';

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS reason TEXT;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS imported_by_agent_scope TEXT NOT NULL DEFAULT 'imported';

UPDATE ami.import_packets
SET imported_by_agent_scope = 'imported'
WHERE imported_by_agent_scope IS NULL
   OR imported_by_agent_scope NOT IN (
       'agent_private',
       'team_shared',
       'project_shared',
       'cross_project_linked',
       'org_global',
       'quarantine',
       'imported'
   );

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS imported_at TIMESTAMPTZ;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS trust_state TEXT NOT NULL DEFAULT 'proposed';

UPDATE ami.import_packets
SET trust_state = 'proposed'
WHERE trust_state IS NULL
   OR trust_state NOT IN (
       'raw',
       'extracted',
       'proposed',
       'verified',
       'disputed',
       'quarantined',
       'deprecated'
   );

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS verification_state TEXT NOT NULL DEFAULT 'unverified';

UPDATE ami.import_packets
SET verification_state = 'unverified'
WHERE verification_state IS NULL
   OR verification_state NOT IN ('unverified', 'verified', 'rejected', 'disputed');

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS borrowed_status TEXT NOT NULL DEFAULT 'borrowed';

UPDATE ami.import_packets
SET borrowed_status = 'borrowed'
WHERE borrowed_status IS NULL
   OR borrowed_status NOT IN (
       'borrowed',
       'unverified',
       'verified_local_copy',
       'rejected',
       'expired'
   );

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS can_promote_after_verification BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS updated_by_agent_id UUID REFERENCES ami.agents(agent_id) ON DELETE SET NULL;

ALTER TABLE ami.import_packets
    ADD COLUMN IF NOT EXISTS override_reason TEXT;

UPDATE ami.import_packets
SET derivation_kind = 'extract'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint c
        JOIN pg_class t ON t.oid = c.conrelid
        JOIN pg_namespace n ON n.oid = t.relnamespace
        WHERE n.nspname = 'ami'
          AND t.relname = 'import_packets'
          AND c.conname = 'import_packets_derivation_kind_check'
    ) THEN
        ALTER TABLE ami.import_packets
            ADD CONSTRAINT import_packets_derivation_kind_check CHECK (
                derivation_kind IN (
                    'raw_capture',
                    'extract',
                    'summary',
                    'merge',
                    'import',
                    'verified_write_back',
                    'operator_write'
                )
            );
    END IF;
EXCEPTION
    WHEN duplicate_object THEN
        NULL;
END
$$;

CREATE TABLE IF NOT EXISTS ami.skill_cards (
    skill_card_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID NOT NULL REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    skill_id TEXT NOT NULL CHECK (skill_id <> ''),
    skill_version INTEGER NOT NULL DEFAULT 1 CHECK (skill_version > 0),
    skill_title TEXT NOT NULL CHECK (skill_title <> ''),
    skill_goal TEXT NOT NULL CHECK (skill_goal <> ''),
    skill_trigger_conditions JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_preconditions JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_execution_steps JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_stop_conditions JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_forbidden_when JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_expected_outcome TEXT,
    skill_scope_type TEXT NOT NULL DEFAULT 'project_private' CHECK (
        skill_scope_type IN ('project_private', 'project_shared', 'team_shared', 'cross_project_linked')
    ),
    skill_owner_scope TEXT NOT NULL DEFAULT 'project' CHECK (
        skill_owner_scope IN ('project', 'workspace', 'agent_private', 'shared_candidate')
    ),
    skill_trust_state TEXT NOT NULL DEFAULT 'candidate' CHECK (
        skill_trust_state IN ('candidate', 'shadow', 'trial', 'verified', 'deprecated', 'quarantined')
    ),
    skill_verification_state TEXT NOT NULL DEFAULT 'unverified' CHECK (
        skill_verification_state IN (
            'unverified',
            'evidence_attached',
            'shadow_ready',
            'trial_ready',
            'verified',
            'rejected'
        )
    ),
    skill_runtime_constraints JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_model_constraints JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_tool_constraints JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_context_constraints JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    skill_evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    skill_candidate_class TEXT NOT NULL DEFAULT 'skill_hint' CHECK (
        skill_candidate_class IN (
            'fact',
            'decision',
            'commitment',
            'skill_hint',
            'artifact_ref',
            'failure_pattern',
            'failure_playbook',
            'repair_sequence',
            'anti_pattern'
        )
    ),
    skill_derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        skill_derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    skill_source_kind TEXT,
    skill_hot_path_write_eligible BOOLEAN NOT NULL DEFAULT FALSE,
    skill_background_consolidation_recommended BOOLEAN NOT NULL DEFAULT TRUE,
    skill_success_count INTEGER NOT NULL DEFAULT 0,
    skill_failure_count INTEGER NOT NULL DEFAULT 0,
    skill_reuse_count INTEGER NOT NULL DEFAULT 0,
    skill_shadow_pass_count INTEGER NOT NULL DEFAULT 0,
    skill_shadow_fail_count INTEGER NOT NULL DEFAULT 0,
    skill_last_used_at TIMESTAMPTZ,
    skill_last_verified_at TIMESTAMPTZ,
    skill_patch_parent_id UUID,
    skill_merge_group_id UUID,
    skill_utility_score DOUBLE PRECISION NOT NULL DEFAULT 0,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (namespace_id, skill_id, skill_version)
);

ALTER TABLE ami.skill_cards
    ADD COLUMN IF NOT EXISTS skill_evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS skill_candidate_class TEXT NOT NULL DEFAULT 'skill_hint',
    ADD COLUMN IF NOT EXISTS skill_derivation_kind TEXT NOT NULL DEFAULT 'extract',
    ADD COLUMN IF NOT EXISTS skill_source_kind TEXT,
    ADD COLUMN IF NOT EXISTS skill_hot_path_write_eligible BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS skill_background_consolidation_recommended BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS skill_patch_parent_id UUID,
    ADD COLUMN IF NOT EXISTS skill_merge_group_id UUID,
    ADD COLUMN IF NOT EXISTS skill_shared_promotion_state TEXT NOT NULL DEFAULT 'not_applicable',
    ADD COLUMN IF NOT EXISTS skill_shared_approved_by TEXT,
    ADD COLUMN IF NOT EXISTS skill_shared_approval_reason TEXT,
    ADD COLUMN IF NOT EXISTS skill_shared_approved_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_skill_cards_patch_parent
    ON ami.skill_cards(skill_patch_parent_id)
    WHERE skill_patch_parent_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_skill_cards_merge_group
    ON ami.skill_cards(skill_merge_group_id)
    WHERE skill_merge_group_id IS NOT NULL;

UPDATE ami.skill_cards
SET skill_candidate_class = 'skill_hint'
WHERE skill_candidate_class IS NULL
   OR skill_candidate_class NOT IN (
       'fact',
       'decision',
       'commitment',
       'skill_hint',
       'artifact_ref',
       'failure_pattern',
       'failure_playbook',
       'repair_sequence',
       'anti_pattern'
   );

ALTER TABLE ami.skill_cards
    DROP CONSTRAINT IF EXISTS skill_cards_candidate_class_check;

ALTER TABLE ami.skill_cards
    ADD CONSTRAINT skill_cards_candidate_class_check CHECK (
        skill_candidate_class IN (
            'fact',
            'decision',
            'commitment',
            'skill_hint',
            'artifact_ref',
            'failure_pattern',
            'failure_playbook',
            'repair_sequence',
            'anti_pattern'
        )
    );

UPDATE ami.skill_cards
SET skill_derivation_kind = 'extract'
WHERE skill_derivation_kind IS NULL
   OR skill_derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

ALTER TABLE ami.skill_cards
    DROP CONSTRAINT IF EXISTS skill_cards_derivation_kind_check;

ALTER TABLE ami.skill_cards
    ADD CONSTRAINT skill_cards_derivation_kind_check CHECK (
        skill_derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

UPDATE ami.skill_cards
SET skill_shared_promotion_state = CASE
    WHEN skill_scope_type = 'project_shared' AND skill_trust_state = 'verified' THEN 'approved'
    WHEN skill_scope_type = 'project_shared' THEN 'pending_approval'
    ELSE 'not_applicable'
END
WHERE skill_shared_promotion_state IS NULL
   OR skill_shared_promotion_state NOT IN (
       'not_applicable',
       'pending_approval',
       'approved'
   );

ALTER TABLE ami.skill_cards
    DROP CONSTRAINT IF EXISTS skill_cards_shared_promotion_state_check;

ALTER TABLE ami.skill_cards
    ADD CONSTRAINT skill_cards_shared_promotion_state_check CHECK (
        skill_shared_promotion_state IN (
            'not_applicable',
            'pending_approval',
            'approved'
        )
    );

CREATE TABLE IF NOT EXISTS ami.skill_evidence_bundles (
    skill_evidence_bundle_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    skill_card_id UUID NOT NULL REFERENCES ami.skill_cards(skill_card_id) ON DELETE CASCADE,
    evidence_kind TEXT NOT NULL CHECK (
        evidence_kind IN ('episode_success', 'episode_failure', 'trace', 'artifact', 'eval_support')
    ),
    summary TEXT,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'skill-evidence-bundle-envelope-v1',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.skill_trial_runs (
    skill_trial_run_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    skill_card_id UUID NOT NULL REFERENCES ami.skill_cards(skill_card_id) ON DELETE CASCADE,
    application_mode TEXT NOT NULL CHECK (
        application_mode IN ('shadow', 'trial', 'verified')
    ),
    task_label TEXT,
    runtime_name TEXT,
    model_name TEXT,
    tool_name TEXT,
    matched BOOLEAN NOT NULL DEFAULT FALSE,
    applied BOOLEAN NOT NULL DEFAULT FALSE,
    outcome TEXT NOT NULL DEFAULT 'neutral' CHECK (
        outcome IN ('neutral', 'success', 'failure')
    ),
    summary TEXT,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'skill-trial-run-envelope-v1',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.skill_evals (
    skill_eval_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    skill_card_id UUID NOT NULL REFERENCES ami.skill_cards(skill_card_id) ON DELETE CASCADE,
    verdict TEXT NOT NULL CHECK (
        verdict IN (
            'candidate_only',
            'promote_shadow',
            'promote_trial',
            'promote_verified',
            'approve_shared_promotion',
            'reject',
            'quarantine',
            'deprecate'
        )
    ),
    evaluator_source TEXT NOT NULL,
    safe_to_apply BOOLEAN NOT NULL DEFAULT FALSE,
    quality_ok BOOLEAN NOT NULL DEFAULT FALSE,
    truth_ok BOOLEAN NOT NULL DEFAULT FALSE,
    utility_delta DOUBLE PRECISION NOT NULL DEFAULT 0,
    summary TEXT,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'skill-eval-envelope-v1',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.skill_trigger_matches (
    skill_trigger_match_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    skill_card_id UUID NOT NULL REFERENCES ami.skill_cards(skill_card_id) ON DELETE CASCADE,
    match_scope TEXT NOT NULL CHECK (
        match_scope IN ('project_task', 'thread', 'restore', 'manual_review')
    ),
    trigger_input TEXT NOT NULL,
    matched BOOLEAN NOT NULL DEFAULT FALSE,
    summary TEXT,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'skill-trigger-match-envelope-v1',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.skill_reuse_logs (
    skill_reuse_log_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    skill_card_id UUID NOT NULL REFERENCES ami.skill_cards(skill_card_id) ON DELETE CASCADE,
    reuse_mode TEXT NOT NULL CHECK (
        reuse_mode IN ('shadow', 'trial', 'verified', 'manual_debug')
    ),
    task_label TEXT,
    outcome TEXT NOT NULL DEFAULT 'neutral' CHECK (
        outcome IN ('neutral', 'success', 'failure')
    ),
    summary TEXT,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'skill-reuse-log-envelope-v1',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_skill_cards_project_state
    ON ami.skill_cards(project_id, namespace_id, skill_trust_state, skill_utility_score DESC, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_skill_evidence_bundles_skill_card
    ON ami.skill_evidence_bundles(skill_card_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_skill_trial_runs_skill_card
    ON ami.skill_trial_runs(skill_card_id, application_mode, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_skill_evals_skill_card
    ON ami.skill_evals(skill_card_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_skill_trigger_matches_skill_card
    ON ami.skill_trigger_matches(skill_card_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_skill_reuse_logs_skill_card
    ON ami.skill_reuse_logs(skill_card_id, created_at DESC);

CREATE TABLE IF NOT EXISTS ami.shared_assets (
    shared_asset_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    source_project_id UUID REFERENCES ami.projects(project_id) ON DELETE SET NULL,
    transfer_policy_id UUID REFERENCES ami.transfer_policies(transfer_policy_id) ON DELETE SET NULL,
    code TEXT NOT NULL CHECK (code <> ''),
    display_name TEXT NOT NULL,
    asset_kind TEXT NOT NULL CHECK (
        asset_kind IN (
            'artifact',
            'document',
            'dependency',
            'component',
            'service',
            'dataset',
            'benchmark_evidence',
            'other'
        )
    ),
    visibility_scope TEXT NOT NULL DEFAULT 'cross_project_linked' CHECK (
        visibility_scope IN (
            'agent_private',
            'team_shared',
            'project_shared',
            'cross_project_linked',
            'org_global',
            'quarantine',
            'imported'
        )
    ),
    status TEXT NOT NULL DEFAULT 'active' CHECK (
        status IN ('active', 'archived', 'disabled', 'quarantined')
    ),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, code)
);

CREATE TABLE IF NOT EXISTS ami.shared_asset_projects (
    shared_asset_id UUID NOT NULL REFERENCES ami.shared_assets(shared_asset_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    binding_kind TEXT NOT NULL CHECK (
        binding_kind IN ('owner', 'consumer', 'dependency', 'reference')
    ),
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'shared-asset-project-binding-v1',
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (shared_asset_id, project_id)
);

ALTER TABLE ami.shared_assets
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.shared_assets
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.shared_assets
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.shared_assets
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.shared_assets
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.shared_assets
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.shared_assets
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'shared-asset-envelope-v1';

UPDATE ami.shared_assets
SET derivation_kind = 'extract'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

ALTER TABLE ami.shared_assets
    DROP CONSTRAINT IF EXISTS shared_assets_derivation_kind_check;

ALTER TABLE ami.shared_assets
    ADD CONSTRAINT shared_assets_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.shared_asset_projects
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.shared_asset_projects
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.shared_asset_projects
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.shared_asset_projects
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.shared_asset_projects
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.shared_asset_projects
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.shared_asset_projects
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'shared-asset-project-binding-v1';

UPDATE ami.shared_asset_projects
SET derivation_kind = 'extract'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

ALTER TABLE ami.shared_asset_projects
    DROP CONSTRAINT IF EXISTS shared_asset_projects_derivation_kind_check;

ALTER TABLE ami.shared_asset_projects
    ADD CONSTRAINT shared_asset_projects_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

CREATE TABLE IF NOT EXISTS ami.scope_override_events (
    scope_override_event_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    entity_kind TEXT NOT NULL CHECK (
        entity_kind IN ('project_relation', 'import_packet', 'access_policy', 'shared_asset')
    ),
    entity_id UUID NOT NULL,
    actor_agent_id UUID REFERENCES ami.agents(agent_id) ON DELETE SET NULL,
    event_kind TEXT NOT NULL CHECK (
        event_kind IN (
            'override',
            'rescope',
            'revoke',
            'quarantine',
            'approve_transfer',
            'reject_transfer'
        )
    ),
    reason TEXT NOT NULL,
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_scope_override_events_entity
    ON ami.scope_override_events(entity_kind, entity_id, created_at DESC);

CREATE TABLE IF NOT EXISTS ami.agent_sessions (
    session_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    agent_id UUID REFERENCES ami.agents(agent_id) ON DELETE SET NULL,
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE SET NULL,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE SET NULL,
    session_label TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ended_at TIMESTAMPTZ
);

CREATE TABLE IF NOT EXISTS ami.code_documents (
    document_id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID NOT NULL REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    repo_root TEXT NOT NULL,
    absolute_path TEXT NOT NULL,
    relative_path TEXT NOT NULL,
    relative_basename TEXT GENERATED ALWAYS AS (
        regexp_replace(relative_path, '^.*/', '')
    ) STORED,
    relative_basename_stem TEXT GENERATED ALWAYS AS (
        regexp_replace(
            regexp_replace(relative_path, '^.*/', ''),
            '\.[^.]+$',
            ''
        )
    ) STORED,
    language TEXT,
    source_kind TEXT NOT NULL,
    git_commit_sha TEXT,
    file_sha256 TEXT NOT NULL,
    line_count INTEGER NOT NULL,
    byte_count BIGINT NOT NULL,
    content TEXT NOT NULL,
    metrics JSONB NOT NULL DEFAULT '{}'::jsonb,
    structure JSONB NOT NULL DEFAULT '[]'::jsonb,
    imports JSONB NOT NULL DEFAULT '[]'::jsonb,
    exports JSONB NOT NULL DEFAULT '[]'::jsonb,
    diagnostics JSONB NOT NULL DEFAULT '[]'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    search_vector TSVECTOR GENERATED ALWAYS AS (
        to_tsvector(
            'simple',
            coalesce(relative_path, '') || ' ' ||
            coalesce(language, '') || ' ' ||
            coalesce(source_kind, '') || ' ' ||
            coalesce(content, '')
        )
    ) STORED,
    UNIQUE (namespace_id, relative_path)
);

ALTER TABLE ami.code_documents
    ADD COLUMN IF NOT EXISTS relative_basename TEXT GENERATED ALWAYS AS (
        regexp_replace(relative_path, '^.*/', '')
    ) STORED;

ALTER TABLE ami.code_documents
    ADD COLUMN IF NOT EXISTS relative_basename_stem TEXT GENERATED ALWAYS AS (
        regexp_replace(
            regexp_replace(relative_path, '^.*/', ''),
            '\.[^.]+$',
            ''
        )
    ) STORED;

CREATE TABLE IF NOT EXISTS ami.code_symbols (
    symbol_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    document_id UUID NOT NULL REFERENCES ami.code_documents(document_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID NOT NULL REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    search_vector TSVECTOR GENERATED ALWAYS AS (
        to_tsvector(
            'simple',
            coalesce(name, '') || ' ' ||
            coalesce(kind, '') || ' ' ||
            coalesce(metadata::text, '')
        )
    ) STORED
);

CREATE TABLE IF NOT EXISTS ami.code_chunks (
    chunk_id UUID PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES ami.code_documents(document_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID NOT NULL REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    qdrant_point_id UUID,
    qdrant_collection_alias TEXT,
    chunk_index INTEGER NOT NULL,
    total_chunks INTEGER NOT NULL,
    start_line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    content TEXT NOT NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    indexed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    search_vector TSVECTOR GENERATED ALWAYS AS (
        to_tsvector(
            'simple',
            coalesce(content, '') || ' ' ||
            coalesce(metadata::text, '')
        )
    ) STORED
);

CREATE TABLE IF NOT EXISTS ami.memory_cards (
    memory_card_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    body TEXT NOT NULL,
    tags JSONB NOT NULL DEFAULT '[]'::jsonb,
    provenance JSONB NOT NULL DEFAULT '{}'::jsonb,
    fact_subject TEXT,
    fact_predicate TEXT,
    fact_object TEXT,
    truth_state TEXT NOT NULL DEFAULT 'current',
    verification_state TEXT NOT NULL DEFAULT 'raw',
    status TEXT NOT NULL DEFAULT 'active',
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    candidate_class TEXT NOT NULL DEFAULT 'fact' CHECK (
        candidate_class IN ('fact', 'decision', 'commitment', 'skill_hint', 'artifact_ref')
    ),
    source_kind TEXT,
    hot_path_write_eligible BOOLEAN NOT NULL DEFAULT FALSE,
    background_consolidation_recommended BOOLEAN NOT NULL DEFAULT TRUE,
    observed_at_epoch_ms BIGINT,
    recorded_at_epoch_ms BIGINT,
    valid_from_epoch_ms BIGINT,
    valid_to_epoch_ms BIGINT,
    last_verified_at_epoch_ms BIGINT,
    superseded_by_memory_card_id UUID REFERENCES ami.memory_cards(memory_card_id) ON DELETE SET NULL,
    qdrant_point_id UUID,
    qdrant_collection_alias TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    search_vector TSVECTOR GENERATED ALWAYS AS (
        to_tsvector(
            'simple',
            coalesce(title, '') || ' ' ||
            coalesce(summary, '') || ' ' ||
            coalesce(body, '')
        )
    ) STORED
);

ALTER TABLE ami.memory_cards
    ADD COLUMN IF NOT EXISTS fact_subject TEXT,
    ADD COLUMN IF NOT EXISTS fact_predicate TEXT,
    ADD COLUMN IF NOT EXISTS fact_object TEXT,
    ADD COLUMN IF NOT EXISTS truth_state TEXT NOT NULL DEFAULT 'current',
    ADD COLUMN IF NOT EXISTS verification_state TEXT NOT NULL DEFAULT 'raw',
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active',
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract',
    ADD COLUMN IF NOT EXISTS candidate_class TEXT NOT NULL DEFAULT 'fact',
    ADD COLUMN IF NOT EXISTS source_kind TEXT,
    ADD COLUMN IF NOT EXISTS hot_path_write_eligible BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS background_consolidation_recommended BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS observed_at_epoch_ms BIGINT,
    ADD COLUMN IF NOT EXISTS recorded_at_epoch_ms BIGINT,
    ADD COLUMN IF NOT EXISTS valid_from_epoch_ms BIGINT,
    ADD COLUMN IF NOT EXISTS valid_to_epoch_ms BIGINT,
    ADD COLUMN IF NOT EXISTS last_verified_at_epoch_ms BIGINT,
    ADD COLUMN IF NOT EXISTS superseded_by_memory_card_id UUID REFERENCES ami.memory_cards(memory_card_id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS qdrant_point_id UUID,
    ADD COLUMN IF NOT EXISTS qdrant_collection_alias TEXT;

UPDATE ami.memory_cards
SET derivation_kind = 'extract'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

UPDATE ami.memory_cards
SET candidate_class = 'fact'
WHERE candidate_class IS NULL
   OR candidate_class NOT IN ('fact', 'decision', 'commitment', 'skill_hint', 'artifact_ref');

ALTER TABLE ami.memory_cards
    DROP CONSTRAINT IF EXISTS memory_cards_derivation_kind_check;

ALTER TABLE ami.memory_cards
    ADD CONSTRAINT memory_cards_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.memory_cards
    DROP CONSTRAINT IF EXISTS memory_cards_candidate_class_check;

ALTER TABLE ami.memory_cards
    ADD CONSTRAINT memory_cards_candidate_class_check CHECK (
        candidate_class IN ('fact', 'decision', 'commitment', 'skill_hint', 'artifact_ref')
    );

CREATE TABLE IF NOT EXISTS ami.memory_relation_edges (
    memory_relation_edge_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    source_memory_card_id UUID NOT NULL REFERENCES ami.memory_cards(memory_card_id) ON DELETE CASCADE,
    target_memory_card_id UUID NOT NULL REFERENCES ami.memory_cards(memory_card_id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL,
    relation_state TEXT NOT NULL DEFAULT 'active' CHECK (
        relation_state IN ('active', 'inactive', 'superseded', 'archived')
    ),
    evidence JSONB NOT NULL DEFAULT '{}'::jsonb,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'memory-relation-edge-envelope-v1',
    recorded_at_epoch_ms BIGINT,
    valid_from_epoch_ms BIGINT,
    valid_to_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_memory_card_id, target_memory_card_id, relation_type)
);

CREATE TABLE IF NOT EXISTS ami.memory_card_transitions (
    memory_card_transition_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    memory_card_id UUID NOT NULL REFERENCES ami.memory_cards(memory_card_id) ON DELETE CASCADE,
    from_truth_state TEXT,
    to_truth_state TEXT,
    from_verification_state TEXT,
    to_verification_state TEXT,
    from_status TEXT,
    to_status TEXT,
    transition_reason TEXT,
    transition_source TEXT,
    recorded_at_epoch_ms BIGINT,
    effective_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE SEQUENCE IF NOT EXISTS ami.memory_item_ingest_seq_seq;

CREATE TABLE IF NOT EXISTS ami.memory_items (
    memory_item_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    source_project_id UUID REFERENCES ami.projects(project_id) ON DELETE SET NULL,
    import_packet_id UUID REFERENCES ami.import_packets(import_packet_id) ON DELETE SET NULL,
    owner_agent_id UUID REFERENCES ami.agents(agent_id) ON DELETE SET NULL,
    visibility_scope TEXT NOT NULL DEFAULT 'project_shared' CHECK (
        visibility_scope IN (
            'agent_private',
            'team_shared',
            'project_shared',
            'cross_project_linked',
            'org_global',
            'quarantine',
            'imported'
        )
    ),
    item_kind TEXT NOT NULL CHECK (
        item_kind IN ('task', 'fact', 'policy', 'skill', 'artifact', 'restore_pack', 'quarantine', 'other')
    ),
    identity_key TEXT CHECK (identity_key IS NULL OR btrim(identity_key) <> ''),
    title TEXT NOT NULL,
    summary TEXT,
    body TEXT,
    sensitivity_class TEXT NOT NULL DEFAULT 'internal' CHECK (
        sensitivity_class IN ('public', 'internal', 'confidential', 'restricted', 'secret')
    ),
    truth_state TEXT NOT NULL DEFAULT 'proposed' CHECK (
        truth_state IN ('raw', 'proposed', 'current', 'verified', 'superseded', 'conflicted', 'retracted', 'archived', 'quarantined')
    ),
    trust_state TEXT NOT NULL DEFAULT 'proposed' CHECK (
        trust_state IN ('raw', 'proposed', 'verified', 'disputed', 'quarantined')
    ),
    verification_state TEXT NOT NULL DEFAULT 'unverified' CHECK (
        verification_state IN ('unverified', 'proposed', 'verified', 'rejected', 'disputed', 'deprecated', 'quarantined')
    ),
    lifecycle_state TEXT NOT NULL DEFAULT 'hot' CHECK (
        lifecycle_state IN ('hot', 'closed', 'archived', 'deprecated', 'quarantined')
    ),
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'raw_capture' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    observed_at_epoch_ms BIGINT,
    recorded_at_epoch_ms BIGINT,
    valid_from_epoch_ms BIGINT,
    valid_to_epoch_ms BIGINT,
    last_verified_at_epoch_ms BIGINT,
    ingest_seq BIGINT NOT NULL DEFAULT nextval('ami.memory_item_ingest_seq_seq'::regclass),
    object_version BIGINT NOT NULL DEFAULT 1 CHECK (object_version >= 1),
    causation_id TEXT,
    correlation_id TEXT,
    utility_score DOUBLE PRECISION NOT NULL DEFAULT 0,
    freshness_score DOUBLE PRECISION NOT NULL DEFAULT 0,
    retention_class TEXT NOT NULL DEFAULT 'standard' CHECK (
        retention_class IN ('ephemeral', 'standard', 'durable', 'archive', 'legal_hold')
    ),
    ttl_epoch_ms BIGINT,
    imported_from JSONB NOT NULL DEFAULT '{}'::jsonb,
    schema_version TEXT NOT NULL DEFAULT 'memory-envelope-v1',
    superseded_by_memory_item_id UUID REFERENCES ami.memory_items(memory_item_id) ON DELETE SET NULL,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS source_project_id UUID REFERENCES ami.projects(project_id) ON DELETE SET NULL;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS import_packet_id UUID REFERENCES ami.import_packets(import_packet_id) ON DELETE SET NULL;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS owner_agent_id UUID REFERENCES ami.agents(agent_id) ON DELETE SET NULL;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS visibility_scope TEXT;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS sensitivity_class TEXT NOT NULL DEFAULT 'internal';

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS trust_state TEXT NOT NULL DEFAULT 'proposed';

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'raw_capture';

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS ingest_seq BIGINT NOT NULL DEFAULT nextval('ami.memory_item_ingest_seq_seq'::regclass);

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS object_version BIGINT NOT NULL DEFAULT 1;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS causation_id TEXT;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS correlation_id TEXT;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS utility_score DOUBLE PRECISION NOT NULL DEFAULT 0;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS freshness_score DOUBLE PRECISION NOT NULL DEFAULT 0;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS retention_class TEXT NOT NULL DEFAULT 'standard';

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS ttl_epoch_ms BIGINT;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS access_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS last_accessed_at TIMESTAMPTZ;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS decay_policy TEXT NOT NULL DEFAULT 'standard';

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS consolidation_status TEXT NOT NULL DEFAULT 'active';

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS imported_from JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.memory_items
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'memory-envelope-v1';

UPDATE ami.memory_items mi
SET visibility_scope = p.visibility_scope
FROM ami.projects p
WHERE mi.project_id = p.project_id
  AND (mi.visibility_scope IS NULL OR btrim(mi.visibility_scope) = '');

ALTER TABLE ami.memory_items
    ALTER COLUMN visibility_scope SET DEFAULT 'project_shared';

ALTER TABLE ami.memory_items
    ALTER COLUMN visibility_scope SET NOT NULL;

ALTER TABLE ami.memory_items
    DROP CONSTRAINT IF EXISTS memory_items_sensitivity_class_check;

ALTER TABLE ami.memory_items
    ADD CONSTRAINT memory_items_sensitivity_class_check CHECK (
        sensitivity_class IN ('public', 'internal', 'confidential', 'restricted', 'secret')
    );

ALTER TABLE ami.memory_items
    DROP CONSTRAINT IF EXISTS memory_items_trust_state_check;

ALTER TABLE ami.memory_items
    ADD CONSTRAINT memory_items_trust_state_check CHECK (
        trust_state IN ('raw', 'proposed', 'verified', 'disputed', 'quarantined')
    );

ALTER TABLE ami.memory_items
    DROP CONSTRAINT IF EXISTS memory_items_derivation_kind_check;

ALTER TABLE ami.memory_items
    ADD CONSTRAINT memory_items_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.memory_items
    DROP CONSTRAINT IF EXISTS memory_items_object_version_check;

ALTER TABLE ami.memory_items
    ADD CONSTRAINT memory_items_object_version_check CHECK (object_version >= 1);

ALTER TABLE ami.memory_items
    DROP CONSTRAINT IF EXISTS memory_items_retention_class_check;

ALTER TABLE ami.memory_items
    ADD CONSTRAINT memory_items_retention_class_check CHECK (
        retention_class IN ('ephemeral', 'standard', 'durable', 'archive', 'legal_hold')
    );

ALTER TABLE ami.memory_items
    DROP CONSTRAINT IF EXISTS memory_items_decay_policy_check;

ALTER TABLE ami.memory_items
    ADD CONSTRAINT memory_items_decay_policy_check CHECK (
        decay_policy IN ('none', 'standard', 'aggressive', 'retain_forever')
    );

ALTER TABLE ami.memory_items
    DROP CONSTRAINT IF EXISTS memory_items_consolidation_status_check;

ALTER TABLE ami.memory_items
    ADD CONSTRAINT memory_items_consolidation_status_check CHECK (
        consolidation_status IN ('active', 'compacted', 'archived', 'pruned', 'pending_review')
    );

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'memory_items_visibility_scope_check'
          AND conrelid = 'ami.memory_items'::regclass
    ) THEN
        ALTER TABLE ami.memory_items
            ADD CONSTRAINT memory_items_visibility_scope_check CHECK (
                visibility_scope IN (
                    'agent_private',
                    'team_shared',
                    'project_shared',
                    'cross_project_linked',
                    'org_global',
                    'quarantine',
                    'imported'
                )
            );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_constraint
        WHERE conname = 'memory_items_cross_project_import_pair_check'
          AND conrelid = 'ami.memory_items'::regclass
    ) THEN
        ALTER TABLE ami.memory_items
            ADD CONSTRAINT memory_items_cross_project_import_pair_check CHECK (
                (source_project_id IS NULL AND import_packet_id IS NULL)
                OR (
                    source_project_id IS NOT NULL
                    AND import_packet_id IS NOT NULL
                    AND source_project_id <> project_id
                )
            );
    END IF;
END
$$;

CREATE OR REPLACE FUNCTION ami.enforce_memory_item_import_packet()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    packet_source_project_id UUID;
    packet_target_project_id UUID;
    packet_transfer_policy_id UUID;
    packet_status TEXT;
    packet_verification_state TEXT;
    packet_borrowed_status TEXT;
    packet_allow_verified_writeback BOOLEAN;
    target_project_visibility_scope TEXT;
    writeback_evidence JSONB;
BEGIN
    SELECT p.visibility_scope
    INTO target_project_visibility_scope
    FROM ami.projects p
    WHERE p.project_id = NEW.project_id;

    IF target_project_visibility_scope IS NULL THEN
        RAISE EXCEPTION 'memory item requires existing project scope for project_id %', NEW.project_id;
    END IF;

    IF NEW.derivation_kind = 'verified_write_back' THEN
        writeback_evidence := NEW.metadata -> 'writeback_evidence';
        IF NEW.verification_state <> 'verified' THEN
            RAISE EXCEPTION 'verified_write_back memory item requires verification_state=verified';
        END IF;
        IF NEW.trust_state <> 'verified' THEN
            RAISE EXCEPTION 'verified_write_back memory item requires trust_state=verified';
        END IF;
        IF NEW.truth_state NOT IN ('current', 'verified') THEN
            RAISE EXCEPTION 'verified_write_back memory item requires current/verified truth_state';
        END IF;
        IF NEW.last_verified_at_epoch_ms IS NULL THEN
            RAISE EXCEPTION 'verified_write_back memory item requires last_verified_at_epoch_ms';
        END IF;
        IF NEW.evidence_span = '{}'::jsonb THEN
            RAISE EXCEPTION 'verified_write_back memory item requires non-empty evidence_span';
        END IF;
        IF jsonb_array_length(COALESCE(NEW.source_event_ids, '[]'::jsonb)) = 0
           AND jsonb_array_length(COALESCE(NEW.artifact_refs, '[]'::jsonb)) = 0
           AND jsonb_array_length(COALESCE(NEW.message_refs, '[]'::jsonb)) = 0 THEN
            RAISE EXCEPTION 'verified_write_back memory item requires source_event_ids, artifact_refs or message_refs';
        END IF;
        IF writeback_evidence IS NULL OR jsonb_typeof(writeback_evidence) <> 'object' THEN
            RAISE EXCEPTION 'verified_write_back memory item requires metadata.writeback_evidence object';
        END IF;
        IF COALESCE(writeback_evidence ->> 'escalated', 'false') <> 'true' THEN
            RAISE EXCEPTION 'verified_write_back memory item requires escalated evidence path';
        END IF;
        IF COALESCE(writeback_evidence ->> 'verified', 'false') <> 'true' THEN
            RAISE EXCEPTION 'verified_write_back memory item requires verified raw/source confirmation';
        END IF;
        IF COALESCE(writeback_evidence ->> 'confirmed_via', '') NOT IN ('raw_evidence', 'artifact', 'log', 'temporal_slice') THEN
            RAISE EXCEPTION 'verified_write_back memory item requires raw/artifact/log/temporal_slice confirmation';
        END IF;
    END IF;

    IF NEW.derivation_kind <> 'operator_write'
       AND NEW.source_project_id IS NULL
       AND NEW.import_packet_id IS NULL
       AND jsonb_array_length(COALESCE(NEW.source_event_ids, '[]'::jsonb)) = 0
       AND jsonb_array_length(COALESCE(NEW.artifact_refs, '[]'::jsonb)) = 0
       AND jsonb_array_length(COALESCE(NEW.message_refs, '[]'::jsonb)) = 0
       AND COALESCE(NEW.evidence_span, '{}'::jsonb) = '{}'::jsonb THEN
        RAISE EXCEPTION
            'local non-operator memory item requires source_event_ids, artifact_refs, message_refs or evidence_span';
    END IF;

    IF NEW.source_project_id IS NULL AND NEW.import_packet_id IS NULL THEN
        IF NEW.visibility_scope <> target_project_visibility_scope THEN
            RAISE EXCEPTION
                'local memory item visibility_scope % must match project visibility_scope %',
                NEW.visibility_scope,
                target_project_visibility_scope;
        END IF;
        RETURN NEW;
    END IF;

    IF NEW.source_project_id IS NULL OR NEW.import_packet_id IS NULL THEN
        RAISE EXCEPTION 'cross-project memory item requires both source_project_id and import_packet_id';
    END IF;

    IF NEW.source_project_id = NEW.project_id THEN
        RAISE EXCEPTION 'cross-project memory item cannot use the same project as source and target';
    END IF;

    SELECT
        ip.source_project_id,
        ip.target_project_id,
        ip.transfer_policy_id,
        ip.status,
        ip.verification_state,
        ip.borrowed_status
    INTO
        packet_source_project_id,
        packet_target_project_id,
        packet_transfer_policy_id,
        packet_status,
        packet_verification_state,
        packet_borrowed_status
    FROM ami.import_packets ip
    WHERE ip.import_packet_id = NEW.import_packet_id;

    IF packet_source_project_id IS NULL THEN
        RAISE EXCEPTION 'cross-project memory item requires existing import_packet_id %', NEW.import_packet_id;
    END IF;

    IF packet_source_project_id <> NEW.source_project_id THEN
        RAISE EXCEPTION 'memory item source_project_id does not match import_packet source';
    END IF;

    IF packet_target_project_id <> NEW.project_id THEN
        RAISE EXCEPTION 'memory item project_id does not match import_packet target';
    END IF;

    IF NEW.derivation_kind = 'verified_write_back' THEN
        IF packet_transfer_policy_id IS NULL THEN
            RAISE EXCEPTION 'verified_write_back import requires transfer_policy_id on import_packet';
        END IF;
        SELECT tp.allow_verified_writeback
        INTO packet_allow_verified_writeback
        FROM ami.transfer_policies tp
        WHERE tp.transfer_policy_id = packet_transfer_policy_id;
        IF COALESCE(packet_allow_verified_writeback, FALSE) <> TRUE THEN
            RAISE EXCEPTION 'verified_write_back import requires allow_verified_writeback transfer policy';
        END IF;
    END IF;

    IF packet_status NOT IN ('borrowed_unverified', 'verified') THEN
        RAISE EXCEPTION 'memory item cannot reference import_packet status %', packet_status;
    END IF;

    IF packet_status = 'borrowed_unverified'
       OR packet_verification_state <> 'verified'
       OR packet_borrowed_status <> 'verified_local_copy' THEN
        IF NEW.visibility_scope <> 'imported' THEN
            RAISE EXCEPTION 'borrowed/unverified memory item must keep imported visibility_scope';
        END IF;
        IF NEW.truth_state IN ('current', 'verified') THEN
            RAISE EXCEPTION 'borrowed/unverified memory item cannot present as local truth_state %', NEW.truth_state;
        END IF;
        IF NEW.verification_state = 'verified' THEN
            RAISE EXCEPTION 'borrowed/unverified memory item cannot present as verified local truth';
        END IF;
        IF NEW.trust_state = 'verified' THEN
            RAISE EXCEPTION 'borrowed/unverified memory item cannot present as verified trust';
        END IF;
    ELSIF NEW.visibility_scope <> target_project_visibility_scope THEN
        RAISE EXCEPTION
            'verified local copy memory item visibility_scope % must match target project visibility_scope %',
            NEW.visibility_scope,
            target_project_visibility_scope;
    END IF;

    RETURN NEW;
END
$$;

CREATE OR REPLACE TRIGGER trg_ami_memory_items_enforce_import_packet
BEFORE INSERT OR UPDATE ON ami.memory_items
FOR EACH ROW
EXECUTE FUNCTION ami.enforce_memory_item_import_packet();

CREATE OR REPLACE FUNCTION ami.touch_memory_item_envelope()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    NEW.updated_at = now();
    NEW.object_version = COALESCE(OLD.object_version, 1) + 1;
    RETURN NEW;
END
$$;

DROP TRIGGER IF EXISTS trg_ami_memory_items_touch_envelope ON ami.memory_items;

CREATE TRIGGER trg_ami_memory_items_touch_envelope
BEFORE UPDATE ON ami.memory_items
FOR EACH ROW
EXECUTE FUNCTION ami.touch_memory_item_envelope();

CREATE TABLE IF NOT EXISTS ami.memory_edges (
    memory_edge_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    source_memory_item_id UUID NOT NULL REFERENCES ami.memory_items(memory_item_id) ON DELETE CASCADE,
    target_memory_item_id UUID NOT NULL REFERENCES ami.memory_items(memory_item_id) ON DELETE CASCADE,
    edge_kind TEXT NOT NULL CHECK (
        edge_kind IN (
            'depends_on',
            'child_of',
            'continues',
            'duplicates',
            'blocks',
            'conflicts_with',
            'supports',
            'supersedes',
            'related_to',
            'other'
        )
    ),
    edge_state TEXT NOT NULL DEFAULT 'active' CHECK (
        edge_state IN ('active', 'inactive', 'superseded', 'archived', 'quarantined')
    ),
    trust_state TEXT NOT NULL DEFAULT 'proposed' CHECK (
        trust_state IN ('raw', 'proposed', 'verified', 'disputed', 'quarantined')
    ),
    validity_basis TEXT NOT NULL DEFAULT 'explicit' CHECK (
        validity_basis IN ('explicit', 'classifier', 'operator', 'imported', 'derived')
    ),
    score DOUBLE PRECISION,
    evidence JSONB NOT NULL DEFAULT '{}'::jsonb,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'memory-edge-envelope-v1',
    valid_from_epoch_ms BIGINT,
    valid_to_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_memory_item_id, target_memory_item_id, edge_kind)
);

CREATE TABLE IF NOT EXISTS ami.memory_conflicts (
    memory_conflict_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    left_memory_item_id UUID REFERENCES ami.memory_items(memory_item_id) ON DELETE CASCADE,
    right_memory_item_id UUID REFERENCES ami.memory_items(memory_item_id) ON DELETE CASCADE,
    conflict_kind TEXT NOT NULL CHECK (
        conflict_kind IN ('truth', 'scope', 'timeline', 'duplicate', 'policy', 'import', 'other')
    ),
    conflict_state TEXT NOT NULL DEFAULT 'open' CHECK (
        conflict_state IN ('open', 'resolved', 'dismissed', 'archived', 'quarantined')
    ),
    severity TEXT NOT NULL DEFAULT 'medium' CHECK (
        severity IN ('low', 'medium', 'high', 'critical')
    ),
    summary TEXT NOT NULL,
    evidence JSONB NOT NULL DEFAULT '{}'::jsonb,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'memory-conflict-envelope-v1',
    resolution JSONB NOT NULL DEFAULT '{}'::jsonb,
    detected_at_epoch_ms BIGINT,
    resolved_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.observability_snapshots (
    snapshot_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    snapshot_kind TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.artifact_refs (
    artifact_ref_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    artifact_kind TEXT NOT NULL,
    bucket TEXT NOT NULL,
    object_key TEXT NOT NULL,
    content_type TEXT,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'artifact-ref-envelope-v1',
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (bucket, object_key)
);

CREATE TABLE IF NOT EXISTS ami.memory_provenance (
    memory_provenance_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    memory_item_id UUID REFERENCES ami.memory_items(memory_item_id) ON DELETE CASCADE,
    source_kind TEXT NOT NULL,
    source_event_id TEXT,
    source_snapshot_id UUID REFERENCES ami.observability_snapshots(snapshot_id) ON DELETE RESTRICT,
    artifact_ref_id UUID REFERENCES ami.artifact_refs(artifact_ref_id) ON DELETE SET NULL,
    trust_level TEXT NOT NULL DEFAULT 'raw' CHECK (
        trust_level IN ('raw', 'extracted', 'proposed', 'verified', 'disputed', 'quarantined')
    ),
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'raw_capture' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    observed_at_epoch_ms BIGINT,
    recorded_at_epoch_ms BIGINT,
    valid_from_epoch_ms BIGINT,
    valid_to_epoch_ms BIGINT,
    schema_version TEXT NOT NULL DEFAULT 'memory-provenance-v1',
    details JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE SEQUENCE IF NOT EXISTS ami.memory_raw_event_server_order_seq_seq;

CREATE TABLE IF NOT EXISTS ami.memory_raw_events (
    memory_raw_event_id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    source_project_id UUID REFERENCES ami.projects(project_id) ON DELETE SET NULL,
    import_packet_id UUID REFERENCES ami.import_packets(import_packet_id) ON DELETE SET NULL,
    owner_agent_id UUID REFERENCES ami.agents(agent_id) ON DELETE SET NULL,
    event_kind TEXT NOT NULL DEFAULT 'memory_candidate_write' CHECK (
        event_kind IN ('memory_candidate_write', 'memory_candidate_import', 'memory_candidate_write_back')
    ),
    item_kind TEXT NOT NULL,
    visibility_scope TEXT NOT NULL,
    sensitivity_class TEXT NOT NULL,
    derivation_kind TEXT NOT NULL,
    truth_state TEXT NOT NULL,
    trust_state TEXT NOT NULL,
    verification_state TEXT NOT NULL,
    lifecycle_state TEXT NOT NULL,
    identity_key TEXT,
    title TEXT NOT NULL,
    summary TEXT,
    body TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    causation_id TEXT,
    correlation_id TEXT,
    source_epoch_ns BIGINT,
    source_monotonic_ns BIGINT,
    server_received_at_epoch_ms BIGINT NOT NULL,
    server_order_seq BIGINT NOT NULL DEFAULT nextval('ami.memory_raw_event_server_order_seq_seq'::regclass),
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.memory_write_outbox (
    memory_write_outbox_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    memory_raw_event_id UUID NOT NULL REFERENCES ami.memory_raw_events(memory_raw_event_id) ON DELETE CASCADE,
    memory_item_id UUID NOT NULL REFERENCES ami.memory_items(memory_item_id) ON DELETE CASCADE,
    subject TEXT NOT NULL,
    delivery_kind TEXT NOT NULL CHECK (
        delivery_kind IN (
            'index_lexical',
            'index_graph',
            'index_embedding',
            'index_restore_summary',
            'cache_invalidation',
            'fanout_created'
        )
    ),
    delivery_state TEXT NOT NULL DEFAULT 'pending' CHECK (
        delivery_state IN ('pending', 'published', 'acked', 'failed', 'cancelled')
    ),
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    published_at_epoch_ms BIGINT,
    acknowledged_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.memory_write_outbox
    ADD COLUMN IF NOT EXISTS last_error TEXT;

CREATE OR REPLACE FUNCTION ami.reject_memory_raw_event_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'ami.memory_raw_events is append-only';
END
$$;

DROP TRIGGER IF EXISTS trg_ami_memory_raw_events_reject_update ON ami.memory_raw_events;
CREATE TRIGGER trg_ami_memory_raw_events_reject_update
BEFORE UPDATE ON ami.memory_raw_events
FOR EACH ROW
EXECUTE FUNCTION ami.reject_memory_raw_event_mutation();

DROP TRIGGER IF EXISTS trg_ami_memory_raw_events_reject_delete ON ami.memory_raw_events;
CREATE TRIGGER trg_ami_memory_raw_events_reject_delete
BEFORE DELETE ON ami.memory_raw_events
FOR EACH ROW
EXECUTE FUNCTION ami.reject_memory_raw_event_mutation();

CREATE OR REPLACE FUNCTION ami.reject_task_event_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'ami.task_events is append-only';
END
$$;

CREATE TABLE IF NOT EXISTS ami.task_nodes (
    task_node_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    parent_task_node_id UUID REFERENCES ami.task_nodes(task_node_id) ON DELETE SET NULL,
    memory_item_id UUID REFERENCES ami.memory_items(memory_item_id) ON DELETE SET NULL,
    task_key TEXT CHECK (task_key IS NULL OR btrim(task_key) <> ''),
    task_role TEXT NOT NULL DEFAULT 'workline' CHECK (
        task_role IN ('root', 'workline', 'child', 'pending_return', 'proposal', 'historical')
    ),
    headline TEXT NOT NULL,
    summary TEXT,
    next_step TEXT,
    execution_state TEXT NOT NULL DEFAULT 'proposed' CHECK (
        execution_state IN ('proposed', 'ready', 'active', 'blocked', 'waiting_external', 'in_review', 'done', 'failed', 'canceled', 'superseded')
    ),
    lifecycle_state TEXT NOT NULL DEFAULT 'hot' CHECK (
        lifecycle_state IN ('hot', 'closed', 'archived', 'deprecated', 'quarantined')
    ),
    confidence DOUBLE PRECISION,
    current_score DOUBLE PRECISION,
    reopened_count INTEGER NOT NULL DEFAULT 0,
    child_count INTEGER NOT NULL DEFAULT 0,
    closed_child_count INTEGER NOT NULL DEFAULT 0,
    pending_return_count INTEGER NOT NULL DEFAULT 0,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    candidate_class TEXT NOT NULL DEFAULT 'commitment' CHECK (
        candidate_class IN ('fact', 'decision', 'commitment', 'skill_hint', 'artifact_ref')
    ),
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    source_kind TEXT,
    hot_path_write_eligible BOOLEAN NOT NULL DEFAULT TRUE,
    background_consolidation_recommended BOOLEAN NOT NULL DEFAULT FALSE,
    status_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    opened_at_epoch_ms BIGINT,
    closed_at_epoch_ms BIGINT,
    archived_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.task_events (
    task_event_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    task_node_id UUID NOT NULL REFERENCES ami.task_nodes(task_node_id) ON DELETE CASCADE,
    source_snapshot_id UUID REFERENCES ami.observability_snapshots(snapshot_id) ON DELETE RESTRICT,
    source_event_id TEXT,
    event_kind TEXT NOT NULL CHECK (
        event_kind IN ('created', 'continued', 'branched_child', 'branched_new', 'resumed', 'pending_return', 'closed', 'archived', 'reopened', 'superseded', 'state_change', 'evidence_request')
    ),
    prior_execution_state TEXT,
    next_execution_state TEXT,
    prior_lifecycle_state TEXT,
    next_lifecycle_state TEXT,
    source_kind TEXT,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'raw_capture' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'task-event-envelope-v1',
    event_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    recorded_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

DROP TRIGGER IF EXISTS trg_ami_task_events_reject_update ON ami.task_events;
CREATE TRIGGER trg_ami_task_events_reject_update
BEFORE UPDATE ON ami.task_events
FOR EACH ROW
EXECUTE FUNCTION ami.reject_task_event_mutation();

DROP TRIGGER IF EXISTS trg_ami_task_events_reject_delete ON ami.task_events;
CREATE TRIGGER trg_ami_task_events_reject_delete
BEFORE DELETE ON ami.task_events
FOR EACH ROW
EXECUTE FUNCTION ami.reject_task_event_mutation();

ALTER TABLE ami.memory_provenance
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.memory_provenance
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.memory_provenance
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'raw_capture';

ALTER TABLE ami.memory_provenance
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'memory-provenance-v1';

UPDATE ami.memory_provenance
SET derivation_kind = 'raw_capture'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

ALTER TABLE ami.memory_provenance
    DROP CONSTRAINT IF EXISTS memory_provenance_derivation_kind_check;

ALTER TABLE ami.memory_provenance
    ADD CONSTRAINT memory_provenance_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.memory_edges
    ADD COLUMN IF NOT EXISTS source_kind TEXT,
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract',
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'memory-edge-envelope-v1';

UPDATE ami.memory_edges
SET derivation_kind = 'extract'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

ALTER TABLE ami.memory_edges
    DROP CONSTRAINT IF EXISTS memory_edges_derivation_kind_check;

ALTER TABLE ami.memory_edges
    ADD CONSTRAINT memory_edges_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.memory_conflicts
    ADD COLUMN IF NOT EXISTS source_kind TEXT,
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract',
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'memory-conflict-envelope-v1';

UPDATE ami.memory_conflicts
SET derivation_kind = 'extract'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

ALTER TABLE ami.memory_conflicts
    DROP CONSTRAINT IF EXISTS memory_conflicts_derivation_kind_check;

ALTER TABLE ami.memory_conflicts
    ADD CONSTRAINT memory_conflicts_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

CREATE TABLE IF NOT EXISTS ami.context_packs (
    context_pack_id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID NOT NULL REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    retrieval_mode TEXT NOT NULL CHECK (
        retrieval_mode IN ('local_strict', 'local_plus_related', 'explicit_foreign', 'audit_global')
    ),
    query_text TEXT NOT NULL,
    visible_projects JSONB NOT NULL DEFAULT '[]'::jsonb,
    payload JSONB NOT NULL,
    artifact_ref_id UUID REFERENCES ami.artifact_refs(artifact_ref_id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.retrieval_traces (
    retrieval_trace_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    context_pack_id UUID REFERENCES ami.context_packs(context_pack_id) ON DELETE SET NULL,
    query_text TEXT NOT NULL,
    requested_mode TEXT,
    effective_mode TEXT,
    scope_filter JSONB NOT NULL DEFAULT '{}'::jsonb,
    candidate_summary JSONB NOT NULL DEFAULT '{}'::jsonb,
    rerank_summary JSONB NOT NULL DEFAULT '{}'::jsonb,
    evidence_sufficiency JSONB NOT NULL DEFAULT '{}'::jsonb,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'retrieval-trace-envelope-v1',
    final_decision TEXT NOT NULL DEFAULT 'abstain' CHECK (
        final_decision IN ('continue', 'child', 'new', 'abstain', 'escalate')
    ),
    temporal_query_epoch_ms BIGINT,
    trace_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.retrieval_traces
    ADD COLUMN IF NOT EXISTS source_kind TEXT,
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract',
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'retrieval-trace-envelope-v1';

UPDATE ami.retrieval_traces
SET derivation_kind = 'extract'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

ALTER TABLE ami.retrieval_traces
    DROP CONSTRAINT IF EXISTS retrieval_traces_derivation_kind_check;

ALTER TABLE ami.retrieval_traces
    ADD CONSTRAINT retrieval_traces_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

CREATE TABLE IF NOT EXISTS ami.restore_packs (
    restore_pack_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    agent_scope TEXT,
    session_id TEXT,
    thread_id TEXT,
    source_snapshot_id UUID REFERENCES ami.observability_snapshots(snapshot_id) ON DELETE RESTRICT,
    pack_kind TEXT NOT NULL CHECK (
        pack_kind IN ('startup', 'restore', 'workspace_restore_pack', 'handoff', 'manual')
    ),
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'summary' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'restore-pack-envelope-v1',
    headline TEXT,
    summary TEXT,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    captured_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT restore_packs_workspace_restore_pack_requires_source_snapshot_check CHECK (
        pack_kind <> 'workspace_restore_pack' OR source_snapshot_id IS NOT NULL
    )
);

ALTER TABLE ami.restore_packs
    ADD COLUMN IF NOT EXISTS source_kind TEXT,
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'summary',
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'restore-pack-envelope-v1';

UPDATE ami.restore_packs
SET derivation_kind = 'summary'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

ALTER TABLE ami.restore_packs
    DROP CONSTRAINT IF EXISTS restore_packs_derivation_kind_check;

ALTER TABLE ami.restore_packs
    ADD CONSTRAINT restore_packs_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

DELETE FROM ami.restore_packs
WHERE pack_kind = 'workspace_restore_pack'
  AND source_snapshot_id IS NULL;

ALTER TABLE ami.restore_packs
    DROP CONSTRAINT IF EXISTS restore_packs_source_snapshot_id_fkey;

ALTER TABLE ami.restore_packs
    ADD CONSTRAINT restore_packs_source_snapshot_id_fkey
    FOREIGN KEY (source_snapshot_id)
    REFERENCES ami.observability_snapshots(snapshot_id)
    ON DELETE RESTRICT;

ALTER TABLE ami.restore_packs
    DROP CONSTRAINT IF EXISTS restore_packs_workspace_restore_pack_requires_source_snapshot_check;

ALTER TABLE ami.restore_packs
    ADD CONSTRAINT restore_packs_workspace_restore_pack_requires_source_snapshot_check CHECK (
        pack_kind <> 'workspace_restore_pack' OR source_snapshot_id IS NOT NULL
    );

CREATE TABLE IF NOT EXISTS ami.policy_rules (
    policy_rule_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    rule_code TEXT NOT NULL CHECK (btrim(rule_code) <> ''),
    rule_scope TEXT NOT NULL CHECK (
        rule_scope IN ('workspace', 'project', 'namespace', 'agent', 'shared')
    ),
    rule_kind TEXT NOT NULL CHECK (
        rule_kind IN ('scope_filter', 'link_decision', 'import', 'restore', 'retrieval', 'quarantine', 'other')
    ),
    rule_status TEXT NOT NULL DEFAULT 'active' CHECK (
        rule_status IN ('active', 'disabled', 'archived', 'quarantined')
    ),
    precedence INTEGER NOT NULL DEFAULT 100,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'operator_write' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'policy-rule-envelope-v1',
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.policy_rules
    ADD COLUMN IF NOT EXISTS source_kind TEXT,
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'operator_write',
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'policy-rule-envelope-v1';

UPDATE ami.policy_rules
SET derivation_kind = 'operator_write'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

ALTER TABLE ami.policy_rules
    DROP CONSTRAINT IF EXISTS policy_rules_derivation_kind_check;

ALTER TABLE ami.policy_rules
    ADD CONSTRAINT policy_rules_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

CREATE TABLE IF NOT EXISTS ami.quarantine_items (
    quarantine_item_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    entity_kind TEXT NOT NULL CHECK (
        entity_kind IN (
            'memory_item',
            'memory_edge',
            'memory_conflict',
            'import_packet',
            'policy_rule',
            'project_relation',
            'skill_card',
            'artifact_ref',
            'other'
        )
    ),
    entity_id TEXT NOT NULL,
    quarantine_reason TEXT NOT NULL,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'quarantine-item-envelope-v1',
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.quarantine_items
    ADD COLUMN IF NOT EXISTS source_kind TEXT,
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract',
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'quarantine-item-envelope-v1';

UPDATE ami.quarantine_items
SET derivation_kind = 'extract'
WHERE derivation_kind IS NULL
   OR derivation_kind NOT IN (
       'raw_capture',
       'extract',
       'summary',
       'merge',
       'import',
       'verified_write_back',
       'operator_write'
   );

ALTER TABLE ami.quarantine_items
    DROP CONSTRAINT IF EXISTS quarantine_items_derivation_kind_check;

ALTER TABLE ami.quarantine_items
    ADD CONSTRAINT quarantine_items_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.quarantine_items
    DROP CONSTRAINT IF EXISTS quarantine_items_entity_kind_check;

ALTER TABLE ami.quarantine_items
    ADD CONSTRAINT quarantine_items_entity_kind_check CHECK (
        entity_kind IN (
            'memory_item',
            'memory_edge',
            'memory_conflict',
            'import_packet',
            'policy_rule',
            'project_relation',
            'skill_card',
            'artifact_ref',
            'other'
        )
    );

CREATE OR REPLACE VIEW ami.memory_envelopes AS
SELECT
    mi.memory_item_id AS memory_id,
    mi.item_kind AS memory_type,
    mi.visibility_scope AS scope_type,
    mi.workspace_id,
    mi.project_id,
    mi.owner_agent_id,
    mi.visibility_scope AS visibility,
    mi.sensitivity_class,
    mi.truth_state,
    mi.trust_state,
    mi.verification_state,
    mi.created_at,
    mi.source_event_ids,
    mi.artifact_refs,
    mi.message_refs,
    mi.evidence_span,
    mi.derivation_kind,
    mi.observed_at_epoch_ms,
    mi.recorded_at_epoch_ms,
    mi.valid_from_epoch_ms,
    mi.valid_to_epoch_ms,
    mi.last_verified_at_epoch_ms,
    supersedes.supersedes,
    conflicts.conflicts_with,
    mi.utility_score,
    mi.freshness_score,
    mi.retention_class,
    mi.ttl_epoch_ms AS ttl,
    mi.imported_from,
    mi.schema_version,
    mi.ingest_seq,
    mi.object_version,
    mi.causation_id,
    mi.correlation_id,
    mi.lifecycle_state,
    mi.identity_key,
    mi.title,
    mi.summary,
    mi.body,
    mi.metadata,
    mi.updated_at,
    COALESCE(
        mi.metadata -> 'stage2_runtime' ->> 'candidate_class',
        CASE
            WHEN mi.item_kind = 'decision' THEN 'decision'
            WHEN mi.item_kind IN ('task', 'commitment') THEN 'commitment'
            WHEN mi.item_kind IN ('skill', 'skill_hint') THEN 'skill_hint'
            WHEN mi.item_kind IN ('artifact', 'artifact_ref') THEN 'artifact_ref'
            ELSE 'fact'
        END
    ) AS candidate_class,
    mi.metadata -> 'stage2_runtime' ->> 'source_kind' AS source_kind,
    COALESCE(
        CASE
            WHEN jsonb_typeof(mi.metadata -> 'stage2_runtime' -> 'hot_path_write_eligible') = 'boolean'
                THEN (mi.metadata -> 'stage2_runtime' ->> 'hot_path_write_eligible')::boolean
            ELSE NULL
        END,
        CASE
            WHEN mi.derivation_kind = 'operator_write' OR mi.item_kind IN ('decision', 'task', 'commitment')
                THEN TRUE
            ELSE FALSE
        END
    ) AS hot_path_write_eligible,
    COALESCE(
        CASE
            WHEN jsonb_typeof(mi.metadata -> 'stage2_runtime' -> 'background_consolidation_recommended') = 'boolean'
                THEN (mi.metadata -> 'stage2_runtime' ->> 'background_consolidation_recommended')::boolean
            ELSE NULL
        END,
        CASE
            WHEN mi.derivation_kind = 'operator_write' OR mi.item_kind IN ('decision', 'task', 'commitment')
                THEN FALSE
            WHEN mi.item_kind IN ('skill', 'skill_hint', 'artifact', 'artifact_ref')
                THEN TRUE
            ELSE TRUE
        END
    ) AS background_consolidation_recommended
FROM ami.memory_items mi
LEFT JOIN LATERAL (
    SELECT COALESCE(jsonb_agg(prev.memory_item_id ORDER BY prev.created_at), '[]'::jsonb) AS supersedes
    FROM ami.memory_items prev
    WHERE prev.superseded_by_memory_item_id = mi.memory_item_id
) supersedes ON TRUE
LEFT JOIN LATERAL (
    SELECT COALESCE(jsonb_agg(DISTINCT related_id), '[]'::jsonb) AS conflicts_with
    FROM (
        SELECT me.target_memory_item_id AS related_id
        FROM ami.memory_edges me
        WHERE me.source_memory_item_id = mi.memory_item_id
          AND me.edge_kind = 'conflicts_with'
        UNION
        SELECT me.source_memory_item_id AS related_id
        FROM ami.memory_edges me
        WHERE me.target_memory_item_id = mi.memory_item_id
          AND me.edge_kind = 'conflicts_with'
        UNION
        SELECT mc.right_memory_item_id AS related_id
        FROM ami.memory_conflicts mc
        WHERE mc.left_memory_item_id = mi.memory_item_id
          AND mc.right_memory_item_id IS NOT NULL
        UNION
        SELECT mc.left_memory_item_id AS related_id
        FROM ami.memory_conflicts mc
        WHERE mc.right_memory_item_id = mi.memory_item_id
          AND mc.left_memory_item_id IS NOT NULL
    ) related
) conflicts ON TRUE;

CREATE TABLE IF NOT EXISTS ami.stack_meta (
    meta_key TEXT PRIMARY KEY,
    meta_value JSONB NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.context_packs (
    context_pack_id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID NOT NULL REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    retrieval_mode TEXT NOT NULL CHECK (
        retrieval_mode IN ('local_strict', 'local_plus_related', 'explicit_foreign', 'audit_global')
    ),
    query_text TEXT NOT NULL,
    visible_projects JSONB NOT NULL DEFAULT '[]'::jsonb,
    payload JSONB NOT NULL,
    artifact_ref_id UUID REFERENCES ami.artifact_refs(artifact_ref_id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.retrieval_traces (
    retrieval_trace_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    context_pack_id UUID REFERENCES ami.context_packs(context_pack_id) ON DELETE SET NULL,
    query_text TEXT NOT NULL,
    requested_mode TEXT,
    effective_mode TEXT,
    scope_filter JSONB NOT NULL DEFAULT '{}'::jsonb,
    candidate_summary JSONB NOT NULL DEFAULT '{}'::jsonb,
    rerank_summary JSONB NOT NULL DEFAULT '{}'::jsonb,
    evidence_sufficiency JSONB NOT NULL DEFAULT '{}'::jsonb,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'retrieval-trace-envelope-v1',
    final_decision TEXT NOT NULL DEFAULT 'abstain' CHECK (
        final_decision IN ('continue', 'child', 'new', 'abstain', 'escalate')
    ),
    temporal_query_epoch_ms BIGINT,
    trace_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.restore_packs (
    restore_pack_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    agent_scope TEXT,
    session_id TEXT,
    thread_id TEXT,
    source_snapshot_id UUID REFERENCES ami.observability_snapshots(snapshot_id) ON DELETE RESTRICT,
    pack_kind TEXT NOT NULL CHECK (
        pack_kind IN ('startup', 'restore', 'workspace_restore_pack', 'handoff', 'manual')
    ),
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'summary' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'restore-pack-envelope-v1',
    headline TEXT,
    summary TEXT,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    captured_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT restore_packs_workspace_restore_pack_requires_source_snapshot_check CHECK (
        pack_kind <> 'workspace_restore_pack' OR source_snapshot_id IS NOT NULL
    )
);

ALTER TABLE ami.context_packs
    ADD COLUMN IF NOT EXISTS artifact_bucket TEXT,
    ADD COLUMN IF NOT EXISTS artifact_object_key TEXT,
    ADD COLUMN IF NOT EXISTS artifact_state TEXT NOT NULL DEFAULT 'materialized',
    ADD COLUMN IF NOT EXISTS artifact_last_error TEXT,
    ADD COLUMN IF NOT EXISTS artifact_updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

ALTER TABLE ami.context_packs
    DROP CONSTRAINT IF EXISTS context_packs_artifact_state_check;

ALTER TABLE ami.context_packs
    ADD CONSTRAINT context_packs_artifact_state_check CHECK (
        artifact_state IN ('pending', 'materializing', 'materialized', 'failed')
    );

ALTER TABLE ami.memory_cards
    DROP CONSTRAINT IF EXISTS memory_cards_truth_state_check;

ALTER TABLE ami.memory_cards
    ADD CONSTRAINT memory_cards_truth_state_check CHECK (
        truth_state IN ('current', 'superseded', 'conflicted', 'retracted', 'unverified')
    );

ALTER TABLE ami.memory_cards
    DROP CONSTRAINT IF EXISTS memory_cards_verification_state_check;

ALTER TABLE ami.memory_cards
    ADD CONSTRAINT memory_cards_verification_state_check CHECK (
        verification_state IN ('raw', 'proposed', 'verified', 'disputed', 'deprecated', 'quarantined')
    );

ALTER TABLE ami.memory_cards
    DROP CONSTRAINT IF EXISTS memory_cards_status_check;

ALTER TABLE ami.memory_cards
    ADD CONSTRAINT memory_cards_status_check CHECK (
        status IN ('active', 'inactive', 'superseded', 'archived')
    );

UPDATE ami.context_packs cp
SET artifact_bucket = COALESCE(cp.artifact_bucket, ar.bucket),
    artifact_object_key = COALESCE(cp.artifact_object_key, ar.object_key),
    artifact_state = CASE
        WHEN cp.artifact_ref_id IS NULL THEN 'pending'
        ELSE 'materialized'
    END,
    artifact_updated_at = now()
FROM ami.artifact_refs ar
WHERE cp.artifact_ref_id = ar.artifact_ref_id
  AND (
      cp.artifact_bucket IS NULL
      OR cp.artifact_object_key IS NULL
      OR cp.artifact_state IS DISTINCT FROM 'materialized'
  );

CREATE TABLE IF NOT EXISTS ami.policy_rules (
    policy_rule_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    rule_code TEXT NOT NULL CHECK (btrim(rule_code) <> ''),
    rule_scope TEXT NOT NULL CHECK (
        rule_scope IN ('workspace', 'project', 'namespace', 'agent', 'shared')
    ),
    rule_kind TEXT NOT NULL CHECK (
        rule_kind IN ('scope_filter', 'link_decision', 'import', 'restore', 'retrieval', 'quarantine', 'other')
    ),
    rule_status TEXT NOT NULL DEFAULT 'active' CHECK (
        rule_status IN ('active', 'disabled', 'archived', 'quarantined')
    ),
    precedence INTEGER NOT NULL DEFAULT 100,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'operator_write' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'policy-rule-envelope-v1',
    rule_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, rule_code)
);

CREATE TABLE IF NOT EXISTS ami.quarantine_items (
    quarantine_item_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    entity_kind TEXT NOT NULL CHECK (
        entity_kind IN ('memory_item', 'memory_edge', 'memory_conflict', 'import_packet', 'policy_rule', 'project_relation', 'skill_card', 'artifact_ref', 'other')
    ),
    entity_id UUID,
    quarantine_reason TEXT NOT NULL,
    quarantine_state TEXT NOT NULL DEFAULT 'active' CHECK (
        quarantine_state IN ('active', 'released', 'rejected', 'archived')
    ),
    evidence JSONB NOT NULL DEFAULT '{}'::jsonb,
    source_kind TEXT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'quarantine-item-envelope-v1',
    quarantined_at_epoch_ms BIGINT,
    released_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.observability_snapshots
    ADD COLUMN IF NOT EXISTS event_key TEXT,
    ADD COLUMN IF NOT EXISTS source_event_id TEXT,
    ADD COLUMN IF NOT EXISTS source_kind TEXT,
    ADD COLUMN IF NOT EXISTS source_class TEXT,
    ADD COLUMN IF NOT EXISTS scope_project_code TEXT,
    ADD COLUMN IF NOT EXISTS scope_namespace_code TEXT,
    ADD COLUMN IF NOT EXISTS captured_at_epoch_ms BIGINT,
    ADD COLUMN IF NOT EXISTS payload_sha256 TEXT,
    ADD COLUMN IF NOT EXISTS replay_count BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now();

UPDATE ami.observability_snapshots
SET event_key = CONCAT('legacy:', snapshot_id::text)
WHERE event_key IS NULL;

UPDATE ami.observability_snapshots
SET payload_sha256 = encode(digest(payload::text, 'sha256'), 'hex')
WHERE payload_sha256 IS NULL OR payload_sha256 = '';

UPDATE ami.observability_snapshots
SET source_kind = snapshot_kind
WHERE source_kind IS NULL;

UPDATE ami.observability_snapshots
SET captured_at_epoch_ms = COALESCE(
    NULLIF(payload #>> '{_observability,captured_at_epoch_ms}', '')::bigint,
    NULLIF(payload #>> '{captured_at_epoch_ms}', '')::bigint,
    NULLIF(payload #>> '{working_state_event,recorded_at_epoch_ms}', '')::bigint,
    NULLIF(payload #>> '{token_budget_event,created_at_epoch_ms}', '')::bigint,
    NULLIF(payload #>> '{continuity_import,imported_at_epoch_ms}', '')::bigint,
    NULLIF(payload #>> '{continuity_thread_index,captured_at_epoch_ms}', '')::bigint,
    NULLIF(payload #>> '{continuity_handoff,captured_at_epoch_ms}', '')::bigint,
    NULLIF(payload #>> '{benchmark,captured_at_epoch_ms}', '')::bigint,
    NULLIF(payload #>> '{accuracy_verification,captured_at_epoch_ms}', '')::bigint,
    NULLIF(payload #>> '{load_verification,captured_at_epoch_ms}', '')::bigint,
    NULLIF(payload #>> '{cold_benchmark,captured_at_epoch_ms}', '')::bigint,
    (EXTRACT(EPOCH FROM created_at) * 1000)::bigint
)
WHERE captured_at_epoch_ms IS NULL;

UPDATE ami.observability_snapshots AS snapshots
SET
    source_event_id = snapshots.payload #>> '{working_state_event,event_id}',
    event_key = snapshots.payload #>> '{working_state_event,event_id}'
WHERE snapshots.snapshot_kind = 'working_state_event'
AND COALESCE(snapshots.payload #>> '{working_state_event,event_id}', '') <> ''
AND (
    snapshots.source_event_id IS DISTINCT FROM snapshots.payload #>> '{working_state_event,event_id}'
    OR snapshots.event_key IS DISTINCT FROM snapshots.payload #>> '{working_state_event,event_id}'
)
AND NOT EXISTS (
    SELECT 1
    FROM ami.observability_snapshots AS other
    WHERE other.snapshot_kind = snapshots.snapshot_kind
      AND other.snapshot_id <> snapshots.snapshot_id
      AND other.event_key = snapshots.payload #>> '{working_state_event,event_id}'
);

UPDATE ami.observability_snapshots
SET source_class = CASE
    WHEN payload->'load_verification'->>'record_live_context' = 'true'
        OR payload->'load_verification'->>'publish_benchmark_snapshot' = 'false'
    THEN 'live_context'
    WHEN snapshot_kind IN (
        'retrieval_benchmark_hot',
        'retrieval_benchmark_cold',
        'retrieval_load_hot',
        'retrieval_load_cold',
        'retrieval_accuracy',
        'cold_path_benchmark',
        'token_benchmark',
        'token_benchmark_suite',
        'text_compare',
        'mcp_task_matrix',
        'memory_task_matrix'
    ) THEN 'benchmark'
    WHEN snapshot_kind = 'system_snapshot' THEN 'live_system'
    ELSE 'operational'
END
WHERE source_class IS NULL;

UPDATE ami.observability_snapshots
SET last_seen_at = created_at
WHERE last_seen_at IS NULL;

ALTER TABLE ami.observability_snapshots
    ALTER COLUMN event_key SET NOT NULL,
    ALTER COLUMN payload_sha256 SET NOT NULL;

CREATE OR REPLACE FUNCTION ami.observability_snapshot_source_class(
    in_snapshot_kind TEXT,
    in_payload JSONB
) RETURNS TEXT
LANGUAGE SQL
IMMUTABLE
AS $$
    SELECT CASE
        WHEN in_payload->'load_verification'->>'record_live_context' = 'true'
            OR in_payload->'load_verification'->>'publish_benchmark_snapshot' = 'false'
        THEN 'live_context'
        WHEN in_snapshot_kind IN (
            'retrieval_benchmark_hot',
            'retrieval_benchmark_cold',
            'retrieval_load_hot',
            'retrieval_load_cold',
            'retrieval_accuracy',
            'cold_path_benchmark',
            'token_benchmark',
            'token_benchmark_suite',
            'text_compare',
            'mcp_task_matrix',
            'memory_task_matrix'
        ) THEN 'benchmark'
        WHEN in_snapshot_kind = 'system_snapshot' THEN 'live_system'
        ELSE 'operational'
    END;
$$;

CREATE OR REPLACE FUNCTION ami.fill_observability_snapshot_defaults()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    computed_payload_sha256 TEXT;
    captured_text TEXT;
BEGIN
    computed_payload_sha256 := COALESCE(
        NULLIF(NEW.payload_sha256, ''),
        encode(digest(NEW.payload::text, 'sha256'), 'hex')
    );

    NEW.source_event_id := COALESCE(
        NULLIF(NEW.source_event_id, ''),
        NULLIF(NEW.payload #>> '{_observability,source_event_id}', ''),
        NULLIF(NEW.payload #>> '{token_budget_event,event_id}', ''),
        NULLIF(NEW.payload #>> '{working_state_event,event_id}', ''),
        NULLIF(NEW.payload #>> '{working_state_event,context_pack_id}', ''),
        NULLIF(NEW.payload #>> '{context_pack_id}', '')
    );
    NEW.source_kind := COALESCE(
        NULLIF(NEW.source_kind, ''),
        NULLIF(NEW.payload #>> '{_observability,source_kind}', ''),
        NULLIF(NEW.payload #>> '{token_budget_event,source_kind}', ''),
        NULLIF(NEW.payload #>> '{working_state_event,source_kind}', ''),
        NULLIF(NEW.payload #>> '{continuity_handoff,source_kind}', ''),
        NEW.snapshot_kind
    );
    NEW.source_class := COALESCE(
        NULLIF(NEW.source_class, ''),
        NULLIF(NEW.payload #>> '{_observability,source_class}', ''),
        ami.observability_snapshot_source_class(NEW.snapshot_kind, NEW.payload)
    );
    NEW.scope_project_code := COALESCE(
        NULLIF(NEW.scope_project_code, ''),
        NULLIF(NEW.payload #>> '{_observability,scope_project_code}', ''),
        NULLIF(NEW.payload #>> '{project,code}', ''),
        NULLIF(NEW.payload #>> '{working_state_event,project,code}', ''),
        NULLIF(NEW.payload #>> '{continuity_import,project,code}', ''),
        NULLIF(NEW.payload #>> '{continuity_handoff,project,code}', ''),
        NULLIF(NEW.payload #>> '{token_budget_event,project_code}', ''),
        NULLIF(NEW.payload #>> '{token_budget_event,project}', ''),
        NULLIF(NEW.payload #>> '{benchmark,project}', ''),
        NULLIF(NEW.payload #>> '{accuracy_verification,project}', ''),
        NULLIF(NEW.payload #>> '{load_verification,project}', ''),
        NULLIF(NEW.payload #>> '{cold_benchmark,project}', '')
    );
    NEW.scope_namespace_code := COALESCE(
        NULLIF(NEW.scope_namespace_code, ''),
        NULLIF(NEW.payload #>> '{_observability,scope_namespace_code}', ''),
        NULLIF(NEW.payload #>> '{namespace,code}', ''),
        NULLIF(NEW.payload #>> '{working_state_event,namespace,code}', ''),
        NULLIF(NEW.payload #>> '{continuity_import,namespace,code}', ''),
        NULLIF(NEW.payload #>> '{continuity_handoff,namespace,code}', ''),
        NULLIF(NEW.payload #>> '{token_budget_event,namespace_code}', ''),
        NULLIF(NEW.payload #>> '{token_budget_event,namespace}', ''),
        NULLIF(NEW.payload #>> '{benchmark,namespace}', ''),
        NULLIF(NEW.payload #>> '{accuracy_verification,namespace}', ''),
        NULLIF(NEW.payload #>> '{load_verification,namespace}', '')
    );

    captured_text := COALESCE(
        NULLIF(NEW.payload #>> '{_observability,captured_at_epoch_ms}', ''),
        NULLIF(NEW.payload #>> '{captured_at_epoch_ms}', ''),
        NULLIF(NEW.payload #>> '{working_state_event,recorded_at_epoch_ms}', ''),
        NULLIF(NEW.payload #>> '{token_budget_event,created_at_epoch_ms}', ''),
        NULLIF(NEW.payload #>> '{continuity_import,imported_at_epoch_ms}', ''),
        NULLIF(NEW.payload #>> '{continuity_thread_index,captured_at_epoch_ms}', ''),
        NULLIF(NEW.payload #>> '{continuity_handoff,captured_at_epoch_ms}', ''),
        NULLIF(NEW.payload #>> '{benchmark,captured_at_epoch_ms}', ''),
        NULLIF(NEW.payload #>> '{accuracy_verification,captured_at_epoch_ms}', ''),
        NULLIF(NEW.payload #>> '{load_verification,captured_at_epoch_ms}', ''),
        NULLIF(NEW.payload #>> '{cold_benchmark,captured_at_epoch_ms}', '')
    );
    IF NEW.captured_at_epoch_ms IS NULL AND captured_text ~ '^-?[0-9]+$' THEN
        NEW.captured_at_epoch_ms := captured_text::BIGINT;
    END IF;

    NEW.payload_sha256 := computed_payload_sha256;
    NEW.event_key := COALESCE(
        NULLIF(NEW.event_key, ''),
        NULLIF(NEW.payload #>> '{_observability,event_key}', ''),
        NEW.source_event_id,
        'sha256:' || computed_payload_sha256
    );
    NEW.replay_count := COALESCE(NEW.replay_count, 0);
    NEW.last_seen_at := COALESCE(NEW.last_seen_at, NEW.created_at, now());

    IF NEW.snapshot_kind IN (
        'retrieval_benchmark_hot',
        'retrieval_benchmark_cold',
        'retrieval_load_hot',
        'retrieval_load_cold',
        'retrieval_accuracy',
        'cold_path_benchmark',
        'token_benchmark',
        'token_benchmark_suite',
        'text_compare',
        'mcp_task_matrix',
        'memory_task_matrix'
    ) AND NEW.source_class <> 'benchmark' THEN
        RAISE EXCEPTION
            'benchmark lane contamination blocked for snapshot_kind=% source_class=%',
            NEW.snapshot_kind,
            NEW.source_class
            USING ERRCODE = '23514';
    END IF;

    IF jsonb_typeof(NEW.payload) = 'object' THEN
        NEW.payload := jsonb_set(
            NEW.payload,
            '{_observability}',
            COALESCE(NEW.payload -> '_observability', '{}'::jsonb) || jsonb_build_object(
                'snapshot_kind', NEW.snapshot_kind,
                'event_key', NEW.event_key,
                'source_event_id', NEW.source_event_id,
                'source_kind', NEW.source_kind,
                'source_class', NEW.source_class,
                'scope_project_code', NEW.scope_project_code,
                'scope_namespace_code', NEW.scope_namespace_code,
                'captured_at_epoch_ms', NEW.captured_at_epoch_ms,
                'payload_sha256', NEW.payload_sha256,
                'replay_protected', NEW.source_event_id IS NOT NULL
            ),
            true
        );
    END IF;

    RETURN NEW;
END;
$$;

UPDATE ami.observability_snapshots
SET
    snapshot_kind = snapshot_kind || '_quarantine',
    source_class = 'live_context'
WHERE snapshot_kind IN (
    'retrieval_benchmark_hot',
    'retrieval_benchmark_cold',
    'retrieval_load_hot',
    'retrieval_load_cold',
    'retrieval_accuracy',
    'cold_path_benchmark',
    'token_benchmark',
    'token_benchmark_suite',
    'text_compare',
    'mcp_task_matrix',
    'memory_task_matrix'
)
AND ami.observability_snapshot_source_class(snapshot_kind, payload) <> 'benchmark';

CREATE OR REPLACE TRIGGER trg_ami_observability_snapshots_fill_defaults
BEFORE INSERT OR UPDATE ON ami.observability_snapshots
FOR EACH ROW
EXECUTE FUNCTION ami.fill_observability_snapshot_defaults();

CREATE TABLE IF NOT EXISTS ami.task_nodes (
    task_node_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    parent_task_node_id UUID REFERENCES ami.task_nodes(task_node_id) ON DELETE SET NULL,
    memory_item_id UUID REFERENCES ami.memory_items(memory_item_id) ON DELETE SET NULL,
    task_key TEXT CHECK (task_key IS NULL OR btrim(task_key) <> ''),
    task_role TEXT NOT NULL DEFAULT 'workline' CHECK (
        task_role IN ('root', 'workline', 'child', 'pending_return', 'proposal', 'historical')
    ),
    headline TEXT NOT NULL,
    summary TEXT,
    next_step TEXT,
    execution_state TEXT NOT NULL DEFAULT 'proposed' CHECK (
        execution_state IN ('proposed', 'ready', 'active', 'blocked', 'waiting_external', 'in_review', 'done', 'failed', 'canceled', 'superseded')
    ),
    lifecycle_state TEXT NOT NULL DEFAULT 'hot' CHECK (
        lifecycle_state IN ('hot', 'closed', 'archived', 'deprecated', 'quarantined')
    ),
    confidence DOUBLE PRECISION,
    current_score DOUBLE PRECISION,
    reopened_count INTEGER NOT NULL DEFAULT 0,
    child_count INTEGER NOT NULL DEFAULT 0,
    closed_child_count INTEGER NOT NULL DEFAULT 0,
    pending_return_count INTEGER NOT NULL DEFAULT 0,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    candidate_class TEXT NOT NULL DEFAULT 'commitment' CHECK (
        candidate_class IN ('fact', 'decision', 'commitment', 'skill_hint', 'artifact_ref')
    ),
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    source_kind TEXT,
    hot_path_write_eligible BOOLEAN NOT NULL DEFAULT TRUE,
    background_consolidation_recommended BOOLEAN NOT NULL DEFAULT FALSE,
    status_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    opened_at_epoch_ms BIGINT,
    closed_at_epoch_ms BIGINT,
    archived_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.task_events (
    task_event_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    task_node_id UUID NOT NULL REFERENCES ami.task_nodes(task_node_id) ON DELETE CASCADE,
    source_snapshot_id UUID REFERENCES ami.observability_snapshots(snapshot_id) ON DELETE SET NULL,
    source_event_id TEXT,
    event_kind TEXT NOT NULL CHECK (
        event_kind IN ('created', 'continued', 'branched_child', 'branched_new', 'resumed', 'pending_return', 'closed', 'archived', 'reopened', 'superseded', 'state_change', 'evidence_request')
    ),
    prior_execution_state TEXT,
    next_execution_state TEXT,
    prior_lifecycle_state TEXT,
    next_lifecycle_state TEXT,
    source_kind TEXT,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'raw_capture' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'task-event-envelope-v1',
    event_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    recorded_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.task_nodes
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.task_nodes
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.task_nodes
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.task_nodes
    ADD COLUMN IF NOT EXISTS candidate_class TEXT NOT NULL DEFAULT 'commitment';

ALTER TABLE ami.task_nodes
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.task_nodes
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.task_nodes
    ADD COLUMN IF NOT EXISTS hot_path_write_eligible BOOLEAN NOT NULL DEFAULT TRUE;

ALTER TABLE ami.task_nodes
    ADD COLUMN IF NOT EXISTS background_consolidation_recommended BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE ami.task_nodes
    DROP CONSTRAINT IF EXISTS task_nodes_candidate_class_check;

ALTER TABLE ami.task_nodes
    ADD CONSTRAINT task_nodes_candidate_class_check CHECK (
        candidate_class IN ('fact', 'decision', 'commitment', 'skill_hint', 'artifact_ref')
    );

ALTER TABLE ami.task_nodes
    DROP CONSTRAINT IF EXISTS task_nodes_derivation_kind_check;

ALTER TABLE ami.task_nodes
    ADD CONSTRAINT task_nodes_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.task_events
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.task_events
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.task_events
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.task_events
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.task_events
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'raw_capture';

ALTER TABLE ami.task_events
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'task-event-envelope-v1';

ALTER TABLE ami.task_events
    DROP CONSTRAINT IF EXISTS task_events_derivation_kind_check;

ALTER TABLE ami.task_events
    ADD CONSTRAINT task_events_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

CREATE TABLE IF NOT EXISTS ami.memory_link_decisions (
    memory_link_decision_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    task_node_id UUID REFERENCES ami.task_nodes(task_node_id) ON DELETE SET NULL,
    retrieval_trace_id UUID REFERENCES ami.retrieval_traces(retrieval_trace_id) ON DELETE SET NULL,
    candidate_task_node_id UUID REFERENCES ami.task_nodes(task_node_id) ON DELETE SET NULL,
    decision_outcome TEXT NOT NULL CHECK (
        decision_outcome IN ('continue', 'child', 'new', 'abstain', 'escalate', 'pending_link_proposal')
    ),
    legality_passed BOOLEAN NOT NULL DEFAULT FALSE,
    scope_filter_passed BOOLEAN NOT NULL DEFAULT FALSE,
    evidence_sufficient BOOLEAN NOT NULL DEFAULT FALSE,
    classifier_label TEXT,
    classifier_score DOUBLE PRECISION,
    decision_reason TEXT,
    decision_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'memory-link-decision-envelope-v1',
    recorded_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.memory_link_decisions
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.memory_link_decisions
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.memory_link_decisions
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.memory_link_decisions
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.memory_link_decisions
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.memory_link_decisions
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'memory-link-decision-envelope-v1';

ALTER TABLE ami.memory_link_decisions
    DROP CONSTRAINT IF EXISTS memory_link_decisions_derivation_kind_check;

ALTER TABLE ami.memory_link_decisions
    ADD CONSTRAINT memory_link_decisions_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

CREATE TABLE IF NOT EXISTS ami.pending_link_proposals (
    pending_link_proposal_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    task_node_id UUID REFERENCES ami.task_nodes(task_node_id) ON DELETE SET NULL,
    retrieval_trace_id UUID REFERENCES ami.retrieval_traces(retrieval_trace_id) ON DELETE SET NULL,
    candidate_task_node_id UUID REFERENCES ami.task_nodes(task_node_id) ON DELETE SET NULL,
    proposal_state TEXT NOT NULL DEFAULT 'pending' CHECK (
        proposal_state IN ('pending', 'accepted', 'rejected', 'expired', 'escalated', 'archived')
    ),
    proposal_reason TEXT NOT NULL,
    evidence_request TEXT,
    evidence_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    classifier_score DOUBLE PRECISION,
    ttl_epoch_ms BIGINT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'pending-link-proposal-envelope-v1',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

ALTER TABLE ami.pending_link_proposals
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.pending_link_proposals
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.pending_link_proposals
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.pending_link_proposals
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.pending_link_proposals
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.pending_link_proposals
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'pending-link-proposal-envelope-v1';

ALTER TABLE ami.pending_link_proposals
    DROP CONSTRAINT IF EXISTS pending_link_proposals_derivation_kind_check;

ALTER TABLE ami.pending_link_proposals
    ADD CONSTRAINT pending_link_proposals_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.artifact_refs
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.artifact_refs
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.artifact_refs
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.artifact_refs
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.artifact_refs
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.artifact_refs
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'artifact-ref-envelope-v1';

ALTER TABLE ami.artifact_refs
    DROP CONSTRAINT IF EXISTS artifact_refs_derivation_kind_check;

ALTER TABLE ami.artifact_refs
    ADD CONSTRAINT artifact_refs_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.skill_evidence_bundles
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.skill_evidence_bundles
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_evidence_bundles
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.skill_evidence_bundles
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.skill_evidence_bundles
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'skill-evidence-bundle-envelope-v1';

ALTER TABLE ami.skill_evidence_bundles
    DROP CONSTRAINT IF EXISTS skill_evidence_bundles_derivation_kind_check;

ALTER TABLE ami.skill_evidence_bundles
    ADD CONSTRAINT skill_evidence_bundles_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.skill_trial_runs
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.skill_trial_runs
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_trial_runs
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_trial_runs
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_trial_runs
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.skill_trial_runs
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.skill_trial_runs
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'skill-trial-run-envelope-v1';

ALTER TABLE ami.skill_trial_runs
    DROP CONSTRAINT IF EXISTS skill_trial_runs_derivation_kind_check;

ALTER TABLE ami.skill_trial_runs
    ADD CONSTRAINT skill_trial_runs_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.skill_evals
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.skill_evals
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_evals
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_evals
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_evals
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.skill_evals
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.skill_evals
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'skill-eval-envelope-v1';

ALTER TABLE ami.skill_evals
    DROP CONSTRAINT IF EXISTS skill_evals_verdict_check;

ALTER TABLE ami.skill_evals
    ADD CONSTRAINT skill_evals_verdict_check CHECK (
        verdict IN (
            'candidate_only',
            'promote_shadow',
            'promote_trial',
            'promote_verified',
            'approve_shared_promotion',
            'reject',
            'quarantine',
            'deprecate'
        )
    );

ALTER TABLE ami.skill_evals
    DROP CONSTRAINT IF EXISTS skill_evals_derivation_kind_check;

ALTER TABLE ami.skill_evals
    ADD CONSTRAINT skill_evals_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.skill_trigger_matches
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.skill_trigger_matches
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_trigger_matches
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_trigger_matches
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_trigger_matches
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.skill_trigger_matches
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.skill_trigger_matches
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'skill-trigger-match-envelope-v1';

ALTER TABLE ami.skill_trigger_matches
    DROP CONSTRAINT IF EXISTS skill_trigger_matches_derivation_kind_check;

ALTER TABLE ami.skill_trigger_matches
    ADD CONSTRAINT skill_trigger_matches_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.skill_reuse_logs
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.skill_reuse_logs
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.skill_reuse_logs
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.skill_reuse_logs
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.skill_reuse_logs
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'skill-reuse-log-envelope-v1';

ALTER TABLE ami.skill_reuse_logs
    DROP CONSTRAINT IF EXISTS skill_reuse_logs_derivation_kind_check;

ALTER TABLE ami.skill_reuse_logs
    ADD CONSTRAINT skill_reuse_logs_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

ALTER TABLE ami.memory_relation_edges
    ADD COLUMN IF NOT EXISTS source_kind TEXT;

ALTER TABLE ami.memory_relation_edges
    ADD COLUMN IF NOT EXISTS source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.memory_relation_edges
    ADD COLUMN IF NOT EXISTS artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.memory_relation_edges
    ADD COLUMN IF NOT EXISTS message_refs JSONB NOT NULL DEFAULT '[]'::jsonb;

ALTER TABLE ami.memory_relation_edges
    ADD COLUMN IF NOT EXISTS evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE ami.memory_relation_edges
    ADD COLUMN IF NOT EXISTS derivation_kind TEXT NOT NULL DEFAULT 'extract';

ALTER TABLE ami.memory_relation_edges
    ADD COLUMN IF NOT EXISTS schema_version TEXT NOT NULL DEFAULT 'memory-relation-edge-envelope-v1';

ALTER TABLE ami.memory_relation_edges
    DROP CONSTRAINT IF EXISTS memory_relation_edges_derivation_kind_check;

ALTER TABLE ami.memory_relation_edges
    ADD CONSTRAINT memory_relation_edges_derivation_kind_check CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    );

CREATE TABLE IF NOT EXISTS ami.memory_link_decisions (
    memory_link_decision_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    task_node_id UUID REFERENCES ami.task_nodes(task_node_id) ON DELETE SET NULL,
    retrieval_trace_id UUID REFERENCES ami.retrieval_traces(retrieval_trace_id) ON DELETE SET NULL,
    candidate_task_node_id UUID REFERENCES ami.task_nodes(task_node_id) ON DELETE SET NULL,
    decision_outcome TEXT NOT NULL CHECK (
        decision_outcome IN ('continue', 'child', 'new', 'abstain', 'escalate', 'pending_link_proposal')
    ),
    legality_passed BOOLEAN NOT NULL DEFAULT FALSE,
    scope_filter_passed BOOLEAN NOT NULL DEFAULT FALSE,
    evidence_sufficient BOOLEAN NOT NULL DEFAULT FALSE,
    classifier_label TEXT,
    classifier_score DOUBLE PRECISION,
    decision_reason TEXT,
    decision_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'memory-link-decision-envelope-v1',
    recorded_at_epoch_ms BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.pending_link_proposals (
    pending_link_proposal_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workspace_id UUID NOT NULL REFERENCES ami.workspaces(workspace_id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    task_node_id UUID REFERENCES ami.task_nodes(task_node_id) ON DELETE SET NULL,
    retrieval_trace_id UUID REFERENCES ami.retrieval_traces(retrieval_trace_id) ON DELETE SET NULL,
    candidate_task_node_id UUID REFERENCES ami.task_nodes(task_node_id) ON DELETE SET NULL,
    proposal_state TEXT NOT NULL DEFAULT 'pending' CHECK (
        proposal_state IN ('pending', 'accepted', 'rejected', 'expired', 'escalated', 'archived')
    ),
    proposal_reason TEXT NOT NULL,
    evidence_request TEXT,
    evidence_payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    classifier_score DOUBLE PRECISION,
    ttl_epoch_ms BIGINT,
    source_event_ids JSONB NOT NULL DEFAULT '[]'::jsonb,
    artifact_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    message_refs JSONB NOT NULL DEFAULT '[]'::jsonb,
    evidence_span JSONB NOT NULL DEFAULT '{}'::jsonb,
    derivation_kind TEXT NOT NULL DEFAULT 'extract' CHECK (
        derivation_kind IN (
            'raw_capture',
            'extract',
            'summary',
            'merge',
            'import',
            'verified_write_back',
            'operator_write'
        )
    ),
    schema_version TEXT NOT NULL DEFAULT 'pending-link-proposal-envelope-v1',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS ami.execctl_task_ledger_entries (
    ledger_entry_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID NOT NULL REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    agent_scope TEXT NOT NULL CHECK (btrim(agent_scope) <> ''),
    session_id TEXT,
    thread_id TEXT,
    source_snapshot_id UUID REFERENCES ami.observability_snapshots(snapshot_id) ON DELETE SET NULL,
    source_event_id TEXT NOT NULL UNIQUE CHECK (btrim(source_event_id) <> ''),
    event_kind TEXT NOT NULL CHECK (event_kind IN ('continuity_handoff')),
    source_kind TEXT NOT NULL,
    headline TEXT NOT NULL,
    next_step TEXT NOT NULL,
    summary TEXT NOT NULL,
    active_files JSONB NOT NULL DEFAULT '[]'::jsonb,
    open_questions JSONB NOT NULL DEFAULT '[]'::jsonb,
    materialized_notes JSONB NOT NULL DEFAULT '[]'::jsonb,
    pending_return_queue JSONB NOT NULL DEFAULT '[]'::jsonb,
    local_path TEXT,
    recorded_at_epoch_ms BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_execctl_task_ledger_scope_recorded
    ON ami.execctl_task_ledger_entries(project_id, namespace_id, agent_scope, recorded_at_epoch_ms DESC);

CREATE INDEX IF NOT EXISTS idx_execctl_task_ledger_snapshot
    ON ami.execctl_task_ledger_entries(source_snapshot_id);

CREATE TABLE IF NOT EXISTS ami.execctl_task_leases (
    lease_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID NOT NULL REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID NOT NULL REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    agent_scope TEXT NOT NULL CHECK (btrim(agent_scope) <> ''),
    owner_session_id TEXT,
    owner_thread_id TEXT,
    source_snapshot_id UUID REFERENCES ami.observability_snapshots(snapshot_id) ON DELETE SET NULL,
    source_event_id TEXT NOT NULL CHECK (btrim(source_event_id) <> ''),
    source_kind TEXT NOT NULL,
    lease_state TEXT NOT NULL CHECK (lease_state IN ('active')),
    headline TEXT NOT NULL,
    next_step TEXT NOT NULL,
    local_path TEXT,
    acquired_at_epoch_ms BIGINT NOT NULL,
    heartbeat_at_epoch_ms BIGINT NOT NULL,
    expires_at_epoch_ms BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (project_id, namespace_id, agent_scope)
);

CREATE INDEX IF NOT EXISTS idx_execctl_task_leases_scope_expiry
    ON ami.execctl_task_leases(project_id, namespace_id, agent_scope, expires_at_epoch_ms DESC);

CREATE INDEX IF NOT EXISTS idx_ami_namespaces_project ON ami.namespaces(project_id);
CREATE INDEX IF NOT EXISTS idx_ami_relations_source_target ON ami.project_relations(source_project_id, target_project_id);
CREATE INDEX IF NOT EXISTS idx_ami_documents_project_namespace ON ami.code_documents(project_id, namespace_id);
CREATE INDEX IF NOT EXISTS idx_ami_documents_relative_path ON ami.code_documents(relative_path);
CREATE INDEX IF NOT EXISTS idx_ami_documents_namespace_relative_basename
    ON ami.code_documents(namespace_id, relative_basename);
CREATE INDEX IF NOT EXISTS idx_ami_documents_namespace_relative_basename_stem
    ON ami.code_documents(namespace_id, relative_basename_stem);
CREATE INDEX IF NOT EXISTS idx_ami_documents_search ON ami.code_documents USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_ami_symbols_document ON ami.code_symbols(document_id);
CREATE INDEX IF NOT EXISTS idx_ami_symbols_name ON ami.code_symbols(name);
CREATE INDEX IF NOT EXISTS idx_ami_symbols_search ON ami.code_symbols USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_ami_chunks_document ON ami.code_chunks(document_id);
CREATE INDEX IF NOT EXISTS idx_ami_chunks_search ON ami.code_chunks USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_ami_memory_search ON ami.memory_cards USING GIN (search_vector);
CREATE INDEX IF NOT EXISTS idx_ami_memory_truth_state ON ami.memory_cards (truth_state);
CREATE INDEX IF NOT EXISTS idx_ami_memory_valid_from ON ami.memory_cards (valid_from_epoch_ms);
CREATE INDEX IF NOT EXISTS idx_ami_memory_valid_to ON ami.memory_cards (valid_to_epoch_ms);
CREATE INDEX IF NOT EXISTS idx_ami_memory_superseded_by ON ami.memory_cards (superseded_by_memory_card_id);
CREATE INDEX IF NOT EXISTS idx_ami_memory_relation_source ON ami.memory_relation_edges (source_memory_card_id);
CREATE INDEX IF NOT EXISTS idx_ami_memory_relation_target ON ami.memory_relation_edges (target_memory_card_id);
CREATE INDEX IF NOT EXISTS idx_ami_memory_relation_type ON ami.memory_relation_edges (relation_type);
CREATE INDEX IF NOT EXISTS idx_ami_memory_relation_scope ON ami.memory_relation_edges (project_id, namespace_id);
CREATE INDEX IF NOT EXISTS idx_ami_memory_transition_card ON ami.memory_card_transitions (memory_card_id);
CREATE INDEX IF NOT EXISTS idx_ami_memory_transition_scope ON ami.memory_card_transitions (project_id, namespace_id);
CREATE INDEX IF NOT EXISTS idx_ami_memory_items_project_kind
    ON ami.memory_items(project_id, namespace_id, item_kind, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_memory_items_identity_key
    ON ami.memory_items(project_id, identity_key)
    WHERE identity_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_ami_memory_edges_source
    ON ami.memory_edges(source_memory_item_id, edge_kind, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_memory_edges_target
    ON ami.memory_edges(target_memory_item_id, edge_kind, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_memory_conflicts_scope_state
    ON ami.memory_conflicts(project_id, namespace_id, conflict_state, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_memory_provenance_item
    ON ami.memory_provenance(memory_item_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_ami_memory_raw_events_scope_order
    ON ami.memory_raw_events(project_id, namespace_id, server_order_seq DESC);

CREATE INDEX IF NOT EXISTS idx_ami_memory_write_outbox_state_created
    ON ami.memory_write_outbox(delivery_state, created_at ASC);

CREATE INDEX IF NOT EXISTS idx_ami_memory_write_outbox_item_kind
    ON ami.memory_write_outbox(memory_item_id, delivery_kind);
CREATE INDEX IF NOT EXISTS idx_ami_context_packs_project_namespace ON ami.context_packs(project_id, namespace_id);
CREATE INDEX IF NOT EXISTS idx_ami_context_packs_artifact_state_created
    ON ami.context_packs(artifact_state, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_ami_retrieval_traces_scope_created
    ON ami.retrieval_traces(project_id, namespace_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_restore_packs_scope_created
    ON ami.restore_packs(project_id, namespace_id, pack_kind, created_at DESC);
WITH ranked_restore_packs AS (
    SELECT
        restore_pack_id,
        ROW_NUMBER() OVER (
            PARTITION BY project_id, namespace_id, pack_kind, source_snapshot_id
            ORDER BY captured_at_epoch_ms DESC NULLS LAST, created_at DESC, restore_pack_id DESC
        ) AS row_rank
    FROM ami.restore_packs
    WHERE source_snapshot_id IS NOT NULL
)
DELETE FROM ami.restore_packs rp
USING ranked_restore_packs ranked
WHERE rp.restore_pack_id = ranked.restore_pack_id
  AND ranked.row_rank > 1;
CREATE UNIQUE INDEX IF NOT EXISTS idx_ami_restore_packs_same_source_snapshot
    ON ami.restore_packs(project_id, namespace_id, pack_kind, source_snapshot_id)
    WHERE source_snapshot_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_ami_observability_snapshots_kind_created
    ON ami.observability_snapshots(snapshot_kind, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_policy_rules_scope_status
    ON ami.policy_rules(workspace_id, project_id, namespace_id, rule_kind, rule_status, precedence ASC);
CREATE INDEX IF NOT EXISTS idx_ami_quarantine_items_entity
    ON ami.quarantine_items(entity_kind, entity_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_task_nodes_scope_state
    ON ami.task_nodes(project_id, namespace_id, execution_state, lifecycle_state, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_task_nodes_parent
    ON ami.task_nodes(parent_task_node_id, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_task_events_task_recorded
    ON ami.task_events(task_node_id, recorded_at_epoch_ms DESC, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_memory_link_decisions_scope_recorded
    ON ami.memory_link_decisions(project_id, namespace_id, recorded_at_epoch_ms DESC, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_pending_link_proposals_scope_state
    ON ami.pending_link_proposals(project_id, namespace_id, proposal_state, updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_observability_snapshots_kind_scope_project_created
    ON ami.observability_snapshots(snapshot_kind, scope_project_code, created_at DESC)
    WHERE scope_project_code IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS idx_ami_observability_snapshots_kind_event_key
    ON ami.observability_snapshots(snapshot_kind, event_key);
CREATE INDEX IF NOT EXISTS idx_ami_observability_snapshots_kind_source_class
    ON ami.observability_snapshots(snapshot_kind, source_class, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ami_observability_working_state_retrieval_thread_captured
    ON ami.observability_snapshots(
        (payload #>> '{working_state_event,thread_id}'),
        captured_at_epoch_ms DESC
    )
    WHERE snapshot_kind = 'working_state_event'
      AND payload #>> '{working_state_event,event_kind}' = 'retrieval_context_pack'
      AND captured_at_epoch_ms IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_ami_observability_token_budget_context_pack_created
    ON ami.observability_snapshots(
        (payload #>> '{token_budget_event,context_pack_id}'),
        created_at DESC
    )
    WHERE snapshot_kind = 'token_budget_event'
      AND payload #>> '{token_budget_event,context_pack_id}' IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_ami_observability_working_state_context_pack_created
    ON ami.observability_snapshots(
        (payload #>> '{working_state_event,context_pack_id}'),
        created_at DESC
    )
    WHERE snapshot_kind = 'working_state_event'
      AND payload #>> '{working_state_event,event_kind}' = 'retrieval_context_pack'
      AND payload #>> '{working_state_event,context_pack_id}' IS NOT NULL;

CREATE OR REPLACE VIEW ami.project_links AS
SELECT
    r.relation_id AS project_link_id,
    sw.workspace_id AS source_workspace_id,
    sw.code AS source_workspace_code,
    tp.workspace_id AS target_workspace_id,
    tw.code AS target_workspace_code,
    r.source_project_id,
    sp.code AS source_project_code,
    sp.display_name AS source_project_display_name,
    r.target_project_id,
    tp.project_id AS target_project_pk,
    tp.code AS target_project_code,
    tp.display_name AS target_project_display_name,
    r.relation_type,
    r.project_link_type,
    r.shared_contour,
    r.visibility_scope,
    r.relation_status,
    r.requires_approval,
    r.access_mode,
    pol.code AS transfer_policy_code,
    r.metadata,
    r.created_at
FROM ami.project_relations r
JOIN ami.projects sp ON sp.project_id = r.source_project_id
JOIN ami.workspaces sw ON sw.workspace_id = sp.workspace_id
JOIN ami.projects tp ON tp.project_id = r.target_project_id
JOIN ami.workspaces tw ON tw.workspace_id = tp.workspace_id
LEFT JOIN ami.transfer_policies pol ON pol.transfer_policy_id = r.transfer_policy_id;

CREATE OR REPLACE VIEW ami.truth_layer_surface_registry AS
SELECT *
FROM (
    VALUES
        ('workspace', 'workspaces', 'table', 'canonical_truth_surface', NULL),
        ('project', 'projects', 'table', 'canonical_truth_surface', NULL),
        ('project_link', 'project_links', 'view', 'canonical_truth_surface', 'backed by ami.project_relations'),
        ('memory_item', 'memory_items', 'table', 'canonical_truth_surface', NULL),
        ('memory_edge', 'memory_edges', 'table', 'canonical_truth_surface', 'item-graph truth surface'),
        ('memory_conflict', 'memory_conflicts', 'table', 'canonical_truth_surface', NULL),
        ('memory_provenance', 'memory_provenance', 'table', 'canonical_truth_surface', NULL),
        ('skill_card', 'skill_cards', 'table', 'canonical_truth_surface', NULL),
        ('policy_rule', 'policy_rules', 'table', 'canonical_truth_surface', 'access_policies remains authorization/control-plane surface'),
        ('retrieval_trace', 'retrieval_traces', 'table', 'canonical_truth_surface', NULL),
        ('restore_pack', 'restore_packs', 'table', 'canonical_truth_surface', NULL),
        ('import_packet', 'import_packets', 'table', 'canonical_truth_surface', NULL),
        ('quarantine_item', 'quarantine_items', 'table', 'canonical_truth_surface', NULL),
        ('memory_card', 'memory_cards', 'table', 'adjunct_truth_surface', 'stage-2 factual/card contour'),
        ('memory_card_edge', 'memory_relation_edges', 'table', 'adjunct_truth_surface', 'card-graph truth surface parallel to item-edge graph'),
        ('access_policy', 'access_policies', 'table', 'control_plane_surface', 'authorization matrix, not replacement for policy_rules')
) AS registry(
    truth_entity_code,
    canonical_surface_name,
    canonical_surface_kind,
    surface_role,
    notes
);

-- Stage 9: forgetting audit log (explainability)
CREATE TABLE IF NOT EXISTS ami.forgetting_audit_log (
    audit_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    memory_item_id UUID NOT NULL REFERENCES ami.memory_items(memory_item_id),
    action TEXT NOT NULL CHECK (
        action IN (
            'prune_ttl_expired',
            'prune_low_utility',
            'archive_cold_tier',
            'revalidate_stale',
            'dedup_compacted'
        )
    ),
    previous_state TEXT NOT NULL,
    new_state TEXT NOT NULL,
    reason TEXT NOT NULL,
    retention_class TEXT NOT NULL,
    decay_policy TEXT NOT NULL,
    project_code TEXT NOT NULL,
    namespace_code TEXT NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_forgetting_audit_log_memory_item_id
    ON ami.forgetting_audit_log(memory_item_id);

CREATE INDEX IF NOT EXISTS idx_forgetting_audit_log_recorded_at
    ON ami.forgetting_audit_log(recorded_at);

DROP MATERIALIZED VIEW IF EXISTS ami.lifecycle_transition_stats_v1;

DROP VIEW IF EXISTS ami.lifecycle_transition_events_v1;

CREATE VIEW ami.lifecycle_transition_events_v1 AS
WITH audit_base AS (
    SELECT
        fal.audit_id AS transition_event_id,
        fal.memory_item_id,
        fal.action,
        fal.previous_state,
        fal.new_state,
        fal.project_code,
        fal.namespace_code,
        fal.recorded_at,
        mi.created_at AS memory_created_at,
        mi.derivation_kind,
        mi.retention_class,
        mi.decay_policy,
        mi.freshness_score,
        mi.utility_score,
        mi.access_count,
        mi.visibility_scope,
        mi.item_kind,
        mi.truth_state,
        mi.trust_state,
        mi.verification_state,
        mi.lifecycle_state,
        LAG(fal.recorded_at) OVER (
            PARTITION BY fal.memory_item_id
            ORDER BY fal.recorded_at, fal.audit_id
        ) AS previous_recorded_at
    FROM ami.forgetting_audit_log fal
    JOIN ami.memory_items mi ON mi.memory_item_id = fal.memory_item_id
),
classified AS (
    SELECT
        *,
        (
            derivation_kind IN ('raw_capture', 'operator_write', 'verified_write_back')
            OR retention_class IN ('durable', 'legal_hold')
            OR decay_policy = 'retain_forever'
        ) AS is_protected,
        (
            visibility_scope = 'quarantine'
            OR item_kind = 'quarantine'
            OR truth_state = 'quarantined'
            OR trust_state = 'quarantined'
            OR verification_state = 'quarantined'
            OR lifecycle_state = 'quarantined'
        ) AS is_quarantined,
        CASE
            WHEN freshness_score < 0.30 THEN 'active_stale'
            ELSE 'active_hot'
        END AS active_state_variant,
        CASE
            WHEN freshness_score < 0.05 THEN 'critical_stale'
            WHEN freshness_score < 0.30 THEN 'stale'
            WHEN freshness_score < 0.70 THEN 'warm'
            ELSE 'fresh'
        END AS freshness_band,
        CASE
            WHEN utility_score < 0.05 THEN 'low'
            WHEN utility_score < 0.30 THEN 'medium'
            ELSE 'high'
        END AS utility_band,
        CASE
            WHEN access_count <= 0 THEN 'none'
            WHEN access_count < 3 THEN 'low'
            WHEN access_count < 10 THEN 'medium'
            ELSE 'high'
        END AS access_band
    FROM audit_base
)
SELECT
    transition_event_id,
    memory_item_id,
    CASE
        WHEN previous_state = 'pending_review' THEN 'pending_review'
        WHEN previous_state = 'compacted' THEN 'compacted'
        WHEN previous_state = 'archived' THEN 'archived'
        WHEN previous_state = 'pruned' THEN 'pruned'
        WHEN previous_state = 'quarantined' THEN 'quarantined'
        WHEN previous_state = 'protected' THEN 'protected'
        WHEN previous_state = 'active' THEN
            CASE
                WHEN is_quarantined THEN 'quarantined'
                WHEN is_protected THEN 'protected'
                WHEN action IN (
                    'prune_ttl_expired',
                    'prune_low_utility',
                    'archive_cold_tier',
                    'revalidate_stale',
                    'dedup_compacted'
                ) THEN 'active_stale'
                ELSE active_state_variant
            END
        ELSE
            CASE
                WHEN is_quarantined THEN 'quarantined'
                WHEN is_protected THEN 'protected'
                ELSE active_state_variant
            END
    END AS observed_state,
    CASE
        WHEN new_state = 'pending_review' THEN 'pending_review'
        WHEN new_state = 'compacted' THEN 'compacted'
        WHEN new_state = 'archived' THEN 'archived'
        WHEN new_state = 'pruned' THEN 'pruned'
        WHEN new_state = 'quarantined' THEN 'quarantined'
        WHEN new_state = 'protected' THEN 'protected'
        WHEN new_state = 'active' THEN
            CASE
                WHEN is_quarantined THEN 'quarantined'
                WHEN is_protected THEN 'protected'
                ELSE active_state_variant
            END
        ELSE
            CASE
                WHEN is_quarantined THEN 'quarantined'
                WHEN is_protected THEN 'protected'
                ELSE active_state_variant
            END
    END AS next_state,
    GREATEST(
        0,
        FLOOR(
            EXTRACT(
                EPOCH FROM (
                    recorded_at - COALESCE(previous_recorded_at, memory_created_at)
                )
            ) * 1000.0
        )
    )::BIGINT AS dwell_ms,
    derivation_kind,
    retention_class,
    decay_policy,
    freshness_band,
    utility_band,
    access_band,
    project_code,
    namespace_code,
    recorded_at
FROM classified;

CREATE MATERIALIZED VIEW ami.lifecycle_transition_stats_v1 AS
SELECT
    project_code,
    namespace_code,
    observed_state,
    next_state,
    derivation_kind,
    retention_class,
    decay_policy,
    freshness_band,
    utility_band,
    access_band,
    COUNT(*)::BIGINT AS transition_count,
    COALESCE(SUM(dwell_ms), 0)::BIGINT AS total_dwell_ms,
    COALESCE(ROUND(AVG(dwell_ms)), 0)::BIGINT AS avg_dwell_ms,
    COALESCE(
        percentile_cont(0.5) WITHIN GROUP (ORDER BY dwell_ms::DOUBLE PRECISION)::BIGINT,
        0::BIGINT
    ) AS p50_dwell_ms,
    COALESCE(
        percentile_cont(0.9) WITHIN GROUP (ORDER BY dwell_ms::DOUBLE PRECISION)::BIGINT,
        0::BIGINT
    ) AS p90_dwell_ms,
    MAX(recorded_at) AS last_recorded_at
FROM ami.lifecycle_transition_events_v1
GROUP BY
    project_code,
    namespace_code,
    observed_state,
    next_state,
    derivation_kind,
    retention_class,
    decay_policy,
    freshness_band,
    utility_band,
    access_band;

CREATE INDEX IF NOT EXISTS idx_lifecycle_transition_stats_v1_scope
    ON ami.lifecycle_transition_stats_v1(project_code, namespace_code);
