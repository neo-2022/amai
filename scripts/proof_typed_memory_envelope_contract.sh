#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-Awarnings"
export CARGO_TERM_COLOR=never

tmp_cleanup_paths=()
cleanup_tmp_paths() {
  local path
  for path in "${tmp_cleanup_paths[@]:-}"; do
    [[ -n "${path}" ]] && rm -f "${path}" 2>/dev/null || true
  done
}

run_amai_last_line() {
  local output_file
  output_file="$(mktemp)"
  tmp_cleanup_paths+=("${output_file}")
  cargo run --quiet -- "$@" >"${output_file}" 2>/dev/null
  python3 - "${output_file}" <<'PY'
import pathlib
import sys

lines = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8").splitlines()
for line in reversed(lines):
    if line.strip():
        print(line)
        break
else:
    raise SystemExit("run_amai_last_line: no non-empty output line captured")
PY
}

./scripts/proof_stage2_setup.sh >/tmp/amai-proof-typed-envelope-setup.log
./scripts/typed_memory_envelope_guard.sh --json >/dev/null

observability_snapshot_list="$(run_amai_last_line observe list-snapshots --kind working_state_restore --limit 1 --ids-only)"
observability_snapshot_id="$(python3 - "${observability_snapshot_list}" <<'PY'
import json
import sys
payload = json.loads(sys.argv[1])
assert isinstance(payload, list) and payload, payload
print(payload[0])
PY
)"
test -n "${observability_snapshot_id}"
observability_snapshot_file="$(mktemp)"
tmp_cleanup_paths+=("${observability_snapshot_file}")
trap 'cleanup_tmp_paths' EXIT
cargo run --quiet -- observe get-snapshot --snapshot-id "${observability_snapshot_id}" >"${observability_snapshot_file}" 2>/dev/null
python3 - "${observability_snapshot_file}" "${observability_snapshot_id}" <<'PY'
import json
import sys
payload = json.load(open(sys.argv[1], "r", encoding="utf-8"))
snapshot_id = sys.argv[2]
assert payload["snapshot_id"] == snapshot_id, payload
assert payload["snapshot_kind"] == "working_state_restore", payload
assert isinstance(payload["payload"], dict), payload
PY

suffix="$(date +%s%N)"
workspace_code="proof_stage2_workspace_${suffix}"
project_code="proof_stage2_project_${suffix}"
project_repo_root="/tmp/amai-proof-stage2-project-${suffix}"
agent_code="proof_stage2_agent_${suffix}"
team_code="proof_stage2_team_${suffix}"
role_code="proof_stage2_role_${suffix}"
dsn="${AMI_POSTGRES_DSN}"

cargo run --quiet -- workspace ensure \
  --code "${workspace_code}" \
  --display-name "Proof Stage2 Workspace ${suffix}" >/dev/null

cli_workspace_list_payload="$(run_amai_last_line workspace list --code "${workspace_code}" --json)"
python3 - "${cli_workspace_list_payload}" "${workspace_code}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
workspace_code = sys.argv[2]
suffix = sys.argv[3]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["code"] == workspace_code), None)
assert match is not None, payload
assert match["display_name"] == f"Proof Stage2 Workspace {suffix}", match
assert match["status"] == "active", match
PY

mkdir -p "${project_repo_root}"
cargo run --quiet -- project register \
  --workspace "${workspace_code}" \
  --code "${project_code}" \
  --display-name "Proof Stage2 Project ${suffix}" \
  --repo-root "${project_repo_root}" >/dev/null

cli_project_list_payload="$(run_amai_last_line project list --code "${project_code}" --repo-root "${project_repo_root}" --json)"
python3 - "${cli_project_list_payload}" "${project_code}" "${project_repo_root}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
project_code = sys.argv[2]
project_repo_root = sys.argv[3]
suffix = sys.argv[4]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["code"] == project_code), None)
assert match is not None, payload
assert match["display_name"] == f"Proof Stage2 Project {suffix}", match
assert match["repo_root"] == project_repo_root, match
assert match["visibility_scope"] == "project_shared", match
PY

cargo run --quiet -- team ensure \
  --workspace default \
  --code "${team_code}" \
  --display-name "Proof Stage2 Team ${suffix}" >/dev/null

cargo run --quiet -- role ensure \
  --workspace default \
  --code "${role_code}" \
  --display-name "Proof Stage2 Role ${suffix}" >/dev/null

cargo run --quiet -- agent ensure \
  --workspace default \
  --team "${team_code}" \
  --role "${role_code}" \
  --code "${agent_code}" \
  --display-name "Proof Stage2 Agent ${suffix}" \
  --visibility-scope project_shared >/dev/null

cli_team_list_payload="$(run_amai_last_line team list --workspace default --code "${team_code}" --json)"
python3 - "${cli_team_list_payload}" "${team_code}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
team_code = sys.argv[2]
suffix = sys.argv[3]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["code"] == team_code), None)
assert match is not None, payload
assert match["workspace_code"] == "default", match
assert match["display_name"] == f"Proof Stage2 Team {suffix}", match
assert match["status"] == "active", match
PY

cli_role_list_payload="$(run_amai_last_line role list --workspace default --code "${role_code}" --json)"
python3 - "${cli_role_list_payload}" "${role_code}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
role_code = sys.argv[2]
suffix = sys.argv[3]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["code"] == role_code), None)
assert match is not None, payload
assert match["workspace_code"] == "default", match
assert match["display_name"] == f"Proof Stage2 Role {suffix}", match
assert match["status"] == "active", match
PY

cli_agent_list_payload="$(run_amai_last_line agent list --workspace default --code "${agent_code}" --json)"
python3 - "${cli_agent_list_payload}" "${agent_code}" "${team_code}" "${role_code}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
agent_code = sys.argv[2]
team_code = sys.argv[3]
role_code = sys.argv[4]
suffix = sys.argv[5]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["code"] == agent_code), None)
assert match is not None, payload
assert match["workspace_code"] == "default", match
assert match["team_code"] == team_code, match
assert match["role_code"] == role_code, match
assert match["display_name"] == f"Proof Stage2 Agent {suffix}", match
assert match["visibility_scope"] == "project_shared", match
assert match["status"] == "active", match
PY

memory_item_create_log="$(cargo run --quiet -- memory create-item \
  --project project_alpha \
  --namespace review \
  --owner-agent "${agent_code}" \
  --item-kind fact \
  --identity-key "proof-stage2-${suffix}" \
  --title "proof stage2 fact ${suffix}" \
  --summary "typed envelope summary" \
  --body "typed envelope body" \
  --sensitivity-class confidential \
  --truth-state current \
  --trust-state verified \
  --verification-state verified \
  --lifecycle-state hot \
  --source-event-id "event:${suffix}" \
  --artifact-ref "artifact://proof/stage2/${suffix}" \
  --message-ref "message:${suffix}" \
  --evidence-span-json '{"path":"fixtures/project_alpha/src/lib.rs","line_start":1,"line_end":3}' \
  --derivation-kind extract \
  --observed-at-epoch-ms 1000 \
  --recorded-at-epoch-ms 1005 \
  --valid-from-epoch-ms 1000 \
  --valid-to-epoch-ms 2000 \
  --last-verified-at-epoch-ms 1500 \
  --causation-id "cause-${suffix}" \
  --correlation-id "corr-${suffix}" \
  --utility-score 0.9 \
  --freshness-score 0.8 \
  --retention-class durable \
  --ttl-epoch-ms 60000 \
  --imported-from-json '{"source":"proof","kind":"local"}' \
  --schema-version memory-envelope-v1 \
  --metadata-json '{"proof":"stage2"}' \
  --json)"
memory_item_id="$(python3 - "${memory_item_create_log}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
print(payload["memory_item_id"])
PY
)"

memory_raw_event_payload="$(run_amai_last_line memory get-latest-raw-event --memory-item-id "${memory_item_id}")"
python3 - "${memory_raw_event_payload}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
suffix = sys.argv[2]
assert payload["event_kind"] == "memory_candidate_write", payload
assert payload["item_kind"] == "fact", payload
assert payload["source_event_ids"] == [f"event:{suffix}"], payload
assert payload["artifact_refs"] == [f"artifact://proof/stage2/{suffix}"], payload
assert payload["message_refs"] == [f"message:{suffix}"], payload
assert payload["payload"]["candidate"]["item_kind"] == "fact", payload
PY

memory_write_outbox_payload="$(run_amai_last_line memory list-write-outbox --memory-item-id "${memory_item_id}")"
python3 - "${memory_write_outbox_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
subjects = {row["subject"] for row in payload}
assert len(payload) == 6, payload
assert "ami.index.memory.lexical" in subjects, payload
assert "ami.index.memory.graph" in subjects, payload
assert "ami.index.memory.embedding" in subjects, payload
assert "ami.index.memory.restore_summary" in subjects, payload
assert "ami.event.memory_item.created" in subjects, payload
assert "ami.event.memory_item.invalidate_cache" in subjects, payload
assert all(row["delivery_state"] == "pending" for row in payload), payload
PY

cli_memory_provenance_log="$(cargo run --quiet -- memory create-provenance \
  --project project_alpha \
  --namespace review \
  --memory-item-id "${memory_item_id}" \
  --source-kind proof_contract \
  --source-event-id "event:${suffix}" \
  --trust-level verified \
  --message-ref "message:${suffix}" \
  --evidence-span-json '{"source":"proof","range":"1-3"}' \
  --derivation-kind extract \
  --observed-at-epoch-ms 1000 \
  --recorded-at-epoch-ms 1005 \
  --valid-from-epoch-ms 1000 \
  --valid-to-epoch-ms 2000 \
  --schema-version memory-provenance-v1 \
  --details-json '{"artifact_refs":["artifact://proof/stage2/'"${suffix}"'"]}')"
cli_memory_provenance_id="$(printf '%s\n' "${cli_memory_provenance_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_memory_provenance_id}"

cli_artifact_ref_log="$(cargo run --quiet -- memory create-artifact-ref \
  --project project_alpha \
  --namespace review \
  --artifact-kind log_excerpt \
  --bucket proof-stage2 \
  --object-key "artifact/${suffix}.json" \
  --content-type application/json \
  --source-kind proof_contract \
  --source-event-id "event:artifact:${suffix}" \
  --message-ref "message:artifact:${suffix}" \
  --evidence-span-json '{"kind":"artifact","path":"artifact/'"${suffix}"'.json"}' \
  --derivation-kind extract \
  --schema-version artifact-ref-envelope-v1 \
  --metadata-json '{"proof":"stage2","surface":"artifact_ref"}')"
cli_artifact_ref_id="$(printf '%s\n' "${cli_artifact_ref_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_artifact_ref_id}"

context_pack_payload="$(run_amai_last_line context pack \
  --project project_alpha \
  --namespace review \
  --query "proof stage2 ${suffix}" \
  --retrieval-mode local_strict \
  --limit-documents 2 \
  --limit-symbols 2 \
  --limit-chunks 2 \
  --limit-semantic-chunks 2 \
  --token-source-kind proof_context_pack)"
context_pack_id="$(python3 - "${context_pack_payload}" <<'PY'
import json
import sys
payload = json.loads(sys.argv[1])
print(payload["context_pack_id"])
PY
)"
test -n "${context_pack_id}"

