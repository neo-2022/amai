<p align="center">
  <img src="media/amai-extension.png" alt="Amai VS Code Bridge" width="128">
</p>

# Amai VS Code Bridge

`Amai VS Code Bridge` adds the `Amai` sidebar and URI bridge inside `VS Code` / `Codium`.

This extension is a bridge layer only. It does **not** install full `Amai`.

## Prerequisites

Install `Amai` first:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) --client vscode --stack-profile default --yes
```

If `raw.githubusercontent.com` is blocked:

```bash
clone_dir="${HOME}/.local/share/amai/repo" && \
if [ -d "${clone_dir}/.git" ]; then
  git -C "${clone_dir}" fetch --depth 1 origin && git -C "${clone_dir}" checkout --force main && git -C "${clone_dir}" reset --hard origin/main
else
  git clone --depth 1 https://github.com/neo-2022/amai.git "${clone_dir}"
fi && \
"${clone_dir}/scripts/install_amai.sh" --client vscode --stack-profile default --yes
```

The install creates MCP config and installs this bridge bundle.

## Install Extension

From `Extensions` search:

`Amai VS Code Bridge`

Published extension:

- OpenVSX: https://open-vsx.org/extension/amai/amai-vscode-bridge

CLI install:

```bash
code --install-extension amai.amai-vscode-bridge --force
```

## Use In VS Code

1. Open any project in `VS Code` / `Codium`.
2. Reload window once after install.
3. Click `Amai` icon in activity bar.
4. Use one of these actions:
   - `Open in Sidebar`
   - `Open in Panel`

Helper actions in sidebar:

- `Open Amai Repo`
- `Reload Window`
- `Open Chat Extension`

If a required part is missing, sidebar shows readiness hint.

Available commands:

- `Amai: Open Clean Chat`
- `Amai: Open Workspace Chat in Sidebar`
- `Amai: Open Workspace Chat in Panel`
- `Amai: Focus Sidebar`

## Verify

Expected local contour:

- local repo exists at `~/.local/share/amai/repo`
- MCP config exists (`~/.config/Code/User/mcp.json` and/or `.vscode/mcp.json`)
- `amai-stack.service` is active
- `Amai` activity-bar icon is visible

Useful checks:

```bash
cd ~/.local/share/amai/repo && ./scripts/status.sh
```

```bash
systemctl --user is-active amai-stack.service
```

```bash
code --list-extensions --show-versions | grep -F amai.amai-vscode-bridge
```

## Troubleshooting

### Icon is missing

- Reload `VS Code` / `Codium` window.
- If needed, fully restart the client.

### Extension installed, but no connection

Check:

- `~/.local/share/amai/repo` exists
- `~/.config/Code/User/mcp.json` or `~/.config/VSCodium/User/mcp.json` contains `amai`
- `systemctl --user is-active amai-stack.service` returns `active`

### Sidebar says chat extension is missing

Bridge launch depends on chat commands from your installed chat extension.

Do this:

- install or enable your chat extension in this editor profile
- reload the window
- open the `Amai` sidebar again

### Remove Amai completely

```bash
~/.local/share/amai/repo/scripts/remove_amai.sh --client vscode
```

## URI

`vscode://amai.amai-vscode-bridge/open-clean-chat?prompt_file=...&result_file=...&repo_root=...&target=sidebar&auto_submit=1`

## License

`PolyForm Noncommercial 1.0.0`
