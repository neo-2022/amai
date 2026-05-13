# Глобальный roadmap памяти Amai

## Зачем нужен этот документ

Сейчас в проекте уже есть несколько сильных документов:
- `README.md`
  - продуктовая картина и базовое объяснение, что такое `Amai`;
- `docs/AMAI_SYSTEM_OVERVIEW.md`
  - единая human-readable картина того, что `Amai` такое целиком:
    память, continuity, задачи, интеграции, платформы, ОС/host слой и scientific advisory
    contour;
- `docs/ARCHITECTURE.md`
  - слои, source of truth, retrieval laws, `ExecCtl` baseline;
- `docs/OPERATIONS.md`
  - как любой новый слой должен доказываться, проверяться и fail-closed защищаться;
- `docs/TOKEN_LEDGER.md`
  - как честно считать экономию и не смешивать truth с красивыми цифрами;
- `docs/AMAI_COMPARE_EXPERIMENT_PLAN.md`
  - как честно сравнивать `с Amai` и `без Amai`;
- `docs/AMAI_TASK_TREE_PLAN.md`
  - как развить task-memory в нормальное дерево или граф задач.

Проблема не в том, что этих планов мало.

Проблема в том, что без общего roadmap они легко начинают восприниматься как отдельные миры.

И даже при наличии roadmap новый агент всё ещё может сузить картину проекта до одного контура,
если не поднимет сначала `docs/AMAI_SYSTEM_OVERVIEW.md`.

Этот документ нужен затем, чтобы:
- ничего не потерять;
- не смешать уровни системы;
- видеть, что является базой, а что надстройкой;
- понимать, в каком порядке это реально внедрять.

## Главный вывод

`Amai` не должна развиваться как одна большая “память всего подряд”.

Правильная цель:
- не один `task tree` вместо всей системы;
- и не один `retrieval store` вместо памяти;
- а модульная память, где каждый слой отвечает за своё.

Простыми словами:
- задачи;
- факты;
- временная история;
- правила;
- восстановление рабочего состояния;
- измерение пользы;
- debug и trust

должны быть связаны, но не смешаны в одну кучу.

## Главный продуктовый закон

Для `Amai` уже зафиксирована формула, которую нельзя нарушать:

- скорость;
- точность;
- качество;
- правдивость.

Это не четыре независимые “хотелки”.
Это единый non-regression contract проекта.

Простое правило:
- нельзя усиливать одну из этих осей ценой тихой деградации другой;
- нельзя покупать экономию памяти ценой правды;
- нельзя покупать скорость ценой точности;
- нельзя покупать удобство ценой качества;
- нельзя покупать богатую память ценой ложного восстановления контекста.

То есть:
- baseline проекта не должен деградировать;
- новые слои памяти, retrieval, procedural memory, benchmark/UI и optimisation-path имеют право только:
  - сохранять текущий уровень;
  - или улучшать его;
- если слой не может это доказать, он не считается готовым к promotion.

## План materialization non-regression contract

Этот закон нельзя оставлять как красивую формулировку.
Для `Amai` он должен исполняться как пошаговый upgrade-plan.

Любой новый слой или upgrade-path обязан идти в таком порядке:

1. Сначала зафиксировать baseline затронутого контура.
   - Что является текущей truth-моделью?
   - Какие speed/accuracy/quality/truth метрики уже materialized?
   - Какие proof, benchmark, dashboard-card или raw-result lane уже authoritative?
2. Затем явно назвать ось риска.
   - Что именно можно испортить этим изменением:
     - скорость;
     - точность;
     - качество;
     - правдивость;
     - isolation / continuity / dashboard truth, если они тоже задеты.
3. Затем выбрать полный proof bundle, а не один удобный тест.
   - Stage-local harness.
   - Companion non-regression harness для соседних осей.
   - Dashboard-check или raw-result check, если contour публикуется наружу.
4. Только после этого менять код, schema, contract, UI или runtime.
5. После изменения обязательно сверять две вещи:
   - не улучшили ли одну ось ценой тихой деградации другой;
   - не превратился ли projection или benchmark surface в источник ложной истины.
6. Promotion разрешён только в одном из двух случаев:
   - baseline сохранён;
   - baseline улучшен и это доказано measured contour-ом.
7. Если доказательства нет, слой остаётся:
   - в draft;
   - в experimental;
   - в pending_approval;
   - или в internal-only contour;
   но не promoted.

Простая operational формула:
- `фиксируем baseline -> называем риск -> выбираем полный proof bundle -> вносим change -> проверяем non-regression -> только потом promotion`

## Легенда качества требований

Чтобы большие списки были однозначными:
- `REQUIRED` — обязательный элемент, без него этап не считается готовым;
- `SHOULD` — обязателен по умолчанию, но может быть отложен только с явной фиксацией причины в `IMPLEMENTATION_STATUS.md`;
- `OPTIONAL` — допустим как расширение, не блокирует прохождение этапа.
Если список не помечен, считать его `REQUIRED` по умолчанию.

## Что именно нужно доказывать перед promotion

Чтобы новый слой считался честно готовым, агент обязан показать:

### 1. Speed non-regression

- latency / throughput / load contour не просел без явного и принятого контракта;
- expensive fallback не стал новым default path;
- rebuild / optimize / cold-start phase не выдаётся за steady-state результат.

### 2. Accuracy non-regression

- retrieval не теряет correctness ради скорости;
- benchmark summary не скрывает слабые профили и не выдаёт best-case за систему целиком;
- stale или partial result не показывается как success.

### 3. Quality non-regression

- UX/UI не деградирует до noisy, misleading или нечитаемого surface;
- supportability, rollback и manual debug path не стали хуже;
- новый слой не делает проект труднее для следующего инженера.

### 4. Truth non-regression

- source of truth не подменён derived projection-слоем;
- memory-layer не создаёт ложного восстановления контекста;
- экономия памяти, агрессивный forgetting или удобный shortcut не скрывают и не уничтожают истину.

Если хотя бы один из этих четырёх пунктов не доказан,
слой не готов к promotion даже если локально “всё работает”.

## Что уже зафиксировано и не должно переизобретаться заново

Это locked-решения для следующего инженерного этапа.

Когда начнётся код, нельзя снова открывать спор по этим пунктам, как будто они ещё не решены.
Любое утверждение формата «уже» в этом документе считается валидным только после сверки с `IMPLEMENTATION_STATUS.md` и `IMPLEMENTATION_GATES.md`.

### Locked-решение 1. Основа task-memory теперь graph-first

- `task tree` больше не считается source of truth;
- основа task-memory = `commitment_graph`;
- дерево, если понадобится, может быть только производным видом из графа.

### Locked-решение 2. Общая память остаётся модульной

Мы уже решили, что нельзя сводить всё к одному task-store.

Зафиксированные слои:
- `Identity / Scope Plane`
- `Immutable Interaction Log`
- `Continuity / Working-State Plane`
- `Commitment / Task Graph`
- `Semantic Memory`
- `Temporal Memory`
- `Procedural Memory`
- `Policy / Trust Plane`
- `Restore Composer Plane`
- `Evaluation / Governance Plane`

### Locked-решение 3. Scope model обязателен до graph-ядра

Перед расширением task-memory должны существовать:
- scope-уровни;
- динамические права;
- `project_link`;
- `import_packet`;
- `borrowed/unverified` flow;
- `default_deny`.

### Locked-решение 4. Время и порядок событий обязательны

Нельзя надеяться только на один `timestamp`.

Для truth-layer и graph-memory уже зафиксировано:
- `observed_at`
- `recorded_at`
- `valid_from / valid_to`
- `last_verified_at`
- `ingest_seq`
- `object_version`
- `causation_id`
- `correlation_id`

### Locked-решение 5. Слабые связи не переписывают truth

Связи вроде:
- `semantically_related_to`
- `shares_context_with`
- `maybe_duplicate_of`
- `maybe_resumes`