context_pack_record="$(run_amai_last_line context get-pack --context-pack-id "${context_pack_id}")"
python3 - "${context_pack_record}" "${context_pack_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
context_pack_id = sys.argv[2]
assert payload["context_pack_id"] == context_pack_id, payload
assert payload["project_code"] == "project_alpha", payload
assert payload["namespace_code"] == "review", payload
assert payload["retrieval_mode"] == "local_strict", payload
assert payload["query_text"].startswith("proof stage2 "), payload
assert isinstance(payload["payload"], dict), payload
assert payload["artifact_state"] in {"pending", "materializing", "materialized", "failed"}, payload
assert isinstance(payload["artifact_updated_at_epoch_ms"], int), payload
PY

pending_context_pack_artifacts_payload_file="$(mktemp)"
cargo run --quiet -- observe list-pending-context-pack-artifacts --limit 64 --context-pack-id "${context_pack_id}" >"${pending_context_pack_artifacts_payload_file}" 2>/dev/null
python3 - "${pending_context_pack_artifacts_payload_file}" "${context_pack_record}" "${context_pack_id}" <<'PY'
import json
import pathlib
import sys

payload = json.loads(pathlib.Path(sys.argv[1]).read_text())
context_pack_record = json.loads(sys.argv[2])
context_pack_id = sys.argv[3]
assert isinstance(payload, list), payload
for row in payload:
    assert isinstance(row["context_pack_id"], str), row
    assert isinstance(row["project_id"], str), row
    assert isinstance(row["namespace_id"], str), row
    assert isinstance(row["bucket"], str) and row["bucket"], row
    assert isinstance(row["object_key"], str) and row["object_key"], row
    assert isinstance(row["payload"], dict), row

if context_pack_record["artifact_state"] in {"pending", "failed"}:
    assert len(payload) == 1, (context_pack_record, payload)
    assert payload[0]["context_pack_id"] == context_pack_id, (context_pack_record, payload)
else:
    assert payload == [], (context_pack_record, payload)
PY

successor_create_log="$(cargo run --quiet -- memory create-item \
  --project project_alpha \
  --namespace review \
  --item-kind fact \
  --title "proof stage2 successor ${suffix}" \
  --sensitivity-class internal \
  --truth-state current \
  --trust-state proposed \
  --verification-state proposed \
  --lifecycle-state hot \
  --source-event-id "event:successor:${suffix}" \
  --evidence-span-json '{}' \
  --derivation-kind merge \
  --observed-at-epoch-ms 2001 \
  --recorded-at-epoch-ms 2001 \
  --valid-from-epoch-ms 2001 \
  --utility-score 0.5 \
  --freshness-score 0.5 \
  --retention-class standard \
  --imported-from-json '{}' \
  --schema-version memory-envelope-v1 \
  --metadata-json '{}' \
  --json)"
successor_id="$(python3 - "${successor_create_log}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
print(payload["memory_item_id"])
PY
)"

memory_item_update_log="$(cargo run --quiet -- memory update-item \
  --memory-item-id "${memory_item_id}" \
  --superseded-by-memory-item-id "${successor_id}" \
  --json)"
python3 - "${memory_item_update_log}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["object_version"] == 2, payload
PY

cli_conflict_log="$(cargo run --quiet -- memory create-edge \
  --project project_alpha \
  --namespace review \
  --source-memory-item-id "${successor_id}" \
  --target-memory-item-id "${memory_item_id}" \
  --edge-kind conflicts_with \
  --edge-state active \
  --trust-state verified \
  --validity-basis explicit \
  --source-kind proof_contract \
  --source-event-id "event:conflict:${suffix}" \
  --message-ref "message:conflict:${suffix}" \
  --evidence-span-json '{"source":"proof","kind":"conflict","id":"'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version memory-edge-envelope-v1 \
  --evidence-json '{"proof":"stage2"}')"
cli_conflict_id="$(printf '%s\n' "${cli_conflict_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_conflict_id}"

view_payload="$(run_amai_last_line memory get-item --memory-item-id "${memory_item_id}")"

python3 - "${view_payload}" "${memory_item_id}" "${successor_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
memory_item_id = sys.argv[2]
successor_id = sys.argv[3]

assert payload["memory_id"] == memory_item_id, payload
assert payload["memory_type"] == "fact", payload
assert payload["scope_type"] == "project_shared", payload
assert payload["visibility"] == "project_shared", payload
assert payload["truth_state"] == "current", payload
assert payload["trust_state"] == "verified", payload
assert payload["verification_state"] == "verified", payload
assert payload["owner_agent_id"] is not None, payload
assert payload["sensitivity_class"] == "confidential", payload
assert len(payload["source_event_ids"]) == 1, payload
assert payload["source_event_ids"][0].startswith("event:"), payload
assert len(payload["artifact_refs"]) == 1, payload
assert payload["artifact_refs"][0].startswith("artifact://proof/stage2/"), payload
assert len(payload["message_refs"]) == 1, payload
assert payload["message_refs"][0].startswith("message:"), payload
assert payload["evidence_span"]["path"] == "fixtures/project_alpha/src/lib.rs", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "memory-envelope-v1", payload
assert payload["object_version"] == 2, payload
assert payload["ingest_seq"] >= 1, payload
assert payload["utility_score"] == 0.9, payload
assert payload["freshness_score"] == 0.8, payload
assert payload["retention_class"] == "durable", payload
assert payload["ttl"] == 60000, payload
assert payload["imported_from"]["source"] == "proof", payload
assert payload["causation_id"].startswith("cause-"), payload
assert payload["correlation_id"].startswith("corr-"), payload
assert payload["observed_at_epoch_ms"] == 1000, payload
assert payload["recorded_at_epoch_ms"] == 1005, payload
assert payload["valid_from_epoch_ms"] == 1000, payload
assert payload["valid_to_epoch_ms"] == 2000, payload
assert payload["supersedes"] == [], payload
assert payload["conflicts_with"] == [successor_id], payload
PY

successor_payload="$(run_amai_last_line memory get-item --memory-item-id "${successor_id}")"

python3 - "${successor_payload}" "${successor_id}" "${memory_item_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
successor_id = sys.argv[2]
memory_item_id = sys.argv[3]

assert payload["memory_id"] == successor_id, payload
assert payload["memory_type"] == "fact", payload
assert payload["scope_type"] == "project_shared", payload
assert payload["visibility"] == "project_shared", payload
assert payload["truth_state"] == "current", payload
assert payload["trust_state"] == "proposed", payload
assert payload["verification_state"] == "proposed", payload
assert payload["derivation_kind"] == "merge", payload
assert payload["supersedes"] == [memory_item_id], payload
assert payload["conflicts_with"] == [memory_item_id], payload
assert payload["schema_version"] == "memory-envelope-v1", payload
assert payload["object_version"] == 1, payload
assert payload["ingest_seq"] >= 1, payload
PY

successor_update_log="$(cargo run --quiet -- memory update-item \
  --memory-item-id "${successor_id}" \
  --summary 'updated envelope summary')"
printf '%s\n' "${successor_update_log}" | grep -q "version=2"

cli_left_create_log="$(cargo run --quiet -- memory create-item \
  --project project_alpha \
  --namespace review \
  --item-kind fact \
  --identity-key "cli-memory-edge-left-${suffix}" \
  --title "cli memory edge left" \
  --summary "cli memory edge left summary" \
  --body "cli memory edge left body" \
  --truth-state current \
  --trust-state verified \
  --verification-state verified \
  --lifecycle-state hot \
  --source-event-id "event:cli-memory-edge-left:${suffix}" \
  --artifact-ref "artifact://proof/cli-memory-edge-left/${suffix}" \
  --message-ref "message:cli-memory-edge-left:${suffix}" \
  --evidence-span-json '{"kind":"message","id":"cli-memory-edge-left:'"${suffix}"'"}' \
  --derivation-kind extract \
  --observed-at-epoch-ms 3001 \
  --recorded-at-epoch-ms 3001 \
  --utility-score 0.6 \
  --freshness-score 0.6 \
  --retention-class durable \
  --imported-from-json '{}' \
  --schema-version memory-envelope-v1 \
  --metadata-json '{}')"
cli_left_id="$(printf '%s\n' "${cli_left_create_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"

cli_right_create_log="$(cargo run --quiet -- memory create-item \
  --project project_alpha \
  --namespace review \
  --item-kind fact \
  --identity-key "cli-memory-edge-right-${suffix}" \
  --title "cli memory edge right" \
  --summary "cli memory edge right summary" \
  --body "cli memory edge right body" \
  --truth-state current \
  --trust-state verified \
  --verification-state verified \
  --lifecycle-state hot \
  --source-event-id "event:cli-memory-edge-right:${suffix}" \
  --artifact-ref "artifact://proof/cli-memory-edge-right/${suffix}" \
  --message-ref "message:cli-memory-edge-right:${suffix}" \
  --evidence-span-json '{"kind":"message","id":"cli-memory-edge-right:'"${suffix}"'"}' \
  --derivation-kind extract \
  --observed-at-epoch-ms 3002 \
  --recorded-at-epoch-ms 3002 \
  --utility-score 0.6 \
  --freshness-score 0.6 \
  --retention-class durable \
  --imported-from-json '{}' \
  --schema-version memory-envelope-v1 \
  --metadata-json '{}')"
