# Агент: старт, статус проекта и ближайший этап

## Зачем нужен этот документ

Если агент впервые заходит в `Amai`, он должен быстро понять:
- что это за проект;
- что уже реально materialized;
- что пока только в roadmap;
- какой этап внедрения следующий;
- какими документами руководствоваться;
- чего нельзя делать, чтобы не сломать проект.

Этот документ не заменяет остальные.

Его роль:
- дать новый вход в проект;
- развести `current-state` и `target-state`;
- убрать двусмысленность для любого агента;
- показать, что делать прямо сейчас.

И отдельный языковой закон:
- этот проект развивается как `Rust-first` system;
- новый внутренний runtime/proof/verify/eval слой по умолчанию нельзя уводить в `python`;
- существующие `python`-пути в проекте относятся только к external benchmark compatibility contour, а не к новому core-развитию.

## Канонический алгоритм агента

Если агент подключился к проекту и должен честно продолжать работу, он должен работать так:

1. Открыть `AGENTS.md`.
2. Обновить machine-readable preflight snapshot:
   - `./scripts/agent_preflight.sh --json`
3. Только после этого открыть этот документ.
4. Открыть `IMPLEMENTATION_STATUS.md`.
5. Понять:
   - что уже materialized;
   - что ещё нет;
   - какой этап следующий;
   - есть ли blocker.
6. Только потом открывать:
   - `ARCHITECTURE.md`;
   - `OPERATIONS.md`;
   - `AMAI_GLOBAL_MEMORY_ROADMAP.md`;
   - `MAINTAINABILITY_ENFORCEMENT.md`, если изменение stage-based, архитектурное или затрагивает critical zone; для остальных содержательных правок этот документ всё равно остаётся binding law, а не опцией;
   - `IMPLEMENTATION_GATES.md`, если работа stage-based;
   - частные планы, если задача про их модуль.
7. Если работа implementation-stage:
   - идти по checkbox/checklist и по соответствующему этапу roadmap;
   - пройти `./scripts/maintainability_gate.sh --json`, если изменение не тривиальное и не purely local;
   - понимать, что gate materialize-ит trace в `.amai/onboarding/project-maintainability-gate-state.json`;
   - если как часть значимого шага меняется `docs/IMPLEMENTATION_STATUS.md`, подтвердить это через `./scripts/implementation_status_sync_guard.sh --json`;
   - перед изменением checkbox значимого этапа пройти `./scripts/maintainability_stage_close_guard.sh --json`;
   - если изменение маленькое и purely local, всё равно не нарушать `MAINTAINABILITY_ENFORCEMENT.md`; отсутствие отдельного gate-run не является разрешением игнорировать standard;
   - открыть matching section в `IMPLEMENTATION_GATES.md`;
   - если для этапа уже есть готовый benchmark/proof harness, использовать сначала его;
   - не ждать, пока пользователь отдельно напомнит про benchmark из подходящего bundle;
   - если touched surface включает retrieval/vector lane, отдельно запускать и external/Qdrant bundle из `IMPLEMENTATION_GATES.md`;
   - если harness публикует результат в dashboard, после прогона проверить и dashboard surface;
   - если contour не surfaced в dashboard, а для него закреплён raw-result lane, использовать именно raw result;
   - не считать reduced-sample benchmark stage-close proof;
   - не перепрыгивать через этапы;
   - не переоткрывать locked-решения.
8. После каждого значимого подшага:
   - tests;
   - manual check;
   - debug/fix;
   - retest;
   - update `IMPLEMENTATION_STATUS.md`;
   - write continuity handoff.

Простое правило:
- агент не должен сначала “изучать весь репозиторий наугад”;
- сначала он обязан пройти preflight gate и этот документный входной алгоритм.

## Дерево решений агента

Это можно представлять не как россыпь документов, а как жёсткое дерево.

### Корень

В корне лежат главные документы:
- `AGENTS.md`
- `README.md`
- `AGENT_START_HERE.md`
- `IMPLEMENTATION_STATUS.md`

Смысл корня:
- понять, что это за проект;
- понять current-state;
- понять target-state;
- понять, что делать прямо сейчас.

### Ствол

Стволом считается `IMPLEMENTATION_STATUS.md`.

Почему:
- там видно, что уже закрыто;
- что ещё не закрыто;
- какой этап следующий;
- по какому checkbox идти дальше.

Простое правило:
- если агент не понял, на каком этапе проект находится сейчас, он ещё не дошёл даже до ствола;
- начинать работу в коде раньше этого нельзя.

### Ветви

От ствола агент идёт по одной из ветвей.

#### Ветвь 1. Current-state разбор или bugfix

Если задача про то, как проект работает уже сейчас:
- открыть `ARCHITECTURE.md`;
- открыть `OPERATIONS.md`;
- при необходимости смотреть runtime, schema, scripts и код;
- не путать это с target-state roadmap.

#### Ветвь 2. Новый implementation-stage

Если задача про развитие memory fabric:
- открыть нужный checkbox в `IMPLEMENTATION_STATUS.md`;
- перейти по ссылке в точный этап roadmap;
- работать только внутри этого этапа;
- не перепрыгивать через ещё не закрытые этапы.

