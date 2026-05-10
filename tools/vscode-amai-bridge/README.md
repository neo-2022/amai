<p align="center">
  <img src="media/amai-extension.png" alt="Amai VS Code Bridge" width="128">
</p>

# Amai VS Code Bridge

`Amai VS Code Bridge` adds the public `Amai` surface inside `VS Code` / `Codium`.

It is only a bridge layer.
It does **not** install the full `Amai` application and it does **not** provide the `Codex/OpenAI` chat surface by itself.

You need **both**:

- the local `Amai` install
- the `OpenAI` / `Codex` extension surface inside `VS Code` / `Codium`

After that, this extension adds:

- `Amai` activity-bar icon
- `Amai` sidebar view
- workspace chat launch commands
- public `vscode://amai.amai-vscode-bridge/open-clean-chat` bridge for restore flows

This extension is the `VS Code` surface for `Amai`, not the whole product.

## Verified Scope

The currently verified contour is:

- `Ubuntu` / `Debian`
- `VS Code` or `Codium`
- local GitHub install of `Amai`

Other operating systems and client contours are still in development.

## Prerequisites

Before using this extension, the currently verified contour expects:

- `Ubuntu` or `Debian`
- `bash`
- `sudo` / administrator access for a clean-machine bootstrap
- network access to GitHub and the system package repositories
- `code` CLI from `VS Code` or `Codium`
- `systemd --user` for the managed local `amai-stack.service`
- an installed and enabled `OpenAI` extension surface that exposes the required `Codex/ChatGPT` commands inside `VS Code` / `Codium`

Important:

- on the currently verified `Ubuntu` / `Debian` contour, the GitHub install path can now bootstrap missing local prerequisites for you;
- that includes `git`, base build dependencies, `rustup` / `cargo` / `rustc`, and `Docker` / compose support;
- the extension still does not replace the separate `Amai` application install.

## Before You Install The Extension

Install the `Amai` application first.

Normal network:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) --client vscode --stack-profile default --yes
```

If `raw.githubusercontent.com` is blocked or unstable:

```bash
rm -rf ~/.local/share/amai/repo && \
git clone --depth 1 https://github.com/neo-2022/amai.git ~/.local/share/amai/repo && \
cd ~/.local/share/amai/repo && \
./scripts/install_amai.sh --client vscode --stack-profile default --yes
```

That install materializes the local Amai repo, the stack bootstrap contour, the MCP config surface, and the `VS Code` bridge bundle.

Then make sure your editor also has the `OpenAI` / `Codex` chat surface available.
Without it, the `Amai` sidebar buttons cannot open the target chat workspace and the bridge will fail with a readiness error.

## Fastest Agent-Assisted Path

If you already use an AI coding agent in `VS Code` / `Codium`, the fastest path is:

1. install `Amai` with one of the commands above;
2. open `~/.local/share/amai/repo`;
3. ask the agent to verify:
   - `.vscode/mcp.json` exists;
   - `systemctl --user is-active amai-stack.service` returns `active`;
   - `Amai VS Code Bridge` is installed;
   - the `OpenAI` / `Codex` chat surface is present for sidebar launch.

This does not replace the normal install.
It is only the quickest way to let an agent finish the editor-side checks and obvious post-install fixes.

## Install The Extension

You can install it from the `Extensions` view in `VS Code` / `Codium` by searching for:

`Amai VS Code Bridge`

Published extension:

- OpenVSX: https://open-vsx.org/extension/amai/amai-vscode-bridge

If you prefer CLI install:

```bash
code --install-extension amai.amai-vscode-bridge --force
```

## Required Chat Surface

The bridge currently launches the target workspace through the `OpenAI` / `Codex` extension commands inside `VS Code` / `Codium`.

That means:

- installing `Amai VS Code Bridge` alone is not enough
- installing the `Amai` application alone is not enough
- both the `Amai` application and the `OpenAI` / `Codex` chat surface must be present

If the `OpenAI` / `Codex` commands are missing, the sidebar will now show that readiness state directly instead of exposing a blind launch button.

## How To Open Amai In VS Code

1. Open `~/.local/share/amai/repo` in `VS Code` / `Codium`.
2. Reload the window once after install.
3. Make sure the `OpenAI` extension is installed and enabled in the same editor profile.
4. Click the `Amai` icon in the activity bar.
5. Use one of these actions:
   - `Open in Sidebar`
   - `Open in Panel`

The sidebar now also exposes helper actions for the exact next steps:

- `Open Amai Workspace`
- `Reload Window`
- `Open OpenAI Extension`

If one of the required parts is still missing, the sidebar should show the missing step directly instead of pretending the launch buttons are ready.

Available commands:

- `Amai: Open Clean Codex Chat`
- `Amai: Open Workspace Chat in Sidebar`
- `Amai: Open Workspace Chat in Panel`
- `Amai: Focus Sidebar`

## How To Verify It Connected

After install, the expected local contour is:

- local repo exists at `~/.local/share/amai/repo`
- workspace file `.vscode/mcp.json` exists
- `amai-stack.service` is active
- the `Amai` activity-bar icon is visible

Note:

- the current verified local contour expects a working `systemd --user` environment
- if your Linux setup does not use `systemd --user`, the extension page is not claiming that contour as verified yet

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

### The `Amai` icon does not appear

- Reload the `VS Code` / `Codium` window.
- If needed, fully close the client and open it again.

### The extension is installed, but Amai does not connect

Check:

- `~/.local/share/amai/repo` exists
- `.vscode/mcp.json` exists in the workspace
- `systemctl --user is-active amai-stack.service` returns `active`

### The button says Codex/OpenAI is missing

The current bridge launch path depends on the `OpenAI` / `Codex` chat commands inside `VS Code` / `Codium`.

Do this:

- install or enable the `OpenAI` extension in the editor
- reload the window
- open the `Amai` sidebar again

If those commands are still unavailable, the bridge should not be treated as ready on that editor profile yet.

### Install fails with stale `ami-*` container conflicts

The current public install contour is designed to reclaim stale conflicting `ami-*` containers from another Amai repo root automatically.

If you still hit a stale-stack failure, capture the exact install output and inspect:

```bash
systemctl --user status amai-stack.service
```

```bash
journalctl --user -u amai-stack.service -n 120 --no-pager
```

### Remove Amai completely

```bash
~/.local/share/amai/repo/scripts/remove_amai.sh --client vscode
```

## URI Shape

`vscode://amai.amai-vscode-bridge/open-clean-chat?prompt_file=...&result_file=...&repo_root=...&target=sidebar&auto_submit=1`

## License

`PolyForm Noncommercial 1.0.0`
