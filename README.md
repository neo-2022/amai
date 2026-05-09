# Amai

`Amai` is a memory and continuity tool for AI agents.
It keeps project context, working state, and restore logic outside a single chat or IDE session.

## Status

Amai is still in development.

At the current stage, the most worked-through and verified client contour is `VS Code`:
- GitHub install is available;
- the `VS Code` install/publish contour has been worked through and verified;
- the `Amai VS Code Bridge` extension is published through `OpenVSX`.

Other clients and applications will be added as the project continues to develop.

## Install

Normal network:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) --client vscode --stack-profile default --yes
```

If `raw.githubusercontent.com` is blocked or unstable, use the git-based one-liner:

```bash
git clone --depth 1 https://github.com/neo-2022/amai.git ~/.local/share/amai/repo && \
cd ~/.local/share/amai/repo && \
./scripts/install_amai.sh --client vscode --stack-profile default --yes
```

## Remove

```bash
~/.local/share/amai/repo/scripts/remove_amai.sh --client vscode
```
