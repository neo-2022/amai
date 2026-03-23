modified_at: 2026-03-23 16:12 MSK
Ручная сверка guide/docs: 2026-03-23 16:12 MSK

# Operations

Каноническое имя проекта:
- `Art-memory-agent-index`
- short name: `Amai`
- текущий path: `/home/art/agent-memory-index`

## Bootstrap

Самый простой путь для локального пользователя:

```bash
cd /home/art/agent-memory-index
./scripts/install_amai.sh
```

Эта команда:
- сначала показывает понятную проверку машины;
- сама сравнивает два профиля установки;
- даёт выбрать `1` или `2`;
- если профиль слишком тяжёлый, печатает `ПРЕДУПРЕЖДЕНИЕ` и не идёт дальше молча;
- ждёт явного подтверждения словом `ДА`;
- создаёт и досинхронизирует `.env`;
- поднимает stack;
- materialize-ит bootstrap;
- собирает release binary;
- ставит внешний compatibility bridge `memory -> Amai`;
- пишет готовый MCP config для клиента.

`install_amai.sh` делает ещё один шаг поверх этого:
- по умолчанию использует `client = auto`;
- пытается определить, какой клиент наиболее вероятен;
- работает как более человеческое имя для product install path.
- если запускать его повторно, он не должен плодить дубликаты, а должен аккуратно пересинхронизировать текущую установку.
- после локальной установки `~/.local/bin/memory` больше не должен указывать на старый bridge; он должен запускать `Amai` compatibility runner.

Если нужен cheap remote/smoke contour под слабый VPS:

```bash
cd /home/art/agent-memory-index
./scripts/onboard_lite_vps.sh --client vscode
```

Этот путь:
- использует `stack_profile = lite_vps`;
- сначала делает Rust preflight;
- потом поднимает тот же baseline stack, но честно объясняет, что это не профиль для рекордных benchmark-цифр.

Симметричное удаление:

```bash
./scripts/remove_amai.sh
./scripts/remove_amai.sh --client codex
```

Если `Amai` уже живёт на удалённом Linux/VPS-host, а локально нужен только клиентский config:

```bash
cd /home/art/agent-memory-index
./scripts/onboard_remote_client.sh \
  --client vscode \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai
```

Этот путь:
- не поднимает локальный stack;
- не требует локального `docker compose up`;
- не требует локального `cargo build --release`;
- просто пишет клиентский config, который запускает удалённый `Amai` через `ssh`.

Список default client targets теперь хранится отдельно в:

```bash
config/client_targets.toml
```

Если нужен ручной инженерный путь:

```bash
cd /home/art/agent-memory-index
cp .env.example .env
./scripts/bootstrap_stack.sh
```

Критичные `.env` поля:
- `AMI_STACK_PROFILE`
  - machine-readable default profile для bootstrap/preflight;
  - сейчас канонические значения:
    - `default`
    - `lite_vps`;
- `AMI_DEFAULT_RETRIEVAL_MODE`
  - режим видимости по умолчанию;
- `AMI_LOCAL_FAST_CACHE_TTL_MS`
  - TTL для process-local hot cache;
  - увеличивать его без нужды не стоит, потому что слишком длинное окно хуже для реактивности на relation/config drift.
- `AMI_WARMUP_PROJECTS`
  - список project codes для автоматического warmup после bootstrap;
- `AMI_OBSERVE_BIND`
  - bind-адрес Rust exporter для Prometheus scrape;
- `AMI_PROMETHEUS_PORT` и `AMI_GRAFANA_PORT`
  - локальные порты monitoring profile.

## Continuity migration from previous toolchain

Если проект уже использовал старую continuity-схему вне `Amai`, её можно не выбрасывать, а аккуратно втянуть внутрь `Amai`.

Канонические generic команды:

```bash
cd /home/art/agent-memory-index
./scripts/import_continuity.sh \
  --project project_alpha \
  --display-name "Project Alpha" \
  --repo-root /path/to/project-alpha \
  --namespace continuity \
  --bootstrap-file /path/to/amai/state/continuity-imports/project-alpha/continuity-snapshot.md
```

Если у проекта есть старый внешний каталог markdown-заметок или memory-export, который ещё нужен для аудита или совместимости с рантаймом клиента, его можно добавить отдельно:

```bash
  --memory-dir /path/to/project-alpha/optional-legacy-notes
```

После этого новый startup-contour уже может идти через `Amai`:

```bash
cd /home/art/agent-memory-index
./scripts/continuity_startup.sh --project project_alpha --namespace continuity
./scripts/continuity_restore.sh --project project_alpha --namespace continuity
```

Что это materialize-ит:
- текущий continuity namespace перед импортом очищается от прошлых continuity docs, чтобы старые snapshot-path не оставались призраками в retrieval;
- полный raw continuity-content сохраняется в artifact storage;
- searchable continuity-layer режется до безопасного размера для `PostgreSQL tsvector` и lexical chunks;
- observability получает отдельный snapshot `continuity_import`;
- startup-summary потом читается не из нескольких разрозненных источников, а из `Amai`.
- поверх обычного handoff теперь materialized ещё и `working_state` слой:
  он автозахватывается после `continuity handoff` и после `context pack`,
  а новый chat-start поднимает не только headline/next-step, но и активные файлы, последние рабочие запросы и текущую рабочую сессию.
  Если в этой линии уже были retrieval-context события, тот же startup-path поднимает и `workspace_graph_summary`,
  чтобы было видно, сколько файлов/символов/структурных узлов уже легло в живой рабочий контур.
- `--active-workline-file` больше не обязателен:
  если write-side handoff уже живёт в `Amai`, import может брать headline и next-step оттуда, а bootstrap/transcript слой остаётся только refresh-evidence.
- `--thread-index-file` добавляет отдельный machine-readable temporal index всех chat threads.
  Это важно, потому что temporal lookup по `previous chat` и `exact time` больше не должен зависеть
  от ширины импорта full rendered transcripts.
- старый каталог заметок теперь должен передаваться явно;
  автоматическое чтение project `.codex` как памяти запрещено, чтобы не возвращать legacy workflow.

Важно:
- старые источники при этом не обязаны сразу исчезать;
- безопасный migration path такой:
  - продолжать писать handoff-файл / transcript mirror;
  - при необходимости держать отдельный старый memory-export только как compatibility-layer;
  - после содержательной работы обновлять continuity import в `Amai`;
  - новый session-start уже поднимать через `Amai continuity startup`.

Для `Art` этот контур теперь настроен так по умолчанию:
- bootstrap snapshot по-прежнему собирается из transcript mirror;
- full rendered transcripts в import остаются ограниченными `ART_TRANSCRIPT_LIMIT=3`;
- но полный temporal index импортируется отдельно из `~/.memory/transcripts/codex/thread_index.json`.
- перед import raw thread index теперь проходит через Rust enrich-step и пишет
  `state/continuity-imports/art/thread_index.enriched.json`;
- именно этот enriched файл потом передаётся в `continuity import`, чтобы compact summary-поля
  уже были готовы до записи в `PostgreSQL`.

Именно поэтому корректный proof сейчас выглядит так:
- `Rendered transcripts` в startup остаётся маленьким числом;
- `continuity_thread_index` в PostgreSQL всё равно покрывает все чаты этого project-space;
- в `continuity_thread_index` теперь живут compact поля `summary_headline` и `summary_next_step`;
- в этом же temporal snapshot теперь живут и `time_slices`: короткие смысловые окна внутри thread-а
  с собственным диапазоном времени и compact anchors;
- temporal lookup продолжает точно отвечать на `previous chat` и `at exact time`.

Практический смысл этих compact полей:
- `summary_headline` даёт короткую линию выбранного chat thread без повторного разбора длинного
  assistant-ответа;
- `summary_next_step` даёт готовый следующий шаг того же thread без дублирования label вроде
  `Следующий шаг: Следующий шаг: ...`;
- `time_slices` дают уже не весь thread целиком, а локальный смысловой кусок вокруг нужного момента
  времени, чтобы `exact time` не жил на приблизительном nearest-thread ответе;
- за счёт этого `previous chat` и `exact time` живут на более готовом temporal snapshot, а не на
  позднем post-hoc parsing.

Жёсткий инвариант exact-time контура:
- если есть `time-local` evidence, ответ строится по нему;
- если выбранный slice слишком далёк по времени или его вообще нет, temporal contour обязан
  fail-closed;
- fallback на просто последние сообщения thread-а для вопроса “что было в точное время” запрещён,
  потому что это создаёт ложную память.

Если нужно прогнать этот upstream enrich отдельно руками:

