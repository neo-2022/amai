#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

usage() {
  cat <<'EOF'
Usage:
  ./scripts/scientific_queue0_baseline_freeze.sh [--output-dir <dir>] [--baseline-allowlist <file>] [--require-clean-worktree] [--print-plan]

Queue 0 launcher for scientific reinforcement overlay.
It captures the canonical baseline artifact set and emits a machine-readable manifest.

Allowlist format:
  - plain text file
  - one entry per line
  - empty lines and lines starting with '#' are ignored
  - accepted forms:
    - <label>
    - <label>|<git_head>
    - <label>|<git_head>|<invocation_sha256>

Known command labels:
  - agent_preflight
  - maintainability_gate
  - benchmark_coverage
  - proof_memory_task_matrix
  - proof_mcp_task_matrix
  - proof_forgetting_consolidation
  - proof_observability
EOF
}

OUTPUT_ROOT="state/scientific/queue0_baseline_freeze"
BASELINE_ALLOWLIST=""
PRINT_PLAN=0
REQUIRE_CLEAN_WORKTREE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --output-dir)
      OUTPUT_ROOT="$2"
      shift 2
      ;;
    --baseline-allowlist)
      BASELINE_ALLOWLIST="$2"
      shift 2
      ;;
    --print-plan)
      PRINT_PLAN=1
      shift
      ;;
    --require-clean-worktree)
      REQUIRE_CLEAN_WORKTREE=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

readonly COMMAND_LABELS=(
  "agent_preflight"
  "maintainability_gate"
  "benchmark_coverage"
  "proof_memory_task_matrix"
  "proof_mcp_task_matrix"
  "proof_forgetting_consolidation"
  "proof_observability"
)

readonly COMMAND_LINES=(
  "./scripts/agent_preflight.sh --json"
  "./scripts/maintainability_gate.sh --json"
  "./scripts/amai_exec.sh benchmark coverage"
  "./scripts/proof_memory_task_matrix.sh"
  "./scripts/proof_mcp_task_matrix.sh"
  "./scripts/proof_forgetting_consolidation.sh"
  "./scripts/proof_observability.sh"
)

if [[ "${PRINT_PLAN}" -eq 1 ]]; then
  for i in "${!COMMAND_LABELS[@]}"; do
    printf '%s\t%s\n' "${COMMAND_LABELS[$i]}" "${COMMAND_LINES[$i]}"
  done
  exit 0
fi

if [[ -n "${BASELINE_ALLOWLIST}" && ! -f "${BASELINE_ALLOWLIST}" ]]; then
  echo "Allowlist file not found: ${BASELINE_ALLOWLIST}" >&2
  exit 2
fi

sanitize_label() {
  printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | tr -c 'a-z0-9_' '_'
}

sha256_file() {
  local target="$1"
  if [[ -f "${target}" ]]; then
    sha256sum "${target}" | awk '{print $1}'
  else
    printf 'null'
  fi
}

json_string() {
  jq -Rn --arg value "$1" '$value'
}