являются только подсказками и не имеют права сами переписывать:
- `resumes`
- `duplicates`
- `depends_on`
- другие сильные truth-связи.

### Locked-решение 6. Procedural memory для Amai не может быть text-only

Для `Amai` уже зафиксировано:
- procedural memory не хранится как обычная заметка;
- durable skill должен быть исполнимым объектом;
- у него должны быть:
  - trigger conditions;
  - preconditions;
  - execution steps;
  - stop conditions;
  - forbidden conditions;
  - expected outcome.
- skill не может стать shared/verified только по одному локальному успеху;
- skill promotion всегда проходит через evaluator/trust contour;
- append-only procedural memory запрещена.

## Что делать перед первым кодовым этапом

Перед началом реализации нужно не “вспоминать с нуля”, а пройти короткий preflight:

1. Прочитать этот roadmap.
2. Прочитать [AMAI_TASK_TREE_PLAN.md](AMAI_TASK_TREE_PLAN.md) как частный план по graph/task memory.
3. Не открывать заново спор по locked-решениям выше.
4. Брать следующий этап по порядку, а не перепрыгивать через scope/provenance/temporal base.
5. После каждого большого подшага сразу делать proof и тесты.

Простыми словами:
- нет, при кодинге не нужно заново всё придумывать;
- нужно брать уже зафиксированные решения как контракт;
- и кодить по ним поэтапно.

## Что уже нельзя потерять

Ниже то, что уже является сильной стороной проекта и не должно быть сломано новой архитектурой.

### 1. `PostgreSQL` как source of truth

Это уже правильно.

Нельзя откатываться к модели, где:
- truth живёт в UI;
- truth живёт только в векторной базе;
- truth живёт в transcript и постфактум угадывается из чата.

### 2. `Qdrant` как ускоритель, а не истина

Это тоже уже правильно.

Поиск по смыслу нужен, но он не должен решать:
- что реально существует;
- к какой ветке относится задача;
- какой проект authoritative.

### 3. `continuity + working-state + ExecCtl`

Это уже очень сильный контур.

Нельзя его выкидывать только потому, что сверху хочется сделать более широкую memory fabric.

Нужно сделать наоборот:
- новая архитектура должна расти поверх него.

### 4. `startup/restore` как обязательный front door

Это одна из лучших частей проекта.

Нельзя возвращаться к модели:
- “пользователь сам должен каждый раз поднимать память”.

### 5. `proof-driven` рост

Это основной закон проекта:
- сначала machine-readable;
- потом proof/evidence;
- потом user-facing.

Новый global roadmap обязан подчиняться этому же закону.

### 6. `tokenonomics truth`

`TOKEN_LEDGER` и live/proof separation нельзя ломать.

Любой новый memory-layer не имеет права:
- портить live lane;
- мешать честному измерению `с Amai` / `без Amai`;
- подмешивать engineering traffic в product cards.

## Как теперь правильно называть общую цель

Общая цель не “идеальное дерево задач”.

Общая цель:
- `Amai Memory Fabric`

Простыми словами:
- большая связанная система памяти;
- внутри которой есть несколько модулей;
- и task-memory только один из них.

## Главные модули будущей памяти

Ниже не “дальняя фантазия”, а целевая структура, в которую уже укладываются текущие документы проекта.

### 1. Identity / Scope Plane

Этот модуль отвечает на вопрос:
- кто работает;
- в каком проекте;
- в каком контуре;
- что можно читать;
- что можно писать;
- что нельзя смешивать.

Сюда относятся:
- будущий `tenant`, если у памяти появится настоящий multi-org contour;
- `workspace`;
- `project_code`;
- `project_link`;
- `agent_scope`;
- будущие `agent_role`, `team`, `access_policy`, `retention_policy`, `sensitivity_policy`;
- `namespace_code`;
- `session_id`;
- `visibility_scope`.

Это первый слой, потому что без него любая memory system начнёт течь между проектами и агентами.

### 2. Immutable Interaction Log

Это сырой append-only след.

Сюда входят:
- сообщения;
- tool calls;
- tool outputs;
- handoff;
- raw runtime observations;
- artifact refs.

Его задача:
- хранить сырое основание для всего остального.

Нельзя строить серьёзную память без сырого следа.

### 3. Continuity / Working-State Plane

Это то, что в проекте уже очень хорошо сделано.

Он отвечает за:
- продолжение работы;
- startup;
- restore;
- current goal;
- next step;
- active files;
- recent actions.

Этот слой должен остаться ядром оперативной continuity.

### 4. Commitment / Task Graph

Это и есть развитие текущего `ExecCtl / task-memory`.

Сюда относятся:
- обязательства;
- планы;
- подзадачи;
- блокировки;
- зависимости;
- ownership;
- leases;
- resume-obligations.

Текущий `task tree` должен вырасти именно сюда.

Важно:
- внутренне это должен быть граф обязательств;
- в UI он может показываться как дерево, если так удобнее.

### 5. Semantic Memory

Это слой фактов и сущностей.

Он отвечает за:
- устойчивые project facts;
- профили;
- связи;
- кодовые и документные знания;
- knowledge updates.

Это не task-memory.
Это память “что известно”.

### 6. Temporal Memory

Это слой истории по времени.

Он отвечает за:
- прошлые чаты;
- exact time lookup;
- прошлые шаги;
- что было истинно когда.

Это уже есть в зачатке через temporal lookup и enriched thread index.

### 7. Procedural Memory

Это память “как лучше делать”.

Сюда должны попасть:
- playbooks;
- skill cards;
- удачные workflow;
- repair sequences;
- failure patterns.

Сейчас этот слой ещё не materialized как first-class память.

Важно:
- это не архив прошлого;
- это память о переиспользуемом способе действия;
- она не должна жить вперемешку с task-memory.

Простыми словами:
- task-memory отвечает на вопрос `что делаем`;
- procedural memory отвечает на вопрос `как это лучше делать`.

Почему этот слой у нас уже зафиксирован:
- `ProcMEM` показывает полезную для `Amai` идею переводить опыт не в длинный эпизодический рассказ, а в исполнимый skill с условиями:
  - когда активировать;
  - как выполнять;
  - когда завершать.
- `ReMe` полезен для нас не как “ещё одно хранилище”, а как напоминание, что procedural memory нельзя делать append-only архивом:
  - нужен distillation;
  - нужен context-adaptive reuse;
  - нужен utility-based refinement.
- `TAME` важен для `Amai` тем, что skill-memory нельзя развивать только по полезности:
  - нужен evaluator contour;
  - нужен trust-state;
  - нужен отдельный контур, который может тормозить плохую эволюцию памяти.

Итог для `Amai`:
 - ссылки на ProcMEM/ReMe/TAME фиксируются в [docs/REFERENCES.md](docs/REFERENCES.md) и считаются `concept-only`, пока не добавлены официальные источники.
- мы не храним procedural memory как обычную заметку;
- мы делаем её как управляемый объект с lifecycle, evidence и trust;
- и не позволяем shared procedural memory расти без проверки.

### 8. Policy / Trust Plane

Это слой правил, ограничений и trust state.

Он отвечает за:
- доступ;
- безопасность;
- compliance;
- quarantine;
- verification state;
- sensitivity;
- cross-project transfer rules.

Без него multi-agent memory будет опасной.

Что здесь должно быть зафиксировано сразу:
- `trust_state`
  - `raw`
  - `proposed`
  - `verified`
  - `disputed`
  - `deprecated`
  - `quarantined`
- `source_trust_score`
  - насколько надёжен сам источник, из которого вырос memory object;
  - это отдельный сигнал, который не равен итоговой `verification_state`.
- `sensitivity_class`
  - `public`
  - `internal`
  - `restricted`
  - `secret_like`
- `secret_redaction_policy`
  - память не должна сохранять токены, ключи, пароли и похожие секреты в открытом виде;
  - такие данные либо редактируются, либо не проходят в durable write вообще.