cli_right_id="$(printf '%s\n' "${cli_right_create_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"

cli_edge_create_log="$(cargo run --quiet -- \
  memory create-edge \
  --project project_alpha \
  --namespace review \
  --source-memory-item-id "${cli_left_id}" \
  --target-memory-item-id "${cli_right_id}" \
  --edge-kind supports \
  --edge-state active \
  --trust-state verified \
  --validity-basis explicit \
  --score 0.91 \
  --evidence-json '{"proof":"stage2-cli-edge"}' \
  --source-kind runtime_cli \
  --source-event-id "event:cli-memory-edge:${suffix}" \
  --artifact-ref "artifact://proof/cli-memory-edge/${suffix}" \
  --message-ref "message:cli-memory-edge:${suffix}" \
  --evidence-span-json '{"kind":"message","id":"cli-memory-edge:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version memory-edge-envelope-v1)"
cli_edge_id="$(printf '%s\n' "${cli_edge_create_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"

cli_edge_payload="$(run_amai_last_line memory get-edge --memory-edge-id "${cli_edge_id}")"

python3 - "${cli_edge_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["edge_kind"] == "supports", payload
assert payload["edge_state"] == "active", payload
assert payload["trust_state"] == "verified", payload
assert payload["validity_basis"] == "explicit", payload
assert payload["source_kind"] == "runtime_cli", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "memory-edge-envelope-v1", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"] is True, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
PY

cli_conflict_create_log="$(cargo run --quiet -- \
  memory create-conflict \
  --project project_alpha \
  --namespace review \
  --left-memory-item-id "${cli_left_id}" \
  --right-memory-item-id "${cli_right_id}" \
  --conflict-kind truth \
  --conflict-state open \
  --severity high \
  --summary "cli conflict summary ${suffix}" \
  --evidence-json '{"proof":"stage2-cli-conflict"}' \
  --source-kind runtime_cli \
  --source-event-id "event:cli-memory-conflict:${suffix}" \
  --artifact-ref "artifact://proof/cli-memory-conflict/${suffix}" \
  --message-ref "message:cli-memory-conflict:${suffix}" \
  --evidence-span-json '{"kind":"message","id":"cli-memory-conflict:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version memory-conflict-envelope-v1)"
cli_conflict_id="$(printf '%s\n' "${cli_conflict_create_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"

cli_conflict_payload="$(run_amai_last_line memory get-conflict --memory-conflict-id "${cli_conflict_id}")"

python3 - "${cli_conflict_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["conflict_kind"] == "truth", payload
assert payload["conflict_state"] == "open", payload
assert payload["severity"] == "high", payload
assert payload["source_kind"] == "runtime_cli", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "memory-conflict-envelope-v1", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"] is True, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
PY

cli_task_node_create_log="$(cargo run --quiet -- \
  memory create-task-node \
  --project project_alpha \
  --namespace review \
  --task-key "cli-task-node-${suffix}" \
  --task-role proposal \
  --headline "cli task node" \
  --summary "cli task node summary" \
  --next-step "cli task node next" \
  --execution-state active \
  --lifecycle-state hot \
  --confidence 1.0 \
  --reopened-count 0 \
  --child-count 0 \
  --closed-child-count 0 \
  --pending-return-count 0 \
  --source-event-id "event:cli-task-node:${suffix}" \
  --artifact-ref "artifact://proof/cli-task-node/${suffix}" \
  --evidence-span-json '{"kind":"task_node","id":"cli-task-node:'"${suffix}"'"}' \
  --derivation-kind extract \
  --status-payload-json '{"source_kind":"continuity_handoff"}' \
  --metadata-json '{}' \
  --opened-at-epoch-ms 4001)"
cli_task_node_id="$(printf '%s\n' "${cli_task_node_create_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_task_node_id}"

cli_task_node_payload="$(run_amai_last_line memory get-task-node --task-node-id "${cli_task_node_id}")"
python3 - "${cli_task_node_payload}" "${cli_task_node_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
task_node_id = sys.argv[2]
assert payload["task_node_id"] == task_node_id, payload
assert payload["task_key"].startswith("cli-task-node-"), payload
assert payload["task_role"] == "proposal", payload
assert payload["execution_state"] == "active", payload
assert payload["lifecycle_state"] == "hot", payload
assert payload["candidate_class"] == "commitment", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["source_kind"] == "continuity_handoff", payload
assert payload["hot_path_write_eligible"] is True, payload
assert payload["background_consolidation_recommended"] is False, payload
PY

cli_task_event_log="$(cargo run --quiet -- \
  memory create-task-event \
  --project project_alpha \
  --namespace review \
  --task-node-id "${cli_task_node_id}" \
  --source-event-id "event:cli-task-event:${suffix}" \
  --event-kind state_transition \
  --prior-execution-state proposed \
  --next-execution-state active \
  --prior-lifecycle-state hot \
  --next-lifecycle-state hot \
  --source-kind continuity_handoff \
  --artifact-ref "artifact://proof/cli-task-event/${suffix}" \
  --message-ref "message:cli-task-event:${suffix}" \
  --evidence-span-json '{"kind":"task_event","id":"cli-task-event:'"${suffix}"'"}' \
  --derivation-kind raw_capture \
  --schema-version task-event-envelope-v1 \
  --event-payload-json '{"source_kind":"continuity_handoff","transition":"proposal_to_active"}' \
  --recorded-at-epoch-ms 4002)"

cli_task_event_id="$(printf '%s\n' "${cli_task_event_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_task_event_id}"

cli_task_event_payload="$(run_amai_last_line memory get-task-event --task-event-id "${cli_task_event_id}")"
python3 - "${cli_task_event_payload}" "${cli_task_node_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
task_node_id = sys.argv[2]
assert payload["task_node_id"] == task_node_id, payload
assert payload["event_kind"] == "state_change", payload
assert payload["source_event_id"].startswith("event:cli-task-event:"), payload
assert payload["source_kind"] == "continuity_handoff", payload
assert payload["derivation_kind"] == "raw_capture", payload
assert payload["schema_version"] == "task-event-envelope-v1", payload
assert payload["artifact_refs"] == [f"artifact://proof/cli-task-event/{payload['source_event_id'].split(':')[-1]}"], payload
assert payload["message_refs"] == [f"message:cli-task-event:{payload['source_event_id'].split(':')[-1]}"], payload
assert payload["evidence_span"]["kind"] == "task_event", payload
PY

cli_retrieval_trace_log="$(cargo run --quiet -- \
  memory create-retrieval-trace \
  --workspace default \
  --project project_alpha \
  --namespace review \
  --query-text "cli retrieval trace ${suffix}" \
  --requested-mode lexical \
  --effective-mode graph \
  --scope-filter-json '{"mode":"project_strict"}' \
  --candidate-summary-json '{"summary_layer":1,"structured_layer":2}' \
  --rerank-summary-json '{"legal":true,"relevance":0.93}' \
  --evidence-sufficiency-json '{"summary_sufficient":false,"structured_sufficient":true,"cheapest_sufficient_layer":"structured_graph_neighborhood"}' \
  --source-kind runtime_cli \
  --source-event-id "event:cli-retrieval-trace:${suffix}" \
  --artifact-ref "artifact://proof/cli-retrieval-trace/${suffix}" \
  --message-ref "message:cli-retrieval-trace:${suffix}" \
  --evidence-span-json '{"kind":"retrieval_trace","layer":"structured_graph_neighborhood","id":"cli-retrieval-trace:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version retrieval-trace-envelope-v1 \
  --final-decision continue \
  --temporal-query-epoch-ms 4003 \
  --trace-payload-json '{"decision_trace":{"cheapest_sufficient_layer":"structured_graph_neighborhood","escalate_if_needed":true}}')"
cli_retrieval_trace_id="$(printf '%s\n' "${cli_retrieval_trace_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_retrieval_trace_id}"

cli_retrieval_trace_payload="$(run_amai_last_line memory get-retrieval-trace --retrieval-trace-id "${cli_retrieval_trace_id}")"
python3 - "${cli_retrieval_trace_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["query_text"].startswith("cli retrieval trace "), payload
assert payload["requested_mode"] == "lexical", payload
assert payload["effective_mode"] == "graph", payload
assert payload["evidence_sufficiency"]["cheapest_sufficient_layer"] == "structured_graph_neighborhood", payload
assert payload["source_kind"] == "runtime_cli", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "retrieval-trace-envelope-v1", payload
assert payload["final_decision"] == "continue", payload
assert payload["trace_payload"]["decision_trace"]["cheapest_sufficient_layer"] == "structured_graph_neighborhood", payload
PY

cli_skill_candidate_log="$(cargo run --quiet -- \
  skill create-candidate \
  --project project_alpha \
  --namespace review \
  --skill-id "cli-skill-${suffix}" \
  --title "cli skill ${suffix}" \
  --goal "prove evidence bundle runtime" \
  --trigger-condition "when task evidence appears" \
  --execution-step "collect recorded basis" \
  --source-event-id "event:cli-skill:${suffix}" \
  --artifact-ref "artifact://proof/cli-skill/${suffix}" \
  --evidence-span-json '{"kind":"skill_card","id":"cli-skill:'"${suffix}"'"}' \
  --derivation-kind extract)"
cli_skill_card_id="$(printf '%s\n' "${cli_skill_candidate_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_skill_card_id}"

cli_skill_evidence_log="$(cargo run --quiet -- \
  skill add-evidence \
  --skill-card-id "${cli_skill_card_id}" \
  --evidence-kind episode_success \
  --summary "cli skill evidence ${suffix}" \
  --source-kind runtime_cli \
  --source-event-id "event:cli-skill-evidence:${suffix}" \
  --artifact-ref "artifact://proof/cli-skill-evidence/${suffix}" \
  --message-ref "message:cli-skill-evidence:${suffix}" \
  --evidence-span-json '{"kind":"skill_evidence_bundle","id":"cli-skill-evidence:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version skill-evidence-bundle-envelope-v1)"
cli_skill_evidence_bundle_id="$(printf '%s\n' "${cli_skill_evidence_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_skill_evidence_bundle_id}"

cli_skill_evidence_payload="$(run_amai_last_line skill get-evidence --skill-evidence-bundle-id "${cli_skill_evidence_bundle_id}")"
python3 - "${cli_skill_evidence_payload}" "${cli_skill_card_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
skill_card_id = sys.argv[2]
assert payload["skill_card_id"] == skill_card_id, payload
assert payload["evidence_kind"] == "episode_success", payload
assert payload["source_kind"] == "runtime_cli", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "skill-evidence-bundle-envelope-v1", payload
assert payload["message_refs"][0].startswith("message:cli-skill-evidence:"), payload
assert payload["evidence_span"]["kind"] == "skill_evidence_bundle", payload
PY

cli_skill_trigger_log="$(cargo run --quiet -- \
  skill record-trigger-match \
  --skill-card-id "${cli_skill_card_id}" \
  --match-scope thread \
  --trigger-input "trigger ${suffix}" \
  --matched \
  --summary "trigger summary ${suffix}" \
  --source-kind skill_trigger_scan \
  --source-event-id "event:cli-skill-trigger:${suffix}" \
  --artifact-ref "artifact://proof/cli-skill-trigger/${suffix}" \
  --message-ref "message:cli-skill-trigger:${suffix}" \
  --evidence-span-json '{"kind":"skill_trigger_match","id":"cli-skill-trigger:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version skill-trigger-match-envelope-v1)"
cli_skill_trigger_id="$(printf '%s\n' "${cli_skill_trigger_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_skill_trigger_id}"

cli_skill_trigger_payload="$(run_amai_last_line skill get-trigger-match --skill-trigger-match-id "${cli_skill_trigger_id}")"
python3 - "${cli_skill_trigger_payload}" "${cli_skill_card_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
skill_card_id = sys.argv[2]
assert payload["skill_card_id"] == skill_card_id, payload
assert payload["match_scope"] == "thread", payload
assert payload["matched"] is True, payload
assert payload["source_kind"] == "skill_trigger_scan", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "skill-trigger-match-envelope-v1", payload
assert payload["evidence_span"]["kind"] == "skill_trigger_match", payload
PY

cli_skill_trial_log="$(cargo run --quiet -- \
  skill record-trial-run \
  --skill-card-id "${cli_skill_card_id}" \
  --application-mode shadow \
  --task-label "task ${suffix}" \
  --context "continuity" \
  --runtime codex \
  --model gpt-5 \
  --tool rg \
  --matched \
  --outcome success \
  --summary "trial summary ${suffix}" \
  --source-kind skill_trial_runtime \
  --source-event-id "event:cli-skill-trial:${suffix}" \
  --artifact-ref "artifact://proof/cli-skill-trial/${suffix}" \
  --message-ref "message:cli-skill-trial:${suffix}" \
  --evidence-span-json '{"kind":"skill_trial_run","id":"cli-skill-trial:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version skill-trial-run-envelope-v1)"
cli_skill_trial_id="$(printf '%s\n' "${cli_skill_trial_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_skill_trial_id}"

cli_skill_trial_payload="$(run_amai_last_line skill get-trial-run --skill-trial-run-id "${cli_skill_trial_id}")"
python3 - "${cli_skill_trial_payload}" "${cli_skill_card_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
skill_card_id = sys.argv[2]
assert payload["skill_card_id"] == skill_card_id, payload
assert payload["application_mode"] == "shadow", payload
assert payload["matched"] is True, payload
assert payload["applied"] is False, payload
assert payload["outcome"] == "success", payload
assert payload["source_kind"] == "skill_trial_runtime", payload
assert payload["schema_version"] == "skill-trial-run-envelope-v1", payload
assert payload["evidence_span"]["kind"] == "skill_trial_run", payload
PY

