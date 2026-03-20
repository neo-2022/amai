#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

ART_REPO_ROOT="${ART_REPO_ROOT:-/home/art/Art}"
ART_BOOTSTRAP_FILE="${ART_BOOTSTRAP_FILE:-$ART_REPO_ROOT/.codex/echovault-project-bootstrap-home-art-art.md}"
ART_ACTIVE_WORKLINE_FILE="${ART_ACTIVE_WORKLINE_FILE:-$ART_REPO_ROOT/.codex/ACTIVE_WORKLINE_ART.md}"
ART_MEMORY_DIR="${ART_MEMORY_DIR:-/home/art/.memory/vault/Art}"

for required in "$ART_REPO_ROOT" "$ART_BOOTSTRAP_FILE" "$ART_ACTIVE_WORKLINE_FILE" "$ART_MEMORY_DIR"; do
  if [[ ! -e "$required" ]]; then
    echo "missing required continuity input: $required" >&2
    exit 1
  fi
done

./scripts/bootstrap_stack.sh

./scripts/import_continuity.sh \
  --project art \
  --display-name Art \
  --repo-root "$ART_REPO_ROOT" \
  --namespace continuity \
  --bootstrap-file "$ART_BOOTSTRAP_FILE" \
  --active-workline-file "$ART_ACTIVE_WORKLINE_FILE" \
  --memory-dir "$ART_MEMORY_DIR" \
  > /tmp/amai-art-continuity-import.json

python3 - <<'PY'
import json
from pathlib import Path
payload = json.loads(Path("/tmp/amai-art-continuity-import.json").read_text())
node = payload["continuity_import"]
assert node["project"]["code"] == "art", node
assert node["namespace"]["code"] == "continuity", node
assert node["documents_imported"] >= 3, node
assert node["session_memory_files"] >= 1, node
assert node["rendered_transcript_files"] >= 1, node
PY

./scripts/continuity_startup.sh --project art --namespace continuity > /tmp/amai-art-continuity-startup.txt

grep -q "Amai continuity startup" /tmp/amai-art-continuity-startup.txt
grep -q "Проект: Art (art)" /tmp/amai-art-continuity-startup.txt
grep -q "Namespace continuity: continuity" /tmp/amai-art-continuity-startup.txt
grep -q "Ближайший обязательный следующий шаг:" /tmp/amai-art-continuity-startup.txt

cargo run --quiet -- context pack \
  --project art \
  --namespace continuity \
  --query "dashboard hardening group" \
  --retrieval-mode local_strict \
  --limit-documents 3 \
  --limit-symbols 0 \
  --limit-chunks 3 \
  --limit-semantic-chunks 0 \
  > /tmp/amai-art-continuity-context-pack.json

python3 - <<'PY'
import json
from pathlib import Path
payload = json.loads(Path("/tmp/amai-art-continuity-context-pack.json").read_text())
assert payload["project"]["code"] == "art", payload
assert payload["namespace"]["code"] == "continuity", payload
retrieval = payload["retrieval"]
assert len(retrieval["exact_documents"]) >= 1 or len(retrieval["lexical_chunks"]) >= 1, payload
PY

echo "Amai Art continuity migration proof: PASS"