- `quarantine_store`
  - подозрительные memory items и подозрительные import-пакеты не исчезают тихо;
  - они уходят в отдельный quarantine contour с audit trail.
- `delete/forget path`
  - у системы должен быть отдельный управляемый путь забывания и удаления;
  - forgetting не должен быть просто побочным эффектом compaction.
- `write_allowlist` для shared procedural memory
  - не любой агент и не любой вывод может писать в общий procedural pool.
- `cross_project_writeback_requires_approval`
  - cross-project import не может тихо превращаться в локальную verified truth без явной проверки и разрешения.

Простое правило:
- долговременная память должна быть не только полезной, но и безопасной;
- если объект памяти нельзя безопасно показать, безопасно переиспользовать или безопасно перенести между scope, он не должен автоматически становиться shared truth.

### 9. Restore Composer Plane

Это слой быстрых срезов.

Он должен собирать:
- `chat_start_restore`;
- `working_state_restore`;
- будущий `workspace_restore_pack`;
- быстрые resume candidates;
- blocked/waiting items;
- recent episodic traces;
- relevant skills/procedures;
- policy overlay;
- permission summary;
- relevant facts;
- relevant tasks;
- relevant artifacts.

Это не truth-source.
Это быстрый готовый рабочий пакет.

### 10. Evaluation / Governance Plane

Этот слой отвечает за:
- proof;
- benchmark;
- memory quality;
- wrong-link rate;
- cross-project leak rate;
- duplicate rate;
- stale-memory failures;
- observability;
- safety alerts.

Сюда же естественно встраивается compare-plan.

## Как текущие документы вкладываются в эту картину

### `README.md`

Это продуктовый верхний слой.

Он должен продолжать объяснять человеку:
- что такое `Amai`;
- зачем она нужна;
- как она продолжает работу;
- как она не теряет проектную линию.

### `docs/ARCHITECTURE.md`

Это документ по фундаменту.

Он уже покрывает:
- слои хранения;
- retrieval laws;
- project identity;
- `ExecCtl` baseline;
- benchmark control plane.

Его нужно расширять в сторону полной memory fabric.

### `docs/OPERATIONS.md`

Это документ по operational truth.

Он должен оставаться законом:
- любой новый слой обязан иметь proof;
- gate;
- reconcile;
- degrade matrix;
- incident path.

### `docs/TOKEN_LEDGER.md`

Это отдельный truth-module.

Он отвечает не за память задач, а за честную оценку цены и пользы.

Любой новый memory-layer должен быть совместим с ним.

### `docs/AMAI_TASK_TREE_PLAN.md`

Это не общий memory master plan.

Это план по модулю:
- `Commitment / Task Graph`.

Его не нужно отменять.
Его нужно поставить на правильное место в общей системе.

### `docs/AMAI_COMPARE_EXPERIMENT_PLAN.md`

Это тоже не общий memory plan.

Это план по модулю:
- `Evaluation / Governance Plane`
- с user-facing dashboard compare surface.

## Какой должен быть глобальный порядок внедрения

Ниже не “всё, что когда-нибудь хотелось бы сделать”, а зависимая цепочка.

Каждый следующий этап опирается на предыдущий.

### Канонический порядок для Amai

Для `Amai` теперь зафиксирован именно такой порядок:

1. новая общая модель memory fabric;
2. scope / identity control plane;
3. typed memory envelope + provenance;
4. commitment / task graph;
5. ранний procedural seed contour;
6. workspace restore pack;
7. semantic + temporal memory strengthening;
8. multi-agent shared/private memory;
9. compare + benchmark plane;
10. full procedural memory;
11. forgetting / consolidation / pruning;
12. governance / safety / evaluator loop.

### Карта соответствия этапов

Чтобы избежать путаницы между списком 1–12 и именованными этапами:
- Этап 0 = пункт 1 (новая общая модель memory fabric).
- Этап 1 = пункт 2 (scope / identity control plane).
- Этап 2 = пункт 3 (typed memory envelope + provenance).
- Этап 3 = пункт 4 (commitment / task graph).
- Этап 3A = пункт 5 (ранний procedural seed contour).
- Этап 4 = пункт 6 (workspace restore pack).
- Этап 5 = пункт 7 (semantic + temporal memory strengthening).
- Этап 6 = пункт 8 (multi-agent shared/private memory).
- Этап 7 = пункт 9 (compare + benchmark plane).
- Этап 8 = пункт 10 (full procedural memory).
- Этап 9 = пункт 11 (forgetting / consolidation / pruning).
- Этап 10 = пункт 12 (governance / safety / evaluator loop).

Это важно не просто как красивый список.

Это значит:
- нельзя перепрыгивать через `scope` и `provenance`, а потом пытаться строить честный graph;
- нельзя откладывать procedural memory до самого конца как “когда-нибудь потом”;
- нельзя запускать full self-evolving procedural layer раньше, чем появились truth, provenance и graph base.

### Два обязательных практических правила внедрения

#### 1. У каждого этапа должен быть stage gate

Нельзя считать этап “примерно готовым”.

Перед переходом дальше у этапа обязаны быть:
- machine-readable контракт;
- maintainability/change-safety gate для значимого изменения;
- passing machine-readable sync guard для значимого обновления `IMPLEMENTATION_STATUS.md`;
- passing machine-readable closure guard для checkbox значимого этапа;
- proof или набор proof-тестов;
- если для этапа уже есть готовый benchmark/proof harness, он обязателен к использованию раньше любых ad-hoc локальных проверок;
- stage-close запрещён, пока не прогнан весь уже materialized и подходящий benchmark/proof bundle этого этапа;
- одного blocking-proof недостаточно;
- если изменение задело соседний shared contour (`speed / accuracy / isolation / truth / dashboard / continuity`), companion non-regression harness для него тоже обязателен;
- benchmark/regression-проверка по четырём осям проекта:
  - скорость;
  - точность;
  - качество;
  - правдивость;
- ручная проверка пользовательского эффекта;
- debug/fix после найденных недостатков;
- повторная проверка после исправлений;
- список известных ограничений;
- явное решение: `готово к следующему этапу` или `нет`.
- обновлённый [IMPLEMENTATION_STATUS.md](IMPLEMENTATION_STATUS.md), чтобы любой агент видел актуальный статус без перелопачивания всего корпуса.

И ещё один жёсткий закон stage gate:
- если benchmark или proof показывает деградацию хотя бы по одной из четырёх осей, этап не закрывается;
- сначала обязателен root-cause разбор:
  - что именно просело;
  - это реальная регрессия или measurement noise;
  - какой слой её вызвал;
- затем обязательны:
  - восстановление baseline или явное исправление регрессии;
  - повторный proof/benchmark;
  - только после этого можно обсуждать закрытие этапа.

#### 2. У каждого этапа должен быть migration и kill-switch план

Нельзя включать новый memory-layer как безусловную замену старого поведения.

Для каждого значимого этапа нужно сразу описывать:
- как он включается рядом со старым контуром;
- как сравнить старое и новое поведение;
- как откатиться без потери truth;
- какой feature flag или kill-switch выключает новый слой;
- какие данные materialized параллельно, а какие уже authoritative.

Простыми словами:
- сначала добавляем новый слой безопасно;
- потом доказываем, что он не ломает baseline;
- только после этого переводим его в normal path.
- и после этого обязательно обновляем живой status snapshot для следующего незнакомого агента.

## Этап 0. Зафиксировать новую общую модель

Что делаем:
- официально признаём, что `Amai` идёт в сторону memory fabric;
- `AMAI_TASK_TREE_PLAN.md` остаётся планом по task-memory, а не по всей памяти;
- compare-plan остаётся eval/dashboard модулем;
- `ARCHITECTURE.md` получает новую общую карту модулей.