```bash
cd /home/art/agent-memory-index
./scripts/enrich_thread_index.sh \
  --input state/continuity-imports/art/thread_index.json \
  --output state/continuity-imports/art/thread_index.enriched.json
```

Это read-only к transcript index и write-only к enriched JSON:
- исходный `thread_index.json` не трогается;
- enriched copy используется как import input;
- temporal lookup затем питается уже этим machine-readable snapshot.

## Working-state recovery и multi-agent изоляция

Для continuity теперь есть два разных, но связанных слоя:

- `continuity handoff`
  - фиксирует главное решение и обязательный следующий шаг;
- `working_state`
  - фиксирует текущее рабочее состояние между чатами:
    - активную цель;
    - последние retrieval-запросы;
    - активные файлы;
    - текущую непрерывную рабочую сессию.
- `chat_start_restore`
  - компактный startup pack для первого содержательного ответа нового чата;
  - строится поверх `handoff + working_state + continuity import`;
  - нужен затем, чтобы не тратить первый ответ на повторное восстановление context.

Каноническая изоляция для этого слоя:
- `project_code`
- `namespace_code`
- `agent_scope`
- `session_id`

Если агент один, это обычно уже работает без ручной настройки.
Если агентов несколько, задавайте каждому свой `AMAI_AGENT_SCOPE`.

Для вопросов вида `на чём остановились` теперь есть отдельный read-only путь:

```bash
./scripts/continuity_answer.sh --project project_alpha --namespace continuity --intent last_chat
```

Он:
- сам использует тот же continuity слой;
- отдаёт сразу готовый короткий ответ;
- не требует отдельной цепочки `startup -> restore/context`;
- не должен порождать новый handoff.

Если нужен именно предыдущий чат и его хвост:

```bash
./scripts/chat_lookup.sh \
  --project project_alpha \
  --namespace continuity \
  --chat-reference previous \
  --messages-count 2
```

Если runtime даёт `CODEX_THREAD_ID`, `Amai` использует его как first-class chat scope,
чтобы не спутать текущий thread с предыдущим внутри одного project-space.
`previous chat` при этом означает ближайший thread по времени в том же
`project + namespace + agent_scope`, включая уже архивные чаты.

Если нужен текущий chat или chat на точный момент времени, используется тот же helper:

```bash
./scripts/chat_lookup.sh --project project_alpha --namespace continuity --chat-reference current --messages-count 2
./scripts/chat_lookup.sh --project project_alpha --namespace continuity --at-time-rfc3339 2026-03-21T11:41:00+03:00 --messages-count 2
./scripts/chat_lookup.sh --project project_alpha --namespace continuity --question "на чем закончили в прошлом чате, какие последние два сообщения?"
```

Это важный инвариант: `current / previous / exact time` идут одним temporal path, а не
разными эвристическими обходами.

Для обычного startup это теперь выглядит так:

```bash
./scripts/continuity_startup.sh --project project_alpha --namespace continuity
./scripts/continuity_restore.sh --project project_alpha --namespace continuity
```

Первый helper печатает:
- human-readable startup summary;
- `Chat-start restore pack`;
- готовый `prompt_text` для первого содержательного ответа;
- затем уже расширенное `working_state`.

Второй helper печатает raw JSON и теперь возвращает сразу два узла:
- `chat_start_restore`
- `working_state_restore`

Практический смысл:
- `chat_start_restore` нужен как короткий первичный injection pack;
- `working_state_restore` нужен как более широкий raw слой для аудита и машинного восстановления.
- `working_state_restore` теперь execution-aware:
  - `next_step_state` всегда показывает, что следующий шаг пока только `planned`;
  - `recent_actions[].execution_state` разводит `attempted / succeeded / superseded / stale`;
  - `state_lineage` хранит `lineage_model_version = lineage-v2`, authoritative event, supporting event ids, truth ranking и явный graph-слой `nodes / edges`;
  - `workspace_graph` хранит versioned structural runtime/workspace graph:
    `context_pack -> file / structure_item / symbol / chunk / import_ref / export_ref / call_ref`,
    а в `workspace-graph-v3` ещё и resolved relations `imports_file / re_exports_file / imports_symbol / re_exports_symbol / resolves_file / resolves_symbol / calls_file / calls_symbol / resolves_call_file / resolves_call_symbol`;
  - `workspace_graph_summary` в `chat_start_restore` и startup-output нужен как короткий human-readable слой поверх этого graph;
  - при битом `session_id` restore-path теперь fail-closed и не смешивает несколько пустых сессий в один bundle.

Если передан `--question`, helper сам должен:
- распознать `прошлый / текущий чат`;
- понимать `previous:2` и вопросы вида `2 чата назад` / `позапрошлый чат`;
- нормализовать относительное время вроде `в прошлую среду в 12:00`;
- понимать и `позапрошлую среду`;
- извлечь желаемый хвост сообщений вроде `последние два сообщения`.

Если задан момент времени вне известного диапазона чатов, temporal contour обязан fail-closed:
- не выбирать ближайший текущий chat молча;
- а отвечать, что для этого момента нет точного совпадения в известных чатах.

Дополнительно operational law теперь такой:
- direct CLI path через `cargo run --manifest-path /home/art/agent-memory-index/Cargo.toml -- ...` тоже должен поднимать `Amai .env` без ручного `cd` в repo root;
- user-facing temporal answers не должны выводить UUID thread-а или системные заголовки среды вместо человеческого label;
- к системному шуму относятся и IDE-import titles вида `# Context from my IDE setup`, `## Active file`, `## Open tabs`.

Пример:

```bash
AMAI_AGENT_SCOPE=agent_alpha ./scripts/continuity_startup.sh --project project_alpha --namespace continuity
AMAI_AGENT_SCOPE=agent_beta  ./scripts/continuity_startup.sh --project project_alpha --namespace continuity
```

Это нужно, чтобы параллельные агенты не поднимали и не продолжали чужую рабочую линию.

## Warmup after bootstrap

Если cold-start нужно ускорить сразу после поднятия стека:

```bash
./scripts/warmup_cache.sh --projects project_alpha,project_beta
```

Если в `.env` задан `AMI_WARMUP_PROJECTS`, то:
- `bootstrap_stack.sh` сам вызовет `warmup_cache.sh`;
- warmup будет best-effort;
- незарегистрированные проекты будут честно перечислены в `skipped`, а bootstrap не сорвётся.

## Deployment profiles

Канонический registry профилей:

```bash
config/deployment_profiles.toml
```

Сейчас materialized два профиля:
- `default`
  - основной workstation/full baseline;
- `lite_vps`
  - cheap remote smoke/demo baseline.

Проверить машину под профиль:

```bash
./scripts/preflight.sh
./scripts/preflight.sh --stack-profile default
./scripts/preflight.sh --stack-profile lite_vps
```

Важно не путать:
- `install_amai.sh`
  - может перейти к реальной установке после выбора профиля и подтверждения;
- `preflight.sh`
  - ничего не устанавливает и ничего не меняет;
  - это режим только для проверки машины и выбора подходящего профиля.

Preflight показывает обычным человеческим языком:
- какой профиль выбран;
- minimum и recommended requirements;
- подходит ли машина под минимум;
- для чего профиль подходит;
- для чего профиль не подходит.

Принцип тут честный:
- `lite_vps` не скрывает ограничения;
- он специально нужен, чтобы пользователь сразу понимал границы ожиданий, а не узнал о них после неудачного benchmark-запуска.

## Deployment targets

`Deployment profile` и `deployment target` — это не одно и то же.

Разница простыми словами:
- `deployment profile`
  - отвечает на вопрос, насколько сильная у вас машина;
- `deployment target`
  - отвечает на вопрос, какой вообще способ развёртывания вы хотите использовать.

Канонический registry режимов теперь хранится в:

```bash
config/deployment_targets.toml
```

Быстрый список:

```bash
./scripts/deployment_targets.sh
```

Подробно по одному режиму:

```bash
cargo run -- deployment explain --target local_docker
cargo run -- deployment explain --target kubernetes_server
```

Готовность именно этой машины:

```bash
./scripts/deployment_preflight.sh --target local_docker
./scripts/deployment_preflight.sh --target remote_ssh
./scripts/deployment_preflight.sh --target kubernetes_server
./scripts/deployment_preflight.sh --target windows_vm_lab
```

Каноническая трактовка на этом шаге такая:
- `local_docker`
  - текущий главный baseline;
- `remote_ssh`
  - уже materialized client/server path;
- `kubernetes_server`
  - следующий team/server deployment layer;
- `windows_vm_lab`
  - отдельный validation contour для честной Windows-проверки через VM.

## Status

```bash
./scripts/status.sh
```

