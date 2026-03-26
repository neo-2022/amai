modified_at: 2026-03-26 09:43 MSK
Ручная сверка guide/docs: 2026-03-26 09:43 MSK

# Art-memory-agent-index (Amai)

![Amai lockup](brand/amai_lockup.svg)

`Amai` — это отдельный внешний инструмент для ИИ-агентов.
Он нужен для того, чтобы агент:
- не начинал каждую новую сессию с нуля;
- не путал один проект с другим;
- быстро находил код и документы;
- получал уже собранный полезный контекст вместо лишнего шума.

Это не плагин только для одной IDE.
Это отдельный backend/tooling contour, который можно подключать к:
- `VS Code`
- `Cursor`
- `JetBrains IDE`
- `CLI`
- `CI`
- другим агентным клиентам через `MCP`

## Что это простыми словами

`Amai` — не “один бесконечный общий чат”.

Правильнее думать о нём так:
- у агента есть `рабочий стол`
  - короткие важные правила и текущая вводная;
- есть `архив`
  - код, документы, история решений и артефакты;
- есть `общая доска`
  - важные вещи, которые могут видеть несколько агентов;
- есть `готовая подборка`
  - уже собранный контекст под конкретный запрос.

То есть `Amai` делает не “вечную болтовню”, а долговременную память с порядком.

## Что это даёт обычному человеку

Если без инженерного жаргона, `Amai` полезен тем, что:
- не нужно повторять ИИ одно и то же снова и снова;
- меньше шанс, что ИИ перепутает проекты;
- новый чат может продолжить ту же рабочую линию, а не собирать её заново по кускам;
- меньше тратятся токены на повторный ввод контекста;
- проще подключить один и тот же внешний инструмент к разным IDE;
- важные решения и правила не пропадают между сессиями.

## Как `Amai` продолжает работу в новом чате

Для этого у `Amai` теперь есть не только обычный `handoff`, но и отдельный слой `working state`.

Простыми словами:
- после `continuity handoff` и после `context pack` `Amai` сам обновляет текущее рабочее состояние;
- новый чат поднимает не только “что решили в конце”, но и:
  - текущую цель;
  - ближайший обязательный следующий шаг;
  - последние рабочие запросы;
  - активные файлы;
  - текущую непрерывную рабочую сессию.

Изоляция строится по 4 ключам:
- `project_code`
- `namespace_code`
- `agent_scope`
- `session_id`

Это сделано специально, чтобы:
- один проект не протекал в другой;
- параллельные агенты не тащили друг другу чужую рабочую линию;
- старые хвосты не подменяли текущую сессию.

Если агент один, обычно это уже работает без ручной настройки.
Если агентов несколько, каждому нужно задавать свой `AMAI_AGENT_SCOPE`.

Для tokenonomics есть ещё одно жёсткое правило:
- инженерные `proof/verify/benchmark` прогоны не должны по умолчанию попадать в тот же `live`
  telemetry lane, из которого строятся карточки `Экономия токенов за текущую сессию` и
  `Экономия токенов за рабочее окно`;
- если proof-runtime идёт через `verify *` CLI или через MCP proof-session и caller не передал
  `token_source_kind`, `Amai` теперь обязан сам подставлять non-live engineering source kind,
  а не молча писать `live_context_pack`.
- это правило относится и к `verify memory-matrix`: archival context-pack вызовы внутри matrix
  теперь тоже обязаны идти с `verify_memory_matrix_context_pack`, а не с CLI default
  `live_context_pack`.
- `current_session` теперь режется по latest `session_id`, а `live_continuity_startup`
  запускает новую logical session boundary; это нужно затем, чтобы новый chat/window не тащил
  старый live хвост только потому, что между ними не было большого time-gap.
- для этого слоя есть отдельный runtime proof:
  `scripts/proof_token_session_boundary.sh`
- если старый synthetic/proof хвост уже успел попасть в `rolling_window`, его теперь можно
  чинить не только новым session boundary, но и явным operator-driven repair path по selector-ам:
  `project`, `project_prefix`, `namespace`, `source_kind` и `rewrite_source_kind`;
- этот путь не делает silent background rewrite: оператор сам задаёт, какой именно contamination
  переводится из `live` в `proof/verify`, а `repair_reason` остаётся в payload как audit-trace.
- кроме этого report-layer теперь сам fail-closed подавляет stale `live_context_pack`, если для
  того же `project + namespace + measurement_scope + correlation_id` уже materialized более новый
  `proof_* / verify_* / benchmark_*` sibling;
- это нужно затем, чтобы старый инженерный live хвост не продолжал портить
  `rolling_window/current_session` и same-meter alignment только потому, что non-live repair
  появился как отдельный новый snapshot, а не как destructive rewrite старого live события.

Поверх этого `Amai` теперь автоматически собирает ещё и `chat-start restore pack`.

Это короткий готовый блок, который уже можно считать восстановленным рабочим контекстом для первого содержательного ответа нового чата.
В нём уже лежат:
- текущая активная линия;
- обязательный следующий шаг;
- незавершённые линии, к которым потом нужно вернуться;
- что уже materialized;
- последние действия;
- активные файлы;
- confidence recovery.

То есть новый чат теперь должен видеть не только summary “о чём проект”, а уже компактный готовый стартовый контекст для продолжения работы.

Поверх этого `ExecCtl` теперь уже держит и durable project-bound task ledger в `PostgreSQL`.
Это значит:
- task inventory больше не живёт только внутри текущего restore-window;
- continuity handoff пишет append-only task entry в SQL;
- startup/restore поднимает project task ledger уже из durable storage lane, а restore-side ledger остаётся fallback/shadow-путём.

Поверх durable ledger теперь surfaced ещё и machine-readable `ExecCtl resume contract`.
Его смысл:
- startup не только показывает, что есть pending lines;
- startup явно говорит, есть ли `required_return_task`;
- новый клиент или новый чат должен видеть это как обязательство к возврату, а не как свободный текстовый совет.

Поверх этого же baseline `ExecCtl` теперь surfaced и active lease:
- `execctl_active_lease`
- `execctl_active_lease_summary`
- durable SQL lane `ami.execctl_task_leases`

И ещё одна важная practical detail:
- `continuity startup`
- `continuity restore`
- `continuity answer`
- `continuity handoff`

теперь сами сначала делают schema-sync через `bootstrap_schema`.
Это нужно затем, чтобы partial-upgrade не ломал новый startup contour на ошибке вида
`relation ami.execctl_task_leases does not exist`.
То есть новый `ExecCtl` lane уже не требует отдельного ручного bootstrap шага перед каждым
startup/handoff: front-door сам приводит schema в совместимое состояние и только потом читает
или пишет lease/task state.

Важно: continuity должна переживать не только новый чат, но и смену окна, IDE и локализации проекта.
Для этого `Amai` держит project identity не только через один текущий `repo_root`, а через
`project_code` и registry привязанных project roots.
Практически это означает:
- если тот же проект переехал в другой path, его continuity не должна обнуляться;
- если агент подключился к тому же проекту из другого клиента, continuity не должна зависеть от того,
  был это `VS Code`, `CLI` или другой MCP-клиент;
- если новый path ещё не привязан к проекту, `Amai` должен fail-closed остановиться и потребовать
  явную relocation/register операцию, а не подмешивать проект “по похожести”.

### Что должно происходить автоматически, а что нет

Здесь важно не путать две разные вещи:

- разовое подключение клиентского runtime к `Amai`;
- автоматический startup/restore в каждом новом чате.

Правильная модель такая:
- один раз вы подключаете свой клиент к `Amai` через onboarding или `mcp config`;
- после этого открываете проект и пишете первое нормальное сообщение;
- специальная команда вроде `подними continuity` не должна быть обязательной;
- агент до первого содержательного ответа сам вызывает normal continuity startup/restore для этого проекта.

Для MCP-клиентов это теперь опирается не на устную договорённость, а на явный runtime entrypoint:
- tool `amai_continuity_startup`;
- prompt `amai-continuity-startup`.

То есть новый клиент теперь должен стартовать не “по ощущению”, а через тот же machine-readable
startup contract, который уже materialized в `continuity startup --json`.

То есть:
- открыть папку проекта недостаточно, если клиент вообще не подключён к `Amai`;
- руками вызывать restore в каждом новом чате не должно быть нужно;
- первое сообщение может быть обычным рабочим сообщением, а не магической фразой.

Если без специальной команды continuity не поднялась, это значит не “так и задумано”, а то, что
auto-start contour этого клиента ещё не доведён до конца.

## Пошаговый старт для обычного пользователя

Если вы хотите просто начать и не разбираться в лишней технике, идите по этим шагам сверху вниз.

## Шаг 0. Понять, где должна выполняться команда

`Amai` здесь означает папку самого проекта.

Проще говоря:
- если вы скачали архив, это папка, в которую вы его распаковали;
- если вы клонировали репозиторий, это папка, которую создал `git clone`;
- если вы уже читаете этот `README.md` внутри IDE, чаще всего нужная папка у вас уже открыта.

Как понять, что это именно она:
- в этой папке лежит сам `README.md`;
- рядом с ним лежат:
  - `Cargo.toml`
  - `compose.yaml`
  - папка `scripts/`
  - папка `docs/`
  - папка `src/`

Именно в этой папке нужно открыть терминал перед следующими шагами.

## Если вы пока не хотите ничего устанавливать

Этот режим нужен не для установки, а только для спокойной проверки.

Разница простыми словами такая:
- `./scripts/install_amai.sh`
  - показывает проверку;
  - даёт выбрать профиль;
  - спрашивает `ДА`;
  - после подтверждения уже начинает что-то менять на машине;
- `./scripts/preflight.sh`
  - только показывает, что тянет машина;
  - ничего не устанавливает;
  - ничего не меняет;
  - ничего не пишет в config.

То есть `preflight` нужен на случай, когда человек пока не хочет ставить продукт, а хочет сначала просто понять:
- подходит ли его машина;
- какой режим для неё лучше;
- стоит ли вообще продолжать.

### Linux и macOS

```bash
./scripts/preflight.sh
```

### Windows PowerShell

```powershell
.\scripts\preflight.ps1
```

### Windows CMD

```bat
scripts\preflight.cmd
```

Если нужен не общий выбор, а проверка только одного конкретного профиля:

```bash
./scripts/preflight.sh --stack-profile default
./scripts/preflight.sh --stack-profile lite_vps
```

## Шаг 1. Запустить установку одной командой

Обычному человеку не нужно заранее угадывать профиль руками.

Нормальный путь теперь такой:
- вы запускаете одну команду;
- `Amai` сам сначала проверяет машину;
- потом показывает два профиля установки;
- вы выбираете `1` или `2`;
- потом пишете `ДА`;
- только после этого стартует установка.

### Linux и macOS

```bash
./scripts/install_amai.sh
```

### Windows PowerShell

```powershell
.\scripts\install_amai.ps1
```

Этот wrapper сейчас не делает вид, что умеет честно поднимать локальный Windows stack.
Если запустить его без `--ssh-destination`, он должен fail-closed и прямо сказать, что для локального stack path пока нужен `WSL2`.
Безопасный Windows path сейчас такой:
- локальный stack поднимать в `WSL2`;
- или использовать этот wrapper только для remote-host onboarding с `--ssh-destination`.

### Windows CMD

```bat
scripts\install_amai.cmd
```

Что будет на экране:
- `Amai` покажет CPU, память и диск;
- затем покажет два варианта:
  - `1` = полноценный локальный режим;
  - `2` = лёгкий удалённый режим;
- если найдёт несколько подходящих клиентов, покажет их и даст выбрать нужный;
- если один из вариантов машине не подходит, вы увидите `ПРЕДУПРЕЖДЕНИЕ`;
- если вариант подходит только с оговорками, вы тоже увидите `ПРЕДУПРЕЖДЕНИЕ` до установки.
- после установки внешний compatibility bridge `memory` на этой машине тоже будет переведён на `Amai`.

Это значит, что привычные команды:
- `memory context`
- `memory search`
- `memory save`
- `memory mcp`

после установки уже должны идти через `Amai`, а не через старый внешний bridge.

Важно:
- `memory search` теперь должен печатать не только найденные записи, но и два коротких explainability-summary:
  - почему результаты вошли в текущую выборку;
  - почему часть retrieval-слоёв ничего не добавила;
- это сделано специально, чтобы compatibility bridge не был “немой прокладкой”, а честно объяснял смысл retrieval даже в старом CLI-потоке.

## Шаг 2. Выбрать профиль `1` или `2`

После первой команды `Amai` сам предложит выбор.

Простыми словами:
- `1` выбирайте, если хотите полноценную локальную установку на нормальной машине;
- `2` выбирайте, если вам нужен лёгкий режим для дешёвого VPS или удалённого сценария.

Если выберете слишком тяжёлый профиль, `Amai` не будет молча идти дальше.
Он сначала явно покажет предупреждение.

## Шаг 3. Написать `ДА`

После выбора профиля `Amai` ещё раз спросит подтверждение.

Просто напишите:

```text
ДА
```

Только после этого `Amai` начнёт что-то менять на машине.

## Шаг 4. Что делает установка

После подтверждения `Amai`:
- создаёт или досинхронизирует `.env`;
- поднимает stack;
- собирает binary;
- пишет MCP config для клиента;
- печатает финальную сводку уже после установки;
- показывает живые метрики машины и stack;
- если live-выборка уже накоплена, показывает главный KPI по токенам:
  - `Проверенная реальная экономия`;
  - за текущую рабочую сессию;
  - за текущее окно лимита;
  - за всё время.

Если `auto-detect` выбрал не тот клиент, можно указать явно.

### Postgres transport и безопасность DSN

`Amai` больше не должен светить пароль из `AMI_POSTGRES_DSN` в текстах ошибок.
Если `PostgreSQL`-подключение падает, в сообщении должен оставаться только безопасный descriptor:
- пользователь;
- host;
- порт;
- dbname;
- `sslmode`;
- но не пароль.

Сам transport теперь тоже зависит не от жёсткого `NoTls`, а от `sslmode` внутри самого DSN:
- `sslmode=disable` оставляет plain path без TLS;
- остальные режимы идут через native TLS connector.

Это важно для двух реальных runtime-path:
- основной `Amai` binary;
- compatibility binary `memory`, который сам резолвит project code через `PostgreSQL`.

### Linux и macOS

```bash
./scripts/install_amai.sh --client vscode
./scripts/install_amai.sh --client cursor
./scripts/install_amai.sh --client codex
```

### Windows PowerShell

```powershell
.\scripts\install_amai.ps1 --ssh-destination ops@example-host --remote-repo-root /srv/amai --client vscode
.\scripts\install_amai.ps1 --ssh-destination ops@example-host --remote-repo-root /srv/amai --client cursor
.\scripts\install_amai.ps1 --ssh-destination ops@example-host --remote-repo-root /srv/amai --client codex
```

### Windows CMD

```bat
scripts\install_amai.cmd --ssh-destination ops@example-host --remote-repo-root /srv/amai --client vscode
scripts\install_amai.cmd --ssh-destination ops@example-host --remote-repo-root /srv/amai --client cursor
scripts\install_amai.cmd --ssh-destination ops@example-host --remote-repo-root /srv/amai --client codex
```

## Шаг 5. Что делать дальше

В конце `Amai` покажет:
- куда записал config;
- какой клиент выбрал;
- почему выбрал именно его;
- какие ещё клиенты были найдены;
- что делать дальше;
- живые цифры по машине и stack.

Обычно после этого остаётся:
- открыть IDE;
- сделать `Reload Window` или перезапустить клиент;
- проверить, что `Amai` подключился.

## Как руками проверить continuity recovery

Если хотите проверить это не на словах, а живьём:

```bash
./scripts/continuity_answer.sh --project art --namespace continuity --intent last_chat
./scripts/continuity_answer.sh --project art --namespace continuity --intent last_chat --json
./scripts/continuity_answer.sh --project art --namespace continuity --intent last_chat --include-previous-chat-messages --messages-count 2
./scripts/chat_lookup.sh --project art --namespace continuity --question "что было в прошлом чате, какие последние два сообщения?" --json
./scripts/continuity_startup.sh --project art --namespace continuity
./scripts/continuity_startup.sh --project art --namespace continuity --json
./scripts/continuity_restore.sh --project art --namespace continuity
cargo run --quiet -- verify continuity --project art --namespace continuity
./scripts/proof_art_continuity_answer.sh
./scripts/proof_art_continuity_restore.sh
./scripts/proof_art_continuity_startup.sh
```