Что нельзя потерять:
- существующий `continuity` baseline;
- existing startup contract;
- current `ExecCtl` lane;
- truth-source laws.

Проверка после этапа:
- документы больше не спорят между собой;
- у каждого документа есть своё место;
- понятно, что task-tree не равен всей памяти.

## Этап 1. Scope и identity control plane

Это первый настоящий строительный этап.

Что делаем:
- расширяем current identity model до полного scope plane;
- добавляем:
  - workspace;
  - agent visibility;
  - project links;
  - cross-project transfer rules;
  - quarantine/import states.

Ниже фиксируется минимальный целевой набор scope-уровней:

- `agent_private`
  - память видна только одному агенту или его личному рабочему контуру.

- `team_shared`
  - память видна ограниченной группе агентов одной команды.

- `project_shared`
  - память доступна агентам внутри одного проекта.

- `cross_project_linked`
  - память может использоваться между явно связанными проектами.

- `org_global`
  - память доступна на уровне всей организации, но только для очень узких типов данных:
    - общих правил;
    - утверждённых процедур;
    - безопасных глобальных справочников.

- `quarantine`
  - память изолирована до проверки.

- `imported`
  - память пришла извне или из другого проекта и ещё не стала полноценной локальной truth-записью.

Важно:
- unrelated projects по умолчанию вообще не видят память друг друга;
- `cross_project_linked` допускается только через явный `project_link`;
- `org_global` не должен становиться свалкой всех фактов подряд.

### Какие права нужны кроме самой видимости

Одной видимости мало.

Нужно сразу закладывать динамические права:

- `can_read`
  - можно ли читать объект памяти;

- `can_write`
  - можно ли создавать или менять запись;

- `can_link`
  - можно ли создавать связи с другими объектами;

- `can_import`
  - можно ли импортировать объект из другого проекта;

- `can_promote`
  - можно ли перевести объект из `borrowed/unverified` в нормальную проверенную запись;

- `can_share_further`
  - можно ли передавать объект ещё дальше;

- `can_archive`
  - можно ли убирать в архив;

- `can_delete`
  - можно ли физически удалять;

- `can_quarantine`
  - можно ли отправить объект в карантин;

- `can_approve_transfer`
  - можно ли одобрить межпроектный перенос.

Простое правило:
- scope говорит, где объект вообще живёт;
- права говорят, что именно с ним можно делать.

### Что ещё обязательно нужно добавить в этот блок

Чтобы scope-model был не декоративным, а рабочим, в плане ещё должны быть такие вещи:

- `default_deny`
  - если правило явно не разрешает доступ или перенос, значит он запрещён;
  - нельзя делать permissive fallback.

- `object_class`
  - разные типы памяти должны иметь разные базовые правила:
    - задача;
    - факт;
    - артефакт;
    - policy;
    - процедура;
    - raw log;
    - benchmark evidence.

- `project_link_type`
  - у связи проектов должен быть не просто факт наличия, а тип:
    - `same_client`
    - `shared_codebase`
    - `depends_on`
    - `knowledge_may_transfer`
    - `forbidden_transfer`
    - `legal_boundary`

- `borrowed_status lifecycle`
  - imported-память должна явно проходить стадии:
    - `borrowed`
    - `unverified`
    - `verified_local_copy`
    - `rejected`
    - `expired`

- `revoke / rescope path`
  - если доступ был ошибочно дан, память или связь должны уметь:
    - потерять старый scope;
    - уйти в quarantine;
    - получить новый scope;
    - оставить audit trail.

- `human override trail`
  - любое ручное одобрение или запрет должно оставлять след:
    - кто;
    - когда;
    - почему;
    - что именно изменили.

- `policy precedence`
  - нужно зафиксировать, кто сильнее:
    - global policy;
    - org policy;
    - project policy;
    - object-specific restriction.

- `scope-aware tests`
  - этот блок нельзя считать готовым без тестов на:
    - leak между unrelated projects;
    - illegal import;
    - illegal promotion;
    - read allowed / write denied;
    - quarantine path;
    - revoke path.

Простыми словами:
- мало назвать scope;
- мало назвать права;
- ещё нужно описать, как они включаются, запрещаются, проверяются и отменяются.

### Как должен выглядеть контролируемый межпроектный перенос

Кросс-проектный перенос не должен идти через “поиск по похожему тексту”.

Правильная модель:
- сначала находится кандидат;
- затем проверяется `project_link`;
- затем формируется `import_packet`;
- и только после этого знание может попасть в другой проект как borrowed/unverified.

Минимальный `import_packet` должен хранить (REQUIRED):
- `import_packet_id`
- `source_project_code`
- `target_project_code`
- `allowed_by_project_link`
- `transfer_policy_id`
- `memory_object_ids`
- `artifact_refs`
- `reason`
- `imported_by_agent_scope`
- `imported_at`
- `trust_state`
- `verification_state`
- `borrowed_status`
- `can_promote_after_verification`

Простыми словами:
- нельзя просто “читать всё везде”;
- сначала связь проектов;
- потом controlled import;
- потом отдельная проверка;
- и только потом локальная правда.

Почему это раньше task-tree:
- без этого multi-agent и multi-project memory будет путать границы.

### Что ещё должно быть first-class в project graph

Кроме `workspace / project / project_link` нужно сразу держать в голове ещё два объекта:

- `shared_asset`
  - общий артефакт, компонент, документ или dependency, который реально используется несколькими проектами;

- `transfer_policy`
  - формальная политика, которая говорит:
    - что можно переносить;
    - в какой scope;
    - с какими ограничениями;
    - кто это может approve.

Простыми словами:
- `project_link` говорит, что связь между проектами вообще существует;
- `transfer_policy` говорит, что именно по ней можно делать;
- `shared_asset` фиксирует общую реальность, а не просто похожесть текста.

Какие частные задачи сюда входят (REQUIRED):
- закрепить `project_link` model;
- ввести visibility scopes;
- зафиксировать exact scope levels и их смысл;
- зафиксировать динамические права поверх видимости;
- описать import packet и borrowed/unverified flow;
- описать правила для связанных и несвязанных проектов;
- запретить free semantic mixing.

Проверка после этапа:
- unrelated projects fail-closed изолированы;
- linked projects читаются только по явной policy;
- cross-project knowledge идёт только через import packet;
- borrowed/unverified память не притворяется локальной truth;
- будущие memory items знают свой scope.

## Этап 2. Typed memory envelope + provenance

Что делаем:
- вводим общий envelope для durable memory objects;
- добавляем provenance, verification, temporal validity.
- сразу закладываем temporal ordering model:
  - `observed_at`;
  - `recorded_at`;
  - `valid_from / valid_to`;
  - `last_verified_at`;
  - `ingest_seq / object_version / causation_id / correlation_id`.

Это нужно для:
- semantic memory;
- task memory;
- future procedural memory;
- cross-project imports.
- параллельной работы многих агентов без временных накладок.

Какие частные задачи сюда входят (REQUIRED):
- `memory_id`;
- `memory_type`;
- `scope_type`;
- `workspace_id`;
- `project_id`;
- `owner_agent_id`;
- `visibility`;
- `sensitivity_class`;
- `truth_state`;
- `trust_state`;
- `verification_state`;
- `created_at`;
- `source_event_ids`;
- `artifact_refs`;
- `message_refs`;
- `evidence_span`;
- `derivation_kind`;
- `valid_from / valid_to`;
- `supersedes / conflicts_with`.
- `utility_score`;
- `freshness_score`;
- `retention_class`;
- `ttl`;
- `imported_from`;
- `schema_version`.

### Что ещё должно быть в provenance-модели

Чтобы provenance был не декоративным, а реальным, нужно ещё сразу зафиксировать:

- `evidence_span`
  - на какой именно кусок сырого источника опирается память;
  - не просто “взято из файла”, а из какого диапазона, сообщения, блока, страницы или лог-участка.