Важно:
- для `Qdrant` и `NATS` канонический health source в этом проекте — не Docker health flag, а именно `status.sh` и `compat check`;
- это сделано специально, чтобы не зависеть от наличия `wget/curl/sh` внутри сторонних контейнерных образов.

## Compatibility check

```bash
cargo run -- compat check
```

Если здесь `FAIL`, дальше нельзя честно считать stack стабильным.
Сначала нужно убрать drift между поддерживаемым профилем и live версиями сервисов.

## Register a project

```bash
cargo run -- project register \
  --code project_alpha \
  --display-name "Project Alpha" \
  --repo-root /path/to/project-alpha
```

## Ensure a workspace inside the project

`namespace` здесь означает именованную рабочую область внутри проекта.
Она нужна для правил поиска и доступа.

```bash
cargo run -- namespace ensure \
  --project project_alpha \
  --code review \
  --display-name Review \
  --retrieval-mode local_strict
```

## Add a relation

```bash
cargo run -- relation add \
  --source project_alpha \
  --target project_beta \
  --relation-type shared_runtime \
  --shared-contour common_contour \
  --access-mode local_plus_related
```

## Index a project

```bash
cargo run -- index project \
  --code project_alpha \
  --path /path/to/project-alpha \
  --namespace default
```

## Build a context pack

```bash
cargo run -- context pack \
  --project project_alpha \
  --namespace review \
  --query "how configuration is loaded" \
  --retrieval-mode local_strict
```

Результат:
- печатается в stdout как JSON;
- кэшируется в SQLite;
- сохраняется в PostgreSQL;
- выгружается в S3 context bucket.
- внутри payload теперь отдельно materialize-ится `workspace_graph`:
  это компактный structural graph по уже найденным scoped-артефактам, а не догадка “по соседним файлам”.
- этот graph собирается только из того, что уже лежит в scope:
  `file`, `structure_item`, `symbol`, `chunk`, `import_ref`, `export_ref`, `call_ref`;
- в `workspace-graph-v3` он теперь дополнительно materialize-ит resolved file-to-file, file-to-symbol и conservative call lineage
  только там, где target реально существует в том же `project + namespace + scope`; неоднозначный target остаётся неразрешённым, а недоказуемый call остаётся просто `call_ref` без target-edge;
- тот же graph потом без дополнительного reparse попадает в `working_state_restore`.

Важно:
- `namespace` участвует в retrieval буквально;
- если вы запросили `default`, `Amai` не должен молча тянуть `smoke` или другой namespace того же проекта;
- если related project не имеет такого же namespace code, он просто не попадает в scope этого `context pack`.

## MCP server

Локальный MCP server:

```bash
cargo run -- mcp serve
```

Практический runner для IDE и других клиентов:

```bash
./scripts/run_mcp_stdio.sh
```

Этот runner:
- поднимает `.env`;
- не заставляет клиента дублировать внутренние credentials;
- стартует `amai mcp serve` как stdio MCP server.

## MCP client config

Сгенерировать client-specific snippet можно прямо из `Amai`:

```bash
cargo run -- mcp config --client generic
cargo run -- mcp config --client vscode --output .vscode/mcp.json
cargo run -- mcp config --client cursor
cargo run -- mcp config --client claude-code
cargo run -- mcp config --client claude-desktop
cargo run -- mcp config --client codex
```

Если нужен platform-specific launcher:

```bash
cargo run -- mcp config --client cursor --launcher-platform windows-powershell
cargo run -- mcp config --client codex --launcher-platform windows-cmd
```

Если `Amai` уже живёт на удалённом Linux/VPS-host:

```bash
cargo run -- mcp config \
  --client vscode \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai
```

Если auto-discovery корня не сработал:

```bash
cargo run -- mcp config --client vscode --cwd /path/to/art-memory-agent-index
```

Подробный user-facing walkthrough:
- [MCP_INTEGRATION.md](/home/art/agent-memory-index/docs/MCP_INTEGRATION.md)

## Onboarding

Если нужен один более простой вход, без ручной склейки шагов:

```bash
./scripts/onboard_local.sh --client vscode
./scripts/onboard_local.sh --client cursor
./scripts/onboard_local.sh --client codex
./scripts/onboard_local.sh --client claude-code
```

По умолчанию onboarding:
- работает внутри текущего repo root;
- пишет config в target path из `config/client_targets.toml`;
- для user-scope клиентов умеет создавать backup перед изменением файла.
- launcher platform тоже может быть указан явно:
  - `auto`
  - `linux`
  - `macos`
  - `windows-cmd`
  - `windows-powershell`

Текущие default outputs:
- `vscode` -> `.vscode/mcp.json`
- `cursor` -> `${home}/.cursor/mcp.json`
- `claude-code` -> `.mcp.json`
- `claude-desktop` -> `tmp/onboarding/claude-desktop-mcp.json`
- `codex` -> `${home}/.codex/config.toml`
- `generic` -> `tmp/onboarding/generic-mcp.json`

Proof:

```bash
./scripts/proof_install_auto.sh
./scripts/proof_onboarding.sh
./scripts/proof_remote_onboarding.sh
./scripts/proof_client_lifecycle.sh
./scripts/proof_profiles.sh
```

## Disconnect

Симметричное удаление клиента:

```bash
./scripts/disconnect_local.sh --client vscode
./scripts/disconnect_local.sh --client cursor
./scripts/disconnect_local.sh --client codex
./scripts/disconnect_local.sh --client claude-code
```

При disconnect:
- удаляется только запись `Amai`, а не весь чужой config целиком;
- если файл после этого становится пустым и включён `purge_empty_file`, пустой файл удаляется;
- для user-scope config перед изменением создаётся backup.

## Platform launchers

Materialized runner files:

```text
scripts/run_mcp_stdio.sh
scripts/run_mcp_stdio.ps1
scripts/run_mcp_stdio.cmd
```

Это значит:
- Linux/macOS path можно обслуживать shell launcher'ом;
- Windows path можно обслуживать через `cmd` или `PowerShell`;
- client config generation теперь умеет честно учитывать platform launcher, а не только Unix-style путь.
- удалённый Linux/VPS-host теперь можно подключать через `ssh` как stdio-transport, не выставляя внутренние базы наружу.

## Hardening proof

Быстрый локальный proof:

```bash
./scripts/proof_local.sh
```

Более жёсткий proof:

```bash
./scripts/proof_hardening.sh
```

Он дополнительно проверяет:
- повторный bootstrap;
- compatibility profile;
- multi-project isolation на fixture-проектах;
- controlled cross-project reading;
- restart recovery после `docker compose restart`.

## Performance proof

```bash
./scripts/proof_performance.sh
```

Этот proof:
- индексирует fixture-проекты с эмбеддингами;
- гоняет и `hot`, и `cold` retrieval path;
- мерит `mean/p50/p95/p99/max`;
- считает hot-path в микросекундах и публикует его как дробные миллисекунды;
- fail-ит, если practical latency baseline выходит за заданные thresholds.

Прямая Rust-команда:

```bash
cargo run --release -- verify benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --warmup 1 \
  --iterations 5 \
  --persist
```

Для cold-path добавляется:

```bash
cargo run --release -- verify benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --disable-cache \
  --warmup 1 \
  --iterations 5 \
  --persist
```

Важно:
- без `--disable-cache` измеряется `hot retrieval`;
- с `--disable-cache` измеряется `cold retrieval`.

Текущий репозиторный guard:
- hot benchmark должен удерживать `p95 < 10ms`
- hot benchmark должен удерживать `max < 15ms`

## End-to-end cold contour

Если нужен не component benchmark, а честный end-to-end cold retrieval contour:

```bash
./scripts/materialize_cold_repo_pool.sh \
  --manifest state/cold-benchmark/generated_manifest.toml \
  --target-repo-count 75 \
  --target-query-slice-count 200

./scripts/cold_benchmark.sh \
  --manifest state/cold-benchmark/generated_manifest.toml \
  --cycles 5 \
  --output-dir state/cold-benchmark/latest
```

Этот путь теперь канонический для большого cold contour:
- сначала materialize-ится repo-pool в `state/cold-benchmark/repo-pool`;
- затем generated manifest указывает только на реально существующие exact relative paths;
- если manifest пытается сослаться на отсутствующий файл, `materialize_cold_repo_pool.sh` падает сразу, а не оставляет скрытый drift до benchmark-стадии.

Для компактного proof-run на реальных локальных репозиториях:

```bash
./scripts/proof_cold_benchmark.sh
```

Этот proof-manifest теперь специально включает:
- `Art` как `docs_heavy + large_monorepo`;
- `Amai` как `code_heavy`;
- дополнительные mixed-repo cases.