Разница такая:
- `continuity answer`
  - печатает уже готовый короткий ответ для continuity-вопросов вроде `на чём остановились`;
  - это read-only путь без нового handoff;
  - если добавить `--json`, тот же one-shot product path возвращает machine-readable payload с `canonical_eval`, `retrieval_science` и честным verdict-классом для конкретного ответа (`recovered_useful` или fail-closed `hit_correct_target`);
  - если в текущем `working_state_restore` уже есть свежий `latest_decision_trace`, тот же ответ теперь прямо показывает:
    - почему вошёл текущий контекст;
    - почему часть retrieval-слоёв ничего не добавила;
    - и в `--json` кладёт это в `included_reasons_summary` / `excluded_reasons_summary`, чтобы explainability была видна не только в dashboard;
  - если добавить `--include-previous-chat-messages`, он ещё поднимет хвост предыдущего чата и последние сообщения;
- `continuity startup`
  - печатает human-readable стартовую сводку для нового чата;
  - и сразу добавляет `Chat-start restore pack`, который уже нужно считать восстановленным рабочим контекстом для первого содержательного ответа;
  - этот path должен вызываться клиентом автоматически при старте нового чата для уже подключённого проекта;
  - пользователь не должен руками запускать его в каждом новом окне;
  - теперь в этом выводе печатается и готовый `prompt_text`, чтобы новый чат получал не только summary, но и прямой compact restore для первого ответа;
  - тот же `Chat-start restore pack` теперь ещё и прямо объясняет:
    - почему вошёл последний контекст;
    - почему часть retrieval-слоёв в последний раз ничего не добавила;
  - если добавить `--json`, тот же startup path теперь отдаёт `continuity_startup.canonical_eval`, `chat_start_restore`, `working_state_restore`, `retrieval_science` и `degradation_policy`, чтобы startup recovery можно было проверять machine-readable слоем, а не только глазами;
- `continuity restore`
  - печатает raw restore-bundle JSON целиком;
  - теперь в этом JSON есть не только `working_state_restore`, но и отдельный `chat_start_restore` с готовым `prompt_text`;
  - в этом же `chat_start_restore` теперь лежат и короткие explainability-summary:
    - `included_reasons_summary`
    - `excluded_reasons_summary`;
  - сам `working_state_restore` теперь тоже поднимает те же summary прямо в своём JSON:
    - `included_reasons_summary`
    - `excluded_reasons_summary`;
  - human-readable `working_state` вывод теперь печатает их как:
    - `Почему вошло`
    - `Почему часть не вошла`;
  - и тот же product path теперь сразу несёт `continuity_restore.canonical_eval`, `retrieval_science` и `degradation_policy`, чтобы machine-readable recovery было видно не только по сырому содержимому, но и по verdict-слою;
  - внутри `working_state_restore` теперь есть ещё и execution-aware слой:
    - `next_step_state = planned`;
    - `recent_actions[].execution_state` (`attempted / succeeded / superseded / stale`);
    - `action_state_counts`;
    - `pending_return_queue`, `pending_return_summary` и `execctl_resume_state`, чтобы новый
      handoff не затирал предыдущую рабочую линию молча, а оставлял machine-readable след
      обязательного возврата;
    - `project_task_tree` и `project_task_tree_summary`, чтобы active line и suspended
      worklines уже жили как project-bound open-task tree, а не как один только headline;
    - `project_task_ledger` и `project_task_ledger_summary`, чтобы continuity restore уже держал
      append-only handoff history как task ledger с `active / pending_return / historical_handoff`,
      а не только текущее открытое дерево;
    - `state_lineage` с `lineage_model_version = lineage-v2`, authoritative event, truth ranking и явным graph-слоем `nodes / edges`, чтобы было видно, какой event authoritative, какие его поддерживают и какие уже superseded.
    - `workspace_graph` с `workspace_graph_model_version = workspace-graph-v10` и `artifact_lineage_model_version = artifact-lineage-v1`, где recent retrieval теперь materialize-ится как graph `context_pack -> file / structure_item / symbol / chunk / import_ref / export_ref / call_ref`, а resolved relations `imports_file / re_exports_file / imports_symbol / re_exports_symbol / resolves_file / resolves_symbol / calls_file / calls_symbol / resolves_call_file / resolves_call_symbol` уже учитывают owner-aware Rust symbol lookup для provable случаев вроде `Type::new`, `Self::helper()`, `self.helper()`, trait-qualified forms вида `<Type as Trait>::make`, тех же trait-qualified forms через доказанный imported alias, module-alias forms вроде `trait_mod::Factory`, owner-side module alias paths вроде `type_mod::Beta::new` и combined forms вроде `<type_mod::Beta as trait_mod::Factory>::make` и `<type_mod::Beta as FactoryAlias>::make`, не переходя к небезопасному type inference; если owner или trait пришли в impl через видимый selector, graph теперь дополнительно пишет `owner_path_canonical` и `trait_name_canonical` только при единственном доказуемом target;
    - fail-closed semantics для plain-symbol и owner-aware resolution дополнительно зажаты property-based tests: неоднозначный кандидат в любом candidate-file или более одного уникального candidate-file обязаны приводить к `None`, а не к “лучшему предположению”.
    - тот же `workspace_graph_summary` теперь поднимается и в `chat_start_restore`, и в human-readable startup/restore вывод, если в текущей линии уже были свежие retrieval-context события.
    - тот же `chat_start_restore.prompt_text` теперь поднимает `Незавершённые линии к возврату`
      и явный `ExecCtl` warning, если в проекте уже есть suspended workline, чтобы новый чат
      не делал silent preemption поверх старой задачи.
- `verify continuity`
  - это уже не human-readable startup, а прямой machine-readable proof-контур для recovery;
  - теперь он покрывает не только startup/import/replay, но и direct temporal chat lookup;
  - а для самого product path есть отдельный proof `scripts/proof_art_continuity_answer.sh`, который прогоняет `continuity answer --json` и `chat_lookup.sh --json` на `last_chat / previous_chat / exact-time fail-closed`.
  - отдельный proof `scripts/proof_art_continuity_restore.sh` теперь так же проверяет уже machine-readable `continuity restore` и ожидает `2 x recovered_useful` по `chat_start_restore` и `working_state_restore`.
  - отдельный proof `scripts/proof_art_continuity_startup.sh` теперь проверяет `continuity startup --json` и ожидает `3 x recovered_useful` по `startup_summary`, `chat_start_restore` и `working_state_restore`.
  - отдельный proof `scripts/proof_execctl_pending_return.sh` проверяет, что второй handoff не
    стирает предыдущую линию молча, а поднимает её в `pending_return_queue`.
  - тот же proof теперь дополнительно проверяет, что restore уже materialize-ит
    `project_task_tree` с active task и pending-return узлом.
  - и тот же proof теперь дополнительно проверяет `project_task_ledger`, где active/pending
    линии уже лежат рядом с append-only historical handoff history.
  - он проверяет уже 9 отдельных probe:
    - есть ли свежий `continuity_handoff`;
    - собрался ли `working_state_restore`;
    - есть ли непустой `chat_start_restore.prompt_text`;
    - не подменяет ли поздний replay старый handoff;
    - не подменяет ли поздний replay старый import;
    - умеет ли direct `previous_chat` вернуть осмысленный прошлый chat tail;
    - умеет ли direct `chat_at_time` вернуть точный смысловой срез по времени;
    - fail-closed ли ведёт себя `previous_chat`, когда такого смещения назад нет;
    - fail-closed ли ведёт себя `chat_at_time`, когда точного временного совпадения нет.
  - этот contour пишет `continuity_verification.canonical_eval`, где:
    - полезное recovery идёт как `recovered_useful`;
    - честный fail-closed на прямом temporal lookup считается как `hit_correct_target` для isolation-boundary pattern;
    - replay-регресс опускается до `stale_target`, а не маскируется под success.

Если у вас несколько параллельных агентов:

```bash
AMAI_AGENT_SCOPE=agent_alpha ./scripts/continuity_startup.sh --project art --namespace continuity
AMAI_AGENT_SCOPE=agent_beta  ./scripts/continuity_startup.sh --project art --namespace continuity
```

Так каждый агент будет продолжать только свою линию.

Если клиент даёт `CODEX_THREAD_ID`, `Amai` теперь может отличать текущий chat thread от предыдущего.
Это нужно, чтобы вопрос вида `на чём закончился прошлый чат, какие были последние два сообщения` не уходил в случайный старый transcript.
Смысл слова `прошлый` здесь строгий:
- это не “что-то старое по смыслу”;
- это ближайший соседний chat thread по времени внутри того же `project + namespace + agent_scope`;
- archived thread тоже считается нормальным кандидатом, потому что прошлый чат обычно уже архивный.

Для temporal lookup теперь есть отдельный универсальный helper:

```bash
./scripts/chat_lookup.sh --project art --namespace continuity --chat-reference current --messages-count 2
./scripts/chat_lookup.sh --project art --namespace continuity --chat-reference previous --messages-count 2
./scripts/chat_lookup.sh --project art --namespace continuity --chat-reference previous:2 --messages-count 2
./scripts/chat_lookup.sh --project art --namespace continuity --at-time-rfc3339 2026-03-21T11:41:00+03:00 --messages-count 2
./scripts/chat_lookup.sh --project art --namespace continuity --question "на чем закончили в прошлом чате, какие последние два сообщения?"
./scripts/chat_lookup.sh --project art --namespace continuity --question "что было в позапрошлом чате?"
./scripts/chat_lookup.sh --project art --namespace continuity --question "о чем мы говорили в позапрошлую среду в 12:00?"
./scripts/chat_lookup.sh --project art --namespace continuity --question "о чем мы говорили в прошлую среду в 12:00?"
```

Этот helper теперь ведёт себя строже и понятнее:
- bare CLI path тоже должен работать из другого каталога, потому что `Amai` сам дочитывает свой `.env` из канонического repo root;
- если `previous:30` или вопрос указывает на момент вне диапазона известных чатов, ответ обязан fail-closed, а не подменять это текущим handoff;
- шумные thread labels уровня `AGENTS.md прочитан`, `Продолжай строго`, `# Context from my IDE setup`, `## Active file`, `## Open tabs` больше не должны попадать в human answer;
- если полезного human label нет, helper лучше опускает строку про chat thread, чем показывает UUID или технический мусор.

Это один и тот же прямой путь для трёх случаев:
- `current chat`
- `previous chat`
- `chat at exact time`

То есть вопрос вида `что было в прошлую среду в 12:00` теперь не должен разваливаться на
несколько обходов через transcript search. `Amai` сначала берёт thread index, потом выбирает
подходящий chat по времени и только затем поднимает нужные сообщения.

Если время выходит за пределы известных чатов, `Amai` теперь должен отвечать честно:
- не подсовывать ближайший текущий thread;
- а писать, что для этого момента нет точного совпадения в известных чатах.

Важно: полнота temporal lookup теперь не должна зависеть от того, сколько full transcript-боди
вообще импортировано в continuity namespace. При refresh `Amai` отдельно втягивает machine-readable
`thread_index.json`, чтобы иметь полный список chat threads и их временные метки, а уже full
rendered transcripts можно ограничивать маленьким `transcript-limit` ради скорости и размера импорта.

Этот шаг теперь идёт отдельным upstream enrich-path, а не поздним пересчётом в момент ответа:

```bash
./scripts/enrich_thread_index.sh --input state/continuity-imports/art/thread_index.json
```

Этот helper:
- читает raw `thread_index.json`;
- materialize-ит compact поля thread summary заранее;
- materialize-ит ещё и `time_slices`: короткие смысловые срезы внутри chat thread с собственным временем,
  user-anchor, assistant-anchor и compact summary;
- пишет enriched index, который потом уже уходит в temporal import;
- позволяет `previous chat` и `exact time` отвечать быстрее и стабильнее.

Дополнительно `Amai` теперь materialize-ит в temporal index не только `thread_id`, время и хвост
сообщений, но и короткие поля:
- `summary_headline`
- `summary_next_step`

Это важно по двум причинам:
- ответ на `прошлый чат` или `что было в точное время` теперь может сразу брать готовую line-summary
  из `Amai`, а не вытаскивать смысл заново из длинного assistant-абзаца;
- временной lookup меньше зависит от post-hoc parsing старых текстов и быстрее отдаёт готовый
  человекочитаемый ответ.

Для `exact time` теперь действует ещё более жёсткое правило:
- `Amai` должен отвечать по готовому `time-local` evidence внутри выбранного thread;
- если такого evidence нет, он обязан честно fail-closed;
- подсовывать просто последние сообщения thread-а вместо точного времени запрещено.

Проще:
- `позапрошлая среда в 12:00` может вернуть короткий смысловой срез, если в temporal index есть
  подходящий `time_slice`;
- `прошлая среда в 12:00`, если для этого окна нет точного среза, теперь должна вернуть честное
  `нет точного совпадения в известных чатах`, а не “примерно похожий” chat.

Для обычного человека это значит ещё проще:
- можно не помнить флаги `--chat-reference` и `--at-time-rfc3339`;
- достаточно передать сам живой вопрос через `--question`;
- `Amai` сам извлечёт из него:
  - текущий или прошлый чат;
  - нужное время;
  - сколько последних сообщений показать.

## Как смотреть пользу онлайн без терминала

Если не хочется каждый раз читать JSON, Prometheus или длинный текст в терминале, у `Amai` теперь есть своя человеческая панель.

### Linux и macOS

```bash
./scripts/human_dashboard.sh
./scripts/human_dashboard_down.sh
```

### Windows PowerShell

```powershell
.\scripts\human_dashboard.ps1
.\scripts\human_dashboard_down.ps1
```

### Windows CMD

```bat
scripts\human_dashboard.cmd
scripts\human_dashboard_down.cmd
```

После запуска `human_dashboard` теперь не держится за открытый терминал.
На Linux launcher сначала поднимает `observe serve` как `systemd --user` сервис `amai-human-dashboard.service`, а если user-level `systemd` недоступен, честно уходит в обычный detached fallback.
Для Linux статус и логи теперь правильно смотреть через `systemctl --user status amai-human-dashboard.service` и `journalctl --user -u amai-human-dashboard.service`.
По умолчанию панель обновляется раз в `1` секунду, чтобы live-цифры были ближе к реальному состоянию, а не висели как старый снимок.
Но сам `observe serve` теперь не пересобирает полный live snapshot на каждый браузерный refresh:
- тяжёлый `build_snapshot` живёт в фоновом cache-refresh контуре процесса;
- `/api/dashboard`, `/api/snapshot`, `/metrics` и `/healthz` читают уже готовый последний снимок;
- в summary панели теперь прямо видны `refresh`, `возраст` и состояние кэша, чтобы было ясно, страница ждёт браузер или сам snapshot-builder;
- там же теперь выводится и самый дорогой stage последнего snapshot-refresh, чтобы operator сразу видел узкое место без отдельного raw JSON разбора.
- сами карточки при этом больше не тянут полный contractual `observe token-report`: для `observe serve` они используют отдельный `dashboard_read_only` token report без quiet sync/write-back и без разворачивания export/settlement contours на каждый refresh.

После запуска откройте в браузере:

```text
http://127.0.0.1:9464/
```

Если у вас поменян `AMI_OBSERVE_BIND`, адрес будет таким же, но с вашим портом.

Если потом нужно остановить human dashboard, используйте симметричную команду `human_dashboard_down`.

Что показывает эта панель:
- сколько токенов `Amai` уже сэкономил по проверенной live-выборке;
- сколько токенов сэкономлено:
  - за текущую сессию;
  - за текущее рабочее окно;
  - за всё время;
- для каждой карточки теперь явно подписано, откуда пришла цифра:
  - живой поток текущей сессии;
  - живой системный probe;
  - последний сохранённый benchmark;