- `derivation_kind`
  - каким способом получен объект памяти:
    - raw capture;
    - extract;
    - summary;
    - merge;
    - import;
    - verified write-back.

- `verification_state`
  - уже есть в контракте, но важно считать его не косметикой, а обязательным фильтром доверия.

Простое правило:
- любая durable память должна уметь показать, из чего именно она выросла.

### Что должен решать typed envelope

Общий envelope нужен не “для красоты”, а чтобы сразу решить:
- какой это тип памяти;
- в каком scope она живёт;
- кто ей владеет;
- откуда она взялась;
- когда она была истинной;
- сколько её хранить;
- можно ли ей доверять;
- какой у неё lifecycle.

Если этого envelope нет, память начинает расползаться по разным сущностям и перестаёт быть управляемой.

Важно:
- общий envelope не означает, что вся память обязана лежать в одной гигантской generic-таблице;
- для `Amai` правильнее общий контракт полей и поведения;
- а физически truth-layer может оставаться набором typed stores, если так честнее, быстрее и проще доказывать proof-ами.

### Как должна работать evidence ladder

Retrieval не должен сразу прыгать в сырые логи и не должен жить только на summary.

Правильная лестница такая:

1. `summary / profile / compact card`
   - самый дешёвый и быстрый слой;
   - используется по умолчанию.

2. `structured node / graph neighborhood`
   - если summary недостаточно;
   - система поднимает структурные узлы, связи и соседние доказательства.

3. `raw event / artifact / log`
   - если уверенности всё ещё мало;
   - система эскалирует к сырому источнику.

После этого, если raw-source подтвердил смысл:
- разрешён `verified write-back` в компактный слой.

Это надо понимать как `runtime sufficiency router`:
- система сначала пытается ответить на самом дешёвом достаточном evidence;
- если summary-layer достаточен, дальше не идём;
- если summary-layer недостаточен, routing обязан поднять structured layer;
- если и этого мало, routing обязан эскалировать в immutable raw evidence;
- после подтверждения разрешён `verified write-back`, а не молчаливое переписывание summary.

То есть:
- быстрые summary нужны;
- но они не являются последней инстанцией истины;
- при сомнении система обязана уметь спуститься к сырому основанию.
- answer path должен быть не “всегда raw” и не “всегда summary”, а `cheapest sufficient evidence`.

### Как должен выглядеть общий write pipeline

Нельзя писать durable memory прямой короткой дорогой “увидели -> сразу сохранили как truth”.

Правильная общая схема для `Amai` такая:

1. `raw event append`
   - сначала событие попадает в immutable log.
2. `memory candidate extraction`
   - из события выделяются candidate objects:
     - факт;
     - решение;
     - commitment;
     - skill hint;
     - artifact ref.
3. `policy and scope filter`
   - можно ли это вообще сохранять;
   - в каком scope;
   - с какой чувствительностью.
4. `verification / conflict check`
   - есть ли evidence;
   - не конфликтует ли это с текущей truth;
   - не poisoned ли это;
   - не private ли это для текущего contour.
5. `truth write`
   - только после этого идёт запись в `PostgreSQL` truth-layer.
6. `async indexing`
   - lexical indexes;
   - graph indexes;
   - embeddings;
   - restore summaries.
7. `cache invalidation / fan-out`
   - через `JetStream / NATS` или совместимый event plane.

Простое правило:
- commitments и явные operator decisions можно писать в hot-path;
- semantic/procedural consolidation лучше дистиллировать в background.

### Как должен выглядеть общий read pipeline

Запрос памяти не должен стартовать сразу с vector search.

Правильная общая схема такая:

1. `intent classifier`
   - это continuity;
   - factual recall;
   - procedural recall;
   - policy check;
   - artifact lookup.
2. `scope resolver`
   - какие scope вообще доступны текущему агенту и проекту.
3. `candidate generation`
   - `exact / lexical / graph / vector / temporal`.
4. `rerank + legality + relevance`
   - не только “похоже”, но и:
     - разрешено;
     - валидно по времени;
     - подходит по trust.
5. `evidence sufficiency check`
   - хватает ли дешёвого слоя evidence.
6. `escalate if needed`
   - structured neighborhood;
   - raw logs;
   - artifacts;
   - temporal slices.
7. `abstain if insufficient`
   - если доказательств всё ещё мало, система не придумывает.

Проверка после этапа:
- новая память хранит не только содержание, но и источник;
- можно понять, откуда она взялась;
- можно понять, когда она была истинной.
- можно отличить:
  - актуальную правду;
  - устаревшую правду;
  - отменённое решение;
  - неподтверждённую гипотезу.
- можно спуститься от summary к structured layer и дальше к raw evidence.
- verified write-back не появляется без evidence escalation.

## Этап 3. Перевести task tree в commitment graph

Вот здесь входит `AMAI_TASK_TREE_PLAN.md`.

Что делаем:
- current task-tree plan реализуем как модуль commitment memory;
- делаем internal graph;
- UI может оставаться деревом.

Какие частные задачи сюда входят (REQUIRED):
- `task_node`;
- `task_event`;
- `memory_link_decision`;
- ветвление по смыслу;
- поднятие старых веток;
- дочерние узлы;
- двухосевые статусы:
  - `execution_state`
  - `lifecycle_state`
- затем более сильные operational states вроде `blocked / waiting_external / in_review`.

Что важно:
- не ломать current `ExecCtl resume contract`;
- не ломать startup/restore;
- не терять pending-return semantics.
- не принимать branch-link решение по одному итоговому score.

Что ещё должно быть зафиксировано сразу:
- score остаётся только одним из признаков;
- legality/scope filtering идёт раньше любого similarity;
- нельзя сначала искать “по похожести”, а потом думать о границах;
- decision pipeline должен быть многошаговым:
  - `scope filtering`
  - `candidate generation`
  - `rerank / classifier`
  - `evidence sufficiency check`
  - `continue / child / new / abstain / escalate`
- при низкой уверенности система не должна делать уверенный ложный выбор;
- вместо этого она должна создавать `pending_link_proposal` с `ttl` и запросом дополнительного evidence.

Проверка после этапа:
- система не теряет линии;
- не плодит лишние дубли;
- поднимает старую ветку, если это реально она;
- видит open/closed/archive честно.

### Какие first-class truth tables должны появиться в PostgreSQL

Чтобы memory fabric не осталась только на уровне слов, в truth-layer должны появиться или быть явно materialized такие first-class сущности:

- `workspace`
- `project`
- `project_link`
- `memory_item`
- `memory_edge`
- `memory_conflict`
- `memory_provenance`
- `skill_card`
- `policy_rule`
- `retrieval_trace`
- `restore_pack`
- `import_packet`
- `quarantine_item`

Простое правило:
- UI, summaries и local caches могут быть производными;
- source of truth по этим слоям должен жить в `PostgreSQL`.

## Этап 3A. Ранний procedural seed contour

Это ранний минимальный контур procedural memory.

Его смысл:
- не откладывать procedural memory слишком далеко;
- но и не запускать full self-evolving layer раньше, чем появились truth, provenance и commitment graph.

Что делаем сразу после graph-ядра:
- заводим первые durable procedural objects;
- учим систему извлекать candidate skills из удачных и повторяющихся эпизодов;
- добавляем отдельный evaluator/trust contour именно для skills;
- запрещаем append-only склад “удачных заметок”.

Что входит в ранний контур:
- `skill_card_candidate`
- `skill_evidence_bundle`
- `skill_trial_run`
- `skill_eval`
- `skill_trigger_match`
- `skill_reuse_log`