cli_skill_eval_log="$(cargo run --quiet -- \
  skill record-eval \
  --skill-card-id "${cli_skill_card_id}" \
  --verdict promote_shadow \
  --evaluator-source eval_contour \
  --safe-to-apply \
  --quality-ok \
  --truth-ok \
  --utility-delta 0.5 \
  --summary "eval summary ${suffix}" \
  --source-kind skill_eval_contour \
  --source-event-id "event:cli-skill-eval:${suffix}" \
  --artifact-ref "artifact://proof/cli-skill-eval/${suffix}" \
  --message-ref "message:cli-skill-eval:${suffix}" \
  --evidence-span-json '{"kind":"skill_eval","id":"cli-skill-eval:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version skill-eval-envelope-v1)"
cli_skill_eval_id="$(printf '%s\n' "${cli_skill_eval_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_skill_eval_id}"

cli_skill_eval_payload="$(run_amai_last_line skill get-eval --skill-eval-id "${cli_skill_eval_id}")"
python3 - "${cli_skill_eval_payload}" "${cli_skill_card_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
skill_card_id = sys.argv[2]
assert payload["skill_card_id"] == skill_card_id, payload
assert payload["verdict"] == "promote_shadow", payload
assert payload["safe_to_apply"] is True, payload
assert payload["quality_ok"] is True, payload
assert payload["truth_ok"] is True, payload
assert payload["source_kind"] == "skill_eval_contour", payload
assert payload["schema_version"] == "skill-eval-envelope-v1", payload
assert payload["evidence_span"]["kind"] == "skill_eval", payload
PY

cli_skill_reuse_log="$(cargo run --quiet -- \
  skill record-reuse \
  --skill-card-id "${cli_skill_card_id}" \
  --reuse-mode shadow \
  --task-label "task ${suffix}" \
  --context "continuity" \
  --matched \
  --applied \
  --outcome success \
  --summary "reuse summary ${suffix}" \
  --source-kind skill_reuse_runtime \
  --source-event-id "event:cli-skill-reuse:${suffix}" \
  --artifact-ref "artifact://proof/cli-skill-reuse/${suffix}" \
  --message-ref "message:cli-skill-reuse:${suffix}" \
  --evidence-span-json '{"kind":"skill_reuse_log","id":"cli-skill-reuse:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version skill-reuse-log-envelope-v1)"
cli_skill_reuse_id="$(printf '%s\n' "${cli_skill_reuse_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_skill_reuse_id}"

cli_skill_reuse_payload="$(run_amai_last_line skill get-reuse --skill-reuse-log-id "${cli_skill_reuse_id}")"
python3 - "${cli_skill_reuse_payload}" "${cli_skill_card_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
skill_card_id = sys.argv[2]
assert payload["skill_card_id"] == skill_card_id, payload
assert payload["reuse_mode"] == "shadow", payload
assert payload["outcome"] == "success", payload
assert payload["source_kind"] == "skill_reuse_runtime", payload
assert payload["schema_version"] == "skill-reuse-log-envelope-v1", payload
assert payload["evidence_span"]["kind"] == "skill_reuse_log", payload
PY

cli_skill_list_payload="$(run_amai_last_line skill list --project project_alpha --namespace review --skill-id "cli-skill-${suffix}" --json)"
python3 - "${cli_skill_list_payload}" "${cli_skill_card_id}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
skill_card_id = sys.argv[2]
suffix = sys.argv[3]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["skill_card_id"] == skill_card_id), None)
assert match is not None, payload
assert match["project_code"] == "project_alpha", match
assert match["namespace_code"] == "review", match
assert match["skill_id"] == f"cli-skill-{suffix}", match
assert match["skill_candidate_class"] == "skill_hint", match
assert match["skill_derivation_kind"] == "extract", match
assert match["skill_source_kind"] == "raw_event_append", match
assert match["skill_hot_path_write_eligible"] is False, match
assert match["skill_background_consolidation_recommended"] is True, match
assert match["skill_evidence_span"]["kind"] == "skill_card", match
PY

cli_import_packet_log="$(cargo run --quiet -- \
  import-packet create \
  --source-project project_alpha \
  --target-project project_beta \
  --status borrowed_unverified \
  --summary "cli import packet ${suffix}" \
  --reason "proof ${suffix}" \
  --imported-by-agent-scope imported \
  --trust-state proposed \
  --verification-state unverified \
  --borrowed-status borrowed \
  --memory-object-id "memory:${suffix}" \
  --artifact-ref "artifact://proof/import-packet/${suffix}" \
  --source-kind runtime_cli \
  --source-event-id "event:import-packet:${suffix}" \
  --message-ref "message:import-packet:${suffix}" \
  --evidence-span-json '{"kind":"import_packet","id":"cli-import-packet:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version import-packet-envelope-v1 \
  --json)"
cli_import_packet_id="$(python3 - "${cli_import_packet_log}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
print(payload["import_packet_id"])
PY
)"
test -n "${cli_import_packet_id}"

cli_import_packet_payload="$(run_amai_last_line import-packet get --import-packet-id "${cli_import_packet_id}")"
python3 - "${cli_import_packet_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["source_project_code"] == "project_alpha", payload
assert payload["target_project_code"] == "project_beta", payload
assert payload["source_kind"] == "runtime_cli", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "import-packet-envelope-v1", payload
assert payload["evidence_span"]["kind"] == "import_packet", payload
PY

cli_shared_asset_log="$(cargo run --quiet -- \
  shared-asset ensure \
  --workspace default \
  --code "cli-shared-asset-${suffix}" \
  --display-name "cli shared asset ${suffix}" \
  --asset-kind document \
  --source-project project_alpha \
  --visibility-scope project_shared \
  --status active \
  --source-kind runtime_cli \
  --source-event-id "event:shared-asset:${suffix}" \
  --artifact-ref "artifact://proof/shared-asset/${suffix}" \
  --message-ref "message:shared-asset:${suffix}" \
  --evidence-span-json '{"kind":"shared_asset","id":"cli-shared-asset:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version shared-asset-envelope-v1 \
  --json)"
cli_shared_asset_id="$(python3 - "${cli_shared_asset_log}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
print(payload["shared_asset_id"])
PY
)"
test -n "${cli_shared_asset_id}"

cli_shared_asset_payload="$(run_amai_last_line shared-asset get --shared-asset-id "${cli_shared_asset_id}")"
python3 - "${cli_shared_asset_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["workspace_code"] == "default", payload
assert payload["source_project_code"] == "project_alpha", payload
assert payload["source_kind"] == "runtime_cli", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "shared-asset-envelope-v1", payload
assert payload["evidence_span"]["kind"] == "shared_asset", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["transfer_policy_required"] is False, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
PY

cli_transfer_policy_log="$(cargo run --quiet -- \
  transfer-policy ensure \
  --workspace default \
  --code "cli-transfer-policy-${suffix}" \
  --display-name "cli transfer policy ${suffix}" \
  --default-decision verified_writeback \
  --allow-cross-project-read \
  --allow-import \
  --allow-verified-writeback)"
cli_transfer_policy_id="$(printf '%s\n' "${cli_transfer_policy_log}" | awk -F' :: ' 'NF>=2{print $2}' | tail -n1)"
test -n "${cli_transfer_policy_id}"

cli_transfer_policy_payload="$(run_amai_last_line transfer-policy get --transfer-policy-id "${cli_transfer_policy_id}")"
python3 - "${cli_transfer_policy_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["workspace_code"] == "default", payload
assert payload["default_decision"] == "verified_writeback", payload
assert payload["allow_cross_project_read"] is True, payload
assert payload["allow_import"] is True, payload
assert payload["allow_verified_writeback"] is True, payload
assert payload["requires_human_approval"] is True, payload
PY

cli_transfer_policy_list_payload="$(run_amai_last_line transfer-policy list --workspace default --code "cli-transfer-policy-${suffix}" --json)"
python3 - "${cli_transfer_policy_list_payload}" "${cli_transfer_policy_id}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
policy_id = sys.argv[2]
suffix = sys.argv[3]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["transfer_policy_id"] == policy_id), None)
assert match is not None, payload
assert match["workspace_code"] == "default", match
assert match["code"] == f"cli-transfer-policy-{suffix}", match
assert match["default_decision"] == "verified_writeback", match
assert match["allow_cross_project_read"] is True, match
assert match["allow_import"] is True, match
assert match["allow_verified_writeback"] is True, match
assert match["requires_human_approval"] is True, match
PY

cli_access_policy_log="$(cargo run --quiet -- \
  access-policy ensure \
  --workspace default \
  --code "cli-access-policy-${suffix}" \
  --display-name "cli access policy ${suffix}" \
  --object-class fact \
  --scope-type project_shared \
  --precedence 42 \
  --can-read \
  --can-import \
  --status active)"
cli_access_policy_id="$(printf '%s\n' "${cli_access_policy_log}" | awk -F' :: ' 'NF>=2{print $2}' | tail -n1)"
test -n "${cli_access_policy_id}"

cli_access_policy_payload="$(run_amai_last_line access-policy get --access-policy-id "${cli_access_policy_id}")"
python3 - "${cli_access_policy_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["workspace_code"] == "default", payload
assert payload["object_class"] == "fact", payload
assert payload["scope_type"] == "project_shared", payload
assert payload["precedence"] == 42, payload
assert payload["can_read"] is True, payload
assert payload["can_import"] is True, payload
assert payload["status"] == "active", payload
PY

cli_access_policy_list_payload="$(run_amai_last_line access-policy list --workspace default --code "cli-access-policy-${suffix}" --json)"
python3 - "${cli_access_policy_list_payload}" "${cli_access_policy_id}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
policy_id = sys.argv[2]
suffix = sys.argv[3]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["access_policy_id"] == policy_id), None)
assert match is not None, payload
assert match["workspace_code"] == "default", match
assert match["code"] == f"cli-access-policy-{suffix}", match
assert match["object_class"] == "fact", match
assert match["scope_type"] == "project_shared", match
assert match["precedence"] == 42, match
assert match["can_read"] is True, match
assert match["can_import"] is True, match
assert match["status"] == "active", match
PY

cli_link_decision_log="$(cargo run --quiet -- \
  memory create-link-decision \
  --project project_alpha \
  --namespace review \
  --task-node-id "${cli_task_node_id}" \
  --candidate-task-node-id "${cli_task_node_id}" \
  --decision-outcome continue \
  --legality-passed \
  --scope-filter-passed \
  --evidence-sufficient \
  --classifier-label continue_existing_branch \
  --classifier-score 0.99 \
  --decision-reason "cli decision reason ${suffix}" \
  --decision-payload-json '{"routing":"continue","evidence":"sufficient"}' \
  --source-event-id "event:cli-link-decision:${suffix}" \
  --artifact-ref "artifact://proof/cli-link-decision/${suffix}" \
  --message-ref "message:cli-link-decision:${suffix}" \
  --evidence-span-json '{"kind":"routing_decision","id":"cli-link-decision:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version memory-link-decision-envelope-v1 \
  --recorded-at-epoch-ms 4002)"