#### Ветвь 3. Модульная работа

Если задача касается конкретного модуля:
- task/graph memory:
  - `AMAI_TASK_TREE_PLAN.md`;
- compare/eval/dashboard:
  - `AMAI_COMPARE_EXPERIMENT_PLAN.md`;
- остальное:
  - master-roadmap + профильный current-state документ.

#### Ветвь 4. После значимого подшага

После любого значимого куска работы агент обязан вернуться к стволу:
- обновить `IMPLEMENTATION_STATUS.md`;
- убедиться, можно ли ставить checkbox;
- записать continuity handoff;
- только потом идти дальше.

То есть это не одноразовое дерево.
Это рабочий цикл:
- корень;
- ствол;
- нужная ветвь;
- обратно в ствол.

## Что это за проект

`Amai` — это отдельный внешний memory/retrieval/continuity инструмент для ИИ-агентов.

Он нужен затем, чтобы агент:
- не терял рабочую линию между чатами и сессиями;
- не смешивал разные проекты по умолчанию;
- поднимал полезный context pack вместо лишнего шума;
- имел source-of-truth вне IDE и вне одного конкретного чата;
- мог работать в multi-agent и multi-project режиме без тихих утечек памяти.

Проще говоря:
- это не “один длинный чат”;
- это не “ещё один vector search”;
- это не “только task tree”.

Это отдельный backend/tooling contour памяти и continuity для агентов.

## В каком состоянии проект сейчас

Ниже честное состояние проекта на сегодня.

### Что уже materialized

Уже есть и реально работает baseline:
- `continuity startup` как machine-readable front door;
- `agent preflight` как machine-readable doc/status front door;
- `working-state` и `chat-start restore` baseline;
- `ExecCtl` первый durable contour:
  - `project_task_ledger`;
  - `pending_return`;
  - `active lease`;
- `PostgreSQL` как truth-source;
- lexical/symbol retrieval + semantic accelerator;
- benchmark registry и measured matrix contour;
- separation между `live` и `proof/verify/benchmark` token lanes;
- install/bootstrap/onboarding/runtime контуры;
- fail-closed startup laws и continuity contract.
- `Этап 1` `scope / identity control plane` закрыт и подтверждён proof bundle;
- `Этап 2` `typed memory envelope + provenance` закрыт и materialized в truth/read pipeline;
- `Этап 3` `commitment / task graph` и `Этап 3A` `procedural seed contour` уже закрыты;
- `Этап 4` `workspace restore pack`, `Этап 5` `semantic + temporal strengthening`, `Этап 6` `multi-agent shared/private memory` и `Этап 7` `compare + benchmark plane` тоже уже закрыты по текущему status checklist.

### Что уже есть, но ещё не является полной целевой реализацией

Есть сильный промежуточный baseline:
- continuity между чатами работает частично;
- pending return и resume obligations уже surfaced;
- dashboard/eval/compare contours уже есть;
- graph-first task-memory baseline уже materialized, но вся memory fabric ещё не доведена до финального target-state;
- startup/restore уже сильный, но ещё не даёт полную memory fabric.

### Что ещё не materialized полностью

Пока ещё не доведены до полного target-state:
- `Memory Fabric` как общая рабочая архитектура;
- полноценный graph-wide restore continuity beyond уже materialized `workspace_restore_pack`;
- полноценная `procedural memory` c evaluator/trust contour;
- forgetting / consolidation / pruning;
- governance / safety / poisoning defense как материализованный enforcement layer;
- по-настоящему бесшовное продолжение длинных чатов с несколькими старыми линиями работы.

### Главная честная оговорка

Полноценный бесшовный переход в новый чат пока ещё не решён до конца.

То есть:
- continuity и restore уже есть;
- но длинная смешанная история задач пока ещё может не подниматься так бесшовно, как должна в target-state.

Это не скрытый баг документации.
Это один из главных product outcomes всего утверждённого roadmap.

## Чем руководствоваться

Агент не должен одинаково читать все документы подряд без понимания их роли.

### Документы про current-state

- `AGENTS.md`
  - обязательный startup law, runtime discipline, fail-closed правила.
- `README.md`
  - продуктовая картина и базовый старт.
- `docs/IMPLEMENTATION_STATUS.md`
  - живая короткая сводка:
    - что уже сделано;
    - что ещё нет;
    - что сейчас в работе;
    - какой этап следующий.
- `docs/ARCHITECTURE.md`
  - текущий materialized baseline и долговременные архитектурные законы.
- `docs/OPERATIONS.md`
  - как новый слой должен доказываться, проверяться и fail-closed защищаться.

### Документы про target-state

- `docs/AMAI_GLOBAL_MEMORY_ROADMAP.md`
  - главный target-state roadmap и канонический порядок внедрения.
- `docs/AMAI_TASK_TREE_PLAN.md`
  - частный план по модулю `Commitment / Task Graph`.
- `docs/AMAI_COMPARE_EXPERIMENT_PLAN.md`
  - частный план по compare/eval/dashboard surface.