Какие поля должны появиться уже здесь:
- `skill_id`
- `skill_version`
- `skill_title`
- `skill_goal`
- `skill_trigger_conditions`
- `skill_preconditions`
- `skill_execution_steps`
- `skill_stop_conditions`
- `skill_forbidden_when`
- `skill_expected_outcome`
- `skill_scope_type`
- `skill_owner_scope`
- `skill_trust_state`
- `skill_verification_state`
- `skill_runtime_constraints`
- `skill_model_constraints`
- `skill_tool_constraints`
- `skill_context_constraints`
- `skill_source_event_ids`
- `skill_artifact_refs`
- `skill_success_count`
- `skill_failure_count`
- `skill_reuse_count`
- `skill_last_used_at`
- `skill_last_verified_at`
- `skill_utility_score`

Как это должно работать:
1. Из эпизода не создаётся verified skill напрямую.
2. Сначала extractor делает `candidate`.
3. Candidate обязан иметь evidence bundle.
4. Потом candidate проходит `trial` на похожих задачах.
5. Отдельный evaluator contour решает:
   - это реально помогает;
   - это безопасно;
   - это не переобучилось на один частный случай;
   - это не конфликтует с policy и trust rules.
6. Только потом candidate может перейти в `verified`.

### Что `Amai` добавляет поверх базовой идеи procedural memory

Это уже не optional-идеи, а утверждённые усиления для проекта.

1. `shadow_mode`
   - новый skill сначала проверяется в фоне;
   - система смотрит, подошёл бы он в этой задаче или нет;
   - skill ещё не влияет на ход работы как обязательная инструкция.

2. `runtime/model/tool binding`
   - у skill должны быть ограничения:
     - для каких runtime он годится;
     - для каких моделей;
     - для каких инструментов;
     - для какого scope и типа задачи.
   - это нужно, чтобы хороший skill не всплывал не в том контуре.

3. `negative procedural memory`
   - ошибки и anti-patterns тоже first-class объекты;
   - failure playbooks и repair sequences живут рядом с success skills, а не теряются в обычном логе.

4. `skill patching instead of clone explosion`
   - при похожем новом skill сначала проверяем:
     - это patch старого;
     - это merge с существующим;
     - это реальный новый skill;
   - нельзя плодить пачки почти одинаковых навыков.

5. `versioned skill history`
   - у skill должна быть версия;
   - должно быть видно, кто и почему изменил skill;
   - skill evolution должна быть прослеживаемой.

6. `restore as execution card`
   - в restore поднимается не длинная procedural заметка;
   - а компактная исполнимая карточка для текущего шага.

7. `shared promotion by approval`
   - shared procedural memory не растёт автоматически;
   - skill попадает в shared contour только после evaluator/trust approval.

Что здесь строго запрещено:
- делать skill обычной текстовой заметкой;
- продвигать skill в shared contour после одного успеха;
- обновлять procedural memory только по utility без trust-check;
- хранить procedural memory без source evidence.

Почему этот этап ранний:
- пользовательский эффект от него очень большой;
- именно он начинает превращать опыт в переиспользуемый способ действия;
- но он уже опирается на truth/provenance/scopes, поэтому не должен идти раньше Этапов 1-3.

Проверка после этапа:
- система умеет поднять не только старую задачу, но и старый способ её решения;
- candidate skills не плодятся как мусор;
- ни один skill не становится verified без evaluator/trust contour.
- skill не всплывает для неподходящего runtime/model/tool contour.
- новые skills сначала проходят shadow-mode, а не сразу давят на executor.

## Этап 4. Собрать workspace restore pack

Это следующий слой над task graph.

Что делаем:
- расширяем current `chat_start_restore` и `working_state_restore`;
- делаем более широкий `workspace_restore_pack`.

Он должен включать:
- active commitments;
- blocked/waiting items;
- paused branches;
- recently closed;
- relevant semantic facts;
- recent episodic traces;
- active constraints;
- permission summary;
- important artifacts;
- unresolved conflicts;
- relevant procedures/skills, когда они появятся.

Важно:
- restore не должен тащить сырой procedural архив;
- если procedural memory materialized, в restore попадает `compact execution card`.

Почему это после task graph:
- сначала нужен нормальный truth по задачам;
- потом уже можно собирать богатый restore.

Проверка после этапа:
- новая чистая рабочая поверхность поднимает не только headline;
- а реальную рабочую картину.
- в restore есть не только задачи, но и blocked/waiting items.
- в restore есть важные свежие эпизоды.
- в restore есть действующие правила, ограничения и права.
- в restore есть релевантные процедуры и навыки, если они уже materialized.

## Этап 5. Semantic + temporal memory strengthening

Что делаем:
- усиливаем semantic memory и temporal truth;
- делаем память не только про задачи, но и про знания.

Какие частные задачи сюда входят (REQUIRED):
- durable semantic facts;
- relation edges;
- valid_from / valid_to;
- explicit truth-state transitions;
- knowledge updates;
- temporal truth repair;
- exact-time semantic slices;
- better old-fact supersession.

Что уже есть как база:
- lexical/symbol retrieval;
- continuity docs;
- temporal lookup;
- enriched thread index;
- workspace graph.

Проверка после этапа:
- система умеет помнить не только задачу, но и факт;
- умеет не путать старый факт и новый;
- умеет понимать, что было верно в разное время.

## Этап 6. Multi-agent shared/private memory

Что делаем:
- добавляем controlled shared memory;
- private/shared/project/cross-project layers;
- access rules и write policies.

Какие частные задачи сюда входят (REQUIRED):
- `agent_private`;
- `project_shared`;
- `cross_project_linked`;
- `org_global`;
- `quarantine`.

Что важно:
- shared memory без trust and policy запрещена;
- cross-project imports должны идти не по похожести, а по controlled transfer.

Проверка после этапа:
- несколько агентов могут работать вместе без silent leakage;
- shared knowledge живёт отдельно от private memory;
- несвязанные проекты по-прежнему изолированы.

## Этап 7. Compare and benchmark plane

Вот здесь входит `AMAI_COMPARE_EXPERIMENT_PLAN.md`.

Что делаем:
- строим честное сравнение `с Amai` / `без Amai`;
- онлайн и benchmark раздельно;
- multi-platform live contract;
- benchmark catalog;
- UI карточки и графики.

Почему это не первый этап:
- сначала memory truth и scopes;
- потом уже user-facing evaluation surface.

Что важно:
- compare-plane не должен стать заменой архитектурных proof;
- он должен быть user-facing измерением пользы поверх truth-layer.
- в benchmark catalog должен входить отдельный procedural-memory benchmark:
  - для `skill reuse`;
  - для `bad-skill suppression`;
  - для `stale-skill suppression`;
  - для `shadow -> verified uplift`;
  - для проверки, что evaluator тормозит memory misevolution.

Проверка после этапа:
- процент на карточке честный;
- графики честные;
- online и benchmark не смешиваются;
- multi-platform runtime не ломает смысл.
- procedural memory bench показывает не generic score, а отдельные skill-метрики.

## Этап 8. Procedural memory

Это следующий большой модуль.

Что делаем:
- из удачных эпизодов и повторяющихся рабочих паттернов выделяем reusable procedures.

Какие частные задачи сюда входят (REQUIRED):
- `skill_card`;
- `skill_execution_card`;
- `skill_shadow_run`;
- `skill_trigger`;
- `skill_steps`;
- `skill_preconditions`;
- `skill_stop_conditions`;
- `skill_forbidden_when`;
- `skill_expected_outcome`;
- `failure_patterns`;
- `repair_playbooks`;
- `evaluator feedback`;
- `skill_constraints`;
- `skill_success_evidence`;
- `skill_failure_evidence`;
- `skill_trust_state`;
- `skill_utility_score`;
- `skill_runtime_constraints`;
- `skill_model_constraints`;
- `skill_tool_constraints`;
- `skill_context_constraints`;
- `skill_shadow_pass_count`;
- `skill_shadow_fail_count`;
- `skill_patch_parent_id`;
- `skill_merge_group_id`;
- `skill_version`;
- `last_used_at`;
- `skill_deprecation_status`.

