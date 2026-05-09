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

Verified install contour right now: `Linux` shell environment, verified on an `Ubuntu`-style machine, with `VS Code` or `Codium`.

This does not currently claim verified support for `macOS`, `Windows`, or other client/runtime combinations.

### Install variants

There are currently two verified GitHub install front doors for this Linux contour:
- `curl` bootstrap for normal network conditions;
- `git clone` bootstrap if `raw.githubusercontent.com` is blocked or unstable.

### System requirements

Current verified baseline:
- `Linux` shell environment, with the current live proof performed on an `Ubuntu`-style machine;
- `bash`, `git`, and either `curl` or direct `git` access to GitHub;
- `rustup` / `cargo` / `rustc`;
- `Docker` and `Docker Compose v2` for local stack bootstrap;
- `VS Code` or `Codium` for the verified client contour.

Machine capacity is checked by the built-in preflight selector:

```bash
./scripts/preflight.sh
```

It evaluates the machine against the currently supported install profiles and shows whether `default` or `lite_vps` is a realistic fit before installation.

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
