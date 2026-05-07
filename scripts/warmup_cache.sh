#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
source ./scripts/load_env.sh

if [[ $# -gt 0 ]]; then
  exec cargo run --release --quiet -- context warm "$@"
fi

if [[ -z "${AMI_WARMUP_PROJECTS:-}" ]]; then
  echo "warmup skipped: AMI_WARMUP_PROJECTS is empty"
  exit 0
fi

mapfile -t registered_projects < <(cargo run --quiet -- project list | awk '{print $1}')
declare -A registered_map=()
for project in "${registered_projects[@]}"; do
  registered_map["$project"]=1
done

warm_projects=()
skipped_projects=()
IFS=',' read -ra requested_projects <<< "${AMI_WARMUP_PROJECTS}"
for raw_project in "${requested_projects[@]}"; do
  project="$(echo "$raw_project" | xargs)"
  if [[ -z "$project" ]]; then
    continue
  fi
  if [[ -n "${registered_map[$project]:-}" ]]; then
    warm_projects+=("$project")
  else
    skipped_projects+=("$project")
  fi
done

if [[ ${#warm_projects[@]} -eq 0 ]]; then
  echo "warmup skipped: no registered projects matched AMI_WARMUP_PROJECTS"
  if [[ ${#skipped_projects[@]} -gt 0 ]]; then
    echo "missing projects: ${skipped_projects[*]}"
  fi
  exit 0
fi

project_csv="$(IFS=,; echo "${warm_projects[*]}")"
args=(
  cargo run --release --quiet -- context warm
  --projects "$project_csv"
  --namespace "${AMI_WARMUP_NAMESPACE}"
  --query "${AMI_WARMUP_QUERY}"
)
if [[ -n "${AMI_WARMUP_RETRIEVAL_MODE:-}" ]]; then
  args+=(--retrieval-mode "${AMI_WARMUP_RETRIEVAL_MODE}")
fi

"${args[@]}"

if [[ ${#skipped_projects[@]} -gt 0 ]]; then
  echo "warmup skipped missing projects: ${skipped_projects[*]}"
fi
