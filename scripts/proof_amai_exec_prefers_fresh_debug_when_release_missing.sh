#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
temp_root="$(mktemp -d)"
trap 'rm -rf "${temp_root}"' EXIT

sandbox_repo="${temp_root}/repo"
fake_bin="${temp_root}/fake-bin"
mkdir -p "${sandbox_repo}/scripts" "${sandbox_repo}/target/debug" "${fake_bin}"

copy_into_sandbox() {
  local rel="$1"
  mkdir -p "${sandbox_repo}/$(dirname "${rel}")"
  cp "${repo_root}/${rel}" "${sandbox_repo}/${rel}"
}

copy_into_sandbox "scripts/amai_exec.sh"
chmod +x "${sandbox_repo}/scripts/amai_exec.sh"

cat >"${sandbox_repo}/scripts/run_stack_service.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'helper script newer than debug binary should not force cargo build\n'
EOF
chmod +x "${sandbox_repo}/scripts/run_stack_service.sh"

cat >"${sandbox_repo}/target/debug/amai" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >"${AMAI_DEBUG_ARGS_PATH}"
EOF
chmod +x "${sandbox_repo}/target/debug/amai"

sleep 1
touch "${sandbox_repo}/target/debug/amai"
sleep 1
touch "${sandbox_repo}/scripts/run_stack_service.sh"

cat >"${fake_bin}/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >"${AMAI_CARGO_ARGS_PATH}"
printf 'cargo should not run when debug binary is fresh and release is missing\n' >&2
exit 91
EOF
chmod +x "${fake_bin}/cargo"

debug_args_path="${temp_root}/debug_args.txt"
cargo_args_path="${temp_root}/cargo_args.txt"

(
  cd "${sandbox_repo}"
  PATH="${fake_bin}:${PATH}" \
  AMAI_DEBUG_ARGS_PATH="${debug_args_path}" \
  AMAI_CARGO_ARGS_PATH="${cargo_args_path}" \
  ./scripts/amai_exec.sh bootstrap stack
)

if [[ -e "${cargo_args_path}" ]]; then
  printf 'proof_amai_exec_prefers_fresh_debug_when_release_missing: cargo unexpectedly ran\n' >&2
  cat "${cargo_args_path}" >&2
  exit 1
fi

grep -Fxq 'bootstrap stack' "${debug_args_path}"

printf 'proof_amai_exec_prefers_fresh_debug_when_release_missing: PASS\n'