cli_link_decision_id="$(printf '%s\n' "${cli_link_decision_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"

cli_decision_payload="$(run_amai_last_line memory get-link-decision --memory-link-decision-id "${cli_link_decision_id}")"

python3 - "${cli_decision_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["decision_outcome"] == "continue", payload
assert payload["legality_passed"] is True, payload
assert payload["scope_filter_passed"] is True, payload
assert payload["evidence_sufficient"] is True, payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "memory-link-decision-envelope-v1", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"] is True, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
PY

cli_pending_proposal_log="$(cargo run --quiet -- \
  memory create-pending-link-proposal \
  --project project_alpha \
  --namespace review \
  --task-node-id "${cli_task_node_id}" \
  --candidate-task-node-id "${cli_task_node_id}" \
  --proposal-state pending \
  --proposal-reason "cli pending proposal ${suffix}" \
  --evidence-request "need more raw evidence" \
  --evidence-payload-json '{"needed":["more_logs"],"routing":"pending"}' \
  --classifier-score 0.51 \
  --ttl-epoch-ms 4999 \
  --source-event-id "event:cli-pending-proposal:${suffix}" \
  --artifact-ref "artifact://proof/cli-pending-proposal/${suffix}" \
  --message-ref "message:cli-pending-proposal:${suffix}" \
  --evidence-span-json '{"kind":"pending_link_proposal","id":"cli-pending-proposal:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version pending-link-proposal-envelope-v1)"
cli_pending_proposal_id="$(printf '%s\n' "${cli_pending_proposal_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"

cli_proposal_payload="$(run_amai_last_line memory get-pending-link-proposal --pending-link-proposal-id "${cli_pending_proposal_id}")"

python3 - "${cli_proposal_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["proposal_state"] == "pending", payload
assert payload["proposal_reason"].startswith("cli pending proposal "), payload
assert payload["ttl_epoch_ms"] == 4999, payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "pending-link-proposal-envelope-v1", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"] is True, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
PY

cli_relation_source_card_log="$(cargo run --quiet -- \
  memory create-card \
  --project project_alpha \
  --namespace review \
  --title "cli relation source ${suffix}" \
  --summary "cli relation source summary" \
  --body "cli relation source body" \
  --provenance-json '{"source_event_ids":["event:cli-relation-source:${suffix}"],"artifact_refs":["artifact://proof/cli-relation-source/'"${suffix}"'"],"message_refs":["message:cli-relation-source:'"${suffix}"'"],"evidence_span":{"kind":"memory_card","id":"cli-relation-source:'"${suffix}"'"}}' \
  --fact-subject "cli-relation-source-${suffix}" \
  --fact-predicate supports \
  --fact-object "cli-relation-target-${suffix}" \
  --truth-state current \
  --verification-state verified \
  --status active \
  --observed-at-epoch-ms 5001 \
  --recorded-at-epoch-ms 5001 \
  --valid-from-epoch-ms 5001 \
  --last-verified-at-epoch-ms 5002)"
cli_relation_source_card_id="$(printf '%s\n' "${cli_relation_source_card_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_relation_source_card_id}"

cli_relation_target_card_log="$(cargo run --quiet -- \
  memory create-card \
  --project project_alpha \
  --namespace review \
  --title "cli relation target ${suffix}" \
  --summary "cli relation target summary" \
  --body "cli relation target body" \
  --provenance-json '{"source_event_ids":["event:cli-relation-target:${suffix}"],"artifact_refs":["artifact://proof/cli-relation-target/'"${suffix}"'"],"message_refs":["message:cli-relation-target:'"${suffix}"'"],"evidence_span":{"kind":"memory_card","id":"cli-relation-target:'"${suffix}"'"}}' \
  --fact-subject "cli-relation-target-${suffix}" \
  --fact-predicate supported_by \
  --fact-object "cli-relation-source-${suffix}" \
  --truth-state current \
  --verification-state verified \
  --status active \
  --observed-at-epoch-ms 5001 \
  --recorded-at-epoch-ms 5001 \
  --valid-from-epoch-ms 5001 \
  --last-verified-at-epoch-ms 5002)"
cli_relation_target_card_id="$(printf '%s\n' "${cli_relation_target_card_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_relation_target_card_id}"

cli_relation_edge_log="$(cargo run --quiet -- \
  memory create-relation-edge \
  --project project_alpha \
  --namespace review \
  --source-memory-card-id "${cli_relation_source_card_id}" \
  --target-memory-card-id "${cli_relation_target_card_id}" \
  --relation-type supports \
  --relation-state active \
  --evidence-json '{"proof":"stage2-cli-relation-edge"}' \
  --source-kind relation_graph_extract \
  --source-event-id "event:cli-relation-edge:${suffix}" \
  --artifact-ref "artifact://proof/cli-relation-edge/${suffix}" \
  --message-ref "message:cli-relation-edge:${suffix}" \
  --evidence-span-json '{"kind":"memory_relation_edge","id":"cli-relation-edge:'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version memory-relation-edge-envelope-v1 \
  --recorded-at-epoch-ms 5003 \
  --valid-from-epoch-ms 5003)"
cli_relation_edge_id="$(printf '%s\n' "${cli_relation_edge_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"

cli_relation_payload="$(run_amai_last_line memory get-relation-edge --memory-relation-edge-id "${cli_relation_edge_id}")"

python3 - "${cli_relation_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["relation_type"] == "supports", payload
assert payload["relation_state"] == "active", payload
assert payload["source_kind"] == "relation_graph_extract", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "memory-relation-edge-envelope-v1", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"] is True, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
PY

cli_relation_list_payload="$(run_amai_last_line memory list-relation-edges \
  --project project_alpha \
  --namespace review \
  --memory-card-id "${cli_relation_source_card_id}" \
  --memory-card-id "${cli_relation_target_card_id}" \
  --at-epoch-ms 5003 \
  --limit 8)"
python3 - "${cli_relation_list_payload}" "${cli_relation_edge_id}" "${cli_relation_source_card_id}" "${cli_relation_target_card_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
relation_edge_id = sys.argv[2]
source_card_id = sys.argv[3]
target_card_id = sys.argv[4]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["memory_relation_edge_id"] == relation_edge_id), None)
assert match is not None, payload
assert match["source_memory_card_id"] == source_card_id, match
assert match["target_memory_card_id"] == target_card_id, match
assert match["relation_type"] == "supports", match
assert match["relation_state"] == "active", match
PY

card_provenance="$(cat <<JSON
{"source_event_ids":["event:cli-card:${suffix}"],"artifact_refs":["artifact://proof/cli-card/${suffix}"],"message_refs":["message:cli-card:${suffix}"],"evidence_span":{"kind":"memory_card","id":"cli-card:${suffix}"},"derivation_kind":"extract"}
JSON
)"

cli_card_create_log="$(cargo run --quiet -- \
  memory create-card \
  --project project_alpha \
  --namespace review \
  --title "cli card ${suffix}" \
  --summary "cli card summary ${suffix}" \
  --body "cli card body ${suffix}" \
  --tag cli \
  --tag stage2 \
  --provenance-json "${card_provenance}" \
  --fact-subject "cli-card-subject-${suffix}" \
  --fact-predicate states \
  --fact-object "cli-card-object-${suffix}" \
  --truth-state current \
  --verification-state verified \
  --status active \
  --observed-at-epoch-ms 5100 \
  --recorded-at-epoch-ms 5101 \
  --valid-from-epoch-ms 5100 \
  --last-verified-at-epoch-ms 5102)"
cli_card_id="$(printf '%s\n' "${cli_card_create_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_card_id}"

cli_card_payload="$(run_amai_last_line memory get-card --memory-card-id "${cli_card_id}")"
python3 - "${cli_card_payload}" "${cli_card_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
card_id = sys.argv[2]
assert payload["memory_card_id"] == card_id, payload
assert payload["candidate_class"] == "fact", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["truth_state"] == "current", payload
assert payload["verification_state"] == "verified", payload
assert payload["status"] == "active", payload
assert payload["source_kind"] == "raw_event_append", payload
PY

cli_card_list_payload="$(run_amai_last_line \
  memory list-cards \
  --project project_alpha \
  --namespace review \
  --truth-state current \
  --status active)"
cli_card_list_payload_file="$(mktemp)"
printf '%s\n' "${cli_card_list_payload}" > "${cli_card_list_payload_file}"
python3 - "${cli_card_list_payload_file}" "${cli_card_id}" <<'PY'
import json
import pathlib
import sys

payload = json.loads(pathlib.Path(sys.argv[1]).read_text())
card_id = sys.argv[2]
assert any(card["memory_card_id"] == card_id for card in payload), payload
PY
rm -f "${cli_card_list_payload_file}"

cargo run --quiet -- \
  memory update-card-truth-state \
  --memory-card-id "${cli_card_id}" \
  --truth-state conflicted \
  --verification-state disputed \
  --status archived \
  --last-verified-at-epoch-ms 5200 >/tmp/amai-proof-stage2-cli-card-state-update.log

cli_card_updated_payload="$(run_amai_last_line memory get-card --memory-card-id "${cli_card_id}")"
python3 - "${cli_card_updated_payload}" "${cli_card_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
card_id = sys.argv[2]
assert payload["memory_card_id"] == card_id, payload
assert payload["truth_state"] == "conflicted", payload
assert payload["verification_state"] == "disputed", payload
assert payload["status"] == "archived", payload
assert payload["last_verified_at_epoch_ms"] == 5200, payload
PY

set +e
invalid_apply_output="$(cargo run --quiet -- \
  memory apply-card-update \
  --project project_alpha \
  --namespace review \
  --title "cli invalid card ${suffix}" \
  --summary "cli invalid card summary ${suffix}" \
  --body "cli invalid card body ${suffix}" \
  --tag cli \
  --provenance-json "${card_provenance}" \
  --truth-state stale \
  --verification-state verified \
  --status active 2>&1)"
invalid_apply_status=$?
set -e
test ${invalid_apply_status} -ne 0
printf '%s' "${invalid_apply_output}" | grep -F "invalid memory card truth_state 'stale' for memory apply-card-update" >/dev/null

cli_card_update_log="$(cargo run --quiet -- \
  memory apply-card-update \
  --project project_alpha \
  --namespace review \
  --title "cli updated card ${suffix}" \
  --summary "cli updated card summary ${suffix}" \
  --body "cli updated card body ${suffix}" \
  --tag cli \
  --tag updated \
  --provenance-json "${card_provenance}" \
  --fact-subject "cli-updated-card-subject-${suffix}" \
  --fact-predicate states \
  --fact-object "cli-updated-card-object-${suffix}" \
  --truth-state current \
  --verification-state verified \
  --status active \
  --observed-at-epoch-ms 5300 \
  --recorded-at-epoch-ms 5301 \
  --valid-from-epoch-ms 5300 \
  --last-verified-at-epoch-ms 5302)"
cli_card_update_id="$(printf '%s\n' "${cli_card_update_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_card_update_id}"

cli_card_update_payload="$(run_amai_last_line memory get-card --memory-card-id "${cli_card_update_id}")"
python3 - "${cli_card_update_payload}" "${cli_card_update_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
card_id = sys.argv[2]
assert payload["memory_card_id"] == card_id, payload
assert payload["truth_state"] == "current", payload
assert payload["verification_state"] == "verified", payload
assert payload["status"] == "active", payload
assert payload["candidate_class"] == "fact", payload
PY

supersede_source_log="$(cargo run --quiet -- \
  memory create-card \
  --project project_alpha \
  --namespace review \
  --title "cli supersede source ${suffix}" \
  --summary "cli supersede source summary ${suffix}" \
  --body "cli supersede source body ${suffix}" \
  --tag cli \
  --provenance-json "${card_provenance}" \
  --fact-subject "cli-supersede-source-${suffix}" \
  --fact-predicate states \
  --fact-object "cli-supersede-target-${suffix}" \
  --truth-state current \
  --verification-state verified \
  --status active)"
