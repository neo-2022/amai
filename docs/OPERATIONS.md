modified_at: 2026-03-27 17:40 MSK
Ручная сверка guide/docs: 2026-03-27 17:40 MSK

# Operations

Каноническое имя проекта:

## Growth guardrails

Operations contour обязан удерживать не одну локальную метрику, а весь набор project laws:
- скорость не должна тихо деградировать;
- точность не должна уступать удобству;
- правдивость claim-ов не должна расширяться быстрее proof/evidence;
- безопасность и isolation не должны уступать ни UX, ни скорости.

Для этого любой новый operational contour обязан сразу materialize-ить:
- `performance budget` и способ его recheck;
- `proof/gate` или честный evidence gap;
- `incident bundle` для расследования;
- `reconcile path`, если автоматический путь может fail-closed остановиться;
- `degradation matrix`, если contour может частично ломаться.

Если новый слой не даёт этих вещей, он считается operationally незавершённым даже при зелёных
локальных тестах.
- `Art-memory-agent-index`
- short name: `Amai`
- текущий path: `/home/art/agent-memory-index`

## Operational tokenonomics hygiene

Карточки `Экономия токенов за текущую сессию` и `Экономия токенов за рабочее окно` используют
только operational `live`-lane. Поэтому engineering-runtime не имеет права тихо писать туда свои
proof/verify события.

Практическое правило:
- если инженерный сценарий идёт через `verify *` CLI, default `live_context_pack` должен быть
  автоматически заменён на non-live engineering source kind;
- если инженерный сценарий идёт через MCP proof-session, `amai_context_pack` и
  `amai_continuity_startup` тоже должны автоматически получать proof token source kind, если caller
  его явно не передал;
- если инженерный сценарий идёт через `verify memory-matrix`, archival retrieval внутри matrix тоже
  обязан явно передавать `verify_memory_matrix_context_pack`, а не наследовать CLI default
  `live_context_pack`;
- `current_session` теперь должен резаться сначала по latest `session_id`, а не только по тайм-gap;
  time-gap остаётся только как legacy fallback для старых событий без session grouping;
- `live_continuity_startup` теперь считается явной boundary для нового logical session, чтобы новый
  chat/window мог стартовать clean-session even if old live traffic был совсем недавно;
- для runtime-доказательства этого контура есть отдельный сценарий:
  `./scripts/proof_token_session_boundary.sh`;
- если destructive rewrite не делался, но для того же
  `project + namespace + measurement_scope + correlation_id` уже появился более новый
  `proof_* / verify_* / benchmark_*` snapshot, report-layer теперь обязан fail-closed выбросить
  старый `live_context_pack` из live cards и same-meter views;
- если contamination уже попал в текущую live session до этого guardrail, repair выполняется только
  честным reverify/new window path, а не тихим задним переписыванием истории.

## Operational continuity schema sync

`continuity startup`, `continuity restore`, `continuity answer` и `continuity handoff` теперь
обязаны сами делать `bootstrap_schema` сразу после admin-connect.

Это operationally важно по двум причинам:
- новый `ExecCtl` lane уже использует durable SQL storage
  (`ami.execctl_task_ledger_entries`, `ami.execctl_task_leases`);
- partial-upgrade не имеет права ломать новый chat-start только потому, что конкретная БД ещё не
  видела последнюю таблицу или индекс.

Практическое следствие:
- если runtime уже обновлён, а schema ещё старая, первый `continuity` front door сам доводит schema
  до совместимого состояния;
- startup не должен падать на `relation ami.execctl_task_leases does not exist`;
- product proof для этого контура теперь идёт не только через обычный `proof_execctl_pending_return`,
  но и через вариант, где `ami.execctl_task_leases` специально удаляется перед новым handoff.

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

Важно для Windows:
- `install_amai.ps1` и `install_amai.cmd` больше не должны делать вид, что умеют честно поднимать локальный Windows stack;
- без `--ssh-destination` этот path должен fail-closed с прямым сообщением про `WSL2`;
- безопасный Windows вариант сейчас такой:
  - локальный stack в `WSL2`;
  - или remote-host onboarding через `--ssh-destination`.

`install_amai.sh` делает ещё один шаг поверх этого:
- по умолчанию использует `client = auto`;
- пытается определить, какой клиент наиболее вероятен;
- работает как более человеческое имя для product install path.
- если запускать его повторно, он не должен плодить дубликаты, а должен аккуратно пересинхронизировать текущую установку.
- после локальной установки `~/.local/bin/memory` больше не должен указывать на старый bridge; он должен запускать `Amai` compatibility runner.
- этот bridge больше не должен уходить в `cargo run` во время обычного runtime;
  install path теперь обязан опираться на уже собранный release binary.
- `memory search` через этот bridge теперь обязан печатать не только hits, но и две explainability-строки:
  - `Почему вошло`
  - `Почему часть не вошла`
- для этого пути есть отдельный proof:

```bash
./scripts/proof_memory_bridge_search.sh
```

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

Правило для `AMI_POSTGRES_DSN`:
- пароль из DSN не должен появляться в runtime error messages;
- безопасный descriptor может показывать только user/host/port/dbname/`sslmode`;
- transport выбирается по `sslmode`, а не через жёстко вшитый `NoTls`.

Практический смысл:
- `sslmode=disable` остаётся plain local/dev path;
- остальные режимы должны поднимать `PostgreSQL` через native TLS connector;
- то же правило действует и для `memory` compatibility binary, потому что он тоже ходит в `PostgreSQL` для project resolution.

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
cargo run --quiet -- verify continuity --project project_alpha --namespace continuity
```

Что это materialize-ит:
- текущий continuity namespace перед импортом очищается от прошлых continuity docs, чтобы старые snapshot-path не оставались призраками в retrieval;
- полный raw continuity-content сохраняется в artifact storage;
- searchable continuity-layer режется до безопасного размера для `PostgreSQL tsvector` и lexical chunks;
- observability получает отдельный snapshot `continuity_import`;
- startup-summary потом читается не из нескольких разрозненных источников, а из `Amai`.
- freshness для `continuity_import` и `continuity_handoff` теперь определяется по semantic времени самого артефакта
  (`imported_at_epoch_ms` / `captured_at_epoch_ms`), а не по простому `created_at` строки в БД;
  это защищает startup/restore от позднего replay старого import или handoff.
- поверх этого теперь есть ещё и отдельный proof path `verify continuity`:
  - он не просто печатает startup/restore, а пишет benchmark-class snapshot `continuity_verification`;
  - внутри него лежит `canonical_eval` с тем же общим verdict vocabulary;
  - direct recovery contour считается полезным только если одновременно подтверждены свежий handoff, живой `working_state_restore`, непустой `chat_start_restore.prompt_text` и оба replay-guard probe;
  - direct temporal lookup теперь входит туда же:
    - `previous_chat_recovered_useful`
    - `exact_time_recovered_useful`
    - `missing_previous_chat_fail_closed`
    - `missing_exact_time_fail_closed`
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
./scripts/continuity_answer.sh --project project_alpha --namespace continuity --intent last_chat --json
./scripts/continuity_startup.sh --project project_alpha --namespace continuity --json
```

