#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

project_code="${1:-amai}"
namespace_code="${2:-continuity}"
report_dir="state/reports"
report_path="${report_dir}/procedural_shadow_mode_review_${project_code}_${namespace_code}.md"

mkdir -p "${report_dir}"

default_card_json="$(cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --runtime "codex" \
  --tool "exec_command" 2>/dev/null)"
trial_card_json="$(cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --runtime "codex" \
  --tool "exec_command" \
  --allow-trial 2>/dev/null)"
shadow_card_json="$(cargo run --quiet -- skill execution-card \
  --project "${project_code}" \
  --namespace "${namespace_code}" \
  --runtime "codex" \
  --tool "exec_command" \
  --include-shadow 2>/dev/null)"

skills_raw="$(cargo run --quiet -- skill list --project "${project_code}" --namespace "${namespace_code}" 2>/dev/null)"
review_lines="$(printf '%s\n' "${skills_raw}" | grep ' :: trust=' | grep -E ' :: trust=(candidate|shadow|trial) :: ' || true)"

{
  echo "# Procedural Shadow-Mode Review"
  echo
  echo "- Project: \`${project_code}\`"
  echo "- Namespace: \`${namespace_code}\`"
  echo "- Generated at: \`$(date -u +"%Y-%m-%dT%H:%M:%SZ")\`"
  echo
} > "${report_path}"

if [[ -z "${review_lines}" ]]; then
  {
    echo "No candidate/shadow/trial skills found."
  } >> "${report_path}"
  echo "${report_path}"
  exit 0
fi

anomaly_count=0
review_count=0

while IFS= read -r line; do
  [[ -z "${line}" ]] && continue
  review_count=$((review_count + 1))
  skill_card_id="$(printf '%s\n' "${line}" | awk -F' :: ' '{print $1}')"
  skill_ref="$(printf '%s\n' "${line}" | awk -F' :: ' '{print $4}')"
  trust_state="$(printf '%s\n' "${line}" | sed -n 's/.* :: trust=\([^ ]*\) :: verify=.*/\1/p')"
  verify_state="$(printf '%s\n' "${line}" | sed -n 's/.* :: verify=\([^ ]*\) :: utility=.*/\1/p')"

  review_json="$(cargo run --quiet -- skill review --skill-card-id "${skill_card_id}" 2>/dev/null)"

  default_visible="no"
  trial_visible="no"
  shadow_visible="no"
  if printf '%s\n' "${default_card_json}" | jq -e --arg skill_card_id "${skill_card_id}" '.[] | select(.skill_card_id == $skill_card_id)' >/dev/null; then
    default_visible="yes"
  fi
  if printf '%s\n' "${trial_card_json}" | jq -e --arg skill_card_id "${skill_card_id}" '.[] | select(.skill_card_id == $skill_card_id)' >/dev/null; then
    trial_visible="yes"
  fi
  if printf '%s\n' "${shadow_card_json}" | jq -e --arg skill_card_id "${skill_card_id}" '.[] | select(.skill_card_id == $skill_card_id)' >/dev/null; then
    shadow_visible="yes"
  fi

  if [[ "${trust_state}" != "verified" && "${default_visible}" == "yes" ]]; then
    anomaly_count=$((anomaly_count + 1))
  fi
  if [[ "${trust_state}" == "trial" && "${trial_visible}" != "yes" ]]; then
    anomaly_count=$((anomaly_count + 1))
  fi
  if [[ "${trust_state}" == "shadow" && "${shadow_visible}" != "yes" ]]; then
    anomaly_count=$((anomaly_count + 1))
  fi

  evidence_count="$(printf '%s\n' "${review_json}" | jq -r '.evidence_count')"
  trigger_count="$(printf '%s\n' "${review_json}" | jq -r '.trigger_matches | length')"
  trial_count="$(printf '%s\n' "${review_json}" | jq -r '.trial_runs | length')"
  eval_verdicts="$(printf '%s\n' "${review_json}" | jq -r '[.evals[].verdict] | join(", ")')"
  reuse_modes="$(printf '%s\n' "${review_json}" | jq -r '[.reuse_logs[].reuse_mode] | join(", ")')"
  shadow_pass="$(printf '%s\n' "${review_json}" | jq -r '.skill.skill_shadow_pass_count')"
  shadow_fail="$(printf '%s\n' "${review_json}" | jq -r '.skill.skill_shadow_fail_count')"

  {
    echo "## ${skill_ref}"
    echo
    echo "- Skill card: \`${skill_card_id}\`"
    echo "- Trust / verify: \`${trust_state}\` / \`${verify_state}\`"
    echo "- Evidence bundles: \`${evidence_count}\`"
    echo "- Trigger matches: \`${trigger_count}\`"
    echo "- Trial runs: \`${trial_count}\`"
    echo "- Eval verdicts: \`${eval_verdicts:-none}\`"
    echo "- Reuse modes: \`${reuse_modes:-none}\`"
    echo "- Shadow pass/fail: \`${shadow_pass}\` / \`${shadow_fail}\`"
    echo "- Visible in default execution card: \`${default_visible}\`"
    echo "- Visible with allow-trial: \`${trial_visible}\`"
    echo "- Visible with include-shadow: \`${shadow_visible}\`"
    echo
  } >> "${report_path}"
done <<< "${review_lines}"

{
  echo "## Summary"
  echo
  echo "- Reviewed skills: \`${review_count}\`"
  echo "- Visibility anomalies: \`${anomaly_count}\`"
} >> "${report_path}"

echo "${report_path}"

if [[ "${anomaly_count}" -ne 0 ]]; then
  exit 1
fi
