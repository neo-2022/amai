#!/usr/bin/env bash
set -euo pipefail

EXT_ROOT="${HOME}/.vscode/extensions"
MANIFEST="$(find "${EXT_ROOT}" -maxdepth 2 -path '*/openai.chatgpt-*-linux-x64/package.json' | sort | tail -n 1)"

if [[ -z "${MANIFEST}" ]]; then
  echo "openai.chatgpt manifest not found" >&2
  exit 1
fi

python3 - <<'PY' "${MANIFEST}"
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
data = json.loads(path.read_text())
activation_events = data.get("activationEvents", [])
data["activationEvents"] = [event for event in activation_events if event != "onStartupFinished"]
path.write_text(json.dumps(data, indent="\t", ensure_ascii=False) + "\n")
print(path)
PY