supersede_source_id="$(printf '%s\n' "${supersede_source_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${supersede_source_id}"

supersede_target_log="$(cargo run --quiet -- \
  memory create-card \
  --project project_alpha \
  --namespace review \
  --title "cli supersede target ${suffix}" \
  --summary "cli supersede target summary ${suffix}" \
  --body "cli supersede target body ${suffix}" \
  --tag cli \
  --provenance-json "${card_provenance}" \
  --fact-subject "cli-supersede-target-${suffix}" \
  --fact-predicate states \
  --fact-object "cli-supersede-source-${suffix}" \
  --truth-state current \
  --verification-state verified \
  --status active)"
supersede_target_id="$(printf '%s\n' "${supersede_target_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${supersede_target_id}"

cargo run --quiet -- \
  memory supersede-card \
  --memory-card-id "${supersede_source_id}" \
  --superseded-by "${supersede_target_id}" \
  --valid-to-epoch-ms 5400 \
  --last-verified-at-epoch-ms 5401 >/tmp/amai-proof-stage2-cli-card-supersede.log

superseded_payload="$(run_amai_last_line memory get-card --memory-card-id "${supersede_source_id}")"
python3 - "${superseded_payload}" "${supersede_target_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
successor_id = sys.argv[2]
assert payload["truth_state"] == "superseded", payload
assert payload["status"] == "superseded", payload
assert payload["superseded_by_memory_card_id"] == successor_id, payload
assert payload["valid_to_epoch_ms"] is not None, payload
assert payload["valid_to_epoch_ms"] >= payload["valid_from_epoch_ms"], payload
assert payload["last_verified_at_epoch_ms"] == 5401, payload
PY

retracted_decision_log="$(cargo run --quiet -- \
  memory create-card \
  --project project_alpha \
  --namespace review \
  --title "decision rollback ${suffix}" \
  --summary "retracted decision summary ${suffix}" \
  --body "retracted decision body ${suffix}" \
  --tag decision \
  --provenance-json "${card_provenance}" \
  --truth-state retracted \
  --verification-state disputed \
  --status archived \
  --observed-at-epoch-ms 5500 \
  --recorded-at-epoch-ms 5501 \
  --valid-from-epoch-ms 5500 \
  --valid-to-epoch-ms 5502 \
  --last-verified-at-epoch-ms 5503)"
retracted_decision_id="$(printf '%s\n' "${retracted_decision_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${retracted_decision_id}"

retracted_decision_payload="$(run_amai_last_line memory get-card --memory-card-id "${retracted_decision_id}")"
python3 - "${retracted_decision_payload}" "${retracted_decision_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
card_id = sys.argv[2]
assert payload["memory_card_id"] == card_id, payload
assert payload["candidate_class"] == "decision", payload
assert payload["truth_state"] == "retracted", payload
assert payload["verification_state"] == "disputed", payload
assert payload["status"] == "archived", payload
assert payload["observed_at_epoch_ms"] == 5500, payload
assert payload["recorded_at_epoch_ms"] == 5501, payload
assert payload["valid_from_epoch_ms"] == 5500, payload
assert payload["valid_to_epoch_ms"] == 5502, payload
PY

hypothesis_card_log="$(cargo run --quiet -- \
  memory create-card \
  --project project_alpha \
  --namespace review \
  --title "hypothesis ${suffix}" \
  --summary "unverified hypothesis summary ${suffix}" \
  --body "unverified hypothesis body ${suffix}" \
  --tag hypothesis \
  --provenance-json "${card_provenance}" \
  --truth-state unverified \
  --verification-state proposed \
  --status active \
  --observed-at-epoch-ms 5600 \
  --recorded-at-epoch-ms 5601 \
  --valid-from-epoch-ms 5600 \
  --last-verified-at-epoch-ms 5602)"
hypothesis_card_id="$(printf '%s\n' "${hypothesis_card_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${hypothesis_card_id}"

hypothesis_card_payload="$(run_amai_last_line memory get-card --memory-card-id "${hypothesis_card_id}")"
python3 - "${hypothesis_card_payload}" "${hypothesis_card_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
card_id = sys.argv[2]
assert payload["memory_card_id"] == card_id, payload
assert payload["truth_state"] == "unverified", payload
assert payload["verification_state"] == "proposed", payload
assert payload["status"] == "active", payload
assert payload["observed_at_epoch_ms"] == 5600, payload
assert payload["recorded_at_epoch_ms"] == 5601, payload
assert payload["valid_from_epoch_ms"] == 5600, payload
assert payload["valid_to_epoch_ms"] is None, payload
PY

current_cards_payload="$(run_amai_last_line \
  memory list-cards \
  --project project_alpha \
  --namespace review \
  --truth-state current)"
superseded_cards_payload="$(run_amai_last_line \
  memory list-cards \
  --project project_alpha \
  --namespace review \
  --truth-state superseded)"
retracted_cards_payload="$(run_amai_last_line \
  memory list-cards \
  --project project_alpha \
  --namespace review \
  --truth-state retracted)"
unverified_cards_payload="$(run_amai_last_line \
  memory list-cards \
  --project project_alpha \
  --namespace review \
  --truth-state unverified)"
current_cards_payload_file="$(mktemp)"
superseded_cards_payload_file="$(mktemp)"
retracted_cards_payload_file="$(mktemp)"
unverified_cards_payload_file="$(mktemp)"
printf '%s\n' "${current_cards_payload}" > "${current_cards_payload_file}"
printf '%s\n' "${superseded_cards_payload}" > "${superseded_cards_payload_file}"
printf '%s\n' "${retracted_cards_payload}" > "${retracted_cards_payload_file}"
printf '%s\n' "${unverified_cards_payload}" > "${unverified_cards_payload_file}"
python3 - "${current_cards_payload_file}" "${superseded_cards_payload_file}" "${retracted_cards_payload_file}" "${unverified_cards_payload_file}" "${cli_card_update_id}" "${supersede_source_id}" "${retracted_decision_id}" "${hypothesis_card_id}" <<'PY'
import json
import pathlib
import sys

current_cards = json.loads(pathlib.Path(sys.argv[1]).read_text())
superseded_cards = json.loads(pathlib.Path(sys.argv[2]).read_text())
retracted_cards = json.loads(pathlib.Path(sys.argv[3]).read_text())
unverified_cards = json.loads(pathlib.Path(sys.argv[4]).read_text())
current_id = sys.argv[5]
superseded_id = sys.argv[6]
retracted_id = sys.argv[7]
unverified_id = sys.argv[8]

assert any(card["memory_card_id"] == current_id for card in current_cards), current_cards
assert any(card["memory_card_id"] == superseded_id for card in superseded_cards), superseded_cards
assert any(card["memory_card_id"] == retracted_id for card in retracted_cards), retracted_cards
assert any(card["memory_card_id"] == unverified_id for card in unverified_cards), unverified_cards
PY
rm -f "${current_cards_payload_file}" "${superseded_cards_payload_file}" "${retracted_cards_payload_file}" "${unverified_cards_payload_file}"

./scripts/proof_runtime_sufficiency_router.sh >/tmp/amai-proof-stage2-runtime-sufficiency-router.log
grep -q "proof_runtime_sufficiency_router: PASS" /tmp/amai-proof-stage2-runtime-sufficiency-router.log

set +e
psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    visibility_scope,
    item_kind,
    title,
    truth_state,
    trust_state,
    verification_state,
    lifecycle_state,
    derivation_kind,
    imported_from,
    metadata
  )
  SELECT
    p.workspace_id,
    p.project_id,
    n.namespace_id,
    p.visibility_scope,
    'fact',
    'invalid local basis-free durable write ${suffix}',
    'proposed',
    'proposed',
    'unverified',
    'hot',
    'extract',
    '{}'::jsonb,
    '{}'::jsonb
  FROM ami.projects p
  JOIN ami.namespaces n
    ON n.project_id = p.project_id
   AND n.code = 'review'
  WHERE p.code = 'project_alpha'
" >/tmp/amai-proof-stage2-basis-free-write.log 2>&1
basis_free_exit=$?

psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    visibility_scope,
    item_kind,
    title,
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
    imported_from,
    metadata
  )
  SELECT
    p.workspace_id,
    p.project_id,
    n.namespace_id,
    p.visibility_scope,
    'fact',
    'invalid verified writeback no metadata ${suffix}',
    'current',
    'verified',
    'verified',
    'hot',
    '[\"event:writeback:${suffix}\"]'::jsonb,
    '[]'::jsonb,
    '[]'::jsonb,
    '{\"source\":\"proof\",\"line\":1}'::jsonb,
    'verified_write_back',
    3000,
    3001,
    3000,
    3002,
    '{}'::jsonb,
    '{}'::jsonb
  FROM ami.projects p
  JOIN ami.namespaces n
    ON n.project_id = p.project_id
   AND n.code = 'review'
  WHERE p.code = 'project_alpha'
" >/tmp/amai-proof-stage2-writeback-missing-metadata.log 2>&1
missing_metadata_exit=$?

psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    visibility_scope,
    item_kind,
    title,
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
    imported_from,
    metadata
  )
  SELECT
    p.workspace_id,
    p.project_id,
    n.namespace_id,
    p.visibility_scope,
    'fact',
    'invalid verified writeback summary-only ${suffix}',
    'current',
    'verified',
    'verified',
    'hot',
    '[\"event:writeback:${suffix}\"]'::jsonb,
    '[]'::jsonb,
    '[]'::jsonb,
    '{\"source\":\"proof\",\"line\":1}'::jsonb,
    'verified_write_back',
    3010,
    3011,
    3010,
    3012,
    '{}'::jsonb,
    '{\"writeback_evidence\":{\"escalated\":true,\"verified\":true,\"confirmed_via\":\"summary_compact\"}}'::jsonb
  FROM ami.projects p
  JOIN ami.namespaces n
    ON n.project_id = p.project_id
   AND n.code = 'review'
  WHERE p.code = 'project_alpha'
" >/tmp/amai-proof-stage2-writeback-summary-only.log 2>&1
summary_only_exit=$?

psql "${dsn}" -v ON_ERROR_STOP=1 -qc "
  INSERT INTO ami.memory_items(
    workspace_id,
    project_id,
    namespace_id,
    visibility_scope,
    item_kind,
    title,
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
    imported_from,
    metadata
  )
  SELECT
    p.workspace_id,
    p.project_id,
    n.namespace_id,
    p.visibility_scope,
    'fact',
    'invalid verified writeback missing refs ${suffix}',
    'current',
    'verified',
    'verified',
    'hot',
    '[]'::jsonb,
    '[]'::jsonb,
    '[]'::jsonb,
    '{\"source\":\"proof\",\"line\":1}'::jsonb,
    'verified_write_back',
    3020,
    3021,
    3020,
    3022,
    '{}'::jsonb,
    '{\"writeback_evidence\":{\"escalated\":true,\"verified\":true,\"confirmed_via\":\"raw_evidence\"}}'::jsonb
  FROM ami.projects p
  JOIN ami.namespaces n
    ON n.project_id = p.project_id
   AND n.code = 'review'
  WHERE p.code = 'project_alpha'
" >/tmp/amai-proof-stage2-writeback-missing-refs.log 2>&1
missing_refs_exit=$?
set -e

test "${basis_free_exit}" -ne 0
test "${missing_metadata_exit}" -ne 0
test "${summary_only_exit}" -ne 0
test "${missing_refs_exit}" -ne 0

valid_writeback_create_log="$(cargo run --quiet -- memory create-item \
  --project project_alpha \
  --namespace review \
  --item-kind fact \
  --title "valid verified writeback ${suffix}" \
  --summary "verified writeback compact summary" \
  --truth-state current \
  --trust-state verified \
  --verification-state verified \
  --lifecycle-state hot \
  --source-event-id "event:writeback:${suffix}" \
  --artifact-ref "artifact://proof/raw/${suffix}" \
  --message-ref "message:writeback:${suffix}" \
  --evidence-span-json '{"source":"proof","kind":"raw_log","range":"12-18"}' \
  --derivation-kind verified_write_back \
  --observed-at-epoch-ms 3030 \
  --recorded-at-epoch-ms 3031 \
  --valid-from-epoch-ms 3030 \
  --last-verified-at-epoch-ms 3032 \
  --imported-from-json '{}' \
  --metadata-json '{"writeback_evidence":{"escalated":true,"verified":true,"confirmed_via":"raw_evidence"}}' \
  --json)"
