> Рекомендация: для `Codex CLI`, `IDE Agent` и `OpenAI`-совместимых клиентов можно использовать API relay [nordrouter](https://nordrouter.com/dashboard/register?ref=XNH4N5). Через него можно управлять ключами, коэффициентами, балансом, логами и статусом сервиса. Есть дешёвые подписки.

<p align="center">
  <img src="brand/amai_lockup.svg" alt="Amai" width="360">
</p>

<p align="center">
  Memory and continuity for AI agents.
</p>

<p align="center">
  <a href="https://github.com/neo-2022/amai"><img alt="Repo" src="https://img.shields.io/badge/repo-GitHub-181717"></a>
  <img alt="License" src="https://img.shields.io/badge/license-PolyForm%20Noncommercial%201.0.0-1f6feb">
  <img alt="Verified contour" src="https://img.shields.io/badge/verified-Linux%20%2B%20VS%20Code%2FCodium-2ea043">
</p>

# Amai

`Amai` — внешний memory/continuity слой для AI-агентов.
Он живет как отдельный repo и подключается к клиентам как `MCP stdio server`.
Для project-scoped работы проект привязывается к canonical `repo_root`: если в workspace есть подпапка с похожим названием, Amai не должен заводить второй проект и должен вернуться к уже bound project.

## Проверенный контур

- ОС: `Ubuntu` / `Debian`
- Клиент: `VS Code` / `Codium`, `Hermes`
- Подключение: MCP stdio server; continuity restore доступен через `amai_continuity_startup`, а автоматический вызов конкретным клиентом требует отдельного live-доказательства

## Основной живой сценарий
Запись, мгновенный смысловой и буквальный поиск, точное время, происхождение, графовая связь, замена старого факта, сырой журнал, защита долговечных данных от забывания и восстановление после удаления местного кэша.

## Быстрый старт

### 1) Materialize repo

```bash
git clone --depth 1 https://github.com/neo-2022/amai.git "${HOME}/.local/share/amai/repo"
cd "${HOME}/.local/share/amai/repo"
```

Если `git` недоступен, можно materialize тот же repo через `codeload.github.com`:

```bash
tmp="$(mktemp -d)" && \
clone_dir="${HOME}/.local/share/amai/repo" && \
curl -fL --retry 5 --retry-delay 1 --retry-all-errors \
  -o "$tmp/amai.tgz" \
  https://codeload.github.com/neo-2022/amai/tar.gz/refs/heads/main && \
tar -xzf "$tmp/amai.tgz" -C "$tmp" && \
rm -rf "$clone_dir" && \
mkdir -p "$(dirname "$clone_dir")" && \
mv "$tmp/amai-main" "$clone_dir" && \
cd "$clone_dir"
```

### 2) Установить Amai

`VS Code` / `Codium`:

```bash
./scripts/install_amai.sh --client vscode --stack-profile default --yes
```

Любой MCP-клиент через `generic`:

```bash
./scripts/install_amai.sh --client generic --stack-profile default --yes
```

Если локальный stack не нужен, добавьте `--skip-stack`.

Если managed clone уже есть и его нужно обновить перед install:

```bash
./scripts/install_from_github.sh --client vscode --stack-profile default --yes
```

### 3) MCP snippet

После `--client generic` готовый snippet лежит в:

```text
~/.local/share/amai/repo/tmp/onboarding/generic-mcp.json
```

Минимальный MCP config выглядит так:

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

Ключевой контракт простой:

- `command` указывает на `scripts/run_mcp_stdio.sh`
- `cwd` указывает на корень установленного repo
- клиент запускает именно установленный repo, а не случайную временную директорию

### 4) Быстрая проверка

```bash
cd "${HOME}/.local/share/amai/repo"
./scripts/run_mcp_stdio.sh </dev/null >/dev/null 2>&1 || true
```

Если клиент видит сервер `amai` и может вызвать tools, MCP path живой.

## Remove

Обычное отключение клиента и runtime-artifacts:

```bash
~/.local/share/amai/repo/scripts/remove_amai.sh --client vscode
```

Жесткий purge хоста для `Amai` + `VS Code/Codium` следов:

```bash
~/.local/share/amai/repo/scripts/purge_amai_vscode_host.sh
```

`purge_amai_vscode_host.sh` удаляет пользовательские и системные следы агрессивно. Это не обычный uninstall.

## Где дальше читать

- Полный MCP/install walkthrough: [docs/MCP_INTEGRATION.md](docs/MCP_INTEGRATION.md)

## License

`PolyForm Noncommercial 1.0.0`
Текст лицензии: [LICENSE](LICENSE)
