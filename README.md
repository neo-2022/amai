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

`Amai` — внешний memory/continuity слой для AI-агентов.
Он хранит рабочий контекст между сессиями и подключается к клиентам как `MCP stdio server`.

## Проверенный контур

- ОС: `Ubuntu` / `Debian`
- Клиент: `VS Code` / `Codium`
- Bridge: `Amai VS Code Bridge` через `OpenVSX`

## MCP: подключение к любому приложению

Принцип всегда один: клиент должен запускать `scripts/run_mcp_stdio.sh` из установленного репозитория `Amai`.
`Amai` не привязан к конкретному поставщику моделей: MCP-клиент может работать с любым LLM-провайдером.

### 1) Установка + генерация snippet (`generic`)

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) --client generic --stack-profile default --yes
```

Если `raw.githubusercontent.com` недоступен:

```bash
clone_dir="${HOME}/.local/share/amai/repo" && \
if [ -d "${clone_dir}/.git" ]; then
  git -C "${clone_dir}" fetch --depth 1 origin && git -C "${clone_dir}" checkout --force main && git -C "${clone_dir}" reset --hard origin/main
else
  git clone --depth 1 https://github.com/neo-2022/amai.git "${clone_dir}"
fi && \
"${clone_dir}/scripts/install_amai.sh" --client generic --stack-profile default --yes
```

Snippet после установки: `~/.local/share/amai/repo/tmp/onboarding/generic-mcp.json`

### 2) Вставить snippet в конфиг клиента

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

Если клиент умеет импорт “чистого server config”, используйте `generic-mcp.json` как есть.

### 3) Важные условия

- `command` должен указывать на `scripts/run_mcp_stdio.sh` установленного `Amai`.
- `cwd` должен быть корнем установленного repo (обычно `~/.local/share/amai/repo`).
- Для локального stack нужен `docker`/`compose` (или используйте `--skip-stack`, если нужен только MCP).

### 4) Быстрая проверка MCP

```bash
cd ~/.local/share/amai/repo && ./scripts/run_mcp_stdio.sh </dev/null >/dev/null 2>&1 || true
```

Если клиент видит сервер `amai` и может вызвать tools — интеграция готова.

## Установка (коротко)

### VS Code / Codium (обычный путь)

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) --client vscode --stack-profile default --yes
```

CLI клиента ищется автоматически (`code`, `codium`, `code-oss`, включая `~/.local/bin`).
Если у вас нестандартный путь, задайте его явно:

```bash
AMAI_VSCODE_CLI_BIN="/абсолютный/путь/к/code-или-codium" \
bash <(curl -fsSL https://raw.githubusercontent.com/neo-2022/amai/main/scripts/install_from_github.sh) --client vscode --stack-profile default --yes
```

### Если `raw.githubusercontent.com` недоступен

**Через `git clone`:**

```bash
clone_dir="${HOME}/.local/share/amai/repo" && \
if [ -d "${clone_dir}/.git" ]; then
  git -C "${clone_dir}" fetch --depth 1 origin && git -C "${clone_dir}" checkout --force main && git -C "${clone_dir}" reset --hard origin/main
else
  git clone --depth 1 https://github.com/neo-2022/amai.git "${clone_dir}"
fi && \
"${clone_dir}/scripts/install_amai.sh" --client vscode --stack-profile default --yes
```

**Через tarball (`codeload.github.com`):**

```bash
tmp="$(mktemp -d)" && \
curl -fL --retry 5 --retry-delay 1 --retry-all-errors -o "$tmp/amai.tgz" https://codeload.github.com/neo-2022/amai/tar.gz/refs/heads/main && \
tar -xzf "$tmp/amai.tgz" -C "$tmp" && \
bash "$tmp/amai-main/scripts/install_amai.sh" --client vscode --stack-profile default --yes
```

## VS Code Bridge

- OpenVSX: https://open-vsx.org/extension/amai/amai-vscode-bridge
- После установки: bridge и MCP-конфиг добавляются автоматически.
- Откройте любой рабочий проект в `VS Code` / `Codium` и сделайте `Reload Window`.
- Важно: install из GitHub ставит bridge из текущего `main`; версия в OpenVSX может отставать до следующей публикации.

## Remove

```bash
~/.local/share/amai/repo/scripts/remove_amai.sh --client vscode
```

## License

`PolyForm Noncommercial 1.0.0`  
Текст лицензии: [LICENSE](LICENSE)

## Контакты

Обратная связь: `Art260679@gmail.com`