- системный блок `Qdrant` теперь разделён на два разных live-источника:
  - `Qdrant Amai live`
    - это основной векторный слой `Amai`;
  - `Qdrant внешнего бенча`
    - это отдельный live-инстанс для внешнего benchmark-прогона;
  - эти две карточки не смешивают точки, память и сегменты друг друга;
  - если внешний benchmark уже закончился или не запущен, карточка не должна превращаться в error-log:
    она держит последние успешные значения и переключается в нейтральный статус `тест не запущен`;
  - статус внешней Qdrant-карточки теперь определяется не только по `/metrics`, а по реальному состоянию внешнего run workspace:
    если отдельный Qdrant ещё отвечает, но сам benchmark-runner уже умер или остановлен, карточка всё равно обязана показывать `тест не запущен`;
- live top-cards теперь честно отделяют `нужно внимание` от `ещё мало доказательной базы`:
  - `Скорость ответа`
    - не уходит в `внимание`, пока живая выборка просто мала;
    - в таком случае карточка показывает нейтральный статус `идёт накопление выборки`;
  - `Текущая работа`
    - не уходит в `внимание`, если локальная рабочая линия уже есть, но ещё не накопила достаточно устойчивый снимок;
    - в таком случае карточка показывает нейтральный статус `ждём устойчивый снимок`;
- machine-cards теперь показывают не только статическую сводку, но и живую hardware-телеметрию:
  - `CPU`
    - живая общая загрузка;
    - текущая температура;
    - максимум частоты;
  - `Оперативная память`
    - автоматический тип;
    - автоматическая скорость;
    - занято/свободно;
    - usage и swap;
  - `Основной диск`
    - автоматически определённый block device и тип (`NVMe SSD` / `SSD` / `HDD`);
    - объём, свободное место и usage;
    - живая нагрузка `I/O`, чтение и запись между refresh-ами;
    - температура и firmware;
  - `Графика и ускорители`
    - вместо одной жёстко пришитой `GPU`-строки теперь есть accelerator-layer;
    - он пытается автоматически найти:
      - встроенную графику `iGPU`;
      - дискретные `GPU`;
      - внешние `eGPU`;
      - другие ускорители, если их реально видит ОС или vendor tooling;
    - основным в карточке показывается устройство с самым богатым live-профилем;
    - если устройств несколько, дополнительные перечисляются отдельно;
    - если ускоритель найден только inventory-путём, а live telemetry недоступна, панель честно оставляет поля пустыми;
    - если ускорителей нет вообще, карточка не исчезает, а показывает `не обнаружено`;
  - `Установленный клиент` и `Сборка`
    - теперь живут как компактные карточки рядом друг с другом, а не занимают место hardware-card первого ряда;
- если внешний benchmark уже остановился, human dashboard больше не пишет ложное `Сейчас` для его памяти, points и segments:
  - при остановленном тесте эти строки явно переходят в режим `Последний срез ...`;
  - это означает не живое текущее состояние, а последний измеренный или последний сохранённый срез перед остановкой;
- для живой скорости ответа теперь есть один общий human-first блок `Как Amai отвечает сейчас`;
- сверху в нём рядом стоят две крупные цифры:
  - `Повторный запрос`
  - `Первый запрос`;
- обе крупные цифры теперь показывают не случайный последний замер, а медиану `P50`, чтобы обычный человек видел более устойчивую картину;
- ниже живёт одна общая comparison table без дублирования:
  - `P50 / P95 / P99 / Max / Выборка`
  - отдельными строками `Эталон` и `Сейчас` для `hot` и `cold`;
- строка `Эталон` теперь заполняется полностью:
  - фиксированные цели не зависят от текущей сессии;
  - в таблице они показываются с буквальным знаком сравнения вроде `<=` и `>=`;
  - для повторного запроса сейчас действует:
    - `P50 < 1 ms`
    - `P95 < 1 ms`
    - `P99 < 2 ms`
    - `Max < 5 ms`
    - `Выборка > 200`;
  - для режима без прогрева сейчас действует более строгий контур:
    - `P50 < 2 ms`
    - `P95 < 4 ms`
    - `P99 < 6 ms`
    - `Max < 10 ms`
    - `Выборка > 100`;
- `mix` как отдельная живая карточка убран, чтобы не смешивать и не дублировать `hot` и `cold`;
- англицизмы и спецтермины теперь расшифровываются подсказкой при наведении курсора на сам термин, без отдельного значка `?`;
- отдельный блок `Последние честные проверки` теперь хранит только непотоковые сохранённые прогоны:
  - быстрый путь под нагрузкой;
  - полный холодный прогон;
  - точность и изоляция.
- launcher `human_dashboard.sh` и `run_observe_exporter.sh` теперь сами подхватывают локальный `state/tooling/cmake-venv/bin`, если он уже materialized, чтобы observability-path не падал только из-за отсутствия системного `cmake`.
- карточка `Быстрый путь под нагрузкой` теперь явно помечена как `benchmark`, а не как live-метрика:
  - в ней есть только последний сохранённый hot-load прогон;
  - живая сессия страницы туда не подмешивается;
  - внутри неё одна compare-table `Метрика / Эталон / Тестовые данные`, а не смесь из benchmark и live-показаний;
  - для этой карточки сейчас фиксирован такой целевой набор:
    - `QPS > 35000`
    - `P50 < 0.012 ms`
    - `P95 < 0.015 ms`
    - `P99 < 0.020 ms`
    - `Max < 0.5 ms`
    - `error rate < 0.00%`
    - `workers > 16`
    - `Выборка > 10000`;
- не течёт ли один проект в другой;
- последний честный `Cold contour` verdict:
  - `TARGET MET`
  - `PARTIALLY MET`
  - `NOT MET`;
- если такой contour уже гоняли, панель сразу показывает:
  - `P50 / P95 / P99 / Max`
  - `precision / recall / hit rate`
  - сколько repo и query slices вошло в последний run;
- compact proof-run теперь не должен искусственно раздувать cold хвост на scope без vector points:
  - если для репозитория embeddings не строились, semantic path честно short-circuit-ится;
  - exact document lookup больше не должен regex-фильтровать весь namespace:
    `relative_path`, `relative_basename` и `relative_basename_stem` теперь живут как индексируемый SQL contour;
  - single exact-document pack и single symbol-only pack теперь не обязаны строить полный file graph:
    для них честно materialize-ится минимальный `context_pack -> file` или `context_pack -> symbol` graph без лишнего provenance-хвоста;
  - это позволяет смотреть на реальный cold-path retrieval, а не на пустую оплату embed/search без единого hit;
- что происходит внутри `PostgreSQL`, `Qdrant`, `NATS` и слоёв точности;
- на каком железе сейчас всё работает и к какому клиенту уже привязана установка.

Это не замена инженерному monitoring-слою.
Это именно human-first страница, где обычный человек видит:
- жива ли система;
- даёт ли она реальную пользу;
- и где именно сейчас проблема, если она появилась.

## Если запускаете установку повторно

Это не должно ломать систему и не должно размножать одинаковые записи.

Повторная установка нужна для нормальной пересинхронизации.
Сейчас `Amai` при повторном запуске:
- снова проверяет машину;
- снова даёт выбрать профиль;
- не клонирует вторую запись `amai` в MCP-config, а обновляет существующую;
- досинхронизирует `.env`, не стирая уже существующие значения;
- убеждается, что stack поднят;
- при необходимости пересобирает binary.

Простыми словами:
- повторный install не означает “вторая копия поверх первой”;
- это скорее “проверить и аккуратно досинхронизировать”.

## Шаг 6. Если нужно удалить

Симметричное удаление:

```bash
./scripts/remove_amai.sh
```

Или явно для конкретного клиента:

```bash
./scripts/remove_amai.sh --client vscode
./scripts/remove_amai.sh --client codex
```

## Если у вас уже есть старая continuity-схема

Если проект уже жил не в `Amai`, это не означает, что нужно всё начинать с нуля.

`Amai` умеет забрать в себя старую continuity-линию из внешних источников, например:
- bootstrap-файл continuity;
- старый handoff-файл, если он ещё существует;
- сохранённые rendered transcripts;
- старый каталог markdown-заметок или memory-export, если он у вас вообще был.

Простыми словами это значит так:
- старые источники не выбрасываются;
- `Amai` забирает из них всё важное;
- после этого новый старт можно делать уже через `Amai`, а не через ручное склеивание нескольких файлов.

### Шаг 1. Импортировать старую continuity-линию

```bash
./scripts/import_continuity.sh \
  --project project_alpha \
  --display-name "Project Alpha" \
  --repo-root /path/to/project-alpha \
  --namespace continuity \
  --bootstrap-file /path/to/amai/state/continuity-imports/project-alpha/continuity-snapshot.md
```

Что делает эта команда:
- читает старые continuity-источники;
- складывает полный raw-content в artifact layer;
- делает безопасный searchable слой для retrieval;
- фиксирует import snapshot, чтобы потом новый старт не зависел от ручной сборки.
- `--active-workline-file` больше не обязателен:
  если write-side handoff уже живёт в `Amai`, import берёт headline и next-step оттуда, а bootstrap/transcript слой остаётся только refresh-evidence.
- при повторном импорте source of truth для freshness теперь не время записи snapshot в БД, а semantic время самого артефакта:
  `imported_at_epoch_ms` / `captured_at_epoch_ms`.
  Поздний replay старого import или handoff не должен маскироваться под свежую рабочую линию.

Если у вас есть ещё и отдельный старый каталог заметок, его можно добавить явно:

```bash
./scripts/import_continuity.sh \
  --project project_alpha \
  --display-name "Project Alpha" \
  --repo-root /path/to/project-alpha \
  --namespace continuity \
  --bootstrap-file /path/to/amai/state/continuity-imports/project-alpha/continuity-snapshot.md \
  --memory-dir /path/to/project-alpha/optional-legacy-notes
```

### Шаг 2. Запустить continuity-startup уже через `Amai`

```bash
./scripts/continuity_startup.sh \
  --project project_alpha \
  --namespace continuity
```

Что вы увидите:
- текущую активную линию;
- ближайший обязательный следующий шаг;
- сколько файлов памяти уже импортировано;
- какой последний rendered transcript считался источником continuity.

Простыми словами:
- старая память не потерялась;
- она стала доступна через новый контур `Amai`;
- следующий чат можно поднимать уже через `Amai continuity startup`.

## Если нужен дешёвый VPS

Если вы хотите не сильную локальную машину, а маленький удалённый сервер для:
- `remote MCP`
- smoke/demo
- pilot-install

используйте профиль `lite_vps`.

Простой путь:

```bash
./scripts/onboard_lite_vps.sh --client vscode
```

Важно понимать честно:
- `lite_vps` подходит для лёгкого удалённого режима;
- он не обещает рекордные benchmark-цифры;
- он не рассчитан на тяжёлый monitoring на том же слабом хосте;
- он не является профилем для больших real-project индексов.

## Если `Amai` живёт на Linux/VPS, а IDE у вас на Windows или macOS

Это нормальный сценарий.

В таком случае хороший короткий путь:

```bash
./scripts/onboard_remote_client.sh \
  --client vscode \
  --ssh-destination ops@example-host \
  --remote-repo-root /srv/amai
```

Что это значит:
- сам `Amai` живёт на сервере;
- базы живут рядом с ним;
- локально у вас только клиент;
- клиент запускает удалённый `Amai` через `ssh`;
- наружу не нужно выставлять `PostgreSQL`, `Qdrant`, `NATS` и `S3`.

## Способы развёртывания

Если не хотите гадать, какой deployment path вообще существует у `Amai`, используйте:

```bash
./scripts/deployment_targets.sh
```

Эта команда покажет канонические режимы простыми словами.

Сейчас логика такая:
- `local_docker`
  - главный режим прямо сейчас;
  - именно он считается обычным baseline для локальной машины или одного Linux-хоста;
- `remote_ssh`
  - уже готовый путь, когда `Amai` живёт на Linux/VPS, а клиент работает удалённо;
- `kubernetes_server`
  - не обязательный путь для обычного пользователя;
  - это следующий server/team deployment layer;
- `windows_vm_lab`
  - не “ещё один install path”;
  - это уже materialized validation contour для честной проверки Windows-пути через виртуальную машину;
  - текущий доказанный сценарий там не “поднять локальный Windows stack”, а fail-closed proof, что `install_amai.ps1` на локальной Windows-машине честно отказывает и требует `WSL2` или `--ssh-destination`.

Если хотите разобрать конкретный режим подробнее:

```bash
cargo run -- deployment explain --target kubernetes_server
```

Если хотите проверить, готова ли именно эта машина к конкретному режиму:

```bash
./scripts/deployment_preflight.sh --target windows_vm_lab
```

Если хотите прогнать живой Windows proof:

```bash
./scripts/proof_windows_vm_lab.sh --iso-path /path/to/windows.iso
```

## Два режима установки

У `Amai` сейчас есть два понятных профиля.

### `default`

Это основной режим для нормальной локальной машины.

Он подходит для:
- полного локального bootstrap;
- индексации реальных проектов;
- жёстких proof и benchmark-контуров;
- observability и monitoring.

### `lite_vps`

Это режим для дешёвого удалённого сервера.

Он подходит для:
- `remote MCP`;
- маленьких fixture-проектов;
- smoke и demo;
- лёгкого удалённого product path.

Главная разница простыми словами:
- `default` = полноценная рабочая база;
- `lite_vps` = дешёвый удалённый режим без обещания топовых цифр.

## Законы роста проекта

По мере роста `Amai` обязаны одновременно усиливаться четыре линии:
- скорость;
- точность;
- правдивость;
- безопасность и isolation.

Нельзя усиливать одну из них ценой тихой деградации другой.

Отсюда следуют постоянные engineering laws:
- `performance budget per contour`
  Любой новый слой обязан иметь явный budget по `latency`, `CPU/RAM` и при необходимости по
  token-cost, иначе проект станет умнее, но тяжелее и медленнее.
- `truth budget`
  Новый surface не имеет права расширять claims быстрее, чем появляются `measurement`, `proof`
  и живое runtime evidence.
- `safety gates before UX growth`
  Нельзя делать новый удобный UI/UX surface, если он усиливает ложную уверенность или маскирует
  неполный truth.
- `semantic compaction`
  Рост ledger/snapshot/event history нельзя лечить только бесконечным хранением; нужен controlled
  compaction без потери truth, replay correctness и audit anchors.
- `operability-first`
  Для каждого нового контура сразу обязательны: `metrics`, `proof`, `incident bundle`,
  `reconcile path`.
- `complexity guard`
  Если новый слой делает систему умнее, но хуже объяснимой, хуже проверяемой или тяжелее
  в эксплуатации, он считается дефектным.
- `regression pressure tracking`
  Нужно мерить не только новые успехи, но и цену роста: `latency drift`, `recovery drift`,
  `wrong-resume risk`, `proof coverage gaps`.
- `anti-copy through semantics, not ornament`
  Ядро преимущества `Amai` должно жить в `semantics`, `evidence`, `proofs`, `replay corpus`,
  `failure libraries`, а не в одном только внешнем UX.
- `claim expansion gate`
  Любой новый product claim сначала проходит через machine-readable contract, затем через proof,
  и только потом попадает в dashboard/export/marketing contour.
- `reference environments`
  Изменения нельзя считать честно materialized, пока они не держатся хотя бы на `main Linux host`,
  `Windows VM`, `ALT Linux laptop` и по возможности на одном более слабом профиле.

## Что `Amai` делает внутри

Внутри у `Amai` есть несколько важных слоёв.

### 1. Изоляция проектов

Это главный закон проекта:
- новый `repo_root` считается отдельным проектом;
- `repo_root` перед записью всегда canonicalize-ится до абсолютного пути без `.` и `..`;
- тот же canonical root используется и при самом `index project`, чтобы во внутренние `relative_path` не утекали ложные префиксы вида `/repo/../repo/...`;
- если тот же самый физический корень уже зарегистрирован под другим `project code`, новая alias-регистрация блокируется fail-closed, а не создаёт второй проект;
- смешивать проекты по умолчанию нельзя;
- чтение другого проекта разрешается только по явным relation/policy правилам.

### 2. Поиск идёт не одним способом

