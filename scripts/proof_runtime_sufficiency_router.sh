#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

./scripts/proof_stage2_setup.sh >/tmp/amai-proof-runtime-sufficiency-router-setup.log

dsn="${AMI_POSTGRES_DSN}"
suffix="$(date +%s%N)"
structured_marker="stage2_structured_router_${suffix}"
raw_marker="stage2_raw_router_${suffix}"

source_card_id="$(psql "${dsn}" -Atqc "
  INSERT INTO ami.memory_cards(
    project_id,
    namespace_id,
    title,
    summary,
    body,
    provenance,
    truth_state,
    verification_state,
    status,
    observed_at_epoch_ms,
    recorded_at_epoch_ms,
    valid_from_epoch_ms
  )
  SELECT
    p.project_id,
    n.namespace_id,
    'structured source ${suffix}',
    '${structured_marker} summary source',
    '${structured_marker} body source',
    '{\"proof\":\"runtime_sufficiency_router\",\"path\":\"fixtures/project_alpha/src/lib.rs\"}'::jsonb,
    'current',
    'verified',
    'active',
    1000,
    1000,
    1000
  FROM ami.projects p
  JOIN ami.namespaces n ON n.project_id = p.project_id AND n.code = 'review'
  WHERE p.code = 'project_alpha'
  RETURNING memory_card_id::text
")"

target_card_id="$(psql "${dsn}" -Atqc "
  INSERT INTO ami.memory_cards(
    project_id,
    namespace_id,
    title,
    summary,
    body,
    provenance,
    truth_state,
    verification_state,
    status,
    observed_at_epoch_ms,
    recorded_at_epoch_ms,
    valid_from_epoch_ms
  )
  SELECT
    p.project_id,
    n.namespace_id,
    'structured target ${suffix}',
    'structured target support summary',
    'structured target support body',
    '{\"proof\":\"runtime_sufficiency_router\",\"path\":\"fixtures/project_alpha/src/lib.rs\"}'::jsonb,
    'current',
    'verified',
    'active',
    1000,
    1000,
    1000
  FROM ami.projects p
  JOIN ami.namespaces n ON n.project_id = p.project_id AND n.code = 'review'
  WHERE p.code = 'project_alpha'
  RETURNING memory_card_id::text
")"

psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_relation_edges(
    project_id,
    namespace_id,
    source_memory_card_id,
    target_memory_card_id,
    relation_type,
    relation_state,
    evidence,
    recorded_at_epoch_ms,
    valid_from_epoch_ms
  )
  SELECT
    p.project_id,
    n.namespace_id,
    '${source_card_id}'::uuid,
    '${target_card_id}'::uuid,
    'supports',
    'active',
    '{\"proof\":\"runtime_sufficiency_router\"}'::jsonb,
    1000,
    1000
  FROM ami.projects p
  JOIN ami.namespaces n ON n.project_id = p.project_id AND n.code = 'review'
  WHERE p.code = 'project_alpha'
"

raw_item_id="$(psql "${dsn}" -Atqc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    visibility_scope,
    item_kind,
    title,
    summary,
    body,
    sensitivity_class,
    truth_state,
    trust_state,
    verification_state,
    lifecycle_state,
    source_event_ids,
    artifact_refs,
    message_refs,
    evidence_span,
    derivation_kind,
    observed_at_epoch_ms,
    recorded_at_epoch_ms,
    valid_from_epoch_ms,
    last_verified_at_epoch_ms,
    utility_score,
    freshness_score,
    retention_class,
    imported_from,
    schema_version,
    metadata
  )
  SELECT
    p.workspace_id,
    p.project_id,
    n.namespace_id,
    p.visibility_scope,
    'fact',
    'raw evidence title ${suffix}',
    '${raw_marker} summary',
    '${raw_marker} body from immutable log',
    'internal',
    'current',
    'verified',
    'verified',
    'hot',
    '[\"event:${suffix}\"]'::jsonb,
    '[\"artifact://proof/router/${suffix}\"]'::jsonb,
    '[\"message:${suffix}\"]'::jsonb,
    '{\"path\":\"fixtures/project_alpha/logs/runtime.log\",\"line_start\":10,\"line_end\":12}'::jsonb,
    'raw_capture',
    2000,
    2000,
    2000,
    2000,
    0.8,
    0.8,
    'durable',
    '{}'::jsonb,
    'memory-envelope-v1',
    '{\"proof\":\"runtime_sufficiency_router\"}'::jsonb
  FROM ami.projects p
  JOIN ami.namespaces n ON n.project_id = p.project_id AND n.code = 'review'
  WHERE p.code = 'project_alpha'
  RETURNING memory_item_id::text
