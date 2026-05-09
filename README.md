<p align="center">
  <img src="brand/amai_lockup.svg" alt="Amai" width="360">
</p>

<p align="center">
  Memory and continuity for AI agents.
</p>

<p align="center">
  <a href="https://github.com/neo-2022/amai"><img alt="Repo" src="https://img.shields.io/badge/repo-GitHub-181717"></a>
  <a href="https://open-vsx.org/extension/amai/amai-vscode-bridge"><img alt="OpenVSX" src="https://img.shields.io/open-vsx/v/amai/amai-vscode-bridge?label=OpenVSX"></a>
  <img alt="License" src="https://img.shields.io/badge/license-PolyForm%20Noncommercial%201.0.0-1f6feb">
  <img alt="Verified contour" src="https://img.shields.io/badge/verified-Linux%20%2B%20VS%20Code-2ea043">
</p>

# Amai

`Amai` is a memory and continuity tool for AI agents.
It keeps project context, working state, restore prompts, and installable client contours outside a single chat or IDE session.

## What It Does

- keeps project-scoped continuity outside one chat window;
- restores working context after chat rotation or clean-surface reopen;
- provides a verified `VS Code` / `Codium` bridge contour through `OpenVSX`;
- keeps the public repository focused on install and run, not internal governance.

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

### Quick Start

Install from GitHub, then use the `Amai` bridge inside `VS Code` / `Codium`.

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

## VS Code Bridge

The current public client contour is `Amai VS Code Bridge`:

- published via `OpenVSX`;
- usable from `VS Code` / `Codium`;
- installs an `Amai` activity-bar entry and clean-chat launch surface;
- designed to carry restore prompts into a fresh chat surface.

Published extension:

- https://open-vsx.org/extension/amai/amai-vscode-bridge

## What Gets Installed

The current GitHub install contour materializes:

- the local Amai repository under `~/.local/share/amai/repo` by default;
- the `amai` Rust binary build output;
- the verified `VS Code` / `Codium` bridge bundle;
- the local runtime/bootstrap surface for the selected stack profile.

## Remove

```bash
~/.local/share/amai/repo/scripts/remove_amai.sh --client vscode
```

This removes the managed local install contour when Amai was installed through the standard GitHub path.

## Roadmap

Current public truth:

- `Linux` + `VS Code` / `Codium` is the verified contour today;
- other operating systems and clients are planned, but not claimed as verified yet;
- public repo content stays minimal and install-oriented by design.

## License

This project currently uses `PolyForm Noncommercial 1.0.0`.

- commercial use is not permitted under the current license;
- the exact license text is in [LICENSE](LICENSE).
