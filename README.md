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
- `Ubuntu` / `Debian` install and run;
- `VS Code` / `Codium` client usage on that Linux contour;
- the `Amai VS Code Bridge` extension published through `OpenVSX`.

Other operating systems, clients, and applications will be added and verified as the project continues to develop.

## Install

Verified install contour right now: `Ubuntu` / `Debian` shell environment, with `VS Code` or `Codium`.

This does not currently claim verified support for `macOS`, `Windows`, or other client/runtime combinations.

## MCP: подключение к любому приложению

`Amai` — это обычный `MCP` `stdio` server. Смысл подключения всегда один:
ваш клиент/приложение должно запустить команду `scripts/run_mcp_stdio.sh` в каталоге установленного `Amai`.

### 1) Установить Amai и сгенерировать MCP-snippet (одной командой)

Если ваш MCP‑клиент не поддержан “из коробки”, используйте `generic`:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) --client generic --stack-profile default --yes
```

После установки snippet будет лежать здесь:

`~/.local/share/amai/repo/tmp/onboarding/generic-mcp.json`

Этот файл содержит три ключевых поля:
- `command` — что запускать (MCP server runner);
- `cwd` — где запускать (корень repo Amai);
- `args` — аргументы (обычно пусто).

### 2) Вставить snippet в конфиг вашего приложения

У разных приложений разная “обёртка” вокруг MCP‑сервера. Чаще всего встречаются два формата.

**Формат `mcpServers`:**

```json
{
  "mcpServers": {
    "amai": {
      "command": "/abs/path/to/amai/scripts/run_mcp_stdio.sh",
      "cwd": "/abs/path/to/amai",
      "args": []
    }
  }
}
```

**Формат `mcp.servers`:**

```json
{
  "mcp": {
    "servers": {
      "amai": {
        "command": "/abs/path/to/amai/scripts/run_mcp_stdio.sh",
        "cwd": "/abs/path/to/amai",
        "args": []
      }
    }
  }
}
```

Если ваш клиент умеет “import server config” без обёртки — используйте содержимое `generic-mcp.json` как есть.

### 3) Что учитывать (чтобы работало без ручных допиливаний)

- `command` должен указывать на `scripts/run_mcp_stdio.sh` из установленного `Amai` (а не на случайный путь с другого ПК).
- `cwd` должен быть корнем установленного repo (обычно `~/.local/share/amai/repo`).
- На Linux для локального стека нужен `docker`/`compose` (или ставьте `--skip-stack`, если вам нужен только MCP без локального stack).

### 4) Быстрая проверка (что MCP реально поднимается)

```bash
cd ~/.local/share/amai/repo && ./scripts/run_mcp_stdio.sh </dev/null >/dev/null 2>&1 || true
```

Если клиент “видит” сервер `amai` и может вызвать tools — интеграция готова.

### Install variants

There are currently three verified GitHub install front doors for this Linux contour:
- `curl` bootstrap for normal network conditions;
- `git clone` bootstrap if `raw.githubusercontent.com` is blocked or unstable.
- a `tarball` bootstrap (`codeload.github.com`) if `github.com:443` is blocked or unstable for `git clone`.

### System requirements

Current verified baseline:
- `Ubuntu` or `Debian`;
- `bash`;
- `sudo` / administrator access for first install on a clean machine;
- network access to GitHub and the system package repositories;
- `VS Code` or `Codium` for the verified client contour.

On this verified `Ubuntu` / `Debian` contour, the one-command installer can now bootstrap the missing local prerequisites for you, including:
- `git`;
- base build dependencies;
- `rustup` / `cargo` / `rustc`;
- `Docker` and compose support for the local stack.

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

If you want `Amai` for a different MCP client (for example `cursor`, `codex`, `claude-code`, `hermes`), pass it via `--client`.

If your MCP client is not on the list yet, use `--client generic` and import the generated snippet:

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) --client generic --stack-profile default --yes
```

It generates `tmp/onboarding/generic-mcp.json` inside the installed repo.
See `docs/MCP_INTEGRATION.md` for how to embed that snippet into your application's config format.

On the verified `Ubuntu` / `Debian` contour, this command can bootstrap the missing local prerequisites automatically.
Expect to grant `sudo` privileges on a clean machine during the first install.

If the install fails on a fresh machine with errors that mention missing build tooling (for example `cmake`, `jq`, or `rsync`), rerun the installer with working `sudo`, or install the missing packages first.

If you use the `VS Code` **snap** build, its extension root lives under `~/snap/code/(common|current)/.vscode/extensions` (not `~/.vscode/extensions`).
You can always override the detected extensions root via `AMAI_VSCODE_EXTENSIONS_ROOT=/absolute/path`.

If `raw.githubusercontent.com` is blocked or unstable, use the git-based one-liner:

```bash
git clone --depth 1 https://github.com/neo-2022/amai.git ~/.local/share/amai/repo && \
cd ~/.local/share/amai/repo && \
./scripts/install_amai.sh --client vscode --stack-profile default --yes
```

If `git clone` fails because `github.com:443` is blocked or unstable, use the tarball-based one-liner:

```bash
tmp="$(mktemp -d)" && \
curl -fL --retry 5 --retry-delay 1 --retry-all-errors -o "$tmp/amai.tgz" https://codeload.github.com/neo-2022/amai/tar.gz/refs/heads/main && \
tar -xzf "$tmp/amai.tgz" -C "$tmp" && \
bash "$tmp/amai-main/scripts/install_amai.sh" --client vscode --stack-profile default --yes
```

`scripts/install_from_github.sh` also supports `--download-mode tarball` (and `--download-mode auto` falls back to tarball when `git` checkout fails).

### Fastest Agent-Assisted Setup

If you already use an AI coding agent inside `VS Code` / `Codium`, the fastest path is:

1. run one of the install commands above;
2. open `~/.local/share/amai/repo` in the editor;
3. ask the agent to verify the local contour end to end:
   - `.vscode/mcp.json` exists;
   - `systemctl --user is-active amai-stack.service` is `active`;
   - `Amai VS Code Bridge` is installed;
   - the `OpenAI` / `Codex` chat surface is available if you want to launch `Amai` from the sidebar.

This does not replace the normal Amai install.
It is simply the quickest way to let an agent verify and finish the editor-side contour without manual spot checks.

## VS Code Bridge

The current public client contour is `Amai VS Code Bridge`:

- published via `OpenVSX`;
- usable from `VS Code` / `Codium`;
- installs an `Amai` activity-bar entry and clean-chat launch surface;
- designed to carry restore prompts into a fresh chat surface;
- depends on a separate `OpenAI` / `Codex` chat surface inside the editor.

After install, the intended first-run path is simple:

1. open `~/.local/share/amai/repo` in `VS Code` / `Codium`;
2. do `Reload Window`;
3. install or enable the `OpenAI` extension if the editor does not already expose the `Codex` / `ChatGPT` surface;
4. click the `Amai` icon in the activity bar;
5. use the sidebar helper actions if the workspace or OpenAI surface is still missing.

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