Если для repo в proof включён `skip_embeddings = true`, runner не должен искусственно платить за semantic path на scope без vector points:
- в таком случае retrieval честно short-circuit-ится;
- cold contour меряет реальный end-to-end lexical/doc path, а не пустой embed хвост.

Этот runner считает:
- отдельно `cold` и `hot shadow`;
- `P50 / P95 / P99 / max / sample_count`;
- `precision / recall / target-hit rate / miss rate / fallback rate`;
- отдельные fixed targets для:
  - `Cold P50 / P95 / P99 / Max`;
  - `precision / recall / hit rate`;
  - `sample_count / repo_count / query_slice_count`;
  - `duration / leakage / error rate`;
- stage breakdown по:
  - `policy`
  - `retrieval`
  - `ranking`
  - `provenance`
  - `pack assembly`
  - `orchestration`;
- hardware / disk / thermal guard;
- cleanup actions;
- итоговый verdict:
  - `TARGET MET`
  - `PARTIALLY MET`
  - `NOT MET`.

Артефакты последнего run:
- `state/cold-benchmark/latest/summary.json`
- `state/cold-benchmark/latest/report.md`
- `state/cold-benchmark/latest/samples.csv`

Для real-project text compare без полного индексирования монорепозитория теперь есть отдельный proof:

```bash
./scripts/proof_text_compare_real_projects.sh
```

Что он делает:
- читает `fixtures/real_project_text_compare_cases.jsonl`;
- из `expected_paths` строит точные allowlist-файлы по `Art` и `Amai`;
- индексирует оба проекта через `index project --paths-file ... --skip-embeddings`;
- затем запускает `verify text-compare` уже на реальных локальных путях и relation-графе `Art -> Amai`.

Кроме файлов, runner теперь ещё и пишет snapshot `cold_path_benchmark`, поэтому:
- human dashboard показывает сервисную карточку `Cold contour`;
- `/metrics` публикует:
  - `amai_cold_contour_p50_ms`
  - `amai_cold_contour_p95_ms`
  - `amai_cold_contour_p99_ms`
  - `amai_cold_contour_max_ms`
  - `amai_cold_contour_precision`
  - `amai_cold_contour_recall`
  - `amai_cold_contour_hit_rate`
  - `amai_cold_contour_fallback_rate`
  - `amai_cold_contour_target_met`.

## Accuracy proof

```bash
./scripts/proof_accuracy.sh
```

Или напрямую:

```bash
cargo run --release -- verify accuracy \
  --project project_alpha \
  --related-project project_beta \
  --namespace review \
  --manifest config/red_team_retrieval_isolation.toml
```

Этот proof:
- проверяет `local_strict` на отсутствие cross-project leakage;
- проверяет отсутствие cross-namespace leakage внутри одного и того же проекта;
- гоняет hostile mixed query из versioned red-team manifest;
- мерит `exact_precision`, `lexical_precision`, `symbol_precision`, `semantic_precision`;
- сохраняет snapshot `retrieval_accuracy`;
- теперь пишет в snapshot:
  - `formal_invariants`;
  - `retrieval_science`;
  - `degradation_policy`;
  - versioned suite metadata из `config/red_team_retrieval_isolation.toml`;
  - отдельные hostile visible/hit invariants по проекту и namespace.

Эти данные теперь используются ещё и как живое evidence для `degradation_model`:
- `cross_project_scope` считается подтверждённым только если zero leakage и проходят все нужные `formal_invariants`;
- `cross_namespace_scope` считается подтверждённым только если zero leakage и проходят namespace-invariants: `strict_local_*`, `hostile_mixed_query_*` и `namespace_strict_*`;
- если proof неполный или snapshot отсутствует, класс честно остаётся в `unknown`, а не красится в зелёный.

## Load proof

```bash
./scripts/proof_load.sh
```

Или напрямую:

```bash
cargo run --release -- verify load \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --workers 8 \
  --iterations-per-worker 250 \
  --warmup-per-worker 2
```

Этот proof:
- мерит concurrent hot-load contour;
- выдаёт `qps`, `error_rate`, `p50/p95/p99/max`;
- сохраняет snapshot `retrieval_load_hot`;
- теперь тоже stamp-ит `retrieval_science` и `degradation_policy`, чтобы результат был воспроизводимым и machine-readable.

## Degradation model

Machine-readable карта деградации и partial-failure поведения теперь собирается прямо в live system snapshot:
- policy source of truth: `config/retrieval_science.toml`;
- raw JSON: `/api/snapshot -> degradation_model`;
- human-visible слой: service-card `Поведение при сбоях`;
- Prometheus:
  - `amai_degradation_pass_total`
  - `amai_degradation_critical_total`
  - `amai_degradation_unknown_total`
  - `amai_degradation_fail_closed_total`
  - `amai_degradation_graceful_fallback_total`
  - `amai_degradation_evidence_gaps_total`

Важно:
- зелёным считаются только классы со свежим machine-readable proof;
- классы без свежего proof не маскируются под `pass`, а остаются `unknown`;
- working-state freshness/confidence может выступать evidence only как сигнал, но не как полноценный proof для isolation/degradation класса, пока не materialized отдельный suite.

Отдельный runnable proof path для этих классов теперь есть:

```bash
cargo run --release -- verify degradation
```

Сейчас он materialize-ит synthetic verification для:
- `cross_agent_scope`
- `corrupt_scope_metadata`
- `partial_refresh`
- `partial_thread_index`
- `qdrant_unavailable`
- `stale_cache`
- `empty_embeddings`
- `stale_handoff`
- `working_state_conflict`

Этот proof:
- использует те же working-state / restore / temporal / lexical fallback алгоритмы, что и product path;
- пишет snapshot `degradation_verification`;
- даёт `last known evidence` для `degradation_model`, поэтому эти классы больше не висят как чистый policy-only gap;
- versioned через `science.suites.degradation_verification`, чтобы same input -> same verdict проверялся как отдельный retrieval-science contour;
- для `working_state_conflict` теперь пишет `lineage-v2` graph (`nodes / edges`), а не только плоский набор supporting event ids;
- дополнительно опирается на property-based tests для fail-closed выбора `agent_scope / session_id` и для exact-time drift в temporal lookup.

Текущий репозиторный guard:
- `qps > 35000`
- `p50 < 0.012ms`
- `p95 < 0.015ms`
- `p99 < 0.020ms`
- `max < 0.5ms`
- `error_rate = 0`
- `workers > 16`
- `sample_count > 10000`

Важно:
- если после warmup `verify load` пишет `execution_mode = hot_cache_only`, это правильный и желаемый режим;
- в этом режиме verifier честно мерит process-local hot retrieval, а не открывает PostgreSQL connection на каждого worker;
- возврат к per-worker DB connections для hot-load считается регрессом, даже если код “выглядит проще”.

## Stress scale proof

```bash
./scripts/proof_stress_scale.sh
```

Этот proof:
- поднимает fixture stack;
- прогревает hot cache;
- последовательно гоняет `50`, `100` и `200` workers;
- fail-ит, если `p95 >= 10ms`, `qps < 5000` или появляется `error_rate`.

Текущий честный measured baseline на референсной машине:
- CPU:
  - `AMD Ryzen 9 7900X 12-Core Processor`
  - `24` логических CPU
- RAM:
  - `62 GiB`
- `50 workers`
  - `p95 = 0.026 ms`
  - `qps ≈ 384 024`
- `100 workers`
  - `p95 = 0.023 ms`
  - `qps ≈ 434 593`
- `200 workers`
  - `p95 = 0.020 ms`
  - `qps ≈ 670 016`

Эти цифры относятся именно к `hot cached retrieval`.
Cold/full path нужно оценивать отдельно через `proof_performance.sh` и при необходимости заранее прогревать `warmup_cache.sh`.

## Token benchmark proof

```bash
./scripts/proof_token_benchmark.sh
./scripts/proof_token_benchmark_suite.sh
```

Или напрямую:

```bash
cargo run --release -- verify token-benchmark \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --tokenizer o200k_base
```

Этот proof:
- считает, сколько токенов потребовал бы наивный полный scope без retrieval reduction;
- строит компактный LLM-ready render текущего `context pack`;
- сравнивает оба результата на одном tokenizer;
- сохраняет snapshot `token_benchmark`.

## Benchmark matrix

Это отдельный Rust-first contour, который переводит внешний benchmark landscape в machine-readable карту для `Amai`.

Источник внешней таксономии:
- `philschmid/ai-agent-benchmark-compendium`
- <https://github.com/philschmid/ai-agent-benchmark-compendium>

Запуск:

```bash
./scripts/benchmark_matrix.sh
./scripts/benchmark_matrix.sh coverage
./scripts/benchmark_matrix.sh explain --benchmark live-mcpbench
./scripts/benchmark_matrix.sh explain --benchmark "SWE-bench Verified"
```

