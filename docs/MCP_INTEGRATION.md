modified_at: 2026-03-26 00:16 MSK
Ручная сверка guide/docs: 2026-03-26 00:16 MSK

# MCP Integration

Этот документ объясняет подключение `Amai` простыми словами.

## Что это даёт

Через `MCP` клиент подключается не к целому репозиторию напрямую, а к уже подготовленному внешнему инструменту.

На практике это значит:
- `Amai` сам держит индекс, retrieval и изоляцию проектов;
- IDE или ИИ-клиент не обязаны читать весь проект целиком;
- агент может просить у `Amai` готовый context pack и measured token benchmark.
- агент может просить и накопительный token report:
  - за текущую сессию;
  - за окно лимита;
  - за всё время.
- тот же token report теперь несёт и честный `agent_cycle` lower bound:
  - не как “полный счёт всей сессии”;
  - а как подтверждённую нижнюю границу measured части цикла.

## Это режим "скачал и всё"?

Почти, но пока не на 100%.

Сейчас правильная честная формулировка такая:
- один раз пройти onboarding;
- один раз дать клиенту готовый MCP config;
- дальше пользоваться как обычным внешним инструментом.

То есть это уже не режим “каждый запуск всё руками”, а режим “один раз настроил и потом работаешь”.

Для `VS Code` путь теперь максимально короткий.

Linux и macOS:

```bash
./scripts/install_amai.sh
```

Windows PowerShell:

```powershell
.\scripts\install_amai.ps1
```

Windows CMD:

```bat
scripts\install_amai.cmd
```

Самый человеческий путь теперь:
- запускается одна команда;
- она сначала проверяет машину;
- показывает два профиля установки;
- если видит несколько подходящих клиентов, показывает их и даёт выбрать нужный;
- даёт выбрать `1` или `2`;
- если профиль плохой, показывает `ПРЕДУПРЕЖДЕНИЕ`;
- только потом просит написать `ДА`;
- после этого делает install path под выбранный режим.
- если потом захотите отключить `Amai`, используйте `./scripts/remove_amai.sh`.

Если auto-detect промахнулся, всегда можно явно указать клиента:

```bash
./scripts/install_amai.sh --client vscode
./scripts/install_amai.sh --client cursor
./scripts/remove_amai.sh --client codex
```

Если клиент должен работать через дешёвый удалённый VPS, появился отдельный короткий путь:

```bash
./scripts/onboard_lite_vps.sh --client vscode
```

Это не “магическая оптимизация”.
Это честный профиль `lite_vps`, который заранее предупреждает:
- такой сервер подходит для remote MCP, smoke и demo;
- но не для наших рекордных benchmark-цифр.

После этого обычно остаётся:
- открыть repo в VS Code;
- сделать `Reload Window`;
- проверить, что MCP server виден клиенту.

Отдельно важно:
- локальный compatibility entrypoint `memory` теперь тоже может быть Amai-backed;
- если install шёл локально, `memory mcp` должен запускать именно `Amai`, а не старый внешний bridge;
- в локальном `~/.codex/config.toml` можно использовать:
  - `command = "/home/art/.local/bin/memory"`
  - `args = ["mcp"]`

В финальном выводе установки теперь видно:
- версию и ревизию `Amai`;
- какой клиент выбран;
- почему он выбран;
- какие ещё клиенты были найдены;
- живые метрики машины и stack;
- token savings из последнего measured benchmark, если они уже есть.
- адрес human dashboard, где уже видны три реальные live-метрики:
  - текущая сессия;
  - рабочее окно;
  - всё время.
- команду для human dashboard и адрес, где его открыть в браузере.

Отдельно важно:
- human dashboard не заменяет MCP;
- это просто самый понятный способ глазами увидеть пользу `Amai`;
- MCP при этом остаётся каналом, через который IDE и ИИ-клиент реально обращаются к инструменту.

Для других клиентов логика теперь тоже стала проще:
- `Cursor`
  - onboarding по умолчанию пишет config в user-scope path;
  - и отдельно materialize-ит project rule file `.cursor/rules/amai-continuity-startup.mdc`;
