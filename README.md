modified_at: 2026-03-21 21:48 MSK
Ручная сверка guide/docs: 2026-03-21 21:48 MSK

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

Поверх этого `Amai` теперь автоматически собирает ещё и `chat-start restore pack`.

Это короткий готовый блок, который уже можно считать восстановленным рабочим контекстом для первого содержательного ответа нового чата.
В нём уже лежат:
- текущая активная линия;
- обязательный следующий шаг;
- что уже materialized;
- последние действия;
- активные файлы;
- confidence recovery.

То есть новый чат теперь должен видеть не только summary “о чём проект”, а уже компактный готовый стартовый контекст для продолжения работы.

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

### Linux и macOS

```bash
./scripts/install_amai.sh --client vscode
./scripts/install_amai.sh --client cursor
./scripts/install_amai.sh --client codex
```

### Windows PowerShell

```powershell
.\scripts\install_amai.ps1 --client vscode
.\scripts\install_amai.ps1 --client cursor
.\scripts\install_amai.ps1 --client codex
```

### Windows CMD

```bat
scripts\install_amai.cmd --client vscode
scripts\install_amai.cmd --client cursor
scripts\install_amai.cmd --client codex
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
./scripts/continuity_answer.sh --project art --namespace continuity --intent last_chat --include-previous-chat-messages --messages-count 2
./scripts/continuity_startup.sh --project art --namespace continuity
./scripts/continuity_restore.sh --project art --namespace continuity
```

Разница такая:
- `continuity answer`
  - печатает уже готовый короткий ответ для continuity-вопросов вроде `на чём остановились`;
  - это read-only путь без нового handoff;
  - если добавить `--include-previous-chat-messages`, он ещё поднимет хвост предыдущего чата и последние сообщения;
- `continuity startup`
  - печатает human-readable стартовую сводку для нового чата;
  - и сразу добавляет `Chat-start restore pack`, который уже нужно считать восстановленным рабочим контекстом для первого содержательного ответа;
  - теперь в этом выводе печатается и готовый `prompt_text`, чтобы новый чат получал не только summary, но и прямой compact restore для первого ответа;
- `continuity restore`
  - печатает raw restore-bundle JSON целиком;
  - теперь в этом JSON есть не только `working_state_restore`, но и отдельный `chat_start_restore` с готовым `prompt_text`.

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
Он сам поднимает observe-server в фоне, пишет PID и путь к логу, а затем возвращает URL.
По умолчанию панель обновляется раз в `1` секунду, чтобы live-цифры были ближе к реальному состоянию, а не висели как старый снимок.

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
  - это отдельный контур для честной проверки Windows-пути через виртуальную машину.

Если хотите разобрать конкретный режим подробнее:

```bash
cargo run -- deployment explain --target kubernetes_server
```

Если хотите проверить, готова ли именно эта машина к конкретному режиму:

```bash
./scripts/deployment_preflight.sh --target windows_vm_lab
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

## Что `Amai` делает внутри

Внутри у `Amai` есть несколько важных слоёв.

### 1. Изоляция проектов

Это главный закон проекта:
- новый `repo_root` считается отдельным проектом;
- смешивать проекты по умолчанию нельзя;
- чтение другого проекта разрешается только по явным relation/policy правилам.

### 2. Поиск идёт не одним способом

`Amai` не опирается только на embeddings.

Он ищет так:
1. exact/lexical поиск;
2. symbols и структура кода;
3. semantic поиск;
4. сборка готового `context pack` с указанием источника каждого фрагмента.

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

Главное правило:
- headline-метрика у `Amai` теперь не raw и не synthetic;
- по умолчанию это `live-only`, `quality-gated` и `recovery-aware` показатель;
- каноническое имя этой метрики:
  - `Verified Effective Savings %`
  - по-русски: `Проверенная реальная экономия`.

По умолчанию proof/benchmark-трафик не смешивается с обычной рабочей активностью.
Если нужно показать всё вместе, используйте:

```bash
./scripts/token_report.sh --include-verify-events true
```

Если у вас есть старые исторические `token_budget_event`, записанные ещё старым форматом, `Amai` умеет подтянуть их до нового качества без ручного SQL:

```bash
cargo run --release -- observe repair-token-ledger --apply
cargo run --release -- observe reverify-token-ledger --apply
```

Что делают эти команды:
- `repair-token-ledger`
  - достраивает недостающие поля старого формата;
- `reverify-token-ledger`
  - заново прогоняет старые live-запросы через текущий retrieval contour;
  - поднимает их из `legacy_unverified` в quality-gated live-выборку, если retrieval реально проходит.

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

Что это даёт простыми словами:
- `list`
  - показывает всю карту benchmark-семейств;
- `coverage`
  - показывает, где у `Amai` уже есть сильный задел, а где долг ещё впереди;
- `explain`
  - раскладывает один benchmark по-человечески: зачем он нужен, что у нас уже есть и какой следующий шаг обязателен.

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
./scripts/proof_mcp_task_matrix.sh
./scripts/proof_memory_task_matrix.sh
./scripts/proof_token_benchmark.sh
./scripts/proof_cold_benchmark.sh
./scripts/proof_observability.sh
./scripts/proof_mcp.sh
./scripts/proof_onboarding.sh
./scripts/proof_client_lifecycle.sh
./scripts/proof_stress_scale.sh
./scripts/proof_text_compare.sh
```

Если нужен уже не короткий smoke, а честный end-to-end cold contour:

```bash
./scripts/cold_benchmark.sh --manifest config/cold_benchmark_manifest.toml
```

Этот runner:
- сам индексирует указанные repo из manifest;
- считает отдельно `cold` и `hot shadow`;
- пишет:
  - `summary.json`
  - `report.md`
  - `samples.csv`;
- сохраняет последний результат в observability snapshot, чтобы его увидел human dashboard.

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
