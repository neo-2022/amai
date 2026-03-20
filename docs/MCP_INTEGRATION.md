modified_at: 2026-03-20 18:30 MSK
Ручная сверка guide/docs: 2026-03-20 18:30 MSK

# MCP Integration

Этот документ объясняет подключение `Amai` простыми словами.

## Что это даёт

Через `MCP` клиент подключается не к целому репозиторию напрямую, а к уже подготовленному внешнему инструменту.

На практике это значит:
- `Amai` сам держит индекс, retrieval и изоляцию проектов;
- IDE или ИИ-клиент не обязаны читать весь проект целиком;
- агент может просить у `Amai` готовый context pack и measured token benchmark.

## Это режим "скачал и всё"?

Пока честно нет.

Сейчас правильный режим такой:
- один раз поднять stack;
- один раз подключить MCP config;
- потом пользоваться как обычным внешним инструментом.

То есть это уже не "каждый запуск руками", а "одна настройка и потом рабочий baseline".

## Минимальные шаги

1. Поднять `Amai`

```bash
./scripts/bootstrap_stack.sh
```

2. Собрать release binary

```bash
cargo build --release
```

3. Сгенерировать config snippet для своего клиента

```bash
./target/release/amai mcp config --client vscode
./target/release/amai mcp config --client cursor
./target/release/amai mcp config --client claude-desktop
./target/release/amai mcp config --client codex
```

4. Вставить snippet в MCP settings нужного клиента.

## Почему клиентский config маленький

Клиент не должен хранить:
- PostgreSQL DSN;
- S3 credentials;
- Qdrant URL;
- NATS URL.

Вместо этого клиент запускает только:
- `scripts/run_mcp_stdio.sh`

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