Что означает этот contour:
- `list`
  - печатает каноническую карту benchmark-семейств и их текущий статус для `Amai`;
- `coverage`
  - показывает общую сводку: что уже materialized, что частично покрыто, что только mapped и что стоит следующим обязательным приоритетом;
- `explain`
  - объясняет один benchmark человеческим языком:
  - зачем он нужен именно `Amai`;
  - какие proof-контуры уже есть;
  - где честные пробелы;
  - какой следующий шаг обязателен.

Отдельный proof:

```bash
./scripts/proof_benchmark_matrix.sh
```

Этот proof:
- проверяет, что список benchmark-семейств печатается из machine-readable registry;
- проверяет coverage summary;
- проверяет explain path по alias и по human-readable benchmark name;
- fail-ит, если benchmark registry или CLI drift-нули.

Отдельно materialized и внешний retrieval/vector readiness contour:

```bash
cargo run -- benchmark external-check
cargo run -- benchmark external-explain --benchmark vectordbbench
cargo run -- benchmark external-datasets
cargo run -- benchmark external-download --dataset dbpedia_openai_1000k_angular
cargo run -- benchmark external-plan --benchmark vectordbbench
cargo run -- benchmark external-adapter --benchmark vectordbbench --dataset dbpedia_openai_1000k_angular
cargo run -- benchmark external-harvest --benchmark vectordbbench --dataset dbpedia_openai_1000k_angular
./scripts/proof_external_benchmark_env.sh
./scripts/proof_external_benchmark_adapter.sh
```

Это не замена `Amai` end-to-end benchmark-ам.
Его задача уже другая:
- проверить, что эта машина готова к внешним comparative benchmark-ам;
- честно развести:
  - `general framework + adapter`
  - `ANN core ceiling`
  - `payload/filter pressure`.

Текущий канонический порядок для Amai:
- `VectorDBBench`
  - как первый apples-to-apples framework для vector/filter layer;
- `ann-benchmarks`
  - как ceiling retrieval-core;
- `Filtered ANN benchmark datasets`
  - как payload/filter layer;
- и только после этого сопоставлять всё с внутренним `Amai` cold/hot contour.

Теперь у этого слоя materialized ещё и dataset-manifest:

```bash
config/external_benchmark_datasets.toml
```

Там уже канонически зафиксированы стартовые HDF5 datasets:
- `dbpedia-openai-1000k-angular`
- `snowflake-msmarco-arctic-embed-m-v1.5-angular`
- `sift-128-euclidean`
- `sphere-10M-meta-dpr`

Их задача:
- не заменить внутренний Amai dataset contour;
- а дать фиксированный внешний comparative baseline для adapter-пути.

Следующий слой теперь уже не теоретический:
- `external-download`
  - Rust-контур для скачивания одного dataset-а или всего manifest-а;
  - `external-adapter`
  - готовит реальный run workspace:
  - `summary.json`
  - `report.md`
  - `run_external.sh`
  - `run_status.json`
  - `run_external.log`
  - для `VectorDBBench` теперь уже materialize-ит Rust conversion contour:
    - читает HDF5 dataset;
    - пишет custom Parquet bundle `train/test/neighbors`;
    - кладёт рядом `conversion_manifest.json`;
    - только после этого помечает adapter как `prepared`;
    - перед новым run очищает старый `vectordbbench-results`, чтобы `latest/` не подмешивал прошлые `result_*.json`;
    - materialize-ит Amai-managed local compatibility patch для `QdrantLocal`:
      - transport `timeout = 600`;
      - расширение upstream CLI флагом `--timeout`;
      - без изменения case threshold и dataset semantics;
    - затем честно запускает upstream `vectordbbench qdrantlocal ...`;
    - если в `.env` заданы `AMI_BENCHMARK_QDRANT_HTTP_URL` и `AMI_BENCHMARK_QDRANT_COLLECTION_CODE`, human dashboard отдельно показывает живые системные числа именно этого benchmark-Qdrant;
  - для `ann-benchmarks` `run_external.sh` теперь идёт безопасным upstream path:
    - локальный `.venv`
    - `pip install -r requirements.txt`
    - symlink dataset в `data/<dataset>.hdf5`
    - `python install.py --algorithm qdrant`
    - `python run.py --dataset ... --algorithm qdrant`
  - `external-harvest`
    - читает workspace короткой сводкой без разбора полного Docker-лога;
    - поднимает `result_verdict` и label из реальных `result_*.json`, а не только `exit code`;
  - `docker compose up` здесь больше не является каноническим launch-path, потому что при отсутствии compose-файла upstream он может по ошибке подцепить parent compose другого проекта;
  - если dataset подходит напрямую и upstream default path реально доступен, workspace помечается как `prepared`;
  - если dataset не скачан, это честно помечается как `blocked_dataset_missing`;
  - если upstream canonical launch path сам помечен как `disabled: true`, это честно помечается как `blocked_upstream_disabled`;
  - если benchmark требует другой формат данных, это честно помечается как `blocked_conversion_required`.

Текущий важный инвариант:
- `ann-benchmarks` принимает HDF5 datasets напрямую;
- но только если upstream уже знает этот dataset по имени в своём `DATASETS` registry;
- если upstream qdrant path помечен `disabled: true` в canonical config, `Amai` не имеет права продолжать показывать такой contour как `prepared`;
- если файл существует, но dataset не поддержан upstream напрямую, `Amai` обязан пометить contour как `blocked_unsupported_dataset`;
- `VectorDBBench` custom dataset contour не должен притворяться, что принимает те же HDF5 напрямую;
- поэтому `Amai` теперь сначала materialize-ит Parquet bundle `train/test/neighbors`,
  и только потом запускает реальный upstream contour;
- `summary.json` теперь должен явно нести `compatibility_overrides`, если честный запуск требует
  локального Amai-managed patch в upstream workspace;
- для этого Rust conversion path нужен `cmake` в `PATH`, иначе bundled HDF5 crate не соберётся.

## MCP task matrix

Это следующий слой после обычного `proof_mcp.sh`.

Если `proof_mcp.sh` отвечает на вопрос:
- жив ли MCP contour вообще?

то `proof_mcp_task_matrix.sh` отвечает уже на другой вопрос:
- выдерживает ли `Amai` measured набор задач класса `LiveMCPBench / MCP-Universe`, включая hostile и isolation-path?

Запуск:

```bash
./scripts/proof_mcp_task_matrix.sh
```

Или напрямую:

```bash
cargo run -- verify mcp-matrix --matrix live_mcpbench_local --project project_alpha --related-project project_beta --namespace review
cargo run -- verify mcp-matrix --matrix mcp_universe_local --project project_alpha --related-project project_beta --namespace review
```

Что делает этот contour:
- поднимает measured task matrix вместо одного smoke;
- считает `tasks_total`, `tasks_passed`, `tasks_failed`, `success_rate`;
- считает `mean/p50/p95/max latency`;
- раскладывает задачи по классам:
  - `happy_path`
  - `hostile`
  - `isolation`

Что именно проверяется сейчас:
- стабильность MCP tool catalog;
- project и namespace discovery;
- `local_strict` isolation;
- `local_plus_related` routing;
- live observe snapshot;
- live token report headline;
- warm cache через MCP;
- fail-closed на `unknown tool`;
- fail-closed на `unknown project`;
- fail-closed на `unknown namespace`;
- default continuity restore для канонического agent scope;
- fail-closed restore для изолированного agent scope.

Это уже не “кажется, MCP работает”, а measured local benchmark contour с честным pass/fail и class breakdown.

## Memory task matrix

Это отдельный measured contour уже не про `MCP`, а про саму память `Amai`.

Референс здесь другой:
- `Letta Leaderboard`
- <https://www.letta.com/blog/letta-leaderboard>

Запуск:

```bash
./scripts/proof_memory_task_matrix.sh
```

Или напрямую:

```bash
cargo run -- verify memory-matrix --matrix letta_memory_local
```

Что делает этот contour:
- мерит память отдельно по двум слоям:
  - `core`
  - `archival`
- отдельно проверяет 4 класса задач:
  - `read`
  - `write`
  - `update`
  - `isolation`

Что именно проверяется сейчас:
- `core memory read`
  - поднимает ли `working-state restore` сохранённый факт;
- `core memory write`
  - переживает ли факт новый `restore`;
- `core memory update`
  - остаётся ли новое значение главным, а старое уходит из authoritative поля;
- `core scope isolation`
  - не течёт ли память между разными `agent_scope`;
- `archival memory read`
  - находит ли `context pack` сохранённый continuity-документ;
- `archival memory write`
  - становится ли новый факт реально searchable;
