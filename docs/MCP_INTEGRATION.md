modified_at: 2026-03-26 06:30 MSK
Ручная сверка guide/docs: 2026-03-26 06:30 MSK

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
- после локальной установки stack поднимается через managed `systemd --user` unit `amai-stack.service`, поэтому ручной `./scripts/bootstrap_stack.sh` не нужен при старте user manager;
- unattended/headless boot без входа пользователя пока не считается доказанным: для этого нужен отдельный `linger`/system-service режим и proof.
- proof-refresh 2026-04-25 подтвердил этот bounded contract через `proof_stack_autostart.sh`, `proof_bootstrap_volume_dirs.sh` и `proof_onboarding.sh`; reboot/headless guarantee не добавлялся.

Отдельно важно:
- локальный compatibility entrypoint `memory` теперь тоже может быть Amai-backed;
- если install шёл локально, `memory mcp` должен запускать именно `Amai`, а не старый внешний bridge;
- в локальном `~/.codex/config.toml` можно использовать:
  - `command = "memory"`
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
- launcher `scripts/run_mcp_stdio.sh` и `scripts/run_mcp_stdio.ps1` теперь correctness-first:
  если на машине есть `cargo`, MCP server поднимается через `cargo run --release --quiet -- mcp serve`,
  чтобы новая чистая рабочая поверхность не цепляла устаревший `target/release/amai` после свежих code changes.
- launcher stdout до первого JSON-RPC ответа обязан оставаться чистым; startup diagnostics пишутся в log/stderr,
  иначе MCP клиент может принять debug строку за protocol payload.

Для других клиентов логика теперь тоже стала проще:
- `Cursor`
  - onboarding по умолчанию пишет config в user-scope path;
  - и отдельно materialize-ит project rule file `.cursor/rules/amai-continuity-startup.mdc`;
- `Codex`
  - onboarding по умолчанию пишет config в user-scope path;
  - startup теперь materialize-ится как bounded managed block внутри project `AGENTS.md`;
  - `Amai` не переписывает весь rule file: он обновляет только marker-bounded startup block;
- `Hermes`
  - onboarding по умолчанию пишет config в `~/.hermes/config.yaml`;
  - MCP server materialize-ится в секции `mcp_servers`;
  - startup теперь materialize-ится в Hermes-native project context файле `.hermes.md`;
  - `.hermes.md` и managed profile `SOUL.md` теперь materialize-ятся как compact contract-pointer, а не как длинная копия полного startup-law;
  - onboarding дополнительно materialize-ит dedicated project-bound Hermes profile и делает его sticky default, чтобы `amai_continuity_startup` подхватывался даже если Hermes стартует не из repo `cwd`;
- `OpenClaw`
  - onboarding по умолчанию пишет config в `~/.openclaw/openclaw.json`;
  - MCP server materialize-ится в секции `mcp.servers`;
  - startup теперь materialize-ится в repo-local `.openclaw/AGENTS.md`;
  - onboarding дополнительно регистрирует отдельный OpenClaw agent, который указывает именно на этот repo-local workspace, не трогая shared global workspace;
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
- `Hermes` получает auto-written MCP config в `~/.hermes/config.yaml`, compact managed startup через `.hermes.md` и sticky project-bound profile, который тянет тот же startup path по умолчанию;
- `OpenClaw` получает auto-written MCP config в `~/.openclaw/openclaw.json`, managed startup в `.openclaw/AGENTS.md` и отдельный project-bound agent/workspace registration;
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
./target/release/amai mcp config --client hermes
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

Тот же `remote_ssh` contour поддерживается и для остальных managed clients:
- `Cursor`
- `Codex`
- `Claude Code`
- `Hermes`
- `OpenClaw`

4. Если onboarding уже запускался, часть этой работы уже сделана автоматически.