`Amai` не опирается только на embeddings.

Он ищет так:
1. exact/lexical поиск;
2. symbols и структура кода;
3. semantic поиск;
4. сборка готового `context pack` с указанием источника каждого фрагмента.

Для exact document lookup это теперь значит ещё и одну честную поблажку:
- если человек спрашивает имя файла без расширения, например `CHECKLIST_00_MASTER_ART_REGART`,
  `Amai` может вернуть `CHECKLIST_00_MASTER_ART_REGART.md`;
- но только как extensionless basename match внутри того же видимого контура, а не как свободную
  догадку по похожему имени из другого проекта.

### 3. Контекст даётся не всем куском проекта

Агент получает не “весь репозиторий в голову”, а:
- нужные документы;
- нужные символы;
- нужные куски кода;
- provenance каждого куска;
- measured token savings contour.

## К чему можно подключать

`Amai` умеет работать через `MCP`.

Это значит, что совместимый клиент может просить у него:
- список проектов;
- список namespaces;
- `context pack`;
- warmup cache;
- token benchmark;
- token report;
- observability snapshot.

`amai_observe_snapshot` теперь отдаёт наружу не только общий SLA-срез, но и
`observe_snapshot_summary.included_reasons_summary / excluded_reasons_summary`,
чтобы внешний клиент видел, почему последний рабочий контекст что-то включил
и почему часть слоёв ничего не добавила.

Тот же `observe snapshot` теперь ещё и публикует `compatibility`:
- `profile`
- `schema_version`
- `compatible`
- per-service reasons

А короткий MCP summary теперь явно показывает `compatibility=<profile>:ok|drift`.
Это нужно, чтобы зелёный health-snapshot был привязан к поддерживаемому stack profile,
а не выглядел как безконтекстная цифра.

`amai_token_report` теперь тоже отдаёт наружу готовый `token_report_summary`, где уже
собраны не только `scope_label`, `status`, `counted_events / events_count` и `note`,
но и честный `agent_cycle` lower-bound summary:
- `agent_cycle_scope_label`
- `agent_cycle_status`
- `agent_cycle_verified_saved_percent`
- `agent_cycle_verified_saved_tokens`
- `agent_cycle_note`

Это нужно затем, чтобы внешний клиент видел сразу две вещи:
- главный retrieval-aware KPI;
- и отдельную подтверждённую нижнюю границу полного агентного цикла,
  без притворства, будто `Amai` уже измерил весь бюджет клиента целиком.

Поверх этого у `amai_token_report` теперь есть ещё и compact contractual summary:
- `contractual_scope_label`
- `contractual_state`
- `contractual_coverage_state`
- `contractual_reconciliation_state`
- `contractual_margin_state`
- `contractual_blockers_summary`
- `contractual_statement_summary`

Это нужно затем, чтобы внешний клиент видел короткий customer-facing contractual слой
без чтения всего raw token report.

`amai_context_pack` теперь тоже отдаёт наружу `context_pack_summary` с
`included_reasons_summary / excluded_reasons_summary`, чтобы внешний клиент видел,
почему именно этот контекст был собран и почему часть слоёв ничего не добавила.

`amai_token_benchmark` теперь тоже отдаёт наружу `token_benchmark_summary`, где уже
собраны:
- `saved_tokens`
- `savings_factor`
- `savings_percent`
- `naive_tokens`
- `context_tokens`
- `files_considered`

Это делает сравнение `naive scope` против `Amai context pack` доступным без ручного
разбора всего benchmark-payload.

`amai_list_projects` и `amai_list_namespaces` теперь тоже не схлопывают discovery
до одного числа: короткий summary сразу показывает коды проектов и namespace/mode
preview, а не только raw count.

`amai_stack_preflight` теперь тоже доступен через MCP. Он отдаёт наружу
`preflight_report` и `preflight_summary`, чтобы внешний клиент machine-readable
способом видел:
- подходит ли эта машина для `default` или `lite_vps`;
- можно ли честно обещать пиковые benchmark-контуры;
- уместен ли remote mode;
- где не хватает минимума, а где просто нет запаса прочности.

`amai_benchmark_coverage` теперь тоже доступен через MCP. Он отдаёт наружу
`benchmark_coverage` и `benchmark_coverage_summary`, чтобы внешний клиент видел
не только список tools, но и сам machine-readable слой product-eval coverage:
- сколько benchmark-эталонов у `Amai` уже materialized;
- сколько покрыто частично;
- сколько пока только mapped;
- какие benchmark-приоритеты ещё висят следующими.

`amai_warm_cache` теперь тоже отдаёт наружу `warm_cache_summary`, где уже собраны
`compact_projects`, `cache_hits`, `exact_documents`, `symbol_hits`,
`lexical_chunks` и `semantic_chunks`. Это позволяет внешнему клиенту понять,
что именно реально прогрелось, а не видеть только финальное количество проектов.

`amai_memory_matrix` теперь тоже доступен через MCP. Он отдаёт наружу
`memory_task_matrix` и `memory_matrix_summary`, чтобы внешний клиент видел
не только raw benchmark-like payload, но и уже собранный product-eval verdict:
- сколько задач прошло;
- сколько упало;
- какой `success_rate`;
- какой `mean_score`;
- какой `p95_ms`;
- какие verdict-классы накопились по памяти и изоляции.

Кроме самих tools, `initialize` теперь отдаёт versioned `amai_protocol_manifest`.
В нём зафиксированы:
- версия MCP contract layer;
- базовые safety laws;
- startup contracts для нового или resumed чата;
- prompt contracts;
- per-tool `summary_field`, который внешний клиент может ожидать в structured output.

Отдельно `startup_contracts` теперь machine-readable фиксируют, что клиент
обязан делать до первого содержательного хода:
- вызвать `amai_continuity_startup`;
- использовать namespace `continuity` по умолчанию;
- не переходить к retrieval и новым действиям, пока не получен
  `continuity_startup_summary`;
- поднимать вместе с этим `execctl_resume_state` и `pending_return` obligations,
  а не только headline/next step.
- `execctl_active_lease` теперь тоже считается обязательной частью startup summary,
  а не необязательным бонусным полем.
- читать `resume_enforcement` из startup contract:
  - `execctl_resume_contract_summary` является каноническим полем resume-obligation;
  - `execctl_resume_obligation` даёт тот же контур уже как machine-readable object;
  - `required_return_task` поднимает сам return target отдельным object-слоем;
  - `startup_next_action` теперь даёт первое обязательное действие после startup;
  - `execctl_active_lease` даёт не только summary, но и machine-readable owner-state текущей линии;
  - `project_task_tree` и `project_task_ledger` теперь тоже поднимаются как objects, а не только
    как summary-строки;
  - если `startup_next_action.action_kind = resume_required_return_task`, клиент обязан
    выполнить именно этот return path до unrelated work;
  - если `execctl_active_lease.lease_owner_state = previous_session_owner`, клиент не имеет права
    тихо захватывать линию и обязан follow `startup_next_action` first;
  - `no_silent_drop = true` запрещает тихо переключаться на unrelated work.

Следующий practical contour теперь тоже materialized не только в docs, но и в onboarding:
- `VS Code` получает managed workspace startup instructions
  `.github/instructions/amai-continuity-startup.instructions.md`;
- `Cursor` получает managed project rule
  `.cursor/rules/amai-continuity-startup.mdc`;
- `Codex` теперь получает managed append-block в project `AGENTS.md`;
- `Claude Code` теперь получает managed append-block в project `CLAUDE.md`;
- все эти клиенты теперь получают и общий workspace-level machine-readable startup contract
  `.amai/onboarding/project-chat-startup-contract.json`;
  в нём теперь отдельно pinned `startup_contract_sha256`, чтобы client/runtime видел expected
  contract hash и мог fail-closed при drift;
- тот же JSON contract теперь несёт отдельный `artifact_enforcement` contour:
  - `workspace_contract_required_before_tool_call = true`;
  - `workspace_contract_relative_path = .amai/onboarding/project-chat-startup-contract.json`;
  - `missing_or_unreadable_fail_closed = true`;
  - `sha256_mismatch_fail_closed = true`;
  это значит, что supported client не имеет права читать только markdown/rule block и продолжать
  работу, если workspace contract artifact отсутствует, не читается или не совпадает по hash;
- `Claude Desktop` и `Generic` пока всё ещё получают только manual startup snippets.
- теперь этот contour можно проверить не только proof-скриптами, но и обычным
  `amai status`: строка `startup_artifacts: ...` показывает, жив ли managed startup artifact,
  совпадает ли workspace contract с текущим pinned hash, остались ли fail-closed поля на месте и
  не потерялись ли `startup_next_action / required_return_task / no_silent_drop` в managed startup block;
- тот же status теперь auditing-ит и contract-side return enforcement:
  не потерялись ли в JSON contract поля `startup_next_action`, `required_return_task`,
  `resume_required_return_task`, `previous_session_owner_must_follow_startup_next_action = true`,
  `no_silent_drop = true`.
- поверх этого contract теперь отдельно pin-ит и field-level semantics самого gate:
  - `startup_execution_gate.must_follow_startup_next_action = true`;
  - `startup_execution_gate.unrelated_work_allowed = false`;
  - `startup_execution_gate.must_read_prompt_text_before_reply = true`;
  - `startup_execution_gate.required_action_kind_when_resume_required = "resume_required_return_task"`;
  - `startup_execution_gate.no_silent_drop = true`;
  это нужно затем, чтобы supported client читал не только факт наличия gate, но и literal meaning
  каждого обязательного поля без prompt-guessing.
- truthful пример:
  если после `disconnect` managed startup block снят, `amai status` должен честно показать
  `startup_artifacts: missing_startup_instruction`, а не делать вид, что startup всё ещё готов.
- рядом status теперь печатает и repair path:
  - при drift/missing для известного клиента:
    `startup_artifacts_repair: rerun ./scripts/onboard_local.sh --client ... --yes`
  - без install state:
    `startup_artifacts_repair: run ./scripts/onboard_local.sh --client <client> --yes ...`
- отдельно startup теперь materialize-ит и dynamic runtime artifact
  `.amai/continuity/project-chat-startup-state.json`;
  это уже не static onboarding contract, а последняя реально поднятая `continuity_startup_summary`
  вместе с `chat_start_restore.prompt_text`;
- сам `startup contract` теперь machine-readable фиксирует и этот dynamic contour через
  `runtime_state_artifact`, чтобы managed startup instructions не теряли path к live
  `project-chat-startup-state.json`, его pinned `artifact_version` и не сваливались обратно к
  одному markdown-only restore;
- тот же runtime artifact теперь нужен затем, чтобы supported clients и operator tools видели не
  только static startup law, но и живой первый обязательный ход:
  - `startup_next_action`;
  - `required_return_task`;
  - `execctl_active_lease`;
  - `project_task_tree`;
  - `project_task_ledger`;
- поверх этих полей runtime artifact теперь отдельно materialize-ит `startup_execution_gate`,
  чтобы клиент видел immediate auto-return decision уже единым machine-readable object-слоем;
- тот же `startup_execution_gate` теперь обязателен и прямо в `continuity_startup_summary`,
  чтобы client runtime мог enforce-ить return уже по самому MCP startup output;
- `amai status` теперь auditing-ит и этот runtime artifact отдельной строкой
  `startup_runtime_state: ...`;
  truthful интерпретация там такая:
  - `ok` — последний startup уже materialized и machine-readable return contour на месте;
  - `not_materialized` — onboarding/contract есть, но startup в этом workspace ещё не был
    выполнен или runtime artifact снят;
  - `startup_runtime_state_drift` — artifact жив, но потерял contract-hash, prompt или обязательные
    machine-readable поля resume/return;
- этот runtime audit теперь обязан поднимать не только
  `must_follow_startup_next_action / unrelated_work_allowed`, но и:
  - `must_read_prompt_text_before_reply`;
  - `required_action_kind_when_resume_required`;
  - `no_silent_drop`;
  если любой из этих gate fields пропал, truthful статус должен стать
  `startup_runtime_state_drift`, а не `ok`.
- в самом runtime artifact теперь уже pinned и top-level поле
  `gate_semantics_consistent = true/false`;
  supported client обязан требовать именно `true`, а отсутствие поля или `false` трактовать как
  fail-closed runtime drift, а не как повод доверять `startup_execution_gate` по привычке.
- поверх наличия этих полей runtime audit теперь ещё и публикует
  `gate_semantics_consistent = true/false`;
  это bounded проверка, что live gate не противоречит startup summary и pinned contract semantics.
- рядом status теперь печатает и runtime repair path:
  `startup_runtime_state_repair: rerun cargo run -- continuity startup --repo-root ... --namespace continuity --json >/dev/null`
- тот же runtime artifact теперь можно inspect-ить и вне `Amai` repo-root через
  `cargo run -- continuity startup-state --repo-root /path/to/project --json`;
  это нужно затем, чтобы operator или внешний клиент мог не только проверить artifact audit, но и
  сразу получить `startup_execution_gate`, `required_return_task` и другие live return fields в
  конкретном workspace, а не только статический onboarding contract.

Это важно читать строго:
- `startup contract` уже общий и machine-readable;
- source of truth теперь не только managed markdown/rule block, но и JSON artifact
  `.amai/onboarding/project-chat-startup-contract.json`;
- но `auto-start readiness` у клиентов пока разная;
- bounded managed block не переписывает весь user rule file целиком: `Amai` теперь обновляет
  только свой собственный marker-bounded startup block и при disconnect/remove удаляет только его;
- truthful onboarding теперь явно печатает, где контур уже instruction-backed,
  а где ещё нужен manual follow-up.
- даже в manual-snippet клиентах startup теперь обязан surface-ить
  `required_return_task`, а не оставлять возврат только как human hint.
- для client automation теперь важнее читать `execctl_resume_obligation.resume_state`
  и `required_return_*`, чем парсить human summary строку.

Тот же manifest теперь фиксирует и `error_contracts`. Это даёт внешнему клиенту
стабильные machine-readable failure classes:
- `invalid_json_rpc_payload`
- `invalid_request`
- `method_not_found`
- `prompt_not_found`
- `invalid_params`
- `tool_not_found`
- `tool_execution_failed`

Для каждого такого класса теперь отдельно виден и `carrier`:
- `jsonrpc_error`
- `tool_is_error`
- `jsonrpc_error_or_tool_is_error`

Это нужно, чтобы клиент не угадывал, искать ли taxonomy в top-level JSON-RPC error
или внутри `tools/call -> structuredContent.error_taxonomy`.

Понятный walkthrough для подключения:
- [docs/MCP_INTEGRATION.md](docs/MCP_INTEGRATION.md)

## Как смотреть накопительную экономию токенов

Если нужен не один последний benchmark, а живая накопительная картина, используйте:

```bash
./scripts/token_report.sh
```

Если хотите отдельно смотреть 5-часовое окно Codex:

```bash
./scripts/token_report.sh --budget-profile codex_5h
```

Каноническая спецификация этого контура:
- [docs/TOKEN_LEDGER.md](docs/TOKEN_LEDGER.md)

Этот отчёт показывает:
- главный честный KPI:
  - `Проверенная реальная экономия`;
- отдельный слой `agent_cycle_economics`
  - это не “весь бюджет клиента”, а подтверждённая нижняя граница полного
    агентного цикла;
  - туда уже входят:
    - retrieval payload;
    - доуточнения, которые пришлось сделать после неполного ответа;
  - туда пока не входят:
    - токены исходного запроса клиента;
    - токены генерации итогового ответа;
    - tool-step и orchestration вне retrieval-контура;
    - continuity restore, если он прошёл вне token-ledger retrieval-событий;
- сколько токенов `Amai` сэкономил за текущую рабочую сессию;
- сколько токенов сэкономлено за текущее окно лимита;
- сколько токенов сэкономлено за всё время;
- насколько часто экономия осталась полезной без деградации:
  - `quality_ok_rate`;
- как часто retrieval пришлось чинить follow-up-ом или fallback:
  - `fallback_rate`;
- какая доля событий дошла до более строгого полезного ответа без лишнего доуточнения:
  - `answer_like_rate`;