- `archival memory update`
  - вытесняет ли новый факт старый в archival слое;
- `archival project isolation`
  - не течёт ли continuity-документ в соседний проект.

Что считает этот contour:
- `tasks_total`
- `tasks_passed`
- `tasks_failed`
- `success_rate`
- `mean_score`
- `p50/p95/max latency`
- `class_breakdown`
- `layer_breakdown`

Это полезно тем, что `Amai` теперь можно проверять не только словами “у нас есть память”, а реальным локальным экзаменом:
- умеет ли память читать;
- умеет ли писать;
- умеет ли обновлять конфликтующий факт;
- не ломает ли изоляцию.

## Token benchmark suite proof

```bash
./scripts/proof_token_benchmark_suite.sh
```

Или напрямую:

```bash
cargo run --release -- verify token-benchmark-suite \
  --project project_alpha \
  --namespace review \
  --retrieval-mode local_plus_related \
  --queries-file "$PWD/fixtures/token_benchmark_queries.txt" \
  --tokenizer o200k_base \
  --naive-limit-files 20 \
  --naive-max-bytes-per-file 32768 \
  --min-mean-savings-factor 1.2 \
  --min-mean-savings-percent 15
```

Этот proof:
- берёт список запросов, а не один удачный пример;
- строит агрегированный `token_benchmark_suite` snapshot;
- считает `mean/p50/p95` по `saved_tokens`, `savings_factor`, `savings_percent`;
- нужен как более честный product contour, который другой пользователь сможет воспроизвести на том же fixture наборе.

## Token ledger report

Если нужно увидеть не только последний benchmark, а накопительный эффект:

```bash
./scripts/token_report.sh
```

Если хотите отдельно смотреть 5-часовое окно Codex:

```bash
./scripts/token_report.sh --budget-profile codex_5h
```

Канонический spec этого слоя:
- [TOKEN_LEDGER.md](/home/art/agent-memory-index/docs/TOKEN_LEDGER.md)

Отдельный proof:

```bash
./scripts/proof_token_ledger.sh
```

Что показывает этот contour:
- `headline`
  - канонический product KPI:
    - `Verified Effective Savings %`
    - по-русски: `Проверенная реальная экономия`;
- `current_session`
  - токены, сэкономленные в текущей рабочей сессии;
- `rolling_window`
  - токены, сэкономленные в текущем лимитном окне профиля;
- `lifetime`
  - токены, сэкономленные за всё записанное время;
- `source_breakdown`
  - откуда пришли цифры:
    - живые `context pack` вызовы;
    - benchmark-события, если вы их явно включили.
- `query_slices`
  - отдельные срезы по типам запросов:
    - `code_lookup`
    - `docs_lookup`
    - `symbol_lookup`
    - `architecture_question`
    - и другие, если они реально накоплены в live-потоке.
- `quality_ok_rate`
  - сколько событий прошло quality gate;
- `fallback_rate`
  - как часто retrieval приходилось чинить повторным ходом;
- `answer_like_rate`
  - какая доля live-событий уже дошла до более строгого answer-like proxy, а не остановилась только на retrieval parity.
- `temperature_slices`
  - отдельные срезы по состоянию retrieval:
    - `cold`
    - `warm`
    - `post_restart`
    - `post_warmup`
    - `post_reindex`
- `median_recovery_tokens`
  - медианный штраф на follow-up/retry/correction токены;
  - нужен затем, чтобы видеть не только красивую экономию, но и цену ошибок retrieval.

Главная честная поправка теперь такая:
- headline считается не просто по raw savings;
- recovery penalties теперь вычитаются из результата;
- при live report один follow-up штрафует только ближайшее подходящее незакрытое событие, а не раздувает статистику несколько раз.
- в текущем runtime это уже materialized не только на уровне отчёта:
  - live events получают `session_id`;
  - получают `rolling_window_profile`;
  - пишут канонические alias-поля `project_code`, `namespace_code`, `baseline_tokens`, `delivered_tokens`, `gross_savings_pct`;
  - и baseline strategy больше не схлопывается почти в одну ветку:
    - `ide_search_top_files` для file/config/symbol path;
    - `semantic_top_k` для architecture/bugfix path;
    - `legacy_pre_amai` для onboarding path;
  - quality gate больше не состоит только из `quality_ok`:
    - runtime пишет `quality_tier`;
    - пишет `head_hit_target`;
    - summary считает `answer_like_rate`;
    - summary считает `verified_answer_like_savings_pct`;
    - summary считает `task_success_like_rate`;
    - summary считает `verified_task_like_savings_pct`;
    - `hybrid_answer_proxy` означает, что контекст уже выглядит достаточным для полезного ответа без follow-up;
  - успешный recovery-follow-up может получить `quality_method = hybrid_answer_success`;
  - и его `recovery_tokens` уже включают стоимость предыдущего промаха.

По умолчанию verification-трафик не смешивается с обычной рабочей активностью.
Если нужно показать всё вместе:

```bash
cargo run --release -- observe token-report --include-verify-events true
```

Если в базе уже есть старые live `token_budget_event`, записанные до quality-gated формата, канонический путь теперь такой:

```bash
cargo run --release -- observe repair-token-ledger --apply
cargo run --release -- observe reverify-token-ledger --apply
```

Смысл по-человечески:
- `repair-token-ledger`
  - чинит старые записи без ручного SQL;
  - достраивает недостающие поля нового ledger-формата;
- `reverify-token-ledger`
  - повторно прогоняет старые live-запросы через текущий retrieval contour;
  - если retrieval реально находит достаточный контекст, событие становится `quality_ok = true`;
  - после этого headline может перейти из `предварительно` в полноценную `Проверенную реальную экономию`.

Для observability snapshot у временного synthetic/debug хвоста теперь есть канонический retention-cleanup:

```bash
cargo run --release -- observe cleanup-snapshots --limit 200
cargo run --release -- observe cleanup-snapshots --apply --limit 2000
```

Это нужно для двух разных задач:
- dry-run честно показывает, сколько aged snapshot уже подпадает под TTL;
- apply-path удаляет только те записи, которые policy реально разрешает удалить;
- benchmark history не переписывается in-place, потому что benchmark snapshot теперь stamped как `immutable_snapshot`;
- версия правил и retention profile приходят из [config/observability.toml](/home/art/agent-memory-index/config/observability.toml) и materialize-ятся в `_observability`.

Для rebuildable локального хвоста теперь есть отдельный policy-driven cleanup:

```bash
cargo run --release -- observe cleanup-artifacts --limit 20
cargo run --release -- observe cleanup-artifacts --apply --limit 20
cargo run --release -- observe cleanup-artifacts --aggressive --limit 20
cargo run --release -- observe cleanup-artifacts --aggressive --apply
```

Смысл по-человечески:
- чистится только rebuildable мусор, который можно честно восстановить:
  - `target/debug`
  - `target/release`
  - `.fastembed_cache`
  - `state/external-benchmarks/*`
- live state (`state/postgres`, `state/qdrant`, `state/minio`, `state/nats`) сюда специально не входит;
- список путей, TTL и `keep_latest` живут в [config/observability.toml](/home/art/agent-memory-index/config/observability.toml);
- auto-path защищает текущий исполняемый бинарь от удаления;
- `observe serve`, `observe snapshot` и `observe sla-check` сами запускают этот cleanup, поэтому локальный мусор должен уходить без ручного обхода.
- `--aggressive` — это уже explicit reclaim path: он не ждёт TTL и не держит `keep_latest`, но всё равно не лезет в live state и защищает текущий исполняемый бинарь.
- human dashboard теперь показывает отдельную карточку `Локальный мусор и retention`, чтобы оператор видел:
  - safe reclaim now;
  - aggressive preview;
  - last reclaim;
  - почему объём проекта может ещё не уменьшаться, даже если aggressive preview уже большой.
- после любого `--apply` summary сразу пересчитывается повторным dry-run, поэтому карточка и warning не должны продолжать показывать уже удалённый хвост как будто он всё ещё лежит на диске.

После `reverify` live event теперь должен нести richer fields:
- `target_kind`
  - какой тип результата нужен запросу;
- `baseline_hit_target`
  - был ли у baseline честный шанс закрыть задачу;
- `amai_hit_target`
  - попал ли `Amai` в нужный тип результата;
- `latency_ms`
  - время retrieval event;
- `file_hits`, `document_hits`, `symbol_hits`
  - какие типы результатов реально пришли;
- `pack_token_count`, `deduped_token_count`
  - сколько токенов реально дошло до prompt после компактной сборки.

Важно:
- headline снимает пометку `предварительно`, если набран хотя бы один из двух порогов:
  - `events_count >= 50`
  - или `baseline_tokens >= 100000`;