- `Codex`
  - onboarding по умолчанию пишет config в user-scope path;
  - startup теперь materialize-ится как bounded managed block внутри project `AGENTS.md`;
  - `Amai` не переписывает весь rule file: он обновляет только marker-bounded startup block;
- `Claude Code`
  - onboarding пишет workspace-local `.mcp.json`;
  - startup теперь materialize-ится как bounded managed block внутри project `CLAUDE.md`;
- `Claude Desktop`
  - пока получает generated file для ручного импорта.

Отдельно важно:
- launcher platform теперь можно выбирать явно;
- это позволяет честно генерировать config не только под Linux/macOS shell path, но и под Windows launchers.

И ещё один важный сдвиг:
- MCP contour теперь materialize-ит не только retrieval tools, но и отдельный startup tool
  `amai_continuity_startup`;
- для него есть парный prompt `amai-continuity-startup`;
- onboarding теперь ещё и честно различает:
  - где startup уже instruction-backed;
  - где пока есть только manual snippet;
  - где автоматический runtime contour ещё не materialized.

То есть truthful правило теперь такое:
- `VS Code` получает managed workspace instruction file
  `.github/instructions/amai-continuity-startup.instructions.md`;
- `Cursor` получает managed project rule file
  `.cursor/rules/amai-continuity-startup.mdc`;
- `Codex` получает instruction-backed startup через managed append-block в `AGENTS.md`;
- `Claude Code` получает instruction-backed startup через managed append-block в `CLAUDE.md`;
- `Claude Desktop` и `Generic` пока получают только сгенерированный snippet.

## Минимальные шаги

1. Самый простой путь

