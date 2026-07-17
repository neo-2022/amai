#!/usr/bin/env bash
# Amai managed Kimi Code SessionStart hook.
# Reads the Kimi Code hook event JSON from stdin and runs the canonical Amai
# continuity startup bound to the exact Kimi session ID. Observation-only by
# contract: this script always exits 0 so a broken startup can never block the
# user's session (Kimi Code hooks are fail-open by design).
set -u

payload="$(cat)"
session_id="$(printf '%s' "${payload}" | sed -n 's/.*"session_id"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
if [ -z "${session_id}" ]; then
  exit 0
fi
if [ -d "/home/art/agent-memory-index" ]; then
  AMAI_PLATFORM_THREAD_ID="${session_id}" "/home/art/agent-memory-index/scripts/continuity_startup.sh" \
    --repo-root "/home/art/agent-memory-index" \
    --namespace continuity \
    --json >/dev/null 2>&1 || true
fi
exit 0