Он:
- сам использует тот же continuity слой;
- отдаёт сразу готовый короткий ответ;
- не требует отдельной цепочки `startup -> restore/context`;
- не должен порождать новый handoff.
- если включить `--json`, тот же product path вернёт `canonical_eval`, `retrieval_science` и answer-level verdict-класс без write-side snapshot.

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
./scripts/chat_lookup.sh --project project_alpha --namespace continuity --question "на чем закончили в прошлом чате, какие последние два сообщения?" --json
```

Это важный инвариант: `current / previous / exact time` идут одним temporal path, а не
разными эвристическими обходами.

Для Art есть и отдельный proof этого product-контура:

```bash
./scripts/proof_art_continuity_answer.sh
./scripts/proof_art_continuity_restore.sh
./scripts/proof_art_continuity_startup.sh
```

Он подтверждает:
- `last_chat --json -> recovered_useful`
- `previous_chat --json -> recovered_useful`
- `exact-time miss --json -> hit_correct_target`
- question-driven `chat_lookup.sh --json` не теряет тот же verdict-layer
- `continuity restore -> 2 x recovered_useful` по `chat_start_restore` и `working_state_restore`
- `continuity startup --json -> 3 x recovered_useful` по `startup_summary`, `chat_start_restore` и `working_state_restore`

`continuity restore` теперь возвращает не только `chat_start_restore` и `working_state_restore`,
но и верхний `continuity_restore.canonical_eval`, плюс `retrieval_science` и
`degradation_policy`. Это нужно затем, чтобы machine-readable recovery можно было проверять
не только по сырым полям, но и по каноническим verdict-классам.

Теперь этот же recovery-path дополнительно поднимает короткие explainability-summary:
- `chat_start_restore.included_reasons_summary`
- `chat_start_restore.excluded_reasons_summary`

А `continuity answer --json` даёт те же summary рядом с готовым ответом, чтобы было видно
не только `что восстановили`, но и `почему этот контекст вошёл` и
`почему часть retrieval-слоёв ничего не добавила`.

`continuity startup --json` теперь даёт тот же класс проверяемого machine-readable слоя:
верхний `continuity_startup.canonical_eval`, рядом `chat_start_restore`,
`working_state_restore`, `retrieval_science` и `degradation_policy`.

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

И этот `working_state` теперь обязан печатать не сырой список strategy-key, а две
человеческие explainability-строки:
- `Почему вошло`
- `Почему часть не вошла`

Второй helper печатает raw JSON и теперь возвращает сразу два узла:
- `chat_start_restore`
- `working_state_restore`

Практический смысл:
- `chat_start_restore` нужен как короткий первичный injection pack;
- `working_state_restore` нужен как более широкий raw слой для аудита и машинного восстановления.
- `working_state_restore` теперь execution-aware:
  - `next_step_state` всегда показывает, что следующий шаг пока только `planned`;
  - `recent_actions[].execution_state` разводит `attempted / succeeded / superseded / stale`;
  - `pending_return_queue`, `pending_return_summary` и `execctl_resume_state` не дают новому
    `continuity_handoff` тихо затереть прошлую рабочую линию;
  - `execctl_resume_contract` и `execctl_resume_contract_summary` теперь machine-readable поднимают
    `required_return_task`, чтобы новый чат или MCP-клиент видел obligation к возврату явно;
  - `project_task_tree` и `project_task_tree_summary` теперь поднимают active line и
    pending-return obligations как project-bound open-task tree;
  - `project_task_ledger` теперь truthful-ly prefers durable SQL lane
    `ami.execctl_task_ledger_entries`, а restore-side ledger остаётся fallback/shadow path;
  - `state_lineage` хранит `lineage_model_version = lineage-v2`, authoritative event, supporting event ids, truth ranking и явный graph-слой `nodes / edges`;
  - `workspace_graph` хранит versioned structural runtime/workspace graph:
    `context_pack -> file / structure_item / symbol / chunk / import_ref / export_ref / call_ref`,
    а в `workspace-graph-v10` ещё и resolved relations `imports_file / re_exports_file / imports_symbol / re_exports_symbol / resolves_file / resolves_symbol / calls_file / calls_symbol / resolves_call_file / resolves_call_symbol`, включая owner-aware Rust symbol lookup для provable path-cases вроде `Type::new`, `Self::helper()`, `self.helper()`, trait-qualified forms вида `<Type as Trait>::make`, trait-qualified forms через доказанный imported alias, module-alias forms вроде `trait_mod::Factory`, owner-side module alias paths вроде `type_mod::Beta::new` и combined forms вроде `<type_mod::Beta as trait_mod::Factory>::make` и `<type_mod::Beta as FactoryAlias>::make`; если impl-owner или impl-trait в symbol metadata видны только как alias, импортированный terminal selector или module alias, graph пишет дополнительные поля `owner_path_canonical` и `trait_name_canonical` только при единственном доказуемом import-target;
  - для single exact-document pack и single symbol-only pack provenance теперь может честно materialize-ить минимальный graph без полного file-обхода; это режет cold latency, но не теряет source-of-truth по самому retrieved file/symbol;
  - `workspace_graph_summary` в `chat_start_restore` и startup-output нужен как короткий human-readable слой поверх этого graph;
  - тот же `chat_start_restore` теперь обязан печатать `Незавершённые линии к возврату` и
    `Контракт возврата ExecCtl`, если в проекте уже есть suspended workline, которую потом надо вернуть;
  - при битом `session_id` restore-path теперь fail-closed и не смешивает несколько пустых сессий в один bundle.
  - product proof для этого minimal `ExecCtl` contour:
    `./scripts/proof_execctl_pending_return.sh`
    и тот же proof теперь проверяет уже не только queue и `project_task_tree`, но и durable
    `project_task_ledger` из SQL.

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
  - отдельный materialized validation contour для честной Windows-проверки через VM;
  - его живой execute-runner сейчас: `./scripts/proof_windows_vm_lab.sh --iso-path /path/to/windows.iso`;
  - текущий доказанный результат там не “локальный Windows install path готов”, а честный fail-closed proof для `install_amai.ps1` без `--ssh-destination`.

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

Правило для этого шага:
- `repo_root` будет сохранён только в canonical absolute form;
- тот же canonical physical root потом должен использоваться и в `index project`, чтобы `relative_path`
  не превращался в почти абсолютный path с `..`;
- если тот же физический корень уже зарегистрирован под другим `project code`, команда обязана завершиться ошибкой;
- использовать alias-path вроде `../Art` вместо уже зарегистрированного `/home/art/Art` запрещено: это не новый проект, а конфликт регистрации.
- повторный `project register` с тем же `project code`, но с новым physical root, считается relocation:
  - `ami.projects.repo_root` переключается на новый root;
  - старый root сохраняется в registry как `relocated_from`;
  - path-based resolve в bridge/startup продолжает узнавать тот же проект и на старом, и на новом root.
- reuse старого relocation-root другим проектом должен fail-closed блокироваться, пока оператор явно не
  пересмотрит binding; молчаливый “path steal” здесь запрещён.
- для filename-запроса без расширения exact document lookup теперь может честно вернуть тот же basename
  с реальным расширением, но только внутри того же `project + namespace` scope.

Проверочный contour для relocation:

```bash
./scripts/proof_project_relocation_contour.sh
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
- внутри payload теперь есть и `decision_trace`:
  machine-readable объяснение, почему именно `exact / symbol / lexical / semantic`
  слои дали вклад или не дали ничего;
- внутри payload теперь отдельно materialize-ится `workspace_graph`:
  это компактный structural graph по уже найденным scoped-артефактам, а не догадка “по соседним файлам”.
- этот graph собирается только из того, что уже лежит в scope:
  `file`, `structure_item`, `symbol`, `chunk`, `import_ref`, `export_ref`, `call_ref`;
- в `workspace-graph-v10` он теперь дополнительно materialize-ит resolved file-to-file, file-to-symbol и conservative call lineage
  только там, где target реально существует в том же `project + namespace + scope`; owner-aware Rust lookup разрешён только для provable случаев вроде `crate::alpha::Beta::new`, `Beta::new` после уже доказанного import-target, `Self::helper()` и `self.helper()` при доказанном локальном `impl`-owner, trait-qualified forms вроде `<Beta as Factory>::make`, а также тех же trait-qualified forms через `use ... as Alias`, module-alias selectors вроде `trait_mod::Factory`, owner-side module alias paths вроде `type_mod::Beta::new` и combined alias forms вроде `<type_mod::Beta as trait_mod::Factory>::make` и `<type_mod::Beta as FactoryAlias>::make`, но только когда visible selector map сам остаётся неамбигуозным; для тех же strict contours metadata теперь может получить `owner_path_canonical` и `trait_name_canonical`, но только если видимый selector однозначно маппится в один import-target или в один module-prefix target; неоднозначный target остаётся неразрешённым, недоказуемый call остаётся просто `call_ref` без target-edge;
- property-based tests отдельно удерживают этот контур fail-closed:
  неоднозначный symbol-owner match в любом candidate-file или более одного уникального candidate-file обязаны давать `None`, а не “лучшее совпадение”.
- тот же graph потом без дополнительного reparse попадает в `working_state_restore`.

Важно:
- `namespace` участвует в retrieval буквально;
- если вы запросили `default`, `Amai` не должен молча тянуть `smoke` или другой namespace того же проекта;
- если related project не имеет такого же namespace code, он просто не попадает в scope этого `context pack`.

Отдельный proof для explainability этого слоя:

```bash
./scripts/proof_context_decision_trace.sh
```

Следующий связанный proof уже проверяет, что этот explainability-layer не обрывается
на raw payload и доходит до restore-path:

```bash
./scripts/proof_working_state_decision_trace.sh
```

После живого `context pack` он смотрит `observe snapshot` и требует, чтобы
`latest_working_state_restore.working_state_restore` уже содержал
`latest_decision_trace` и `recent_decision_traces`.

Следующий user-facing слой поверх этого теперь тоже закрыт proof-контуром:
- `scripts/proof_art_continuity_startup.sh`
- `scripts/proof_art_continuity_restore.sh`
- `scripts/proof_art_continuity_answer.sh`

Они уже требуют, чтобы explainability дошла до:
- `chat_start_restore.included_reasons_summary`
- `chat_start_restore.excluded_reasons_summary`
- `continuity_answer.included_reasons_summary`
- `continuity_answer.excluded_reasons_summary`
- и до human-readable строк `Почему вошёл...` / `Почему часть не вошла...`

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
- `amai_observe_snapshot` теперь отдаёт наружу не только SLA summary, но и
  `observe_snapshot_summary.included_reasons_summary / excluded_reasons_summary`,
  чтобы клиентский слой видел причины включения и невключения последнего контекста.
- тот же `observe snapshot` теперь несёт ещё и `compatibility` с
  `profile`, `schema_version`, `compatible` и per-service reasons; short summary
  у `amai_observe_snapshot` показывает это как `compatibility=<profile>:ok|drift`.
- `amai_token_report` теперь тоже отдаёт наружу `token_report_summary`, где уже
  собраны не только `scope_label`, `status`, `counted_events / events_count` и `note`,
  но и machine-readable summary для `agent_cycle` lower bound:
  - `agent_cycle_scope_label`
  - `agent_cycle_status`
  - `agent_cycle_verified_saved_percent`
  - `agent_cycle_verified_saved_tokens`
  - `agent_cycle_note`
  - теперь ещё и compact contractual summary:
    - `contractual_scope_label`
    - `contractual_state`
    - `contractual_coverage_state`
    - `contractual_reconciliation_state`
    - `contractual_margin_state`
    - `contractual_blockers_summary`
    - `contractual_statement_summary`
- `amai_context_pack` теперь тоже отдаёт наружу `context_pack_summary`, где уже
  лежат `included_reasons_summary / excluded_reasons_summary` для последнего
  собранного context pack.
- `amai_token_benchmark` теперь тоже отдаёт наружу `token_benchmark_summary`, где
  уже собраны `saved_tokens`, `savings_factor`, `savings_percent`,
  `naive_tokens`, `context_tokens` и `files_considered`.
- `amai_list_projects` и `amai_list_namespaces` теперь тоже дают compact discovery
  preview в short summary, а не только count.
- `amai_stack_preflight` теперь даёт наружу `preflight_summary` и `preflight_report`,
  чтобы внешний клиент видел честный verdict по deployment profile и machine guarantees,
  а не только локальный human-print из CLI.
- `amai_benchmark_coverage` теперь даёт наружу `benchmark_coverage` и
  `benchmark_coverage_summary`, чтобы внешний клиент видел machine-readable
  карту benchmark/eval coverage без парсинга human CLI-вывода.
- `amai_warm_cache` теперь тоже отдаёт наружу `warm_cache_summary`, где уже
  собраны `compact_projects`, `cache_hits`, `exact_documents`, `symbol_hits`,
  `lexical_chunks` и `semantic_chunks`, а не только итоговый count warmed projects.