- это соответствует принятой ledger-spec и не требует одновременно проходить оба порога.

## MCP proof

```bash
./scripts/proof_mcp.sh
```

Отдельный proof для удалённого `ssh` config generation:

```bash
./scripts/proof_remote_ssh_config.sh
```

Сравнительный text contour:

```bash
./scripts/proof_text_compare.sh
```

Ручной запуск comparative benchmark:

```bash
cargo run -- verify text-compare \
  --project project_alpha \
  --namespace review \
  --retrieval-mode local_plus_related \
  --cases-file fixtures/text_compare_cases.jsonl
```

Или напрямую:

```bash
cargo run --release -- verify mcp \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related
```

Этот proof:
- поднимает child MCP server;
- проходит `initialize`;
- проверяет `tools/list`;
- проверяет `prompts/list` и `prompts/get`;
- вызывает через MCP:
  - `amai_list_projects`
  - `amai_list_namespaces`
  - `amai_context_pack`
  - `amai_token_benchmark`
  - `amai_observe_snapshot`
  - `amai_warm_cache`.

Важно:
- на маленьких fixture-проектах экономия токенов будет честно умеренной;
- на больших реальных репозиториях этот contour должен расти заметно сильнее;
- proof нужен именно затем, чтобы показывать пользователю measured effect, а не обещание.

## Hostile proof

```bash
./scripts/proof_hostile.sh
```

Этот proof:
- специально создаёт `stack_meta` drift;
- по очереди выключает `postgres`, `qdrant`, `minio`, `nats`;
- проверяет, что compatibility path fail-closed;
- затем поднимает сервис обратно и доказывает recovery.

Прямая Rust-команда:

```bash
cargo run -- verify hostile --scenario all
```

Допустимые точечные сценарии:
- `stack_meta_drift`
- `postgres`
- `qdrant`
- `minio`
- `nats`

Текущий AST coverage:
- `rust`
- `toml`
- `javascript`
- `typescript`
- `tsx`
- `json`

Если файл попадает вне этого набора, индексер обязан перейти в lexical fallback, а не валить весь проход.

Для smoke-проверки:

```bash
cargo run -- index project \
  --code project_alpha \
  --path /path/to/project-alpha/src \
  --namespace review \
  --limit-files 10
```

Быстрый smoke без эмбеддингов:

```bash
cargo run -- index project \
  --code project_alpha \
  --path /path/to/project-alpha/src \
  --namespace review \
  --limit-files 5 \
  --skip-embeddings
```

## Observability / SLA proof

```bash
./scripts/proof_observability.sh
```

Или напрямую:

```bash
cargo run --release -- observe guardrails
cargo run --release -- observe snapshot
cargo run --release -- observe sla-check
cargo run --release -- observe cleanup-snapshots --limit 200
```

Что это даёт:
- DB-level negative proof для anti-replay, idempotent replay counter, immutable benchmark snapshot и contamination block;
- live snapshot по `PostgreSQL`, `Qdrant`, `NATS`, `S3-compatible storage`;
- последние `index_project` и `retrieval_benchmark` snapshots;
- последние `retrieval_accuracy` и `retrieval_load_hot` snapshots;
- последний `token_benchmark` snapshot;
- SLA-оценку по [observability.toml](/home/art/agent-memory-index/config/observability.toml).
- machine-readable `_observability` metadata со `schema_version`, `classification_rules_version`, `retention_profile`, `retention_class` и `immutable_snapshot`.
- Prometheus-ready `/metrics` exporter без persistence на каждый scrape.

Сейчас hot retrieval stretch-goal в SLA считается только по реальному measured `p95_ms`, а не по округлению до целых миллисекунд.

Сейчас `observe sla-check` fail-ит только если:
- есть `critical` нарушение;
- или есть `unknown`, то есть обязательный контур ещё не был измерен.

## Monitoring profile

## Human dashboard

Если нужен не инженерный scrape-слой, а обычная человеческая страница с живыми цифрами:

```bash
./scripts/human_dashboard.sh
./scripts/human_dashboard_down.sh
```

Windows PowerShell:

```powershell
.\scripts\human_dashboard.ps1
.\scripts\human_dashboard_down.ps1
```

Windows CMD:

```bat
scripts\human_dashboard.cmd
scripts\human_dashboard_down.cmd
```

Это поднимает тот же `observe serve`, но теперь он отдаёт сразу несколько уровней:
- `/`
  - human-first HTML dashboard;
- `/api/dashboard`
  - тот же смысл в удобном JSON для внешней автоматизации;
- `/api/snapshot`
  - полный live snapshot без human-упаковки;
- `/metrics`
  - Prometheus scrape layer;
- `/healthz`
  - быстрый health JSON.

Для обычного пользователя правильный путь теперь такой:
- сначала открыть human dashboard;
- а уже потом при необходимости идти глубже в Prometheus/Grafana.

Launcher human dashboard теперь:
- сам поднимает observe-server в фоне;
- на Linux предпочитает `systemd --user` сервис `amai-human-dashboard.service`, а не временный терминальный обход;
- для Linux живой статус и логи нужно смотреть через `systemctl --user status amai-human-dashboard.service` и `journalctl --user -u amai-human-dashboard.service`;
- если user-level `systemd` недоступен, launcher честно падает в обычный detached fallback с логом в `tmp/human_dashboard.log`;
- не требует держать терминал открытым только ради живой панели.
- по умолчанию обновляет страницу раз в `1` секунду, чтобы live-картина была ближе к текущему состоянию.
- если локальный `cmake` уже materialized в `state/tooling/cmake-venv/bin`, launcher сам подхватывает этот `PATH`, чтобы observability-path не падал только из-за отсутствия системного `cmake`.

Встроенный exporter:

```bash
./scripts/run_observe_exporter.sh
```

Prometheus + Grafana:

```bash
./scripts/render_monitoring_config.sh
./scripts/monitoring_up.sh
```

После этого доступны:
- `Prometheus`: `http://127.0.0.1:${AMI_PROMETHEUS_PORT:-59090}`
- `Grafana`: `http://127.0.0.1:${AMI_GRAFANA_PORT:-53000}`

Grafana login берётся из `.env`:
- user: `AMI_GRAFANA_ADMIN_USER`
- password: `AMI_GRAFANA_ADMIN_PASSWORD`

По умолчанию в dev baseline:
- user: `admin`
- password: `admin_change_me`

Канонические файлы:
- [config/prometheus/prometheus.yml](/home/art/agent-memory-index/config/prometheus/prometheus.yml)
- [config/prometheus/rules/alerts.yml](/home/art/agent-memory-index/config/prometheus/rules/alerts.yml)
- [config/grafana/dashboards/amai_stack.json](/home/art/agent-memory-index/config/grafana/dashboards/amai_stack.json)
- [scripts/render_monitoring_config.sh](/home/art/agent-memory-index/scripts/render_monitoring_config.sh)

Базовые алерты уже materialized:
- `AmaiQdrantIndexOptimizeQueueHigh`
- `AmaiNatsConsumerLagHigh`
- `AmaiPostgresReplicaLagHigh`
- `AmaiRetrievalHotBudgetMiss`
- `AmaiCrossProjectLeakageDetected`
- `AmaiBenchmarkContaminationDetected`
- `AmaiPostgresDeadlocksDetected`

Ключевой engineering law:
- scrape path не должен менять operational truth;
- поэтому `/metrics` собирает live snapshot read-only и не пишет `system_snapshot` в PostgreSQL на каждый Prometheus scrape;
- persistence остаётся только у явных `observe snapshot` и `observe sla-check`.
- `observe snapshot`, `observe sla-check` и `observe serve` теперь ещё запускают retention sweep для временных observability snapshot, чтобы synthetic/debug хвост не копился бесконечно;
- те же entrypoint-ы теперь ещё запускают policy-driven cleanup для rebuildable локального мусора по TTL из `config/observability.toml`;
- ручной `observe cleanup-snapshots --apply` остаётся явным операторским инструментом для отдельного dry-run/apply proof;
- ручной `observe cleanup-artifacts --apply` остаётся отдельным операторским путём для dry-run/apply по локальным build/cache хвостам;
- ручной `observe cleanup-artifacts --aggressive --apply` остаётся отдельным operator-only reclaim path, когда нужно быстро вернуть место без затрагивания live state;
- benchmark snapshot запрещено переписывать update-path-ом: для них действует `immutable_snapshot`, а lifecycle дальше управляется retention policy, а не тихим in-place drift.
- human dashboard использует тот же read-only snapshot contour и тоже не пишет state на каждый refresh;
- верхние hero-карты human dashboard теперь intentionally живут только на real live ledger:
  - текущая сессия;
  - текущее рабочее окно профиля;
  - всё время;
  - отдельный benchmark больше не подменяет собой третью карту.
