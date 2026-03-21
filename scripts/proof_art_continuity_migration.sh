#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

ART_REPO_ROOT="${ART_REPO_ROOT:-/home/art/Art}"
AMAI_REPO_ROOT="${AMAI_REPO_ROOT:-/home/art/agent-memory-index}"
ART_BOOTSTRAP_SCRIPT="${ART_BOOTSTRAP_SCRIPT:-$ART_REPO_ROOT/scripts/tools/amai_art_project_bootstrap.py}"
ART_BOOTSTRAP_FILE="${ART_BOOTSTRAP_FILE:-$AMAI_REPO_ROOT/state/continuity-imports/art/continuity-snapshot-home-art-art.md}"
ART_LEGACY_NOTES_DIR="${ART_LEGACY_NOTES_DIR:-$ART_REPO_ROOT/.codex}"
ART_MEMORY_DIR="${ART_MEMORY_DIR:-/home/art/.memory/vault/Art}"
ART_INCLUDE_MEMORY_BRIDGE="${ART_INCLUDE_MEMORY_BRIDGE:-0}"

mkdir -p "$(dirname "$ART_BOOTSTRAP_FILE")"
AMAI_REPO_ROOT="$AMAI_REPO_ROOT" \
  python3 "$ART_BOOTSTRAP_SCRIPT" --cwd-prefix "$ART_REPO_ROOT" --write-path "$ART_BOOTSTRAP_FILE" >/dev/null

for required in "$ART_REPO_ROOT" "$ART_BOOTSTRAP_FILE"; do
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
  --transcript-limit 3 \
  > /tmp/amai-art-continuity-import.json

if [[ "$ART_INCLUDE_MEMORY_BRIDGE" == "1" && -d "$ART_MEMORY_DIR" ]]; then
  ./scripts/import_continuity.sh \
    --project art \
    --display-name Art \
    --repo-root "$ART_REPO_ROOT" \
    --namespace continuity \
    --bootstrap-file "$ART_BOOTSTRAP_FILE" \
    --transcript-limit 3 \
    --memory-dir "$ART_MEMORY_DIR" \
    > /tmp/amai-art-continuity-import.json
fi

ART_LEGACY_NOTES_DIR="$ART_LEGACY_NOTES_DIR" \
./scripts/continuity_handoff.sh \
  --project art \
  --namespace continuity \
  --headline "Amai continuity migration proof" \
  --next-step "Убедиться, что startup summary и retrieval уже живут без project .codex"

python3 - <<'PY'
import json
from pathlib import Path
payload = json.loads(Path("/tmp/amai-art-continuity-import.json").read_text())
node = payload["continuity_import"]
assert node["project"]["code"] == "art", node
assert node["namespace"]["code"] == "continuity", node
assert node["documents_imported"] >= 2, node
assert node["rendered_transcript_files"] >= 1, node
assert "continuity-snapshot-home-art-art.md" in node["bootstrap_summary"]["bootstrap_file"], node
PY

./scripts/continuity_startup.sh --project art --namespace continuity > /tmp/amai-art-continuity-startup.txt

grep -q "Amai continuity startup" /tmp/amai-art-continuity-startup.txt
grep -q "Проект: Art (art)" /tmp/amai-art-continuity-startup.txt
grep -q "Namespace continuity: continuity" /tmp/amai-art-continuity-startup.txt
grep -q "Ближайший обязательный следующий шаг:" /tmp/amai-art-continuity-startup.txt
grep -q "Amai continuity migration proof" /tmp/amai-art-continuity-startup.txt

cargo run --quiet -- context pack \
  --project art \
  --namespace continuity \
  --query "Continuity snapshot" \
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
paths = [item["relative_path"] for item in retrieval["exact_documents"]]
paths.extend(item["relative_path"] for item in retrieval["lexical_chunks"])
assert any(
    "continuity-snapshot.md" in path or "live-handoff" in path
    for path in paths
), payload
PY

echo "Amai Art continuity migration proof: PASS"
