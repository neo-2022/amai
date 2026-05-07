#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temp_root="$(mktemp -d)"
trap 'rm -rf "${temp_root}"' EXIT

sandbox_repo="${temp_root}/repo"
fake_bin="${temp_root}/fake-bin"
mkdir -p "${sandbox_repo}/scripts" "${sandbox_repo}/config/postgres" "${sandbox_repo}/config/nats" "${fake_bin}"

copy_into_sandbox() {
  local rel="$1"
  mkdir -p "${sandbox_repo}/$(dirname "${rel}")"
  cp "${repo_root}/${rel}" "${sandbox_repo}/${rel}"
}

copy_into_sandbox ".env.example"
copy_into_sandbox "compose.yaml"
copy_into_sandbox "scripts/bootstrap_stack.sh"
copy_into_sandbox "scripts/load_env.sh"
copy_into_sandbox "scripts/resolve_cargo.sh"
copy_into_sandbox "scripts/resolve_rustc.sh"
copy_into_sandbox "scripts/render_postgres_config.sh"
copy_into_sandbox "scripts/render_nats_config.sh"
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

cat >"${fake_bin}/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [[ "${1:-}" == "compose" && "${2:-}" == "up" ]]; then
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
    if [[ ! -e "${path}" ]]; then
      printf 'missing required bootstrap artifact before compose up: %s\n' "${path}" >&2
      exit 1
    fi
  done
  exit 0
fi
printf 'unexpected fake docker invocation: %s\n' "$*" >&2
exit 1
EOF
chmod +x "${fake_bin}/docker"

(
  cd "${sandbox_repo}"
  PATH="${fake_bin}:${PATH}" \
  AMAI_SKIP_STACK_PREFLIGHT=1 \
  AMAI_CARGO_BIN="${fake_bin}/cargo" \
  AMAI_RUSTC_BIN="${fake_bin}/rustc" \
  ./scripts/bootstrap_stack.sh
)

echo "proof_bootstrap_volume_dirs: ok"