allowlisted_label() {
  local label="$1"
  local git_head="$2"
  local invocation_sha256="$3"
  if [[ -z "${BASELINE_ALLOWLIST}" ]]; then
    return 1
  fi
  while IFS= read -r raw_line; do
    local line
    line="$(printf '%s' "${raw_line}" | sed 's/[[:space:]]*$//')"
    [[ -z "${line}" || "${line}" =~ ^# ]] && continue
    if [[ "${line}" == "${label}" ]]; then
      return 0
    fi
    if [[ "${line}" == "${label}|${git_head}" ]]; then
      return 0
    fi
    if [[ "${line}" == "${label}|${git_head}|${invocation_sha256}" ]]; then
      return 0
    fi
  done < "${BASELINE_ALLOWLIST}"
  return 1
}

command_output_kind() {
  local output_path="$1"
  if jq -e . "${output_path}" >/dev/null 2>&1; then
    printf 'json'
  else
    printf 'text'
  fi
}

render_text_excerpt_json() {
  local output_path="$1"
  awk 'NF { print; count += 1; if (count == 12) exit }' "${output_path}" \
    | jq -R -s '
        split("\n")
        | map(select(length > 0))
      '
}

render_benchmark_coverage_summary_json() {
  local output_path="$1"
  local total materialized partial mapped next_priority future_only
  total="$(grep -m1 'Всего benchmark-эталонов в матрице:' "${output_path}" | grep -oE '[0-9]+' | head -n1 || true)"
  materialized="$(grep -m1 'Уже materialized напрямую:' "${output_path}" | grep -oE '[0-9]+' | head -n1 || true)"
  partial="$(grep -m1 'Частично покрыто текущими proof/harness слоями:' "${output_path}" | grep -oE '[0-9]+' | head -n1 || true)"
  mapped="$(grep -m1 'Уже mapped в канонический план и Rust-first contours:' "${output_path}" | grep -oE '[0-9]+' | head -n1 || true)"
  next_priority="$(grep -m1 'Следующий обязательный приоритет:' "${output_path}" | grep -oE '[0-9]+' | head -n1 || true)"
  future_only="$(grep -m1 'Пока только будущий слой:' "${output_path}" | grep -oE '[0-9]+' | head -n1 || true)"
  jq -n \
    --arg output_kind "text" \
    --arg total "${total:-}" \
    --arg materialized "${materialized:-}" \
    --arg partial "${partial:-}" \
    --arg mapped "${mapped:-}" \
    --arg next_priority "${next_priority:-}" \
    --arg future_only "${future_only:-}" \
    --argjson excerpt "$(render_text_excerpt_json "${output_path}")" \
    '{
      output_kind: $output_kind,
      totals: {
        benchmark_total: ($total | if . == "" then null else tonumber end),
        materialized_direct: ($materialized | if . == "" then null else tonumber end),
        partial_proof_coverage: ($partial | if . == "" then null else tonumber end),
        mapped_to_plan: ($mapped | if . == "" then null else tonumber end),
        next_required_priority: ($next_priority | if . == "" then null else tonumber end),
        future_only: ($future_only | if . == "" then null else tonumber end)
      },
      excerpt_lines: $excerpt
    }'
}

render_generic_text_summary_json() {
  local output_path="$1"
  local ok_sentinel="false"
  if rg -q '(^|[[:space:][:punct:]])ok($|[[:space:][:punct:]])' "${output_path}"; then
    ok_sentinel="true"
  fi
  jq -n \
    --arg output_kind "text" \
    --arg ok_sentinel "${ok_sentinel}" \
    --argjson excerpt "$(render_text_excerpt_json "${output_path}")" \
    '{
      output_kind: $output_kind,
      ok_sentinel: ($ok_sentinel == "true"),
      excerpt_lines: $excerpt
    }'
}

render_summary_json() {
  local label="$1"
  local output_path="$2"
  local summary_path="$3"
  local output_kind
  output_kind="$(command_output_kind "${output_path}")"
  if [[ "${output_kind}" == "json" ]]; then
    jq '.' "${output_path}" > "${summary_path}"
    return 0
  fi
  case "${label}" in
    benchmark_coverage)
      render_benchmark_coverage_summary_json "${output_path}" > "${summary_path}"
      ;;
    *)
      render_generic_text_summary_json "${output_path}" > "${summary_path}"
      ;;
  esac
}

lockfile_hashes_json() {
  local files=(
    "Cargo.lock"
    "compose.yaml"
    "package-lock.json"
    "pnpm-lock.yaml"
    "yarn.lock"
    "poetry.lock"
    "requirements.txt"
  )
  local obj='{}'
  local file
  for file in "${files[@]}"; do
    if [[ -f "${file}" ]]; then
      obj="$(jq \
        --arg path "${file}" \
        --arg sha256 "$(sha256_file "${file}")" \
        '. + {($path): $sha256}' <<<"${obj}")"
    fi
  done
  printf '%s\n' "${obj}"
}