valid_writeback_id="$(python3 - "${valid_writeback_create_log}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
print(payload["memory_item_id"])
PY
)"

valid_writeback_payload="$(run_amai_last_line memory get-item --memory-item-id "${valid_writeback_id}")"

python3 - "${valid_writeback_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["derivation_kind"] == "verified_write_back", payload
assert payload["truth_state"] == "current", payload
assert payload["trust_state"] == "verified", payload
assert payload["verification_state"] == "verified", payload
assert payload["evidence_span"]["kind"] == "raw_log", payload
PY

provenance_payload="$(run_amai_last_line memory get-provenance --memory-provenance-id "${cli_memory_provenance_id}")"

python3 - "${provenance_payload}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
assert payload["trust_level"] == "verified", payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "memory-provenance-v1", payload
assert payload["message_refs"] == ["message:" + payload["source_event_id"].split(":")[-1]], payload
assert payload["evidence_span"]["source"] == "proof", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"] is True, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
assert payload["source_kind"] == "proof_contract", payload
PY

artifact_ref_payload="$(run_amai_last_line memory get-artifact-ref --artifact-ref-id "${cli_artifact_ref_id}")"

python3 - "${artifact_ref_payload}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
suffix = sys.argv[2]
assert payload["artifact_kind"] == "log_excerpt", payload
assert payload["bucket"] == "proof-stage2", payload
assert payload["object_key"] == f"artifact/{suffix}.json", payload
assert payload["content_type"] == "application/json", payload
assert payload["source_kind"] == "proof_contract", payload
assert payload["source_event_ids"] == [f"event:artifact:{suffix}"], payload
assert payload["message_refs"] == [f"message:artifact:{suffix}"], payload
assert payload["evidence_span"]["kind"] == "artifact", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"] is True, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "artifact-ref-envelope-v1", payload
assert payload["metadata"]["surface"] == "artifact_ref", payload
PY

cli_shared_asset_log="$(cargo run --quiet -- shared-asset ensure \
  --workspace default \
  --code "proof-shared-asset-${suffix}" \
  --display-name "Proof Shared Asset ${suffix}" \
  --asset-kind artifact \
  --source-project project_alpha \
  --transfer-policy "cli-transfer-policy-${suffix}" \
  --visibility-scope cross_project_linked \
  --status active \
  --source-kind shared_runtime \
  --source-event-id "event:shared-asset:${suffix}" \
  --artifact-ref "artifact://proof/shared-asset/${suffix}" \
  --message-ref "message:shared-asset:${suffix}" \
  --evidence-span-json '{"surface":"shared_asset","id":"'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version shared-asset-envelope-v1 \
  --json)"
cli_shared_asset_id="$(python3 - "${cli_shared_asset_log}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
print(payload["shared_asset_id"])
PY
)"
test -n "${cli_shared_asset_id}"

cli_shared_asset_payload="$(run_amai_last_line shared-asset get --shared-asset-id "${cli_shared_asset_id}")"
python3 - "${cli_shared_asset_payload}" "${cli_shared_asset_id}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
shared_asset_id = sys.argv[2]
suffix = sys.argv[3]
assert payload["shared_asset_id"] == shared_asset_id, payload
assert payload["workspace_code"] == "default", payload
assert payload["code"] == f"proof-shared-asset-{suffix}", payload
assert payload["display_name"] == f"Proof Shared Asset {suffix}", payload
assert payload["asset_kind"] == "artifact", payload
assert payload["source_project_code"] == "project_alpha", payload
assert payload["source_kind"] == "shared_runtime", payload
assert payload["source_event_ids"] == [f"event:shared-asset:{suffix}"], payload
assert payload["artifact_refs"] == [f"artifact://proof/shared-asset/{suffix}"], payload
assert payload["message_refs"] == [f"message:shared-asset:{suffix}"], payload
assert payload["evidence_span"]["surface"] == "shared_asset", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["transfer_policy_required"] is True, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
assert payload["derivation_kind"] == "extract", payload
assert payload["schema_version"] == "shared-asset-envelope-v1", payload
assert payload["visibility_scope"] == "cross_project_linked", payload
assert payload["status"] == "active", payload
PY

cli_shared_asset_bind_log="$(cargo run --quiet -- shared-asset bind \
  --asset "proof-shared-asset-${suffix}" \
  --project project_beta \
  --binding-kind consumer \
  --source-kind shared_runtime \
  --source-event-id "event:shared-asset-bind:${suffix}" \
  --artifact-ref "artifact://proof/shared-asset-bind/${suffix}" \
  --message-ref "message:shared-asset-bind:${suffix}" \
  --evidence-span-json '{"surface":"shared_asset_bind","id":"'"${suffix}"'"}' \
  --derivation-kind extract \
  --schema-version shared-asset-project-binding-v1 \
  --json)"
python3 - "${cli_shared_asset_bind_log}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
suffix = sys.argv[2]
assert payload["asset"] == f"proof-shared-asset-{suffix}", payload
assert payload["project"] == "project_beta", payload
assert payload["binding_kind"] == "consumer", payload
PY

cli_shared_asset_list_payload="$(run_amai_last_line shared-asset list --project project_beta --code "proof-shared-asset-${suffix}" --json)"
python3 - "${cli_shared_asset_list_payload}" "${cli_shared_asset_id}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
shared_asset_id = sys.argv[2]
suffix = sys.argv[3]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["shared_asset_id"] == shared_asset_id), None)
assert match is not None, payload
assert match["code"] == f"proof-shared-asset-{suffix}", match
assert match["display_name"] == f"Proof Shared Asset {suffix}", match
assert match["asset_kind"] == "artifact", match
assert match["source_project_code"] == "project_alpha", match
assert match["status"] == "active", match
PY

cli_import_packet_log="$(cargo run --quiet -- import-packet create \
  --source-project project_alpha \
  --target-project project_beta \
  --requested-by-agent "${agent_code}" \
  --status borrowed_unverified \
  --summary "proof import packet ${suffix}" \
  --reason "proof relation import ${suffix}" \
  --imported-by-agent-scope imported \
  --trust-state proposed \
  --verification-state unverified \
  --borrowed-status borrowed \
  --memory-object-id "${memory_item_id}" \
  --artifact-ref "artifact://proof/import-packet/${suffix}" \
  --source-kind cross_project_import \
  --source-event-id "event:import-packet:${suffix}" \
  --message-ref "message:import-packet:${suffix}" \
  --evidence-span-json '{"surface":"import_packet","id":"'"${suffix}"'"}' \
  --derivation-kind import \
  --schema-version import-packet-envelope-v1 \
  --json)"
cli_import_packet_id="$(python3 - "${cli_import_packet_log}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
print(payload["import_packet_id"])
PY
)"
test -n "${cli_import_packet_id}"

cli_import_packet_payload="$(run_amai_last_line import-packet get --import-packet-id "${cli_import_packet_id}")"
python3 - "${cli_import_packet_payload}" "${cli_import_packet_id}" "${suffix}" "${agent_code}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
import_packet_id = sys.argv[2]
suffix = sys.argv[3]
agent_code = sys.argv[4]
assert payload["import_packet_id"] == import_packet_id, payload
assert payload["source_project_code"] == "project_alpha", payload
assert payload["target_project_code"] == "project_beta", payload
assert payload["requested_by_agent_code"] == agent_code, payload
assert payload["source_kind"] == "cross_project_import", payload
assert payload["source_event_ids"] == [f"event:import-packet:{suffix}"], payload
assert payload["artifact_refs"] == [f"artifact://proof/import-packet/{suffix}"], payload
assert payload["message_refs"] == [f"message:import-packet:{suffix}"], payload
assert payload["evidence_span"]["surface"] == "import_packet", payload
assert payload["derivation_kind"] == "import", payload
assert payload["schema_version"] == "import-packet-envelope-v1", payload
assert payload["status"] == "borrowed_unverified", payload
assert payload["summary"] == f"proof import packet {suffix}", payload
assert payload["reason"] == f"proof relation import {suffix}", payload
assert payload["imported_by_agent_scope"] == "imported", payload
assert payload["trust_state"] == "proposed", payload
assert payload["verification_state"] == "unverified", payload
assert payload["borrowed_status"] == "borrowed", payload
assert payload["can_promote_after_verification"] is False, payload
PY

cli_import_packet_update_log="$(cargo run --quiet -- import-packet update \
  --import-packet-id "${cli_import_packet_id}" \
  --status quarantined \
  --reason "proof quarantine ${suffix}" \
  --summary "proof quarantined packet ${suffix}" \
  --imported-by-agent-scope quarantined \
  --trust-state untrusted \
  --verification-state disputed \
  --borrowed-status blocked \
  --actor-agent "${agent_code}" \
  --json)"
python3 - "${cli_import_packet_update_log}" "${cli_import_packet_id}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
import_packet_id = sys.argv[2]
assert payload["import_packet_id"] == import_packet_id, payload
assert payload["status"] == "quarantined", payload
PY

cli_import_packet_updated_payload="$(run_amai_last_line import-packet get --import-packet-id "${cli_import_packet_id}")"
python3 - "${cli_import_packet_updated_payload}" "${cli_import_packet_id}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
import_packet_id = sys.argv[2]
suffix = sys.argv[3]
assert payload["import_packet_id"] == import_packet_id, payload
assert payload["status"] == "quarantined", payload
assert payload["summary"] == f"proof quarantined packet {suffix}", payload
assert payload["reason"] == f"proof quarantine {suffix}", payload
assert payload["imported_by_agent_scope"] == "quarantined", payload
assert payload["trust_state"] == "untrusted", payload
assert payload["verification_state"] == "disputed", payload
assert payload["borrowed_status"] == "blocked", payload
PY

cli_import_packet_list_payload="$(run_amai_last_line import-packet list --project project_alpha --import-packet-id "${cli_import_packet_id}" --json)"
python3 - "${cli_import_packet_list_payload}" "${cli_import_packet_id}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
import_packet_id = sys.argv[2]
suffix = sys.argv[3]
assert isinstance(payload, list) and payload, payload
match = next((row for row in payload if row["import_packet_id"] == import_packet_id), None)
assert match is not None, payload
assert match["source_project_code"] == "project_alpha", match
assert match["target_project_code"] == "project_beta", match
assert match["status"] == "quarantined", match
assert match["summary"] == f"proof quarantined packet {suffix}", match
assert match["reason"] == f"proof quarantine {suffix}", match
assert match["trust_state"] == "untrusted", match
assert match["verification_state"] == "disputed", match
PY

cli_policy_rule_log="$(cargo run --quiet -- memory create-policy-rule \
  --workspace default \
  --project project_alpha \
  --namespace review \
  --rule-code "proof-policy-rule-${suffix}" \
  --rule-scope project \
  --rule-kind scope_filter \
  --rule-status active \
  --precedence 42 \
  --source-kind operator_panel \
  --source-event-id "event:policy-rule:${suffix}" \
  --artifact-ref "artifact://proof/policy-rule/${suffix}" \
  --message-ref "message:policy-rule:${suffix}" \
  --evidence-span-json '{"surface":"policy_rule","id":"'"${suffix}"'"}' \
  --derivation-kind operator_write \
  --schema-version policy-rule-envelope-v1 \
  --rule-payload-json '{"allow":["project_shared"],"deny":[]}' )"
