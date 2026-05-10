#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temp_root="$(mktemp -d)"
trap 'rm -rf "${temp_root}"' EXIT

sandbox_repo="${temp_root}/repo"
fake_bin="${temp_root}/home/.local/bin"
mkdir -p "${sandbox_repo}/scripts" "${sandbox_repo}/target/debug" "${sandbox_repo}/state/locks" "${fake_bin}"

copy_into_sandbox() {
  local rel="$1"
  mkdir -p "${sandbox_repo}/$(dirname "${rel}")"
  cp "${repo_root}/${rel}" "${sandbox_repo}/${rel}"
}

copy_into_sandbox "scripts/run_stack_service.sh"
copy_into_sandbox "scripts/docker_wrapper.sh"
chmod +x "${sandbox_repo}/scripts/run_stack_service.sh"
chmod +x "${sandbox_repo}/scripts/docker_wrapper.sh"

cat >"${sandbox_repo}/scripts/load_env.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
EOF
chmod +x "${sandbox_repo}/scripts/load_env.sh"

cat >"${sandbox_repo}/scripts/resolve_cargo.sh" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' cargo
EOF
chmod +x "${sandbox_repo}/scripts/resolve_cargo.sh"

cat >"${sandbox_repo}/scripts/resolve_rustc.sh" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' rustc
EOF
chmod +x "${sandbox_repo}/scripts/resolve_rustc.sh"

cat >"${sandbox_repo}/scripts/prepare_stack_runtime.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'prepare stack runtime\n' >>"${AMAI_TRACE_PATH}"
EOF
chmod +x "${sandbox_repo}/scripts/prepare_stack_runtime.sh"

cat >"${sandbox_repo}/target/debug/amai-bootstrap" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${AMAI_TRACE_PATH}"
EOF
chmod +x "${sandbox_repo}/target/debug/amai-bootstrap"

sleep 1
touch "${sandbox_repo}/target/debug/amai-bootstrap"

cat >"${fake_bin}/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'cargo unexpectedly ran: %s\n' "$*" >&2
exit 91
EOF
chmod +x "${fake_bin}/cargo"

cat >"${fake_bin}/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'docker %s\n' "$*" >>"${AMAI_TRACE_PATH}"
EOF
chmod +x "${fake_bin}/docker"

trace_path="${temp_root}/trace.txt"

(
  cd "${sandbox_repo}"
  HOME="${temp_root}/home" \
  PATH="${fake_bin}:${PATH}" \
  AMAI_TRACE_PATH="${trace_path}" \
  ./scripts/run_stack_service.sh
)

grep -Fq 'prepare stack runtime' "${trace_path}"
grep -Fq 'docker compose up -d --remove-orphans' "${trace_path}"
grep -Fxq 'stack' "${trace_path}"

if grep -Fq 'cargo unexpectedly ran' "${trace_path}"; then
  printf 'proof_run_stack_service_prefers_compact_debug_binary: cargo unexpectedly ran\n' >&2
  exit 1
fi

printf 'proof_run_stack_service_prefers_compact_debug_binary: PASS\n'
