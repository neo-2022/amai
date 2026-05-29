#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

TARGET_FILE=""
MODEL="${MODEL:-gemma4:e4b}"
HEAD_LINES="${HEAD_LINES:-120}"
MAX_FN_LINES="${MAX_FN_LINES:-800}"
MIN_COVERAGE_RATIO="${MIN_COVERAGE_RATIO:-0.60}"

usage() {
  cat >&2 <<'EOF'
Usage:
  gemma_monolith_split.sh --file <relative/path.rs> [--model <name>] [--head-lines <n>] [--max-fn-lines <n>]

What it does:
- builds a structured monolith-split prompt
- runs Gemma in split mode
- extracts JSON even if the model wrapped it in fences
- validates schema
- estimates symbol coverage
- retries once with corrective feedback if the first answer is weak
EOF
  exit 2
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --file)
      TARGET_FILE="$2"
      shift 2
      ;;
    --model)
      MODEL="$2"
      shift 2
      ;;
    --head-lines)
      HEAD_LINES="$2"
      shift 2
      ;;
    --max-fn-lines)
      MAX_FN_LINES="$2"
      shift 2
      ;;
    -h|--help)
      usage
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      ;;
  esac
done

if [[ -z "$TARGET_FILE" ]]; then
  echo "Требуется --file" >&2
  usage
fi

ABS_TARGET="$REPO_ROOT/$TARGET_FILE"
if [[ ! -f "$ABS_TARGET" ]]; then
  echo "Файл не найден: $ABS_TARGET" >&2
  exit 2
fi

WORK_DIR="$(mktemp -d)"
cleanup() {
  rm -rf "$WORK_DIR"
}
trap cleanup EXIT

META_JSON="$WORK_DIR/meta.json"
BASE_PROMPT="$WORK_DIR/base_prompt.txt"
RAW1="$WORK_DIR/raw1.json"
RAW2="$WORK_DIR/raw2.json"
PARSED1="$WORK_DIR/parsed1.json"
PARSED2="$WORK_DIR/parsed2.json"
RETRY_PROMPT="$WORK_DIR/retry_prompt.txt"

python3 - "$ABS_TARGET" "$TARGET_FILE" "$HEAD_LINES" "$MAX_FN_LINES" "$META_JSON" "$BASE_PROMPT" <<'PY'
from pathlib import Path
import json
import re
import sys

abs_target = Path(sys.argv[1])
target_file = sys.argv[2]
head_lines = int(sys.argv[3])
max_fn_lines = int(sys.argv[4])
meta_json = Path(sys.argv[5])
base_prompt = Path(sys.argv[6])

text = abs_target.read_text(errors='ignore').splitlines()

head = "\n".join(f"{i+1}: {line}" for i, line in enumerate(text[:head_lines]))

fn_names = []
fn_lines = []
test_names = []
test_lines = []

fn_patterns = ("fn ", "pub fn ", "pub(crate) fn ", "pub(super) fn ")

for i, line in enumerate(text, start=1):
    s = line.strip()
    if s.startswith(fn_patterns):
        fn_lines.append(f"{i}: {s}")
        name = s.split("fn ", 1)[1].split("(", 1)[0].strip()
        fn_names.append(name)
        if "#[test]" in s or re.search(r"(^|_)test(s|_)?", name):
            test_names.append(name)
            test_lines.append(f"{i}: {s}")

if not test_lines:
    for i, line in enumerate(text, start=1):
        s = line.strip()
        if s.startswith("fn "):
            name = s.split("fn ", 1)[1].split("(", 1)[0].strip()
            if re.search(r"(^|_)test(s|_)?", name):
                test_names.append(name)
                test_lines.append(f"{i}: {s}")

fn_inventory = "\n".join(fn_lines[:max_fn_lines])
test_surface = "\n".join(test_lines[:300]) if test_lines else "none_detected"

meta = {
    "target_file": target_file,
    "absolute_path": str(abs_target),
    "total_lines": len(text),
    "function_names": fn_names,
    "test_function_names": sorted(set(test_names)),
}
meta_json.write_text(json.dumps(meta, ensure_ascii=False, indent=2))

prompt = f"""You are performing a real Rust monolith split under strict domain-driven maintainability law.

Target file: {target_file}
Absolute path: {abs_target}
Total lines: {len(text)}

File head:
{head}

Function inventory:
{fn_inventory}

Detected test surface:
{test_surface}

Task:
Propose a realistic domain split for this file.

Return STRICT JSON object with keys:
- summary
- proposed_modules (array of objects with path, purpose)
- symbol_groups (array of objects with module, symbols)
- migration_order (array of strings)
- invariants (array of strings)
- risks (array of strings)
- tests_strategy (array of strings)
- uncovered_symbols (array of strings)
- coverage_confidence
- likely_missed_symbol_groups (array of strings)
- test_only_surface (array of strings)
- shared_cross_module_contracts (array of strings)
- verdict

Rules:
- Split by domain boundaries, invariants, source-of-truth, and policy/runtime/projection separation.
- Do not split by file length alone.
- Cover production symbols as completely as possible.
- If you cannot confidently place part of the symbol surface, list it in uncovered_symbols.
- Explicitly account for test surface migration.
- Output only one JSON object.
- Do not use markdown fences.
- Empty uncovered_symbols is allowed only if coverage is truly high.
"""
base_prompt.write_text(prompt)
PY