safe_env_snapshot_json() {
  local keys=(
    "AMAI_EXEC_FORCE_CARGO"
    "AMAI_EXEC_SUPPRESS_BUILD_NOISE"
    "AMI_OBSERVE_BIND"
    "AMI_PROMETHEUS_PORT"
    "AMI_GRAFANA_PORT"
    "AMI_PROMETHEUS_SCRAPE_TARGET"
    "RUSTFLAGS"
    "CARGO_BUILD_JOBS"
  )
  local obj='{}'
  local key value
  for key in "${keys[@]}"; do
    value="${!key-}"
    if [[ -n "${value}" ]]; then
      obj="$(jq \
        --arg key "${key}" \
        --arg value "${value}" \
        '. + {($key): $value}' <<<"${obj}")"
    fi
  done
  printf '%s\n' "${obj}"
}

timestamp_epoch_ms="$(python3 - <<'PY'
import time
print(int(time.time() * 1000))
PY
)"
run_stamp="$(date -u +%Y%m%dT%H%M%SZ)"
run_dir="${OUTPUT_ROOT}/run_${run_stamp}"
commands_dir="${run_dir}/commands"
mkdir -p "${commands_dir}"

run_dir_abs="$(cd "$(dirname "${run_dir}")" && pwd)/$(basename "${run_dir}")"
manifest_path="${run_dir}/queue0_baseline_manifest.json"
manifest_tmp_path="${manifest_path}.tmp"

git_head="$(git rev-parse HEAD)"
git_branch="$(git rev-parse --abbrev-ref HEAD)"
worktree_status_initial="$(git status --porcelain=v1 --untracked-files=all || true)"
worktree_fingerprint_initial_sha256="$(printf '%s' "${worktree_status_initial}" | sha256sum | awk '{print $1}')"
allowlist_sha256="null"
if [[ -n "${BASELINE_ALLOWLIST}" ]]; then
  allowlist_sha256="$(sha256_file "${BASELINE_ALLOWLIST}")"
fi

rustc_version="$(rustc --version 2>/dev/null || true)"
cargo_version="$(cargo --version 2>/dev/null || true)"
python_version="$(python3 --version 2>/dev/null || true)"
uname_kernel="$(uname -srmo 2>/dev/null || true)"
docker_version="$(docker --version 2>/dev/null || true)"

if [[ "${REQUIRE_CLEAN_WORKTREE}" -eq 1 && -n "${worktree_status_initial}" ]]; then
  jq -n \
    --arg artifact_version "scientific-queue0-baseline-freeze-v1" \
    --arg queue_code "queue0_preflight_baseline_freeze" \
    --arg status "preflight_failed_dirty_worktree" \
    --arg repo_root "${REPO_ROOT}" \
    --arg run_dir "${run_dir_abs}" \
    --arg observed_at_epoch_ms "${timestamp_epoch_ms}" \
    --arg git_head "${git_head}" \
    --arg git_branch "${git_branch}" \
    --arg worktree_fingerprint_initial_sha256 "${worktree_fingerprint_initial_sha256}" \
    --arg allowlist_path "${BASELINE_ALLOWLIST}" \
    --arg allowlist_sha256 "${allowlist_sha256}" \
    --argjson require_clean_worktree "${REQUIRE_CLEAN_WORKTREE}" \
    --argjson worktree_lines "$(printf '%s\n' "${worktree_status_initial}" | jq -R . | jq -s 'map(select(length > 0))')" \
    '{
      artifact_version: $artifact_version,
      queue_code: $queue_code,
      status: $status,
      observed_at_epoch_ms: ($observed_at_epoch_ms | tonumber),
      repo_root: $repo_root,
      run_dir: $run_dir,
      git_head: $git_head,
      git_branch: $git_branch,
      require_clean_worktree: ($require_clean_worktree == 1),
      worktree_fingerprints: {
        initial_sha256: $worktree_fingerprint_initial_sha256,
        final_sha256: null
      },
      baseline_allowlist: {
        path: (if $allowlist_path == "" then null else $allowlist_path end),
        sha256: (if $allowlist_sha256 == "null" then null else $allowlist_sha256 end),
        labels: []
      },
      dirty_worktree_excerpt: $worktree_lines,
      command_count: 0,
      status_counts: {
        passed: 0,
        pre_existing_known_failure: 0,
        current_failure: 0
      },
      commands: []
    }' > "${manifest_tmp_path}"
  mv "${manifest_tmp_path}" "${manifest_path}"
  ln -sfn "$(basename "${run_dir}")" "${OUTPUT_ROOT}/latest"
  printf '%s\n' "${manifest_path}"
  exit 2
