#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temp_root="$(mktemp -d)"
trap 'rm -rf "${temp_root}"' EXIT

sandbox_repo="${temp_root}/repo"
fake_bin="${temp_root}/fake-bin"
fake_home="${temp_root}/home"
mkdir -p "${sandbox_repo}/scripts" "${sandbox_repo}/config/postgres" "${sandbox_repo}/config/nats" "${fake_bin}" "${fake_home}/.local/bin" "${fake_home}/.cargo/bin"

copy_into_sandbox() {
  local rel="$1"
  mkdir -p "${sandbox_repo}/$(dirname "${rel}")"
  cp "${repo_root}/${rel}" "${sandbox_repo}/${rel}"
}

copy_into_sandbox ".env.example"
copy_into_sandbox "compose.yaml"
copy_into_sandbox "scripts/bootstrap_stack.sh"
copy_into_sandbox "scripts/run_stack_service.sh"
copy_into_sandbox "scripts/prepare_stack_runtime.sh"
copy_into_sandbox "scripts/load_env.sh"
copy_into_sandbox "scripts/resolve_cargo.sh"
copy_into_sandbox "scripts/resolve_rustc.sh"
copy_into_sandbox "scripts/render_postgres_config.sh"
copy_into_sandbox "scripts/render_nats_config.sh"
copy_into_sandbox "scripts/amai_exec.sh"
copy_into_sandbox "config/postgres/postgresql.conf.tpl"
copy_into_sandbox "config/postgres/pg_hba.conf.tpl"
copy_into_sandbox "config/nats/server.conf.tpl"

cat >"${fake_bin}/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "--version" ]]; then
  printf 'cargo 1.0.0-fake\n'
  exit 0
fi
if [[ "${1:-}" == "run" ]]; then
  exit 0
fi
printf 'unexpected fake cargo invocation: %s\n' "$*" >&2
exit 1
EOF
chmod +x "${fake_bin}/cargo"
cp "${fake_bin}/cargo" "${fake_home}/.cargo/bin/cargo"

cat >"${fake_bin}/rustc" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "-vV" ]]; then
  printf 'rustc 1.0.0-fake\n'
  exit 0
fi
printf 'unexpected fake rustc invocation: %s\n' "$*" >&2
exit 1
EOF
chmod +x "${fake_bin}/rustc"
cp "${fake_bin}/rustc" "${fake_home}/.cargo/bin/rustc"

cat >"${fake_bin}/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
log_file="${FAKE_DOCKER_LOG:?}"
if [[ "${1:-}" == "inspect" ]]; then
  case "${2:-}" in
    ami-postgres)
      printf 'false bind|/tmp/missing-foreign-postgres;\n'
      exit 0
      ;;
    ami-minio)
      printf 'true bind|%s;\n' "${FAKE_FOREIGN_MINIO_PATH:?}"
      exit 0
      ;;
    ami-qdrant|ami-nats|ami-prometheus|ami-grafana)
      exit 1
      ;;
  esac
fi
if [[ "${1:-}" == "rm" && "${2:-}" == "-f" ]]; then
  printf 'rm %s\n' "${3:-}" >>"${log_file}"
  exit 0
fi
if [[ "${1:-}" == "compose" && "${2:-}" == "up" ]]; then
  printf 'compose up\n' >>"${log_file}"
  for path in \
    state/postgres \
    state/qdrant \
    state/minio \
    state/nats \
    tmp/postgres \
    tmp/nats \
    tmp/postgres/postgresql.conf \
    tmp/postgres/pg_hba.conf \
    tmp/nats/server.conf
  do
    [[ -e "${path}" ]] || {
      printf 'missing required bootstrap artifact before compose up: %s\n' "${path}" >&2
      exit 1
    }
  done
  exit 0
fi
printf 'unexpected fake docker invocation: %s\n' "$*" >&2
exit 1
EOF
chmod +x "${fake_bin}/docker"
cp "${fake_bin}/docker" "${fake_home}/.local/bin/docker"
foreign_minio_path="${temp_root}/existing-foreign-minio"
mkdir -p "${foreign_minio_path}"

cat >"${sandbox_repo}/scripts/amai_exec.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "bootstrap" && "${2:-}" == "stack" ]]; then
  exit 0
fi
printf 'unexpected fake amai_exec invocation: %s\n' "$*" >&2
exit 1
EOF
chmod +x "${sandbox_repo}/scripts/amai_exec.sh"

fake_log="${temp_root}/fake-docker.log"
(
  cd "${sandbox_repo}"
  HOME="${fake_home}" \
  PATH="${fake_bin}:${PATH}" \
  FAKE_DOCKER_LOG="${fake_log}" \
  FAKE_FOREIGN_MINIO_PATH="${foreign_minio_path}" \
  AMAI_SKIP_STACK_PREFLIGHT=1 \
  AMAI_CARGO_BIN="${fake_bin}/cargo" \
  AMAI_RUSTC_BIN="${fake_bin}/rustc" \
  ./scripts/bootstrap_stack.sh
)
grep -Fx 'rm ami-postgres' "${fake_log}" >/dev/null
grep -Fx 'rm ami-minio' "${fake_log}" >/dev/null
grep -Fx 'compose up' "${fake_log}" >/dev/null

rm -f "${fake_log}"
(
  cd "${sandbox_repo}"
  HOME="${fake_home}" \
  PATH="${fake_bin}:${PATH}" \
  FAKE_DOCKER_LOG="${fake_log}" \
  FAKE_FOREIGN_MINIO_PATH="${foreign_minio_path}" \
  ./scripts/run_stack_service.sh
)
grep -Fx 'rm ami-postgres' "${fake_log}" >/dev/null
grep -Fx 'rm ami-minio' "${fake_log}" >/dev/null
grep -Fx 'compose up' "${fake_log}" >/dev/null

echo "proof_stack_runtime_prepare_shared: ok"