- срезы по типам запросов:
  - где `Amai` экономит на `code_lookup`;
  - где на `docs_lookup`;
  - где на `symbol_lookup`;
- откуда пришли цифры:
  - живые `context pack` вызовы;
  - verification/benchmark события, если вы их явно включили.
- chart-ready cumulative timelines:
  - `all_live_timeline`
  - `verified_live_timeline`
  - они нужны для будущих live-графиков вида `без Amai / с Amai`,
    но уже сейчас штампуются как machine-readable data contract.
- отдельный `contract` слой
  - он явно публикует версии meter schema / baseline / quality / coverage /
    excluded taxonomy / billing policy;
  - это нужно затем, чтобы tokenonomics не “плыла” незаметно после смены
    формулы или semantics.
- отдельный `usage_event_schema`
  - там уже machine-readable видны dedup key, canonical event-time field,
    lifecycle status codes, backfill/correction policy;
  - это первый мост от measuring engine к будущему billing-grade contract.
- `baseline_contract`
  - отдельно опубликованы allowed/disallowed baseline classes и fairness note;
  - это защищает savings от нечестного раздутого baseline.
- `billing_policy`
  - видно, что сейчас реально billable отключён и весь слой остаётся `report_only`;
  - там же живут preliminary thresholds и правило `live + quality gate`;
  - отдельно зафиксированы truth-термины:
    - `savings floor`
    - `confirmed lower bound`
    - `retrieval savings floor`
    - `partial whole-agent-cycle lower bound`
- `suitability_contract`
  - отдельный слой пригодности цифры для разных surface-ов:
    - `operational_live`
    - `product_kpi`
    - `customer_review`
    - `contractual_export`
    - `billing_amount`
    - `compensation_pricing`
  - он отвечает не на вопрос, хорошая цифра или плохая, а на вопрос, где её можно
    использовать без подмены смысла.
- `rate_card`
  - честно показывает не только `currency_profile`, но и отдельный
    `rate_card_binding_model_version`;
  - truthful statuses теперь такие:
    - `not_configured`
    - `read_error`
    - `parse_error`
    - `bound_but_unpriced`
    - `priced_bound`
  - денежная конверсия включается только в состоянии `priced_bound`, а не просто
    по одному факту, что где-то в config уже написан `unpriced-v1`.
- `settlement_contract`
  - публикует statement-version, freeze/close policy, late-arrival policy,
  correction/dispute policy;
  - теперь ещё явно публикует `settlement_lifecycle_model_version`,
    `current_operational_state` и `current_contractual_state`;
  - freeze/close semantics теперь versioned до `v2`: scope можно честно маркировать как
    `provisionally stable` или `provisional hold`, но это по-прежнему только
    `report_only preview`, а не денежный close.
- `metering_freshness_contract`
  - отдельно публикует ingest warning/SLO и `late_arrival_grace_minutes`;
  - это нужно затем, чтобы freshness и lag semantics не были неявной магией в UI.
- `telemetry_surfaces`
  - явно разводит `operational live telemetry` и `contractual tokenonomics`;
  - это нужно затем, чтобы dashboard headline и live rollups не принимали за invoice.
- `reconciliation_contract`
  - публикует, какие внешние truth-sources нужны для будущей сверки с provider;
  - теперь делает это не только общим списком, а по слоям:
    - `required_sources_for_usage_truth`
    - `required_sources_for_cost_truth`
    - `optional_sources_for_invoice_evidence`
    - `unready_*`, если source ещё не bound или bound недостаточно честно для этого слоя;
  - честно показывает, что сейчас они ещё не привязаны к runtime.
- `reconciliation_previews`
  - по каждому scope показывают внутренний measured lower bound;
  - но внешние provider usage / cost / drift остаются `null`, пока нет настоящей external truth binding.
- `margin_contract`
  - показывает, включена ли вообще money-margin арифметика;
  - честно фиксирует отсутствие priced rate card и infra cost profile;
  - теперь ещё отдельно публикует `required_sources_for_margin_truth` и
    `unready_required_sources_for_margin_truth`;
  - отдельно раскладывает:
    - `customer_savings_money_truth_completeness_state`
    - `amai_cost_truth_completeness_state`
    - `margin_truth_completeness_state`
    чтобы было видно, готова ли денежная нижняя граница savings, готова ли оценка
    собственного infra cost и готов ли их общий margin preview.
- `margin_view`
  - по каждому scope показывает token-side lower bound savings клиента;
  - но деньги и маржа остаются `null`, пока это нельзя доказать.
  - `margin_contract.rate_card_status` теперь должен совпадать с truthful runtime
    rate-card binding, а не жить отдельной догадкой.
- `contractual_evidence_pack`
  - по каждому scope можно выгрузить report-only evidence pack;
  - туда входят `settlement_report_preview`, `statement_preview`,
    `reconciliation_preview`, `margin_scope`, hashes включённых/исключённых line items
    и сами line items без сырого текста запроса;
  - это нужно затем, чтобы customer-facing audit/export не подменялся dashboard-экраном;
  - но это по-прежнему не invoice и не готовый settlement.
- `statement_export_previews`
  - по каждому scope теперь есть стабильный export-preview слой;
  - он публикует:
    - `statement_preview_id`
    - `settlement_report_preview`
    - `included_events_hash`
    - `excluded_events_hash`
    - `provisional_close_state`
    - `provisional_close_candidate`
    - `credit_action_state`
    - `dispute_action_state`
    - `evidence_pack_command`
  - это нужно затем, чтобы customer review/export начинался с компактного preview,
    а не сразу с full evidence pack.
  - export preview теперь ещё несёт `suitability`, чтобы review/export и будущие
    money-facing surface-ы не путались между собой.
  - statement preview теперь ещё несёт `client_limit_meter_alignment`;
  - в нём отдельно фиксируются:
    - `alignment_state`
    - `same_meter_as_client_limit`
    - `live_events_count / non_live_events_count`
    - `measured_components / partially_measured_components / missing_components`
    - `component_event_coverage`
    - `blocking_reasons`
    - `baseline_equivalence`
  Это нужно затем, чтобы lower-bound savings не выглядели как уже эквивалентные
  тому же самому метру, которым клиент считает живой лимит `5h`.
  - тот же слой теперь ещё честно различает:
    - `partial_lower_bound_not_meter_equivalent`
    - `whole_cycle_partially_observed_not_meter_equivalent`
    - `whole_cycle_observed_baseline_partial`
  Это нужно затем, чтобы по мере materialize-инга observed whole-cycle fields было видно,
  что покрытие уже стало шире, но baseline всё ещё не разрешено выдавать за full same-meter.
  - начиная с этого шага тот же contour ещё и публикует versioned
    `baseline_equivalence`, где machine-readable фиксируются:
    - `state`
    - `remaining_gap_reason`
    - `applicable_components / fully_observed_components / incomplete_components`
    - `measured_baseline_components / explicitly_unmodeled_baseline_components / missing_baseline_components`
    - `measured_baseline_tokens_lower_bound`
    - `whole_cycle_components_fully_observed`
  Это нужно затем, чтобы `same_meter_baseline_unmeasured /
  same_meter_baseline_partially_measured / same_meter_baseline_explicit_boundary`
  перестали жить только как строки-blockers и стали
  отдельным truthful/measured object про незавершённый baseline-equivalent слой.
  - dashboard token-cards теперь поднимают этот же слой в user-facing surface:
    - добавляют строку `Связь с лимитом клиента`;
    - прямо показывают, когда текущий scope содержит только `non-live` активность;
    - для `whole_cycle_observed_baseline_partial` tooltip/note теперь ещё поднимают
      `baseline_equivalence.fully_observed_components /
      measured_baseline_components / explicitly_unmodeled_baseline_components /
      missing_baseline_components`, чтобы operator видел, какие applicable whole-cycle
      компоненты уже дотянуты, по каким baseline-equivalent semantics уже materialized,
      а какой remaining gap является не просто missing implementation, а explicit
      truth-boundary без guessed baseline;
    - и отдельно объясняют, что even confirmed lower bound всё ещё не обязан
      двигаться вместе с внешней шкалой клиентского лимита.
  - preview, settlement report preview и evidence pack теперь ещё несут общий
    `customer_contractual_boundary`;
  - в нём отдельно фиксируются:
    - `review_surface_state`
    - `review_surface_blocking_reasons`
    - `future_settlement_activation_state`
    - `future_settlement_activation_blocking_reasons`
  Это нужно затем, чтобы customer review surface не выдавал свою готовность за готовность
  будущего settlement activation.
  - те же surface-ы теперь ещё несут отдельный `settlement_activation_governance`;
  - в нём отдельно фиксируются:
    - `governance_state`
    - `next_settlement_stage_candidate / blockers`
    - `provisional_close_barriers / billing_close_barriers / close_barriers`
    - `registry_status / adjustment_status`
    - `credit_action_state / dispute_action_state`
  Это нужно затем, чтобы future settlement activation не выглядел как одна абстрактная
  blocked-ready метка, а показывал конкретные governance-барьеры и adjustment semantics.
  - те же surface-ы теперь ещё несут отдельный `adjustment_activation_governance`;
  - в нём отдельно фиксируются:
    - `future_adjustment_activation_state / blocking_reasons`
    - `request_schema_version / registry_version`
    - `correction_action_state / credit_action_state / dispute_action_state`
  Это нужно затем, чтобы future adjustment path не прятался внутри raw adjustment preview,
  а показывал отдельный report-only governance слой.
- `settlement_report_previews`
  - по каждому scope теперь есть отдельный review-grade settlement object;
  - он собирает в одном месте:
    - period anchors
    - `included_events_hash / excluded_events_hash`
    - policy versions
    - truth/completeness states
    - adjustment summary
  - это нужно затем, чтобы audit/export держались на одном стабильном объекте,
    а не на разрозненных полях из нескольких поверхностей.
- first-class `coverage`
  - у каждого rollup теперь есть не только savings, но и честный охват:
    `measured / included / excluded`;
  - без него savings нельзя подавать как полный охват сессии.
- `excluded_breakdown`
  - видно, какие измеренные события не попали в главный verified итог и почему.
- reporting layers у `agent_cycle_economics`
  - `billable`
  - `measured_non_billable`
  - `unmeasured`
  - в текущем runtime это всё ещё `report_only`, а не готовый billing.

## Token Terminal Interaction Contract

Для будущего expanded token terminal утверждён такой desktop interaction contract:

- по двойному клику левой кнопкой по лейблу token-card карточка переворачивается по
  горизонтали вокруг своей оси и показывает обратную сторону с графиком;
- повторный двойной клик левой кнопкой возвращает карточку в исходное положение;
- по двойному клику правой кнопкой по любой карточке, в любом состоянии
  (`front/back`), карточка разворачивается на весь экран;
- повторный двойной клик правой кнопкой возвращает карточку из full-screen в исходное
  положение;
- mobile path использует прямое touch-отображение этого же контракта:
  - двойной тап одним пальцем = левая кнопка = flip front/back;
  - двойной тап двумя пальцами = правая кнопка = full-screen toggle.

Жёсткие ограничения для этого terminal view:

- expanded side не имеет права показывать больше truth, чем уже materialized backend;
- default main chart должен показывать только две primary verified lower-bound линии:
  `Без Amai` и `С Amai`;
- overlays и analyzers не могут подменять main truth line;
- карточка не имеет права намекать на `full session economics`, пока backend честно не
  materialize-ил такой слой.

Главное правило:
- headline-метрика у `Amai` теперь не raw и не synthetic;
- по умолчанию это `live-only`, `quality-gated` и `recovery-aware` показатель;
- каноническое имя этой метрики:
  - `Verified Effective Savings %`
  - по-русски: `Проверенная реальная экономия`.
- рядом с любой крупной savings-цифрой теперь нужно смотреть и `coverage`;
- `agent_cycle_economics` по-прежнему нельзя трактовать как полностью измеренную
  экономику всей сессии.
- event-level `usage_state` тоже теперь важен:
  - `verified_included`
  - `excluded_*`
  - `legacy_ingest / reverified_backfill / live_ingest`
  Он нужен затем, чтобы savings не теряла связь с event lifecycle.
- baseline fairness теперь тоже first-class:
  - `baseline_strategy_slices` показывают, какой baseline реально использовался и
    как по нему выглядит verified/coverage contour.
- settlement preview теперь тоже first-class:
  - `statement_previews.current_session / rolling_window / lifetime`
  - там уже видно measured non-billable lower bound по scope;
  - там теперь ещё явно видны `lifecycle_state`, `contractual_state`, `settlement_stage`,
    `next_settlement_stage_candidate` и `close_barriers`;
  - там теперь ещё есть `transactional_statuses`, чтобы customer-facing export видел отдельно
    `measured / review / billable / settled / invoiced / credited / disputed / closed`
    и не путал materialized report-only стадии с будущими reserved billing стадиями;
  - statement export и evidence pack теперь ещё несут `export_semantics`, чтобы self-serve
    customer review не смешивался с operational telemetry и не выглядел как invoice-grade слой;
  - там теперь ещё живёт и `freshness`, чтобы было видно ingest health и окно поздних событий;
  - там теперь ещё есть `period` с `period_start / period_end / window_anchor` и
    `adjustment_preview` для будущих credit/correction semantics;
  - top-level `settlement_contract` теперь отдельно публикует `current_materialized_boundary`,
    `materialized_settlement_stages` и `future_reserved_settlement_stages`, чтобы report-only
    lifecycle не выглядел как уже готовый billable workflow;
  - но `billable_lower_bound_tokens` и `final_amount` остаются пустыми, пока billing не
    включён честно.
- correction/dispute слой теперь тоже versioned:
  - `adjustment_request_schema` публикует allowed kinds/statuses и запрет на тихую
    ретро-перезапись прошлого period;
  - `adjustment_registry` публикует status источника и per-scope counts/hashes;
  - по умолчанию registry ищется в repo-local
    `state/token_adjustment_registry.json`;
  - если env-binding не задан и repo-local файл ещё не materialized, этот слой честно
    остаётся `default_path_missing`, а не притворяется уже живым credit workflow;
  - для operator-safe report-only entries теперь есть явные команды:
    - `amai observe token-adjustment-registry --scope lifetime`
    - `amai observe token-adjustment-add --scope lifetime --kind adjustment_entry --status pending_review --reason-code ...`
    - если correction должен быть привязан к текущему statement preview, теперь можно
      не вытаскивать id вручную:
      - `amai observe token-adjustment-add --scope lifetime --status pending_review --reason-code ... --resolve-related-statement-id`
  - прошлые periods по-прежнему нельзя тихо переписывать задним числом: adjustments живут
    отдельными entries со статусами `pending_review / applied_report_only / disputed / rejected`.