После этого для новой чистой рабочей поверхности правильная логика такая:
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
- machine-readable startup artifact:
  `.amai/onboarding/project-chat-startup-contract.json`;
- `artifact_enforcement` внутри этого contract теперь буквально фиксирует:
  - `workspace_contract_required_before_tool_call = true`;
  - `workspace_contract_relative_path = .amai/onboarding/project-chat-startup-contract.json`;
  - `missing_or_unreadable_fail_closed = true`;
  - `sha256_mismatch_fail_closed = true`;
- before substantive work клиент обязан получить
  `continuity_startup_summary`, где уже surfaced:
  - `execctl_resume_state`;
  - `execctl_resume_contract_summary`;
  - `execctl_resume_obligation`;
  - `startup_next_action`;
  - `execctl_active_lease`;
  - `required_return_task`;
  - `project_task_tree`;
  - `project_task_tree_summary`;
  - `project_task_ledger`;
  - `project_task_ledger_summary`.
- тот же handshake теперь публикует и `resume_enforcement`:
  - `contract_field = execctl_resume_contract_summary`;
  - `resume_state_field = execctl_resume_state`;
  - `obligation_field = execctl_resume_obligation`;
  - `startup_next_action_field = startup_next_action`;
  - `active_lease_field = execctl_active_lease`;
  - `active_lease_owner_state_field = lease_owner_state`;
  - `previous_session_owner_value = previous_session_owner`;
  - `must_resume_required_return_task_before_unrelated_work = true`;
  - `previous_session_owner_must_follow_startup_next_action = true`;
  - `no_silent_drop = true`.

Это нужно понимать буквально:
- managed markdown/rule block больше не считается достаточным source-of-truth сам по себе;
- если workspace startup contract artifact отсутствует, не читается или не проходит hash-check,
  client runtime обязан fail-closed остановиться до tool call;
- `execctl_resume_obligation` существует именно затем, чтобы клиент не парсил
  human summary строку ради `required_return_task`;
- `required_return_task` теперь surfaced отдельно, чтобы client runtime видел сам return target
  как object, а не выводил его из пары `required_return_headline/next_step`;
- `required_task_set` и `required_task_set_summary` теперь тоже обязаны surfaced отдельно:
  multi-task obligation нельзя молча схлопывать до одного `required_return_task` или только до
  human summary строки;
- `chat_start_restore.prompt_text` и human-readable CLI output считаются projection/delivery
  layer: они могут показать компактную подсказку оператору, но не заменяют
  machine-readable `required_task_set`, runtime artifact и startup audit;
- `startup_next_action` существует затем, чтобы первый ход после startup был machine-readable
  и не зависел от текстового парсинга prompt/summary.
- `execctl_active_lease` существует затем, чтобы клиент видел owner-state текущей линии как object,
  а не пытался угадывать его по summary-строке.
- `project_task_tree` и `project_task_ledger` теперь тоже surfaced как objects:
  клиенту больше не нужно довольствоваться только summary-строкой, если он хочет machine-readable
  active/pending-return tree и durable append-only ledger.
- если `execctl_resume_contract_summary` не `clear`, клиент не имеет права
  молча начинать unrelated work;
- если `execctl_active_lease.lease_owner_state = previous_session_owner`, клиент не имеет права
  тихо захватывать workline и обязан follow `startup_next_action` first;
- startup artifact или managed rule должны прямо говорить про
  `required_return_task`, а не только про общий restore.
- onboarding теперь materialize-ит и отдельный workspace JSON artifact:
  `.amai/onboarding/project-chat-startup-contract.json`;
  клиент может читать его как machine-readable source-of-truth вместо парсинга markdown/rule file.
- тот же artifact теперь pinned через `startup_contract_sha256`;
  managed startup instructions поднимают expected hash, чтобы client/runtime мог fail-closed
  при contract drift, а не продолжал работу по устаревшему startup block.
