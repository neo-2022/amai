# Amai VS Code Bridge

Public VS Code URI bridge that opens a fresh Codex chat surface and injects an
Amai restore prompt through public VS Code commands.

URI shape:

`vscode://amai.amai-vscode-bridge/open-clean-chat?prompt_file=...&result_file=...&repo_root=...&target=sidebar&auto_submit=1`

## What It Adds

- Activity Bar icon `Amai`
- Sidebar view `Amai`
- Commands for opening a workspace-bound clean chat in sidebar or panel
- Public `vscode://amai.amai-vscode-bridge/open-clean-chat` bridge for restore flows

## Packaging

Build a local VSIX:

```bash
./scripts/package_vscode_amai_bridge.sh
```

Fail-closed publish wrappers:

```bash
./scripts/publish_vscode_amai_bridge.sh --target openvsx
./scripts/publish_vscode_amai_bridge.sh --target marketplace
```

These wrappers require explicit publish tokens and intentionally fail if the
token for the selected registry is missing.
