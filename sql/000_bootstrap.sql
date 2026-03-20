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

CREATE INDEX IF NOT EXISTS idx_ami_namespaces_project ON ami.namespaces(project_id);
CREATE INDEX IF NOT EXISTS idx_ami_relations_source_target ON ami.project_relations(source_project_id, target_project_id);
CREATE INDEX IF NOT EXISTS idx_ami_documents_project_namespace ON ami.code_documents(project_id, namespace_id);
CREATE INDEX IF NOT EXISTS idx_ami_documents_relative_path ON ami.code_documents(relative_path);
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