cli_policy_rule_id="$(printf '%s\n' "${cli_policy_rule_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_policy_rule_id}"

cli_policy_rule_payload="$(run_amai_last_line memory get-policy-rule --policy-rule-id "${cli_policy_rule_id}")"
python3 - "${cli_policy_rule_payload}" "${cli_policy_rule_id}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
policy_rule_id = sys.argv[2]
suffix = sys.argv[3]
assert payload["policy_rule_id"] == policy_rule_id, payload
assert payload["workspace_code"] == "default", payload
assert payload["project_code"] == "project_alpha", payload
assert payload["namespace_code"] == "review", payload
assert payload["rule_code"] == f"proof-policy-rule-{suffix}", payload
assert payload["rule_scope"] == "project", payload
assert payload["rule_kind"] == "scope_filter", payload
assert payload["rule_status"] == "active", payload
assert payload["precedence"] == 42, payload
assert payload["source_kind"] == "operator_panel", payload
assert payload["source_event_ids"] == [f"event:policy-rule:{suffix}"], payload
assert payload["artifact_refs"] == [f"artifact://proof/policy-rule/{suffix}"], payload
assert payload["message_refs"] == [f"message:policy-rule:{suffix}"], payload
assert payload["evidence_span"]["surface"] == "policy_rule", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"] is True, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
assert payload["derivation_kind"] == "operator_write", payload
assert payload["schema_version"] == "policy-rule-envelope-v1", payload
assert payload["rule_payload"]["allow"] == ["project_shared"], payload
PY

cli_quarantine_item_log="$(cargo run --quiet -- memory create-quarantine-item \
  --workspace default \
  --project project_alpha \
  --namespace review \
  --entity-kind policy_rule \
  --entity-id "${cli_policy_rule_id}" \
  --quarantine-reason "proof quarantine ${suffix}" \
  --quarantine-state active \
  --evidence-json '{"proof":"stage2","surface":"quarantine_item"}' \
  --source-kind operator_panel \
  --source-event-id "event:quarantine-item:${suffix}" \
  --artifact-ref "artifact://proof/quarantine-item/${suffix}" \
  --message-ref "message:quarantine-item:${suffix}" \
  --evidence-span-json '{"surface":"quarantine_item","id":"'"${suffix}"'"}' \
  --derivation-kind operator_write \
  --schema-version quarantine-item-envelope-v1 \
  --quarantined-at-epoch-ms 7100)"
cli_quarantine_item_id="$(printf '%s\n' "${cli_quarantine_item_log}" | grep -Eo '[0-9a-f-]{36}' | head -n1)"
test -n "${cli_quarantine_item_id}"

cli_quarantine_item_payload="$(run_amai_last_line memory get-quarantine-item --quarantine-item-id "${cli_quarantine_item_id}")"
python3 - "${cli_quarantine_item_payload}" "${cli_quarantine_item_id}" "${cli_policy_rule_id}" "${suffix}" <<'PY'
import json
import sys

payload = json.loads(sys.argv[1])
quarantine_item_id = sys.argv[2]
policy_rule_id = sys.argv[3]
suffix = sys.argv[4]
assert payload["quarantine_item_id"] == quarantine_item_id, payload
assert payload["workspace_code"] == "default", payload
assert payload["project_code"] == "project_alpha", payload
assert payload["namespace_code"] == "review", payload
assert payload["entity_kind"] == "policy_rule", payload
assert payload["entity_id"] == policy_rule_id, payload
assert payload["quarantine_reason"] == f"proof quarantine {suffix}", payload
assert payload["quarantine_state"] == "active", payload
assert payload["evidence"]["surface"] == "quarantine_item", payload
assert payload["source_kind"] == "operator_panel", payload
assert payload["source_event_ids"] == [f"event:quarantine-item:{suffix}"], payload
assert payload["artifact_refs"] == [f"artifact://proof/quarantine-item/{suffix}"], payload
assert payload["message_refs"] == [f"message:quarantine-item:{suffix}"], payload
assert payload["evidence_span"]["surface"] == "quarantine_item", payload
assert payload["evidence_span"]["stage2_runtime"]["policy_and_scope_filter"]["scope_binding_valid"] is True, payload
assert payload["evidence_span"]["stage2_runtime"]["verification_conflict_check"]["write_allowed"] is True, payload
assert payload["derivation_kind"] == "operator_write", payload
assert payload["schema_version"] == "quarantine-item-envelope-v1", payload
assert payload["quarantined_at_epoch_ms"] == 7100, payload
assert payload["released_at_epoch_ms"] is None, payload
PY

cargo test --quiet postgres::tests::write_pipeline_materializes_stage_two_contract -- --exact
cargo test --quiet postgres::tests::memory_candidate_extraction_marks_background_semantic_consolidation -- --exact
cargo test --quiet postgres::tests::memory_candidate_validation_rejects_basis_free_extract -- --exact
cargo test --quiet postgres::tests::memory_card_candidate_extraction_marks_runtime_contract -- --exact
cargo test --quiet postgres::tests::memory_card_candidate_validation_rejects_basis_free_extract -- --exact
cargo test --quiet postgres::tests::canonical_candidate_class_covers_all_five_classes -- --exact
cargo test --quiet postgres::tests::create_memory_card_surfaces_stage2_runtime_fields -- --exact
cargo test --quiet postgres::tests::skill_card_candidate_extraction_marks_runtime_contract -- --exact
cargo test --quiet postgres::tests::skill_card_candidate_validation_rejects_basis_free_extract -- --exact
cargo test --quiet postgres::tests::create_skill_card_candidate_surfaces_stage2_runtime_fields -- --exact
cargo test --quiet postgres::tests::task_node_candidate_extraction_marks_runtime_contract -- --exact
cargo test --quiet postgres::tests::task_node_candidate_validation_rejects_basis_free_extract -- --exact
cargo test --quiet postgres::tests::create_task_node_surfaces_stage2_runtime_fields -- --exact
cargo test --quiet postgres::tests::task_event_validation_rejects_basis_free_raw_capture -- --exact
cargo test --quiet postgres::tests::create_task_event_surfaces_raw_event_provenance_fields -- --exact
cargo test --quiet postgres::tests::memory_link_decision_validation_rejects_basis_free_extract -- --exact
cargo test --quiet postgres::tests::pending_link_proposal_validation_rejects_missing_ttl_and_evidence_request -- --exact
cargo test --quiet postgres::tests::artifact_ref_validation_rejects_basis_free_extract -- --exact
cargo test --quiet postgres::tests::skill_evidence_bundle_validation_rejects_basis_free_extract -- --exact
cargo test --quiet postgres::tests::memory_relation_edge_validation_rejects_basis_free_extract -- --exact
cargo test --quiet postgres::tests::skill_activity_validation_rejects_basis_free_extract -- --exact
cargo test --quiet postgres::tests::create_memory_link_decision_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::create_memory_link_decision_policy_scope_filter_rejects_scope_mismatch -- --exact
cargo test --quiet postgres::tests::create_memory_link_decision_verification_conflict_check_detects_poisoned_evidence_span -- --exact
cargo test --quiet postgres::tests::create_pending_link_proposal_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::create_pending_link_proposal_policy_scope_filter_rejects_scope_mismatch -- --exact
cargo test --quiet postgres::tests::create_pending_link_proposal_verification_conflict_check_detects_poisoned_evidence_span -- --exact
cargo test --quiet postgres::tests::insert_artifact_ref_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::create_skill_evidence_bundle_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::create_memory_relation_edge_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::create_memory_relation_edge_policy_scope_filter_rejects_source_scope_mismatch -- --exact
cargo test --quiet postgres::tests::create_memory_relation_edge_verification_conflict_check_detects_poisoned_evidence_span -- --exact
cargo test --quiet postgres::tests::create_artifact_ref_surfaces_stage2_runtime_fields -- --exact
cargo test --quiet postgres::tests::create_artifact_ref_policy_scope_filter_rejects_namespace_mismatch -- --exact
cargo test --quiet postgres::tests::create_artifact_ref_verification_conflict_check_detects_poisoned_evidence_span -- --exact
cargo test --quiet postgres::tests::record_skill_trigger_match_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::record_skill_trial_run_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::record_skill_eval_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::record_skill_reuse_log_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::stage2_system_tables_surface_provenance_columns -- --exact
cargo test --quiet postgres::tests::import_and_shared_surface_validation_rejects_basis_free_extract -- --exact
cargo test --quiet postgres::tests::create_import_packet_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::ensure_shared_asset_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::bind_shared_asset_to_project_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::create_retrieval_trace_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::create_retrieval_trace_policy_scope_filter_rejects_context_pack_scope_mismatch -- --exact
cargo test --quiet postgres::tests::create_retrieval_trace_verification_conflict_check_detects_decision_trace_mismatch -- --exact
cargo test --quiet postgres::tests::create_memory_provenance_surfaces_stage2_runtime_fields -- --exact
cargo test --quiet postgres::tests::create_memory_provenance_policy_scope_filter_rejects_memory_item_scope_mismatch -- --exact
cargo test --quiet postgres::tests::create_memory_provenance_verification_conflict_check_detects_poisoned_evidence_span -- --exact
cargo test --quiet postgres::tests::create_restore_pack_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::create_restore_pack_policy_scope_filter_requires_source_snapshot_for_workspace_restore_pack -- --exact
cargo test --quiet postgres::tests::create_restore_pack_policy_scope_filter_rejects_snapshot_scope_mismatch -- --exact
cargo test --quiet postgres::tests::create_restore_pack_verification_conflict_check_detects_poisoned_evidence_span -- --exact
cargo test --quiet postgres::tests::ensure_policy_surfaces_materialize_policy_rules -- --exact
cargo test --quiet postgres::tests::update_import_packet_quarantine_materializes_and_releases_quarantine_item -- --exact
cargo test --quiet postgres::tests::update_relation_quarantine_materializes_and_resolves_quarantine_item -- --exact
cargo test --quiet postgres::tests::create_memory_edge_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::create_memory_conflict_surfaces_stage2_provenance_fields -- --exact
cargo test --quiet postgres::tests::create_memory_edge_policy_scope_filter_rejects_source_scope_mismatch -- --exact
cargo test --quiet postgres::tests::create_memory_edge_verification_conflict_check_detects_poisoned_evidence_span -- --exact
cargo test --quiet postgres::tests::create_memory_conflict_policy_scope_filter_rejects_left_scope_mismatch -- --exact
cargo test --quiet postgres::tests::create_memory_conflict_verification_conflict_check_detects_poisoned_evidence_span -- --exact
cargo test --quiet working_state::tests::record_handoff_event_materializes_workspace_restore_pack -- --exact
cargo test --quiet postgres::tests::stage2_runtime_metadata_is_augmented_for_read_projection -- --exact
cargo test --quiet postgres::tests::create_memory_item_materializes_raw_event_and_outbox -- --exact
cargo test --quiet postgres::tests::relay_memory_write_outbox_marks_rows_published -- --exact
cargo test --quiet postgres::tests::memory_envelope_view_surfaces_stage2_runtime_fields -- --exact

printf 'proof_typed_memory_envelope_contract: PASS\n'