- сам `amai_continuity_startup` теперь перед чтением restore-state ещё и делает schema-sync;
  это важно затем, чтобы новый `ExecCtl` lease lane не рвал MCP startup после partial-upgrade
  на ошибке `relation ami.execctl_task_leases does not exist`.
- кроме static onboarding contract, сам startup теперь materialize-ит и dynamic runtime artifact
  `.amai/continuity/project-chat-startup-state.json`;
  там лежит последняя `continuity_startup_summary` и компактный `chat_start_restore.prompt_text`.
- static startup contract теперь прямо несёт `runtime_state_artifact`, чтобы client/runtime видел:
  - какой file path ожидать;
  - какую `artifact_version` ожидать;
  - какой tool его пишет;
  - какое summary field там является source-of-truth;
  - какое top-level поле обязано доказать runtime consistency;
  - какое поле внутри runtime artifact является immediate startup gate.
- это нужно затем, чтобы supported clients и operator tooling могли проверить не только
  installation-time law, но и фактический live return contour:
  `startup_execution_gate`, `startup_next_action`, `required_return_task`, `required_task_set`,
  `required_task_set_summary`, `execctl_active_lease`, `project_task_tree`, `project_task_ledger`.
- тот же `startup_execution_gate` теперь идёт и прямо в `continuity_startup_summary`, а не только
  в runtime artifact/fallback path.
- startup contract теперь ещё и pin-ит field-level gate semantics, чтобы клиент знал literal
  meaning без prompt-guessing:
  - `startup_execution_gate.must_follow_startup_next_action = true`;
  - `startup_execution_gate.unrelated_work_allowed = false`;
  - `startup_execution_gate.must_read_prompt_text_before_reply = true`;
  - `startup_execution_gate.required_action_kind_when_resume_required = "resume_required_return_task"`;
  - `startup_execution_gate.no_silent_drop = true`.
- поверх этого runtime artifact теперь обязан нести и
  `gate_semantics_consistent = true/false`;
  supported client не имеет права доверять `startup_execution_gate`, если это поле отсутствует
  или не равно `true`.
- managed startup instructions для supported clients теперь обязаны повторять и literal
  `workspace_runtime_state_artifact_version = "workspace-startup-runtime-state-v4"`,
  чтобы client fail-closed не только на contract hash, но и на drift runtime artifact shape.
- `amai status` теперь читает и этот runtime artifact; если он не materialized, status честно
  показывает `startup_runtime_state: not_materialized`, а если он потерял hash или required fields —
  `startup_runtime_state: startup_runtime_state_drift`.
- если нужно inspect-ить этот runtime artifact в конкретном project workspace, а не в самом
  `Amai` repo-root, теперь есть отдельный CLI path:
  `cargo run -- continuity startup-state --repo-root /path/to/project --json`.
- этот fallback path pinned и в startup contract: клиент может через него поднять тот же
  `startup_execution_gate`, если direct file-read неудобен.

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
- `amai-continuity-startup`
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

Proof-refresh 2026-04-25: `./scripts/proof_mcp.sh` прошёл с `proof_scope=full`,
`critical=0`, `memory_matrix_tasks_failed=0`, prompt set
`amai-context-pack / amai-continuity-startup / amai-onboarding` и token savings
`83.69098712446352%`. Это доказывает MCP handshake/runtime contract, а не live
client UX.

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

Оба proof теперь идут как multi-client matrix, а не как single-client smoke:
- `proof_remote_ssh_config.sh` проверяет `VS Code`, `Cursor`, `Claude Code`, `Codex`, `Hermes` и `OpenClaw`;
- `proof_remote_onboarding.sh` проверяет тот же набор через `onboard_remote_client.sh`, включая local startup artifacts и `OpenClaw` project-bound workspace/agent path.

Для deployment profile preflight:

```bash
./scripts/proof_profiles.sh
```

Для короткого auto-install path:

```bash
./scripts/proof_install_auto.sh
```
