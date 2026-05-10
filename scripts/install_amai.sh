#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

source ./scripts/ensure_verified_linux_prereqs.sh

has_stack_profile=0
skip_stack=0
remote_mode=0
client_target="auto"
for arg in "$@"; do
  case "$arg" in
    --stack-profile|--stack-profile=*)
      has_stack_profile=1
      ;;
    --client)
      client_target="__next__"
      ;;
    --client=*)
      client_target="${arg#*=}"
      ;;
    --skip-stack)
      skip_stack=1
      ;;
    --ssh-destination|--ssh-destination=*)
      remote_mode=1
      ;;
    *)
      if [[ "${client_target}" == "__next__" ]]; then
        client_target="$arg"
      fi
      ;;
  esac
done

require_local_stack_bootstrap_prereqs() {
  if ! command -v docker >/dev/null 2>&1; then
    printf '%s\n' \
      'Amai install requires docker for local stack bootstrap. Install Docker or rerun with --skip-stack / --ssh-destination.' >&2
    exit 127
  fi
  if ! ./scripts/docker_wrapper.sh compose version >/dev/null 2>&1; then
    printf '%s\n' \
      'Amai install requires docker compose v2 for local stack bootstrap. Install the docker compose plugin or rerun with --skip-stack / --ssh-destination.' >&2
    exit 127
  fi
}

if [[ "${remote_mode}" -eq 0 && "${skip_stack}" -eq 0 ]]; then
  ensure_verified_linux_prereqs 1
else
  ensure_verified_linux_prereqs 0
fi

cargo_bin="$(./scripts/resolve_cargo.sh)"
rustc_bin="$(./scripts/resolve_rustc.sh)"
if [[ "${skip_stack}" -eq 0 && "${remote_mode}" -eq 0 ]]; then
  require_local_stack_bootstrap_prereqs
fi

if [[ $has_stack_profile -eq 0 && "${AMAI_NO_INSTALL_PROMPT:-0}" != "1" ]]; then
  if [[ "${AMAI_FORCE_INTERACTIVE_PROMPT:-0}" == "1" || ( -t 0 && -t 1 ) ]]; then
    exec env AMAI_SELECTOR_MODE=install ./scripts/preflight.sh "$@"
  fi
fi

if [[ "${remote_mode}" -eq 0 ]]; then
  normalized_client="$(printf '%s' "${client_target}" | tr '[:upper:]' '[:lower:]')"
  if [[ -z "${normalized_client}" || "${normalized_client}" == "auto" || "${normalized_client}" == "vscode" ]]; then
    exec env \
      RUSTC="${rustc_bin}" \
      CARGO_PROFILE_DEV_DEBUG=0 \
      CARGO_PROFILE_DEV_SPLIT_DEBUGINFO=off \
      "${cargo_bin}" run --quiet --release --bin amai-bootstrap -- install "$@"
  fi
fi

exec env \
  RUSTC="${rustc_bin}" \
  CARGO_PROFILE_DEV_DEBUG=0 \
  CARGO_PROFILE_DEV_SPLIT_DEBUGINFO=off \
  "${cargo_bin}" run --quiet -- bootstrap install --skip-release-build "$@"