```bash
./scripts/install_amai.sh --client vscode
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

После этого для нового чата правильная логика такая:
- сначала вызвать `amai_continuity_startup` для текущего проекта;
- только потом, если нужен дополнительный retrieval, вызывать `amai_context_pack`;
- не перескакивать сразу к retrieval, если continuity ещё не поднята.

Если onboarding пишет managed startup artifact, это теперь видно прямо в финальном выводе:
- статус `Startup contract для клиента`;
- путь до startup artifact;
- truthful признак `Auto-start readiness`.

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
- `amai_stack_preflight`
- `amai_benchmark_coverage`
- `amai_context_pack`
- `amai_token_benchmark`
- `amai_token_report`
- `amai_memory_matrix`
- `amai_observe_snapshot`
- `amai_warm_cache`

У `amai_stack_preflight` structured output теперь даёт не один human verdict,
а две machine-readable формы:
- `preflight_report`
- `preflight_summary`

У `amai_token_report` short summary теперь тоже богаче прежнего headline-only слоя:
- retrieval KPI summary;
- и отдельный `agent_cycle` lower-bound summary,
  чтобы внешний клиент мог строить токенометрию честно, без подмены понятий.
- теперь ещё и compact contractual summary:
  - `contractual_scope_label`
  - `contractual_state`
  - `contractual_coverage_state`
  - `contractual_metering_ingest_state`
  - `contractual_lag_state`
  - `contractual_freshness_state`
  - `contractual_reconciliation_state`
  - `contractual_margin_state`
  - `contractual_blockers_summary`
  - `contractual_statement_summary`
  - `statement_export_preview`

Когда runtime честно привяжет `provider usage + rate card + infra cost profile`,
тот же `statement_export_preview` уже может нести:
- `line_item_surfaces.reconciliation_preview.drift_amount`
- `line_item_surfaces.margin_scope.customer_saved_amount_lower_bound`
- `line_item_surfaces.margin_scope.amai_infra_cost_amount`
- `line_item_surfaces.margin_scope.margin_amount`

Но это всё равно остаётся `report_only preview`, а не invoice.

Это нужно затем, чтобы клиентский слой мог показать короткий contractual state для review/audit
без парсинга полного raw `token_budget_report`.

Это нужно для внешних клиентов, которым важно понимать не только текущий health
stack-а, но и какие deployment promises вообще честно достижимы на этой машине.

У `amai_warm_cache` короткий summary теперь тоже не ограничивается сообщением
`сколько проектов прогрето`. Он показывает preview проектов и totals по
`cache_hit / exact / symbol / lexical / semantic`, а в structured output это же
лежит отдельно в `warm_cache_summary`.

У `amai_memory_matrix` structured output теперь тоже не схлопывается до одной
строки. Он отдаёт:
- полный `memory_task_matrix` payload;
- короткий `memory_matrix_summary`.

Во втором уже собраны:
- `tasks_total / tasks_passed / tasks_failed`;
- `success_rate`;
- `mean_score`;
- `p95_ms`;
- `gate_failures_count`;
- compact verdict-counts по canonical eval classes.

У `amai_benchmark_coverage` structured output теперь тоже не ограничивается
одним human summary. Он отдаёт:
- полный `benchmark_coverage` payload;
- короткий `benchmark_coverage_summary`.

Во втором уже лежат:
- `total_benchmarks`;
- `materialized / partial / mapped / next_priority / future`;
- compact summary следующих benchmark-приоритетов.

Отдельно `initialize` теперь отдаёт `amai_protocol_manifest`. Это нужно, чтобы
клиент при первом handshake видел не только список tools/prompts, но и:
- contract version;
- базовые safety laws;
- startup contract для project-scoped chat restore;
- per-tool `summary_field`, который должен появляться в structured output;
- назначение MCP prompts.

Этот startup contract теперь важен не меньше списка tools:
- canonical startup tool: `amai_continuity_startup`;
- canonical startup prompt: `amai-continuity-startup`;
- default namespace: `continuity`;
- before substantive work клиент обязан получить
  `continuity_startup_summary`, где уже surfaced:
  - `execctl_resume_state`;
  - `execctl_resume_contract_summary`;
  - `execctl_resume_obligation`;
  - `startup_next_action`;
  - `project_task_tree_summary`;
  - `project_task_ledger_summary`.
- тот же handshake теперь публикует и `resume_enforcement`:
  - `contract_field = execctl_resume_contract_summary`;
  - `resume_state_field = execctl_resume_state`;
  - `obligation_field = execctl_resume_obligation`;
  - `startup_next_action_field = startup_next_action`;
  - `must_resume_required_return_task_before_unrelated_work = true`;
  - `no_silent_drop = true`.

Это нужно понимать буквально:
- `execctl_resume_obligation` существует именно затем, чтобы клиент не парсил
  human summary строку ради `required_return_task`;
- `startup_next_action` существует затем, чтобы первый ход после startup был machine-readable
  и не зависел от текстового парсинга prompt/summary.
- если `execctl_resume_contract_summary` не `clear`, клиент не имеет права
  молча начинать unrelated work;
- startup artifact или managed rule должны прямо говорить про
  `required_return_task`, а не только про общий restore.
- сам `amai_continuity_startup` теперь перед чтением restore-state ещё и делает schema-sync;
  это важно затем, чтобы новый `ExecCtl` lease lane не рвал MCP startup после partial-upgrade
  на ошибке `relation ami.execctl_task_leases does not exist`.

Кроме success-shape, handshake теперь публикует и `error_contracts`. Это даёт
клиенту стабильную карту failure classes до первого сбоя, а в runtime:
- JSON-RPC errors несут `error.data.amai_error_code / amai_error_class`;
- tool-level failures в `tools/call` несут `structuredContent.error_taxonomy`.

Отдельно у каждого error class теперь есть `carrier`. Это важно, потому что
`invalid_params` может прийти и как top-level JSON-RPC error, и как `tool_is_error`
внутри `tools/call`, а клиенту нельзя угадывать transport по тексту ошибки.

Для `amai_observe_snapshot` клиент теперь получает ещё и `compatibility` слой:
- какой `compatibility_profile` активен;
- совместим ли текущий stack с поддерживаемым профилем;
- по каким сервисам есть drift или его нет.

Это привязывает live health snapshot к конкретному supported environment profile,
а не оставляет health и reproducibility раздельными мирами.

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
- вызывает `amai_continuity_startup`;
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

Для deployment profile preflight:

```bash
./scripts/proof_profiles.sh
```

Для короткого auto-install path:

```bash
./scripts/proof_install_auto.sh
```
