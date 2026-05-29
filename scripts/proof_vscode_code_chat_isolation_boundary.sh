#!/usr/bin/env bash
set -euo pipefail

chat_help="$(code chat --help 2>&1 || true)"
if [[ -z "${chat_help}" ]]; then
  echo "proof_vscode_code_chat_isolation_boundary: unable to read code chat --help" >&2
  exit 1
fi

grep -Fq -- "--profile <profileName>" <<<"${chat_help}"
grep -Fq -- "--new-window" <<<"${chat_help}"

if grep -Fq -- "--user-data-dir" <<<"${chat_help}"; then
  echo "proof_vscode_code_chat_isolation_boundary: code chat now surfaces --user-data-dir; boundary proof is stale" >&2
  exit 1
fi

if grep -Fq -- "--extensions-dir" <<<"${chat_help}"; then
  echo "proof_vscode_code_chat_isolation_boundary: code chat now surfaces --extensions-dir; boundary proof is stale" >&2
  exit 1
fi

printf 'proof_vscode_code_chat_isolation_boundary: ok\n'