Что ещё важно зафиксировать сразу:
- skill не должен рождаться из одного случайного эпизода;
- сначала это candidate skill;
- потом проверка на повторяемость;
- потом evaluator feedback;
- только потом verified reusable procedure.
- verified skill должен быть исполнимым объектом:
  - с trigger conditions;
  - с preconditions;
  - с execution steps;
  - с stop conditions;
  - с forbidden conditions;
  - с expected outcome.
- procedural memory должна обновляться не append-only, а через controlled refinement:
  - distill;
  - compare with existing skills;
  - merge/patch;
  - deprecate/quarantine weak variants.
- перед verified reuse skill должен уметь пройти shadow-mode на похожих задачах.
- skill обязан знать свой runtime/model/tool fit.

Простая модель жизни procedural memory:
- `candidate`
- `shadow`
- `trial`
- `verified`
- `deprecated`
- `quarantined`

Что именно берём из свежих работ:
- ссылки на ProcMEM/ReMe/TAME см. [docs/REFERENCES.md](docs/REFERENCES.md); без источника считать только концептуальными ориентирами.
- из `ProcMEM`:
  - skill должен быть исполнимым объектом, а не абзацем текста;
  - у skill должны быть trigger/activation conditions, execution steps и termination conditions;
  - skill pool должен быть компактным, а не бесконечным.
- из `ReMe`:
  - память должна не только накапливаться, но и перерабатываться;
  - reuse должен быть context-aware;
  - слабые или устаревшие процедуры должны деградировать и удаляться из hot contour.
- из `TAME`:
  - нужен отдельный evaluator feedback contour;
  - skill нельзя считать verified только потому, что он однажды помог;
  - нужна защита от memory misevolution, когда память вроде бы “эволюционирует”, но качество или безопасность падают.

Какой у нас должен быть write-path для procedural memory:
1. `episode capture`
   - сохраняем сырой эпизод, trace и evidence.
2. `candidate extraction`
   - выделяем предполагаемый reusable method.
3. `distillation`
   - убираем частный мусор;
   - оставляем trigger, условия, шаги, ограничения, ожидаемый результат.
4. `similar-skill check`
   - не плодим дубль, если похожий skill уже есть.
5. `shadow application`
   - candidate проверяется в фоне на похожей задаче без обязательного влияния на executor.
6. `trial application`
   - используем candidate на похожей задаче в контролируемом режиме.
7. `evaluator review`
   - отдельно оцениваем utility, safety, stability, scope legality.
8. `promote / patch / deprecate / quarantine`
   - только после этого меняем durable skill pool.

Какой у нас должен быть read-path для procedural memory:
1. `trigger match`
   - похожа ли текущая задача на условия запуска skill.
2. `scope / legality check`
   - можно ли вообще использовать этот skill в текущем проекте, агенте и policy scope.
3. `context fit check`
   - подходят ли текущие инструменты, модель, runtime и ограничения.
4. `trust / verification gate`
   - можно ли поднимать только verified skill или допускается trial skill.
5. `compact execution card`
   - в ход работы попадает не весь архив, а короткая исполнимая карточка.

Как должен выглядеть evaluator/trust contour:
- он не смешан с executor memory;
- он хранит отдельные verdict:
  - `useful / not_useful`
  - `safe / unsafe`
  - `stable / unstable`
  - `generalizable / overfit`
  - `fresh / stale`
- он может:
  - не дать повысить skill;
  - откатить skill в `trial`;
  - отправить skill в `deprecated` или `quarantined`.

Что обязательно проверять proof-тестами:
- skill не создаётся из одного случайного успеха;
- похожие skills не плодятся пачками;
- bad skill не проходит в verified только из-за utility;
- stale skill не продолжает всплывать как лучший способ;
- cross-project/shared procedural reuse obeys scope and policy;
- evaluator реально умеет тормозить memory misevolution.
- skill version history не рвётся при patch/merge;
- restore поднимает execution card, а не сырые procedural простыни.

### Мои дополнительные предложения именно для Amai

Это тоже уже заложено как approved design.

- `skill` должен уметь ссылаться на:
  - какие артефакты он использует;
  - какие policy его ограничивают;
  - какие task/fact graph-узлы его породили.
- `Amai` должна уметь отвечать:
  - почему skill был выбран;
  - почему skill был отвергнут;
  - почему skill понижен из `verified` обратно в `trial`.
- procedural memory нельзя мерить только “помогло / не помогло”;
  нужны отдельные линии:
  - utility;
  - trust;
  - stability;
  - freshness;
  - generalizability.
- для мультиагентной среды skill reuse должен сначала быть:
  - `agent_private`
  - потом `project_shared`
  - и только потом, при отдельном approval, выше.

То есть:
- нельзя автоматически считать любой удачный эпизод “навыком”;
- и нельзя позволять shared procedural memory писать без trust/provenance.

Почему не раньше:
- сначала нужно надёжно хранить задачи, факты и provenance;
- без этого procedural layer начнёт быстро деградировать и врать.

Проверка после этапа:
- агент помнит не только что делал, но и как лучше делать похожую работу.

## Этап 9. Forgetting, consolidation, pruning

Без этого память со временем станет токсичным складом.

Что делаем:
- utility scoring;
- freshness scoring;
- de-dup;
- compaction;
- pruning;
- archive to cold tier;
- quarantine/deprecation.

Какие частные задачи сюда входят (REQUIRED):
- `utility_score`;
- `freshness_score`;
- `access_count`;
- `last_accessed_at`;
- `retention_class`;
- `ttl`;
- `decay_policy`;
- `consolidation_status`.

Что ещё важно зафиксировать сразу:
- забывание не равно безусловному удалению;
- нельзя “чистить” immutable raw log как обычный мусор;
- нельзя удалять provenance и доказательства, на которых держится truth;
- сначала должны сжиматься и деградировать дешёвые производные слои:
  - summary;
  - soft hints;
  - stale caches;
  - weak graph suggestions;
- а не первичный source-of-truth.

Почему это уже не optional:
- `ReMe` прямо усиливает для нас идею utility-based refinement;
- `TAME` показывает, что без evaluator loop память может эволюционировать в плохую сторону;
- значит forgetting/consolidation для `Amai` это не “уборка на потом”, а часть correctness-модели.
Ссылки на ReMe/TAME фиксируются в [docs/REFERENCES.md](docs/REFERENCES.md) и считаются `concept-only`, пока не добавлены официальные источники.

Простая и безопасная модель такая:
- raw evidence и audit trail:
  - почти не забываются, только уходят в cold tier;
- verified truth:
  - не исчезает тихо, а либо устаревает, либо заменяется;
- summaries и hints:
  - могут схлопываться, деградировать и удаляться раньше.

Ещё нужно закрепить отдельные фоновые процессы:
- `de_duplication_job`
- `summarization_job`
- `compaction_job`
- `pruning_job`
- `cold_archive_job`
- `revalidation_job`

Текущая materialization truth (обязательная сверка):
- статус и факт materialization для jobs сверяются по `IMPLEMENTATION_STATUS.md` и `IMPLEMENTATION_GATES.md`;
- утверждения ниже считаются валидными только после такой сверки:
  - named forgetting jobs surfaced в runtime через `memory run-job --job-kind ...`;
  - `de_duplication_job`, `compaction_job`, `pruning_job`, `cold_archive_job`, `revalidation_job` исполняются как first-class CLI contour;
  - `summarization_job` materialized как explicit no-op contract, а не скрытое отсутствие реализации.

И отдельно правило:
- forgetting должен быть explainable;
- система должна уметь ответить, почему объект схлопнули, устарели, архивировали или выкинули из горячего слоя.

Проверка после этапа:
- память не превращается в мусор;
- retrieval не деградирует со временем;
- stale knowledge не продолжает всплывать как новое.
- immutable truth и raw evidence не пропадают из-за агрессивной очистки.

## Этап 10. Governance, safety, evaluator loop

Это верхний слой качества и доверия.

