#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
repo_root="$(pwd)"

mkdir -p state/locks
exec 9>state/locks/graph_first_startup_restore_projection.lock
flock --exclusive 9

cargo test --quiet task_graph_projection_
cargo test --quiet compact_project_task_surfaces_preserve_already_compacted_counts
cargo test --quiet continuity_startup_summary_surfaces_execctl_and_prompt_state

./scripts/continuity_startup.sh \
  --repo-root "${repo_root}" \
  --namespace continuity \
  --json >/dev/null

state_file=".amai/continuity/project-chat-startup-state.json"
test -f "${state_file}"

jq -e '
  .continuity_startup_summary.task_graph_projection_validation as $validation
  | .continuity_startup_summary.project_task_tree as $tree
  | .continuity_startup_summary.project_task_ledger as $ledger
  | ($validation.status // "") as $status
  | ($validation.truth_claim == false)
    and (
      if $status == "fallback_to_execctl_ledger" then
        $validation.projection_source == "execctl_ledger_fallback"
        and ($validation.reason | type == "string" and length > 0)
        and ($ledger.storage_lane == "ami.execctl_task_ledger_entries")
      else
        $status == "valid"
        and $validation.projection_source == "graph_first_sql_validated"
        and $validation.control_invariant.status == "passed"
        and (($validation.warnings.legacy_hot_historical_sql_nodes_count // 0) == 0)
        and $tree.projection_kind == "task-graph-startup-projection-v1"
        and (
          if (($validation.projection_excluded_sql_nodes_count // 0) > 0) then
            (.continuity_startup_summary.project_task_tree_summary | type == "string" and contains("excluded_legacy("))
            and (.continuity_startup_summary.project_task_ledger_summary | type == "string" and contains("excluded_legacy("))
          else
            true
          end
        )
      end
    )
' "${state_file}" >/dev/null

echo "proof_graph_first_startup_restore_projection: ok"
