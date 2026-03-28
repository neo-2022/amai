#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

ART_REPO_ROOT="${ART_REPO_ROOT:-/home/art/Art}"
AMAI_REPO_ROOT="${AMAI_REPO_ROOT:-/home/art/agent-memory-index}"
ART_BOOTSTRAP_SCRIPT="${ART_BOOTSTRAP_SCRIPT:-$ART_REPO_ROOT/scripts/tools/amai_art_project_bootstrap.py}"
ART_BOOTSTRAP_FILE="${ART_BOOTSTRAP_FILE:-$AMAI_REPO_ROOT/state/continuity-imports/art/continuity-snapshot-home-art-art.md}"
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
assert node["session_memory_files"] == 0, node
assert "continuity-snapshot-home-art-art.md" in node["bootstrap_summary"]["bootstrap_file"], node
assert all(source["source_kind"] != "continuity_session_memory" for source in node["sources"]), node
PY

./scripts/continuity_startup.sh --project art --namespace continuity > /tmp/amai-art-continuity-startup.txt

grep -q "Amai continuity startup" /tmp/amai-art-continuity-startup.txt
grep -q "Проект: Art (art)" /tmp/amai-art-continuity-startup.txt
grep -q "Namespace continuity: continuity" /tmp/amai-art-continuity-startup.txt
grep -q "Ближайший обязательный следующий шаг:" /tmp/amai-art-continuity-startup.txt
grep -q "Amai continuity migration proof" /tmp/amai-art-continuity-startup.txt
if grep -q "${ART_REPO_ROOT}/\\.codex" /tmp/amai-art-continuity-startup.txt; then
  echo "startup output leaked project-local .codex path" >&2
  exit 1
fi

cargo run --quiet -- context pack \
  --project art \
  --namespace continuity \
  --query "Continuity snapshot" \
  --retrieval-mode local_strict \
  --token-source-kind proof_context_pack \
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
assert any(path.startswith(".amai-continuity/") for path in paths), payload
assert all(
    not path.startswith(".amai-continuity/external-memory-bridge/")
    for path in paths
), payload
PY

cargo run --quiet -- verify continuity \
  --project art \
  --namespace continuity \
  > /tmp/amai-art-continuity-verify.json

python3 - <<'PY'
import json
from pathlib import Path
payload = json.loads(Path("/tmp/amai-art-continuity-verify.json").read_text())
node = payload["continuity_verification"]
canonical_eval = node["canonical_eval"]
assert payload["retrieval_science"]["suite_key"] == "continuity_verification", payload
assert canonical_eval["eval_verdict_model_version"] == "memory-eval-verdict-v1", canonical_eval
assert canonical_eval["verdict_counts"]["recovered_useful"] == 7, canonical_eval
assert canonical_eval["verdict_counts"]["hit_correct_target"] == 2, canonical_eval
assert len(canonical_eval["probes"]) == 9, canonical_eval
assert node["working_state_restore_present"] is True, node
assert node["handoff_summary_source"] == "continuity_handoff", node
assert node["chat_start_restore"]["prompt_text"], node
PY

echo "Amai Art continuity migration proof: PASS"
