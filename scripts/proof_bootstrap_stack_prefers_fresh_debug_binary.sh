#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temp_root="$(mktemp -d)"
trap 'rm -rf "${temp_root}"' EXIT

sandbox_repo="${temp_root}/repo"
fake_bin="${temp_root}/fake-bin"
mkdir -p "${sandbox_repo}/scripts" "${sandbox_repo}/target/debug" "${sandbox_repo}/state/locks" "${fake_bin}"

copy_into_sandbox() {
  local rel="$1"
  mkdir -p "${sandbox_repo}/$(dirname "${rel}")"
  cp "${repo_root}/${rel}" "${sandbox_repo}/${rel}"
}

copy_into_sandbox "scripts/bootstrap_stack.sh"
copy_into_sandbox "scripts/docker_wrapper.sh"
chmod +x "${sandbox_repo}/scripts/bootstrap_stack.sh"
chmod +x "${sandbox_repo}/scripts/docker_wrapper.sh"

cat >"${sandbox_repo}/scripts/load_env.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
EOF
chmod +x "${sandbox_repo}/scripts/load_env.sh"

cat >"${sandbox_repo}/scripts/prepare_stack_runtime.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'prepare stack runtime\n' >>"${AMAI_TRACE_PATH}"
EOF
chmod +x "${sandbox_repo}/scripts/prepare_stack_runtime.sh"

cat >"${sandbox_repo}/scripts/warmup_cache.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'warmup cache\n' >>"${AMAI_TRACE_PATH}"
EOF
chmod +x "${sandbox_repo}/scripts/warmup_cache.sh"

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

cat >"${sandbox_repo}/target/debug/amai" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"${AMAI_TRACE_PATH}"
EOF
chmod +x "${sandbox_repo}/target/debug/amai"

sleep 1
touch "${sandbox_repo}/target/debug/amai"

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

cat >"${fake_bin}/flock" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
shift 3
"$@"
EOF
chmod +x "${fake_bin}/flock"

trace_path="${temp_root}/trace.txt"

(
  cd "${sandbox_repo}"
  PATH="${fake_bin}:${PATH}" \
  AMAI_TRACE_PATH="${trace_path}" \
  ./scripts/bootstrap_stack.sh --stack-profile default
)

grep -Fxq 'bootstrap preflight --stack-profile default' "${trace_path}"
grep -Fxq 'bootstrap stack' "${trace_path}"
grep -Fq 'docker compose up -d --remove-orphans' "${trace_path}"

if grep -Fq 'cargo unexpectedly ran' "${trace_path}"; then
  printf 'proof_bootstrap_stack_prefers_fresh_debug_binary: cargo unexpectedly ran\n' >&2
  exit 1
fi

printf 'proof_bootstrap_stack_prefers_fresh_debug_binary: PASS\n'