fi

command_entries=()
pass_count=0
pre_existing_count=0
current_failure_count=0

for i in "${!COMMAND_LABELS[@]}"; do
  label="${COMMAND_LABELS[$i]}"
  invocation="${COMMAND_LINES[$i]}"
  safe_label="$(sanitize_label "${label}")"
  stdout_path="${commands_dir}/${safe_label}.stdout"
  stderr_path="${commands_dir}/${safe_label}.stderr"
  summary_path="${commands_dir}/${safe_label}.summary.json"
  invocation_sha256="$(printf '%s' "${invocation}" | sha256sum | awk '{print $1}')"

  set +e
  bash -lc "${invocation}" >"${stdout_path}" 2>"${stderr_path}"
  exit_code=$?
  set -e

  render_summary_json "${label}" "${stdout_path}" "${summary_path}"

  status_class="passed"
  if [[ "${exit_code}" -ne 0 ]]; then
    if allowlisted_label "${label}" "${git_head}" "${invocation_sha256}"; then
      status_class="pre_existing_known_failure"
      pre_existing_count=$((pre_existing_count + 1))
    else
      status_class="current_failure"
      current_failure_count=$((current_failure_count + 1))
    fi
  else
    pass_count=$((pass_count + 1))
  fi

  failure_signature=""
  if [[ "${exit_code}" -ne 0 ]]; then
    failure_signature="$(sed -n '1,20p' "${stderr_path}")"
    if [[ -z "${failure_signature}" ]]; then
      failure_signature="$(sed -n '1,20p' "${stdout_path}")"
    fi
  fi

  entry="$(jq -n \
    --arg label "${label}" \
    --arg invocation "${invocation}" \
    --arg stdout_path "${stdout_path#${run_dir_abs}/}" \
    --arg stderr_path "${stderr_path#${run_dir_abs}/}" \
    --arg summary_path "${summary_path#${run_dir_abs}/}" \
    --arg invocation_sha256 "${invocation_sha256}" \
    --arg stdout_sha256 "$(sha256_file "${stdout_path}")" \
    --arg stderr_sha256 "$(sha256_file "${stderr_path}")" \
    --arg summary_sha256 "$(sha256_file "${summary_path}")" \
    --arg status_class "${status_class}" \
    --arg failure_signature "${failure_signature}" \
    --arg output_kind "$(command_output_kind "${stdout_path}")" \
    --argjson exit_code "${exit_code}" \
    '{
      label: $label,
      invocation: $invocation,
      invocation_sha256: $invocation_sha256,
      exit_code: $exit_code,
      status_class: $status_class,
      output_kind: $output_kind,
      stdout_path: $stdout_path,
      stderr_path: $stderr_path,
      summary_path: $summary_path,
      artifact_checksums: {
        stdout_sha256: $stdout_sha256,
        stderr_sha256: $stderr_sha256,
        summary_sha256: $summary_sha256
      },
      failure_signature: (if $failure_signature == "" then null else $failure_signature end)
    }')"
  command_entries+=("${entry}")
done

overall_status="baseline_captured_green"
if [[ "${current_failure_count}" -gt 0 ]]; then
  overall_status="baseline_captured_with_current_failures"
elif [[ "${pre_existing_count}" -gt 0 ]]; then
  overall_status="baseline_captured_with_allowlisted_failures"
fi

