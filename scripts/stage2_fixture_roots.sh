#!/usr/bin/env bash

stage2_prepare_fixture_roots() {
  local repo_root="$1"
  local repo_real
  repo_real="$(cd "$repo_root" && pwd -P)"

  local fixture_base="${AMAI_STAGE2_FIXTURE_ROOT:-}"
  if [[ -z "$fixture_base" ]]; then
    local repo_hash
    repo_hash="$(printf '%s' "$repo_real" | sha256sum | awk '{print substr($1, 1, 12)}')"
    fixture_base="/tmp/amai-stage2-fixtures/${repo_hash}"
  fi

  mkdir -p "$fixture_base"
  local fixture_base_real
  fixture_base_real="$(cd "$fixture_base" && pwd -P)"
  fixture_base_real="$(mktemp -d "${fixture_base_real}/run-XXXXXX")"
  fixture_base_real="$(cd "$fixture_base_real" && pwd -P)"

  case "${fixture_base_real}/" in
    "${repo_real}/"*)
      printf 'stage2 fixture root must be outside repo root: %s\n' "$fixture_base_real" >&2
      return 1
      ;;
  esac

  rm -rf "${fixture_base_real}/project_alpha" "${fixture_base_real}/project_beta"
  cp -a "${repo_real}/fixtures/project_alpha" "${fixture_base_real}/project_alpha"
  cp -a "${repo_real}/fixtures/project_beta" "${fixture_base_real}/project_beta"

  export AMAI_STAGE2_PROJECT_ALPHA_ROOT="${fixture_base_real}/project_alpha"
  export AMAI_STAGE2_PROJECT_BETA_ROOT="${fixture_base_real}/project_beta"
}