### Простое правило чтения

- если нужно быстро понять, где проект находится прямо сейчас:
  - сначала читать `IMPLEMENTATION_STATUS`;
- если нужно понять, как проект работает сейчас:
  - читать `README`, `ARCHITECTURE`, `OPERATIONS`;
- если нужно понять, что строим дальше:
  - читать `AMAI_GLOBAL_MEMORY_ROADMAP`;
- если задача касается конкретного модуля:
  - затем читать его частный план.

## Что делать прямо сейчас

Для нового инженерного шага правильный порядок такой:

1. Сначала открыть `IMPLEMENTATION_STATUS` и увидеть, что уже сделано, а что нет.
2. Потом понять, это задача про current-state fix или про следующий roadmap stage.
3. Если это roadmap-внедрение, не перепрыгивать через этапы.
4. Брать следующий незавершённый этап из master-roadmap.
5. Перед кодом проверить:
   - proof/gate;
   - migration path;
   - kill-switch;
   - fail-closed поведение;
   - impact на truth, speed, quality и isolation.
   - и отдельно: не покупается ли одна ось проекта ценой тихой деградации другой.
6. После подшага сразу делать:
   - tests;
   - manual check;
   - debug/fix;
   - retest.
7. После этого обновлять `IMPLEMENTATION_STATUS` и continuity handoff.

## Какой этап следующий

Сейчас проект уже прошёл ранние этапы memory fabric и дошёл до следующего незакрытого большого модуля.

### Ближайший обязательный этап

`Этап 8. Procedural memory`

Это значит:
- procedural seed и compare-plane proof уже materialized, но этого ещё недостаточно для честного full procedural memory;
- следующий обязательный шаг теперь не про scope/provenance base, а про executable procedural memory с trust/evaluator discipline.

Что должно войти в этот этап:
- executable skill memory как first-class truth contour, а не text-only заметки;
- trigger/precondition/execution/stop/forbidden/expected-outcome contract;
- evaluator/trust contour для shadow -> trial -> verified promotion;
- suppression плохих и устаревших навыков;
- сохранение procedural provenance и version history;
- non-regression against already closed compare-plane and procedural-seed proofs.

### Что нельзя делать раньше этого этапа

Нельзя:
- считать, что `proof_procedural_seed.sh` или compare-plane procedural benchmark уже равны full procedural memory;
- продвигать skill в shared/verified path без evaluator/trust contour;
- подменять executable procedural object обычной заметкой или summary-only карточкой;
- перепрыгивать из compare-plane сразу в forgetting/governance, не закрыв procedural stage.

## Короткая карта внедрения

Ниже короткая карта, чтобы агент не потерял порядок.

1. `Memory Fabric model`
   - общая модель уже зафиксирована в документах.
2. `Scope / Identity Control Plane`
   - следующий кодовый этап.
3. `Typed Memory Envelope + Provenance`
   - сразу после scope plane.
4. `Commitment / Task Graph`
   - после scope и provenance.
5. `Early Procedural Seed`
   - ранний управляемый слой навыков.
6. `Workspace Restore Pack`
   - богатый restore вместо task-centric restore.
7. `Semantic + Temporal strengthening`
8. `Shared/Private multi-agent memory`
9. `Compare + Benchmark plane`
10. `Full Procedural Memory`
11. `Forgetting / Consolidation / Pruning`
12. `Governance / Safety / Evaluator loop`

Подробности этапов живут в [AMAI_GLOBAL_MEMORY_ROADMAP.md](AMAI_GLOBAL_MEMORY_ROADMAP.md).

## Что предусмотреть заранее

Агент не должен кодить “на удачу”.

Перед любым значимым этапом нужно заранее предусмотреть:
- machine-readable контракт;
- proof или честный evidence gap;
- migration plan;
- kill-switch;
- rollback path;
- audit/provenance след;
- fail-closed поведение;
- scope/isolation последствия;
- benchmark или eval impact;
- обновление документации и continuity handoff.

Upgrade-law для любого нового слоя простой:
- сначала фиксируем baseline затронутого контура;
- затем называем риск по `speed / accuracy / quality / truth`;
- затем выбираем полный stage-local и companion non-regression bundle;
- и только потом делаем promotion-решение.

## Чего нельзя делать

Агент не имеет права:
- считать `Qdrant` или UI source-of-truth;
- смешивать current-state и target-state как будто они уже одно и то же;
- перескакивать через `scope` и `provenance` к более “красивым” слоям;
- подмешивать unrelated projects “по смыслу”;
- молча считать seamless continuity уже решённой;
- внедрять новый слой без stage gate и rollback path;
- сохранять секреты в continuity или memory.

## Короткий итог

Если совсем коротко:
- `Amai` уже имеет сильный current-state baseline;
- глобальная memory fabric уже спроектирована;
- проект готов к началу `Этапа 1`;
- главный ближайший фокус:
  - `scope / identity control plane`;
- главный общий закон:
  - скорость, точность, качество и правдивость нельзя разменивать друг на друга;
- главный продуктовый недостающий outcome:
  - по-настоящему бесшовное продолжение работы в новом чате.