- human dashboard теперь жёстко разводит три источника:
  - live поток текущей сессии;
  - живые system probes;
  - последний сохранённый benchmark;
- benchmark-карточки теперь обязаны прямо писать слово `benchmark` в source label, а live-карточки — прямо писать, что benchmark-данные туда не подмешиваются;
- `Qdrant` в human dashboard теперь intentionally разделён на два разных live-инстанса:
  - `Qdrant Amai live`
    - основной векторный слой `Amai`;
  - `Qdrant внешнего бенча`
    - отдельный live-инстанс для внешнего benchmark-прогона;
  - этим карточкам запрещено делить между собой `points / segments / resident memory`;
  - если внешний benchmark остановился, карточка `Qdrant внешнего бенча` не должна печатать error-reason:
    она обязана показывать последние успешные числа и жёлтый статус `тест не запущен`;
  - этот жёлтый статус теперь должен определяться по реальному состоянию benchmark-runner-а:
    одного живого `/metrics` у отдельного Qdrant уже недостаточно, если сам внешний run workspace больше не активен;
- live latency слой на human dashboard теперь схлопнут в один общий блок `Как Amai отвечает сейчас`, а не размазан по трём похожим карточкам;
- сверху в этом блоке стоят две крупные живые цифры:
  - `Повторный запрос`
  - `Первый запрос`;
- крупная цифра в каждой из двух плиток означает `P50`, то есть медиану живой выборки, а не случайный единичный замер;
- ниже идёт одна общая comparison table без дублирования по карточкам:
  - колонки: `P50 / P95 / P99 / Max / Выборка`;
  - строки: `Повторный запрос — эталон`, `Повторный запрос — сейчас`, `Первый запрос — эталон`, `Первый запрос — сейчас`;
- строка `Эталон` у этого live блока теперь должна быть заполнена полностью:
  - это фиксированные цифры из machine-readable observability profile;
  - они не зависят от текущей сессии;
  - в UI показываются с буквальным оператором `<=` или `>=`, а не просто голым числом;
  - для повторного запроса сейчас целевой набор такой:
    - `P50 < 1 ms`
    - `P95 < 1 ms`
    - `P99 < 2 ms`
    - `Max < 5 ms`
    - `Выборка > 200`;
  - для режима без прогрева сейчас целевой набор такой:
    - `P50 < 2 ms`
    - `P95 < 4 ms`
    - `P99 < 6 ms`
    - `Max < 10 ms`
    - `Выборка > 100`;
- `mix` как отдельный live слой из human dashboard убран, чтобы не смешивать общую картину с прямым сравнением `hot` и `cold`;
- machine-cards в human dashboard теперь несут не только статический host baseline, но и живую hardware telemetry:
  - `CPU`
    - общая загрузка;
    - температура `Tctl`;
    - максимум частоты;
  - `Оперативная память`
    - автоопределённый тип;
    - автоопределённая скорость;
    - занято/свободно;
    - usage и swap;
  - `Основной диск`
    - автоматически найденный device для текущего `repo_root`;
    - тип (`NVMe SSD` / `SSD` / `HDD`);
    - usage;
    - текущая `I/O`-нагрузка;
    - скорость чтения/записи между refresh-ами;
    - температура и firmware;
  - `Графика и ускорители`
    - accelerator-first слой вместо одной жёсткой `GPU`-строки;
    - основной карточкой показывается устройство с самым богатым live-profile;
    - inventory-path обязан честно поддерживать:
      - `iGPU`;
      - дискретные `GPU`;
      - внешние `eGPU`;
      - другие ускорители, если ОС или driver/tooling реально их раскрывают;
    - если live telemetry для найденного устройства недоступна, карточка не врёт и оставляет поле пустым;
    - если ускорителей несколько, дополнительные устройства выводятся отдельно;
    - если ускорителя нет, карточка остаётся на месте и показывает `не обнаружено`;
- `Установленный клиент` и `Сборка` теперь intentionally живут как компактные machine-cards, чтобы не выталкивать аппаратную карточку ускорителей из первого ряда;
- у карточки `Qdrant внешнего бенча` после остановки теста строки `resident memory / points / segments` больше не могут называться `Сейчас ...`:
  - при неактивном benchmark-run они обязаны быть подписаны как `Последний срез ...`;
  - это означает последний измеренный или последний сохранённый срез, а не живой текущий state;
- визуально большие section/panel блоки human dashboard теперь держат более выраженную внешнюю тень, а внутренние карточки получают мягкую inset-тень:
  - край читается жёстче;
  - внутрь тень уходит мягко, как градиент;
  - это нужно не для украшательства, а чтобы человек быстрее различал уровень контейнера и уровень карточек внутри него;
- status этого общего live блока остаётся привязан к честному live `P95` той же выборки, а не к чужому benchmark snapshot;
- блок непотоковых измерений теперь человекочитаемо называется `Последние честные проверки`;
- benchmark-карты внутри него отдельно показывают:
  - явный источник `benchmark`, а не live;
  - отдельную comparison table `Метрика / Эталон / Тестовые данные`;
  - покрытие выборки;
  - `repo_count`;
  - `query_slice_count`;
- для карточки `Быстрый путь под нагрузкой` теперь обязательно показываются:
  - `QPS`;
  - `P50`;
  - `P95`;
  - `P99`;
  - `Max`;
  - `error rate`;
  - `workers`;
  - `sample_count`;
- для неё сейчас действует именно такой benchmark-эталон:
  - `QPS > 35000`
  - `P50 < 0.012 ms`
  - `P95 < 0.015 ms`
  - `P99 < 0.020 ms`
  - `Max < 0.5 ms`
  - `error rate < 0.00%`
  - `workers > 16`
  - `sample_count > 10000`
- спецтермины и англицизмы на human dashboard теперь имеют русскую подсказку при наведении курсора на сам термин, без отдельного значка `?`.
- runtime scrape targets и monitoring ports не должны быть вшиты в конфиг как абсолютные литералы;
- поэтому monitoring profile рендерится из `.env` перед `docker compose --profile monitoring up`.
- token-economy metrics тоже приходят в exporter:
  - из последнего `token_benchmark` snapshot:
  - `amai_tokens_naive_scope_total`
  - `amai_tokens_context_pack_total`
  - `amai_tokens_saved_total`
  - `amai_tokens_savings_factor`
- live retrieval exporter теперь отдельно отдаёт по каждому состоянию `mixed/hot/cold`:
  - `current_ms`
  - `max_ms`
  - `p50_ms`
  - `p95_ms`
  - `p99_ms`
  - `sample_count`
  - `p50_ms`
  - `p95_ms`
  - `p99_ms`
  - `max_ms`
  - `sample_count`
  чтобы live dashboard и monitoring не подменяли текущую картину старым benchmark-only `p95`.
  - `amai_tokens_savings_percent`
  - и как накопительный ledger по умолчанию уже в verified/live-only semantics:
  - `amai_tokens_saved_session_total`
  - `amai_tokens_saved_window_total`
  - `amai_tokens_saved_lifetime_total`
  - `amai_tokens_savings_percent_session`
  - `amai_tokens_savings_percent_window`
  - `amai_tokens_savings_percent_lifetime`
  - дополнительные quality rollups:
  - `amai_tokens_quality_ok_rate_session`
  - `amai_tokens_quality_ok_rate_window`
  - `amai_tokens_quality_ok_rate_lifetime`
  - `amai_tokens_fallback_rate_session`
  - `amai_tokens_fallback_rate_window`
  - `amai_tokens_fallback_rate_lifetime`
  - `amai_tokens_answer_like_rate_session`
  - `amai_tokens_answer_like_rate_window`
  - `amai_tokens_answer_like_rate_lifetime`
  - raw savings теперь остаются отдельно, чтобы не подменять headline:
  - `amai_tokens_raw_saved_session_total`
  - `amai_tokens_raw_saved_window_total`
  - `amai_tokens_raw_saved_lifetime_total`
  - `amai_tokens_raw_savings_percent_session`
  - `amai_tokens_raw_savings_percent_window`
  - `amai_tokens_raw_savings_percent_lifetime`

## Hardware baseline

Текущий репозиторный latency/load baseline materialized на таком host:
- CPU: `AMD Ryzen 9 7900X`
- `12c / 24t`
- RAM: `62 GiB`
- storage: `NVMe HS-SSD-G4000 2048G`
- architecture: `x86_64`

Повторная проверка другими инженерами должна делаться:
- на железе не хуже;
- теми же proof-командами;
- с тем же разделением `hot` и `cold` contours.
