<p align="center">
  <img src="media/amai-extension.png" alt="Amai VS Code Bridge" width="128">
</p>

# Amai VS Code Bridge

Public `VS Code` / `Codium` bridge for opening a fresh chat surface and injecting an
Amai restore prompt through public VS Code commands.

## What It Adds

- `Amai` activity-bar icon;
- `Amai` sidebar view;
- workspace-scoped chat launch commands;
- public `vscode://amai.amai-vscode-bridge/open-clean-chat` bridge for restore flows.

## Published Extension

- OpenVSX: https://open-vsx.org/extension/amai/amai-vscode-bridge

URI shape:

`vscode://amai.amai-vscode-bridge/open-clean-chat?prompt_file=...&result_file=...&repo_root=...&target=sidebar&auto_submit=1`

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

## License

`PolyForm Noncommercial 1.0.0`
