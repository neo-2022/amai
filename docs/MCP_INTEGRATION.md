modified_at: 2026-03-20 20:31 MSK
Ручная сверка guide/docs: 2026-03-20 20:31 MSK

# MCP Integration

Этот документ объясняет подключение `Amai` простыми словами.

## Что это даёт

Через `MCP` клиент подключается не к целому репозиторию напрямую, а к уже подготовленному внешнему инструменту.

На практике это значит:
- `Amai` сам держит индекс, retrieval и изоляцию проектов;
- IDE или ИИ-клиент не обязаны читать весь проект целиком;
- агент может просить у `Amai` готовый context pack и measured token benchmark.

## Это режим "скачал и всё"?

Почти, но пока не на 100%.

Сейчас правильная честная формулировка такая:
- один раз пройти onboarding;
- один раз дать клиенту готовый MCP config;
- дальше пользоваться как обычным внешним инструментом.

То есть это уже не режим “каждый запуск всё руками”, а режим “один раз настроил и потом работаешь”.

Для `VS Code` путь теперь максимально короткий:

```bash
./scripts/onboard_local.sh --client vscode
```

После этого обычно остаётся:
- открыть repo в VS Code;
- сделать `Reload Window`;
- проверить, что MCP server виден клиенту.

Для других клиентов логика теперь тоже стала проще:
- `Cursor`
  - onboarding по умолчанию пишет config в user-scope path;
- `Codex`
  - onboarding по умолчанию пишет config в user-scope path;
- `Claude Code`
  - onboarding пишет workspace-local `.mcp.json`;
- `Claude Desktop`
  - пока получает generated file для ручного импорта.

Отдельно важно:
- launcher platform теперь можно выбирать явно;
- это позволяет честно генерировать config не только под Linux/macOS shell path, но и под Windows launchers.

## Минимальные шаги

1. Самый простой путь

```bash
./scripts/onboard_local.sh --client vscode
```

2. Если нужен ручной путь, вместо onboarding можно сделать всё отдельно:

```bash
./scripts/bootstrap_stack.sh
cargo build --release
```

3. Сгенерировать config snippet для своего клиента

```bash
./target/release/amai mcp config --client vscode
./target/release/amai mcp config --client cursor
./target/release/amai mcp config --client claude-code
./target/release/amai mcp config --client claude-desktop
./target/release/amai mcp config --client codex
```

Если нужен Windows launcher:

```bash
./target/release/amai mcp config --client cursor --launcher-platform windows-powershell
./target/release/amai mcp config --client codex --launcher-platform windows-cmd
```

Если `Amai` уже стоит на удалённом Linux/VPS-host, можно сгенерировать `ssh`-launcher вместо локального runner:

```bash
./target/release/amai mcp config \
  --client vscode \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai
```

Если хочется совсем короткий путь без ручной сборки snippet:

```bash
./scripts/onboard_remote_client.sh \
  --client vscode \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai
```

4. Если onboarding уже запускался, часть этой работы уже сделана автоматически.

## Как отключить клиента обратно

Теперь lifecycle симметричный:
- можно не только подключить `Amai`;
- можно и убрать его из клиентского конфига одной командой.

Примеры:

```bash
./scripts/disconnect_local.sh --client vscode
./scripts/disconnect_local.sh --client cursor
./scripts/disconnect_local.sh --client codex
```

Если после удаления config оказался пустым, `Amai` умеет честно убрать и сам пустой файл.

## Что это значит для Windows и macOS

`MCP` как стандарт не привязан к одной ОС.
Поэтому:
- `VS Code` на Windows и macOS можно подключать к `Amai`;
- вопрос упирается не в стандарт, а в launcher path и install contour.

Текущий baseline теперь умеет:
- Linux/macOS shell launcher;
- Windows `cmd`;
- Windows `PowerShell`.

Это ещё не полный polished cross-platform installer, но это уже реальный materialized шаг, а не обещание на будущее.

Отдельно для удалённого режима:
- `Amai` можно держать на Linux/VPS;
- клиент на Windows/macOS может запускать его через `ssh`;
- в этом режиме MCP остаётся `stdio`, просто transport идёт поверх `ssh`, а не через локальный shell runner;
- это безопаснее, чем выставлять `PostgreSQL/Qdrant/NATS/S3` наружу напрямую.

Для такого режима достаточно:
- чтобы `Amai` уже был поднят на удалённой машине;
- чтобы клиент умел выполнять `ssh user@host`;
- чтобы вы знали путь до repo на сервере, например `/srv/amai`.
- если хотите автоматическую запись клиентского конфига, используйте `scripts/onboard_remote_client.sh`;
- если нужен только snippet без записи файла, используйте `amai mcp config`.

## Почему клиентский config маленький

Клиент не должен хранить:
- PostgreSQL DSN;
- S3 credentials;
- Qdrant URL;
- NATS URL.

Вместо этого клиент запускает только:
- `scripts/run_mcp_stdio.sh`
- или `ssh user@host 'cd /srv/amai && ./scripts/run_mcp_stdio.sh'`

Этот runner:
- подтягивает `.env`;
- стартует `amai mcp serve`;
- не заставляет пользователя вручную дублировать внутренние настройки стека.

## Что клиент получает через MCP

Сейчас доступны:
- `amai_list_projects`
- `amai_list_namespaces`
- `amai_context_pack`
- `amai_token_benchmark`
- `amai_observe_snapshot`
- `amai_warm_cache`

И prompts:
- `amai-onboarding`
- `amai-context-pack`

Это помогает новому ИИ сразу понять:
- что `Amai` вообще делает;
- почему проекты нельзя смешивать;
- почему по умолчанию нужен `local_strict`.

И ещё важно:
- `Amai` не обязан показывать что-нибудь любой ценой;
- если запрос не подтверждается exact/symbol/lexical evidence, а semantic layer не даёт надёжного совпадения, MCP-клиент получает честный пустой retrieval вместо слабого шума.

## Как проверить, что MCP contour живой

```bash
./scripts/proof_mcp.sh
```

Этот proof реально:
- запускает MCP server;
- делает handshake;
- читает tools;
- читает prompts;
- вызывает `context pack`, `token benchmark`, `observe snapshot`.

Если этот proof зелёный, значит MCP path не остался "только на бумаге".

Отдельно для install/remove lifecycle:

```bash
./scripts/proof_client_lifecycle.sh
```

Для remote `ssh` config generation:

```bash
./scripts/proof_remote_ssh_config.sh
```

Для короткого remote onboarding path:

```bash
./scripts/proof_remote_onboarding.sh
```