commands_json="$(printf '%s\n' "${command_entries[@]}" | jq -s '.')"
allowlist_snapshot_json='[]'
if [[ -n "${BASELINE_ALLOWLIST}" ]]; then
  allowlist_snapshot_json="$(grep -Ev '^[[:space:]]*(#|$)' "${BASELINE_ALLOWLIST}" | jq -R . | jq -s '.')"
fi

worktree_status_final="$(git status --porcelain=v1 --untracked-files=all || true)"
worktree_fingerprint_final_sha256="$(printf '%s' "${worktree_status_final}" | sha256sum | awk '{print $1}')"

jq -n \
  --arg artifact_version "scientific-queue0-baseline-freeze-v1" \
  --arg queue_code "queue0_preflight_baseline_freeze" \
  --arg status "${overall_status}" \
  --arg repo_root "${REPO_ROOT}" \
  --arg run_dir "${run_dir_abs}" \
  --arg observed_at_epoch_ms "${timestamp_epoch_ms}" \
  --arg git_head "${git_head}" \
  --arg git_branch "${git_branch}" \
  --arg worktree_fingerprint_initial_sha256 "${worktree_fingerprint_initial_sha256}" \
  --arg worktree_fingerprint_final_sha256 "${worktree_fingerprint_final_sha256}" \
  --arg rustc_version "${rustc_version}" \
  --arg cargo_version "${cargo_version}" \
  --arg python_version "${python_version}" \
  --arg uname_kernel "${uname_kernel}" \
  --arg docker_version "${docker_version}" \
  --arg allowlist_path "${BASELINE_ALLOWLIST}" \
  --arg allowlist_sha256 "${allowlist_sha256}" \
  --argjson pass_count "${pass_count}" \
  --argjson pre_existing_count "${pre_existing_count}" \
  --argjson current_failure_count "${current_failure_count}" \
  --argjson commands "${commands_json}" \
  --argjson lockfile_hashes "$(lockfile_hashes_json)" \
  --argjson safe_env_snapshot "$(safe_env_snapshot_json)" \
  --argjson allowlist_snapshot "${allowlist_snapshot_json}" \
  --argjson require_clean_worktree "${REQUIRE_CLEAN_WORKTREE}" \
  '{
    artifact_version: $artifact_version,
    queue_code: $queue_code,
    status: $status,
    observed_at_epoch_ms: ($observed_at_epoch_ms | tonumber),
    repo_root: $repo_root,
    run_dir: $run_dir,
    git_head: $git_head,
    git_branch: $git_branch,
    require_clean_worktree: ($require_clean_worktree == 1),
    worktree_fingerprints: {
      initial_sha256: $worktree_fingerprint_initial_sha256,
      final_sha256: $worktree_fingerprint_final_sha256
    },
    command_count: ($commands | length),
    status_counts: {
      passed: $pass_count,
      pre_existing_known_failure: $pre_existing_count,
      current_failure: $current_failure_count
    },
    environment_snapshot: {
      uname_kernel: (if $uname_kernel == "" then null else $uname_kernel end),
      rustc_version: (if $rustc_version == "" then null else $rustc_version end),
      cargo_version: (if $cargo_version == "" then null else $cargo_version end),
      python_version: (if $python_version == "" then null else $python_version end),
      docker_version: (if $docker_version == "" then null else $docker_version end),
      safe_env_vars: $safe_env_snapshot,
      dependency_hashes: $lockfile_hashes
    },
    baseline_allowlist: {
      path: (if $allowlist_path == "" then null else $allowlist_path end),
      sha256: (if $allowlist_sha256 == "null" then null else $allowlist_sha256 end),
      labels: $allowlist_snapshot
    },
    commands: $commands
  }' > "${manifest_tmp_path}"

mv "${manifest_tmp_path}" "${manifest_path}"

ln -sfn "$(basename "${run_dir}")" "${OUTPUT_ROOT}/latest"

printf '%s\n' "${manifest_path}"

if [[ "${current_failure_count}" -gt 0 ]]; then
  exit 1
fi