- `amai_memory_matrix` теперь даёт наружу `memory_task_matrix` и
  `memory_matrix_summary`, чтобы внешний клиент видел measured product-eval
  по памяти и изоляции без ручного разбора полного verify payload.
- `initialize` теперь тоже отдаёт `amai_protocol_manifest`:
  versioned contract layer с `default_scope_rule`, `default_retrieval_mode`,
  `startup_contracts`, `tool_contracts`, `prompt_contracts` и `safety_laws`.
- `startup_contracts.project_chat_startup` теперь отдельно фиксирует
  canonical resume path для нового или resumed чата:
  `amai_continuity_startup` обязателен до retrieval и любой новой работы,
  а client runtime должен поднимать не только headline, но и
  `execctl_resume_state` вместе с pending-return obligations.
- `execctl_active_lease` теперь тоже входит в required startup summary fields;
  client runtime не имеет права считать lease owner необязательной косметикой.
- тот же contract теперь несёт `resume_enforcement`, чтобы runtime не угадывал,
  как трактовать `execctl_resume_contract_summary`:
  если summary не `clear`, это `required_return_task`, а `no_silent_drop = true`
  запрещает тихо уйти в unrelated work.
- рядом с human summary теперь materialized и machine-readable
  `execctl_resume_obligation`, чтобы client runtime видел
  `resume_state / pending_return_count / required_return_*` без строкового парсинга.
- `required_return_task` теперь surfaced и отдельным object-слоем:
  client runtime больше не должен собирать return target из двух строковых полей.
- в тот же `resume_enforcement` теперь отдельно surfaced ещё и active-lease contour:
  - `active_lease_field = execctl_active_lease`
  - `active_lease_owner_state_field = lease_owner_state`
  - `previous_session_owner_value = previous_session_owner`
  - `previous_session_owner_must_follow_startup_next_action = true`
- это нужно затем, чтобы client runtime не пытался сам выводить ownership active line из human
  summary: если lease owner пришёл как `previous_session_owner`, client обязан follow
  `startup_next_action` first и не имеет права silently seize the workline.
- тот же startup summary теперь обязан нести и два project-bound `ExecCtl` слоя:
  - `project_task_tree`
  - `project_task_tree_summary`
  - `project_task_ledger`
  - `project_task_ledger_summary`
  чтобы клиент видел не только open-task tree, но и append-only handoff ledger.
- onboarding теперь materialize-ит и отдельный workspace-level JSON artifact
  `.amai/onboarding/project-chat-startup-contract.json`;
  это нужно затем, чтобы supported clients имели machine-readable startup source-of-truth и не
  зависели только от парсинга managed markdown/rule block.
- тот же artifact теперь несёт `startup_contract_sha256`, а managed instructions поднимают тот же
  expected hash; при drift client/runtime должен fail-closed, а не quietly continue на старом
  startup contract.
- рядом с этим теперь materialized и отдельный `artifact_enforcement` contour:
  - `workspace_contract_required_before_tool_call = true`
  - `workspace_contract_relative_path = .amai/onboarding/project-chat-startup-contract.json`
  - `missing_or_unreadable_fail_closed = true`
  - `sha256_mismatch_fail_closed = true`
- operationally это значит буквально:
  supported client не имеет права делать `amai_continuity_startup`, если workspace contract artifact
  пропал, не читается или не совпадает по pinned hash; correct behavior здесь fail-closed stop, а
  не fallback на старый markdown/rule block.
- этот же contour теперь обязан быть виден и в обычном `amai status`:
  строка `startup_artifacts: ...` показывает, есть ли managed startup instruction, совпадает ли
  текущий workspace contract с pinned hash из install state и не потерялись ли literal
  fail-closed flags в artifact/instruction layer.
- тот же status теперь auditing-ит и return-enforcement literals внутри managed startup block:
  `startup_next_action`, `required_return_task`, `resume_required_return_task`,
  `previous_session_owner_must_follow_startup_next_action = true`, `no_silent_drop = true`.
- и тот же status теперь auditing-ит contract-side `resume_enforcement` и required summary fields:
  `startup_next_action`, `required_return_task`,
  `required_action_kind_when_resume_required = resume_required_return_task`,
  `previous_session_owner_must_follow_startup_next_action = true`,
  `no_silent_drop = true`.
- начиная с текущего contract slice status обязан ловить и drift в field-level gate semantics:
  managed startup instruction и JSON contract должны literally удерживать:
  - `startup_execution_gate.must_follow_startup_next_action = true`;
  - `startup_execution_gate.unrelated_work_allowed = false`;
  - `startup_execution_gate.must_read_prompt_text_before_reply = true`;
  - `startup_execution_gate.required_action_kind_when_resume_required = "resume_required_return_task"`;
  - `startup_execution_gate.no_silent_drop = true`.
- truthful интерпретация status такая:
  - `ok` — managed startup artifact на месте и contract drift не обнаружен;
  - `missing_startup_instruction` — onboarding когда-то materialized startup artifact, но сейчас он
    снят или пропал;
  - `startup_instruction_drift` или `startup_contract_drift` — artifact жив, но уже не совпадает с
    текущим contract/enforcement baseline.
- рядом тот же status теперь обязан печатать и repair path:
  - для известного client binding:
    `startup_artifacts_repair: rerun ./scripts/onboard_local.sh --client ... --yes`
  - без install state:
    `startup_artifacts_repair: run ./scripts/onboard_local.sh --client <client> --yes ...`
- отдельно `amai continuity startup` теперь materialize-ит и dynamic runtime artifact
  `.amai/continuity/project-chat-startup-state.json`;
  operationally это уже не onboarding contract, а последний реально поднятый startup-state для
  текущего workspace.
- тот же static startup contract теперь обязан не терять этот path:
  `runtime_state_artifact.workspace_runtime_state_relative_path =
  .amai/continuity/project-chat-startup-state.json`;
  иначе managed startup instructions считаются drifted.
- тот же contract теперь pin-ит и
  `runtime_state_artifact.workspace_runtime_state_artifact_version =
  workspace-startup-runtime-state-v3`;
  supported client обязан проверять и этот literal перед доверием к runtime artifact.
- в этом runtime artifact должны лежать:
  - `continuity_startup_summary`;
  - `chat_start_restore.prompt_text`;
  - `gate_semantics_consistent`;
  - `startup_execution_gate`;
  - `startup_next_action`;
  - `required_return_task`;
  - `execctl_active_lease`;
  - `project_task_tree`;
  - `project_task_ledger`.
- `startup_execution_gate` считается immediate operator/client gate:
  он machine-readable фиксирует:
  - `must_follow_startup_next_action`;
  - `unrelated_work_allowed`;
  - `must_read_prompt_text_before_reply`;
  - `required_action_kind_when_resume_required`;
  - `no_silent_drop`.
- этот gate теперь обязателен не только в runtime artifact и fallback CLI, но и прямо в
  `continuity_startup_summary`.
- `amai status` теперь auditing-ит runtime artifact отдельной строкой `startup_runtime_state: ...`.
  Правильное чтение:
  - `ok` — живой startup-state materialized и return contour виден machine-readable;
  - `not_materialized` — клиент/оператор ещё не поднимал startup в этом workspace;
  - `startup_runtime_state_drift` — artifact есть, но он потерял expected contract hash,
    `prompt_text` или обязательные machine-readable return поля.
- в эти обязательные runtime gate fields теперь входят не только
  `must_follow_startup_next_action / unrelated_work_allowed`, но и:
  - `must_read_prompt_text_before_reply`;
  - `required_action_kind_when_resume_required`;
  - `no_silent_drop`.
- status/runtime audit теперь ещё отдельно публикует `gate_semantics_consistent`;
  он обязан падать в `false`, если live gate противоречит:
  - pinned contract semantics;
  - `startup_next_action`;
  - `required_return_task`;
  - `previous_session_owner` lease contour.
- supported client теперь обязан требовать и top-level literal
  `gate_semantics_consistent = true` внутри runtime artifact;
  отсутствие этого поля или значение `false` считается fail-closed drift, даже если сам
  `startup_execution_gate` формально присутствует.
- repair path для такого drift теперь честный и bounded:
  `startup_runtime_state_repair: rerun cargo run -- continuity startup --repo-root ... --namespace continuity --json >/dev/null`
- если нужно audit-ить runtime artifact не в самом `Amai` repo-root, а в конкретном project
  workspace, теперь есть отдельный read-only path:
  `cargo run -- continuity startup-state --repo-root /path/to/project --json`
- этот CLI path теперь считается pinned fallback для клиентов, которым неудобно читать runtime
  artifact напрямую: он обязан вернуть тот же `startup_execution_gate`.
- тот же `amai_protocol_manifest` теперь несёт `error_contracts`, а `tools/call`
  и JSON-RPC errors отдают machine-readable taxonomy вместо голого текста:
  `invalid_json_rpc_payload`, `invalid_request`, `method_not_found`,
  `prompt_not_found`, `invalid_params`, `tool_not_found`, `tool_execution_failed`.
- в `error_contracts` теперь отдельно указан `carrier`, чтобы клиент заранее знал,
  придёт ли ошибка через top-level JSON-RPC error, через `tool_is_error`,
  или может встретиться в обоих каналах.

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

Операционно это нужно читать так:
- `mcp config` / onboarding — это разовая настройка клиента;
- это не команда, которую пользователь должен повторять перед каждым новым чатом;
- после такой настройки новый чат может честно рассчитывать на auto-start только если
  для этого клиента materialized startup artifact или у проекта уже есть свой rule file;
- для MCP-клиентов этот startup path теперь должен идти через tool `amai_continuity_startup`,
  а не через свободную prompt-реконструкцию;
- просто открыть папку проекта недостаточно, если сам клиент ещё не подключён к `Amai`;
- первое сообщение пользователя должно быть обычным рабочим сообщением, а не специальной
  фразой для восстановления continuity.
- если startup вернул `startup_next_action.action_kind = resume_required_return_task`,
  клиент обязан взять именно это действие как первый ход после restore, а не оставлять
  pending-return только в human summary.

