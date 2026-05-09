# Amai

`Amai` is a memory and continuity tool for AI agents.
It keeps project context, working state, and restore logic outside a single chat or IDE session.

## Status

Amai is still in development.

At the current stage, the verified contour is strictly limited to:
- `Linux` / `Ubuntu`-style install and run;
- `VS Code` / `Codium` client usage on that Linux contour;
- the `Amai VS Code Bridge` extension published through `OpenVSX`.

Other operating systems, clients, and applications will be added and verified as the project continues to develop.

## Install

Verified install contour right now: `Linux` / `Ubuntu`-style shell environment with `VS Code` or `Codium`.

This does not currently claim verified support for `macOS`, `Windows`, or other client/runtime combinations.

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
