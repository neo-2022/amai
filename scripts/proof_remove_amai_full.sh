#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

temp_home="${tmp_dir}/home"
temp_repo="${tmp_dir}/repo"
temp_bin="${tmp_dir}/bin"
temp_output="${tmp_dir}/mcp.json"
mkdir -p "${temp_home}" "${temp_repo}/scripts" "${temp_repo}/config" "${temp_bin}"

cat >"${temp_repo}/Cargo.toml" <<'EOF'
[package]
name = "amai-proof-remove"
version = "0.0.0"
edition = "2021"
EOF
cat >"${temp_repo}/compose.yaml" <<'EOF'
services: {}
EOF
cat >"${temp_repo}/scripts/run_mcp_stdio.sh" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "${temp_repo}/scripts/run_mcp_stdio.sh"
cp "${repo_root}/config/client_targets.toml" "${temp_repo}/config/client_targets.toml"

mkdir -p "${temp_repo}/state/postgres/pgdata" "${temp_repo}/tmp/nats"
printf 'payload\n' > "${temp_repo}/state/postgres/pgdata/PG_VERSION"
printf 'nats\n' > "${temp_repo}/tmp/nats/server.conf"

mkdir -p "${temp_repo}/.github/instructions" "${temp_repo}/.amai/onboarding"
cat >"${temp_repo}/.github/instructions/amai-continuity-startup.instructions.md" <<'EOF'
<!-- AMAI MANAGED STARTUP INSTRUCTIONS v1 -->
managed startup
<!-- /AMAI MANAGED STARTUP INSTRUCTIONS v1 -->
EOF
cat >"${temp_repo}/.amai/onboarding/project-chat-startup-contract.json" <<'EOF'
{"artifact_version":"workspace-startup-contract-v1"}
EOF

cat >"${temp_repo}/state/install_state.json" <<EOF
{
  "package_version": "0.1.0",
  "repo_revision": "proof",
  "client_key": "vscode",
  "client_config": "${temp_output}",
  "stack_profile": "default",
  "installed_at_epoch_seconds": 1,
  "startup_instruction_path": "${temp_repo}/.github/instructions/amai-continuity-startup.instructions.md",
  "startup_instruction_status": "managed_workspace_instruction_installed",
  "startup_contract_path": "${temp_repo}/.amai/onboarding/project-chat-startup-contract.json",
  "startup_contract_status": "workspace_startup_contract_materialized",
  "startup_contract_sha256": "218c603815692422ef3fd648b7672acae69eea587e3ba23cf5c75d6fb481f1da"
}
EOF

cat >"${temp_output}" <<'EOF'
{
  "servers": {
    "amai": {
      "type": "stdio",
      "command": "amai"
    }
  }
}
EOF

mkdir -p "${temp_home}/.config/systemd/user" "${temp_home}/.vscode/extensions" "${temp_home}/.vscode-oss/extensions"
printf '[Unit]\nDescription=Amai\n' > "${temp_home}/.config/systemd/user/amai-stack.service"
mkdir -p "${temp_home}/.vscode/extensions/amai.amai-vscode-bridge-0.0.3"
printf '{}' > "${temp_home}/.vscode/extensions/amai.amai-vscode-bridge-0.0.3/package.json"
mkdir -p "${temp_home}/.vscode/extensions/art-local.amai-vscode-bridge-0.0.2"
printf '{}' > "${temp_home}/.vscode/extensions/art-local.amai-vscode-bridge-0.0.2/package.json"
mkdir -p "${temp_home}/.vscode-oss/extensions/amai.amai-vscode-bridge-0.0.3"
printf '{}' > "${temp_home}/.vscode-oss/extensions/amai.amai-vscode-bridge-0.0.3/package.json"
cat >"${temp_home}/.vscode/extensions/extensions.json" <<'EOF'
[
  {
    "identifier": { "id": "amai.amai-vscode-bridge" },
    "version": "0.0.3",
    "relativeLocation": "amai.amai-vscode-bridge-0.0.3",
    "location": {
      "path": "/tmp/placeholder",
      "fsPath": "/tmp/placeholder",
      "external": "file:///tmp/placeholder",
      "scheme": "file"
    }
  },
  {
    "identifier": { "id": "art-local.amai-vscode-bridge" },
    "version": "0.0.2",
    "relativeLocation": "art-local.amai-vscode-bridge-0.0.2",
    "location": {
      "path": "/tmp/placeholder-old",
      "fsPath": "/tmp/placeholder-old",
      "external": "file:///tmp/placeholder-old",
      "scheme": "file"
    }
  }
]
EOF
cat >"${temp_home}/.vscode-oss/extensions/extensions.json" <<'EOF'
[
  {
    "identifier": { "id": "amai.amai-vscode-bridge" },
    "version": "0.0.3",
    "relativeLocation": "amai.amai-vscode-bridge-0.0.3",
    "location": {
      "$mid": 1,
      "fsPath": "/tmp/fake-vscode-oss/amai.amai-vscode-bridge-0.0.3",
      "external": "file:///tmp/fake-vscode-oss/amai.amai-vscode-bridge-0.0.3",
      "path": "/tmp/fake-vscode-oss/amai.amai-vscode-bridge-0.0.3",
      "scheme": "file"
    }
  }
]
EOF