- provider reconciliation теперь тоже first-class:
  - `reconciliation_previews.current_session / rolling_window / lifetime`
  - repo-local truth sources теперь тоже поддерживаются по умолчанию:
    - `state/provider_usage_export.json`
    - `state/provider_invoice_export.json`
    - `state/provider_rate_card.json`
    - `state/infra_cost_profile.json`
  - если env-binding не задан, но repo-local файл уже есть, bind идёт по нему честно;
  - если файла ещё нет, статус источника теперь честно `default_path_missing`, а не
    расплывчатое `not_configured`;
  - там теперь отдельно видны:
    - `internal_delivered_tokens`
    - `internal_recovery_tokens`
    - `internal_observed_whole_cycle_lower_bound_tokens`
    - `internal_provider_billed_tokens`
  - `internal_provider_billed_tokens` теперь больше не равен только `delivered + recovery`:
    он поднимается до всей честно наблюдаемой `whole-cycle lower bound`, то есть включает
    retrieval payload, recovery и те observed whole-cycle компоненты, которые уже
    materialized без гадания;
  - drift по токенам теперь считается только как
    `internal_provider_billed_tokens - external_provider_usage_tokens`;
  - это сделано специально, чтобы не сравнивать provider usage с saved tokens;
  - если provider usage/invoice export и rate-card честно подключены, там уже могут
    появляться:
    - `external_provider_usage_tokens`
    - `external_provider_cost_amount`
    - `external_invoice_amount`
    - `drift_tokens`
    - `internal_provider_cost_estimate_amount`
    - `drift_amount`
    - `invoice_drift_amount`
  - `reconciliation_state` теперь честно различает:
    - `external_usage_aligned_report_only`
    - `external_usage_drift_report_only`
    - `external_usage_and_invoice_aligned_report_only`
    - `external_usage_and_invoice_drift_report_only`
  - отдельно materialized governance-layer:
    - `usage_truth_completeness_state`
    - `provider_cost_truth_completeness_state`
    - `invoice_evidence_completeness_state`
    - `money_truth_completeness_state`
    - `reconciliation_readiness_state`
    - `governance_blocking_reasons`
  - `money_truth_completeness_state` теперь остаётся как агрегированный итог,
    а не как единственная широкая ось:
    сначала отдельно видны `provider cost truth` и `invoice evidence`,
    а уже потом их общий `money truth`;
  - отдельно materialized temporal truth layer:
    - `provider_usage_scope_alignment_state`
    - `provider_invoice_scope_alignment_state`
    - `rate_card_scope_alignment_state`
    - `reconciliation_temporal_truth_state`
  - отдельно materialized provider-identity truth layer:
    - `rate_card_provider_alignment_state`
    - `invoice_provider_alignment_state`
    - `provider_identity_state`
  - это нужно затем, чтобы provider usage и priced rate card не выглядели честно
    применимыми к текущему statement period без отдельной проверки по времени;
  - это нужно затем, чтобы rate card и invoice export не выглядели честно применимыми
    к тому же provider truth, если они вообще смотрят на другого provider;
  - это нужно затем, чтобы не смешивать в одну строку:
    - отсутствие usage truth
    - отсутствие money truth
    - и уже реально найденный drift
  - для operator/debug review теперь есть отдельная команда:
    - `amai observe token-contractual-sources --scope lifetime`
  - она печатает source bindings, reconciliation preview, margin scope и statement export
    preview в одном inspect-layer, без парсинга полного token report.
  - поверх этого теперь есть и отдельный export-bundle:
    - `amai observe token-statement-export --scope lifetime --output-dir /tmp/amai-token-statement`
  - он materialize-ит в один каталог:
    - `settlement_report_preview.json`
    - `manifest.json`
    - `statement_export_preview.json`
    - `contractual_evidence_pack.json`
    - `token_contractual_sources.json`
  - `statement_export_preview.json` и `contractual_evidence_pack.json` теперь ещё отдельно несут
    `external_truth_manifest` с fingerprint внешних truth sources:
    - `provider_usage_export`
    - `provider_invoice_export`
    - `provider_rate_card`
    - `infra_cost_profile`
    - `token_adjustment_registry`
  - это нужно затем, чтобы customer-facing review видел не только current status, но и
    machine-readable evidence того, какие именно source files реально были привязаны:
    - `resolved_path`
    - `source_bytes`
    - `source_sha256`
    - `source_last_modified_epoch_ms`
    - `bound_version`
  - это уже customer-facing review surface с hashes и contract states, но всё ещё строго
    `report_only`, а не invoice.
  - export/evidence surface versions теперь подняты до:
    - `contractual-statement-export-v19`
    - `settlement-report-preview-v10`
    - `contractual-evidence-pack-v19`
  потому что customer-facing payload теперь уже явно различает:
    - `customer review ready`
    - `internal money arithmetic ready`
    - `contractual settlement ready`
    - `future settlement activation governance`
    - `future adjustment activation governance`
  - начиная с этого шага `statement_preview` и все customer-facing export/evidence surface-ы
    тоже поднимают turn-scoped `assistant_generation`, если оно materialized через
    direct turn attach или rollout turn timeline и не может быть честно разложено
    по каждому retrieval event без дублирования токенов.
  - operational metering contract теперь ещё несёт:
  - `client_limit_meter_alignment_version = client-limit-meter-alignment-v7`
  - `client_limit_baseline_equivalence_version = client-limit-baseline-equivalence-v3`
  - это отдельный truth-layer, который прямо объясняет, почему высокая measured
    lower bound ещё не обязана означать такое же падение клиентской шкалы `5h`.
  - начиная с `v7/v3` слой ещё и честно поднимает `baseline_equivalence` как отдельный
    machine-readable contour; теперь он умеет различать не только
    `baseline_semantics_unmaterialized`, `baseline_component_semantics_partial`, но и
    `baseline_component_semantics_explicit_boundary`, когда часть remaining gap должна
    остаться явной truth-boundary без guessed pre-Amai baseline, например для
    `continuity_restore_outside_retrieval`.
  - и тот же `v7` продолжает честно поднимать `client_prompt` как observed component
    из уже записанных `query + tokenizer`, даже если старое событие не несло
    отдельный `whole_cycle_observed.client_prompt_tokens`.
  - тот же `v7` теперь ещё публикует
    `assistant_generation_observation_source`, чтобы `current_session /
    rolling_window` можно было объяснить не только общей missing-меткой, но и
    machine-readable source-gap причиной:
    `assistant_generation_source_unavailable /
    assistant_generation_source_no_scope_overlap /
    assistant_generation_source_partial_scope_overlap /
    assistant_generation_source_covers_missing_scope`.
  - этот же `v7` уже различает не только rollout path, но и direct turn attach:
    `source_kind` теперь может честно быть
    `direct_turn_attach_v1 / codex_rollout_turn_timeline_v1 /
    direct_turn_attach_plus_rollout_turn_timeline_v1`.
  - rollout matcher теперь ещё и жёстче режет ложные overlap:
    mention `context pack` внутри heredoc, handoff note или другого shell-text больше
    не считается approved context-pack call сам по себе; учитываются только реальные
    command invocation path (`mcp__amai__amai_context_pack`, `cargo run ... context pack`,
    `./target/release/amai context pack`, `$AMAI context pack`, `memory search`).
  - и тот же `v7` больше не делит все whole-cycle компоненты на один и тот же
    denominator:
    `client_prompt`, `assistant_generation`, `tool_overhead_outside_retrieval` и
    `continuity_restore_outside_retrieval` теперь публикуются с
    `target_live_events_count`, `target_scope_kind` и
    `not_applicable_components`, чтобы `continuity_restore` не считался missing
    там, где в scope вообще не было restore-event.
  - runtime same-meter path теперь умеет и прямой turn-scoped attach:
    `observe token-whole-cycle-turn-attach --thread-id ... --turn-id ...
    --context-pack-id ... --assistant-generation-tokens ...`.
    Этот путь считает `assistant_generation` один раз на turn-group и не
    дублирует токены по каждому retrieval event.
  - тот же contour теперь открыт и через MCP:
    `amai_observe_whole_cycle_turn`.
    Там `thread_id` можно не передавать, если все `context_pack_ids` принадлежат
    одному thread в `working_state`; при неоднозначности inference fail-closed.
  - statement/reconciliation path теперь тоже перестал быть retrieval-only:
    внутренний meter lower bound для provider drift/cost preview поднимается от
    `delivered + recovery` к `observed whole-cycle lower bound`, как только такие
    компоненты действительно materialized в ledger.
  - runtime path тоже открыт честно: `ContextPackArgs`, прямой CLI `context pack`,
    MCP `amai_context_pack` / `amai_token_benchmark` и compatibility `memory search`
    теперь могут нести observed overrides для
    `client_prompt_tokens`,
    `assistant_generation_tokens`,
    `tool_overhead_tokens`,
    `continuity_restore_tokens`,
    чтобы upstream client мог передавать whole-cycle evidence прямо в ledger, а не через
    задний repair path.
  - MCP `amai_context_pack` теперь ещё materialize-ит и свой собственный
    `tool_overhead_outside_retrieval` path:
    после построения context pack tool result он связывает ответ с тем же
    `context_pack_id` и дописывает в это же usage event observed `tool_overhead_tokens`
    только по MCP summary/stats payload, а не по полному retrieval payload;
  - это нужно затем, чтобы tool result envelope не оставался полностью тёмным same-meter
    компонентом и при этом не происходило двойного счёта retrieval tokens;
  - тот же MCP front door теперь принимает и `token_source_kind`, чтобы proof/verify
    вызовы можно было уводить в `proof_*` / `verify_*`, а не contaminate live lane;
  - CLI `context pack` front door теперь тоже автоматически materialize-ит свой
    `tool_overhead_outside_retrieval` path:
    после записи token-budget event он берёт реально сериализованный stdout JSON payload,
    вычисляет observed CLI output overhead и дописывает его в то же usage event по
    `context_pack_id`;
  - для этого path есть отдельный proof:
    `scripts/proof_token_cli_tool_overhead.sh`;
  - report path теперь умеет не только ждать будущие новые CLI/MCP события:
    если в текущем `current_session / rolling_window` есть live retrieval events без
    `tool_overhead_tokens`, но corresponding `context_pack` уже сохранён в registry,
    `token-report` сам честно достаёт stored payload по `context_pack_id`,
    пересчитывает CLI-equivalent output overhead и дописывает его в missing scope;
  - для этого repair-free auto-sync есть отдельный proof:
    `scripts/proof_token_report_tool_overhead_autosync.sh`;
  - отдельный post-call attach path теперь materialized и для `assistant_generation`:
    после того как upstream client уже узнал реальные output tokens своего ответа,
    он может привязать их к тому же `context_pack_id` через:
    - CLI `amai observe token-whole-cycle-attach --context-pack-id ... --assistant-generation-tokens ...`
    - MCP tool `amai_observe_whole_cycle`
  - этот path сделан именно post-call, потому что `assistant_generation` становится известен
    только после ответа клиента, а не в момент build-time у `amai_context_pack`;
  - conflicting overwrite тут fail-closed:
    новое число нельзя тихо переписать поверх уже наблюдённого другого значения;
    разрешён только первый attach или повтор того же самого значения;
  - дополнительно materialized rollout-backed observation path:
    если у upstream runtime уже есть raw Codex rollout JSONL с `token_count`,
    `Amai` умеет взять оттуда unambiguous candidate по `turn_id + context_pack_id`
    и применить observed `assistant_generation_tokens` через:
    - CLI `amai observe token-rollout-assistant-generation --rollout-path ... --repo-root ... --apply`
    - proof `scripts/proof_token_rollout_assistant_generation.sh`
  - этот path тоже fail-closed:
    candidate принимается только если в выбранном turn есть ровно один
    `context_pack_id` и ненулевой `assistant_generation_tokens`;
    ambiguous rollout не имеет права silently guess-ить attribution;
  - verify/MCP contour теперь отдельно прогоняет и turn-scoped front door:
    `verify mcp` обязан уметь вызвать `amai_observe_whole_cycle_turn` и получить
    `assistant_generation_turn_observed_attach`, а не проверять только single-event attach.
  - поверх ручного `--apply` report path теперь сам пытается сделать scoped auto-sync:
    он берёт не один arbitrary latest candidate, а все unambiguous rollout observations,
    фильтрует их по тем live retrieval events, где `assistant_generation` ещё отсутствует,
    и только потом дописывает observed value в ledger;
  - дополнительно same-meter path теперь умеет materialize-ить и turn-scoped observation:
    он берёт `working_state_event` для live `retrieval_context_pack`, поднимает оттуда
    `thread_id + captured_at_epoch_ms`, затем ищет rollout turn-timeline, в который этот
    retrieval попадает по времени, и считает `assistant_generation` один раз на весь
    matched turn-group, а не дублирует одни и те же output tokens на каждый `context_pack_id`;
  - это нужно затем, чтобы длинный один ответ клиента, внутри которого было несколько
    retrieval context packs, перестал выглядеть как полный source-gap только потому, что
    turn уже не был unambiguous по одному `context_pack_id`;
  - это важно затем, чтобы same-meter path не зависел только от ручного attach и не
    поднимал lifetime-only evidence, когда для текущего report scope есть более точное
    совпадение;
  - truthful ограничение остаётся жёстким:
    если usable rollout `context_pack_id` не пересекаются с `current_session /
    rolling_window` correlation set, `assistant_generation` в этих scope честно остаётся
    unmeasured, даже если lifetime уже начал получать rollout-backed observed values;
  - теперь этот gap не прячется внутри общей blocker-строки:
    `client_limit_meter_alignment` отдельно публикует
    `assistant_generation_observation_source`, чтобы оператор видел, есть ли
    rollout source вообще и покрывает ли он missing live scope;
  - `continuity startup` тоже начал материализовать self-observed component:
    он может записывать `continuity_restore_tokens` от собственного `CHAT_START_RESTORE`
    prompt-text, а engineering/proof вызовы обязаны уводить это в `proof_/verify_`
    source kind через `--token-source-kind`.
- metering freshness теперь тоже first-class:
  - `metering_freshness.current_session / rolling_window / lifetime`
  - она отдельно показывает:
    - `metering_ingest_state`
    - `contractual_lag_state`
    - `contractual_freshness_state`
    - `latest_event_age_ms`
    - `latest_ingest_lag_ms`
    - `p95_ingest_lag_ms`
  - это важно затем, чтобы customer-facing preview честно различал:
    - pipeline lag;
    - и просто ещё открытое окно поздних событий.
- contractual summary теперь тоже несёт freshness semantics:
  - `contractual_statement_summaries.*`
  - там теперь видны `metering_ingest_state`, `contractual_lag_state` и
    `contractual_freshness_state`;
  - там теперь ещё отдельно materialized readiness-axis:
    - `internal_money_arithmetic_readiness_state`
    - `internal_money_arithmetic_blocking_reasons`
    - `contractual_settlement_readiness_state`
    - `contractual_settlement_blocking_reasons`
  - это нужно затем, чтобы customer-facing export честно различал:
    - можно ли уже посчитать внутреннюю money-arithmetic preview;
    - и можно ли вообще говорить о более строгой settlement readiness;
  - там теперь ещё отдельно видны pricing source поля:
    - `rate_card_status`
    - `rate_card_truth_completeness_state`
    - `rate_card_version`
    - `rate_card_provider`
    - `rate_card_currency_profile`
    - `provider_usage_provider`
    - `provider_invoice_provider`
    - `pricing_truth_completeness_state`
    - `margin_readiness_state`
    - `margin_blocking_reasons`
  - blocking reasons теперь собираются поверх `statement + reconciliation + margin + freshness`,
    а не только из одного reconciliation слоя.
- margin view теперь тоже first-class:
  - `margin_view.current_session / rolling_window / lifetime`
  - там уже видно `customer_saved_tokens_lower_bound`;
  - при честно привязанных `provider usage + rate card + infra cost profile`
    `margin_view` теперь уже имеет право materialize-ить:
    - `customer_saved_amount_lower_bound`
    - `amai_infra_cost_amount`
    - `margin_amount`
    - `savings_to_cost_ratio`
  - отдельно теперь видны:
    - `rate_card_truth_completeness_state`
    - `infra_cost_truth_completeness_state`
    - `pricing_truth_completeness_state`
    - `margin_confidence_state`
    - `margin_readiness_state`
    - `rate_card_scope_alignment_state`
    - `infra_cost_scope_alignment_state`
    - `margin_temporal_truth_state`
    - `provider_identity_state`
  - если priced inputs есть, но их период не покрывает statement scope, margin preview
    теперь честно уходит в `pricing_period_mismatch`, а не делает вид, что money truth уже
    готова;
  - если provider usage и priced rate card смотрят на разные provider identities, margin preview
    обязан уходить в `provider_identity_mismatch`, а не притворяться честным money-preview;
  - если provider usage показывает drift, margin preview не прячется и не
    “зеленеет”, а прямо уходит в `priced_preview_with_provider_drift`.

По умолчанию proof/benchmark-трафик не смешивается с обычной рабочей активностью.
Если нужно показать всё вместе, используйте:

```bash
./scripts/token_report.sh --include-verify-events true
```

Если вы запускаете прямой `context pack` не как пользовательский live-запрос, а как proof/verify
контур, обязательно задавайте source kind явно:

```bash
cargo run -- context pack \
  --project project_alpha \
  --namespace review \
  --query "shared_runtime_marker" \
  --retrieval-mode local_plus_related \
  --token-source-kind proof_context_pack
```

Это нужно затем, чтобы engineering-run не записывался как `live_context_pack` и не портил
карточки живой tokenonomics текущей сессии. Старые события, уже записанные как `live` до этого
разделения, сами не исчезают: для них нужен новый session window или явный repair/reverify path.