Текущий truthful runtime contour по клиентам теперь такой:
- `VS Code`
  - onboarding пишет managed workspace instruction file
    `.github/instructions/amai-continuity-startup.instructions.md`;
  - это instruction-backed auto-start contour;
- `Cursor`
  - onboarding пишет managed project rule file
    `.cursor/rules/amai-continuity-startup.mdc`;
  - это instruction-backed auto-start contour;
- `Codex`
  - onboarding пишет MCP config;
  - startup теперь materialize-ится как bounded managed block в project `AGENTS.md`;
  - `Amai` не трогает остальной user content файла и удаляет при disconnect только свой block;
  - это instruction-backed auto-start contour;
- `Claude Code`
  - onboarding пишет workspace-local `.mcp.json`;
  - startup теперь materialize-ится как bounded managed block в project `CLAUDE.md`;
  - это instruction-backed auto-start contour;
- `Claude Desktop`, `Generic`
  - пока получают manual startup snippets и не должны считаться auto-start guaranteed.

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
- после успешного onboarding именно клиент обязан уметь автоматически поднимать continuity
  для уже зарегистрированного проекта в новом чате, без ручного restore-step со стороны пользователя.
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

`./scripts/proof_onboarding.sh` теперь дополнительно проверяет, что локальный
onboarding печатает explainability последнего собранного контекста, а не только
готовность stack/config.

`./scripts/proof_client_lifecycle.sh` теперь дополнительно проверяет, что все
generated startup artifacts для `Codex`, `Cursor` и `Claude Code` явно содержат
`execctl_resume_contract_summary`, `execctl_resume_obligation` и `required_return_task`, а не теряют
resume-obligation на client edge.

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
- для внешних git repo candidate files теперь materialize-ятся через sparse-checkout tracked worktree, чтобы pool не оставлял скрытые deleted-tracked files и не ломал следующий benchmark ложными quality misses;
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

Exact document lookup в этом контуре теперь идёт через индексируемые SQL поля:
- `relative_path`
- `relative_basename`
- `relative_basename_stem`

То есть cold exact-path contour больше не обязан regex-фильтровать весь namespace, если нужен один точный file/path hit.
Это же покрывает extensionless basename-файлы вроде `Makefile`: они теперь не проваливаются в broken fallback query и могут честно проходить как exact document target.

Этот runner считает:
- отдельно `cold` и `hot shadow`;
- `P50 / P95 / P99 / max / sample_count`;
- `precision / recall / target-hit rate / miss rate / fallback rate`;
- `cold_benchmark.canonical_eval` с probe-level verdict-классами по каждому cold case;
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

`samples.csv` теперь дополнительно содержит `eval_verdict_class` и `eval_reason`,
а proof `./scripts/proof_cold_benchmark.sh` проверяет, что `summary.json` несёт
не только aggregate numbers, но и machine-readable `cold_benchmark.canonical_eval`.

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
  - `canonical_eval`;
  - `retrieval_science`;
  - `degradation_policy`;
  - versioned suite metadata из `config/red_team_retrieval_isolation.toml`;
  - отдельные hostile visible/hit invariants по проекту и namespace.
- `canonical_eval` здесь уже не memory-only:
  - `strict_local_fail_closed`
  - `related_retrieval_target`
  - `symbol_target`
  - `namespace_boundary`
  - `hostile_fail_closed`
  все они используют тот же общий verdict vocabulary `hit_correct_target / hit_wrong_target / stale_target / under_retrieved / over_included / recovered_useful / not_useful`.

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

## Continuity correctness

Отдельный machine-readable слой continuity correctness теперь тоже собирается прямо в live system snapshot:
- raw JSON: `/api/snapshot -> continuity_correctness_model`;
- human-visible слой: service-card `Правильное продолжение`;
- MCP: `amai_observe_snapshot -> observe_snapshot_summary.continuity_*`;
- Prometheus:
  - `amai_continuity_verified_probes_total`
  - `amai_continuity_failed_probes_total`
  - `amai_continuity_recovered_useful_total`
  - `amai_continuity_fail_closed_total`

Этот слой показывает уже не косвенные признаки working-state, а именно последний explicit `verify continuity` proof:
- сколько continuity probes подтверждены;
- сколько из них полезно восстановили рабочую линию;
- сколько fail-closed проверок подтвердили, что Amai не подменяет отсутствующий прошлый чат или точное время ближайшим похожим результатом;
- сколько probes реально провалилось.

Важно:
- `verify continuity` теперь сохраняет observability snapshot и при PASS, и при реальном провале;
- поэтому human dashboard и observe snapshot видят честный последний verdict, а не только последний удачный run.

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

Теперь этот contour ещё и пишет общий verdict-layer:
- `mcp_task_matrix.retrieval_science`
- `mcp_task_matrix.canonical_eval.verdict_counts`
- `tasks[*].eval_verdict_class`
- `tasks[*].eval_reason`

Смысл этого слоя:
- обычные MCP happy-path задачи считаются как `hit_correct_target`;
- hostile fail-closed задачи считаются не как “просто ошибка”, а как корректно сохранённая граница, то есть тоже `hit_correct_target` для isolation-boundary pattern;
- `continuity_restore_success` считается как `recovered_useful`, потому что контур реально восстановил рабочее состояние, а не просто вернул произвольный успешный ответ.
- тот же recovery vocabulary теперь существует и вне MCP:
  - `verify continuity` пишет `continuity_verification.canonical_eval`;
  - на каноническом Art proof-контуре это сейчас `7 x recovered_useful` и `2 x hit_correct_target`, потому что recovery-path восстанавливает handoff, working state, prompt, direct temporal lookup и fail-closed защищён как от replay stale import/handoff, так и от fake previous/exact-time fallback.

На текущем каноническом proof-контуре это даёт:
- `live_mcpbench_local` -> `11 x hit_correct_target`
- `mcp_universe_local` -> `8 x hit_correct_target`, `1 x recovered_useful`

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
- `canonical_eval.verdict_counts`
- per-task `eval_verdict_class`
- per-task `eval_reason`

Канонический eval-слой теперь versioned и machine-readable:
- source of truth: `config/retrieval_science.toml`
- canonical verdict logic: `src/eval_verdict.rs`
- model version: `memory-eval-verdict-v1`
- verdict-классы:
  - `hit_correct_target`
  - `hit_wrong_target`
  - `stale_target`
  - `under_retrieved`
  - `over_included`
  - `recovered_useful`
  - `not_useful`
- direct continuity recovery теперь тоже использует этот vocabulary:
  - `handoff_summary_present`
  - `working_state_restore_present`
  - `chat_start_prompt_present`
  - `handoff_replay_rejected`
  - `import_replay_rejected`
  - `previous_chat_recovered_useful`
  - `exact_time_recovered_useful`
  - `missing_previous_chat_fail_closed`
  - `missing_exact_time_fail_closed`
  - хороший recovery path обязан давать `recovered_useful`, честный temporal fail-closed должен считаться как `hit_correct_target`, а replay-регресс обязан опускаться до `stale_target`, а не маскироваться под success.

Proof теперь проверяет не только happy-path, но и повторяемость:
- `./scripts/proof_memory_task_matrix.sh`
  - запускает `verify memory-matrix` два раза подряд;
  - подтверждает, что повторный прогон не ломает continuity-handoff и не даёт idempotency drift на `.amai-continuity/live-handoff/HANDOFF.md`.

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

Если нужен прямой `context pack` именно как engineering probe, а не как живой пользовательский
запрос, задавайте source kind явно:

```bash
cargo run -- context pack \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --token-source-kind proof_context_pack
```

Иначе оператор сам создаст `live_context_pack` событие и испортит текущую session-card. Это не
исправляется молча задним числом: pre-fix загрязнение старой сессии остаётся историческим фактом
до нового session window или отдельного repair/reverify контура.

Что показывает этот contour:
- `headline`
  - канонический product KPI:
    - `Verified Effective Savings %`
    - по-русски: `Проверенная реальная экономия`;
- `agent_cycle_economics`
  - честная нижняя граница полного агентного цикла;
  - measured part:
    - retrieval payload;
    - follow-up/recovery tokens, которые уже видно в ledger;
  - missing part:
    - токены исходного запроса клиента;
    - токены генерации итогового ответа;
    - tool-step вне retrieval;
    - continuity restore вне token-ledger retrieval-событий;
  - chart contract:
    - `all_live_timeline`
    - `verified_live_timeline`
    - cumulative lines `without_amai_measured_tokens / with_amai_measured_tokens / measured_saved_tokens`;
  - reporting layers:
    - `billable`
    - `measured_non_billable`
    - `unmeasured`
    - текущий runtime честно держит это в `report_only`, а не в money-facing режиме;
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
- `contract`
  - versioned contract на meter schema / baseline / quality / coverage /
    excluded taxonomy / billing policy;
- `usage_event_schema`
  - machine-readable usage contract:
    - required identity fields;
    - dedup key format;
    - canonical event-time field;
    - lifecycle status codes;
    - backfill/correction policy;
- `baseline_contract`
  - allowed/disallowed baseline classes;
  - fairness note, почему `entire_repo/all_docs` нельзя считать честным baseline;
- `billing_policy`
  - current mode;
  - current billable state;
  - required traffic class и quality gate;
  - preliminary thresholds;
  - explicit truth terms:
    - `savings floor`
    - `confirmed lower bound`
    - `retrieval savings floor`
    - `partial whole-agent-cycle lower bound`
- `suitability_contract`
  - versioned truth-layer, который фиксирует пригодность scope для:
    - `operational_live`
    - `product_kpi`
    - `customer_review`
    - `contractual_export`
    - `billing_amount`
    - `compensation_pricing`
  - negative result тоже может быть suitable для KPI, если рядом честно показаны
    coverage и completeness state.
- `rate_card`
  - `rate_card_binding_model_version`, rate-card version и currency profile;
  - truthful statuses:
    - `not_configured`
    - `read_error`
    - `parse_error`
    - `bound_but_unpriced`
    - `priced_bound`
  - `money_conversion_enabled = false`, пока binding не дошёл до `priced_bound`;