Что делаем:
- poisoning detection;
- conflict objects;
- human overrides;
- memory trust states;
- abstention/evidence sufficiency;
- quality and safety evals.

Какие частные задачи сюда входят (REQUIRED):
- wrong-link rate;
- duplicate branch rate;
- stale-memory error rate;
- cross-project leak rate;
- poisoning alerts;
- abstention quality;
- recovery quality.

Что ещё обязательно входит в этот этап:
- memory poisoning detection
  - поиск подозрительных write-path, suspicious imports и unsafe write-back.
- privacy and extraction defense
  - проверка, что retrieval/restore не вытаскивают чувствительные данные в неподходящий scope.
- quarantine and appeal flow
  - подозрительная память уходит в quarantine;
  - дальше возможны verify, deprecate или hard delete по policy.
- trustworthy evolution control
  - память не может “улучшаться” только по utility;
  - evaluator contour обязан смотреть и на безопасность, и на стабильность, и на scope legality.
- human override + audit trail
  - любой ручной promote/quarantine/delete/revive должен оставлять след.

Проверка после этапа:
- система не только помнит больше;
- она помнит безопаснее, честнее и устойчивее.

## Scientific reinforcement contour

Это не отдельный "второй roadmap" и не новая параллельная архитектура.

Это documentation-grade adoption contour поверх уже существующих Stage 7 / 9 / 10.
Источник ручной сверки и triage:
- [AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md](AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md)

Implementation law для этого контура:
- `AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md` теперь выполняет не только роль synthesis-doc, но и роль authoritative execution-spec;
- exact queue order `Queue 0-5`, exact `v1` scope и out-of-scope границы берутся именно оттуда;
- implementer не имеет права silently reorder-ить очереди или расширять production scope beyond этого execution-spec.

Что сюда входит:
- probabilistic/statistical ideas берутся только после ручной сверки с текущим repo state;
- старые snapshot-claims не поднимаются в canonical laws автоматически;
- methodological ideas не получают authority выше текущих truth/proof/gate contours.

Production scope этого reinforcement contour:
- statistical benchmark honesty;
- lifecycle transition discipline;
- `Markov / hazard lifecycle v1` как advisory/explain contour;
- regression explain surface;
- Poisson/arrival capacity forecast как planning/observability contour.

Out-of-scope без отдельной ревизии execution-spec:
- truth-authoritative Bayesian belief-layer;
- destructive probabilistic auto-decision;
- replacement of `verified truth` with mathematical projection.

Как раскладывать по текущим этапам:
- `Bayesian / confidence / calibration`
  - future extension к `Этапу 10`;
  - не source of truth и не materialized truth law;
  - measured approval overlay уже частично materialized поверх compare/promotion plane;
  - final promotion остаётся human-gated;
  - статус: `partially_materialized / no truth-authority`.
- `statistical benchmark discipline + drift checks`
  - extension к `Этапу 7` и `Этапу 10`;
  - Queue 1 уже materialize-ит statistics/promotion/approval slices для `memory_task_matrix` и `mcp_task_matrix`;
  - fresh proof-refresh 2026-04-24: `memory_task_matrix`, `mcp_task_matrix`, benchmark contamination preflight and dependent observability/dashboard parity are green after the MCP matrix red-state/no-data defect was fixed;
  - статус: `in_progress / fresh-proof-green / measured-approval-human-gated`.
- `Markov / hazard lifecycle`
  - extension к `Этапу 9`;
  - нужен как explainable lifecycle planner, а не как truth-engine;
  - должен строиться поверх уже существующих lifecycle/policy traces:
    - `lifecycle_state`
    - `retention_class`
    - `decay_policy`
    - `consolidation_status`
    - forgetting/revalidation audit trail;
  - practical effect, который от него требуется:
    - лучшее `pending_review`/revalidation targeting;
    - более честный archive/prune forecast;
    - policy simulation по cohort-level переходам;
  - minimal advisory slice уже materialized как `memory cohort-risk`, governance/dashboard summary и MCP/observe snapshot summary;
  - broader approval/policy-simulation contour ещё не закрыт;
  - статус: `materialized_minimal_slice / advisory-only`.
- `Poisson capacity`
  - performance/capacity planning contour;
  - Queue 5 minimal forecast-only slice уже materialized для `nats_events`;
  - runtime/routing/truth authority запрещены;
  - статус: `materialized_minimal_slice / forecast-only`.
- `KAN-style context-pack utility explain`
  - research candidate under the scientific reinforcement contour, not a new
    core stage and not a product promise;
  - source-of-truth for this idea lives in `Candidate Queue 4A` inside
    `AMAI_SCIENTIFIC_MEMORY_ADOPTION_PLAN.md`;
  - until `shadow_approved`, it stays `spec_only / not_materialized`;
  - even after future implementation it may only start as read-only shadow
    explanation over already allowed context-pack candidates.
- `regression / explainability`
  - read-only explain surface;
  - не authoritative truth-layer;
  - Queue 4 minimal slice уже materialized как CLI/snapshot/dashboard contour;
  - live measured quality пока честно остаётся `insufficient_sample`;
  - статус: `materialized_minimal_slice / insufficient_sample_for_measured_quality`.

Жёсткие ограничения:
- probabilistic score не имеет права переписывать verified truth без policy/evidence path;
- benchmark significance/drift не заменяют domain proof, а только усиливают measured discipline;
- lifecycle math обязана оставаться explainable и audit-safe;
- lifecycle model не имеет права сама объявлять truth verdict или destructive forgetting decision без policy/evidence path;
- regression допустима только как explain/forecast contour, а не как источник истины.

## Какие частные планы уже встроены в этот roadmap

### Частный план 1. `AMAI_TASK_TREE_PLAN.md`

Статус:
- не весь memory master plan;
- а модуль `Commitment / Task Graph`.

Значит:
- его продолжаем;
- но реализуем внутри Этапа 3.

### Частный план 2. `AMAI_COMPARE_EXPERIMENT_PLAN.md`

Статус:
- не memory core;
- а модуль user-facing evaluation.

Значит:
- его продолжаем;
- но реализуем внутри Этапа 7.

### Частный план 3. Token ledger / KPI work

Статус:
- отдельный truth-module для стоимости, пользы и product metering.

Значит:
- он проходит через все этапы;
- но особенно важен для Этапов 7 и 10.

## Что делать нельзя

Нельзя:
- свести всю память к одному дереву задач;
- свести всю память к одному retrieval store;
- сделать один общий memory pool на все проекты;
- искать по всем проектам сначала по похожести, а про scope думать потом;
- считать summary истиной;
- позволять shared procedural memory писать без trust/provenance;
- накапливать память бесконечно без forgetting/consolidation;
- делать user-facing UI раньше, чем готов truth/gate/debug.

## Как внедрять без накопления хаоса

После каждого большого этапа должно быть 6 обязательных вещей:
- machine-readable contract;
- schema/storage changes;
- runtime path;
- proof/eval;
- debug/explainability;
- human-readable docs update.

И только после этого следующий этап.

То есть нельзя:
- сначала сделать пять красивых слоёв “на глаз”;
- а потом надеяться, что они сложатся в систему.

## Что должно получиться в итоге

В итоге `Amai` должна стать не “архивом заметок” и не “одним деревом задач”, а связанной memory fabric, где:
- `scope plane` защищает границы;
- `continuity` продолжает работу;
- `working-state` держит живой рабочий контур;
- `commitment graph` держит обязательства и планы;
- `semantic memory` держит факты и связи;
- `temporal memory` держит историю по времени;
- `procedural memory` держит reusable способы работы;
- `policy/trust` защищает систему;
- `restore composer` поднимает готовый рабочий пакет;
- `evaluation/governance` доказывает, что всё это реально помогает, а не просто выглядит умно.

Именно в этом порядке проект будет расти без потери уже сделанного и без распада на несвязанные частные инициативы.