")"

psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_provenance(
    workspace_id,
    project_id,
    namespace_id,
    memory_item_id,
    source_kind,
    source_event_id,
    trust_level,
    message_refs,
    evidence_span,
    derivation_kind,
    observed_at_epoch_ms,
    recorded_at_epoch_ms,
    valid_from_epoch_ms,
    schema_version,
    details
  )
  SELECT
    p.workspace_id,
    p.project_id,
    n.namespace_id,
    '${raw_item_id}'::uuid,
    'raw_log',
    'event:${suffix}',
    'verified',
    '[\"message:${suffix}\"]'::jsonb,
    '{\"path\":\"fixtures/project_alpha/logs/runtime.log\",\"line_start\":10,\"line_end\":12}'::jsonb,
    'raw_capture',
    2000,
    2000,
    2000,
    'memory-provenance-v1',
    '{\"raw_excerpt\":\"${raw_marker} immutable runtime log excerpt\",\"path\":\"fixtures/project_alpha/logs/runtime.log\"}'::jsonb
  FROM ami.projects p
  JOIN ami.namespaces n ON n.project_id = p.project_id AND n.code = 'review'
  WHERE p.code = 'project_alpha'
"

cargo run --release --quiet -- context pack \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --token-source-kind proof_context_pack \
  --limit-documents 8 \
  --limit-symbols 8 \
  --limit-chunks 8 \
  --limit-semantic-chunks 8 >/tmp/amai-proof-runtime-sufficiency-router-summary.json

jq -e '.decision_trace.evidence_ladder.cheapest_sufficient_layer == "summary_compact"' /tmp/amai-proof-runtime-sufficiency-router-summary.json >/dev/null
jq -e '.retrieval.raw_evidence | length == 0' /tmp/amai-proof-runtime-sufficiency-router-summary.json >/dev/null

cargo run --release --quiet -- context pack \
  --project project_alpha \
  --namespace review \
  --query "${structured_marker}" \
  --retrieval-mode local_plus_related \
  --token-source-kind proof_context_pack \
  --limit-documents 8 \
  --limit-symbols 8 \
  --limit-chunks 8 \
  --limit-semantic-chunks 8 >/tmp/amai-proof-runtime-sufficiency-router-structured.json

jq -e '.decision_trace.evidence_ladder.cheapest_sufficient_layer == "structured_graph"' /tmp/amai-proof-runtime-sufficiency-router-structured.json >/dev/null
jq -e '.retrieval.memory_cards | length == 1' /tmp/amai-proof-runtime-sufficiency-router-structured.json >/dev/null
jq -e '.retrieval.memory_relation_edges | length > 0' /tmp/amai-proof-runtime-sufficiency-router-structured.json >/dev/null
jq -e '.retrieval.raw_evidence | length == 0' /tmp/amai-proof-runtime-sufficiency-router-structured.json >/dev/null

cargo run --release --quiet -- context pack \
  --project project_alpha \
  --namespace review \
  --query "${raw_marker}" \
  --retrieval-mode local_plus_related \
  --token-source-kind proof_context_pack \
  --limit-documents 8 \
  --limit-symbols 8 \
  --limit-chunks 8 \
  --limit-semantic-chunks 8 >/tmp/amai-proof-runtime-sufficiency-router-raw.json

jq -e '.decision_trace.evidence_ladder.cheapest_sufficient_layer == "raw_evidence"' /tmp/amai-proof-runtime-sufficiency-router-raw.json >/dev/null
jq -e '.retrieval.raw_evidence | length > 0' /tmp/amai-proof-runtime-sufficiency-router-raw.json >/dev/null
jq -e '.decision_trace.evidence_ladder.layers[] | select(.layer == "raw_evidence") | .strategies == ["raw_evidence"]' /tmp/amai-proof-runtime-sufficiency-router-raw.json >/dev/null

printf 'proof_runtime_sufficiency_router: PASS\n'