- `settlement_contract`
  - statement version;
  - freeze/close policy version;
  - late-arrival policy version;
  - correction/dispute policy versions;
  - explicit `current_operational_state / current_contractual_state`;
  - current truthful status: `report_only preview`;
- `metering_freshness_contract`
  - versioned thresholds для ingest warning/SLO и late-arrival grace;
  - именно он задаёт честную разницу между pipeline lag и открытым окном поздних событий;
- `telemetry_surfaces`
  - явный split между live engineering telemetry и contractual tokenonomics;
  - это защищает от подмены dashboard-графика customer billing-витриной;
- `reconciliation_contract`
  - versioned contract для будущей сверки internal lower bound с provider truth;
  - список обязательных и optional external sources;
  - теперь отдельно по truth-слоям:
    - `required_sources_for_usage_truth`
    - `required_sources_for_cost_truth`
    - `optional_sources_for_invoice_evidence`
    - `unready_*`, если source ещё не готов для этого слоя;
  - current truthful status: external truth ещё не bound в runtime;
- `reconciliation_previews`
  - `current_session / rolling_window / lifetime`;
  - internal measured lower bound по scope;
  - provider usage / provider cost / drift пока остаются `null`, если truth source ещё не подключён;
  - после честного bind runtime теперь может показать:
    - `internal_provider_cost_estimate_amount`
    - `drift_amount`
    - `invoice_drift_amount`
    - states `external_usage_aligned_report_only` / `external_usage_drift_report_only`;
- `margin_contract`
  - versioned truth-layer для product margin;
  - current truthful status зависит от реального bind `rate card + provider usage + infra cost profile`;
  - `rate_card_status` обязан повторять runtime status настоящего rate-card binding;
  - теперь ещё отдельно публикуются `required_sources_for_margin_truth` и
    `unready_required_sources_for_margin_truth`, чтобы margin-blocker был виден не только как
    общий state, но и как нехватка конкретных sources;
  - отдельно раскладываются:
    - `customer_savings_money_truth_completeness_state`
    - `amai_cost_truth_completeness_state`
    - `margin_truth_completeness_state`
    чтобы readiness margin не выглядела одной широкой меткой.
- `margin_view`
  - `current_session / rolling_window / lifetime`;
  - customer lower-bound savings в токенах;
  - money fields materialize-ятся только если честно привязаны
    `provider usage + rate card + infra cost profile`;
  - при provider drift state обязан стать `priced_preview_with_provider_drift`, а не
    притворяться нормой;
- `contractual_evidence_pack`
  - report-only export по `current_session / rolling_window / lifetime`;
  - содержит `settlement_report_preview`, `statement_preview`,
    `reconciliation_preview`, `margin_scope`, hashes включённых и исключённых line items;
  - raw `query` туда не попадает, только `query_hash` и usage-state поля;
  - это contractual evidence для review/export, но не invoice.
- `statement_export_previews`
  - `current_session / rolling_window / lifetime`;
  - compact export-layer поверх statement/reconciliation/margin/freshness;
  - публикует `statement_preview_id`, hashes line items, adjustment/dispute action state
    и готовую команду для on-demand evidence pack;
  - теперь ещё публикует `suitability`, чтобы contractual export и будущий billing
    не смешивались в одну шкалу пригодности;
  - это bridge между live report и full contractual evidence pack.
- `settlement_report_previews`
  - отдельный review-grade settlement object по каждому scope;
  - собирает period anchors, hashes, policy versions и truth states в один stable surface;
  - нужен затем, чтобы audit/export опирался на один объект, а не на смесь из preview,
    pack и источников.
- `adjustment_request_schema`
  - versioned request contract для future credit/correction/dispute entries;
  - запрещает тихую ретро-перезапись прошлого statement;
- `adjustment_registry`
  - optional report-only registry;
  - публикует source status, counts и per-scope hashes;
  - по умолчанию registry ищется в repo-local
    `/home/art/agent-memory-index/state/token_adjustment_registry.json`;
  - если env `AMAI_TOKEN_ADJUSTMENT_REGISTRY_PATH` не задан и repo-local файл ещё не
    materialized, честный статус остаётся `default_path_missing`;
  - operator-safe команды:
  - `./target/release/amai observe token-adjustment-registry --scope lifetime`
  - `./target/release/amai observe token-adjustment-add --scope lifetime --kind adjustment_entry --status pending_review --reason-code ...`
  - если нужно сразу связать correction entry с текущим `statement_preview_id`, теперь есть
    autofill-path:
    - `./target/release/amai observe token-adjustment-add --scope lifetime --status pending_review --reason-code ... --resolve-related-statement-id`
  - adjustments живут отдельными entries со статусами
    `pending_review / approved_but_unapplied / applied_report_only / disputed / rejected`
    и не дают quietly переписывать старый period.
- `coverage`
  - отдельный truth-layer поверх каждого rollup:
    - `measured_events`
    - `included_events`
    - `excluded_events`
    - `event_coverage_pct`
    - `baseline_token_coverage_pct`
    - `completeness_state`
- `excluded_breakdown`
  - taxonomy измеренных, но не включённых событий:
    - `quality_gate_failed`
    - `awaiting_followup_reconciliation`
    - `legacy_unverified`
    - `synthetic_verify`
    - `synthetic_proof`
    - `synthetic_benchmark`
    - `non_live_other`

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
- любая большая savings-цифра теперь должна читаться вместе с `coverage`, иначе
  её слишком легко принять за полный охват всей сессии.
- `agent_cycle_economics` остаётся `partial lower bound`, а не полной session economics.
- event-level `usage_state` теперь тоже каноничен:
  - `verified_included`
  - `excluded_quality_gate_failed`
  - `excluded_awaiting_followup_reconciliation`
  - `excluded_legacy_unverified`
  - `excluded_non_live`
  - `backfill_status` тоже виден отдельно и не маскируется под обычный live ingest.
- `baseline_strategy_slices` теперь тоже каноничны:
  - для каждого baseline класса видно `events_count`, `counted_events`,
    `verified_effective_savings_pct`, `quality_ok_rate`, `coverage`.
- `statement_previews` теперь тоже каноничны:
  - `current_session`
  - `rolling_window`
  - `lifetime`
  - в них видно measured non-billable lower bound по каждому scope;
  - в них отдельно видны `lifecycle_state`, `contractual_state`, `settlement_stage`,
    `next_settlement_stage_candidate`, `next_settlement_stage_blockers`, `close_barriers`;
  - в них теперь ещё есть `transactional_statuses`, которые по-честному разводят
    `measured / review / billable / settled / invoiced / credited / disputed / closed`;
  - statement export / evidence pack теперь ещё публикуют `export_semantics`, чтобы
    customer-facing self-serve surface не смешивался с operational telemetry;
  - в них теперь ещё есть `period`, `adjustment_preview`, `freshness`,
    `provisional_close_state`, `provisional_close_candidate`,
    `provisional_close_barriers`, `billing_close_barriers`;
  - billable amount и final amount остаются пустыми, пока settlement layer ещё не materialized.
  - top-level `settlement_contract` теперь отдельно фиксирует
    `current_materialized_boundary = measured_report_only`,
    `materialized_settlement_stages` и `future_reserved_settlement_stages`.
- `metering_freshness` теперь тоже канонична:
  - `current_session`
  - `rolling_window`
  - `lifetime`
  - отдельно видны:
    - `metering_ingest_state`
    - `contractual_lag_state`
    - `contractual_freshness_state`
    - `latest_event_age_ms`
    - `latest_ingest_lag_ms`
    - `p95_ingest_lag_ms`
  - это защищает от ложного вывода вида `счётчик старый => pipeline сломан`: теперь
    pipeline lag и late-arrival window видны раздельно.
- `adjustment_preview` теперь тоже каноничен:
  - видно `registry_status`, `pending_entries_count`, `applied_entries_count`,
    `disputed_entries_count`, `scope_hash`;
  - без materialized registry file этот слой не придумывает credits, а честно показывает
    `default_path_missing`.
- `reconciliation_previews` теперь тоже каноничны:
  - `current_session`
  - `rolling_window`
  - `lifetime`
  - repo-local sources теперь допускаются как operator-safe truth bindings:
    - `/home/art/agent-memory-index/state/provider_usage_export.json`
    - `/home/art/agent-memory-index/state/provider_invoice_export.json`
    - `/home/art/agent-memory-index/state/provider_rate_card.json`
    - `/home/art/agent-memory-index/state/infra_cost_profile.json`
  - если env-binding не задан, но repo-local файл уже materialized, runtime честно
    поднимает `default_existing_path`;
  - если файла ещё нет, источник остаётся `default_path_missing`;
  - governance-layer теперь отдельно показывает:
    - `usage_truth_completeness_state`
    - `provider_cost_truth_completeness_state`
    - `invoice_evidence_completeness_state`
    - `money_truth_completeness_state`
    - `reconciliation_readiness_state`
    - `governance_blocking_reasons`
  - `money_truth_completeness_state` остаётся верхним агрегатом,
    но runtime теперь сначала показывает, дошли ли мы до `provider cost truth`
    и отдельно дошли ли до `invoice evidence`;
  - в них теперь видно не только lower bound, но и внутренний delivered usage:
    - `internal_delivered_tokens`
    - `internal_recovery_tokens`
    - `internal_provider_billed_tokens`
  - drift по токенам считается только между `internal_provider_billed_tokens` и
    `external_provider_usage_tokens`;
  - отдельно теперь видны scope-level temporal states:
    - `provider_usage_scope_alignment_state`
    - `provider_invoice_scope_alignment_state`
    - `rate_card_scope_alignment_state`
    - `temporal_truth_state`
  - это нужно затем, чтобы provider usage и rate card не выглядели пригодными к period
    только из-за того, что источник вообще удалось привязать;
  - при реальном source binding там уже могут появляться:
    - `external_provider_usage_tokens`
    - `external_provider_cost_amount`
    - `external_invoice_amount`
    - `drift_tokens`
    - `internal_provider_cost_estimate_amount`
    - `drift_amount`
    - `invoice_drift_amount`
