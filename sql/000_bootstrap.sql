CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE SCHEMA IF NOT EXISTS ami;

CREATE TABLE IF NOT EXISTS ami.projects (
    project_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    code TEXT NOT NULL UNIQUE CHECK (code <> ''),
    display_name TEXT NOT NULL,
    repo_root TEXT NOT NULL UNIQUE,
    default_branch TEXT,
    status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'archived', 'disabled')),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
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
    shared_contour TEXT NOT NULL,
    access_mode TEXT NOT NULL CHECK (
        access_mode IN ('local_strict', 'local_plus_related', 'explicit_foreign', 'audit_global')
    ),
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_project_id, target_project_id, relation_type, shared_contour)
);

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
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

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

CREATE TABLE IF NOT EXISTS ami.artifact_refs (
    artifact_ref_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id UUID REFERENCES ami.projects(project_id) ON DELETE CASCADE,
    namespace_id UUID REFERENCES ami.namespaces(namespace_id) ON DELETE CASCADE,
    artifact_kind TEXT NOT NULL,
    bucket TEXT NOT NULL,
    object_key TEXT NOT NULL,
    content_type TEXT,
    metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (bucket, object_key)
);

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

CREATE TABLE IF NOT EXISTS ami.observability_snapshots (
    snapshot_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    snapshot_kind TEXT NOT NULL,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
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

DROP TRIGGER IF EXISTS trg_ami_observability_snapshots_fill_defaults
    ON ami.observability_snapshots;

CREATE TRIGGER trg_ami_observability_snapshots_fill_defaults
BEFORE INSERT OR UPDATE ON ami.observability_snapshots
FOR EACH ROW
EXECUTE FUNCTION ami.fill_observability_snapshot_defaults();

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
CREATE INDEX IF NOT EXISTS idx_ami_context_packs_project_namespace ON ami.context_packs(project_id, namespace_id);
CREATE INDEX IF NOT EXISTS idx_ami_observability_snapshots_kind_created
    ON ami.observability_snapshots(snapshot_kind, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_ami_observability_snapshots_kind_event_key
    ON ami.observability_snapshots(snapshot_kind, event_key);
CREATE INDEX IF NOT EXISTS idx_ami_observability_snapshots_kind_source_class
    ON ami.observability_snapshots(snapshot_kind, source_class, created_at DESC);