Если у вас есть старые исторические `token_budget_event`, записанные ещё старым форматом, `Amai` умеет подтянуть их до нового качества без ручного SQL:

```bash
cargo run --release -- observe repair-token-ledger --apply
cargo run --release -- observe reverify-token-ledger --apply
```

Если contamination уже не legacy, а просто был честно, но ошибочно записан в `live`-lane,
используйте explicit reclassification path:

```bash
cargo run --release -- observe repair-token-ledger \
  --apply \
  --project-prefix memory_eval \
  --namespace continuity \
  --source-kind live_context_pack \
  --rewrite-source-kind verify_memory_matrix_context_pack \
  --repair-reason operator_memory_eval_cleanup
```

Если нужен report-only contractual export по одному scope, используйте:

```bash
cargo run --release -- observe token-evidence-pack --scope lifetime
cargo run --release -- observe token-evidence-pack --scope rolling_window --output /tmp/amai-token-evidence-pack.json
```

Для живого proof, что contractual tokenonomics уже умеет пройти путь до
денежного preview при честно привязанных external inputs, используйте:

```bash
./scripts/proof_token_contractual_pricing.sh
./scripts/proof_token_suitability.sh
```

Что важно по правде:
- pack redacts raw `query` и оставляет только `query_hash`, scope, usage-state и token arithmetic;
- `included_events_hash` и `excluded_events_hash` дают audit-friendly proof состава;
- это contractual evidence pack для review/export, а не invoice и не денежный settlement.

Что делают эти команды:
- `repair-token-ledger`
  - достраивает недостающие поля старого формата;
  - либо по явным selector-ам переводит уже записанные события в другой `source_kind`,
    если contamination попал не в тот telemetry lane;
- `reverify-token-ledger`
  - заново прогоняет старые live-запросы через текущий retrieval contour;
  - поднимает их из `legacy_unverified` в quality-gated live-выборку, если retrieval реально проходит.

Для synthetic/debug/temporary observability snapshot теперь есть и отдельный retention path:

```bash
cargo run --release -- observe cleanup-snapshots --limit 200
cargo run --release -- observe cleanup-snapshots --apply --limit 2000
```

Что это значит:
- команда честно показывает, сколько aged snapshot реально попало под TTL;
- удаляет только те записи, которые policy разрешает удалять;
- benchmark history теперь штампуется как `immutable_snapshot`, поэтому benchmark snapshot больше нельзя незаметно переписать update-ом;
- source of truth для `schema_version`, `classification_rules_version` и `retention_profile` живёт в [config/observability.toml](/home/art/agent-memory-index/config/observability.toml) и materialize-ится в `_observability` каждого snapshot.

Для rebuildable локального мусора теперь есть отдельный cleanup path с запасом по времени:

```bash
cargo run --release -- observe cleanup-artifacts --limit 20
cargo run --release -- observe cleanup-artifacts --apply --limit 20
cargo run --release -- observe cleanup-artifacts --aggressive --limit 20
cargo run --release -- observe cleanup-artifacts --aggressive --apply
```

Что это значит:
- cleanup path читает policy из [config/observability.toml](/home/art/agent-memory-index/config/observability.toml), а не из вшитого списка;
- под auto-retention сейчас попадают только rebuildable хвосты:
  - `target/debug`
  - `target/release`
  - `.fastembed_cache`
  - `state/external-benchmarks/*`
- live state вроде `state/postgres`, `state/qdrant`, `state/minio`, `state/nats` этот контур не удаляет;
- `observe serve`, `observe snapshot` и `observe sla-check` теперь сами запускают auto-cleanup по TTL, поэтому старый локальный мусор не должен копиться бесконечно;
- текущий запущенный бинарь защищён от удаления, даже если его директория уже попала под TTL.
- `--aggressive` выключает возрастной запас и `keep_latest` только для rebuildable хвоста, поэтому это уже explicit reclaim path, а не обычный auto-retention;
- в human dashboard теперь есть отдельная карточка `Локальный мусор и retention`, где явно видны:
  - сколько можно убрать безопасно прямо сейчас;
  - сколько можно убрать explicit aggressive path-ом;
  - сколько вернул последний apply-run;
  - почему safe policy может пока держать хвост, даже если aggressive preview уже большой;
- после apply-run карточка больше не должна врать старым preview:
  - summary сразу пересчитывается повторным dry-run;
  - dashboard показывает текущий reclaim contour отдельно от `Last reclaim`, а не смешивает уже удалённый хвост с тем, что ещё реально лежит на диске.

После этого в live-отчёте у события появляются уже более сильные поля:
- `target_kind`
  - что именно система считала целью запроса:
    - файл;
    - символ;
    - документ;
    - трассу между файлами;
    - набор доказательств для более широкого вопроса;
- `amai_hit_target`
  - действительно ли `Amai` попал в нужный тип результата;
- `baseline_hit_target`
  - был ли у baseline вообще шанс честно покрыть этот запрос;
- `latency_ms`
  - сколько реально занял этот retrieval event;
- `query_slices`
  - отдельная сводка по типам запросов, а не одна усреднённая куча.
- `temperature_slices`
  - отдельная сводка по состоянию запроса:
    - `cold`
    - `warm`
    - дальше при накоплении могут появляться:
      - `post_restart`
      - `post_warmup`
      - `post_reindex`
- `median_recovery_tokens`
  - сколько токенов обычно пришлось вернуть назад на исправление, если первый ответ `Amai` оказался недостаточным.
- `session_id`
  - к какой непрерывной рабочей сессии относится событие;
- `rolling_window_profile`
  - в каком лимитном профиле это событие считается.
- `baseline_tokens` и `delivered_tokens`
  - прямые канонические числа для сравнения “сколько было бы без Amai” против “сколько реально отправил Amai”.
- realistic baseline classes теперь materialized и в runtime:
  - `ide_search_top_files` для file/config/symbol lookup;
  - `semantic_top_k` для architecture/bugfix вопросов;
  - `legacy_pre_amai` для onboarding path.
- quality layer стал глубже и тоже materialized в runtime:
  - `quality_tier`
    - `retrieval`
    - `answer_proxy`
    - `task_proxy`
    - `answer_success_recovered`
    - `task_success_recovered`
    - `partial`
  - `head_hit_target`
    - показывает, попал ли уже верхний слой retrieval в ожидаемую цель, а не только “нашлось что-то где-то”.
  - `answer_like_rate`
    - доля событий, где `Amai` не просто что-то нашёл, а уже дошёл до более строгого answer-like proxy.
  - `verified_answer_like_savings_pct`
    - более строгая secondary-метрика: какая доля экономии уже относится к событиям, где контекст выглядит достаточным для полезного ответа без лишнего follow-up.
  - `task_success_like_rate`
    - legacy-compatible secondary view: доля событий, где контур уже дошёл до более сильного task-level proxy, а не остановился на одном retrieval parity.
  - `verified_task_like_savings_pct`
    - legacy-compatible secondary-метрика: какая часть экономии уже относится именно к task-like событиям, а не только к широкому quality gate.

Это важно не для красоты, а для честности:
- если `Amai` сначала сэкономил токены, но потом заставил делать follow-up, retry или correction, эти токены теперь идут в штраф;
- headline при этом считается уже не по сырой экономии, а по `Verified Effective Savings %`.
- Prometheus exporter и human dashboard теперь по умолчанию тоже живут на verified/live-only цифрах;
  - raw savings остаются доступными отдельно как secondary engineering layer и не подменяют собой продуктовую цифру.
- если follow-up реально исправил предыдущий промах, у успешного события quality method может стать:
  - `hybrid_answer_success`;
  - это значит, что `Amai` не просто что-то нашёл, а довёл цепочку до более строгого answer-like результата уже с учётом recovery penalty.
- если follow-up не понадобился и нужная цель попала прямо в верхние retrieval hits, событие может получить:
  - `hybrid_answer_proxy`;
  - это честнее, чем просто `retrieval_parity`, но всё ещё не притворяется полноценным answer-LLM judge.

## Честно о скорости и пределах

### Что уже очень сильное

У проекта уже materialized сильный `hot cached retrieval` contour.

На референсной машине:
- `proof_load.sh`
  - `p95 = 0.022 ms`
  - `qps ≈ 62 266`
- `proof_stress_scale.sh`
  - `50 workers`
    - `p95 = 0.026 ms`
    - `qps ≈ 384 024`
  - `100 workers`
    - `p95 = 0.023 ms`
    - `qps ≈ 434 593`
  - `200 workers`
    - `p95 = 0.020 ms`
    - `qps ≈ 670 016`

Текущий стандарт для честного hot-load benchmark в human dashboard теперь не считается достаточным на маленькой выборке.
Для карточки `Быстрый путь под нагрузкой` последним сохранённым прогоном теперь считается только прогон с крупной выборкой:
- `workers > 16`
- `sample_count > 10000`

Это сделано специально, чтобы карточка не выглядела “рекордной” на слишком маленьком прогоне в несколько десятков запросов.

### Что здесь важно понимать

Здесь есть два разных сценария, и их нельзя сваливать в одну цифру.

- `быстрый повторный запрос`
  - это ситуация, когда `Amai` уже прогрет и нужные данные уже лежат в быстром локальном кэше;
  - именно в этом режиме получаются самые маленькие задержки;
- `первый запрос после старта`
  - это ситуация, когда `Amai` только что поднялся или ещё не успел прогреть нужный контекст;
  - такой запрос всегда тяжелее и поэтому медленнее.

Именно поэтому нельзя честно говорить пользователю только одну “красивую” цифру.
Нужно понимать:
- самая быстрая цифра относится к уже прогретому состоянию;
- первый запрос после запуска всё ещё заметно тяжелее.

Сейчас первый запрос после старта всё ещё измеряется десятками миллисекунд.
Чтобы обычный пользователь не видел лишнюю задержку сразу после запуска, полезен предварительный прогрев:

```bash
./scripts/warmup_cache.sh --projects project_alpha,project_beta
```

Проще говоря:
- если `Amai` уже поработал, ответы будут максимально быстрыми;
- если `Amai` только что запустили, первый запрос обычно будет медленнее;
- `warmup` нужен именно для того, чтобы заранее подготовить систему к работе.

### На каком железе это измерялось

Референсная машина:
- CPU:
  - `AMD Ryzen 9 7900X 12-Core Processor`
  - `12` физических ядер
  - `24` логических потока
  - до `5.7 GHz`
- RAM:
  - `62 GiB`
  - swap: `2 GiB`
- Storage:
  - `NVMe HS-SSD-G4000 2048G`
  - общий объём: `1.9 TiB`
- Архитектура:
  - `x86_64`

Что это значит простыми словами:
- эти цифры получены не на слабом ноутбуке и не на дешёвом VPS;
- это сильная локальная машина с быстрым NVMe-диском;
- на более слабом железе такие же результаты обещать нельзя.

На human dashboard это железо теперь тоже видно не только как статический baseline, но и как живая machine telemetry:
- CPU load / temperature;
- RAM type / speed / usage;
- primary disk type / load / temperature / firmware.

Честное ожидание такое:
- на таком же или более сильном железе эти proof-команды должны повторяться в том же порядке величин;
- на более слабой машине результаты будут хуже, особенно для первого запроса после запуска и для тяжёлых proof-контуров.

## Если нужен ручной инженерный путь

Если вы хотите не product path, а ручной контроль над каждым шагом:

```bash
cp .env.example .env
./scripts/bootstrap_stack.sh
./scripts/status.sh
```

Потом можно:
- зарегистрировать проекты;
- зарегистрировать relation graph;
- индексировать код;
- собирать `context pack`;
- запускать proof-контуры.

Примеры:

```bash
cargo run -- project register --code project_alpha --display-name "Project Alpha" --repo-root /path/to/project-alpha
cargo run -- namespace ensure --project project_alpha --code review
cargo run -- index project --code project_alpha --path /path/to/project-alpha --namespace review
cargo run -- context pack --project project_alpha --namespace review --query "how configuration is loaded"
```

Важно:
- `project register` хранит `repo_root` только в canonical absolute form;
- строки вроде `../Art`, `/srv/amai/.` и `/repo/subdir/..` не считаются разными проектами, если они указывают на один и тот же физический корень;
- если такой корень уже занят другим `project code`, `Amai` честно остановит регистрацию с ошибкой, а не создаст скрытый alias.
- если тот же `project code` регистрируется на новом physical root, это трактуется как relocation:
  - новый root становится primary;
  - старый root сохраняется как machine-readable alias этого же проекта;
  - continuity и path-based resolve продолжают видеть тот же логический проект.
- старый root после relocation нельзя тихо украсть другим проектом: для reuse такого path нужен
  явный operator-controlled contour, а не неявная новая регистрация.
- тот же canonical physical root потом используется и во время `index project`, чтобы exact-path retrieval
  оставался действительно relative к repo root, а не к случайной форме входного пути.

## Benchmark matrix

Чтобы не жить на одном удобном локальном proof и не придумывать benchmark-цели из головы, в `Amai` теперь есть machine-readable benchmark matrix.

Источник внешней карты:
- `philschmid/ai-agent-benchmark-compendium`
- <https://github.com/philschmid/ai-agent-benchmark-compendium>

Эта матрица нужна для трёх вещей:
- видеть, какие benchmark-семейства для `Amai` уже хотя бы mapped;
- не потерять следующий обязательный приоритет;
- привязывать наши proof-контуры к реальным внешним benchmark-классам: `MCP`, `continuity`, `coding`, `multi-agent isolation`, `browser/GUI`.

Смотреть матрицу можно так:

```bash
./scripts/benchmark_matrix.sh
./scripts/benchmark_matrix.sh coverage
./scripts/benchmark_matrix.sh explain --benchmark live-mcpbench
./scripts/benchmark_matrix.sh explain --benchmark "SWE-bench Verified"
```

Поверх этой карты уже materialized первый живой eval слой для `MCP`:

```bash
./scripts/proof_mcp_task_matrix.sh
```

Он делает то, чего раньше не было:
- гоняет не один `smoke`, а measured task matrix;
- считает успешность по задачам;
- отдельно держит:
  - happy-path;
  - hostile fail-closed;
  - multi-agent isolation;
- теперь ещё и раскладывает этот MCP-contour в общий `canonical_eval` слой:
  - `live_mcpbench_local` даёт `11 x hit_correct_target`;
  - `mcp_universe_local` даёт `8 x hit_correct_target` и `1 x recovered_useful`;
  - continuity restore внутри `mcp_universe_local` теперь считается не просто как `status=success`, а как канонический recovery verdict;
- даёт честный локальный bridge между `Amai` и классами `LiveMCPBench` / `MCP-Universe`.

Отдельно materialized и memory-eval слой по мотивам `Letta Leaderboard`:

```bash
./scripts/proof_memory_task_matrix.sh
```

Он проверяет уже не `MCP`, а саму память `Amai`:
- умеет ли `core memory` поднять сохранённый факт;
- переживает ли факт новый restore/startup;
- заменяется ли старое значение новым при update;
- не течёт ли память между agent scopes;
- не течёт ли archival memory между проектами.
- дополнительно раскладывает результат в один канонический `canonical_eval` слой:
  - `hit_correct_target`
  - `hit_wrong_target`
  - `stale_target`
  - `under_retrieved`
  - `over_included`
  - `recovered_useful`
  - `not_useful`
- этот слой теперь живёт не внутри одной матрицы, а в общем shared-контуре:
  - catalog и versioning — `config/retrieval_science.toml`
  - canonical verdict semantics — `src/eval_verdict.rs`
  - `proof_memory_task_matrix.sh` гоняет матрицу два раза подряд, чтобы повторный прогон не ломался на continuity/idempotency path.

Что это даёт простыми словами:
- `list`
  - показывает всю карту benchmark-семейств;
- `coverage`
  - показывает, где у `Amai` уже есть сильный задел, а где долг ещё впереди;
- `explain`
  - раскладывает один benchmark по-человечески: зачем он нужен, что у нас уже есть и какой следующий шаг обязателен.

Отдельно теперь materialized и внешний retrieval/vector comparative contour:

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