- `contractual_statement_summaries` теперь тоже каноничны:
  - `current_session`
  - `rolling_window`
  - `lifetime`
  - это короткий customer-facing summary поверх `statement + reconciliation + margin`;
  - теперь ещё и поверх `freshness`;
  - там отдельно видны `metering_ingest_state`, `contractual_lag_state` и
    `contractual_freshness_state`;
  - он пригоден для review/audit, но не для invoice.
- `margin_view` теперь тоже каноничен:
  - `current_session`
  - `rolling_window`
  - `lifetime`
  - в нём видно token-side lower bound savings клиента;
  - при честно привязанном priced rate card он обязан перескочить с общего
    `awaiting_rate_card` на следующий реальный блокер;
  - для этого теперь отдельно публикуются:
    - `rate_card_truth_completeness_state`
    - `infra_cost_truth_completeness_state`
    - `pricing_truth_completeness_state`
    - `margin_readiness_state`
  - temporal truth теперь тоже first-class:
    - `margin_confidence_state`
    - `rate_card_scope_alignment_state`
    - `infra_cost_scope_alignment_state`
    - `temporal_truth_state`
    - `provider_identity_state`
  - customer-facing summary/export теперь ещё обязан показывать:
    - `rate_card_status`
    - `rate_card_version`
    - `rate_card_provider`
    - `rate_card_currency_profile`
    - `provider_usage_provider`
    - `provider_invoice_provider`
    - `margin_blocking_reasons`
  - если честно привязаны `provider usage + rate card + infra cost profile`, он уже имеет право
    materialize-ить `customer_saved_amount_lower_bound`, `amai_infra_cost_amount`,
    `margin_amount` и `savings_to_cost_ratio`;
  - если priced inputs есть, но их период не покрывает statement scope, state обязан стать
    `pricing_period_mismatch`, а не притворяться aligned money preview;
  - если provider usage и priced rate card смотрят на разные provider identities, state обязан
    стать `provider_identity_mismatch`, а не продолжать money preview как будто источник истины един;
  - если provider usage показывает drift, этот drift должен оставаться видимым и в margin-preview.

По умолчанию verification-трафик не смешивается с обычной рабочей активностью.
Если нужно показать всё вместе:

```bash
cargo run --release -- observe token-report --include-verify-events true
```

Если нужен customer-facing evidence/export pack по одному scope:

```bash
cargo run --release -- observe token-evidence-pack --scope lifetime
cargo run --release -- observe token-evidence-pack --scope current_session --output /tmp/amai-token-evidence-pack.json
```

Если нужен proof именно для contractual money-preview слоя:

```bash
./scripts/proof_token_contractual_pricing.sh
./scripts/proof_token_contractual_sources.sh
./scripts/proof_token_freeze_close_semantics.sh
./scripts/proof_token_suitability.sh
./scripts/proof_token_meter_alignment.sh
./scripts/proof_token_mcp_tool_overhead.sh
./scripts/proof_token_cli_tool_overhead.sh
./scripts/proof_token_report_tool_overhead_autosync.sh
./scripts/proof_token_mcp_assistant_generation.sh
./scripts/proof_token_rollout_assistant_generation.sh
```

Dashboard hero-cards теперь тоже обязаны поднимать `client_limit_meter_alignment`.
Это нужно затем, чтобы оператор видел distinction между:
- lower-bound savings inside Amai;
- и тем самым внешним meter, которым клиент реально сжигает свой live `5h` лимит.

MCP `amai_context_pack` теперь тоже участвует в same-meter path:
- tool result после построения summary/stats дописывает observed `tool_overhead_tokens`
  в то же usage event по `context_pack_id`;
- для этого path есть отдельный proof `scripts/proof_token_mcp_tool_overhead.sh`;
- engineering/proof вызовы MCP должны уводиться через `token_source_kind = proof_* /
  verify_*`, чтобы не contaminate live lane.

CLI `context pack` front door теперь тоже участвует в same-meter path:
- после записи usage event он автоматически считает observed CLI output overhead по
  реально сериализованному stdout JSON payload;
- для этого path есть отдельный proof:
  `scripts/proof_token_cli_tool_overhead.sh`;
- это нужно затем, чтобы same-meter coverage по `tool_overhead_outside_retrieval`
  не зависел только от MCP front door.

Если missing `tool_overhead_tokens` остались в active live scope после старых context-pack events,
report path теперь умеет auto-sync их из stored `ami.context_packs.payload`:
- `observe token-report` берёт только active scope (`current_session / rolling_window`), а не весь
  lifetime ledger;
- по `context_pack_id` он достаёт сохранённый payload, пересчитывает CLI-equivalent output
  overhead и дописывает `tool_overhead_tokens` в latest snapshot;
- для этого path есть отдельный proof:
  `scripts/proof_token_report_tool_overhead_autosync.sh`

`assistant_generation` теперь тоже получил честный post-call attach path:
- CLI:
  `cargo run --release -- observe token-whole-cycle-attach --context-pack-id ... --assistant-generation-tokens ...`
- MCP:
  `amai_observe_whole_cycle`
- turn-group path:
  - CLI:
    `cargo run --release -- observe token-whole-cycle-turn-attach --thread-id ... --turn-id ... --context-pack-id ... --assistant-generation-tokens ...`
  - MCP:
    `amai_observe_whole_cycle_turn`
  - в MCP `thread_id` разрешено опустить только если `working_state` для всех
    `context_pack_ids` даёт ровно один thread; иначе attach должен fail-closed.
- для front-door proof есть отдельный сценарий:
  `scripts/proof_token_mcp_assistant_generation.sh`
- conflicting overwrite запрещён:
  если observed value уже materialized, другой attach с новым числом должен fail-closed.

Если runtime уже сохранил raw Codex rollout JSONL, можно materialize-ить
`assistant_generation_tokens` и через rollout-backed path:
- CLI:
  `cargo run --release -- observe token-rollout-assistant-generation --rollout-path ... --repo-root ... --apply`
- proof:
  `scripts/proof_token_rollout_assistant_generation.sh`
- этот path тоже fail-closed:
  attach выполняется только если rollout даёт unambiguous candidate с ровно одним
  `context_pack_id` внутри turn и ненулевым observed output token count.
- report path теперь поверх этого делает scoped auto-sync:
  при построении `token-report` он собирает все unambiguous rollout observations и
  пытается attach-ить только те `context_pack_id`, которые реально входят в current
  live retrieval scope и ещё не имеют `assistant_generation`.
- same-meter contour теперь дополнительно умеет делать turn-scoped matching:
  он поднимает `working_state_event` для live `retrieval_context_pack`, берёт
  `thread_id + captured_at_epoch_ms`, затем ищет rollout turn-timeline с тем же ответом
  клиента и считает `assistant_generation` один раз на matched turn-group.
- если пересечение между usable rollout IDs и текущим session/window correlation set равно
  нулю, `assistant_generation` в этом scope должен честно остаться `unmeasured`;
  это не баг surface, а truthful source-gap.
- этот source-gap теперь обязан быть виден machine-readable в
  `client_limit_meter_alignment.assistant_generation_observation_source`, чтобы operator
  мог увидеть состояния `assistant_generation_source_unavailable /
  assistant_generation_source_no_scope_overlap /
  assistant_generation_source_partial_scope_overlap /
  assistant_generation_source_covers_missing_scope`, а не только общую blocker-строку.
- rollout parser теперь обязан игнорировать shell heredoc/document text, где просто
  упоминаются `context pack` или `context packs`; approved source можно считать только
  по реальному command invocation path (`mcp__amai__amai_context_pack`,
  `cargo run ... context pack`, `./target/release/amai context pack`, `$AMAI context pack`,
  `memory search`), иначе same-meter contour начинает ловить ложные overlap из handoff/debug
  команд.
- тот же contour теперь умеет и прямой turn-scoped attach:
  `observe token-whole-cycle-turn-attach --thread-id ... --turn-id ...
  --context-pack-id ... --assistant-generation-tokens ...`
  и обязан считать такие токены один раз на `thread_id + turn_id`, а не
  дублировать их по каждому retrieval event.
- report/export path теперь тоже обязан поднимать этот turn-scoped observed output в
  `statement_preview.internal_observed_whole_cycle_lower_bound_tokens`, чтобы customer-facing
  preview и evidence pack не занижали same-meter lower bound только потому, что event-level
  attach для `assistant_generation` был бы нечестным.
- verify/MCP contour теперь тоже обязан это проверять:
  `verify mcp` вызывает `amai_observe_whole_cycle_turn` и ожидает
  `assistant_generation_turn_observed_attach`, а не ограничивается single-event attach.

`client_limit_meter_alignment` теперь нельзя читать как flat denominator по всем live events.
Operator contour обязан учитывать:
- `component_event_coverage[].target_live_events_count`
- `component_event_coverage[].target_scope_kind`
- `not_applicable_components`
- `baseline_equivalence.state / remaining_gap_reason`

Именно это позволяет не считать `continuity_restore_outside_retrieval` missing в scope,
где вообще не было live restore-event, и не завышать remaining gap искусственным
denominator drift.

Если remaining blocker сводится уже не к missing whole-cycle components, а к
`same_meter_baseline_unmeasured / same_meter_baseline_partially_measured /
same_meter_baseline_explicit_boundary`, operator обязан смотреть не только
`blocking_reasons`,
но и отдельный versioned contour:
- `client_limit_meter_alignment.baseline_equivalence.model_version`
- `client_limit_meter_alignment.baseline_equivalence.state`
- `client_limit_meter_alignment.baseline_equivalence.applicable_components`
- `client_limit_meter_alignment.baseline_equivalence.fully_observed_components`
- `client_limit_meter_alignment.baseline_equivalence.incomplete_components`
- `client_limit_meter_alignment.baseline_equivalence.measured_baseline_components`
- `client_limit_meter_alignment.baseline_equivalence.explicitly_unmodeled_baseline_components`
- `client_limit_meter_alignment.baseline_equivalence.missing_baseline_components`
- `client_limit_meter_alignment.baseline_equivalence.measured_baseline_tokens_lower_bound`
- `client_limit_meter_alignment.strict_client_meter_slice`
- `client_limit_meter_alignment.explicit_boundary_surface`