run_split() {
  local prompt_file="$1"
  local out_file="$2"
  "$REPO_ROOT/scripts/gemma_code_assist.sh" \
    --model "$MODEL" \
    --mode split \
    --json \
    --prompt "$(cat "$prompt_file")" > "$out_file"
}

extract_and_validate() {
  local raw_file="$1"
  local parsed_file="$2"
  local report_file="$3"
  python3 - "$raw_file" "$parsed_file" "$META_JSON" "$report_file" "$MIN_COVERAGE_RATIO" <<'PY'
from pathlib import Path
import json
import re
import sys

raw_path = Path(sys.argv[1])
parsed_path = Path(sys.argv[2])
meta_path = Path(sys.argv[3])
report_path = Path(sys.argv[4])
min_ratio = float(sys.argv[5])

raw_doc = json.loads(raw_path.read_text())
content = raw_doc["message"]["content"]

match = re.search(r"\{.*\}\s*$", content, re.S)
if not match:
    report = {
        "ok": False,
        "reason": "no_json_object_found",
        "raw_content": content[:2000],
    }
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2))
    sys.exit(0)

candidate = match.group(0)
try:
    parsed = json.loads(candidate)
except Exception as exc:
    report = {
        "ok": False,
        "reason": "json_parse_failed",
        "error": str(exc),
        "candidate": candidate[:4000],
    }
    report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2))
    sys.exit(0)

required = [
    "summary",
    "proposed_modules",
    "symbol_groups",
    "migration_order",
    "invariants",
    "risks",
    "tests_strategy",
    "uncovered_symbols",
    "coverage_confidence",
    "likely_missed_symbol_groups",
    "test_only_surface",
    "shared_cross_module_contracts",
    "verdict",
]
missing = [key for key in required if key not in parsed]

meta = json.loads(meta_path.read_text())
all_functions = set(meta["function_names"])
test_functions = set(meta["test_function_names"])
production_functions = sorted(all_functions - test_functions)

covered = set()
for group in parsed.get("symbol_groups", []):
    if not isinstance(group, dict):
        continue
    for symbol in group.get("symbols", []):
        if isinstance(symbol, str) and symbol in all_functions:
            covered.add(symbol)

uncovered_declared = {
    s for s in parsed.get("uncovered_symbols", [])
    if isinstance(s, str)
}

production_covered = sorted(set(production_functions) & covered)
coverage_ratio = 1.0 if not production_functions else len(production_covered) / len(production_functions)
actual_uncovered = sorted(set(production_functions) - set(production_covered))

weak = []
if missing:
    weak.append(f"missing_keys:{','.join(missing)}")
if coverage_ratio < min_ratio:
    weak.append(f"coverage_ratio_below_threshold:{coverage_ratio:.3f}")
if actual_uncovered and not uncovered_declared:
    weak.append("uncovered_symbols_missing_despite_real_gaps")
if "```" in content:
    weak.append("markdown_fences_present")

parsed_path.write_text(json.dumps(parsed, ensure_ascii=False, indent=2))
report = {
    "ok": not weak,
    "weak_reasons": weak,
    "coverage_ratio": coverage_ratio,
    "production_function_count": len(production_functions),
    "production_covered_count": len(production_covered),
    "actual_uncovered": actual_uncovered[:200],
    "declared_uncovered": sorted(uncovered_declared),
    "missing_keys": missing,
    "raw_had_markdown_fences": "```" in content,
    "content_preview": content[:3000],
}
report_path.write_text(json.dumps(report, ensure_ascii=False, indent=2))
PY
}

REPORT1="$WORK_DIR/report1.json"
REPORT2="$WORK_DIR/report2.json"

run_split "$BASE_PROMPT" "$RAW1"
extract_and_validate "$RAW1" "$PARSED1" "$REPORT1"

if jq -e '.ok == true' "$REPORT1" >/dev/null 2>&1; then
  cat "$PARSED1"
  exit 0
fi

python3 - "$BASE_PROMPT" "$REPORT1" "$RETRY_PROMPT" <<'PY'
from pathlib import Path
import json
import sys

base_prompt = Path(sys.argv[1]).read_text()
report = json.loads(Path(sys.argv[2]).read_text())
retry_path = Path(sys.argv[3])

reasons = "\n".join(f"- {reason}" for reason in report.get("weak_reasons", []))
actual_uncovered = "\n".join(f"- {name}" for name in report.get("actual_uncovered", []))

retry = f"""{base_prompt}

Your previous answer was not accepted.

Validation failures:
{reasons}

Detected uncovered production functions:
{actual_uncovered if actual_uncovered else "- none listed"}

Retry requirements:
- output only one JSON object
- no markdown fences
- fill uncovered_symbols honestly if coverage is incomplete
- keep required keys exactly
- improve coverage of production symbol surface
"""
retry_path.write_text(retry)
PY

run_split "$RETRY_PROMPT" "$RAW2"
extract_and_validate "$RAW2" "$PARSED2" "$REPORT2"

if jq -e '.ok == true' "$REPORT2" >/dev/null 2>&1; then
  cat "$PARSED2"
  exit 0
fi

echo "Gemma split output failed validation after retry." >&2
echo "--- first report ---" >&2
cat "$REPORT1" >&2
echo "--- second report ---" >&2
cat "$REPORT2" >&2
echo "--- second parsed candidate ---" >&2
cat "$PARSED2" >&2 || true
exit 1