Что он делает:
- не подменяет внутренний `Amai` cold/hot benchmark;
- проверяет, готова ли эта машина к внешним comparative benchmark-ам;
- печатает канонический dataset catalog для внешнего comparative contour;
- умеет скачивать dataset-ы Rust-контуром в канонический каталог;
- показывает adapter-plan `dataset -> ingest -> warmup -> workload -> metrics`;
- materialize-ит реальный adapter workspace для выбранного benchmark + dataset:
  - `summary.json`
  - `report.md`
  - `run_external.sh`
  - `run_status.json`
  - `run_external.log`
- для `VectorDBBench` этот contour теперь уже не остаётся на `blocked_conversion_required`:
  - `Amai` сам читает исходный HDF5 dataset;
  - Rust-конвертер materialize-ит custom Parquet bundle:
    - `train.parquet`
    - `test.parquet`
    - `neighbors.parquet`
    - `conversion_manifest.json`
  - затем тот же adapter workspace уже может честно запускать upstream `vectordbbench qdrantlocal ...`;
  - перед реальным run `Amai` теперь сам чистит старый `vectordbbench-results`, чтобы новый verdict не унаследовал прошлый провал из `latest/`;
  - для `QdrantLocal` adapter теперь materialize-ит локальный compatibility patch:
    - добавляет transport `timeout = 600` в upstream `QdrantLocalConfig`;
    - расширяет upstream CLI флагом `--timeout`;
    - затем уже запускает тот же `vectordbbench qdrantlocal` без изменения case threshold или dataset semantics;
  - `run_external.sh` теперь оценивает завершение не только по `exit code`, но и по реальному `result label` из `result_*.json`:
    - `finished_ok`
    - `finished_benchmark_failed`
    - `finished_without_results`;
  - если в `.env` заданы:
    - `AMI_BENCHMARK_QDRANT_HTTP_URL`
    - `AMI_BENCHMARK_QDRANT_COLLECTION_CODE`
    то human dashboard начинает отдельно показывать живые системные числа именно этого benchmark-Qdrant, не подмешивая их в обычный `Qdrant Amai live`;
- для `ann-benchmarks` workspace теперь строит безопасный upstream path:
  - `python3 -m venv .venv`
  - `pip install -r requirements.txt`
  - symlink dataset в `data/<dataset>.hdf5`
  - `python install.py --algorithm qdrant`
  - `python run.py --dataset ... --algorithm qdrant`
  - без ложного `docker compose up`, который мог бы случайно зацепить родительский compose другого проекта;
- разводит 3 разных слоя:
  - `VectorDBBench`
    - общий framework `engine + dataset + scenario`;
  - `ann-benchmarks`
    - ceiling именно ANN/retrieval-core;
  - `Filtered ANN benchmark datasets`
    - payload/filter pressure layer.

Правильный смысл этого слоя:
- весь продукт `Amai` как black-box потом идёт через `general framework + adapter`;
- search/retrieval-core отдельно идёт через `ANN benchmark`;
- payload/filter behaviour отдельно идёт через filtered ANN datasets.

То есть внешний benchmark contour нужен не ради красивой ссылки на GitHub, а чтобы рядом с внутренними proof-ами был ещё и честный apples-to-apples сравнительный слой.

HDF5-датасеты, которые уже зафиксированы как стартовый каталог:
- `dbpedia-openai-1000k-angular`
- `snowflake-msmarco-arctic-embed-m-v1.5-angular`
- `sift-128-euclidean`
- `sphere-10M-meta-dpr`

Важный инвариант этого слоя:
- `ann-benchmarks` нельзя считать универсальным входом для любого HDF5 только потому, что файл существует;
- сейчас `ann-benchmarks` честно готовится только там, где upstream уже знает этот dataset по имени;
- если upstream держит canonical launch path выключенным, `Amai` обязан пометить это как `blocked_upstream_disabled`, а не продолжать называть contour `prepared`;
- если dataset не поддержан upstream напрямую, `Amai` adapter обязан fail-closed пометить его как `blocked_unsupported_dataset`, а не выдавать ложное `prepared`.

Они не запускаются автоматически при обычном proof-cycle.
Но они уже materialized как канонический dataset-manifest для внешнего adapter-контура, чтобы следующий шаг не жил на устной инструкции из чата.

Важно:
- для `ann-benchmarks` HDF5 datasets подходят напрямую;
- для `VectorDBBench` HDF5 больше не считается прямым input:
  сначала `Amai` materialize-ит custom Parquet bundle `train/test/neighbors`,
  а уже потом запускает честный upstream path;
- для `VectorDBBench/QdrantLocal` у `Amai` теперь есть отдельный machine-readable список
  `compatibility_overrides` в `summary.json`, чтобы было видно, где benchmark идёт
  совсем без вмешательства, а где есть локальный Amai-managed patch;
- для Rust-конвертера нужен рабочий `cmake` в `PATH`, потому что bundled HDF5 crate собирает native слой локально.

## Ключевые proof-команды

Если нужен честный локальный proof:

```bash
./scripts/proof_local.sh
./scripts/proof_hardening.sh
./scripts/proof_performance.sh
./scripts/proof_accuracy.sh
./scripts/proof_load.sh
./scripts/proof_hostile.sh
./scripts/proof_benchmark_matrix.sh
./scripts/proof_external_benchmark_env.sh
./scripts/proof_external_benchmark_adapter.sh
./scripts/proof_mcp_task_matrix.sh
./scripts/proof_memory_task_matrix.sh
./scripts/proof_art_continuity_migration.sh
./scripts/proof_token_benchmark.sh
./scripts/proof_cold_benchmark.sh
./scripts/proof_context_decision_trace.sh
./scripts/proof_working_state_decision_trace.sh
./scripts/proof_observability.sh
./scripts/proof_mcp.sh
./scripts/proof_onboarding.sh
./scripts/proof_client_lifecycle.sh
./scripts/proof_stress_scale.sh
./scripts/proof_text_compare.sh
./scripts/proof_text_compare_real_projects.sh
```

`./scripts/proof_cold_benchmark.sh` теперь проверяет не только
`cold_benchmark.machine_readable_summary`, но и
`cold_benchmark.canonical_eval` с probe-level verdict layer по каждому cold case.

`./scripts/proof_context_decision_trace.sh` теперь отдельно проверяет, что обычный
`context pack` возвращает machine-readable `decision_trace`, а не только raw retrieval arrays.

`./scripts/proof_working_state_decision_trace.sh` проверяет следующий product path:
живой `context pack` должен дойти до `latest_working_state_restore` и принести туда
`latest_decision_trace / recent_decision_traces`, а не потеряться между retrieval и restore.

После этого human dashboard уже может показывать эти причины в карточке `Текущая работа`:
отдельно почему последний retrieval что-то включил и отдельно почему часть слоёв ничего не добавила.

Важно:
- карточка `Текущая работа` теперь берёт не глобально самый новый `working_state_restore`,
  а именно `latest_repo_working_state_restore` для текущего repo;
- если локального рабочего снимка для этого repo ещё нет, панель честно показывает пустое состояние
  и не подмешивает более свежую рабочую линию другого проекта;
- если последняя локальная линия пришла не из свежего `context pack`, строки
  `Почему включено / Почему не вошло` не рисуются пустыми заглушками.

`./scripts/proof_onboarding.sh` теперь проверяет, что install/onboarding path тоже
печатает explainability последнего собранного контекста, а не только сухие runtime-метрики.

Для отдельного DB-level proof именно по observability guardrails:

```bash
cargo run --release -- observe guardrails
```

Для retrieval science и isolation source of truth теперь machine-readable:

- [config/retrieval_science.toml](/home/art/agent-memory-index/config/retrieval_science.toml)
  - версии methodology/scoring/degradation/execution-state/lineage;
  - фиксированные suite-version для hot/cold/load/accuracy/continuity/token/text-compare;
  - truth ranking и политика `same input -> same verdict`;
  - cold contour теперь тоже пишет machine-readable `canonical_eval`, а не только latency/precision summary;
  - machine-readable degradation matrix:
    - какие классы должны fail-closed;
    - какие должны уходить в безопасный мягкий откат;
    - какой proof/runbook закреплён за каждым классом.
  - теперь сюда же входит и `continuity_verification`:
    - direct recovery proof вне MCP-обёртки;
    - machine-readable verdict по `handoff / working_state / chat_start_prompt / replay freshness / previous_chat / exact_time`;
    - reproducibility contract для continuity import/handoff/startup и direct temporal lookup.
- [config/red_team_retrieval_isolation.toml](/home/art/agent-memory-index/config/red_team_retrieval_isolation.toml)
  - фиксированный red-team retrieval isolation contour для `project_alpha/project_beta`;
  - hostile mixed query;
  - отдельные hostile visible/hit invariants по проекту и namespace;
  - versioned query suite и scoring rules для `verify accuracy`;
  - тот же `verify accuracy` теперь тоже пишет `canonical_eval` через тот же общий verdict layer, а не только raw precision/invariant числа.
- [fixtures/text_compare_cases.jsonl](/home/art/agent-memory-index/fixtures/text_compare_cases.jsonl)
  - versioned comparative retrieval quality suite для `verify text-compare`;
  - теперь этот contour тоже пишет `canonical_eval`, а не только precision/hit ratio и token contour;
  - по каждому кейсу и по каждой стратегии (`hybrid / lexical_only / semantic_only`) сохраняются:
    - `eval_verdict_class`
    - `eval_reason`;
  - сверху строится общий `text_compare.canonical_eval` с `verdict_counts`, `strategy_breakdown` и probe-level details.

В human dashboard это теперь видно отдельной service-card `Поведение при сбоях`.

Она показывает не только сам policy, но и:
- сколько классов уже подтверждены свежим machine-readable proof;
- сколько пока остаются только policy и честно помечены как evidence gap;
- какой порядок истины действует, если несколько слоёв спорят друг с другом.

Чтобы не держать часть этих классов только как policy, теперь есть и отдельный proof path:

```bash
cargo run --release -- verify degradation
```

Он:
- прогоняет versioned synthetic degradation suite для:
  - `cross_agent_scope`
  - `corrupt_scope_metadata`
  - `partial_refresh`
  - `partial_thread_index`
  - `qdrant_unavailable`
  - `stale_cache`
  - `empty_embeddings`
  - `stale_handoff`
  - `working_state_conflict`
- пишет snapshot `degradation_verification`;
- поднимает их из `unknown` в `pass` только после реального machine-readable proof, а не по описанию policy;
- использует те же product-path функции для working-state, temporal fail-closed и retrieval fallback, а не отдельную декоративную логику только для теста;
- для `working_state_conflict` теперь пишет уже не плоский lineage, а `lineage-v2` graph с `nodes / edges`;
- а proof-слой в коде дополнительно усилен property-based tests для fail-closed выбора `agent_scope / session_id` и для exact-time drift в temporal lookup.

Рядом с этим в live snapshot и human dashboard теперь materialized ещё и отдельный continuity-layer:
- raw JSON: `/api/snapshot -> continuity_correctness_model`;
- human-visible слой: service-card `Правильное продолжение`;
- Prometheus:
  - `amai_continuity_verified_probes_total`
  - `amai_continuity_failed_probes_total`
  - `amai_continuity_recovered_useful_total`
  - `amai_continuity_fail_closed_total`

Этот слой показывает не policy, а именно последний explicit `verify continuity` proof:
- сколько continuity probes подтверждены;
- сколько из них полезно восстанавливают рабочую линию;
- сколько честно fail-closed на отсутствующем прошлом чате или точном времени;
- сколько continuity checks реально провалилось.

Важно:
- `verify continuity` теперь сохраняет snapshot не только при PASS, но и при реальном провале;
- поэтому dashboard не обязан угадывать состояние по косвенным признакам и может честно показать `critical`, если continuity proof сломался.

Если нужен уже не короткий smoke, а честный end-to-end cold contour на большом real-repo pool:

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

Этот runner:
- сначала materialize-ит repo-pool так, чтобы `expected_paths` указывали только на реально существующие exact relative paths;
- для внешних git-репозиториев теперь materialize-ит выбранные candidate files через sparse-checkout tracked worktree, а не через `git show > file`, поэтому repo-pool не должен оставлять скрытые `D`-хвосты и ложные cold-quality misses на следующем refresh;
- для локальных репозиториев использует канонические `project code`, чтобы один и тот же repo не жил в benchmark-пуле под alias и под обычным именем одновременно;
- перед live indexing canonicalize-ит `repo_root` ещё раз, чтобы внешний repo из manifest не оставлял
  в индексе path-хвосты с `..` и не ломал честный exact-path score;
- exact document lookup теперь идёт через адресные SQL ветки:
  `relative_path`, `relative_basename`, `relative_basename_stem`, а не через regex-фильтр по всему namespace;
- тот же exact-path contour теперь честно держит и extensionless basename files вроде `Makefile`, не проваливаясь в broken fallback query;
- если case-set для локального repo не требует semantic path и держит `limit_semantic_chunks = 0`, manifest может и должен честно ставить `skip_embeddings = true`, чтобы cold contour не платил лишнюю цену за vector layer там, где этот конкретный набор проверок его всё равно не использует;
- сам индексирует указанные repo из manifest;
- считает отдельно `cold` и `hot shadow`;
- держит фиксированный набор эталонов для `Cold P50 / P95 / P99 / Max`, `precision / recall / hit rate`, `sample_count`, `repo_count`, `query_slice_count`, `duration`, `leakage` и `error rate`;
- пишет:
  - `summary.json`
  - `report.md`
  - `samples.csv`;
- сохраняет последний результат в observability snapshot, чтобы его увидел human dashboard.

Для детерминированного proof по отдельным реальным файлам теперь есть ещё один path:

```bash
./scripts/proof_text_compare_real_projects.sh
```

Этот proof:
- не делает слепой full-index огромных repo только ради нескольких checks;
- сначала строит точные allowlist-файлы из `fixtures/real_project_text_compare_cases.jsonl`;
- затем индексирует `Art` и `Amai` только по этим exact relative paths через `index project --paths-file ...`;
- после этого запускает `verify text-compare` уже на реальных локальных проектах, а не только на маленьких fixture-repo;
- и теперь дополнительно проверяет, что в output materialized canonical verdict layer:
  - `text_compare.canonical_eval.eval_verdict_model_version`
  - `text_compare.canonical_eval.probes`.

## Что означают ключевые слова

- `проект`
  - отдельный репозиторий или рабочий корень;
- `namespace`
  - именованная рабочая зона внутри проекта;
- `retrieval`
  - поиск нужного контекста;
- `semantic search`
  - поиск по смыслу;
- `context pack`
  - готовая подборка нужных материалов;
- `provenance`
  - откуда именно пришёл каждый фрагмент;
- `MCP`
  - стандарт подключения внешнего инструмента к IDE и ИИ-клиентам.

## Карта Файлов Текущего Уровня

- `AGENTS.md`
  - вход для нового ИИ-агента;
- `README.md`
  - главный человеческий вход;
- `Cargo.toml`
  - Rust package и binary `amai`;
- `compose.yaml`
  - локальный stack;
- `.env.example`
  - шаблон конфигурации.

## Карта Поддоменов

- `brand/`
  - branding contour проекта;
- `docs/`
  - подробная архитектура, lifecycle и операции;
- `config/`
  - machine-readable registry и профили;
- `sql/`
  - схема PostgreSQL;
- `scripts/`
  - пользовательские и инженерные launcher-скрипты;
- `fixtures/`
  - нейтральные test fixtures;
- `src/`
  - Rust CLI и runtime логика;
- `tests/`
  - локальные проверки;
- `state/`
  - локальные runtime-данные;
- `tmp/`
  - временные артефакты.

## Куда идти дальше

Если вам нужен:
- понятный пользовательский путь:
  - [docs/MCP_INTEGRATION.md](docs/MCP_INTEGRATION.md)
- инженерный lifecycle:
  - [docs/OPERATIONS.md](docs/OPERATIONS.md)
- deployment-режимы простым языком:
  - [docs/DEPLOYMENT_TARGETS.md](docs/DEPLOYMENT_TARGETS.md)
- архитектурная глубина:
  - [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
- branding:
  - [brand/README.md](brand/README.md)