Это нужно затем, чтобы baseline-gap был machine-readable и не зависел только от human tooltip
в dashboard или одной blocker-строки.

Dashboard/operator contour теперь обязан это поднимать и user-facing:
- в tooltip строки `Связь с лимитом клиента`;
- в note для `whole_cycle_observed_baseline_partial`;
- с human-readable перечислением `fully_observed_components`,
  `measured_baseline_components`, `explicitly_unmodeled_baseline_components` и
  `missing_baseline_components`, если baseline-gap уже свёлся не к missing component
  coverage, а именно к partially materialized same-meter contour с explicit truth-boundary.

Отдельно operator теперь обязан смотреть `strict_client_meter_slice`:
- `strict_client_meter_slice.state`
- `strict_client_meter_slice.lower_bound_tokens`
- `strict_client_meter_slice.components`
- `strict_client_meter_slice.explicit_boundary_components`

Это нужно затем, чтобы already-measured strict same-meter lower bound не терялся внутри
общего `same_meter_as_client_limit = false`.

Отдельно operator теперь обязан смотреть `explicit_boundary_surface`:
- `explicit_boundary_surface.state`
- `explicit_boundary_surface.components`
- `explicit_boundary_surface.note`

Это нужно затем, чтобы explicit continuity boundary была surfaced отдельно от
already-measured strict same-meter slice и не выглядела как обычный missing gap.

Начиная с `client-limit-meter-alignment-v10` operator обязан проверять ещё и обратный
переход из boundary в exact pair:
- если `pre_amai_baseline_source_status.state = materialized`;
- и `baseline_equivalence.state = baseline_semantics_materialized`;
- и `blocking_reasons = []`;
- то `same_meter_as_client_limit` обязан стать `true`, а dashboard-строка
  `Экономия токенов модели` обязана показывать exact pair `без Amai / с Amai / экономия / процент`;
- если same-meter ещё не materialized, эта строка обязана fail-closed не показывать
  процент и прямо говорить, что `exact pair` ещё отсутствует.

Для live operator-review этого уже недостаточно.
Начиная с текущего dashboard contour оператор обязан видеть ещё и separate client-turn pressure:
- `Последний запрос клиента` обязан считаться только по
  `rollout token_count.last_token_usage.total_tokens / model_context_window`;
- `Лимит клиента сейчас` обязан подниматься из live `rate_limits.primary/secondary`;
- `Amai в полном live-turn` обязан показывать exact same-meter delta как долю полного
  observed client turn, а не как долю внутреннего retrieval slice;
- если exact same-turn pair для текущего live-turn не materialized, current-session card
  обязана fail-closed показывать `не доказано` и не имеет права переносить процент из
  внутреннего Amai-slice на полную шкалу клиента;
- human dashboard service не имеет права silently терять этот surface только потому,
  что он запущен вне `CODEX_THREAD_ID`: если thread-bound env отсутствует, service обязан
  сначала взять `thread_id` из latest repo `working_state_restore`, а не просто самый свежий
  repo-thread из SQLite;
- запрещено использовать `total_token_usage.total_tokens` как surrogate для текущего
  context-window pressure: это cumulative source, а не snapshot последнего запроса.
- если live client-turn pressure уже высокий, остаток 5h лимита низкий, а доля
  `Amai в полном live-turn` остаётся слабой, current-session card обязана уйти в
  operator-facing warning `новый чат рекомендован` или `новый чат нужен сейчас`;
- тот же operator-facing warning теперь обязан включаться и раньше, если exact full-turn pair
  ещё отсутствует, а live-turn уже раздут настолько, что реальную экономию на полной шкале
  честно доказать нельзя без перехода в свежий чат;
- такой warning обязан не просто красить статус, а давать прямое follow-up действие:
  сохранить continuity handoff и продолжить работу в свежем чате через continuity startup.
- managed startup instructions для поддерживаемых клиентов обязаны повторять этот же
  budget-law буквально: если текущий live-turn уже раздут и real full-scale effect
  не доказан, агент не имеет права продолжать дожигать текущий thread только потому,
  что внутренний Amai-slice выглядит экономным.

Customer-facing contractual export surface теперь тоже обязан поднимать
`adjustment_activation_governance`, чтобы future adjustment path был виден отдельно от
`settlement_activation_governance` и raw `adjustment_preview`.

Если нужен отдельный operator-safe inspect-layer по provider bindings, reconciliation и margin:

```bash
cargo run --release -- observe token-contractual-sources --scope lifetime
```

Если нужен customer-facing export bundle с manifest и готовыми JSON-файлами:

```bash
cargo run --release -- observe token-statement-export \
  --scope lifetime \
  --output-dir /tmp/amai-token-statement
```

Что это даёт:
- на выходе один JSON-пакет для review/export;
- в нём уже есть hashes по included/excluded line items;
- `statement_preview`, `reconciliation_preview` и `margin_scope` лежат рядом;
- pack остаётся `report_only`, пока billing/settlement не materialized честно.
- inspect-layer `token-contractual-sources` не заменяет evidence pack, а нужен затем,
  чтобы быстро увидеть:
  - какие truth sources реально bound;
  - какой `reconciliation_state` сейчас действует;
  - materialized ли `priced_preview_report_only` в margin.
- export bundle `token-statement-export` нужен затем, чтобы customer-facing review не
  собирать вручную из нескольких команд:
  - `manifest.json` даёт stable identity и список файлов;
  - `statement_export_preview.json` даёт compact review summary;
  - `contractual_evidence_pack.json` даёт line-item evidence;
  - `token_contractual_sources.json` даёт bindings и source-side explainability.

Для будущего token terminal действует утверждённый desktop interaction contract:

- double left click по label/token-card переворачивает карточку по горизонтали вокруг
  своей оси и показывает reverse side с chart surface;
- повторный double left click возвращает front side;
- double right click по любой карточке, независимо от `front/back`, переводит её в
  full-screen mode;
- повторный double right click возвращает её обратно;
- mobile path использует прямое touch-отображение этих же жестов:
  - двойной тап одним пальцем = double left click = flip front/back;
  - двойной тап двумя пальцами = double right click = full-screen toggle.

Это нужно учитывать заранее:

- terminal renderer ещё не materialized, поэтому gesture contract должен жить в docs
  раньше UI-кода;
- flip/full-screen interaction не даёт права показывать неmaterialized semantics;
- expanded mode остаётся truth-terminal, а не visual bypass текущих tokenonomics guardrails.

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

Для non-legacy contamination, который уже успел записаться в `live`-lane, теперь есть отдельный
operator-driven repair path без ручного SQL:

```bash
cargo run --release -- observe repair-token-ledger \
  --project-prefix memory_eval \
  --namespace continuity \
  --source-kind live_context_pack \
  --rewrite-source-kind verify_memory_matrix_context_pack \
  --repair-reason operator_memory_eval_cleanup
```

Важно:
- selector-ы без `--rewrite-source-kind` запрещены fail-closed;
- rewrite path не делает semantic guess, а переводит только те события, которые оператор явно
  выбрал по `project/project_prefix + namespace + source_kind`;
- факт repair остаётся внутри `token_budget_event.repair.operator_source_kind_rewrite`;
- runtime proof этого контура:
  - `scripts/proof_token_ledger_reclassify.sh`;
  - `scripts/proof_token_session_boundary.sh` теперь сам переводит свои synthetic live события в
    `proof_*`, чтобы не пачкать рабочее окно после PASS.

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
- отдельный manual-only cleanup contour есть для:
  - `output/windows-vm-lab`
- live state (`state/postgres`, `state/qdrant`, `state/minio`, `state/nats`) сюда специально не входит;
- тяжёлые output/evidence roots вроде `output/windows-vm-lab` не входят в auto-retention по умолчанию;
- список путей, TTL и `keep_latest` живут в [config/observability.toml](/home/art/agent-memory-index/config/observability.toml);
- auto-path защищает текущий исполняемый бинарь от удаления;
- `observe serve`, `observe snapshot` и `observe sla-check` сами запускают этот cleanup, поэтому локальный мусор должен уходить без ручного обхода.
- `--aggressive` — это уже explicit reclaim path: он не ждёт TTL и не держит `keep_latest`, но всё равно не лезет в live state и защищает текущий исполняемый бинарь.
- для `output/windows-vm-lab` cleanup остаётся explicit/manual:
  - `observe cleanup-artifacts --target output/windows-vm-lab --apply` теперь режет только тяжёлый rebuildable VM-хвост внутри proof-run старше `24h`, сохраняет `keep_latest = 2` и не трогает evidence/log артефакты;
  - `observe cleanup-artifacts --target output/windows-vm-lab --aggressive --apply` режет тот же rebuildable VM-хвост без ожидания TTL и без `keep_latest`, но всё равно сохраняет evidence/log артефакты;
  - auto-path их не удаляет;
  - symlink `output/windows-vm-lab/latest` игнорируется cleanup-сканером и не вмешивается в `keep_latest`;
  - mounted `payload.mount` больше не считается prune-target, чтобы случайный root-owned mountpoint не валил весь reclaim run;
  - evidence-preserving prune восстанавливает исходный `mtime` run-root, чтобы сам cleanup не переворачивал порядок `keep_latest` на следующем цикле;
  - после apply рядом с run-root пишется `windows_vm_lab_cleanup_manifest.json`, чтобы было видно, какие тяжёлые пути реально срезаны и сколько места вернулось.
