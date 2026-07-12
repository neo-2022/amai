#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

if [[ ! -f .env ]]; then
  cp .env.example .env
fi

while IFS= read -r line; do
  [[ -z "${line}" ]] && continue
  [[ "${line}" == \#* ]] && continue
  [[ "${line}" != *=* ]] && continue
  key="${line%%=*}"
  if ! grep -q "^${key}=" .env; then
    printf '%s\n' "${line}" >> .env
  fi
done < .env.example

while IFS= read -r line; do
  [[ -z "${line}" ]] && continue
  [[ "${line}" == \#* ]] && continue
  [[ "${line}" != *=* ]] && continue
  key="${line%%=*}"
  value="${line#*=}"
  if [[ -z "${!key+x}" ]]; then
    export "${key}=${value}"
  fi
done < .env

amai_unique_suffix() {
  local epoch_nanos=""
  epoch_nanos="$(date +%s%N 2>/dev/null || true)"
  if [[ -n "${epoch_nanos}" && "${epoch_nanos}" != *N* ]]; then
    printf '%s\n' "${epoch_nanos}"
    return 0
  fi
  printf '%s%s%05d\n' "$(date +%s)" "$$" "${RANDOM}"
}

amai_trim_shell_value() {
  local value="${1-}"
  value="${value#"${value%%[![:space:]]*}"}"
  value="${value%"${value##*[![:space:]]}"}"
  printf '%s' "${value}"
}

resolve_amai_thread_id_from_env() {
  local platform_value legacy_value hermes_value explicit_value
  platform_value="$(amai_trim_shell_value "${AMAI_PLATFORM_THREAD_ID-}")"
  legacy_value="$(amai_trim_shell_value "${CODEX_THREAD_ID-}")"
  hermes_value="$(amai_trim_shell_value "${HERMES_SESSION_ID-}")"
  if [[ -n "${platform_value}" && -n "${legacy_value}" && "${platform_value}" != "${legacy_value}" ]]; then
    printf '%s\n' "conflicting thread identity aliases: AMAI_PLATFORM_THREAD_ID and CODEX_THREAD_ID differ" >&2
    return 2
  fi
  if [[ -n "${platform_value}" ]]; then
    explicit_value="${platform_value}"
  elif [[ -n "${legacy_value}" ]]; then
    explicit_value="${legacy_value}"
  fi
  if [[ -n "${explicit_value-}" ]]; then
    printf '%s\n' "${explicit_value}"
    return 0
  fi
  if [[ -n "${hermes_value}" ]]; then
    printf '%s\n' "${hermes_value}"
    return 0
  fi
  return 1
}

resolve_amai_thread_id_or_empty_from_env() {
  local resolved status errexit_was_set=0
  case "$-" in
    *e*)
      errexit_was_set=1
      set +e
      ;;
  esac
  resolved="$(resolve_amai_thread_id_from_env)"
  status=$?
  if [[ "${errexit_was_set}" -eq 1 ]]; then
    set -e
  fi
  case "${status}" in
    0)
      printf '%s\n' "${resolved}"
      return 0
      ;;
    1)
      return 0
      ;;
    *)
      return "${status}"
      ;;
  esac
}