cat >"${temp_bin}/docker" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'docker %s\n' "$*" >> "${AMAI_PROOF_LOG}"
exit 0
EOF
cat >"${temp_bin}/systemctl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'systemctl %s\n' "$*" >> "${AMAI_PROOF_LOG}"
if [[ "${1:-}" == "--user" && "${2:-}" == "is-active" ]]; then
  printf 'inactive\n'
fi
exit 0
EOF
cat >"${temp_bin}/podman" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'podman %s\n' "$*" >> "${AMAI_PROOF_LOG}"
if [[ "${1:-}" == "unshare" && "${2:-}" == "rm" ]]; then
  shift 3
  rm -rf "$@"
  exit 0
fi
exit 0
EOF
chmod +x "${temp_bin}/docker" "${temp_bin}/systemctl" "${temp_bin}/podman"

proof_log="${tmp_dir}/proof.log"
touch "${proof_log}"
RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"

HOME="${temp_home}" \
PATH="${temp_bin}:$PATH" \
AMAI_PROOF_LOG="${proof_log}" \
AMAI_GITHUB_CLONE_DIR="${temp_repo}" \
AMAI_BOOTSTRAP_REMOVE_MODE=full \
AMAI_EXEC_FORCE_CARGO=0 \
RUSTUP_HOME="${RUSTUP_HOME}" \
CARGO_HOME="${CARGO_HOME}" \
./scripts/remove_amai.sh \
  --cwd "${temp_repo}" \
  --client vscode \
  --output "${temp_output}" \
  > "${tmp_dir}/remove.out"

rg '^disconnect completed$' "${tmp_dir}/remove.out" >/dev/null
rg '^server_removed: true$' "${tmp_dir}/remove.out" >/dev/null
rg '^startup_instruction_removed: true$' "${tmp_dir}/remove.out" >/dev/null
rg '^full_remove: true$' "${tmp_dir}/remove.out" >/dev/null
rg '^systemd_user_unit_removed: true$' "${tmp_dir}/remove.out" >/dev/null
rg '^stack_down_succeeded: true$' "${tmp_dir}/remove.out" >/dev/null
rg '^state_tree_removed: true$' "${tmp_dir}/remove.out" >/dev/null
rg '^install_state_removed: true$' "${tmp_dir}/remove.out" >/dev/null
rg '^repo_root_removed: true$' "${tmp_dir}/remove.out" >/dev/null
rg '^vscode_bridge_removed: true$' "${tmp_dir}/remove.out" >/dev/null

test ! -e "${temp_repo}"
test ! -e "${temp_home}/.config/systemd/user/amai-stack.service"
test ! -e "${temp_home}/.vscode/extensions/amai.amai-vscode-bridge-0.0.3"
test ! -e "${temp_home}/.vscode/extensions/art-local.amai-vscode-bridge-0.0.2"
test ! -e "${temp_home}/.vscode-oss/extensions/amai.amai-vscode-bridge-0.0.3"
jq -e 'map(.identifier.id) | index("amai.amai-vscode-bridge") == null and index("art-local.amai-vscode-bridge") == null' \
  "${temp_home}/.vscode/extensions/extensions.json" >/dev/null
jq -e 'map(.identifier.id) | index("amai.amai-vscode-bridge") == null and index("art-local.amai-vscode-bridge") == null' \
  "${temp_home}/.vscode-oss/extensions/extensions.json" >/dev/null

rg '^docker compose --profile monitoring down --remove-orphans --volumes$' "${proof_log}" >/dev/null
rg '^systemctl --user disable --now amai-stack.service$' "${proof_log}" >/dev/null
rg '^systemctl --user daemon-reload$' "${proof_log}" >/dev/null

echo "proof_remove_amai_full: PASS"