- human dashboard теперь показывает отдельную карточку `Локальный мусор и retention`, чтобы оператор видел:
  - общий repo footprint;
  - сколько места сейчас покрывает managed cleanup policy;
  - сколько веса сейчас уже лежит вне policy и поэтому не может исчезнуть auto-cleanup path-ом;
  - какие крупные unmanaged roots сейчас формируют этот out-of-policy рост;
  - какие unreadable live-state paths не дают inventory дочитать repo полностью и почему footprint тогда считается best-effort lower bound;
  - какие manual-only contours уже заведены и каким explicit cleanup command их reclaim-ить;
  - safe reclaim now;
  - aggressive preview;
  - policy-retained hot storage, который уже covered cleanup policy, но ещё удерживается TTL/keep_latest;
  - exact operator reclaim commands для самых тяжёлых target-ов, если место нужно вернуть раньше auto-retention;
  - last reclaim;
  - почему объём проекта может ещё не уменьшаться, даже если aggressive preview уже большой.
- если объём уже policy-covered и удерживается только возрастным запасом/`keep_latest`, карточка должна surfac-ить это как `waiting`, а не как broken unmanaged growth.
- если основной диск уже упёрся в cleanup pressure thresholds, тот же policy-held объём должен эскалироваться в `alert/critical` и давать operator hint на target-specific aggressive reclaim вместо пассивного `waiting`.
- даже без disk pressure warning теперь обязан показывать ближайший exact reclaim command для крупнейшего retained target, чтобы оператор мог вернуть место раньше TTL осознанным manual run-ом.
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

Теперь этот contour пишет не только:
- `mean_precision`
- `case_hit_ratio`
- `head_hit_ratio`
- `mean_prompt_tokens`
- `mean_hybrid_savings_factor_vs_naive`

Но и один канонический eval-layer:
- `text_compare.canonical_eval.verdict_counts`
- `text_compare.canonical_eval.strategy_breakdown`
- `text_compare.canonical_eval.probes[*].eval_verdict_class`
- `runs[*].strategies.*.eval_verdict_class`

Смысл этого слоя:
- для каждого `case x strategy` verdict берётся из того же общего vocabulary `hit_correct_target / hit_wrong_target / stale_target / under_retrieved / over_included / recovered_useful / not_useful`;
- `hybrid` и `lexical_only` здесь часто показывают `over_included`, если цель нашлась, но вместе с ней приехал лишний контекст;
- `semantic_only` может показать `hit_wrong_target`, если retrieval что-то вернул, но нужная цель не попала в ответ.

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
- отдельно показывает `Правильное продолжение`, чтобы operator видел не только policy по сбоям, но и последний реальный proof того, что новый чат продолжает работу правильно.

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
- сам HTTP path human dashboard теперь не имеет права заново собирать весь live snapshot на каждый browser refresh:
  - тяжёлый `build_snapshot` вынесен в background cache-refresh внутри `observe serve`;
  - `/api/dashboard`, `/api/snapshot`, `/metrics` и `/healthz` читают последний готовый снимок из process-local cache;
  - из-за этого даже если очередной snapshot дорогой, страница должна отдавать последний готовый слой быстро, а не висеть на полном probe-contour;
  - summary панели теперь обязана поднимать `refresh`, `возраст` и stale-state этого process-local cache;
  - туда же теперь выводится самый дорогой stage последнего snapshot-refresh, чтобы operator видел реальное узкое место (`token_budget_report`, `collect_nats_live` и т.п.) без отдельного forensic-разбора raw snapshot;
  - hero-карты и token-rows внутри `observe serve` теперь строятся через отдельный `dashboard_read_only` token report; он сохраняет live/current/rolling/lifetime rollups и `client_limit_meter_alignment`, не имеет права тащить full contractual/export path в browser refresh contour и допускает только ограниченный quiet same-meter sync/write-back для active live scope текущей сессии и рабочего окна;
  - при этом live `client_live_meter` и строка `Лимит клиента сейчас` не имеют права оставаться на старом `token_count`, если для того же thread в rollout уже materialized более новый meter: request-side freshness guard обязан добрать этот refresh до ответа, а не только надеяться на background loop;
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
    она обязана показывать последние успешные числа и нейтральный статус `тест не запущен`;
  - этот нейтральный статус теперь должен определяться по реальному состоянию benchmark-runner-а:
    одного живого `/metrics` у отдельного Qdrant уже недостаточно, если сам внешний run workspace больше не активен;
- live latency слой на human dashboard теперь схлопнут в один общий блок `Скорость ответа`, а не размазан по трём похожим карточкам;
- `Скорость ответа` при маленькой live-выборке не имеет права притворяться деградацией:
  - пока живая выборка не добрала честный целевой объём, карточка остаётся в нейтральном статусе `идёт накопление выборки`;
  - только после достаточной выборки невыполнение эталона имеет право переводить этот блок в problem-status;
  - это session-scoped contour, а не историческая витрина: новый live `session/window` обязан
    начинать эту выборку заново;
  - пока отдельный historical latency contour не materialized, UI обязан прямо говорить, что
    карточка показывает только текущую сессию;
  - если runtime в текущей сессии дал только `mixed` latency slice без честного `hot/cold`
    разделения, human dashboard обязан показывать именно этот mixed live поток, а не пустые
    hot/cold placeholders;
  - в таком режиме target-rows для `Повторный запрос` и `Первый запрос` не должны подменять
    отсутствие классификации ложным `ещё нет данных`: карточка обязана честно сузиться до общего
    live потока, последнего запроса и реального размера mixed-выборки;
- карточка `Текущая работа` теперь должна показывать не только цель, следующий шаг и активные файлы, но и две отдельные explainability-строки:
  - почему последний retrieval что-то включил;
  - почему часть retrieval-слоёв в последний раз ничего не добавила;
- эта карточка теперь обязана читать именно `latest_repo_working_state_restore`, а не глобально самый новый `latest_working_state_restore`;
- `Текущая работа` при предварительном локальном снимке не должна краснеть:
  - пока рабочая линия ещё только накапливается, карточка держит нейтральный статус `ждём устойчивый снимок`;
  - problem-status разрешён только если restore-confidence реально низкий или есть другой честный сигнал сбоя;
- если локального рабочего снимка для текущего repo нет, human dashboard должен честно показать пустое состояние и не подмешивать свежий handoff другого проекта;
- если последняя локальная линия пришла не из свежего retrieval path, human dashboard не должен печатать пустые заглушки `ещё нет объяснения`: explainability-строки в таком случае просто скрываются;
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
- machine summary contour на human dashboard больше не имеет права дёргать privileged hardware
  probes на каждом секундном refresh:
  - после initial cache fill при startup background refresh loop не должен делать второй полный
    refresh немедленно:
    - initial `refresh_observe_cache(...)` уже заполняет cache до bind;
    - loop обязан сначала ждать `refresh_ms`, а не запускать ещё один cold pass сразу после старта;
  - `token_budget_dashboard_report` тоже не должен every-second re-cold'иться при неизменном
    semantic input:
    - steady-state dashboard обязан переиспользовать cached report по signature текущих
      `current_session / rolling_window / lifetime` events и assistant-scope contour;
    - при cache-hit runtime обязан обновлять только live age-поля scope без полной пересборки
      отчёта, чтобы wall-clock drift сам по себе не возвращал cold rebuild;
  - dashboard token ledger input тоже обязан жить через отдельный event-cache:
    - перед полным `load_events()` runtime сначала читает дешёвую DB summary по
      `token_budget_event / token_benchmark`;
    - если `count + latest_created_at` не изменились, cached parsed events должны
      переиспользоваться без нового полного ledger parse;
    - если summary изменилась только append-only хвостом, runtime обязан дозагрузить только
      ограниченный delta-tail вместо немедленного full reload всего lifetime ledger;
  - long-lived `observe serve` теперь обязан переиспользовать machine summary cache до `60` секунд;
  - static memory inventory provider chain (`sudo dmidecode`, `dmidecode`, `lshw`, `inxi`) теперь
    должен жить отдельно от минутного live cache:
    - memory type / speed разрешено держать как host-static cache до `6` часов;
    - это убирает бессмысленный минутный `sudo dmidecode --type 17`, но не отменяет live usage
      refresh по CPU/memory/disk telemetry;
  - CPU/memory refresh через `sysinfo` не должен индексировать процессы только ради machine cards;
  - если точные hardware details требуют тяжёлого provider chain, dashboard лучше покажет слегка
    устаревший machine summary, чем будет каждые секунды раздувать `/proc`-FD хвост и подвешивать
    весь live refresh.
- same-meter assist contour для human dashboard тоже не должен повторять одну и ту же тяжёлую
  работу каждую секунду, если active live scope не менялся:
  - внутри одного refresh нельзя повторно перечитывать и парсить тот же rollout JSONL ради
    rollout observations и turn observations:
    - parsed rollout turn observations теперь reuse-ятся по file signature rollout-файла;
    - это должно снимать лишний двойной parse active rollout даже тогда, когда input-driven
      invalidation уже действительно произошёл;
  - repo rollout observations теперь reuse-ятся по file signature текущего thread rollout, но
    downstream invalidation должен смотреть уже на semantic contents parsed observations, а не на
    любой raw file churn;
  - upstream source-bundle для `dashboard assistant scope` тоже должен жить отдельно от готового
    result-cache:
    - `working_state_event` и `assistant_generation_turn_observed` не должны every-tick
      перечитываться по full history-path, если `target_context_pack_ids`, summary этих
      snapshot-kind'ов и rollout source signatures задействованных thread-ов не менялись;
  - derived assistant scopes reuse-ятся по combined input signature:
    missing target sets + direct-turn snapshots + working-state meta + semantic contents parsed
    turn observations задействованных thread-ов;
  - quiet same-meter sync/write-back для dashboard повторяется только если меняется набор missing
    `assistant_generation/tool_overhead` context_pack_ids или semantic contents current rollout
    observations;
  - если такой quiet sync что-то дописал, тот же refresh больше не обязан сразу перечитывать весь
    token-event слой:
    - write-back materialize-ится в текущем тике;
    - token events подтягиваются следующим refresh, чтобы свежая live-активность не вносила
      лишний same-pass full reload в post-live contour;
  - truthful semantics остаётся fail-closed: как только меняется любой из этих inputs, cache обязан
    invalidated, а dashboard — снова пересчитать assist contour честно.
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
